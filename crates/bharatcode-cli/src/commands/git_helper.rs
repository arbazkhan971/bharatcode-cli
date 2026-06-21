//! `bharatcode git` — a concise, read-only repository summary.
//!
//! Prints quick context about the current Git repository: the active branch
//! (with upstream ahead/behind when available), the short HEAD commit, a count
//! and listing of working-tree changes, and the most recent commits. It is a
//! convenience helper for orienting yourself before asking BharatCode for help.
//!
//! This command is intentionally **read-only**: it only ever runs Git query
//! subcommands (`rev-parse`, `status --porcelain`, `log`, `rev-list`) and never
//! mutates the repository, the index, or any configuration.
//!
//! User-facing labels are routed through the i18n layer via [`label`], which
//! falls back to the English default when the active locale table has no entry
//! for the key. This keeps English output stable today while leaving room for
//! translations to land later without touching this file.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Result};
use console::style;

/// Options for the `git` helper command.
#[derive(Debug, Clone)]
pub struct GitOptions {
    /// Maximum number of recent commits to list.
    pub limit: usize,
    /// Optional path to a repository or a directory inside one. Defaults to the
    /// current working directory.
    pub path: Option<PathBuf>,
}

impl Default for GitOptions {
    fn default() -> Self {
        Self {
            limit: 5,
            path: None,
        }
    }
}

/// Look up a user-facing label through the i18n layer, falling back to the
/// English `default` when the active locale table has no entry for `key`.
///
/// `tr!` echoes the key back when it is missing, so an unchanged key is treated
/// as "untranslated" and the English default is used.
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// A single parsed recent commit.
struct CommitLine {
    short_hash: String,
    subject: String,
    author: String,
    age: String,
}

/// Entry point for `bharatcode git`.
pub fn handle_git(options: GitOptions) -> Result<()> {
    let dir: PathBuf = match &options.path {
        Some(p) => p.clone(),
        None => std::env::current_dir()
            .map_err(|e| anyhow!("could not determine current directory: {e}"))?,
    };

    // Confirm we are inside a work tree before doing anything else. This is the
    // only place a non-repo path produces a hard error.
    let inside = run_git(&dir, &["rev-parse", "--is-inside-work-tree"])
        .map(|s| s.trim() == "true")
        .unwrap_or(false);
    if !inside {
        return Err(anyhow!(
            "{}",
            label(
                "git.not_a_repo",
                "Not a Git repository (run this inside a repository, or pass --path)."
            )
        ));
    }

    // Branch (or detached HEAD), short commit, and optional upstream tracking.
    let branch = run_git(&dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let short_hash = run_git(&dir, &["rev-parse", "--short", "HEAD"])
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    let branch_display = if branch.is_empty() || branch == "HEAD" {
        label("git.detached", "(detached HEAD)")
    } else if let Some((ahead, behind)) = upstream_ahead_behind(&dir) {
        if ahead == 0 && behind == 0 {
            branch.clone()
        } else {
            format!(
                "{} ({} {}, {} {})",
                branch,
                label("git.ahead", "ahead"),
                ahead,
                label("git.behind", "behind"),
                behind
            )
        }
    } else {
        branch.clone()
    };

    // Working-tree status (porcelain is the stable, machine-readable form).
    let status_raw = run_git(&dir, &["status", "--porcelain"]).unwrap_or_default();
    let changes: Vec<(String, String)> = status_raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            // Porcelain v1 format: two status columns, a space, then the path.
            let (code, rest) = line.split_at(line.len().min(2));
            let path = rest.trim_start();
            (code.trim().to_string(), path.to_string())
        })
        .collect();

    // Header.
    println!();
    println!(
        "{}",
        crate::theme::heading(label("git.title", "BharatCode Git Summary"))
    );
    print_row(&label("git.branch", "Branch"), &branch_display);
    if !short_hash.is_empty() {
        print_row(&label("git.commit", "Commit"), &short_hash);
    }

    if changes.is_empty() {
        print_row(
            &label("git.status", "Status"),
            &crate::theme::success(label("git.clean", "clean working tree")).to_string(),
        );
    } else {
        print_row(
            &label("git.status", "Status"),
            &style(format!(
                "{} {}",
                changes.len(),
                label("git.changed_files", "changed file(s)")
            ))
            .yellow()
            .to_string(),
        );
    }

    // Changes listing (capped to keep the summary compact).
    if !changes.is_empty() {
        const MAX_CHANGES: usize = 20;
        println!();
        println!("{}", style(label("git.changes", "Changes:")).bold());
        for (code, path) in changes.iter().take(MAX_CHANGES) {
            let code_disp = if code.is_empty() { "?" } else { code.as_str() };
            println!("  {:<3} {}", style(code_disp).yellow(), path);
        }
        if changes.len() > MAX_CHANGES {
            let more = changes.len() - MAX_CHANGES;
            println!(
                "  {}",
                style(format!("... {} {}", more, label("git.more", "more"))).dim()
            );
        }
    }

    // Recent commits.
    let commits = recent_commits(&dir, options.limit.max(1));
    if !commits.is_empty() {
        println!();
        println!(
            "{}",
            style(label("git.recent_commits", "Recent commits:")).bold()
        );
        for c in &commits {
            println!(
                "  {}  {} {}",
                style(&c.short_hash).yellow(),
                c.subject,
                style(format!("({}, {})", c.author, c.age)).dim()
            );
        }
    }

    println!();
    Ok(())
}

fn print_row(name: &str, value: &str) {
    println!("  {:<10} {}", format!("{}:", name), value);
}

/// Run a Git query subcommand in `dir`, returning trimmed stdout on success.
///
/// Returns `None` on any spawn failure, non-zero exit, or non-UTF-8 output, so
/// callers can treat every Git query as best-effort.
fn run_git(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

/// Resolve ahead/behind counts for the current branch against its upstream.
///
/// Returns `None` when there is no configured upstream (a common, non-error
/// state for fresh local branches).
fn upstream_ahead_behind(dir: &Path) -> Option<(u64, u64)> {
    // `--count` with the `...` range yields "<behind>\t<ahead>" relative to the
    // left side; with `@{u}...HEAD` the left is upstream, so left=behind,
    // right=ahead.
    let raw = run_git(dir, &["rev-list", "--left-right", "--count", "@{u}...HEAD"])?;
    let mut parts = raw.split_whitespace();
    let behind: u64 = parts.next()?.parse().ok()?;
    let ahead: u64 = parts.next()?.parse().ok()?;
    Some((ahead, behind))
}

/// Read up to `limit` recent commits as structured records.
fn recent_commits(dir: &Path, limit: usize) -> Vec<CommitLine> {
    // Unit-separator (\x1f) delimited fields avoid clashes with commit text.
    let fmt = "--pretty=format:%h\x1f%s\x1f%an\x1f%ar";
    let count = format!("-n{limit}");
    let raw = match run_git(dir, &["log", count.as_str(), fmt]) {
        Some(r) => r,
        None => return Vec::new(),
    };
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            let mut fields = line.split('\u{1f}');
            let short_hash = fields.next()?.to_string();
            let subject = fields.next().unwrap_or("").to_string();
            let author = fields.next().unwrap_or("").to_string();
            let age = fields.next().unwrap_or("").to_string();
            Some(CommitLine {
                short_hash,
                subject,
                author,
                age,
            })
        })
        .collect()
}
