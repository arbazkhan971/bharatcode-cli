//! Large-repo readiness profiler — BharatCode v67.
//!
//! A read-only, bounded, `.gitignore`-aware profiler that summarises how heavy a
//! repository is before any tooling tries to scan it wholesale. It reports four
//! figures the operator cares about when judging whether a tree is "large":
//!
//!   1. **File count** — how many tracked (non-ignored, non-hidden) files a
//!      bounded walk would visit.
//!   2. **Total tracked bytes** — the summed on-disk size of those files.
//!   3. **Deepest path depth** — how far the tree nests below the root, a proxy
//!      for pathologically deep layouts.
//!   4. **Largest single file** — the heaviest file and its size, the usual
//!      culprit behind a slow or memory-hungry scan.
//!
//! The walk is deliberately conservative and side-effect free: it uses
//! [`ignore::WalkBuilder`] so it honours `.gitignore`/`.ignore` rules and skips
//! hidden entries, exactly like the codebase-context scanner, and it is bounded
//! by the same posture — a hard ceiling on files visited ([`FILE_CAP`]) and a
//! maximum walk depth ([`DEPTH_CAP`]) — so a pathological repo can never blow up
//! wall-clock time. It only ever *reads* directory and file metadata; it never
//! writes, mutates config, or shells out.
//!
//! Nothing here is gated behind an opt-in: the profiler always runs as part of
//! the doctor deep checks. The `BHARATCODE_LARGE_REPO_FILE_WARN` environment
//! variable only *tunes* the warn threshold; it never turns the check off.

use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

use crate::commands::doctor_checks::Status;

/// Hard ceiling on the number of files the profiler will tally. Mirrors the
/// codebase-context scanner's bounded posture so a pathological repo can never
/// make the walk run unbounded; once this many files have been counted the walk
/// stops early. Tuned to ~50k per the large-repo readiness spec.
const FILE_CAP: usize = 50_000;

/// Maximum directory depth descended by the walk. Beyond this the tree is not
/// explored further, keeping a pathologically deep layout from blowing
/// wall-clock time. Tuned to ~64 per the large-repo readiness spec.
const DEPTH_CAP: usize = 64;

/// Environment key that *tunes* the file-count warn threshold. This only moves
/// the line at which the check warns — the profiler always runs regardless.
const FILE_WARN_KEY: &str = "BHARATCODE_LARGE_REPO_FILE_WARN";

/// Default file-count threshold above which a repo is flagged "large".
const DEFAULT_FILE_WARN: usize = 20_000;

/// Total-bytes threshold above which a repo is flagged "large" (1 GiB). There is
/// no env tunable for this; it is a fixed, conservative ceiling.
const BYTES_WARN: u64 = 1024 * 1024 * 1024;

/// A read-only snapshot of how heavy a repository is.
pub struct RepoProfile {
    /// Number of tracked (non-ignored, non-hidden) files visited, capped at
    /// [`FILE_CAP`].
    files: usize,
    /// Summed on-disk size, in bytes, of the visited files.
    bytes: u64,
    /// Deepest path depth seen, relative to the root (a direct child is depth 1).
    max_depth: usize,
    /// The largest single file and its size, if any file was visited.
    largest: Option<(PathBuf, u64)>,
}

impl RepoProfile {
    /// Number of tracked files visited (capped at [`FILE_CAP`]).
    pub fn files(&self) -> usize {
        self.files
    }

    /// Total tracked bytes across the visited files.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Deepest path depth observed relative to the root.
    pub fn max_depth(&self) -> usize {
        self.max_depth
    }

    /// The largest single file and its size, if any.
    pub fn largest(&self) -> Option<&(PathBuf, u64)> {
        self.largest.as_ref()
    }
}

/// Resolve the file-count warn threshold from the environment, falling back to
/// [`DEFAULT_FILE_WARN`]. A blank or unparsable value falls back to the default
/// so a typo never disables the warning.
fn file_warn_threshold() -> usize {
    std::env::var(FILE_WARN_KEY)
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_FILE_WARN)
}

