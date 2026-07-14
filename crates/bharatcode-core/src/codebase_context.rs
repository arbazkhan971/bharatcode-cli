//! Opt-in lightweight codebase context (RAG-lite) — BharatCode v26.
//!
//! A single, bounded, *fast* repository scan that produces a compact text blob
//! summarising a project so the agent can be seeded with high-level context
//! (the kind of orientation a human gets from glancing at the repo root):
//!
//!   1. **Top-level layout** — the directories directly under the repo root.
//!   2. **Manifests** — build/package manifests (`Cargo.toml`, `package.json`,
//!      `pyproject.toml`, `go.mod`, …) found near the root.
//!   3. **README excerpt** — the first few kilobytes of the root README.
//!
//! The scan is deliberately conservative and side-effect free:
//!
//!   * It **respects `.gitignore`** (and git excludes / hidden files) via the
//!     [`ignore`] walker, so build artefacts and vendored trees are skipped.
//!   * It is **bounded** by [`ScanLimits`]: a maximum walk depth, a cap on the
//!     number of entries visited, caps on how many directories / manifests are
//!     listed, a per-README byte cap, and a final cap on the whole blob. A
//!     pathological repo can never blow up memory or wall-clock time here.
//!   * It only ever **reads** files; it never writes, mutates config, or shells
//!     out.
//!
//! The whole feature is **opt-in and defaults to off**. It is gated on the
//! `BHARATCODE_CODEBASE_CONTEXT` switch (environment variable first, then the
//! on-disk config as a boolean — mirroring the other BharatCode switches). When
//! the switch is off, [`maybe_codebase_context`] returns `None` and nothing is
//! scanned, so default behaviour is completely unchanged.
//!
//! This module is intentionally self-contained: it exposes a pure
//! [`scan_codebase`] function plus a thin config-gated [`maybe_codebase_context`]
//! helper that callers (e.g. session/agent setup) can opt into without this
//! module reaching into any agent-side state.

use std::collections::BTreeSet;
use std::path::Path;

use ignore::WalkBuilder;

use crate::config::Config;

/// Config / environment key for the codebase-context switch. Defaults to off.
pub const CODEBASE_CONTEXT_KEY: &str = "BHARATCODE_CODEBASE_CONTEXT";

/// Manifest / project-marker file names recognised by the scanner.
///
/// Matched case-insensitively against the file name only. Kept small and
/// curated so the listing stays signal-dense across common ecosystems.
const MANIFEST_NAMES: &[&str] = &[
    "cargo.toml",
    "package.json",
    "pnpm-workspace.yaml",
    "deno.json",
    "deno.jsonc",
    "tsconfig.json",
    "pyproject.toml",
    "setup.py",
    "setup.cfg",
    "requirements.txt",
    "pipfile",
    "go.mod",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
    "gemfile",
    "composer.json",
    "build.sbt",
    "cmakelists.txt",
    "makefile",
    "dockerfile",
    "pubspec.yaml",
    "mix.exs",
    "package.swift",
];

/// Bounds applied to a single scan. Every field has a conservative default via
/// [`ScanLimits::default`]; construct directly to tune (mainly for tests).
#[derive(Debug, Clone, Copy)]
pub struct ScanLimits {
    /// Maximum directory depth to descend (the repo root is depth 0, so a value
    /// of `2` visits the root, its children, and its grandchildren).
    pub max_depth: usize,
    /// Hard ceiling on the number of filesystem entries inspected, so a huge or
    /// pathological tree can never make the scan run unbounded.
    pub max_entries: usize,
    /// Maximum number of top-level directories listed.
    pub max_top_dirs: usize,
    /// Maximum number of manifest files listed.
    pub max_manifests: usize,
    /// Maximum number of bytes read from the README excerpt.
    pub readme_bytes: usize,
    /// Final cap on the size of the whole produced context blob.
    pub max_total_bytes: usize,
}

impl Default for ScanLimits {
    fn default() -> Self {
        Self {
            max_depth: 2,
            max_entries: 5_000,
            max_top_dirs: 40,
            max_manifests: 40,
            readme_bytes: 2_048,
            max_total_bytes: 8_192,
        }
    }
}

/// Interpret a raw flag value as truthy/falsy. Returns `None` for anything that
/// is neither clearly on nor off, so a typo never silently flips the switch.
fn parse_flag(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "on" | "yes" | "enable" | "enabled" => Some(true),
        "0" | "false" | "off" | "no" | "disable" | "disabled" | "" => Some(false),
        _ => None,
    }
}

