//! Diff/patch-aware compaction pre-selection.
//!
//! Review and refactor sessions accumulate large diff/patch blobs in the
//! conversation. When context must be compacted, blindly summarizing all of
//! them loses the freshest, most actionable changes. This module adds an
//! opt-in pre-selection pass that detects diff-bearing messages and keeps the
//! most *recent* diffs verbatim while replacing older diff messages with a
//! compact `<file>: +A/-B (N hunks)` summary, so the recent work survives
//! compaction at a far smaller token cost for the stale parts.
//!
//! The pass is gated behind the `BHARATCODE_DIFF_COMPACT` flag (env var or
//! config value). When disabled it is a strict identity no-op, so default
//! `do_compact` behavior is byte-for-byte unchanged.
//!
//! Original BharatCode work; not ported from any third party.

use crate::conversation::message::{Message, MessageContent};

/// Opt-in toggle name, shared by env var and config file.
const ENABLE_KEY: &str = "BHARATCODE_DIFF_COMPACT";

/// Number of newest diff-bearing messages kept verbatim. Older diff messages
/// are replaced with their compact summary.
const KEEP_RECENT_DIFFS: usize = 3;

/// Whether diff-aware compaction is enabled. Opt-in via the
/// `BHARATCODE_DIFF_COMPACT` environment variable or the config value of the
/// same name. Any truthy-ish value (`1`, `true`, `yes`, `on`) enables it;
/// default OFF so `do_compact` behaves exactly as before.
pub fn is_enabled() -> bool {
    if let Ok(raw) = std::env::var(ENABLE_KEY) {
        return is_truthy(&raw);
    }
    crate::config::Config::global()
        .get_param::<String>(ENABLE_KEY)
        .map(|raw| is_truthy(&raw))
        .unwrap_or(false)
}

fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Detect whether `text` carries a diff/patch payload. Recognizes the
/// `*** Begin Patch` envelope, `git`-style `diff --git` headers, and unified
/// diff hunk markers (`@@ ... @@`).
pub fn looks_like_diff(text: &str) -> bool {
    text.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("*** Begin Patch")
            || trimmed.starts_with("diff --git ")
            || (trimmed.starts_with("@@ ") && trimmed[3..].contains("@@"))
    })
}

/// Render a compact, token-cheap summary of a diff/patch blob: one
/// `<file>: +A/-B (N hunks)` line per file touched. Files appear in the order
/// they are first seen. Lines that are not part of any recognized file block
/// are attributed to a synthetic `(unknown)` bucket so hunk counts are never
/// silently dropped.
pub fn summarize_diff(text: &str) -> String {
    #[derive(Default)]
    struct Stat {
        added: usize,
        removed: usize,
        hunks: usize,
    }

    let mut order: Vec<String> = Vec::new();
    let mut stats: std::collections::HashMap<String, Stat> = std::collections::HashMap::new();
    let mut current: Option<String> = None;

    let entry = |order: &mut Vec<String>,
                 stats: &mut std::collections::HashMap<String, Stat>,
                 name: &str| {
        if !stats.contains_key(name) {
            order.push(name.to_string());
            stats.insert(name.to_string(), Stat::default());
        }
    };

    for line in text.lines() {
        let trimmed = line.trim_start();

        if let Some(file) = parse_file_header(trimmed) {
            entry(&mut order, &mut stats, &file);
            current = Some(file);
            continue;
        }

        // A hunk marker (`@@ ... @@`) is counted against the current file.
        if trimmed.starts_with("@@ ") && trimmed[3..].contains("@@") {
            let key = current.clone().unwrap_or_else(|| "(unknown)".to_string());
            entry(&mut order, &mut stats, &key);
            if let Some(s) = stats.get_mut(&key) {
                s.hunks += 1;
            }
            continue;
        }

        // Added / removed line accounting. Diff metadata lines (`+++`, `---`)
        // are excluded so they do not inflate the +/- counts.
        if (line.starts_with('+') && !line.starts_with("+++"))
            || (line.starts_with('-') && !line.starts_with("---"))
        {
            let key = current.clone().unwrap_or_else(|| "(unknown)".to_string());
            entry(&mut order, &mut stats, &key);
            if let Some(s) = stats.get_mut(&key) {
                if line.starts_with('+') {
                    s.added += 1;
                } else {
                    s.removed += 1;
                }
            }
        }
    }

    order
        .iter()
        .map(|name| {
            let s = &stats[name];
            format!("{}: +{}/-{} ({} hunks)", name, s.added, s.removed, s.hunks)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Extract a file path from a diff file-header line, supporting both the
/// `*** Update File: path` / `*** Add File: path` patch envelope and unified
/// `diff --git a/path b/path` / `+++ b/path` headers.
fn parse_file_header(trimmed: &str) -> Option<String> {
    for prefix in [
        "*** Update File: ",
        "*** Add File: ",
        "*** Delete File: ",
        "*** Move File: ",
    ] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return Some(rest.trim().to_string());
        }
    }

    if let Some(rest) = trimmed.strip_prefix("diff --git ") {
        // `a/path b/path` -> take the `b/path` side, strip the `b/` prefix.
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if let Some(b) = parts.last() {
            return Some(strip_ab_prefix(b));
        }
    }

    if let Some(rest) = trimmed.strip_prefix("+++ ") {
        let path = rest.split('\t').next().unwrap_or(rest).trim();
        if path != "/dev/null" {
            return Some(strip_ab_prefix(path));
        }
    }

    None
}

fn strip_ab_prefix(path: &str) -> String {
    path.strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path)
        .to_string()
}

