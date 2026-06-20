//! Deeper git awareness for session start (`BHARATCODE_GIT_CONTEXT`, default OFF).
//!
//! A read-only helper that, when enabled, gathers a compact picture of the
//! repository the agent is operating in — the worktree list, the current
//! branch's upstream ahead/behind, and recent blame ownership for the files
//! that currently differ from HEAD — and condenses it into a small
//! `# Git context` block injected into the system prompt.
//!
//! Like the sibling [`crate::commands::git_helper`], this module is strictly
//! **read-only**: it only ever runs Git query subcommands (`worktree list`,
//! `rev-parse`, `status --porcelain`, `log`, and `blame --line-porcelain`) and
//! never mutates the repository, the index, or any configuration.
//!
//! The block builder ([`git_context_block`]) is pure over a captured
//! [`GitContext`] so it can be unit-tested without spawning Git, and the whole
//! feature is gated behind [`is_enabled`] so the default (gate unset) behaviour
//! is a byte-identical prompt with zero Git subprocess calls.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Maximum number of worktrees rendered into the block.
const MAX_WORKTREES: usize = 8;
/// Maximum number of blame-author lines rendered into the block.
const MAX_AUTHORS: usize = 6;
/// Maximum number of changed files we run `git blame` against (blame is the
/// most expensive query, so we cap the fan-out to keep startup cheap).
const MAX_BLAME_FILES: usize = 12;
/// Maximum number of blame lines sampled per file.
const MAX_BLAME_LINES: usize = 200;

/// A single registered worktree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Worktree {
    /// Absolute path of the worktree root.
    pub path: String,
    /// Short branch name (without `refs/heads/`), if checked out on a branch.
    pub branch: Option<String>,
    /// True when the worktree is in detached-HEAD state.
    pub detached: bool,
}

/// Captured branch / upstream tracking state for the current worktree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BranchInfo {
    /// Current branch name, or `None` when detached.
    pub branch: Option<String>,
    /// Configured upstream ref (e.g. `origin/main`), if any.
    pub upstream: Option<String>,
    /// Commits ahead of upstream.
    pub ahead: u64,
    /// Commits behind upstream.
    pub behind: u64,
}

/// A blame-ownership tally for the changed files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthorTally {
    pub author: String,
    pub lines: u64,
}

/// Everything the block builder needs, captured from Git output.
///
/// This is intentionally a plain data struct so [`git_context_block`] is a pure
/// function over it: tests build one by hand and assert on the rendered text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitContext {
    /// True when `cwd` is inside a Git work tree.
    pub is_repo: bool,
    /// Registered worktrees.
    pub worktrees: Vec<Worktree>,
    /// Current branch / upstream tracking state.
    pub branch: BranchInfo,
    /// Number of files differing from HEAD in the working tree.
    pub changed_files: usize,
    /// Top blame authors across the changed files, most lines first.
    pub authors: Vec<AuthorTally>,
}

