//! Screen-reader transcript export: a flat, plain-text session log.
//!
//! Renders a session's messages as a linear, labelled plain-text transcript
//! (`You:` / `Assistant:` / `Tool <name>:` / `Result:` blocks) with zero ANSI
//! escapes and no spinners or decorative glyphs, so blind users and
//! accessibility tooling can re-read a session top-to-bottom without fighting
//! the interactive renderer.
//!
//! This surface is read-only and always available: it never mutates a session,
//! and it leaves the interactive output path completely untouched. The render
//! is deterministic — the same message list always produces byte-identical
//! text — which makes the output stable for diffing and for screen readers that
//! cache and re-announce content.
//!
//! The role/tool labels reuse the accessibility label keys already defined in
//! `i18n/a11y_en.json` (`a11y.tool_call`, `a11y.result`), parsed here directly
//! from the same embedded JSON so the keys stay the single source of truth.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;

use anyhow::Result;
use goose::conversation::message::{Message, MessageContent};
use goose::session::SessionManager;
use rmcp::model::Role;

/// Accessibility label table, parsed from the same embedded JSON the streaming
/// a11y renderer uses, so the `a11y.*` keys remain a single source of truth.
static A11Y_LABELS: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../i18n/a11y_en.json"))
        .expect("transcript: a11y_en.json is not valid JSON")
});

/// Speaker label for user-authored messages. A fixed, screen-reader-friendly
/// literal: terse, unambiguous, and never localized away from the colon form a
/// reader is expected to announce.
const LABEL_YOU: &str = "You";

/// Speaker label for assistant messages.
const LABEL_ASSISTANT: &str = "Assistant";

fn a11y_label(key: &str, fallback: &str) -> String {
    A11Y_LABELS
        .get(key)
        .cloned()
        .unwrap_or_else(|| fallback.to_string())
}

/// Options for [`handle_transcript`].
#[derive(Debug, Default)]
pub struct TranscriptOptions {
    /// Explicit session id to render. When `None`, the most recently updated
    /// session is used (the "current/last" session).
    pub session_id: Option<String>,
    /// Optional output file. When `None`, the transcript is printed to stdout.
    pub out: Option<PathBuf>,
}

/// Load the requested (or most recent) session and render it as a flat,
/// screen-reader-friendly plain-text transcript.
///
/// Read-only: this never writes to the session store. The only side effect is
/// emitting the rendered transcript to stdout or, when `--out` is given, to a
/// file on disk.
pub async fn handle_transcript(opts: TranscriptOptions) -> Result<()> {
    let session_manager = SessionManager::instance();

    let session_id = match opts.session_id {
        Some(id) => id,
        None => {
            // `list_sessions` is ordered most-recent-first, so the head is the
            // "current/last" session.
            let sessions = session_manager.list_sessions().await?;
            sessions
                .into_iter()
                .next()
                .map(|s| s.id)
                .ok_or_else(|| anyhow::anyhow!("No sessions found."))?
        }
    };

    let session = session_manager
        .get_session(&session_id, true)
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Session '{}' not found or failed to read: {}",
                session_id,
                e
            )
        })?;

    let messages = session
        .conversation
        .as_ref()
        .map(|c| c.messages().clone())
        .unwrap_or_default();

    let transcript = render_transcript(&messages);

    if let Some(path) = opts.out {
        std::fs::write(&path, &transcript).map_err(|e| {
            anyhow::anyhow!("Failed to write transcript to {}: {}", path.display(), e)
        })?;
        println!("Transcript written to {}", path.display());
    } else {
        print!("{transcript}");
    }

    Ok(())
}