/// Resolve the switch from an explicit environment value (if any) and a config
/// fallback. Factored out so the precedence logic can be unit-tested without
/// touching real process environment or global config.
fn enabled_from(env_value: Option<&str>, config_fallback: impl FnOnce() -> Option<bool>) -> bool {
    if let Some(raw) = env_value {
        if let Some(flag) = parse_flag(raw) {
            return flag;
        }
    }
    config_fallback().unwrap_or(false)
}

/// Returns true when codebase context is enabled. Defaults to `false`.
///
/// The environment variable `BHARATCODE_CODEBASE_CONTEXT` takes precedence;
/// when it is unset (or an unrecognised value) the on-disk config is read as a
/// boolean. Any ambiguity resolves to "off" so default behaviour never changes
/// by accident.
pub fn is_enabled() -> bool {
    let env_value = std::env::var(CODEBASE_CONTEXT_KEY).ok();
    enabled_from(env_value.as_deref(), || {
        Config::global()
            .get_param::<bool>(CODEBASE_CONTEXT_KEY)
            .ok()
    })
}

/// Opt-in entry point: returns a compact codebase-context blob for `root` when
/// the feature is enabled *and* the scan found something worth seeding, else
/// `None`. This is the function other modules should call.
pub fn maybe_codebase_context(root: impl AsRef<Path>) -> Option<String> {
    if !is_enabled() {
        return None;
    }
    let context = scan_codebase(root);
    if context.is_empty() {
        None
    } else {
        Some(context)
    }
}

/// Pure, bounded scan of `root` using the default [`ScanLimits`]. Returns the
/// compact context blob (empty string when nothing useful was found). This is
/// independent of the config gate so it can be reused and tested directly.
pub fn scan_codebase(root: impl AsRef<Path>) -> String {
    scan_codebase_with(root.as_ref(), &ScanLimits::default())
}

/// Like [`scan_codebase`] but with caller-supplied [`ScanLimits`].
pub fn scan_codebase_with(root: &Path, limits: &ScanLimits) -> String {
    if !root.is_dir() {
        return String::new();
    }

    // Sorted + de-duplicated so the listing is stable and noise-free.
    let mut top_dirs: BTreeSet<String> = BTreeSet::new();
    let mut manifests: BTreeSet<String> = BTreeSet::new();
    let mut readme_path: Option<std::path::PathBuf> = None;

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
        let depth = rel.components().count();
        let is_dir = entry.file_type().is_some_and(|t| t.is_dir());

        // Top-level directories only.
        if is_dir {
            if depth == 1 {
                if let Some(name) = rel.file_name().and_then(|n| n.to_str()) {
                    top_dirs.insert(format!("{name}/"));
                }
            }
            continue;
        }

        let file_name = match rel.file_name().and_then(|n| n.to_str()) {
            Some(name) => name,
            None => continue,
        };
        let lower = file_name.to_ascii_lowercase();

        // README at the repo root wins; prefer the shallowest one found.
        if lower.starts_with("readme") && depth == 1 && readme_path.is_none() {
            readme_path = Some(path.to_path_buf());
        }

        if MANIFEST_NAMES.contains(&lower.as_str()) {
            if let Some(rel_str) = rel.to_str() {
                manifests.insert(rel_str.replace('\\', "/"));
            }
        }
    }

    render(&top_dirs, &manifests, readme_path.as_deref(), limits)
}

/// Assemble the final compact blob from the collected pieces and apply the
/// overall size cap. Returns an empty string when there is nothing to report.
fn render(
    top_dirs: &BTreeSet<String>,
    manifests: &BTreeSet<String>,
    readme_path: Option<&Path>,
    limits: &ScanLimits,
) -> String {
    let readme_excerpt = readme_path.and_then(|p| read_excerpt(p, limits.readme_bytes));

    if top_dirs.is_empty() && manifests.is_empty() && readme_excerpt.is_none() {
        return String::new();
    }

    let mut out = String::new();
    out.push_str("# Project context\n");

    if !top_dirs.is_empty() {
        out.push_str("\n## Top-level layout\n");
        for dir in top_dirs.iter().take(limits.max_top_dirs) {
            out.push_str("- ");
            out.push_str(dir);
            out.push('\n');
        }
    }

    if !manifests.is_empty() {
        out.push_str("\n## Manifests\n");
        for manifest in manifests.iter().take(limits.max_manifests) {
            out.push_str("- ");
            out.push_str(manifest);
            out.push('\n');
        }
    }

    if let Some(excerpt) = readme_excerpt {
        out.push_str("\n## README (excerpt)\n");
        out.push_str(excerpt.trim_end());
        out.push('\n');
    }

    truncate_on_char_boundary(&mut out, limits.max_total_bytes);
    out
}

