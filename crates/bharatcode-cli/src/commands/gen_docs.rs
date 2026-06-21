//! `bharatcode gen-docs` — a focused, single-pass documentation-draft generator
//! for a source file.
//!
//! This is a lightweight sibling of `gen-tests`: where `gen-tests` reads a
//! target source file and hands it to a single test-authoring agent turn,
//! `gen-docs` reads a target source file and hands it to a single
//! documentation-authoring agent turn through the same shared run/session path.
//! The agent is asked to produce a concise module-level doc-comment and an
//! API-reference draft (a README-style section) for the code.
//!
//! The draft is streamed to stdout. With `--write`, the agent is additionally
//! instructed to save the draft to a sibling Markdown file (`<stem>.md` next to
//! the source), so the proposal lands on disk next to the code it documents.
//!
//! The instruction encodes the repo's `AGENTS.md` comment policy: write
//! self-documenting docs for the *why* and the public surface, and do **not**
//! restate trivial getters/setters, constructors, or self-evident operations.
//!
//! User-facing labels route through the i18n layer via [`label`], falling back
//! to English when the active locale has no entry for a key, so English output
//! stays stable while translations can land later. It is intentionally
//! read-only with respect to the source: it only ever reads the target file.

use std::path::Path;

use anyhow::{Context, Result};

use crate::session::{build_session, SessionBuilderConfig};

/// The "no trivial comments" rule string, lifted from the repo's `AGENTS.md`
/// comment policy. It is embedded verbatim in the agent instruction so the
/// generated documentation matches the conventions enforced elsewhere in the
/// codebase, and is exposed for tests and the builtin doc-guide section.
const NO_TRIVIAL_RULE: &str = "Do not restate what the code does or document \
trivial getters, setters, constructors, or self-evident operations; document \
the public API surface, intent, invariants, and the \"why\".";

/// Options for `bharatcode gen-docs`.
#[derive(Debug, Clone, Default)]
pub struct GenDocsOptions {
    /// Path to the source file to document.
    pub path: String,
    /// When set, the draft is also saved to `<stem>.md` next to the source.
    pub write: bool,
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

/// Compute the sibling Markdown output path for a source file: the source path
/// with its extension replaced by `md`.
///
/// `output_path("src/foo.rs")` returns `src/foo.md`. A path with no extension
/// gains an `.md` extension; a directory component is preserved.
pub fn output_path(path: &str) -> String {
    Path::new(path)
        .with_extension("md")
        .to_string_lossy()
        .into_owned()
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

/// A short, embeddable guide describing what `gen-docs` produces and the
/// comment policy it follows.
///
/// This is consumed by the builtin doc/skill surface so the documentation
/// conventions stay discoverable from inside a session, and is also asserted in
/// the unit tests to keep the policy string and the feature in sync.
pub fn doc_guide_section() -> String {
    format!(
        "## gen-docs\n\nGenerate a concise module-level doc-comment and an \
         API-reference draft for a source file in one agent turn. {rule}",
        rule = NO_TRIVIAL_RULE
    )
}

/// Entry point for the `bharatcode gen-docs` subcommand.
pub async fn handle_gen_docs(opts: GenDocsOptions) -> Result<()> {
    let source = std::fs::read_to_string(&opts.path)
        .with_context(|| format!("failed to read source file: {}", opts.path))?;

    let out_path = if opts.write {
        Some(output_path(&opts.path))
    } else {
        None
    };

    let prompt = build_prompt(&opts.path, &source, out_path.as_deref());

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

/// Assemble the documentation-generation prompt sent to the agent.
///
/// The source is embedded in a fenced code block and prefixed with an
/// i18n-routed instruction header that frames the turn as documentation
/// authoring, names the file's basename, and embeds the no-trivial-comments
/// rule. When `out_path` is `Some`, the agent is told to also write the draft
/// to that sibling Markdown path; otherwise it prints the draft to stdout.
fn build_prompt(path: &str, source: &str, out_path: Option<&str>) -> String {
    let name = basename(path);
    let instructions = label(
        "gen_docs.prompt_instructions",
        "Produce concise API documentation for the following code: a \
         module-level doc-comment describing intent and a README-style section \
         documenting the public surface (types, functions, and their \
         contracts).",
    );
    let policy = label("gen_docs.comment_policy", NO_TRIVIAL_RULE);
    let docs_header = label("gen_docs.output_header", "## Documentation draft");

    let mut out = String::new();
    out.push_str(&instructions);
    out.push_str("\n\n");
    out.push_str(&policy);
    out.push_str("\n\nTarget file: `");
    out.push_str(&name);
    out.push_str("`\n");

    match out_path {
        Some(dest) => {
            out.push_str("Write the finished draft to the file `");
            out.push_str(dest);
            out.push_str("` next to the source, then summarise what you wrote.\n");
        }
        None => {
            out.push_str("Print the finished draft to your reply.\n");
        }
    }

    out.push_str("\nReport your output under a ");
    out.push_str(&docs_header);
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
    fn output_path_swaps_extension_to_md() {
        assert_eq!(output_path("src/foo.rs"), "src/foo.md");
        assert_eq!(output_path("a/b/widget.rs"), "a/b/widget.md");
        assert_eq!(output_path("notes.py"), "notes.md");
    }

    #[test]
    fn output_path_adds_md_when_no_extension() {
        assert_eq!(output_path("README"), "README.md");
        assert_eq!(output_path("src/mod"), "src/mod.md");
    }

    #[test]
    fn basename_strips_directory() {
        assert_eq!(basename("a/b/foo.rs"), "foo.rs");
        assert_eq!(basename("foo.py"), "foo.py");
    }

    #[test]
    fn prompt_embeds_file_header_and_source() {
        let prompt = build_prompt("src/widget.rs", "fn main() {}", None);
        assert!(prompt.contains("Target file: `widget.rs`"));
        assert!(prompt.contains("## Source: `widget.rs`"));
        assert!(prompt.contains("```\nfn main() {}\n```"));
        assert!(prompt.contains("## Documentation draft"));
    }

    #[test]
    fn prompt_respects_no_trivial_comments_rule() {
        let prompt = build_prompt("src/widget.rs", "fn main() {}", None);
        assert!(prompt.contains("trivial getters"));
        assert!(prompt.contains(NO_TRIVIAL_RULE));
    }

    #[test]
    fn prompt_without_write_prints_to_reply() {
        let prompt = build_prompt("foo.rs", "x", None);
        assert!(prompt.contains("Print the finished draft"));
        assert!(!prompt.contains("Write the finished draft to the file"));
    }

    #[test]
    fn prompt_with_write_targets_sibling_md() {
        let dest = output_path("src/foo.rs");
        let prompt = build_prompt("src/foo.rs", "x", Some(&dest));
        assert!(prompt.contains("Write the finished draft to the file `src/foo.md`"));
    }

    #[test]
    fn doc_guide_section_embeds_policy() {
        let guide = doc_guide_section();
        assert!(guide.contains("## gen-docs"));
        assert!(guide.contains(NO_TRIVIAL_RULE));
    }
}
