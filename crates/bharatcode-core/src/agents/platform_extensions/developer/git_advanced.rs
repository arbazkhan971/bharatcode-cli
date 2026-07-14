//! Deep-git inspection: the `git_advanced` developer tool.
//!
//! Exposes a handful of read-only "deep git" operations the agent can call
//! without resorting to a raw shell command:
//!
//! * `worktree_list` — every linked/main work tree (path, HEAD, branch, flags),
//!   parsed from `git worktree list --porcelain`,
//! * `blame <file> [<range>]` — per-line authorship for a file (or a 1-based
//!   `start,end` line range), parsed from `git blame --porcelain`,
//! * `pr_context` — the current branch, its upstream, ahead/behind counts, and
//!   the files changed against the merge-base with that upstream — i.e. the
//!   review surface of a would-be pull request.
//!
//! Every operation shells out to a *read-only* Git query subcommand
//! (`worktree list`, `blame`, `rev-parse`, `merge-base`, `rev-list`, `diff
//! --name-status`). The tool never mutates the repository, the index, the
//! working tree, or any configuration, and it never touches the network, so it
//! is safe by default and carries no opt-in env gate (mirroring `tree` /
//! `read_lines` / `editor_locator`). Output is returned both as a human-readable
//! text summary and as structured JSON for programmatic consumption.

use std::path::{Path, PathBuf};
use std::process::Command;

use rmcp::model::{CallToolResult, Content};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::mcp_utils::ToolResult;

/// Input schema for the `git_advanced` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GitAdvancedParams {
    /// Operation to run: `worktree_list`, `blame`, or `pr_context`.
    pub op: String,
    /// File to blame (required for `op = "blame"`). Resolved relative to the
    /// working directory when not absolute. Ignored by the other operations.
    #[serde(default)]
    pub file: Option<String>,
    /// Optional 1-based inclusive line range for `blame`, written as
    /// `start,end` (e.g. `"10,40"`) or a single line `"10"`. Omitted means the
    /// whole file.
    #[serde(default)]
    pub range: Option<String>,
}

/// One linked/main work tree as reported by `git worktree list --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    /// Absolute path to the work tree root.
    pub path: String,
    /// Checked-out commit (full SHA), or empty when the tree is bare.
    pub head: String,
    /// Branch ref (e.g. `refs/heads/main`), or empty when detached/bare.
    pub branch: String,
    /// True when this entry is the bare repository itself.
    pub bare: bool,
    /// True when the work tree is in detached-HEAD state.
    pub detached: bool,
    /// True when the work tree is administratively locked.
    pub locked: bool,
}

/// One `(commit, line)` pair from a porcelain blame, in file order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameLine {
    /// Full commit SHA that last touched this line.
    pub commit: String,
    /// 1-based line number in the final file.
    pub line: u32,
    /// The line's text content (without the trailing newline).
    pub content: String,
}

/// Run a read-only Git query subcommand in `dir`, returning trimmed-free stdout
/// on success.
///
/// Returns `Err(message)` on any spawn failure, non-zero exit, or non-UTF-8
/// output, surfacing git's stderr so the caller can produce a clean tool error
/// instead of panicking. Mirrors the `run_git` helper in the CLI git helper but
/// is kept local to this module so the developer extension has no dependency on
/// the CLI crate.
fn run_git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let msg = stderr.trim();
        return Err(if msg.is_empty() {
            format!("git {} failed", args.first().copied().unwrap_or("command"))
        } else {
            msg.to_string()
        });
    }
    String::from_utf8(output.stdout).map_err(|_| "git produced non-UTF-8 output".to_string())
}

/// Parse `git worktree list --porcelain` output into structured entries.
///
/// The porcelain format groups one work tree per blank-line-separated stanza,
/// each beginning with a `worktree <path>` line followed by optional
/// `HEAD <sha>`, `branch <ref>`, and the bare/detached/locked flag lines.
pub fn parse_worktree_porcelain(stdout: &str) -> Vec<WorktreeEntry> {
    let mut entries = Vec::new();
    let mut current: Option<WorktreeEntry> = None;

    let flush = |current: &mut Option<WorktreeEntry>, out: &mut Vec<WorktreeEntry>| {
        if let Some(entry) = current.take() {
            out.push(entry);
        }
    };

    for line in stdout.lines() {
        if line.is_empty() {
            flush(&mut current, &mut entries);
            continue;
        }
        let (key, rest) = match line.split_once(' ') {
            Some((k, r)) => (k, r),
            None => (line, ""),
        };
        match key {
            "worktree" => {
                flush(&mut current, &mut entries);
                current = Some(WorktreeEntry {
                    path: rest.to_string(),
                    head: String::new(),
                    branch: String::new(),
                    bare: false,
                    detached: false,
                    locked: false,
                });
            }
            "HEAD" => {
                if let Some(e) = current.as_mut() {
                    e.head = rest.to_string();
                }
            }
            "branch" => {
                if let Some(e) = current.as_mut() {
                    e.branch = rest.to_string();
                }
            }
            "bare" => {
                if let Some(e) = current.as_mut() {
                    e.bare = true;
                }
            }
            "detached" => {
                if let Some(e) = current.as_mut() {
                    e.detached = true;
                }
            }
            "locked" => {
                if let Some(e) = current.as_mut() {
                    e.locked = true;
                }
            }
            _ => {}
        }
    }
    flush(&mut current, &mut entries);
    entries
}

