//! RAG / index readiness doctor check — BharatCode v49.
//!
//! A single, read-only diagnostic that reports whether the current codebase is
//! ready for semantic indexing (the kind of pre-flight a RAG pipeline wants
//! before it scans a tree):
//!
//!   1. **Scannable file count** — how many files a bounded, gitignore-aware
//!      walk would visit. Build artefacts, vendored trees, and hidden files are
//!      skipped so the figure reflects real source, not noise.
//!   2. **`.gitignore` presence** — a `.gitignore` keeps an index scan bounded
//!      and signal-dense; its absence is worth flagging.
//!   3. **RAG switch** — whether `BHARATCODE_RAG` is enabled, surfaced as a hint
//!      so the operator knows whether indexing would actually run.
//!
//! The walk is deliberately conservative and side-effect free: it is bounded by
//! the same scan limits the codebase-context scanner uses (a maximum depth and a
//! hard ceiling on entries visited, so a pathological repo can never blow up
//! wall-clock time), it respects `.gitignore` directory rules and hidden files,
//! and it only ever *reads* directory metadata — it never writes, mutates
//! config, or shells out.
//!
//! This module is intentionally self-contained: it does not depend on the
//! codebase-context module, only mirroring its bounds, so the doctor check stays
//! decoupled from that opt-in feature.

use std::collections::BTreeSet;
use std::path::Path;

use crate::commands::doctor_checks::Status;

/// Config / environment key for the RAG switch. Read-only here: this check only
/// reports whether it is on, it never flips it.
const RAG_KEY: &str = "BHARATCODE_RAG";

/// Maximum directory depth descended by the readiness walk. Mirrors the
/// codebase-context scanner's depth posture but a little deeper, since here we
/// want a representative *file count*, not just a top-level layout.
const MAX_DEPTH: usize = 8;

/// Hard ceiling on filesystem entries inspected, so a huge or pathological tree
/// can never make the walk run unbounded. Mirrors `ScanLimits::max_entries`.
const MAX_ENTRIES: usize = 5_000;

/// Above this scannable-file count the tree is considered "huge" for a single
/// bounded scan, and the check warns rather than passing outright.
const HUGE_FILE_COUNT: usize = 4_000;

/// Interpret a raw flag value as truthy. Anything not clearly "on" is off, so a
/// typo never silently enables RAG.
fn flag_is_on(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "on" | "yes" | "enable" | "enabled"
    )
}

/// Whether RAG is enabled via the `BHARATCODE_RAG` environment variable.
/// Defaults to `false` (read-only diagnostic; never mutates anything).
fn rag_enabled() -> bool {
    std::env::var(RAG_KEY)
        .ok()
        .map(|v| flag_is_on(&v))
        .unwrap_or(false)
}

/// Outcome of the bounded readiness walk.
struct ScanOutcome {
    /// Number of scannable (non-hidden, non-ignored) files visited.
    files: usize,
    /// Whether the walk hit the entry ceiling before finishing.
    truncated: bool,
}

/// Lightweight, self-contained `.gitignore` matcher.
///
/// We deliberately avoid pulling in the full `ignore` crate here: a readiness
/// pre-flight only needs to skip the bulk-noise directories people actually put
/// in a `.gitignore` (e.g. `target/`, `node_modules/`, `dist/`). We honour
/// simple directory-name and plain-name patterns and ignore the fancier glob
/// syntax — over-counting a few files is harmless for a readiness figure.
#[derive(Default)]
struct GitignoreRules {
    /// Directory / file names to skip anywhere in the tree.
    names: BTreeSet<String>,
}

impl GitignoreRules {
    /// Parse a `.gitignore` at `root` (if present) into a small name set.
    fn load(root: &Path) -> Self {
        let mut names = BTreeSet::new();
        if let Ok(text) = std::fs::read_to_string(root.join(".gitignore")) {
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
                    continue;
                }
                // Reduce a pattern to its final path segment, stripped of leading
                // anchors and trailing slashes: `/target/` -> `target`,
                // `**/dist` -> `dist`. Glob-bearing leftovers are dropped.
                let trimmed = line.trim_matches('/');
                let segment = trimmed.rsplit('/').next().unwrap_or(trimmed);
                if !segment.is_empty() && !segment.contains(['*', '?', '[']) {
                    names.insert(segment.to_string());
                }
            }
        }
        Self { names }
    }

    /// Whether an entry with this file name should be skipped.
    fn skips(&self, name: &str) -> bool {
        self.names.contains(name)
    }
}

