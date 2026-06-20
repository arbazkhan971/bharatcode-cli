//! `bharatcode review-diff` — a focused, single-pass code review of the working
//! git diff.
//!
//! This is the lightweight sibling of the orchestrated `bharatcode review`
//! command. Where `review` discovers `.agents/checks/*.md` subagents and fans a
//! review out across parallel subprocesses, `review-diff` does the simplest
//! useful thing: gather the working diff, hand it to a single review-focused
//! agent turn through the shared run/session path, and let the agent stream its
//! findings.
//!
//! It is intentionally **read-only** with respect to git: it only ever runs
//! `git rev-parse` and `git diff`, never mutating the index or the working tree.
//!
//! User-facing labels route through the i18n layer via [`label`], falling back
//! to English when the active locale has no entry for a key, so English output
//! stays stable while translations can land later.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::session::{build_session, SessionBuilderConfig};

/// Options for `bharatcode review-diff`.
#[derive(Debug, Clone, Default)]
pub struct ReviewDiffOptions {
    /// Diff range to review (e.g. `main...HEAD`). When `None`, reviews the
    /// working tree against `HEAD`.
    pub range: Option<String>,
    /// Provider override for the review agent.
    pub provider: Option<String>,
    /// Model override for the review agent.
    pub model: Option<String>,
    /// Suppress non-result output from the underlying agent.
    pub quiet: bool,
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

/// Entry point for the `bharatcode review-diff` subcommand.
pub async fn handle_review_diff(opts: ReviewDiffOptions) -> Result<()> {
    let repo_root = find_repo_root().context("not inside a git repository")?;
    let diff = collect_diff(&repo_root, opts.range.as_deref())?;

    if diff.trim().is_empty() {
        eprintln!(
            "{}",
            label(
                "review.no_changes",
                "bharatcode review: no changes to review"
            )
        );
        return Ok(());
    }

    let prompt = build_prompt(&diff, opts.range.as_deref());

    let mut session = build_session(SessionBuilderConfig {
        session_id: None,
        no_session: true,
        no_profile: true,
        builtins: vec!["developer".to_string()],
        provider: opts.provider.clone(),
        model: opts.model.clone(),
        quiet: opts.quiet,
        output_format: "text".to_string(),
        ..SessionBuilderConfig::default()
    })
    .await;

    session.headless(prompt).await
}

/// Assemble the review-focused prompt sent to the agent.
///
/// The diff is embedded in a fenced ```diff block and prefixed with an
/// i18n-routed instruction header that frames the turn as a code review.
fn build_prompt(diff: &str, range: Option<&str>) -> String {
    let instructions = label(
        "review.prompt_instructions",
        "You are reviewing the following git diff. Identify correctness bugs, \
         security issues, and clear simplifications. For each finding, name the \
         file and line, explain the problem, and suggest a fix. Be concise and \
         skip praise. If there are no meaningful issues, say so.",
    );
    let findings_header = label("review.findings_header", "## Review findings");
    let scope = match range {
        Some(r) => format!("`{r}`"),
        None => label("review.scope_working_tree", "the working tree vs HEAD"),
    };

    let mut out = String::new();
    out.push_str(&instructions);
    out.push_str("\n\nScope: ");
    out.push_str(&scope);
    out.push_str("\n\nReport your output under a ");
    out.push_str(&findings_header);
    out.push_str(" heading.\n\n## Diff\n\n```diff\n");
    out.push_str(diff.trim_end_matches('\n'));
    out.push_str("\n```\n");
    out
}

/// Walk up from the current directory to find the repository root.
fn find_repo_root() -> Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("failed to run git rev-parse")?;
    if !output.status.success() {
        bail!("not inside a git repository");
    }
    let path = String::from_utf8(output.stdout)
        .context("git rev-parse produced non-UTF-8 output")?
        .trim()
        .to_string();
    Ok(PathBuf::from(path))
}

/// Collect the diff to review. With an explicit `range` the diff is that range;
/// otherwise it is the working tree (staged + unstaged) against `HEAD`.
fn collect_diff(repo_root: &PathBuf, range: Option<&str>) -> Result<String> {
    let mut cmd = Command::new("git");
    cmd.current_dir(repo_root).arg("diff");
    match range {
        Some(r) => {
            cmd.arg(r);
        }
        None => {
            cmd.arg("HEAD");
        }
    }
    let output = cmd.output().context("failed to run git diff")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git diff failed: {}", stderr.trim());
    }
    String::from_utf8(output.stdout).context("git diff produced non-UTF-8 output")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_embeds_diff_and_findings_header() {
        let prompt = build_prompt("--- a\n+++ b\n", None);
        assert!(prompt.contains("```diff\n--- a\n+++ b\n```"));
        assert!(prompt.contains("## Review findings"));
        assert!(prompt.contains("## Diff"));
    }

    #[test]
    fn prompt_reports_explicit_range_scope() {
        let prompt = build_prompt("x", Some("main...HEAD"));
        assert!(prompt.contains("`main...HEAD`"));
    }

    #[test]
    fn prompt_falls_back_to_working_tree_scope() {
        let prompt = build_prompt("x", None);
        assert!(prompt.contains("the working tree vs HEAD"));
    }
}