/// Parse `git blame --porcelain` output into `(commit, line)` pairs with text.
///
/// In the porcelain format each line group starts with a header
/// `<sha> <orig-line> <final-line> [<num-lines>]`, is followed by optional
/// metadata lines (`author`, `summary`, …), and ends with the actual file line
/// prefixed by a single TAB.
pub fn parse_blame_porcelain(stdout: &str) -> Vec<BlameLine> {
    let mut result = Vec::new();
    let mut pending: Option<(String, u32)> = None;

    for line in stdout.lines() {
        if let Some(content) = line.strip_prefix('\t') {
            if let Some((commit, final_line)) = pending.take() {
                result.push(BlameLine {
                    commit,
                    line: final_line,
                    content: content.to_string(),
                });
            }
            continue;
        }

        // A header line begins with a 40-hex SHA followed by line numbers.
        let mut parts = line.split(' ');
        if let (Some(sha), Some(_orig), Some(final_no)) = (parts.next(), parts.next(), parts.next())
        {
            let is_sha = sha.len() >= 40 && sha.bytes().all(|b| b.is_ascii_hexdigit());
            if is_sha {
                if let Ok(final_line) = final_no.parse::<u32>() {
                    pending = Some((sha.to_string(), final_line));
                }
            }
        }
    }
    result
}

/// Validate and normalise a `start,end` (or single-line) range into the
/// `-L start,end` argument git's blame expects. Returns `Err` for malformed or
/// non-positive ranges so the caller can produce a clean error.
fn normalise_range(range: &str) -> Result<String, String> {
    let trimmed = range.trim();
    let (start, end) = match trimmed.split_once(',') {
        Some((s, e)) => (s.trim(), e.trim()),
        None => (trimmed, trimmed),
    };
    let start: u32 = start
        .parse()
        .map_err(|_| format!("invalid range start: {start:?}"))?;
    let end: u32 = end
        .parse()
        .map_err(|_| format!("invalid range end: {end:?}"))?;
    if start == 0 || end == 0 {
        return Err("range line numbers are 1-based and must be >= 1".to_string());
    }
    if end < start {
        return Err(format!("range end ({end}) precedes start ({start})"));
    }
    Ok(format!("{start},{end}"))
}

/// Resolve `path_str` against `working_dir` when it is not already absolute.
fn resolve_path(path_str: &str, working_dir: Option<&Path>) -> PathBuf {
    let path = PathBuf::from(path_str);
    if path.is_absolute() {
        path
    } else {
        working_dir
            .map(Path::to_path_buf)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."))
            .join(path)
    }
}