/// Render a slice of messages as a flat, screen-reader-friendly plain-text
/// transcript.
///
/// Each message becomes one labelled block:
/// * a user text message → `You:` followed by its text;
/// * an assistant message → `Assistant:` followed by its text, plus a
///   `Tool <name>:` line per tool request (with arguments rendered inline);
/// * a tool result (carried on a user message) → `Result:` followed by the
///   result text.
///
/// All emitted text has ANSI escape sequences stripped, so the output contains
/// zero `\x1b` bytes and no decorative characters. The function is pure and
/// deterministic: the same `messages` always yields byte-identical output.
pub fn render_transcript(messages: &[Message]) -> String {
    let tool_label = a11y_label("a11y.tool_call", "Tool call");
    let result_label = a11y_label("a11y.result", "Result");

    let mut blocks: Vec<String> = Vec::new();

    for message in messages {
        // A message can carry several kinds of content. Group them into the
        // labelled lines a screen reader expects, in document order.
        let mut body = String::new();

        // 1) Plain text (user prose or assistant prose).
        let text = collect_text(message);
        if !text.trim().is_empty() {
            body.push_str(&strip_ansi(text.trim_end()));
        }

        // 2) Tool requests: one labelled line each, e.g. `Tool shell:`.
        for content in &message.content {
            if let MessageContent::ToolRequest(req) = content {
                if !body.is_empty() {
                    body.push('\n');
                }
                match &req.tool_call {
                    Ok(call) => {
                        body.push_str(&format!("{} {}:", tool_label, strip_ansi(&call.name)));
                        if let Some(args) = &call.arguments {
                            let rendered = render_arguments(args);
                            if !rendered.is_empty() {
                                body.push('\n');
                                body.push_str(&strip_ansi(&rendered));
                            }
                        }
                    }
                    Err(e) => {
                        body.push_str(&format!("{}: {}", tool_label, strip_ansi(&e.to_string())));
                    }
                }
            }
        }

        // 3) Tool responses: a `Result:` block with the response text.
        for content in &message.content {
            if let MessageContent::ToolResponse(_) = content {
                let result_text = content.as_tool_response_text().unwrap_or_default();
                if !body.is_empty() {
                    body.push('\n');
                }
                body.push_str(&format!("{}:", result_label));
                let trimmed = result_text.trim();
                if !trimmed.is_empty() {
                    body.push('\n');
                    body.push_str(&strip_ansi(trimmed));
                }
            }
        }

        if body.trim().is_empty() {
            continue;
        }

        // Choose the speaker label. A user message that only carries tool
        // responses is already labelled `Result:` above, so it must not also be
        // prefixed with `You:`.
        let label = if message.role == Role::Assistant {
            Some(LABEL_ASSISTANT)
        } else if is_only_tool_responses(message) {
            None
        } else {
            Some(LABEL_YOU)
        };

        let block = match label {
            Some(label) => format!("{label}:\n{body}"),
            None => body,
        };

        blocks.push(block);
    }

    let mut out = blocks.join("\n\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// Concatenate the plain-text content of a message, newline-separated.
fn collect_text(message: &Message) -> String {
    message
        .content
        .iter()
        .filter_map(|c| c.as_text())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether a message's content is exclusively tool responses (the shape used
/// when a tool's result is carried back on a synthetic user message).
fn is_only_tool_responses(message: &Message) -> bool {
    !message.content.is_empty()
        && message
            .content
            .iter()
            .all(|c| matches!(c, MessageContent::ToolResponse(_)))
}

/// Render tool-call arguments as a compact, deterministic, ANSI-free string.
/// Keys are emitted in sorted order so the transcript is stable across runs.
fn render_arguments(args: &serde_json::Map<String, serde_json::Value>) -> String {
    if args.is_empty() {
        return String::new();
    }
    let mut keys: Vec<&String> = args.keys().collect();
    keys.sort();
    keys.into_iter()
        .map(|k| {
            let value = match args.get(k) {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(other) => other.to_string(),
                None => String::new(),
            };
            format!("  {k}: {value}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Remove ANSI/CSI escape sequences so the transcript contains zero `\x1b`
/// bytes and no terminal control glyphs.
///
/// Self-contained on purpose: `console::strip_ansi_codes` is gated behind the
/// crate's `ansi-parsing` feature, which this build does not enable, so any
/// `ESC [ ... <final byte 0x40..=0x7e>` sequence (and a bare `ESC`) is dropped
/// here directly.
fn strip_ansi(s: &str) -> String {
    if !s.contains('\u{1b}') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for cc in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&cc) {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{CallToolRequestParams, CallToolResult, Content};
    use rmcp::object;

    fn sample_messages() -> Vec<Message> {
        let user = Message::user().with_text("List the files please");

        let assistant = Message::assistant()
            .with_text("Sure, running that now.")
            .with_tool_request(
                "call-1",
                Ok(CallToolRequestParams::new("shell")
                    .with_arguments(object!({ "command": "ls -la" }))),
            );

        let tool_response = Message::user().with_tool_response(
            "call-1",
            Ok(CallToolResult::success(vec![Content::text(
                "total 0\nfile.txt",
            )])),
        );

        vec![user, assistant, tool_response]
    }

    #[test]
    fn renders_labelled_blocks_without_ansi() {
        let messages = sample_messages();
        let out = render_transcript(&messages);

        assert!(out.contains("You:"), "missing You label:\n{out}");
        assert!(
            out.contains("Assistant:"),
            "missing Assistant label:\n{out}"
        );
        assert!(out.contains("Tool"), "missing Tool label:\n{out}");

        // The whole point: a screen-reader transcript must contain zero ANSI
        // escape bytes.
        assert!(
            !out.contains('\u{1b}'),
            "transcript must contain zero ESC (\\x1b) bytes"
        );
        assert!(
            !out.as_bytes().contains(&0x1b),
            "transcript bytes must not contain 0x1b"
        );
    }

    #[test]
    fn includes_user_prose_tool_name_and_result() {
        let out = render_transcript(&sample_messages());
        assert!(out.contains("List the files please"));
        assert!(out.contains("Sure, running that now."));
        assert!(
            out.contains("Tool shell:"),
            "tool name line missing:\n{out}"
        );
        assert!(out.contains("ls -la"), "tool args missing:\n{out}");
        assert!(out.contains("Result:"), "result label missing:\n{out}");
        assert!(out.contains("file.txt"), "result body missing:\n{out}");
    }

    #[test]
    fn tool_response_message_is_not_labelled_you() {
        // A user message that only carries a tool response must be a `Result:`
        // block, never a `You:` block.
        let tool_response = Message::user().with_tool_response(
            "call-1",
            Ok(CallToolResult::success(vec![Content::text("ok")])),
        );
        let out = render_transcript(&[tool_response]);
        assert!(out.contains("Result:"));
        assert!(!out.contains("You:"), "tool-only message must not be You:");
    }

    #[test]
    fn empty_input_is_empty_string() {
        assert_eq!(render_transcript(&[]), "");
    }

    #[test]
    fn render_is_deterministic() {
        let messages = sample_messages();
        assert_eq!(render_transcript(&messages), render_transcript(&messages));
    }

    #[test]
    fn strip_ansi_removes_escape_sequences() {
        let coloured = format!("{}[31mred{}[0m plain", '\u{1b}', '\u{1b}');
        let cleaned = strip_ansi(&coloured);
        assert_eq!(cleaned, "red plain");
        assert!(!cleaned.contains('\u{1b}'));
    }

    #[test]
    fn no_upstream_branding_leaks() {
        let out = render_transcript(&sample_messages()).to_lowercase();
        assert!(!out.contains("goose"), "must not leak the goose name");
        assert!(!out.contains("block"), "must not leak the Block name");
    }
}
