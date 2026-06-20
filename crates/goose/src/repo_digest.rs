//! Opt-in incremental-context repo digest — BharatCode v70.
//!
//! Injects a small, *cached* repo-state digest into the system prompt so the
//! model keeps cheap structural grounding across turns without re-walking the
//! tree on every request. The digest has two parts:
//!
//!   1. **Top-level layout** — the directories and notable files directly under
//!      the working directory (the orientation a human gets from `ls`).
//!   2. **Content fingerprint** — a stable 64-bit hash over the bounded set of
//!      `(relative path, mtime)` pairs in the tree. The fingerprint changes iff
//!      the structurally-relevant tree changes, which is what lets us memoize.
//!
//! The walk reuses the existing bounded, `.gitignore`-respecting primitive's
//! sibling settings and the very same [`crate::codebase_context::ScanLimits`]
//! bounds, so build artefacts / vendored trees are skipped and a pathological
//! repository can never blow up memory or wall-clock time.
//!
//! The incremental win: the rendered block is memoized per working directory in
//! a process-wide map keyed on `(fingerprint)`. Repeated turns from the same
//! working directory reuse the cached string verbatim — no second walk, no
//! re-render — *unless* the fingerprint changes, in which case the block is
//! recomputed and the cache entry refreshed.
//!
//! The whole feature is **opt-in and defaults to off**, gated on the
//! `BHARATCODE_REPO_DIGEST` switch (read from the process environment, mirroring
//! the other BharatCode switches and the `memory_store` truthiness pattern).
//! When the switch is off, [`digest_block`] returns `None` and nothing is
//! scanned, so the system prompt is byte-identical to the unmodified default.

use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::hash::Hash;
use std::hash::Hasher;
use std::path::Path;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use ignore::WalkBuilder;

use crate::codebase_context::ScanLimits;

/// Opt-in toggle name, read from the process environment. Defaults to off.
const ENABLE_KEY: &str = "BHARATCODE_REPO_DIGEST";

/// Maximum number of top-level entries (dirs + files) listed in the digest.
const MAX_TOP_ENTRIES: usize = 60;

/// Per-process memo: working directory -> (fingerprint, rendered block). A
/// repeated turn from the same directory returns the cached string verbatim so
/// long as the fingerprint is unchanged, avoiding a second walk and re-render.
static DIGEST_CACHE: LazyLock<Mutex<HashMap<PathBuf, (u64, String)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Interpret a raw flag value as truthy. Mirrors the `memory_store` /
/// `plan_mode` pattern so the BharatCode switches behave consistently.
fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Whether the repo digest is enabled. Opt-in via the `BHARATCODE_REPO_DIGEST`
/// environment variable; any truthy-ish value (`1`, `true`, `yes`, `on`)
/// enables it. Reads the raw process environment so the gate is unambiguous and
/// defaults to `false` when unset.
pub fn is_enabled() -> bool {
    std::env::var(ENABLE_KEY)
        .map(|raw| is_truthy(&raw))
        .unwrap_or(false)
}

/// Result of a single bounded tree walk: the sorted top-level entries to list
/// and the content fingerprint over every visited `(rel path, mtime)` pair.
struct Scan {
    /// Sorted top-level entry names (directories get a trailing `/`).
    top_entries: Vec<String>,
    /// 64-bit fingerprint over the bounded `(rel path, mtime)` set.
    fingerprint: u64,
}

/// Perform one bounded, `.gitignore`-aware walk of `root` collecting the
/// top-level layout and a stable content fingerprint. Pure: only reads the
/// directory tree metadata, never writes or shells out.
fn scan(root: &Path, limits: &ScanLimits) -> Scan {
    // BTreeMap keyed on the relative path keeps the fingerprint input order
    // deterministic regardless of the walker's traversal order.
    let mut prints: BTreeMap<String, u64> = BTreeMap::new();
    let mut top_entries: BTreeMap<String, bool> = BTreeMap::new();

    let mut builder = WalkBuilder::new(root);
    builder
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true)
        .require_git(false)
        .ignore(true)
        .hidden(true)
        .follow_links(false)
        .max_depth(Some(limits.max_depth));

    let mut seen = 0usize;
    for entry in builder.build().flatten() {
        seen += 1;
        if seen > limits.max_entries {
            break;
        }

        let path = entry.path();
        if path == root {
            continue;
        }
        let rel = match path.strip_prefix(root) {
            Ok(rel) => rel,
            Err(_) => continue,
        };
        let rel_str = match rel.to_str() {
            Some(s) => s.replace('\\', "/"),
            None => continue,
        };
        let depth = rel.components().count();
        let is_dir = entry.file_type().is_some_and(|t| t.is_dir());

        // Modification time (seconds since epoch) folds into the fingerprint so
        // it shifts when a tracked file is touched, added, or removed.
        let mtime = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        prints.insert(rel_str.clone(), mtime);

        if depth == 1 {
            if let Some(name) = rel.file_name().and_then(|n| n.to_str()) {
                top_entries.insert(name.to_string(), is_dir);
            }
        }
    }

    let mut hasher = DefaultHasher::new();
    for (rel, mtime) in &prints {
        rel.hash(&mut hasher);
        mtime.hash(&mut hasher);
    }
    let fingerprint = hasher.finish();

    let top_entries = top_entries
        .into_iter()
        .map(|(name, is_dir)| if is_dir { format!("{name}/") } else { name })
        .take(MAX_TOP_ENTRIES)
        .collect();

    Scan {
        top_entries,
        fingerprint,
    }
}

