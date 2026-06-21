//! `bharatcode gen-tests` — a focused, single-pass unit-test generator for a
//! source file.
//!
//! This is a lightweight sibling of `review-diff`: where `review-diff` hands a
//! git diff to a single review-focused agent turn, `gen-tests` reads a target
//! source file and hands it to a single test-authoring agent turn through the
//! same shared run/session path. The agent is asked to write idiomatic unit
//! tests that respect the project's existing test conventions and to emit the
//! proposed test file(s).
//!
//! Language/framework is detected from the file extension as a *hint* only
//! (`rs` → `cargo test`, `py` → `pytest`, `ts`/`tsx`/`js` → `vitest`/`jest`),
//! and is woven into the instruction so the agent matches the ecosystem.
//!
//! User-facing labels route through the i18n layer via [`label`], falling back
//! to English when the active locale has no entry for a key, so English output
//! stays stable while translations can land later. It is intentionally
//! read-only: it only ever reads the target file.

use std::path::Path;

use anyhow::{Context, Result};

use crate::session::{build_session, SessionBuilderConfig};

/// Options for `bharatcode gen-tests`.
#[derive(Debug, Clone, Default)]
pub struct GenTestsOptions {
    /// Path to the source file (or directory) to generate tests for.
    pub path: String,
    /// Optional explicit framework override (e.g. `pytest`, `vitest`). When
    /// `None`, the framework is inferred from the file extension.
    pub framework: Option<String>,
}

/// Look up a user-facing label through the i18n layer, falling back to the
/// English `default` when the active locale table has no entry for `key`.
///
/// `tr!` echoes the key back when it is missing, so an unchanged key is treated
/// as "untranslated" and the English default is used.
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// Infer a build/test framework hint string from a path's extension.
///
/// The returned string is a short, human-readable hint (e.g. `cargo test`)
/// that frames the kind of test the agent should write. Unknown extensions
/// fall back to a generic hint so the agent still produces *something*
/// idiomatic for the ecosystem it sees.
fn framework_hint(path: &str) -> &'static str {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "rs" => "cargo test",
        "py" => "pytest unit tests",
        "ts" | "tsx" => "vitest or jest tests",
        "js" | "jsx" | "mjs" | "cjs" => "jest tests",
        "go" => "go test",
        "java" => "JUnit tests",
        "rb" => "RSpec tests",
        _ => "the project's existing test framework",
    }
}

/// The basename (file name component) of `path`, falling back to the full path
/// when it has no file-name component.
fn basename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| path.to_string())
}

/// Entry point for the `bharatcode gen-tests` subcommand.
pub async fn handle_gen_tests(opts: GenTestsOptions) -> Result<()> {
    let source = std::fs::read_to_string(&opts.path)
        .with_context(|| format!("failed to read source file: {}", opts.path))?;

    let hint = match opts.framework.as_deref() {
        Some(fw) if !fw.trim().is_empty() => fw.trim().to_string(),
        _ => framework_hint(&opts.path).to_string(),
    };

    let prompt = build_prompt(&opts.path, &source, &hint);

    let mut session = build_session(SessionBuilderConfig {
        session_id: None,
        no_session: true,
        no_profile: true,
        builtins: vec!["developer".to_string()],
        provider: None,
        model: None,
        quiet: false,
        output_format: "text".to_string(),
        ..SessionBuilderConfig::default()
    })
    .await;

    session.headless(prompt).await
}

/// Assemble the test-generation prompt sent to the agent.
///
/// The source is embedded in a fenced code block and prefixed with an
/// i18n-routed instruction header that frames the turn as test authoring,
/// naming the file's basename and the inferred framework hint.
fn build_prompt(path: &str, source: &str, hint: &str) -> String {
    let name = basename(path);
    let instructions = label(
        "gen_tests.prompt_instructions",
        "Write idiomatic unit tests for the following code. Respect the \
         project's existing test conventions: match the style, naming, and \
         layout of nearby tests, and reuse existing helpers and fixtures where \
         they exist. Cover the important behaviours and edge cases. Emit the \
         proposed test file(s) and say where each should live.",
    );
    let tests_header = label("gen_tests.output_header", "## Proposed tests");

    let mut out = String::new();
    out.push_str(&instructions);
    out.push_str("\n\nTarget file: `");
    out.push_str(&name);
    out.push_str("`\nFramework hint: ");
    out.push_str(hint);
    out.push_str("\n\nReport your output under a ");
    out.push_str(&tests_header);
    out.push_str(" heading.\n\n## Source: `");
    out.push_str(&name);
    out.push_str("`\n\n```\n");
    out.push_str(source.trim_end_matches('\n'));
    out.push_str("\n```\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framework_hint_rust_is_cargo_test() {
        assert_eq!(framework_hint("foo.rs"), "cargo test");
        assert_eq!(framework_hint("src/lib/widget.rs"), "cargo test");
    }

    #[test]
    fn framework_hint_python_is_pytest() {
        assert!(framework_hint("foo.py").contains("pytest"));
    }

    #[test]
    fn framework_hint_typescript_and_javascript() {
        assert!(framework_hint("foo.ts").contains("vitest"));
        assert!(framework_hint("foo.tsx").contains("vitest"));
        assert!(framework_hint("foo.js").contains("jest"));
    }

    #[test]
    fn framework_hint_unknown_falls_back() {
        assert_eq!(
            framework_hint("README"),
            "the project's existing test framework"
        );
        assert_eq!(
            framework_hint("notes.txt"),
            "the project's existing test framework"
        );
    }

    #[test]
    fn basename_strips_directory() {
        assert_eq!(basename("a/b/foo.rs"), "foo.rs");
        assert_eq!(basename("foo.py"), "foo.py");
    }

    #[test]
    fn prompt_includes_basename_and_hint() {
        let prompt = build_prompt("src/widget.rs", "fn main() {}", "cargo test");
        assert!(prompt.contains("widget.rs"));
        assert!(!prompt.contains("src/widget.rs```"));
        assert!(prompt.contains("cargo test"));
        assert!(prompt.contains("## Proposed tests"));
        assert!(prompt.contains("fn main() {}"));
    }

    #[test]
    fn prompt_embeds_source_in_code_block() {
        let prompt = build_prompt("foo.py", "def add(a, b):\n    return a + b\n", "pytest");
        assert!(prompt.contains("```\ndef add(a, b):\n    return a + b\n```"));
    }
}