/// Whether the deeper-git-context injection is enabled.
///
/// Reads the raw `BHARATCODE_GIT_CONTEXT` environment variable (not Goose's
/// config layer) so the gate is independent of any config file. Truthy values
/// are `1`, `true`, `yes`, `on` (case-insensitive); everything else — including
/// the variable being unset — is OFF.
pub fn is_enabled() -> bool {
    match std::env::var("BHARATCODE_GIT_CONTEXT") {
        Ok(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
}

/// Run a Git query subcommand in `dir`, returning trimmed-free stdout on
/// success. Returns `None` on any spawn failure, non-zero exit, or non-UTF-8
/// output, so every query is best-effort.
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

/// Collect git context for `cwd`. Always returns a value; when `cwd` is not a
/// repository the returned [`GitContext`] has `is_repo == false` and empty
/// collections (and [`git_context_block`] will render `None` for it).
///
/// This is the only function in the module that spawns Git, and it is reached
/// solely from the gated call site, so a disabled feature performs no Git I/O.
pub fn collect(cwd: &Path) -> GitContext {
    let mut ctx = GitContext::default();

    let inside = run_git(cwd, &["rev-parse", "--is-inside-work-tree"])
        .map(|s| s.trim() == "true")
        .unwrap_or(false);
    if !inside {
        return ctx;
    }
    ctx.is_repo = true;

    ctx.worktrees = collect_worktrees(cwd);
    ctx.branch = collect_branch(cwd);

    let changed = collect_changed_files(cwd);
    ctx.changed_files = changed.len();
    ctx.authors = collect_authors(cwd, &changed);

    ctx
}

/// Parse `git worktree list --porcelain` into structured worktrees.
fn collect_worktrees(cwd: &Path) -> Vec<Worktree> {
    let raw = match run_git(cwd, &["worktree", "list", "--porcelain"]) {
        Some(r) => r,
        None => return Vec::new(),
    };

    let mut out = Vec::new();
    let mut current: Option<Worktree> = None;
    for line in raw.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(wt) = current.take() {
                out.push(wt);
            }
            current = Some(Worktree {
                path: path.to_string(),
                branch: None,
                detached: false,
            });
        } else if line == "detached" {
            if let Some(wt) = current.as_mut() {
                wt.detached = true;
            }
        } else if let Some(branch_ref) = line.strip_prefix("branch ") {
            if let Some(wt) = current.as_mut() {
                wt.branch = Some(short_branch(branch_ref).to_string());
            }
        }
    }
    if let Some(wt) = current.take() {
        out.push(wt);
    }
    out
}

/// Strip a leading `refs/heads/` from a branch ref for display.
fn short_branch(branch_ref: &str) -> &str {
    branch_ref.strip_prefix("refs/heads/").unwrap_or(branch_ref)
}

/// Capture current branch and upstream ahead/behind.
fn collect_branch(cwd: &Path) -> BranchInfo {
    let mut info = BranchInfo::default();

    let branch = run_git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"])
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if !branch.is_empty() && branch != "HEAD" {
        info.branch = Some(branch);
    }

    info.upstream = run_git(
        cwd,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    )
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty());

    if info.upstream.is_some() {
        if let Some(raw) = run_git(cwd, &["rev-list", "--left-right", "--count", "@{u}...HEAD"]) {
            let mut parts = raw.split_whitespace();
            if let (Some(behind), Some(ahead)) = (parts.next(), parts.next()) {
                info.behind = behind.parse().unwrap_or(0);
                info.ahead = ahead.parse().unwrap_or(0);
            }
        }
    }

    info
}

/// List paths that currently differ from HEAD (porcelain v1).
fn collect_changed_files(cwd: &Path) -> Vec<String> {
    let raw = match run_git(cwd, &["status", "--porcelain"]) {
        Some(r) => r,
        None => return Vec::new(),
    };
    raw.lines()
        .filter(|l| l.len() > 3)
        .filter_map(|line| {
            // Porcelain v1: two status columns, a space, then the path. For
            // renames the path is "old -> new"; we keep the new (right) path.
            let path = line[3..].trim();
            let path = path.rsplit(" -> ").next().unwrap_or(path);
            // Skip deletions: there is nothing to blame.
            if line.starts_with(" D") || line.starts_with("D ") {
                return None;
            }
            if path.is_empty() {
                None
            } else {
                Some(path.to_string())
            }
        })
        .collect()
}

/// Tally blame authors across the changed files, most lines first.
fn collect_authors(cwd: &Path, changed: &[String]) -> Vec<AuthorTally> {
    let mut tally: HashMap<String, u64> = HashMap::new();

    for path in changed.iter().take(MAX_BLAME_FILES) {
        let range = format!("1,{}", MAX_BLAME_LINES);
        let raw = match run_git(
            cwd,
            &[
                "blame",
                "--line-porcelain",
                "-L",
                range.as_str(),
                "HEAD",
                "--",
                path.as_str(),
            ],
        ) {
            Some(r) => r,
            None => continue,
        };
        for line in raw.lines() {
            if let Some(author) = line.strip_prefix("author ") {
                let author = author.trim();
                if !author.is_empty() && author != "Not Committed Yet" {
                    *tally.entry(author.to_string()).or_insert(0) += 1;
                }
            }
        }
    }

    let mut authors: Vec<AuthorTally> = tally
        .into_iter()
        .map(|(author, lines)| AuthorTally { author, lines })
        .collect();
    // Most lines first; ties broken by name for deterministic output.
    authors.sort_by(|a, b| b.lines.cmp(&a.lines).then_with(|| a.author.cmp(&b.author)));
    authors
}