/// Determine the directory to run git in: the working dir, else the cwd.
fn git_dir(working_dir: Option<&Path>) -> PathBuf {
    working_dir
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Entry point for the `git_advanced` tool.
///
/// Dispatches on `params.op`; an unrecognised op yields a clean tool error
/// (never a panic). All work is read-only.
pub fn run_git_advanced(
    params: GitAdvancedParams,
    working_dir: Option<&Path>,
) -> ToolResult<CallToolResult> {
    let dir = git_dir(working_dir);
    let result = match params.op.as_str() {
        "worktree_list" => op_worktree_list(&dir),
        "blame" => op_blame(&params, working_dir, &dir),
        "pr_context" => op_pr_context(&dir),
        other => Err(format!(
            "Unknown op: {other:?} (expected one of: worktree_list, blame, pr_context)"
        )),
    };

    Ok(match result {
        Ok(result) => result,
        Err(message) => error_result(&message),
    })
}

fn op_worktree_list(dir: &Path) -> Result<CallToolResult, String> {
    let stdout = run_git(dir, &["worktree", "list", "--porcelain"])?;
    let entries = parse_worktree_porcelain(&stdout);

    let mut summary = format!("{} work tree(s):", entries.len());
    for e in &entries {
        let mut flags = Vec::new();
        if e.bare {
            flags.push("bare");
        }
        if e.detached {
            flags.push("detached");
        }
        if e.locked {
            flags.push("locked");
        }
        let branch = if e.branch.is_empty() {
            "-".to_string()
        } else {
            e.branch.clone()
        };
        let head = if e.head.is_empty() {
            "-"
        } else {
            e.head.get(..12).unwrap_or(&e.head)
        };
        let suffix = if flags.is_empty() {
            String::new()
        } else {
            format!(" [{}]", flags.join(","))
        };
        summary.push_str(&format!("\n  {}  {head}  {branch}{suffix}", e.path));
    }

    let structured = json!({
        "op": "worktree_list",
        "worktrees": entries
            .iter()
            .map(|e| json!({
                "path": e.path,
                "head": e.head,
                "branch": e.branch,
                "bare": e.bare,
                "detached": e.detached,
                "locked": e.locked,
            }))
            .collect::<Vec<Value>>(),
    });

    Ok(success_result(summary, structured))
}

fn op_blame(
    params: &GitAdvancedParams,
    working_dir: Option<&Path>,
    dir: &Path,
) -> Result<CallToolResult, String> {
    let file = match &params.file {
        Some(f) if !f.is_empty() => f,
        _ => return Err("`blame` requires a `file` argument".to_string()),
    };
    let resolved = resolve_path(file, working_dir);
    let resolved_str = resolved.display().to_string();

    let mut args: Vec<String> = vec!["blame".to_string(), "--porcelain".to_string()];
    if let Some(range) = &params.range {
        let normalised = normalise_range(range)?;
        args.push("-L".to_string());
        args.push(normalised);
    }
    args.push("--".to_string());
    args.push(resolved_str.clone());

    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let stdout = run_git(dir, &arg_refs)?;
    let lines = parse_blame_porcelain(&stdout);

    let mut summary = format!("blame for {resolved_str} ({} line(s)):", lines.len());
    const MAX_PREVIEW: usize = 40;
    for bl in lines.iter().take(MAX_PREVIEW) {
        let short = bl.commit.get(..8).unwrap_or(&bl.commit);
        summary.push_str(&format!("\n  {short} {:>5}  {}", bl.line, bl.content));
    }
    if lines.len() > MAX_PREVIEW {
        summary.push_str(&format!(
            "\n  ... {} more line(s)",
            lines.len() - MAX_PREVIEW
        ));
    }

    let structured = json!({
        "op": "blame",
        "file": resolved_str,
        "range": params.range,
        "lines": lines
            .iter()
            .map(|bl| json!({
                "commit": bl.commit,
                "line": bl.line,
                "content": bl.content,
            }))
            .collect::<Vec<Value>>(),
    });

    Ok(success_result(summary, structured))
}

fn op_pr_context(dir: &Path) -> Result<CallToolResult, String> {
    let branch = run_git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    // Upstream tracking ref, if any. A missing upstream is a normal, non-error
    // state for fresh local branches, so we degrade gracefully.
    let upstream = run_git(
        dir,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    )
    .ok()
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty());

    let (ahead, behind) = match upstream {
        Some(_) => match run_git(dir, &["rev-list", "--left-right", "--count", "@{u}...HEAD"]) {
            Ok(raw) => {
                let mut parts = raw.split_whitespace();
                let behind = parts
                    .next()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0);
                let ahead = parts
                    .next()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0);
                (Some(ahead), Some(behind))
            }
            Err(_) => (None, None),
        },
        None => (None, None),
    };

    // Files changed versus the merge-base with the upstream (the PR review
    // surface). Without an upstream we fall back to the working-tree diff
    // against HEAD so the op still yields something useful.
    let changed_files: Vec<(String, String)> = match &upstream {
        Some(up) => match run_git(dir, &["merge-base", "HEAD", up]) {
            Ok(base) => {
                let base = base.trim().to_string();
                diff_name_status(dir, &[&base, "HEAD"])
            }
            Err(_) => Vec::new(),
        },
        None => diff_name_status(dir, &["HEAD"]),
    };

    let mut summary = format!(
        "branch: {}",
        if branch.is_empty() {
            "(unknown)"
        } else {
            &branch
        }
    );
    match &upstream {
        Some(up) => {
            summary.push_str(&format!("\nupstream: {up}"));
            if let (Some(a), Some(b)) = (ahead, behind) {
                summary.push_str(&format!("\nahead {a}, behind {b}"));
            }
        }
        None => summary.push_str("\nupstream: (none)"),
    }
    summary.push_str(&format!("\nchanged files ({}):", changed_files.len()));
    for (status, path) in changed_files.iter().take(40) {
        summary.push_str(&format!("\n  {status:<3} {path}"));
    }
    if changed_files.len() > 40 {
        summary.push_str(&format!("\n  ... {} more", changed_files.len() - 40));
    }

    let structured = json!({
        "op": "pr_context",
        "branch": branch,
        "upstream": upstream,
        "ahead": ahead,
        "behind": behind,
        "changed_files": changed_files
            .iter()
            .map(|(status, path)| json!({ "status": status, "path": path }))
            .collect::<Vec<Value>>(),
    });

    Ok(success_result(summary, structured))
}

