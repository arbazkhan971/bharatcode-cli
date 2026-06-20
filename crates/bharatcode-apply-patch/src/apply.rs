// Derived from OpenAI Codex `codex-apply-patch` (Apache-2.0, Copyright 2025 OpenAI).
// See LICENSES/LICENSE-codex and NOTICE.
//
// The `compute_replacements` and `apply_replacements` transforms are ported
// verbatim from Codex's `lib.rs`. The filesystem applier (`apply_patch_to_disk`,
// `apply_hunks_to_disk`, `derive_new_contents`) is a new synchronous std::fs
// implementation that replaces Codex's async `ExecutorFileSystem` sandbox path,
// which depended on the (un-vendorable) `codex-exec-server` crate.

use std::fs;
use std::path::Path;
use std::path::PathBuf;

use thiserror::Error;

use crate::parser::Hunk;
use crate::parser::ParseError;
use crate::parser::UpdateFileChunk;
use crate::parser::parse_patch;
use crate::seek_sequence::seek_sequence;

#[derive(Debug, Error)]
pub enum ApplyPatchError {
    #[error(transparent)]
    Parse(#[from] ParseError),
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
    /// Error that occurs while computing replacements when applying patch chunks.
    #[error("{0}")]
    ComputeReplacements(String),
}

impl ApplyPatchError {
    fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        ApplyPatchError::Io {
            context: context.into(),
            source,
        }
    }
}

/// Files affected by applying a patch, keeping the path spelling from the patch
/// for user-facing summaries.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ApplySummary {
    pub added: Vec<PathBuf>,
    pub modified: Vec<PathBuf>,
    pub deleted: Vec<PathBuf>,
}

impl ApplySummary {
    /// Render a git-style summary of the affected files.
    pub fn render(&self) -> String {
        let mut out = String::from("Success. Updated the following files:\n");
        for path in &self.added {
            out.push_str(&format!("A {}\n", path.display()));
        }
        for path in &self.modified {
            out.push_str(&format!("M {}\n", path.display()));
        }
        for path in &self.deleted {
            out.push_str(&format!("D {}\n", path.display()));
        }
        out
    }
}

/// Parse `patch` (in the `*** Begin Patch` format) and apply every hunk to the
/// filesystem rooted at `cwd`. Relative hunk paths are resolved against `cwd`;
/// absolute paths are used as-is.
pub fn apply_patch_to_disk(patch: &str, cwd: &Path) -> Result<ApplySummary, ApplyPatchError> {
    let args = parse_patch(patch)?;
    apply_hunks_to_disk(&args.hunks, cwd)
}

/// Apply already-parsed `hunks` to the filesystem rooted at `cwd`.
pub fn apply_hunks_to_disk(hunks: &[Hunk], cwd: &Path) -> Result<ApplySummary, ApplyPatchError> {
    let mut summary = ApplySummary::default();
    for hunk in hunks {
        match hunk {
            Hunk::AddFile { path, contents } => {
                let dest = resolve(cwd, path);
                create_parent_dirs(&dest)?;
                fs::write(&dest, contents).map_err(|e| {
                    ApplyPatchError::io(format!("Failed to write {}", dest.display()), e)
                })?;
                summary.added.push(path.clone());
            }
            Hunk::DeleteFile { path } => {
                let target = resolve(cwd, path);
                fs::remove_file(&target).map_err(|e| {
                    ApplyPatchError::io(format!("Failed to delete {}", target.display()), e)
                })?;
                summary.deleted.push(path.clone());
            }
            Hunk::UpdateFile {
                path,
                move_path,
                chunks,
            } => {
                let src = resolve(cwd, path);
                let original = fs::read_to_string(&src).map_err(|e| {
                    ApplyPatchError::io(
                        format!("Failed to read file to update {}", src.display()),
                        e,
                    )
                })?;
                let new_contents = derive_new_contents(&original, &path.to_string_lossy(), chunks)?;
                match move_path {
                    Some(dest_rel) => {
                        let dest = resolve(cwd, dest_rel);
                        create_parent_dirs(&dest)?;
                        fs::write(&dest, &new_contents).map_err(|e| {
                            ApplyPatchError::io(format!("Failed to write {}", dest.display()), e)
                        })?;
                        if dest != src {
                            fs::remove_file(&src).map_err(|e| {
                                ApplyPatchError::io(
                                    format!("Failed to remove original {}", src.display()),
                                    e,
                                )
                            })?;
                        }
                        summary.modified.push(dest_rel.clone());
                    }
                    None => {
                        fs::write(&src, &new_contents).map_err(|e| {
                            ApplyPatchError::io(format!("Failed to write {}", src.display()), e)
                        })?;
                        summary.modified.push(path.clone());
                    }
                }
            }
        }
    }
    Ok(summary)
}