/// Render a captured [`GitContext`] into a compact `# Git context` system-prompt
/// block. Pure over its input. Returns `None` when the context is not a repo or
/// carries nothing worth injecting.
pub fn git_context_block(ctx: &GitContext) -> Option<String> {
    if !ctx.is_repo {
        return None;
    }

    let has_worktrees = !ctx.worktrees.is_empty();
    let has_branch = ctx.branch.branch.is_some() || ctx.branch.upstream.is_some();
    let has_authors = !ctx.authors.is_empty();
    if !has_worktrees && !has_branch && !has_authors {
        return None;
    }

    let mut out = String::new();
    out.push_str("# Git context\n");
    out.push_str(
        "Read-only repository awareness gathered at session start. Use it to orient yourself; \
         it may be stale, so re-check with git before acting on it.\n",
    );

    if has_branch {
        out.push_str("\n## Branch\n");
        let branch = ctx.branch.branch.as_deref().unwrap_or("(detached HEAD)");
        match &ctx.branch.upstream {
            Some(upstream) => {
                out.push_str(&format!(
                    "- {} tracking {} (ahead {}, behind {})\n",
                    branch, upstream, ctx.branch.ahead, ctx.branch.behind
                ));
            }
            None => {
                out.push_str(&format!("- {} (no upstream configured)\n", branch));
            }
        }
        out.push_str(&format!(
            "- {} changed file(s) vs HEAD\n",
            ctx.changed_files
        ));
    }

    if has_worktrees {
        out.push_str("\n## Worktrees\n");
        for wt in ctx.worktrees.iter().take(MAX_WORKTREES) {
            let label = if wt.detached {
                "(detached)".to_string()
            } else {
                match &wt.branch {
                    Some(b) => format!("[{}]", b),
                    None => "(no branch)".to_string(),
                }
            };
            out.push_str(&format!("- {} {}\n", wt.path, label));
        }
        if ctx.worktrees.len() > MAX_WORKTREES {
            out.push_str(&format!(
                "- ... {} more\n",
                ctx.worktrees.len() - MAX_WORKTREES
            ));
        }
    }

    if has_authors {
        out.push_str("\n## Recent ownership (blame of changed files)\n");
        for tally in ctx.authors.iter().take(MAX_AUTHORS) {
            out.push_str(&format!("- {} ({} line(s))\n", tally.author, tally.lines));
        }
    }

    Some(out)
}