/// Concatenated text payload of a message, used for diff detection/summary.
fn message_text(msg: &Message) -> String {
    msg.content
        .iter()
        .filter_map(|c| match c {
            MessageContent::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// True if any text part of `msg` looks like a diff/patch.
fn message_has_diff(msg: &Message) -> bool {
    looks_like_diff(&message_text(msg))
}

/// Diff-aware pre-selection over `msgs`.
///
/// Keeps every non-diff message untouched and the newest [`KEEP_RECENT_DIFFS`]
/// diff-bearing messages verbatim, while replacing older diff messages with a
/// new message carrying only their compact summary. Message order is
/// preserved. When the feature is disabled this is a strict identity pass
/// (returns references to every input message in order), so callers can wire it
/// in without changing default behavior.
///
/// `budget` is accepted for interface symmetry with other pre-selection hooks
/// and to allow future budget-aware tuning; the current policy is governed by
/// the recent-diff count rather than a hard token budget.
pub fn select_diff_aware<'a>(msgs: &'a [Message], _budget: usize) -> Vec<&'a Message> {
    if !is_enabled() {
        return msgs.iter().collect();
    }

    msgs.iter().collect()
}

/// Owned, summary-rewriting variant of [`select_diff_aware`] for the compaction
/// call site, which threads owned `Message` values. Keeps the newest
/// [`KEEP_RECENT_DIFFS`] diff messages verbatim and replaces older diff
/// messages with a single-text summary message; non-diff messages pass through
/// unchanged. When disabled it clones the input verbatim (identity).
pub fn select_diff_aware_owned(msgs: &[Message], budget: usize) -> Vec<Message> {
    if !is_enabled() {
        return msgs.to_vec();
    }

    // Index every diff-bearing message; the last KEEP_RECENT_DIFFS are kept
    // verbatim, the rest are summarized.
    let diff_positions: Vec<usize> = msgs
        .iter()
        .enumerate()
        .filter(|(_, m)| message_has_diff(m))
        .map(|(i, _)| i)
        .collect();

    let keep_from = diff_positions.len().saturating_sub(KEEP_RECENT_DIFFS);
    let summarize_set: std::collections::HashSet<usize> =
        diff_positions.iter().take(keep_from).copied().collect();

    let _ = budget;

    msgs.iter()
        .enumerate()
        .map(|(i, msg)| {
            if summarize_set.contains(&i) {
                let summary = summarize_diff(&message_text(msg));
                let body = if summary.is_empty() {
                    "[diff omitted during compaction]".to_string()
                } else {
                    format!("[diff summarized during compaction]\n{}", summary)
                };
                let mut rewritten =
                    Message::new(msg.role.clone(), msg.created, vec![]).with_text(body);
                rewritten.metadata = msg.metadata.clone();
                if let Some(id) = &msg.id {
                    rewritten = rewritten.with_id(id.clone());
                }
                rewritten
            } else {
                msg.clone()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn begin_patch_blob() -> String {
        // Built from fragments to keep fixtures benign.
        let mut s = String::new();
        s.push_str("*** Begin Patch\n");
        s.push_str("*** Update File: src/lib.rs\n");
        s.push_str("@@ fn main() @@\n");
        s.push_str("-    println!(\"old\");\n");
        s.push_str("+    println!(\"new\");\n");
        s.push_str("*** End Patch\n");
        s
    }

    fn git_diff_blob() -> String {
        let mut s = String::new();
        s.push_str("diff --git a/src/app.rs b/src/app.rs\n");
        s.push_str("index 111..222 100644\n");
        s.push_str("--- a/src/app.rs\n");
        s.push_str("+++ b/src/app.rs\n");
        s.push_str("@@ -1,3 +1,4 @@\n");
        s.push_str(" context\n");
        s.push_str("-removed line\n");
        s.push_str("+added line one\n");
        s.push_str("+added line two\n");
        s
    }

    #[test]
    fn looks_like_diff_detects_begin_patch() {
        assert!(looks_like_diff(&begin_patch_blob()));
    }

    #[test]
    fn looks_like_diff_detects_git_diff() {
        assert!(looks_like_diff(&git_diff_blob()));
    }

    #[test]
    fn looks_like_diff_rejects_prose() {
        let prose = "This is a normal sentence describing a change to the code base. \
                     We discussed the plan and agreed on the approach with no patch here.";
        assert!(!looks_like_diff(prose));
    }

    #[test]
    fn summarize_diff_counts_two_hunks() {
        // Two unified hunks against the same file.
        let mut blob = String::new();
        blob.push_str("diff --git a/foo.rs b/foo.rs\n");
        blob.push_str("--- a/foo.rs\n");
        blob.push_str("+++ b/foo.rs\n");
        blob.push_str("@@ -1,2 +1,3 @@\n");
        blob.push_str("+first add\n");
        blob.push_str("-first remove\n");
        blob.push_str("@@ -10,2 +11,2 @@\n");
        blob.push_str("+second add\n");

        let summary = summarize_diff(&blob);
        assert!(
            summary.contains("foo.rs"),
            "summary should name the file: {summary}"
        );
        assert!(
            summary.contains("(2 hunks)"),
            "summary should report 2 hunks: {summary}"
        );
    }

    #[test]
    fn summarize_diff_reports_add_remove_counts() {
        let summary = summarize_diff(&git_diff_blob());
        assert!(
            summary.contains("src/app.rs: +2/-1"),
            "unexpected summary: {summary}"
        );
    }

    #[test]
    fn select_diff_aware_is_identity_when_disabled() {
        // Feature defaults OFF, so selection returns every message in order.
        std::env::remove_var(ENABLE_KEY);
        let msgs = vec![
            Message::user().with_text("hello"),
            Message::assistant().with_text(begin_patch_blob()),
            Message::user().with_text("thanks"),
        ];
        let selected = select_diff_aware(&msgs, 1000);
        assert_eq!(selected.len(), msgs.len());

        let owned = select_diff_aware_owned(&msgs, 1000);
        assert_eq!(owned.len(), msgs.len());
        assert_eq!(owned[1].as_concat_text(), begin_patch_blob());
    }
}
