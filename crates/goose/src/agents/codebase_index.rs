//! Opt-in lightweight semantic codebase index (RAG) — BharatCode v43.
//!
//! Builds a tiny in-memory inverted index over the repository so the agent can
//! be seeded with a compact "Relevant files" shortlist in its system prompt.
//! The index keys on two cheap, regex-free signals per file:
//!
//!   1. **Path components** — directory + file-name tokens (split on
//!      non-alphanumeric boundaries, lower-cased).
//!   2. **Top-level identifiers** — words pulled from the file's first
//!      documentation line plus any obvious symbol-ish tokens on the leading
//!      lines (again split purely on non-alphanumeric boundaries; no regex).
//!
//! Files are enumerated by reusing the existing bounded, `.gitignore`-respecting
//! walker [`crate::codebase_context::scan_codebase_with`]'s sibling primitive —
//! we share the very same [`crate::codebase_context::ScanLimits`] bounds and the
//! [`ignore`] walker so build artefacts and vendored trees are skipped and a
//! pathological repo can never blow up memory or wall-clock time.
//!
//! Scoring is a small BM25-style ranker over the inverted index. Given a free
//! text query we tokenise it the same way, look up each term's posting list, and
//! accumulate an IDF-weighted, length-normalised score per file. The top `k`
//! file paths are returned, highest score first.
//!
//! The whole feature is **opt-in and defaults to off**, gated on the
//! `BHARATCODE_CODEBASE_INDEX` switch (environment variable first, then the
//! on-disk config, mirroring the other BharatCode switches and the
//! `memory_store` truthiness pattern). When the switch is off,
//! [`relevant_files_block`] returns `None` and nothing is scanned, so the system
//! prompt is byte-identical to the unmodified default.

use std::collections::HashMap;
use std::path::Path;

use ignore::WalkBuilder;

use crate::codebase_context::ScanLimits;

/// Config / environment key for the codebase-index switch. Defaults to off.
pub const CODEBASE_INDEX_KEY: &str = "BHARATCODE_CODEBASE_INDEX";

/// Maximum number of bytes read from the head of each file when extracting
/// identifier / first-doc-line tokens. Kept small so indexing stays fast.
const HEAD_BYTES: usize = 1_024;

/// Default number of files surfaced in the "Relevant files" block.
const DEFAULT_TOP_K: usize = 8;

/// A built inverted index over a repository.
///
/// `docs` holds one entry per indexed file (its relative path plus the total
/// token count used for length normalisation). `postings` maps a term to the
/// list of `(doc_index, term_frequency)` pairs that mention it.
#[derive(Debug, Default)]
pub struct Index {
    docs: Vec<Doc>,
    postings: HashMap<String, Vec<(usize, u32)>>,
    total_len: u64,
}

#[derive(Debug)]
struct Doc {
    path: String,
    len: u32,
}

impl Index {
    /// Number of indexed files.
    pub fn len(&self) -> usize {
        self.docs.len()
    }

    /// True when no files were indexed.
    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    fn avg_len(&self) -> f64 {
        if self.docs.is_empty() {
            0.0
        } else {
            self.total_len as f64 / self.docs.len() as f64
        }
    }
}

/// Interpret a raw flag value as truthy. Mirrors the `memory_store` pattern so
/// the BharatCode switches behave consistently.
fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Returns true when the codebase semantic index is enabled. Defaults to
/// `false`.
///
/// The environment variable `BHARATCODE_CODEBASE_INDEX` takes precedence; when
/// it is unset the on-disk config is read. Any ambiguity resolves to "off" so
/// default behaviour never changes by accident.
pub fn is_enabled() -> bool {
    if let Ok(raw) = std::env::var(CODEBASE_INDEX_KEY) {
        return is_truthy(&raw);
    }
    crate::config::Config::global()
        .get_param::<String>(CODEBASE_INDEX_KEY)
        .map(|raw| is_truthy(&raw))
        .unwrap_or(false)
}

/// Split `s` into lower-cased alphanumeric tokens. Regex-free: we simply break
/// on any character that is not ASCII-alphanumeric. Single-character and purely
/// numeric tokens are dropped as low-signal noise.
fn tokenize(s: &str, out: &mut Vec<String>) {
    for raw in s.split(|c: char| !c.is_ascii_alphanumeric()) {
        if raw.len() < 2 {
            continue;
        }
        if raw.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        out.push(raw.to_ascii_lowercase());
    }
}