/// Bounded, gitignore-respecting walk of `root`, counting scannable files.
///
/// Hidden entries (names starting with `.`) and entries matched by the loaded
/// `.gitignore` rules are skipped, mirroring the codebase-context scanner's
/// posture. The walk stops once [`MAX_ENTRIES`] entries have been inspected or
/// [`MAX_DEPTH`] is exceeded, so it is always bounded.
fn count_scannable_files(root: &Path, rules: &GitignoreRules) -> ScanOutcome {
    let mut files = 0usize;
    let mut seen = 0usize;
    // Manual stack keeps the walk iterative (no recursion-depth surprises) and
    // lets us enforce the entry ceiling precisely.
    let mut stack: Vec<(std::path::PathBuf, usize)> = vec![(root.to_path_buf(), 0)];

    while let Some((dir, depth)) = stack.pop() {
        if depth > MAX_DEPTH {
            continue;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > MAX_ENTRIES {
                return ScanOutcome {
                    files,
                    truncated: true,
                };
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || rules.skips(&name) {
                continue;
            }
            let file_type = match entry.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if file_type.is_dir() {
                stack.push((entry.path(), depth + 1));
            } else if file_type.is_file() {
                files += 1;
            }
        }
    }

    ScanOutcome {
        files,
        truncated: false,
    }
}

/// Look up a user-facing string through the i18n layer, falling back to the
/// English `default` when the active locale table has no entry for `key`.
///
/// `t()` echoes the key back when it is missing, so an unchanged key is treated
/// as "untranslated". Mirrors the helper in `doctor.rs`/`doctor_checks.rs` so the
/// row renders in English without depending on the i18n table.
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// Report whether `root` is ready for semantic indexing.
///
/// Returns a [`Status`] plus a human-readable message summarising the scannable
/// file count, `.gitignore` presence, and the RAG switch. The result is always
/// non-fatal:
///
/// * [`Status::Ok`] — a reasonable, bounded tree with a `.gitignore`.
/// * [`Status::Warn`] — no `.gitignore` (scans are unbounded by ignore rules) or
///   the tree is large enough that a single index pass is heavy.
///
/// An empty or missing directory yields a sensible non-error status with a
/// `0`-file count (an empty repo is trivially "ready" — there is nothing to
/// over-index), so callers never have to special-case it.
pub fn index_readiness(root: &Path) -> (Status, String) {
    let lbl = label("doctor.check.index_readiness", "RAG / index readiness");

    if !root.is_dir() {
        let msg = label(
            "doctor.check.index_no_dir",
            "no directory to scan; 0 files indexable",
        );
        return (Status::Ok, format!("{} ({})", lbl, msg));
    }

    let has_gitignore = root.join(".gitignore").is_file();
    let rules = GitignoreRules::load(root);
    let outcome = count_scannable_files(root, &rules);

    let rag = if rag_enabled() {
        label("doctor.on", "on")
    } else {
        label("doctor.off", "off")
    };

    // Build the descriptive core: "N files indexable", noting RAG state and the
    // .gitignore situation so the single row is self-explanatory.
    let files_word = label("doctor.check.index_files", "files indexable");
    let rag_label = label("doctor.check.index_rag", "RAG");
    let core = format!("{} {} — {} {}", outcome.files, files_word, rag_label, rag);

    if outcome.truncated || outcome.files > HUGE_FILE_COUNT {
        let hint = label(
            "doctor.check.index_huge",
            "tree is large for a single index pass; add/extend .gitignore to bound the scan",
        );
        return (Status::Warn, format!("{} ({}; {})", lbl, core, hint));
    }

    if !has_gitignore {
        let hint = label(
            "doctor.check.index_no_gitignore",
            "no .gitignore; an index scan is unbounded by ignore rules",
        );
        return (Status::Warn, format!("{} ({}; {})", lbl, core, hint));
    }

    let with_gitignore = label("doctor.check.index_gitignore_ok", ".gitignore present");
    (
        Status::Ok,
        format!("{} ({}; {})", lbl, core, with_gitignore),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn flag_parsing_only_accepts_clear_on_values() {
        assert!(flag_is_on("1"));
        assert!(flag_is_on(" TRUE "));
        assert!(flag_is_on("on"));
        assert!(!flag_is_on("0"));
        assert!(!flag_is_on(""));
        assert!(!flag_is_on("maybe"));
    }

    #[test]
    fn one_file_with_gitignore_is_ok_and_counts() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join(".gitignore"), "target/\n").unwrap();

        let (status, msg) = index_readiness(root);
        assert_eq!(status, Status::Ok, "msg: {msg}");
        // Message must surface the file count.
        assert!(msg.contains('1'), "expected a file count in: {msg}");
        assert!(msg.contains("indexable"), "msg: {msg}");
    }

    #[test]
    fn empty_dir_is_non_error_with_zero_count() {
        let dir = TempDir::new().unwrap();
        let (status, msg) = index_readiness(dir.path());
        // Empty + no .gitignore => Warn (no ignore rules), never an error.
        assert_ne!(status, Status::Fail, "msg: {msg}");
        assert!(msg.contains('0'), "expected a 0 count in: {msg}");
    }

    #[test]
    fn missing_dir_is_ok_with_zero() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("does-not-exist");
        let (status, msg) = index_readiness(&missing);
        assert_eq!(status, Status::Ok, "msg: {msg}");
        assert!(msg.contains('0'), "msg: {msg}");
    }

    #[test]
    fn gitignored_and_hidden_entries_are_skipped() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::write(root.join(".gitignore"), "target/\n").unwrap();
        fs::write(root.join("keep.rs"), "x").unwrap();

        // A gitignored directory full of files must not inflate the count.
        fs::create_dir_all(root.join("target")).unwrap();
        fs::write(root.join("target/a.rs"), "x").unwrap();
        fs::write(root.join("target/b.rs"), "x").unwrap();

        // Hidden directory contents are skipped too.
        fs::create_dir_all(root.join(".cache")).unwrap();
        fs::write(root.join(".cache/c.rs"), "x").unwrap();

        let rules = GitignoreRules::load(root);
        let outcome = count_scannable_files(root, &rules);
        // Only keep.rs counts (.gitignore itself is hidden and skipped).
        assert_eq!(outcome.files, 1, "counted more than the one real file");
    }

    #[test]
    fn nested_files_are_counted_within_bounds() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::write(root.join(".gitignore"), "node_modules/\n").unwrap();
        fs::create_dir_all(root.join("src/inner")).unwrap();
        fs::write(root.join("src/a.rs"), "x").unwrap();
        fs::write(root.join("src/inner/b.rs"), "x").unwrap();

        let (status, msg) = index_readiness(root);
        assert_eq!(status, Status::Ok, "msg: {msg}");
        assert!(msg.contains('2'), "expected 2 files in: {msg}");
    }
}