/// Run `git diff --name-status <revs>` and parse the `STATUS\tPATH` lines.
fn diff_name_status(dir: &Path, revs: &[&str]) -> Vec<(String, String)> {
    let mut args = vec!["diff", "--name-status"];
    args.extend_from_slice(revs);
    match run_git(dir, &args) {
        Ok(stdout) => stdout
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|line| {
                let mut parts = line.splitn(2, '\t');
                let status = parts.next()?.trim().to_string();
                let path = parts.next()?.trim().to_string();
                Some((status, path))
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn success_result(summary: String, structured: Value) -> CallToolResult {
    let mut result = CallToolResult::success(vec![Content::text(summary).with_priority(0.0)]);
    result.structured_content = Some(structured);
    result
}

fn error_result(message: &str) -> CallToolResult {
    CallToolResult::error(vec![
        Content::text(format!("Error: {message}")).with_priority(0.0)
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_worktree_porcelain_fixture() {
        // A representative `git worktree list --porcelain` dump: a main tree on
        // a branch, a detached linked tree, a locked linked tree, and a bare
        // repository stanza.
        let fixture = "\
worktree /home/user/project
HEAD 1111111111111111111111111111111111111111
branch refs/heads/main

worktree /home/user/project-wt
HEAD 2222222222222222222222222222222222222222
detached

worktree /home/user/project-lock
HEAD 3333333333333333333333333333333333333333
branch refs/heads/feature
locked

worktree /home/user/bare-repo
bare
";
        let entries = parse_worktree_porcelain(fixture);
        assert_eq!(entries.len(), 4);

        assert_eq!(entries[0].path, "/home/user/project");
        assert_eq!(entries[0].head, "1111111111111111111111111111111111111111");
        assert_eq!(entries[0].branch, "refs/heads/main");
        assert!(!entries[0].detached);
        assert!(!entries[0].bare);

        assert!(entries[1].detached);
        assert!(entries[1].branch.is_empty());

        assert!(entries[2].locked);
        assert_eq!(entries[2].branch, "refs/heads/feature");

        assert!(entries[3].bare);
        assert_eq!(entries[3].path, "/home/user/bare-repo");
    }

    #[test]
    fn parses_blame_porcelain_into_commit_line_pairs() {
        // A two-line porcelain blame: the first line group carries full header
        // metadata, the second reuses the same commit with a terse header.
        let fixture = "\
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa 1 1 2
author Ada Lovelace
author-mail <ada@example.com>
summary first commit
filename src/lib.rs
\tfn main() {
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa 2 2
\t    println!(\"hi\");
";
        let lines = parse_blame_porcelain(fixture);
        assert_eq!(lines.len(), 2);

        assert_eq!(lines[0].commit, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert_eq!(lines[0].line, 1);
        assert_eq!(lines[0].content, "fn main() {");

        assert_eq!(lines[1].commit, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert_eq!(lines[1].line, 2);
        assert_eq!(lines[1].content, "    println!(\"hi\");");
    }

    #[test]
    fn unknown_op_returns_clean_error_not_panic() {
        let params = GitAdvancedParams {
            op: "frobnicate".to_string(),
            file: None,
            range: None,
        };
        let result = run_git_advanced(params, None).unwrap();
        assert_eq!(result.is_error, Some(true));

        let text = match &result.content[0].raw {
            rmcp::model::RawContent::Text(t) => t.text.clone(),
            _ => panic!("expected text content"),
        };
        assert!(text.contains("Unknown op"), "message was: {text}");
        assert!(text.contains("frobnicate"), "message was: {text}");
        // No upstream-brand leakage in any user-facing string.
        assert!(!text.to_lowercase().contains("goose"));
    }

    #[test]
    fn blame_without_file_errors_cleanly() {
        let params = GitAdvancedParams {
            op: "blame".to_string(),
            file: None,
            range: None,
        };
        let result = run_git_advanced(params, None).unwrap();
        assert_eq!(result.is_error, Some(true));
    }

    #[test]
    fn normalise_range_accepts_pair_and_single() {
        assert_eq!(normalise_range("10,40").unwrap(), "10,40");
        assert_eq!(normalise_range(" 7 ").unwrap(), "7,7");
    }

    #[test]
    fn normalise_range_rejects_bad_input() {
        assert!(normalise_range("0,5").is_err());
        assert!(normalise_range("40,10").is_err());
        assert!(normalise_range("abc").is_err());
    }
}
