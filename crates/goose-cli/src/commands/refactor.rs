//! `bharatcode refactor` — a gitignore-respecting, dry-run-by-default
//! multi-file literal find/replace with a per-file diff preview.
//!
//! The command walks the working tree (honouring `.gitignore` exactly like the
//! codebase scanners in the `goose` crate, which build on [`ignore::WalkBuilder`]),
//! optionally filters by a glob, counts literal substring matches in each file,
//! and prints a unified-ish preview of the change. Nothing is written to disk
//! unless `--apply` is passed, so the default behaviour is safe and inspectable.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;

/// Options that drive a refactor run.
pub struct RefactorOptions {
    /// Literal substring to search for. Not a regular expression.
    pub find: String,
    /// Replacement string substituted for every occurrence of `find`.
    pub replace: String,
    /// Optional glob (e.g. `*.rs` or `src/**/*.ts`) restricting which files are
    /// considered. When `None`, every non-ignored text file is eligible.
    pub glob: Option<String>,
    /// When `false` (the default) the run is a dry preview and nothing is
    /// written. When `true`, matching files are rewritten in place.
    pub apply: bool,
}

/// A single file that contains at least one occurrence of the search string,
/// together with the rewritten content and an occurrence count.
#[derive(Debug, Clone)]
pub struct FileChange {
    /// Path to the file, relative to (or under) the scanned root.
    pub path: PathBuf,
    /// Number of literal occurrences of `find` replaced in this file.
    pub occurrences: usize,
    /// The file's content after substitution.
    pub new_content: String,
    /// The file's content before substitution.
    pub old_content: String,
}

/// Walk `root` (respecting `.gitignore`), apply the optional glob filter, and
/// compute the set of files that contain the literal search string.
///
/// This is the testable core of the command: it performs no I/O beyond reading
/// candidate files and never mutates the filesystem. The returned changes carry
/// both the original and rewritten content so callers can preview or apply.
pub fn plan_replacements(root: &Path, opts: &RefactorOptions) -> Vec<FileChange> {
    if opts.find.is_empty() {
        return Vec::new();
    }

    let mut builder = WalkBuilder::new(root);
    builder.standard_filters(true).hidden(false);

    if let Some(glob) = opts.glob.as_deref() {
        let mut overrides = OverrideBuilder::new(root);
        if overrides.add(glob).is_ok() {
            if let Ok(built) = overrides.build() {
                builder.overrides(built);
            }
        }
    }

    let mut changes = Vec::new();
    for entry in builder.build().flatten() {
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let path = entry.path();
        let Ok(old_content) = std::fs::read_to_string(path) else {
            // Skip binary / non-UTF-8 files silently.
            continue;
        };
        let occurrences = old_content.matches(&opts.find).count();
        if occurrences == 0 {
            continue;
        }
        let new_content = old_content.replace(&opts.find, &opts.replace);
        changes.push(FileChange {
            path: path.to_path_buf(),
            occurrences,
            new_content,
            old_content,
        });
    }

    changes.sort_by(|a, b| a.path.cmp(&b.path));
    changes
}

/// Render a compact, colourless line-level preview of a single file change to
/// stdout. Removed lines are prefixed with `-`, added lines with `+`.
pub fn render_preview(change: &FileChange) {
    println!(
        "\n{} ({} {})",
        change.path.display(),
        change.occurrences,
        if change.occurrences == 1 {
            "occurrence"
        } else {
            "occurrences"
        }
    );

    let old_lines: Vec<&str> = change.old_content.lines().collect();
    let new_lines: Vec<&str> = change.new_content.lines().collect();
    let max = old_lines.len().max(new_lines.len());

    for i in 0..max {
        let old = old_lines.get(i).copied();
        let new = new_lines.get(i).copied();
        if old == new {
            continue;
        }
        if let Some(o) = old {
            println!("  - {o}");
        }
        if let Some(n) = new {
            println!("  + {n}");
        }
    }
}

/// Entry point for the `refactor` subcommand. Prints a preview for every file
/// that would change and, only when `opts.apply` is set, writes the new content
/// back to disk via [`std::fs`].
pub fn handle_refactor(opts: RefactorOptions) -> Result<()> {
    let root = std::env::current_dir().context("resolving current directory")?;
    let changes = plan_replacements(&root, &opts);

    if changes.is_empty() {
        println!("No files contain '{}'.", opts.find);
        return Ok(());
    }

    let total: usize = changes.iter().map(|c| c.occurrences).sum();
    for change in &changes {
        render_preview(change);
    }

    println!(
        "\n{} occurrence(s) across {} file(s) would be replaced.",
        total,
        changes.len()
    );

    if opts.apply {
        for change in &changes {
            std::fs::write(&change.path, &change.new_content)
                .with_context(|| format!("writing {}", change.path.display()))?;
        }
        println!("Applied changes to {} file(s).", changes.len());
    } else {
        println!("Dry run: no files were modified. Re-run with --apply to write changes.");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn plan_finds_files_and_counts_occurrences() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        fs::write(root.join("a.txt"), "foo bar foo\nfoo end\n").expect("write a");
        fs::write(root.join("b.txt"), "nothing here\n").expect("write b");
        fs::write(root.join("c.txt"), "one foo only\n").expect("write c");

        let opts = RefactorOptions {
            find: "foo".to_string(),
            replace: "qux".to_string(),
            glob: None,
            apply: false,
        };

        let changes = plan_replacements(root, &opts);

        assert_eq!(changes.len(), 2, "only files containing 'foo' should match");

        let a = changes
            .iter()
            .find(|c| c.path.ends_with("a.txt"))
            .expect("a.txt present");
        assert_eq!(a.occurrences, 3);
        assert_eq!(a.new_content, "qux bar qux\nqux end\n");

        let c = changes
            .iter()
            .find(|c| c.path.ends_with("c.txt"))
            .expect("c.txt present");
        assert_eq!(c.occurrences, 1);
        assert_eq!(c.new_content, "one qux only\n");
    }

    #[test]
    fn dry_run_writes_nothing_to_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        let a_path = root.join("a.txt");
        let b_path = root.join("b.txt");
        let a_before = "foo bar foo\n";
        let b_before = "plain text\n";
        fs::write(&a_path, a_before).expect("write a");
        fs::write(&b_path, b_before).expect("write b");

        let opts = RefactorOptions {
            find: "foo".to_string(),
            replace: "qux".to_string(),
            glob: None,
            apply: false,
        };

        let changes = plan_replacements(root, &opts);
        assert_eq!(changes.len(), 1);

        // Dry-run: planning must not touch disk — files are byte-identical.
        assert_eq!(fs::read(&a_path).expect("read a"), a_before.as_bytes());
        assert_eq!(fs::read(&b_path).expect("read b"), b_before.as_bytes());
    }

    #[test]
    fn glob_filter_restricts_candidates() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        fs::write(root.join("keep.rs"), "let foo = 1;\n").expect("write rs");
        fs::write(root.join("skip.md"), "foo in markdown\n").expect("write md");

        let opts = RefactorOptions {
            find: "foo".to_string(),
            replace: "bar".to_string(),
            glob: Some("*.rs".to_string()),
            apply: false,
        };

        let changes = plan_replacements(root, &opts);
        assert_eq!(changes.len(), 1);
        assert!(changes[0].path.ends_with("keep.rs"));
    }
}
