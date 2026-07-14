//! Multi-file symbol rename tool (BharatCode v44).
//!
//! A real `rename_symbol` developer tool that performs a safe,
//! gitignore-respecting, word-boundary rename of an identifier across the
//! working tree. It enumerates candidate files with a local gitignore-aware walk
//! (the same `ignore` crate the codebase scanner uses), replaces only
//! whole-word matches of `old_name` (a manual `\bold\b` char-boundary check, no
//! regex dependency), and returns a per-file change count plus a compact
//! unified-diff-style preview.
//!
//! Safety first: `dry_run` defaults to `true`, so by default nothing is written
//! to disk and the model gets a preview it can inspect before committing to the
//! edit. Pass `dry_run: false` to actually rewrite the matched files.

use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use rmcp::model::{CallToolResult, Content};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

/// Resolve a user-facing label, preferring the i18n `tr!` macro when present and
/// otherwise falling back to the supplied English string. The macro does not yet
/// exist in every build, so the fallback keeps this tool compiling and localized
/// labels can be layered in later without touching call sites.
macro_rules! label {
    ($fallback:expr) => {{
        let _ = $fallback;
        $fallback
    }};
}

/// Largest file we are willing to load and rewrite, in bytes. Anything bigger is
/// skipped so the tool never tries to slurp a multi-gigabyte blob into memory.
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
/// Hard ceiling on how many files we report individually, to keep the result
/// compact for the model.
const MAX_REPORTED_FILES: usize = 200;
/// Number of leading characters of a changed line kept in the preview snippet.
const SNIPPET_LINE_BUDGET: usize = 160;
/// Maximum number of changed lines shown per file in the preview.
const MAX_SNIPPET_LINES_PER_FILE: usize = 3;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RefactorParams {
    /// The existing identifier to rename. Only whole-word (identifier-boundary)
    /// matches are replaced, so `widget` will not touch `widgetize`.
    pub old_name: String,
    /// The replacement identifier.
    pub new_name: String,
    /// Optional case-sensitive suffix filter applied to file names (for example
    /// `.rs` or `lib.rs`). When omitted, every text file under the working
    /// directory is considered.
    #[serde(default)]
    pub path_glob: Option<String>,
    /// When `true` (the default) no files are written; the result is a preview
    /// only. Set to `false` to actually apply the rename on disk.
    #[serde(default)]
    pub dry_run: Option<bool>,
}

/// Per-file outcome of a rename.
struct FileChange {
    /// Path relative to the working directory, using `/` separators.
    rel_path: String,
    /// Number of whole-word replacements made in this file.
    count: usize,
    /// Short before/after preview lines for the first few changed lines.
    snippets: Vec<DiffSnippet>,
}

/// A single changed line rendered as a minimal unified-diff hunk.
struct DiffSnippet {
    before: String,
    after: String,
}

pub struct RefactorTool;

impl RefactorTool {
    pub fn new() -> Self {
        Self
    }

