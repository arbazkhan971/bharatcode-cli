//! Opt-in lexical codebase retriever (RAG-lite) — BharatCode v43.
//!
//! On each provider turn the agent can prepend a compact `# Relevant files`
//! block to the system prompt so the model is pointed at the handful of repo
//! files most likely related to the latest user request. The ranking is a
//! deliberately simple, *lexical* signal — token-set overlap between the
//! user's query and a small per-file token index (path components plus the
//! first few lines of each file), with a small bonus when a query token is a
//! substring of the file path. There is no embedding model, no network, and no
//! persistent state: it is a pure, bounded, side-effect-free scan.
//!
//! The index reuses the same scanning discipline as
//! [`crate::codebase_context`]: a `.gitignore`-respecting walk via the
//! [`ignore`] crate, bounded by [`ScanLimits`] so a pathological repository can
//! never blow up memory or wall-clock time. Files are only ever **read**.
//!
//! The whole feature is **opt-in and defaults to off**, gated on the raw
//! `BHARATCODE_RAG` environment variable. When the switch is off,
//! [`retrieval_block`] returns `None` and nothing is walked, so default
//! behaviour is completely unchanged.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

use crate::codebase_context::ScanLimits;

/// Environment key for the retriever switch. Defaults to off.
pub const RAG_KEY: &str = "BHARATCODE_RAG";

/// Maximum number of files listed in a `# Relevant files` block.
const DEFAULT_TOP_K: usize = 8;

/// Number of leading lines read from each file to seed its token set. Kept
/// small so the walk stays fast and signal-dense (imports / headers / the
/// opening doc-comment are usually the most descriptive part of a file).
const INDEX_HEAD_LINES: usize = 20;

/// Maximum number of bytes read from any single file while indexing, so a huge
/// minified or generated file can never dominate the scan.
const INDEX_HEAD_BYTES: usize = 4_096;

/// Interpret a raw flag value as truthy. Mirrors the other BharatCode switches:
/// only a clearly affirmative value enables the feature; everything else
/// (including unset / unrecognised) leaves it off so default behaviour is never
/// flipped by accident.
fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on" | "enable" | "enabled"
    )
}

/// Returns true when the lexical retriever is enabled. Defaults to `false`.
///
/// Reads the raw `BHARATCODE_RAG` environment variable; any truthy value turns
/// the feature on. Unset or unrecognised resolves to "off".
pub fn is_enabled() -> bool {
    std::env::var(RAG_KEY)
        .ok()
        .map(|raw| is_truthy(&raw))
        .unwrap_or(false)
}

/// A bounded, in-memory lexical index over a repository: for each indexed file,
/// its path plus the lowercased token set derived from the path components and
/// the file's leading lines.
pub struct Index {
    entries: Vec<(PathBuf, Vec<String>)>,
}

impl Index {
    /// Number of indexed entries. Used by tests and external callers.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the index is empty (no files were indexed).
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Construct an index directly from `(path, tokens)` pairs. Tokens are
    /// lowercased on the way in so ranking is case-insensitive regardless of
    /// how the index was built. Primarily useful for tests and callers that
    /// already have file contents in hand.
    #[allow(dead_code)]
    pub fn from_entries(entries: Vec<(PathBuf, Vec<String>)>) -> Self {
        let entries = entries
            .into_iter()
            .map(|(path, tokens)| {
                let tokens = tokens.iter().flat_map(|t| tokenize(t)).collect::<Vec<_>>();
                (path, tokens)
            })
            .collect();
        Index { entries }
    }
}

/// Split `text` into lowercased alphanumeric tokens of length >= 2. Anything
/// that is not a letter or digit is treated as a separator, so paths,
/// snake_case, kebab-case and camelCase boundaries (at non-alphanumeric breaks)
/// all decompose into sensible terms.
fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 2)
        .map(|t| t.to_ascii_lowercase())
        .collect()
}

/// Build a bounded, `.gitignore`-respecting lexical index of `root` using the
/// default [`ScanLimits`]. Returns an empty index when `root` is not a
/// directory.
pub fn build_index(root: &Path) -> Index {
    build_index_with(root, &ScanLimits::default())
}