/// Read at most `max_bytes` from `path` as lossy UTF-8, trimmed. Returns `None`
/// when the file cannot be read or is effectively empty.
fn read_excerpt(path: &Path, max_bytes: usize) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let slice = &bytes[..bytes.len().min(max_bytes)];
    let text = String::from_utf8_lossy(slice);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Truncate `s` in place to at most `max_bytes`, never splitting a UTF-8 code
/// point.
fn truncate_on_char_boundary(s: &mut String, max_bytes: usize) {
    if s.len() <= max_bytes {
        return;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn parse_flag_recognises_on_off_and_ambiguous() {
        assert_eq!(parse_flag("1"), Some(true));
        assert_eq!(parse_flag("TRUE"), Some(true));
        assert_eq!(parse_flag(" on "), Some(true));
        assert_eq!(parse_flag("0"), Some(false));
        assert_eq!(parse_flag(""), Some(false));
        assert_eq!(parse_flag("maybe"), None);
    }

    #[test]
    fn gate_defaults_off_and_respects_precedence() {
        // No env, no config => off (default behaviour unchanged).
        assert!(!enabled_from(None, || None));
        // Env wins over config.
        assert!(enabled_from(Some("1"), || Some(false)));
        assert!(!enabled_from(Some("off"), || Some(true)));
        // Unrecognised env falls back to config.
        assert!(enabled_from(Some("garbage"), || Some(true)));
        assert!(!enabled_from(Some("garbage"), || Some(false)));
        // Config-only path.
        assert!(enabled_from(None, || Some(true)));
    }

    #[test]
    fn scan_of_missing_or_file_root_is_empty() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("does-not-exist");
        assert!(scan_codebase(&missing).is_empty());

        let file = dir.path().join("a.txt");
        fs::write(&file, "hello").unwrap();
        assert!(scan_codebase(&file).is_empty());
    }

    #[test]
    fn scan_collects_layout_manifests_and_readme() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        fs::write(
            root.join("README.md"),
            "# Demo Project\n\nA small example used by the scanner test.\n",
        )
        .unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();

        let out = scan_codebase(root);

        assert!(out.contains("# Project context"), "got: {out}");
        assert!(out.contains("- src/"), "got: {out}");
        assert!(out.contains("- docs/"), "got: {out}");
        assert!(out.contains("Cargo.toml"), "got: {out}");
        assert!(out.contains("Demo Project"), "got: {out}");
    }

    #[test]
    fn scan_respects_gitignore_and_hidden() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        fs::write(root.join(".gitignore"), "ignored_dir/\n").unwrap();
        fs::create_dir_all(root.join("ignored_dir")).unwrap();
        fs::write(root.join("ignored_dir/Cargo.toml"), "x").unwrap();
        fs::create_dir_all(root.join(".hidden_dir")).unwrap();
        fs::create_dir_all(root.join("visible")).unwrap();

        let out = scan_codebase(root);
        assert!(out.contains("- visible/"), "got: {out}");
        assert!(!out.contains("ignored_dir"), "got: {out}");
        assert!(!out.contains(".hidden_dir"), "got: {out}");
    }

    #[test]
    fn readme_excerpt_is_byte_capped_on_char_boundary() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        // Multi-byte content to exercise the char-boundary truncation.
        let big = "नमस्ते ".repeat(2_000);
        fs::write(root.join("README.md"), &big).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();

        let limits = ScanLimits {
            readme_bytes: 64,
            ..ScanLimits::default()
        };
        let out = scan_codebase_with(root, &limits);
        // Must be valid UTF-8 (String guarantees it) and bounded overall.
        assert!(out.len() <= ScanLimits::default().max_total_bytes);
        assert!(out.contains("README (excerpt)"));
    }

    #[test]
    fn total_blob_is_capped() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::write(
            root.join("README.md"),
            "# Title\n".to_string() + "x".repeat(50_000).as_str(),
        )
        .unwrap();
        for i in 0..50 {
            fs::create_dir_all(root.join(format!("dir_{i:03}"))).unwrap();
        }
        let out = scan_codebase(root);
        assert!(
            out.len() <= ScanLimits::default().max_total_bytes,
            "len={}",
            out.len()
        );
    }
}