    /// Perform a word-boundary rename of `old_name` -> `new_name` across the
    /// working tree and return a [`CallToolResult`] with a preview and structured
    /// summary. Honors `dry_run` (default `true`).
    pub async fn rename_symbol(
        &self,
        params: RefactorParams,
        working_dir: Option<&Path>,
    ) -> CallToolResult {
        let old = params.old_name.as_str();
        let new = params.new_name.as_str();

        if old.is_empty() {
            return error_result(label!("old_name cannot be empty"));
        }
        if old == new {
            return error_result(label!("old_name and new_name are identical"));
        }

        let root: PathBuf = match working_dir {
            Some(dir) => dir.to_path_buf(),
            None => match std::env::current_dir() {
                Ok(dir) => dir,
                Err(error) => {
                    return error_result(&format!(
                        "{}: {error}",
                        label!("could not resolve working directory")
                    ))
                }
            },
        };

        if !root.is_dir() {
            return error_result(label!("working directory is not a directory"));
        }

        let dry_run = params.dry_run.unwrap_or(true);
        let suffix = params.path_glob.as_deref();

        let mut changes: Vec<FileChange> = Vec::new();
        let mut total_replacements: usize = 0;
        let mut truncated = false;

        let walker = WalkBuilder::new(&root)
            .git_ignore(true)
            .git_exclude(true)
            .git_global(true)
            .require_git(false)
            .ignore(true)
            .hidden(true)
            .follow_links(false)
            .build();

        for entry in walker.flatten() {
            let path = entry.path();
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            if let Some(suffix) = suffix {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !name.ends_with(suffix) {
                    continue;
                }
            }
            if std::fs::metadata(path).map(|m| m.len()).unwrap_or(u64::MAX) > MAX_FILE_BYTES {
                continue;
            }

            let original = match std::fs::read_to_string(path) {
                Ok(text) => text,
                // Binary / non-UTF-8 files are skipped silently.
                Err(_) => continue,
            };

            let (rewritten, count) = replace_whole_word(&original, old, new);
            if count == 0 {
                continue;
            }
            total_replacements += count;

            let rel_path = path
                .strip_prefix(&root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");

            let snippets = collect_snippets(&original, old, new);

            if !dry_run {
                if let Err(error) = std::fs::write(path, &rewritten) {
                    return error_result(&format!(
                        "{} {rel_path}: {error}",
                        label!("failed to write")
                    ));
                }
            }

            if changes.len() < MAX_REPORTED_FILES {
                changes.push(FileChange {
                    rel_path,
                    count,
                    snippets,
                });
            } else {
                truncated = true;
            }
        }

        let summary = render_summary(old, new, dry_run, &changes, total_replacements, truncated);

        let mut result = CallToolResult::success(vec![Content::text(summary).with_priority(0.0)]);
        result.structured_content = Some(json!({
            "old_name": old,
            "new_name": new,
            "dry_run": dry_run,
            "total_replacements": total_replacements,
            "files_changed": changes
                .iter()
                .map(|c| json!({ "path": c.rel_path, "replacements": c.count }))
                .collect::<Vec<_>>(),
        }));
        result
    }
}

impl Default for RefactorTool {
    fn default() -> Self {
        Self::new()
    }
}

fn error_result(message: &str) -> CallToolResult {
    CallToolResult::error(vec![
        Content::text(format!("Error: {message}")).with_priority(0.0)
    ])
}

/// True when `byte` is part of an identifier (so a match is *not* on a word
/// boundary on that side). We treat ASCII alphanumerics and `_` as identifier
/// characters, mirroring the usual `\w` interpretation for source symbols.
fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Replace every whole-word occurrence of `old` in `haystack` with `new`,
/// returning the rewritten string and the number of replacements. A match
/// counts only when neither the character immediately before nor after it is an
/// identifier character, so `widget` never matches inside `widgetize`.
fn replace_whole_word(haystack: &str, old: &str, new: &str) -> (String, usize) {
    let bytes = haystack.as_bytes();
    let old_bytes = old.as_bytes();
    let old_len = old_bytes.len();

    let mut out = String::with_capacity(haystack.len());
    let mut count = 0usize;
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i..].starts_with(old_bytes) {
            let before_ok = i == 0 || !is_word_byte(bytes[i - 1]);
            let after_idx = i + old_len;
            let after_ok = after_idx >= bytes.len() || !is_word_byte(bytes[after_idx]);
            if before_ok && after_ok {
                out.push_str(new);
                count += 1;
                i = after_idx;
                continue;
            }
        }
        // Push this whole UTF-8 char (not just one byte) so multibyte content is
        // preserved correctly.
        let ch_len = utf8_char_len(bytes[i]);
        let end = (i + ch_len).min(bytes.len());
        out.push_str(
            haystack
                .get(i..end)
                .expect("indices span one complete UTF-8 character"),
        );
        i = end;
    }

    (out, count)
}

/// Length in bytes of the UTF-8 character whose leading byte is `b`.
fn utf8_char_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else if b >> 3 == 0b11110 {
        4
    } else {
        1
    }
}

/// Build a few before/after preview lines for the changed lines of a file.
fn collect_snippets(original: &str, old: &str, new: &str) -> Vec<DiffSnippet> {
    let mut snippets = Vec::new();
    for line in original.lines() {
        let (rewritten, count) = replace_whole_word(line, old, new);
        if count == 0 {
            continue;
        }
        snippets.push(DiffSnippet {
            before: clip(line),
            after: clip(&rewritten),
        });
        if snippets.len() >= MAX_SNIPPET_LINES_PER_FILE {
            break;
        }
    }
    snippets
}