/// Pull tokens from the head of a file: the path-relative name plus the first
/// non-empty documentation/comment line and any leading symbol-ish lines.
fn file_tokens(rel_path: &str, head: &str, out: &mut Vec<String>) {
    // Path components (directories + file name, extension included).
    tokenize(rel_path, out);

    // First few lines: capture the first meaningful doc/comment line and any
    // top-level identifiers. We deliberately read only the head of the file.
    let mut doc_line_taken = false;
    for line in head.lines().take(40) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let is_comment = trimmed.starts_with("//")
            || trimmed.starts_with('#')
            || trimmed.starts_with("/*")
            || trimmed.starts_with('*')
            || trimmed.starts_with("--")
            || trimmed.starts_with("\"\"\"");
        if is_comment && !doc_line_taken {
            tokenize(trimmed, out);
            doc_line_taken = true;
            continue;
        }
        // Definition-ish leading lines contribute their identifiers too.
        let looks_like_def = trimmed.starts_with("fn ")
            || trimmed.starts_with("pub ")
            || trimmed.contains("struct ")
            || trimmed.contains("enum ")
            || trimmed.contains("class ")
            || trimmed.contains("def ")
            || trimmed.contains("function ")
            || trimmed.contains("interface ")
            || trimmed.contains("type ")
            || trimmed.contains("const ")
            || trimmed.contains("impl ");
        if looks_like_def {
            tokenize(trimmed, out);
        }
    }
}

/// Read at most [`HEAD_BYTES`] from `path` as lossy UTF-8. Returns an empty
/// string when the file cannot be read.
fn read_head(path: &Path) -> String {
    match std::fs::read(path) {
        Ok(bytes) => {
            let slice = &bytes[..bytes.len().min(HEAD_BYTES)];
            String::from_utf8_lossy(slice).into_owned()
        }
        Err(_) => String::new(),
    }
}

/// Build the inverted index for `root` using the default scan bounds.
pub fn build_index(root: &Path) -> Index {
    build_index_with(root, &ScanLimits::default())
}

/// Like [`build_index`] but with caller-supplied bounds (mainly for tests).
pub fn build_index_with(root: &Path, limits: &ScanLimits) -> Index {
    let mut index = Index::default();
    if !root.is_dir() {
        return index;
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

    let mut seen = 0usize;
    let mut tokens: Vec<String> = Vec::new();
    for entry in builder.build().flatten() {
        seen += 1;
        if seen > limits.max_entries {
            break;
        }
        if index.docs.len() >= limits.max_entries {
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

        tokens.clear();
        let head = read_head(path);
        file_tokens(&rel_str, &head, &mut tokens);
        if tokens.is_empty() {
            continue;
        }

        let doc_index = index.docs.len();
        let mut term_freq: HashMap<&str, u32> = HashMap::new();
        for tok in &tokens {
            *term_freq.entry(tok.as_str()).or_insert(0) += 1;
        }
        let doc_len = tokens.len() as u32;
        index.total_len += doc_len as u64;
        index.docs.push(Doc {
            path: rel_str,
            len: doc_len,
        });
        for (term, freq) in term_freq {
            index
                .postings
                .entry(term.to_string())
                .or_default()
                .push((doc_index, freq));
        }
    }

    index
}

/// Rank indexed files against `query` and return the paths of the top `k`,
/// highest score first. Returns an empty vector when the index or query is
/// empty.
pub fn top_files(index: &Index, query: &str, k: usize) -> Vec<String> {
    if index.is_empty() || k == 0 {
        return Vec::new();
    }
    let mut q_tokens: Vec<String> = Vec::new();
    tokenize(query, &mut q_tokens);
    if q_tokens.is_empty() {
        return Vec::new();
    }
    q_tokens.sort();
    q_tokens.dedup();

    // BM25 parameters (standard defaults).
    const K1: f64 = 1.2;
    const B: f64 = 0.75;
    let n = index.docs.len() as f64;
    let avg_len = index.avg_len();

    let mut scores: HashMap<usize, f64> = HashMap::new();
    for term in &q_tokens {
        let Some(postings) = index.postings.get(term) else {
            continue;
        };
        let df = postings.len() as f64;
        // BM25 IDF with the usual +0.5 smoothing; clamped to be non-negative.
        let idf = (((n - df + 0.5) / (df + 0.5)) + 1.0).ln();
        if idf <= 0.0 {
            continue;
        }
        for &(doc_index, tf) in postings {
            let doc_len = index.docs[doc_index].len as f64;
            let tf = tf as f64;
            let denom = tf + K1 * (1.0 - B + B * (doc_len / avg_len.max(1.0)));
            let contribution = idf * (tf * (K1 + 1.0)) / denom;
            *scores.entry(doc_index).or_insert(0.0) += contribution;
        }
    }

    let mut ranked: Vec<(usize, f64)> = scores.into_iter().collect();
    // Sort by score descending; break ties by path for stable, cache-friendly
    // output.
    ranked.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| index.docs[a.0].path.cmp(&index.docs[b.0].path))
    });
    ranked
        .into_iter()
        .take(k)
        .map(|(doc_index, _)| index.docs[doc_index].path.clone())
        .collect()
}