fn resolve(cwd: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

fn create_parent_dirs(path: &Path) -> Result<(), ApplyPatchError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        fs::create_dir_all(parent).map_err(|e| {
            ApplyPatchError::io(
                format!("Failed to create parent directories for {}", path.display()),
                e,
            )
        })?;
    }
    Ok(())
}

/// Return the new file contents after applying `chunks` to `original_contents`.
fn derive_new_contents(
    original_contents: &str,
    path_text: &str,
    chunks: &[UpdateFileChunk],
) -> Result<String, ApplyPatchError> {
    let mut original_lines: Vec<String> = original_contents.split('\n').map(String::from).collect();

    // Drop the trailing empty element that results from the final newline so
    // that line counts match the behaviour of standard `diff`.
    if original_lines.last().is_some_and(String::is_empty) {
        original_lines.pop();
    }

    let replacements = compute_replacements(&original_lines, path_text, chunks)?;
    let mut new_lines = apply_replacements(original_lines, &replacements);
    if !new_lines.last().is_some_and(String::is_empty) {
        new_lines.push(String::new());
    }
    Ok(new_lines.join("\n"))
}

/// Compute a list of replacements needed to transform `original_lines` into the
/// new lines, given the patch `chunks`. Each replacement is returned as
/// `(start_index, old_len, new_lines)`.
fn compute_replacements(
    original_lines: &[String],
    path: &str,
    chunks: &[UpdateFileChunk],
) -> Result<Vec<(usize, usize, Vec<String>)>, ApplyPatchError> {
    let mut replacements: Vec<(usize, usize, Vec<String>)> = Vec::new();
    let mut line_index: usize = 0;

    for chunk in chunks {
        // If a chunk has a `change_context`, we use seek_sequence to find it, then
        // adjust our `line_index` to continue from there.
        if let Some(ctx_line) = &chunk.change_context {
            if let Some(idx) = seek_sequence(
                original_lines,
                std::slice::from_ref(ctx_line),
                line_index,
                /*eof*/ false,
            ) {
                line_index = idx + 1;
            } else {
                return Err(ApplyPatchError::ComputeReplacements(format!(
                    "Failed to find context '{ctx_line}' in {path}"
                )));
            }
        }

        if chunk.old_lines.is_empty() {
            // Pure addition (no old lines). We'll add them at the end or just
            // before the final empty line if one exists.
            let insertion_idx = if original_lines.last().is_some_and(String::is_empty) {
                original_lines.len() - 1
            } else {
                original_lines.len()
            };
            replacements.push((insertion_idx, 0, chunk.new_lines.clone()));
            continue;
        }

        // Otherwise, try to match the existing lines in the file with the old lines
        // from the chunk. If found, schedule that region for replacement.
        // Attempt to locate the `old_lines` verbatim within the file.  In many
        // real-world diffs the last element of `old_lines` is an *empty* string
        // representing the terminating newline of the region being replaced.
        // This sentinel is not present in `original_lines` because we strip the
        // trailing empty slice emitted by `split('\n')`.  If a direct search
        // fails and the pattern ends with an empty string, retry without that
        // final element so that modifications touching the end-of-file can be
        // located reliably.

        let mut pattern: &[String] = &chunk.old_lines;
        let mut found = seek_sequence(original_lines, pattern, line_index, chunk.is_end_of_file);

        let mut new_slice: &[String] = &chunk.new_lines;

        if found.is_none() && pattern.last().is_some_and(String::is_empty) {
            // Retry without the trailing empty line which represents the final
            // newline in the file.
            pattern = &pattern[..pattern.len() - 1];
            if new_slice.last().is_some_and(String::is_empty) {
                new_slice = &new_slice[..new_slice.len() - 1];
            }

            found = seek_sequence(original_lines, pattern, line_index, chunk.is_end_of_file);
        }

        if let Some(start_idx) = found {
            replacements.push((start_idx, pattern.len(), new_slice.to_vec()));
            line_index = start_idx + pattern.len();
        } else {
            return Err(ApplyPatchError::ComputeReplacements(format!(
                "Failed to find expected lines in {}:\n{}",
                path,
                chunk.old_lines.join("\n"),
            )));
        }
    }

    replacements.sort_by_key(|(index, _, _)| *index);

    Ok(replacements)
}