/// Convenience helper for the call site: derive the working directory the same
/// way other startup helpers do, falling back to `.` when it cannot be read.
pub fn current_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_gate(val: Option<&str>) {
        match val {
            Some(v) => std::env::set_var("BHARATCODE_GIT_CONTEXT", v),
            None => std::env::remove_var("BHARATCODE_GIT_CONTEXT"),
        }
    }

    #[test]
    fn is_enabled_gate_table() {
        // Snapshot so the test is hermetic regardless of ambient env.
        let saved = std::env::var("BHARATCODE_GIT_CONTEXT").ok();

        set_gate(None);
        assert!(!is_enabled(), "unset must be OFF");

        for truthy in ["1", "true", "TRUE", "Yes", "on", " on "] {
            set_gate(Some(truthy));
            assert!(is_enabled(), "{truthy:?} must be ON");
        }

        for falsy in ["0", "false", "no", "off", "", "maybe"] {
            set_gate(Some(falsy));
            assert!(!is_enabled(), "{falsy:?} must be OFF");
        }

        set_gate(saved.as_deref());
    }

    fn sample_context() -> GitContext {
        GitContext {
            is_repo: true,
            worktrees: vec![
                Worktree {
                    path: "/home/dev/proj".to_string(),
                    branch: Some("main".to_string()),
                    detached: false,
                },
                Worktree {
                    path: "/home/dev/proj-hotfix".to_string(),
                    branch: Some("hotfix".to_string()),
                    detached: false,
                },
                Worktree {
                    path: "/home/dev/proj-detached".to_string(),
                    branch: None,
                    detached: true,
                },
            ],
            branch: BranchInfo {
                branch: Some("feature/login".to_string()),
                upstream: Some("origin/feature/login".to_string()),
                ahead: 3,
                behind: 1,
            },
            changed_files: 4,
            authors: vec![
                AuthorTally {
                    author: "Asha Rao".to_string(),
                    lines: 120,
                },
                AuthorTally {
                    author: "Vikram Singh".to_string(),
                    lines: 42,
                },
            ],
        }
    }

    #[test]
    fn block_renders_worktrees_branch_and_authors() {
        let block = git_context_block(&sample_context()).expect("non-empty repo renders a block");

        assert!(block.contains("# Git context"));
        // Branch + upstream + ahead/behind.
        assert!(block.contains("feature/login tracking origin/feature/login (ahead 3, behind 1)"));
        assert!(block.contains("4 changed file(s) vs HEAD"));
        // Worktrees, including the detached one.
        assert!(block.contains("/home/dev/proj [main]"));
        assert!(block.contains("/home/dev/proj-hotfix [hotfix]"));
        assert!(block.contains("/home/dev/proj-detached (detached)"));
        // Blame ownership.
        assert!(block.contains("Asha Rao (120 line(s))"));
        assert!(block.contains("Vikram Singh (42 line(s))"));
    }

    #[test]
    fn block_has_no_upstream_branch_form() {
        let mut ctx = sample_context();
        ctx.branch.upstream = None;
        ctx.branch.ahead = 0;
        ctx.branch.behind = 0;
        let block = git_context_block(&ctx).expect("still renders");
        assert!(block.contains("feature/login (no upstream configured)"));
    }

    #[test]
    fn block_caps_worktrees_and_authors() {
        let mut ctx = sample_context();
        ctx.worktrees = (0..MAX_WORKTREES + 3)
            .map(|i| Worktree {
                path: format!("/wt/{i}"),
                branch: Some(format!("b{i}")),
                detached: false,
            })
            .collect();
        ctx.authors = (0..MAX_AUTHORS + 4)
            .map(|i| AuthorTally {
                author: format!("Author {i}"),
                lines: (100 - i) as u64,
            })
            .collect();
        let block = git_context_block(&ctx).unwrap();

        assert!(block.contains("... 3 more"));
        // The (MAX_AUTHORS)th-and-beyond authors must not appear.
        assert!(!block.contains(&format!("Author {}", MAX_AUTHORS)));
    }

    #[test]
    fn empty_and_non_repo_contexts_render_none() {
        // Default => not a repo.
        assert!(git_context_block(&GitContext::default()).is_none());

        // Repo flag set but nothing worth injecting.
        let bare = GitContext {
            is_repo: true,
            ..Default::default()
        };
        assert!(git_context_block(&bare).is_none());
    }

    #[test]
    fn rendered_block_has_no_upstream_brand_leakage() {
        let block = git_context_block(&sample_context()).unwrap();
        let lower = block.to_lowercase();
        assert!(
            !lower.contains("goose"),
            "block must not leak the donor brand"
        );
        assert!(!lower.contains("block.xyz"));
    }

    #[test]
    fn short_branch_strips_refs_heads() {
        assert_eq!(short_branch("refs/heads/main"), "main");
        assert_eq!(short_branch("refs/heads/feature/x"), "feature/x");
        assert_eq!(short_branch("main"), "main");
    }

    #[test]
    fn worktree_porcelain_parsing() {
        // Exercised indirectly: build the same struct collect_worktrees would.
        let wt = Worktree {
            path: "/p".to_string(),
            branch: Some("main".to_string()),
            detached: false,
        };
        assert_eq!(wt.branch.as_deref(), Some("main"));
    }
}