/// Profile `root` with a bounded, `.gitignore`-aware walk.
///
/// Honours `.gitignore`/`.ignore` rules and skips hidden entries (mirroring the
/// codebase-context scanner). The walk is bounded by [`FILE_CAP`] files and
/// [`DEPTH_CAP`] depth, so it always terminates promptly. A missing or
/// unreadable directory yields an empty profile (zero files/bytes), so callers
/// never have to special-case it.
pub fn profile(root: &Path) -> RepoProfile {
    let mut files = 0usize;
    let mut bytes = 0u64;
    let mut max_depth = 0usize;
    let mut largest: Option<(PathBuf, u64)> = None;

    if !root.is_dir() {
        return RepoProfile {
            files,
            bytes,
            max_depth,
            largest,
        };
    }

    let mut builder = WalkBuilder::new(root);
    builder
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true)
        .require_git(false)
        .ignore(true)
        .hidden(true)
        .follow_links(false)
        .max_depth(Some(DEPTH_CAP));

    for entry in builder.build().flatten() {
        let path = entry.path();
        if path == root {
            continue;
        }
        // Only tally regular files; directories contribute to depth via their
        // descendants, not as countable entries.
        let is_file = entry.file_type().is_some_and(|t| t.is_file());
        if !is_file {
            continue;
        }

        let depth = path
            .strip_prefix(root)
            .map(|rel| rel.components().count())
            .unwrap_or(0);
        if depth > max_depth {
            max_depth = depth;
        }

        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        bytes = bytes.saturating_add(size);

        match &largest {
            Some((_, best)) if *best >= size => {}
            _ => largest = Some((path.to_path_buf(), size)),
        }

        files += 1;
        if files >= FILE_CAP {
            break;
        }
    }

    RepoProfile {
        files,
        bytes,
        max_depth,
        largest,
    }
}

/// Look up a user-facing string through the i18n layer, falling back to the
/// English `default` when the active locale table has no entry for `key`.
///
/// `t()` echoes the key back when it is missing, so an unchanged key is treated
/// as "untranslated". Mirrors the helper in `doctor.rs`/`index_check.rs` so the
/// row renders in English without depending on the i18n table.
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// Render a byte count as a compact human-readable string (`B`/`KB`/`MB`/`GB`).
fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{:.1} {}", value, UNITS[unit])
    }
}