/// Apply the `(start_index, old_len, new_lines)` replacements to `original_lines`,
/// returning the modified file contents as a vector of lines.
fn apply_replacements(
    mut lines: Vec<String>,
    replacements: &[(usize, usize, Vec<String>)],
) -> Vec<String> {
    // We must apply replacements in descending order so that earlier replacements
    // don't shift the positions of later ones.
    for (start_idx, old_len, new_segment) in replacements.iter().rev() {
        let start_idx = *start_idx;
        let old_len = *old_len;

        // Remove old lines.
        for _ in 0..old_len {
            if start_idx < lines.len() {
                lines.remove(start_idx);
            }
        }

        // Insert new lines.
        for (offset, new_line) in new_segment.iter().enumerate() {
            lines.insert(start_idx + offset, new_line.clone());
        }
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn wrap(body: &str) -> String {
        format!("*** Begin Patch\n{body}\n*** End Patch")
    }

    #[test]
    fn add_file_creates_file_with_contents() {
        let dir = tempdir().unwrap();
        let patch = wrap("*** Add File: sub/new.txt\n+hello\n+world");
        let summary = apply_patch_to_disk(&patch, dir.path()).unwrap();

        assert_eq!(
            fs::read_to_string(dir.path().join("sub/new.txt")).unwrap(),
            "hello\nworld\n"
        );
        assert_eq!(summary.added, vec![PathBuf::from("sub/new.txt")]);
        assert!(summary.modified.is_empty());
        assert!(summary.deleted.is_empty());
    }

    #[test]
    fn update_file_replaces_matched_lines() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("file.py");
        fs::write(&path, "def f():\n    pass\n").unwrap();

        let patch = wrap("*** Update File: file.py\n@@ def f():\n-    pass\n+    return 123");
        let summary = apply_patch_to_disk(&patch, dir.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "def f():\n    return 123\n"
        );
        assert_eq!(summary.modified, vec![PathBuf::from("file.py")]);
    }

    #[test]
    fn update_file_uses_fuzzy_context_matching() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("file.txt");
        // Trailing whitespace in the file that the patch context omits.
        fs::write(&path, "alpha   \nbeta\ngamma\n").unwrap();

        let patch = wrap("*** Update File: file.txt\n@@\n alpha\n-beta\n+BETA");
        apply_patch_to_disk(&patch, dir.path()).unwrap();

        // Fuzzy matching locates the region despite the file's trailing
        // whitespace; the matched context line is rewritten to the patch's
        // spelling.
        assert_eq!(fs::read_to_string(&path).unwrap(), "alpha\nBETA\ngamma\n");
    }

    #[test]
    fn delete_file_removes_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("gone.txt");
        fs::write(&path, "bye\n").unwrap();

        let patch = wrap("*** Delete File: gone.txt");
        let summary = apply_patch_to_disk(&patch, dir.path()).unwrap();

        assert!(!path.exists());
        assert_eq!(summary.deleted, vec![PathBuf::from("gone.txt")]);
    }

    #[test]
    fn update_file_with_move_renames_and_rewrites() {
        let dir = tempdir().unwrap();
        let old = dir.path().join("old.rs");
        fs::write(&old, "old\n").unwrap();

        let patch = wrap("*** Update File: old.rs\n*** Move to: nested/new.rs\n@@\n-old\n+new");
        let summary = apply_patch_to_disk(&patch, dir.path()).unwrap();

        assert!(!old.exists());
        assert_eq!(
            fs::read_to_string(dir.path().join("nested/new.rs")).unwrap(),
            "new\n"
        );
        assert_eq!(summary.modified, vec![PathBuf::from("nested/new.rs")]);
    }

    #[test]
    fn missing_context_reports_compute_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("file.txt");
        fs::write(&path, "one\ntwo\n").unwrap();

        let patch = wrap("*** Update File: file.txt\n@@\n-nope\n+yep");
        let err = apply_patch_to_disk(&patch, dir.path()).unwrap_err();
        assert!(matches!(err, ApplyPatchError::ComputeReplacements(_)));
    }

    #[test]
    fn summary_renders_git_style() {
        let summary = ApplySummary {
            added: vec![PathBuf::from("a.txt")],
            modified: vec![PathBuf::from("b.txt")],
            deleted: vec![PathBuf::from("c.txt")],
        };
        let rendered = summary.render();
        assert!(rendered.contains("A a.txt"));
        assert!(rendered.contains("M b.txt"));
        assert!(rendered.contains("D c.txt"));
    }
}