/// Derive a stable, cache-friendly query from the repository itself when no
/// explicit user query is available: the root directory name plus the
/// shallowest source file names. This keeps the injected block deterministic
/// across sessions so prompt caching is preserved.
fn fallback_query(root: &Path, index: &Index) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(name) = root.file_name().and_then(|n| n.to_str()) {
        parts.push(name.to_string());
    }
    for doc in index.docs.iter().take(16) {
        parts.push(doc.path.clone());
    }
    parts.join(" ")
}

/// Opt-in entry point used by prompt assembly: returns a compact "Relevant
/// files" block for `root` ranked against `query`, or `None` when the feature
/// is disabled or nothing useful was found.
///
/// When `query` is empty a stable fallback query is derived from the repository
/// layout so the block remains deterministic (and the prompt cacheable).
pub fn relevant_files_block(root: &Path, query: &str) -> Option<String> {
    if !is_enabled() {
        return None;
    }
    let index = build_index(root);
    if index.is_empty() {
        return None;
    }

    let effective_query = if query.trim().is_empty() {
        fallback_query(root, &index)
    } else {
        query.to_string()
    };

    let files = top_files(&index, &effective_query, DEFAULT_TOP_K);
    if files.is_empty() {
        return None;
    }

    let mut out = String::new();
    out.push_str("# Relevant files\n");
    out.push_str(
        "These repository files look most relevant to the current task; consult them first:\n",
    );
    for file in files {
        out.push_str("- ");
        out.push_str(&file);
        out.push('\n');
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, body).unwrap();
    }

    #[test]
    fn tokenize_is_regex_free_and_drops_noise() {
        let mut out = Vec::new();
        tokenize("src/foo_handler.rs", &mut out);
        assert!(out.contains(&"src".to_string()));
        assert!(out.contains(&"foo".to_string()));
        assert!(out.contains(&"handler".to_string()));
        assert!(out.contains(&"rs".to_string()));

        let mut noise = Vec::new();
        tokenize("a 1 22 ok", &mut noise);
        // Single chars and pure digits are dropped.
        assert_eq!(noise, vec!["ok".to_string()]);
    }

    #[test]
    fn build_index_ranks_matching_file_first() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        write(
            root,
            "src/foo_handler.rs",
            "//! Handler for foo requests.\npub fn handle_foo() {}\n",
        );
        write(
            root,
            "README.md",
            "# Demo Project\n\nThis is a small example repository.\n",
        );
        write(
            root,
            "src/util.rs",
            "pub fn add(a: i32, b: i32) -> i32 { a + b }\n",
        );

        let index = build_index(root);
        assert!(index.len() >= 2, "indexed {} files", index.len());

        let ranked = top_files(&index, "handler", 3);
        assert!(!ranked.is_empty(), "no results");
        assert_eq!(
            ranked[0], "src/foo_handler.rs",
            "handler query should rank foo_handler first, got {ranked:?}"
        );
    }

    #[test]
    fn empty_query_yields_no_results() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write(root, "src/main.rs", "fn main() {}\n");
        let index = build_index(root);
        assert!(top_files(&index, "", 5).is_empty());
        assert!(top_files(&index, "   ", 5).is_empty());
    }

    #[test]
    fn index_respects_gitignore() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write(root, ".gitignore", "ignored/\n");
        write(root, "ignored/secret_handler.rs", "//! secret handler\n");
        write(root, "src/visible_handler.rs", "//! visible handler\n");

        let index = build_index(root);
        let ranked = top_files(&index, "handler", 5);
        assert!(
            ranked.iter().any(|p| p.contains("visible_handler")),
            "expected visible file, got {ranked:?}"
        );
        assert!(
            !ranked.iter().any(|p| p.contains("secret_handler")),
            "gitignored file leaked into results: {ranked:?}"
        );
    }

    #[test]
    fn relevant_files_block_is_none_when_disabled() {
        // The switch defaults to off; with no env override the block must be
        // None so the system prompt is byte-identical to default.
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write(root, "src/foo_handler.rs", "//! handler\n");

        let _guard = env_lock::lock_env([(CODEBASE_INDEX_KEY, None::<&str>)]);
        assert!(
            relevant_files_block(root, "handler").is_none(),
            "disabled feature must return None"
        );
    }

    #[test]
    fn relevant_files_block_lists_files_when_enabled() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write(root, "src/foo_handler.rs", "//! Handler for foo.\n");
        write(root, "README.md", "# Demo\n");

        let _guard = env_lock::lock_env([(CODEBASE_INDEX_KEY, Some("1"))]);
        let block = relevant_files_block(root, "handler").expect("enabled => Some");
        assert!(block.contains("# Relevant files"), "got: {block}");
        assert!(block.contains("src/foo_handler.rs"), "got: {block}");
        // No user-facing Goose/Block leakage.
        assert!(!block.to_lowercase().contains("goose"), "got: {block}");
    }
}
