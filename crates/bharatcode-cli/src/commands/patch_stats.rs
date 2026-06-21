//! Apply-patch hunk statistics for BharatCode.
//!
//! Codex-style `apply_patch` envelopes describe edits as a sequence of file
//! hunks wrapped between `*** Begin Patch` / `*** End Patch` markers, with one
//! `*** Add File:` / `*** Update File:` / `*** Delete File:` header per touched
//! file. This module reads such an envelope and produces a `git diff --stat`
//! style summary: how many files were touched, and how many lines were added /
//! removed per file.
//!
//! It is intentionally self-contained — a tiny, dependency-free line scanner
//! over the envelope text — so it can be reused from the `cost` summary footer
//! (and elsewhere) without pulling in the full patch applier. It only *reads*
//! envelope text; it never touches the filesystem or applies anything.
//!
//! The line-counting rule mirrors the envelope grammar: inside a hunk body,
//! lines starting with `+` are additions and lines starting with `-` are
//! removals, with the diff sentinels `+++` / `---` excluded so a file-marker
//! line is never miscounted as content.
//!
//! Original BharatCode work; not ported from any third party. The envelope
//! grammar it parses originates with OpenAI Codex `apply_patch` (Apache-2.0);
//! this is an independent stats-only reader, not a port of that parser.

use std::path::PathBuf;

use bharatcode_core::config::paths::Paths;

/// Envelope markers (a subset of the apply-patch grammar). Kept local so this
/// reader stays independent of the full patch crate.
const BEGIN_PATCH_MARKER: &str = "*** Begin Patch";
const END_PATCH_MARKER: &str = "*** End Patch";
const ADD_FILE_MARKER: &str = "*** Add File: ";
const UPDATE_FILE_MARKER: &str = "*** Update File: ";
const DELETE_FILE_MARKER: &str = "*** Delete File: ";

/// File name (under the config dir) holding the most recent apply-patch
/// envelope for the active session. When this sidecar exists and contains an
/// envelope, `bharatcode cost` renders a one-line patch-activity footer; when
/// it is absent, the cost output is byte-identical to before (footer omitted).
pub const RECENT_PATCH_FILE: &str = "recent-patch.log";

/// Per-file add/remove counts for a single touched file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchStat {
    /// Path of the touched file, as written in the envelope header.
    pub path: String,
    /// Number of added lines attributed to this file (`+` lines, excluding the
    /// `+++` diff sentinel).
    pub added: usize,
    /// Number of removed lines attributed to this file (`-` lines, excluding the
    /// `---` diff sentinel).
    pub removed: usize,
}

/// Parse an apply-patch envelope into per-file add/remove counts.
///
/// Walks the envelope line by line, tracking the "current file" set by each
/// `*** Add File:` / `*** Update File:` / `*** Delete File:` header, and
/// attributes every subsequent `+`/`-` content line to that file. The diff
/// sentinels `+++` and `---` are ignored so a unified-diff-style header line is
/// never miscounted. Lines outside any file header (and the `*** Begin/End
/// Patch` markers) contribute nothing.
///
/// Files appear in the returned vector in first-seen order. A file header with
/// no content lines (e.g. a bare `*** Delete File:`) still yields a `PatchStat`
/// with zero counts, so "files touched" is faithful.
pub fn parse_patch_stats(envelope: &str) -> Vec<PatchStat> {
    let mut stats: Vec<PatchStat> = Vec::new();
    // Index into `stats` of the file the scanner is currently inside, if any.
    let mut current: Option<usize> = None;

    for raw in envelope.lines() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let trimmed = line.trim_start();

        if let Some(path) = file_header_path(trimmed) {
            stats.push(PatchStat {
                path: path.to_string(),
                added: 0,
                removed: 0,
            });
            current = Some(stats.len() - 1);
            continue;
        }

        // Patch-level markers and the move/eof/context markers carry no counts.
        if trimmed == BEGIN_PATCH_MARKER
            || trimmed == END_PATCH_MARKER
            || trimmed.starts_with("*** ")
            || trimmed.starts_with("@@")
        {
            continue;
        }

        let Some(idx) = current else {
            continue;
        };

        // Use the original (untrimmed) line for +/- classification: leading
        // whitespace before a real change marker is not part of the grammar,
        // but trimming first keeps lenient envelopes counting correctly. We
        // classify on the trimmed view to match the parser's leniency.
        if is_addition(trimmed) {
            stats[idx].added += 1;
        } else if is_removal(trimmed) {
            stats[idx].removed += 1;
        }
    }

    stats
}