/// Trim a line to the snippet budget on a char boundary, appending an ellipsis
/// when it was shortened.
fn clip(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.chars().count() <= SNIPPET_LINE_BUDGET {
        return trimmed.to_string();
    }
    let mut out: String = trimmed.chars().take(SNIPPET_LINE_BUDGET).collect();
    out.push('…');
    out
}

fn render_summary(
    old: &str,
    new: &str,
    dry_run: bool,
    changes: &[FileChange],
    total_replacements: usize,
    truncated: bool,
) -> String {
    let mut out = String::new();
    let mode = if dry_run {
        label!("dry run - no files written")
    } else {
        label!("applied")
    };
    out.push_str(&format!(
        "{}: {old} -> {new} ({mode})\n",
        label!("rename_symbol")
    ));
    out.push_str(&format!(
        "{} {} {} across {} {}\n",
        label!("Total"),
        total_replacements,
        label!("replacements"),
        changes.len(),
        label!("file(s)"),
    ));

    for change in changes {
        out.push_str(&format!("\n--- {} ({}×)\n", change.rel_path, change.count));
        for snippet in &change.snippets {
            out.push_str(&format!("- {}\n", snippet.before));
            out.push_str(&format!("+ {}\n", snippet.after));
        }
    }

    if truncated {
        out.push_str(&format!(
            "\n({})\n",
            label!("additional files changed but omitted from this preview")
        ));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn replace_respects_word_boundaries() {
        let (out, count) =
            replace_whole_word("widget widgetize widget_x x_widget", "widget", "gadget");
        assert_eq!(count, 1);
        assert_eq!(out, "gadget widgetize widget_x x_widget");
    }

    #[test]
    fn replace_handles_punctuation_boundaries() {
        let (out, count) = replace_whole_word("a.widget(); widget;", "widget", "gadget");
        assert_eq!(count, 2);
        assert_eq!(out, "a.gadget(); gadget;");
    }

    #[tokio::test]
    async fn rename_symbol_word_boundary_across_files() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        fs::write(
            root.join("a.rs"),
            "let widget = 1;\nfn widgetize() {}\nuse widget;\n",
        )
        .unwrap();
        fs::write(root.join("b.rs"), "struct widget;\nwidgetize();\n").unwrap();

        let tool = RefactorTool::new();
        let params = RefactorParams {
            old_name: "widget".to_string(),
            new_name: "gadget".to_string(),
            path_glob: None,
            dry_run: Some(false),
        };
        let result = tool.rename_symbol(params, Some(root)).await;
        assert_eq!(result.is_error, Some(false));

        let a = fs::read_to_string(root.join("a.rs")).unwrap();
        let b = fs::read_to_string(root.join("b.rs")).unwrap();

        // Whole-word `widget` renamed, `widgetize` left untouched.
        assert_eq!(a, "let gadget = 1;\nfn widgetize() {}\nuse gadget;\n");
        assert_eq!(b, "struct gadget;\nwidgetize();\n");

        let structured = result.structured_content.unwrap();
        assert_eq!(structured["total_replacements"], json!(3));

        let files = structured["files_changed"].as_array().unwrap();
        assert_eq!(files.len(), 2);
        let mut counts: Vec<(String, u64)> = files
            .iter()
            .map(|f| {
                (
                    f["path"].as_str().unwrap().to_string(),
                    f["replacements"].as_u64().unwrap(),
                )
            })
            .collect();
        counts.sort();
        assert_eq!(
            counts,
            vec![("a.rs".to_string(), 2), ("b.rs".to_string(), 1)]
        );
    }

    #[tokio::test]
    async fn dry_run_defaults_true_and_writes_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("c.rs"), "let widget = 1;\n").unwrap();

        let tool = RefactorTool::new();
        let params = RefactorParams {
            old_name: "widget".to_string(),
            new_name: "gadget".to_string(),
            path_glob: None,
            dry_run: None,
        };
        let result = tool.rename_symbol(params, Some(root)).await;
        assert_eq!(result.is_error, Some(false));

        // File on disk is unchanged because dry_run defaulted to true.
        assert_eq!(
            fs::read_to_string(root.join("c.rs")).unwrap(),
            "let widget = 1;\n"
        );

        let structured = result.structured_content.unwrap();
        assert_eq!(structured["dry_run"], json!(true));
        assert_eq!(structured["total_replacements"], json!(1));
    }
}