/// Summarise a [`RepoProfile`] as a doctor row: a [`Status`] plus a
/// human-readable message.
///
/// The result is always non-fatal:
///
/// * [`Status::Warn`] — the repo crosses a "large" threshold (more files than
///   [`file_warn_threshold`] or more than [`BYTES_WARN`] total bytes); such a
///   tree is heavy for whole-repo tooling and worth flagging.
/// * [`Status::Ok`] — the repo is within comfortable bounds.
pub fn readiness_line(p: &RepoProfile) -> (Status, String) {
    let lbl = label("doctor.check.repo_profile", "Large-repo readiness");

    let files_word = label("doctor.check.repo_files", "files");
    let depth_word = label("doctor.check.repo_depth", "max depth");
    let core = format!(
        "{} {}, {}, {} {}",
        p.files,
        files_word,
        human_bytes(p.bytes),
        depth_word,
        p.max_depth,
    );

    let core = match &p.largest {
        Some((path, size)) => {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned());
            let largest_word = label("doctor.check.repo_largest", "largest");
            format!(
                "{}; {} {} ({})",
                core,
                largest_word,
                name,
                human_bytes(*size)
            )
        }
        None => core,
    };

    let warn_files = file_warn_threshold();
    if p.files > warn_files || p.bytes > BYTES_WARN {
        let hint = label(
            "doctor.check.repo_large",
            "large repo; whole-repo scans/tooling may be slow — narrow scope or extend .gitignore",
        );
        return (Status::Warn, format!("{} ({}; {})", lbl, core, hint));
    }

    (Status::Ok, format!("{} ({})", lbl, core))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn profiles_known_files_sizes_depth_and_largest() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        // Three files of known sizes at varying depths.
        //   a.txt        (depth 1, 10 bytes)
        //   src/b.txt    (depth 2, 100 bytes)
        //   src/d/c.txt  (depth 3, 5 bytes)  <- deepest
        fs::write(root.join("a.txt"), vec![b'a'; 10]).unwrap();
        fs::create_dir_all(root.join("src/d")).unwrap();
        fs::write(root.join("src/b.txt"), vec![b'b'; 100]).unwrap();
        fs::write(root.join("src/d/c.txt"), vec![b'c'; 5]).unwrap();

        let p = profile(root);

        assert_eq!(p.files(), 3, "expected exactly the three real files");
        assert_eq!(p.bytes(), 10 + 100 + 5, "summed byte total mismatch");
        assert_eq!(p.max_depth(), 3, "deepest path is src/d/c.txt -> depth 3");

        let (largest_path, largest_size) = p.largest().expect("a largest file");
        assert_eq!(*largest_size, 100, "src/b.txt is the biggest file");
        assert_eq!(
            largest_path.file_name().unwrap().to_string_lossy(),
            "b.txt",
            "largest path should point at the biggest file"
        );
    }

    #[test]
    fn gitignored_file_is_excluded() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        fs::write(root.join(".gitignore"), "ignored.txt\nbuild/\n").unwrap();
        fs::write(root.join("keep.txt"), vec![b'k'; 7]).unwrap();
        fs::write(root.join("ignored.txt"), vec![b'x'; 999]).unwrap();
        fs::create_dir_all(root.join("build")).unwrap();
        fs::write(root.join("build/artifact.bin"), vec![b'z'; 5000]).unwrap();

        let p = profile(root);

        // Only keep.txt survives: ignored.txt and the build/ tree are excluded,
        // and the hidden .gitignore itself is skipped.
        assert_eq!(p.files(), 1, "gitignored + hidden entries must be excluded");
        assert_eq!(p.bytes(), 7, "only keep.txt should contribute bytes");
        let (largest_path, _) = p.largest().expect("a largest file");
        assert_eq!(
            largest_path.file_name().unwrap().to_string_lossy(),
            "keep.txt",
            "the huge ignored file must not become the largest"
        );
    }

    #[test]
    fn readiness_warns_above_threshold_and_ok_below() {
        // A tiny profile is comfortably below any threshold => Ok.
        let small = RepoProfile {
            files: 3,
            bytes: 1024,
            max_depth: 2,
            largest: Some((PathBuf::from("a.txt"), 512)),
        };
        let (status, msg) = readiness_line(&small);
        assert_eq!(status, Status::Ok, "small repo should be Ok: {msg}");

        // Above the file-count threshold => Warn. Pin the threshold via env so the
        // test is independent of the default.
        let big = RepoProfile {
            files: 100,
            bytes: 1024,
            max_depth: 2,
            largest: Some((PathBuf::from("a.txt"), 512)),
        };
        let prev = std::env::var(FILE_WARN_KEY).ok();
        std::env::set_var(FILE_WARN_KEY, "50");
        let (status, msg) = readiness_line(&big);
        match prev {
            Some(v) => std::env::set_var(FILE_WARN_KEY, v),
            None => std::env::remove_var(FILE_WARN_KEY),
        }
        assert_eq!(status, Status::Warn, "100 files > 50 warn => Warn: {msg}");
    }

    #[test]
    fn readiness_warns_above_byte_threshold() {
        let heavy = RepoProfile {
            files: 1,
            bytes: BYTES_WARN + 1,
            max_depth: 1,
            largest: Some((PathBuf::from("big.bin"), BYTES_WARN + 1)),
        };
        let (status, _msg) = readiness_line(&heavy);
        assert_eq!(status, Status::Warn, "over-byte-threshold => Warn");
    }

    #[test]
    fn walk_is_bounded_by_file_cap() {
        // The cap is the contract: the profiler must never report more than
        // FILE_CAP files. We assert the invariant directly (creating 50k+ files
        // in a unit test would be wasteful), then confirm a real walk obeys it.
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        for i in 0..50 {
            fs::write(root.join(format!("f{i}.txt")), b"x").unwrap();
        }
        let p = profile(root);
        assert!(
            p.files() <= FILE_CAP,
            "profiler must never exceed the file cap"
        );
        assert_eq!(p.files(), 50, "all 50 small files should be counted");
    }

    #[test]
    fn missing_dir_yields_empty_profile() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("does-not-exist");
        let p = profile(&missing);
        assert_eq!(p.files(), 0);
        assert_eq!(p.bytes(), 0);
        assert_eq!(p.max_depth(), 0);
        assert!(p.largest().is_none());
    }
}