/// Like [`build_index`] but with caller-supplied [`ScanLimits`]. Mirrors the
/// scanning approach of [`crate::codebase_context`]: a bounded walk that
/// respects `.gitignore`, git excludes and hidden-file rules, reads only the
/// leading bytes of each file, and stops after `max_entries` filesystem
/// entries.
pub fn build_index_with(root: &Path, limits: &ScanLimits) -> Index {
    if !root.is_dir() {
        return Index {
            entries: Vec::new(),
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
        .max_depth(Some(limits.max_depth));

    let mut entries: Vec<(PathBuf, Vec<String>)> = Vec::new();
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
        if entry.file_type().is_some_and(|t| t.is_dir()) {
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

        let mut tokens: Vec<String> = tokenize(&rel_str);
        if let Some(head) = read_head(path, INDEX_HEAD_LINES, INDEX_HEAD_BYTES) {
            tokens.extend(tokenize(&head));
        }
        if tokens.is_empty() {
            continue;
        }
        entries.push((PathBuf::from(rel_str), tokens));
    }

    Index { entries }
}

/// Read up to `max_lines` leading lines (and at most `max_bytes`) from `path`
/// as lossy UTF-8. Returns `None` when the file cannot be read or is empty.
fn read_head(path: &Path, max_lines: usize, max_bytes: usize) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let slice = &bytes[..bytes.len().min(max_bytes)];
    let text = String::from_utf8_lossy(slice);
    let head: String = text.lines().take(max_lines).collect::<Vec<_>>().join("\n");
    if head.trim().is_empty() {
        None
    } else {
        Some(head)
    }
}

/// Rank the indexed files by lexical relevance to `query` and return the paths
/// of the `top_k` best matches, most relevant first.
///
/// Scoring is the size of the token-set overlap between the (lowercased) query
/// tokens and each entry's token set, plus a small bonus per query token that
/// appears as a substring of the entry's path. Entries with a zero score are
/// dropped, so an unrelated query yields an empty result rather than arbitrary
/// files. Ties break on path order for determinism.
pub fn rank_files(query: &str, index: &Index, top_k: usize) -> Vec<PathBuf> {
    if top_k == 0 {
        return Vec::new();
    }
    let query_tokens: BTreeSet<String> = tokenize(query).into_iter().collect();
    if query_tokens.is_empty() {
        return Vec::new();
    }

    let mut scored: Vec<(i64, &PathBuf)> = index
        .entries
        .iter()
        .filter_map(|(path, tokens)| {
            let token_set: BTreeSet<&str> = tokens.iter().map(|t| t.as_str()).collect();
            let overlap = query_tokens
                .iter()
                .filter(|q| token_set.contains(q.as_str()))
                .count() as i64;

            let path_lower = path.to_string_lossy().to_ascii_lowercase();
            let substring_bonus = query_tokens
                .iter()
                .filter(|q| path_lower.contains(q.as_str()))
                .count() as i64;

            let score = overlap * 2 + substring_bonus;
            if score > 0 {
                Some((score, path))
            } else {
                None
            }
        })
        .collect();

    // Highest score first; tie-break on path for stable, deterministic output.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));

    scored
        .into_iter()
        .take(top_k)
        .map(|(_, path)| path.clone())
        .collect()
}

/// Opt-in entry point: when the retriever is enabled, build a bounded index of
/// `root`, rank it against `query`, and return a compact `# Relevant files`
/// block listing the best matches. Returns `None` when the feature is disabled,
/// the query is empty, or nothing relevant was found — in which case nothing is
/// walked or injected.
pub fn retrieval_block(query: &str, root: &Path) -> Option<String> {
    if !is_enabled() {
        return None;
    }
    let query = query.trim();
    if query.is_empty() {
        return None;
    }

    let index = build_index(root);
    if index.is_empty() {
        return None;
    }

    let ranked = rank_files(query, &index, DEFAULT_TOP_K);
    if ranked.is_empty() {
        return None;
    }

    let mut out = String::from("# Relevant files\n");
    for path in &ranked {
        out.push_str("- ");
        out.push_str(&path.to_string_lossy());
        out.push('\n');
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, words: &[&str]) -> (PathBuf, Vec<String>) {
        (
            PathBuf::from(path),
            words.iter().map(|w| w.to_string()).collect(),
        )
    }

    #[test]
    fn is_truthy_recognises_common_values() {
        assert!(is_truthy("1"));
        assert!(is_truthy("TRUE"));
        assert!(is_truthy(" yes "));
        assert!(is_truthy("on"));
        assert!(!is_truthy("0"));
        assert!(!is_truthy("false"));
        assert!(!is_truthy(""));
        assert!(!is_truthy("garbage"));
    }

    #[test]
    fn rank_files_picks_the_overlapping_entry_first() {
        // Three synthetic entries; only one is about authentication tokens.
        let index = Index::from_entries(vec![
            entry(
                "src/auth/token.rs",
                &["authenticate", "token", "refresh", "login", "session"],
            ),
            entry("src/ui/button.rs", &["render", "button", "click", "widget"]),
            entry("docs/readme.md", &["overview", "project", "introduction"]),
        ]);

        let ranked = rank_files("how does the login token refresh work", &index, 3);
        assert!(!ranked.is_empty(), "expected at least one match");
        assert_eq!(
            ranked[0],
            PathBuf::from("src/auth/token.rs"),
            "the auth/token file should rank first, got: {ranked:?}"
        );
    }

    #[test]
    fn rank_files_returns_empty_for_unrelated_query() {
        let index = Index::from_entries(vec![
            entry("src/auth/token.rs", &["authenticate", "token"]),
            entry("src/ui/button.rs", &["render", "button"]),
        ]);
        // No overlapping tokens at all => no results (not arbitrary files).
        let ranked = rank_files("quantum chromodynamics", &index, 5);
        assert!(ranked.is_empty(), "got: {ranked:?}");
    }

    #[test]
    fn rank_files_top_k_is_respected() {
        let index = Index::from_entries(vec![
            entry("a/token.rs", &["token", "alpha"]),
            entry("b/token.rs", &["token", "beta"]),
            entry("c/token.rs", &["token", "gamma"]),
        ]);
        let ranked = rank_files("token", &index, 2);
        assert_eq!(ranked.len(), 2);
        // top_k == 0 yields nothing.
        assert!(rank_files("token", &index, 0).is_empty());
    }

    #[test]
    fn rank_files_path_substring_bonus_applies() {
        // Neither entry has overlapping head tokens; the query word "button"
        // only appears as a path substring, which should still surface it.
        let index = Index::from_entries(vec![
            entry("src/widgets/button_view.rs", &["render", "view"]),
            entry("src/data/store.rs", &["persist", "load"]),
        ]);
        let ranked = rank_files("button", &index, 2);
        assert_eq!(
            ranked.first(),
            Some(&PathBuf::from("src/widgets/button_view.rs"))
        );
    }

    #[test]
    fn retrieval_block_is_none_when_disabled() {
        // Feature defaults OFF: with BHARATCODE_RAG unset, no walk and no block.
        // (Avoid mutating process env in tests; just assert the gate is off.)
        assert!(
            !is_enabled(),
            "BHARATCODE_RAG must default to OFF in the test environment"
        );
        let dir = std::env::temp_dir();
        assert!(
            retrieval_block("anything", &dir).is_none(),
            "retrieval_block must return None while the feature is disabled"
        );
    }

    #[test]
    fn build_index_from_temp_dir_is_bounded_and_gitignore_aware() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        std::fs::write(root.join(".gitignore"), "ignored.rs\n").unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/login.rs"),
            "// authentication and token refresh logic\nfn login() {}\n",
        )
        .unwrap();
        std::fs::write(root.join("ignored.rs"), "fn token() {}\n").unwrap();

        let index = build_index(root);
        let ranked = rank_files("login token authentication", &index, 5);
        assert!(
            ranked
                .iter()
                .any(|p| p.to_string_lossy().contains("login.rs")),
            "expected src/login.rs to be ranked, got: {ranked:?}"
        );
        assert!(
            !ranked
                .iter()
                .any(|p| p.to_string_lossy().contains("ignored.rs")),
            "gitignored file must not be indexed, got: {ranked:?}"
        );
    }

    #[test]
    fn empty_query_yields_no_ranking() {
        let index = Index::from_entries(vec![entry("a.rs", &["token"])]);
        assert!(rank_files("", &index, 5).is_empty());
        assert!(rank_files("   ", &index, 5).is_empty());
    }
}