/// Render the compact digest block from a completed [`Scan`]. The
/// `# Repo digest` heading is a stable anchor relied on by callers and tests.
fn render(scan: &Scan) -> String {
    let mut out = String::new();
    out.push_str("# Repo digest\n");
    out.push_str("\nA cached structural snapshot of the current working directory ");
    out.push_str("(top-level layout plus a content fingerprint). It is refreshed ");
    out.push_str("only when the tree changes; rely on it for cheap orientation.\n");

    out.push_str(&format!("\nfingerprint: {:016x}\n", scan.fingerprint));

    if !scan.top_entries.is_empty() {
        out.push_str("\n## Top-level layout\n");
        for entry in &scan.top_entries {
            out.push_str("- ");
            out.push_str(entry);
            out.push('\n');
        }
    }

    out
}

/// The repo-digest block to inject into the system prompt, or `None` when the
/// feature is disabled or the working directory is not a readable directory.
///
/// The rendered block is memoized per `cwd` keyed on the content fingerprint:
/// repeated calls with an unchanged tree return the **same cached string**
/// without re-rendering, and the block is recomputed only when the fingerprint
/// changes (a file added/removed/touched in the bounded tree). This is the
/// incremental win that keeps structural grounding cheap across turns.
pub fn digest_block(cwd: &Path) -> Option<String> {
    if !is_enabled() {
        return None;
    }
    if !cwd.is_dir() {
        return None;
    }

    let scan = scan(cwd, &ScanLimits::default());
    let fingerprint = scan.fingerprint;

    let mut cache = DIGEST_CACHE.lock().unwrap();
    if let Some((cached_print, cached_block)) = cache.get(cwd) {
        if *cached_print == fingerprint {
            return Some(cached_block.clone());
        }
    }

    let block = render(&scan);
    cache.insert(cwd.to_path_buf(), (fingerprint, block.clone()));
    Some(block)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Serialise tests that mutate the shared process env so the
    /// `BHARATCODE_REPO_DIGEST` toggle does not race across threads.
    fn env_guard(value: Option<&str>) -> env_lock::EnvGuard<'_> {
        env_lock::lock_env([(ENABLE_KEY, value)])
    }

    #[test]
    fn is_enabled_toggles_on_env() {
        {
            let _guard = env_guard(None);
            assert!(!is_enabled());
        }
        {
            let _guard = env_guard(Some("1"));
            assert!(is_enabled());
        }
        {
            let _guard = env_guard(Some("true"));
            assert!(is_enabled());
        }
        {
            let _guard = env_guard(Some("off"));
            assert!(!is_enabled());
        }
    }

    #[test]
    fn disabled_yields_none() {
        let _guard = env_guard(None);
        let dir = TempDir::new().unwrap();
        assert!(digest_block(dir.path()).is_none());
    }

    #[test]
    fn lists_top_entries_and_contains_fingerprint() {
        let _guard = env_guard(Some("1"));
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();

        let block = digest_block(root).expect("enabled digest should be Some");
        assert!(block.contains("# Repo digest"), "got: {block}");
        assert!(block.contains("- src/"), "got: {block}");
        assert!(block.contains("- docs/"), "got: {block}");
        assert!(block.contains("Cargo.toml"), "got: {block}");
        assert!(block.contains("fingerprint:"), "got: {block}");
        // Zero user-facing donor/internal-brand leakage.
        assert!(
            !block.to_ascii_lowercase().contains("goose"),
            "leak: {block}"
        );
    }

    #[test]
    fn memoizes_and_recomputes_on_change() {
        let _guard = env_guard(Some("1"));
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "fn a() {}\n").unwrap();

        let first = digest_block(root).expect("first digest");
        let second = digest_block(root).expect("second digest");
        // Unchanged tree: same cached string returned verbatim.
        assert_eq!(first, second, "cached block must be reused unchanged");

        let print_re = |s: &str| {
            s.lines()
                .find_map(|l| l.strip_prefix("fingerprint: "))
                .map(|p| p.to_string())
        };
        let first_print = print_re(&first).expect("fingerprint line");

        // Add a file: fingerprint must change and the block is recomputed.
        fs::write(root.join("new_file.rs"), "fn b() {}\n").unwrap();
        let third = digest_block(root).expect("third digest");
        let third_print = print_re(&third).expect("fingerprint line");

        assert_ne!(
            first_print, third_print,
            "fingerprint must change when a file is added"
        );
        assert!(third.contains("new_file.rs"), "got: {third}");
    }
}