/// If `trimmed` is a file-introducing header, return the file path it names.
fn file_header_path(trimmed: &str) -> Option<&str> {
    if let Some(rest) = trimmed.strip_prefix(ADD_FILE_MARKER) {
        Some(rest.trim())
    } else if let Some(rest) = trimmed.strip_prefix(UPDATE_FILE_MARKER) {
        Some(rest.trim())
    } else {
        trimmed.strip_prefix(DELETE_FILE_MARKER).map(str::trim)
    }
}

/// Whether `line` is a counted addition: starts with `+` but is not the `+++`
/// unified-diff header sentinel.
fn is_addition(line: &str) -> bool {
    line.starts_with('+') && !line.starts_with("+++")
}

/// Whether `line` is a counted removal: starts with `-` but is not the `---`
/// unified-diff header sentinel.
fn is_removal(line: &str) -> bool {
    line.starts_with('-') && !line.starts_with("---")
}

/// Render a one-line `git diff --stat` style summary: `N files, +X/-Y`.
///
/// `N` is the number of touched files, `X` the total additions and `Y` the
/// total removals across all of them. Pluralisation follows English
/// conventions (`1 file` vs `2 files`).
pub fn render_diffstat(stats: &[PatchStat]) -> String {
    let files = stats.len();
    let added: usize = stats.iter().map(|s| s.added).sum();
    let removed: usize = stats.iter().map(|s| s.removed).sum();
    let noun = if files == 1 { "file" } else { "files" };
    format!("{files} {noun}, +{added}/-{removed}")
}

/// Absolute path of the recent-patch sidecar under the config dir.
pub fn recent_patch_log_path() -> PathBuf {
    Paths::in_config_dir(RECENT_PATCH_FILE)
}

/// Read the most recent apply-patch envelope from the session sidecar, if one
/// exists and is non-empty.
///
/// Returns `None` when the sidecar is absent, unreadable, or blank — which is
/// the default case, so callers that gate optional output on this stay
/// byte-identical to their previous behaviour when no patch data is present.
pub fn recent_patch_envelope() -> Option<String> {
    let contents = std::fs::read_to_string(recent_patch_log_path()).ok()?;
    if contents.trim().is_empty() {
        None
    } else {
        Some(contents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A two-file envelope: one Add (3 `+` lines) and one Update (2 `+`, 1 `-`).
    /// The Update header carries a `+++`/`---` pair that must be excluded.
    const TWO_FILE_ENVELOPE: &str = "\
*** Begin Patch
*** Add File: src/new.rs
+fn added_one() {}
+fn added_two() {}
+fn added_three() {}
*** Update File: src/old.rs
@@ fn existing()
+++ src/old.rs
--- src/old.rs
+let added_a = 1;
+let added_b = 2;
-let removed_a = 0;
*** End Patch
";

    #[test]
    fn parses_per_file_add_remove_counts() {
        let stats = parse_patch_stats(TWO_FILE_ENVELOPE);
        assert_eq!(stats.len(), 2);

        assert_eq!(stats[0].path, "src/new.rs");
        assert_eq!(stats[0].added, 3);
        assert_eq!(stats[0].removed, 0);

        assert_eq!(stats[1].path, "src/old.rs");
        assert_eq!(stats[1].added, 2);
        assert_eq!(stats[1].removed, 1);
    }

    #[test]
    fn diffstat_totals_files_adds_dels() {
        let stats = parse_patch_stats(TWO_FILE_ENVELOPE);
        assert_eq!(render_diffstat(&stats), "2 files, +5/-1");
    }

    #[test]
    fn delete_header_counts_as_a_touched_file_with_zero_lines() {
        let envelope = "\
*** Begin Patch
*** Delete File: src/gone.rs
*** End Patch
";
        let stats = parse_patch_stats(envelope);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].path, "src/gone.rs");
        assert_eq!(stats[0].added, 0);
        assert_eq!(stats[0].removed, 0);
        assert_eq!(render_diffstat(&stats), "1 file, +0/-0");
    }

    #[test]
    fn diff_sentinels_are_not_counted() {
        let envelope = "\
*** Begin Patch
*** Update File: a.txt
+++ a.txt
--- a.txt
+real add
-real del
*** End Patch
";
        let stats = parse_patch_stats(envelope);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].added, 1);
        assert_eq!(stats[0].removed, 1);
    }

    #[test]
    fn empty_envelope_yields_no_stats() {
        assert!(parse_patch_stats("").is_empty());
        assert_eq!(render_diffstat(&[]), "0 files, +0/-0");
    }

    #[test]
    fn lines_before_any_file_header_are_ignored() {
        let envelope = "\
*** Begin Patch
+stray line outside any file
*** Add File: x
+inside
*** End Patch
";
        let stats = parse_patch_stats(envelope);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].added, 1);
    }
}
