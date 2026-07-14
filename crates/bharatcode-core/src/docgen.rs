//! Opt-in documentation-generation context preservation.
//!
//! When the conversation is compacted to fit within a token budget, the public
//! API surface that appeared in code shared during the conversation (function,
//! struct, enum and trait signatures, plus exported TypeScript / Python
//! symbols) can be lost along with the rest of the trimmed history. If the user
//! is asking the model to *document* that code, losing those symbols is exactly
//! the wrong thing to forget.
//!
//! This module scans the conversation for code and distills a compact
//! "public API to document" digest that is retained across compaction, so the
//! model keeps the list of symbols it is supposed to write docs for.
//!
//! The helper is gated behind the `BHARATCODE_DOCGEN` environment variable and
//! is a no-op (returns `None`) when disabled, so the default compaction path is
//! completely unchanged unless explicitly opted in.

use crate::conversation::message::{Message, MessageContent};

/// Environment variable that turns the doc-gen API-preservation helper on.
/// Default: off.
const ENABLE_KEY: &str = "BHARATCODE_DOCGEN";

/// Returns true when the doc-gen helper is enabled.
///
/// Reads the raw `BHARATCODE_DOCGEN` environment variable; any truthy-ish value
/// (`1`, `true`, `yes`, `on`) enables it. Defaults to `false` so the
/// surrounding compaction path behaves exactly as before unless opted in.
pub fn is_enabled() -> bool {
    std::env::var(ENABLE_KEY)
        .map(|raw| is_truthy(&raw))
        .unwrap_or(false)
}

fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Scan a source snippet line-by-line and pull out public / exported API
/// declarations: Rust `pub fn` / `pub struct` / `pub enum` / `pub trait`, plus
/// TypeScript `export function` and Python `def`.
///
/// Returns the matched declaration lines (trimmed). Private declarations such as
/// a bare `fn baz()` are intentionally ignored. This is a deliberately simple,
/// dependency-free heuristic — it does not parse the language, it only surfaces
/// the symbol-bearing line so the model is reminded which symbols exist.
pub fn extract_public_api(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw_line in src.lines() {
        let line = raw_line.trim();
        if line.starts_with("pub fn ")
            || line.starts_with("pub struct ")
            || line.starts_with("pub enum ")
            || line.starts_with("pub trait ")
            || line.starts_with("pub async fn ")
            || line.starts_with("export function ")
            || line.starts_with("export class ")
            || line.starts_with("export const ")
            || line.starts_with("def ")
        {
            let signature = signature_of(line);
            if !signature.is_empty() && !out.contains(&signature) {
                out.push(signature);
            }
        }
    }
    out
}

/// Reduce a declaration line to a compact signature: keep everything up to the
/// opening brace / arrow body / colon so the symbol name and parameters survive
/// but the implementation does not.
fn signature_of(line: &str) -> String {
    let cut = line
        .find('{')
        .or_else(|| line.find(" =>"))
        .unwrap_or(line.len());
    line.get(..cut)
        .unwrap_or(line)
        .trim_end()
        .trim_end_matches(':')
        .to_string()
}

/// Build the retained "public API to document" digest from the conversation,
/// or `None` when the helper is disabled or there is nothing to preserve.
///
/// Scans every message's text for fenced code blocks (```), runs
/// [`extract_public_api`] over their contents, and renders a single compact
/// block listing the discovered symbols. Returns `None` unless the feature is
/// enabled, so the default compaction path is untouched.
pub fn api_digest_block(messages: &[Message]) -> Option<String> {
    if !is_enabled() {
        return None;
    }

    let mut symbols: Vec<String> = Vec::new();
    for message in messages {
        for content in &message.content {
            if let MessageContent::Text(text) = content {
                for code in fenced_code_blocks(&text.text) {
                    for sym in extract_public_api(&code) {
                        if !symbols.contains(&sym) {
                            symbols.push(sym);
                        }
                    }
                }
            }
        }
    }

    if symbols.is_empty() {
        return None;
    }

    let mut block = String::from("# Public API to document\n");
    block.push_str(
        "These public symbols appeared in code shared earlier; retain them so they can be documented:\n",
    );
    for sym in &symbols {
        block.push_str("- ");
        block.push_str(sym);
        block.push('\n');
    }
    Some(block)
}

/// Extract the contents of triple-backtick fenced code blocks from a text body.
/// Unterminated fences capture to end of text so a trailing snippet still counts.
fn fenced_code_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut in_block = false;
    let mut current = String::new();
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            if in_block {
                blocks.push(std::mem::take(&mut current));
                in_block = false;
            } else {
                in_block = true;
            }
            continue;
        }
        if in_block {
            current.push_str(line);
            current.push('\n');
        }
    }
    if in_block && !current.is_empty() {
        blocks.push(current);
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_with(text: &str) -> Message {
        Message::user().with_text(text)
    }

    #[test]
    fn test_extract_public_api_keeps_public_drops_private() {
        let src = "\
pub fn foo(x: u32) -> u32 {
    let _ = x;
    fn baz() {}
    42
}

pub struct Bar {
    field: u32,
}
";
        let api = extract_public_api(src);
        assert!(
            api.iter().any(|s| s.contains("pub fn foo")),
            "expected pub fn foo, got: {api:?}"
        );
        assert!(
            api.iter().any(|s| s.contains("pub struct Bar")),
            "expected pub struct Bar, got: {api:?}"
        );
        assert!(
            !api.iter().any(|s| s.contains("baz")),
            "private fn baz must be ignored, got: {api:?}"
        );
    }

    #[test]
    fn test_extract_public_api_handles_ts_and_py() {
        let ts = "export function greet(name: string): string { return name; }";
        let py = "def compute(value):\n    return value * 2";
        let ts_api = extract_public_api(ts);
        let py_api = extract_public_api(py);
        assert!(ts_api.iter().any(|s| s.contains("export function greet")));
        assert!(py_api.iter().any(|s| s.contains("def compute")));
    }

    #[test]
    fn test_signature_drops_body() {
        let api = extract_public_api("pub fn add(a: i32, b: i32) -> i32 { a + b }");
        assert_eq!(api, vec!["pub fn add(a: i32, b: i32) -> i32".to_string()]);
    }

    #[test]
    fn test_api_digest_block_none_when_disabled() {
        // Default (env unset) => feature off => no digest, so compaction is
        // unchanged. Guarded so a stray BHARATCODE_DOCGEN in the test env does
        // not make this assertion flaky.
        if !is_enabled() {
            let messages = vec![user_with("```\npub fn foo() {}\n```")];
            assert!(api_digest_block(&messages).is_none());
        }
    }

    #[test]
    fn test_fenced_code_blocks_extracts_between_fences() {
        let text = "intro\n```rust\npub fn foo() {}\n```\noutro";
        let blocks = fenced_code_blocks(text);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].contains("pub fn foo"));
        assert!(!blocks[0].contains("intro"));
    }
}
