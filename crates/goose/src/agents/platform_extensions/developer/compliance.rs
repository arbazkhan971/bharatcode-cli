//! Final compliance + trademark gate: the read-only `verify_compliance` tool.
//!
//! This is the capstone audit surface. It performs two independent checks over
//! the repository and folds them into a single structured pass/fail report:
//!
//! 1. **Apache-2.0 obligations** — confirms the five license/attribution files a
//!    derivative work must ship are present *and* non-empty:
//!    `LICENSE`, `LICENSES/LICENSE-goose`, `LICENSES/LICENSE-codex`, `NOTICE`,
//!    and `MODIFICATIONS.md`. A missing or empty file is a hard `Fail`.
//!
//! 2. **Residual trademark leakage** — greps a *bounded* set of user-facing
//!    surfaces (`README.md`, `NOTICE`, `MODIFICATIONS.md`) for upstream company
//!    marks (`goose` / `Block` / `Codex` / `OpenAI`). Matches that correspond to
//!    documented internal Rust identifiers (`GooseMode`, `GooseClient`,
//!    `goose_*` field/fn names, `ContentBlock`) or the ordinary English word
//!    "block" are allow-listed per the leak-gate rules and never counted as
//!    leaks. NOTICE/MODIFICATIONS.md legitimately *name* the upstream projects
//!    for attribution, so a hit there is reported as an informational `Warn`
//!    rather than a `Fail`; an unexpected hit in `README.md` is a `Fail`.
//!
//! The tool is **strictly read-only**: it opens files for reading only and never
//! writes, renames, or deletes anything. It carries no opt-in env gate and is
//! always available, mirroring `read_lines` / `editor_locator` / `git_advanced`.

use std::path::{Path, PathBuf};

use rmcp::model::{CallToolResult, Content, JsonObject, Tool, ToolAnnotations};
use schemars::{schema_for, JsonSchema};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::mcp_utils::ToolResult;

/// The five files an Apache-2.0 derivative work must ship, relative to the
/// repository root. Each must exist and be non-empty for compliance.
pub const REQUIRED_FILES: &[&str] = &[
    "LICENSE",
    "LICENSES/LICENSE-goose",
    "LICENSES/LICENSE-codex",
    "NOTICE",
    "MODIFICATIONS.md",
];

/// User-facing surfaces scanned for residual trademark leakage. `README.md` is
/// the front door (any company mark there is a `Fail`); `NOTICE` /
/// `MODIFICATIONS.md` legitimately name upstream projects for attribution, so a
/// mark there is an informational `Warn`.
pub const SCAN_SURFACES: &[&str] = &["README.md", "NOTICE", "MODIFICATIONS.md"];

/// Documented internal identifiers / words that are *not* trademark leaks, per
/// the iterations.md leak-gate rules. Matching is case-insensitive against whole
/// word-tokens; `goose_*` snake_case identifiers and the bare English word
/// "block" are handled by dedicated rules in [`is_allowed_token`].
pub const DEFAULT_ALLOW: &[&str] = &[
    "GooseMode",
    "GooseClient",
    "ContentBlock",
    "block",
];

/// The company marks scanned for. Lowercased; matching is case-insensitive.
const MARKS: &[&str] = &["goose", "block", "codex", "openai"];

/// Pass/fail classification for a single check or the overall report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// The obligation is met / no unexpected leak found.
    Ok,
    /// A non-fatal advisory (e.g. an attribution file legitimately names the
    /// upstream project).
    Warn,
    /// A hard failure (a required file is missing/empty, or a leak appears on a
    /// front-door surface).
    Fail,
}

impl Status {
    /// Lowercase wire name for structured output.
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warn => "warn",
            Status::Fail => "fail",
        }
    }

    /// Glyph for the human-readable summary.
    fn glyph(self) -> char {
        match self {
            Status::Ok => '+',
            Status::Warn => '~',
            Status::Fail => 'x',
        }
    }

    /// Combine two statuses, keeping the more severe one
    /// (`Fail` > `Warn` > `Ok`).
    fn worse(self, other: Status) -> Status {
        match (self, other) {
            (Status::Fail, _) | (_, Status::Fail) => Status::Fail,
            (Status::Warn, _) | (_, Status::Warn) => Status::Warn,
            _ => Status::Ok,
        }
    }
}

/// Result of checking one required compliance file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCheck {
    /// Repo-root-relative file name.
    pub name: String,
    /// Whether the file exists on disk.
    pub present: bool,
    /// Whether the file is non-empty (only meaningful when `present`).
    pub non_empty: bool,
    /// `Ok` when present and non-empty, otherwise `Fail`.
    pub status: Status,
}

/// One residual-trademark hit found while scanning a surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkHit {
    /// The company mark that matched (lowercased: `goose`/`block`/...).
    pub mark: String,
    /// 1-based line number within the scanned text.
    pub line: usize,
    /// The exact token (verbatim case) that triggered the hit.
    pub token: String,
}

/// Per-surface scan outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceScan {
    /// Repo-root-relative surface name.
    pub name: String,
    /// Whether the surface file existed and was scanned.
    pub scanned: bool,
    /// Residual marks found (after the allow-list filter).
    pub hits: Vec<MarkHit>,
    /// `Ok` / `Warn` / `Fail` for this surface.
    pub status: Status,
}

/// The full structured report returned by [`verify_compliance`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComplianceReport {
    /// Per-file compliance results, in [`REQUIRED_FILES`] order.
    pub files: Vec<FileCheck>,
    /// Per-surface trademark scan results, in [`SCAN_SURFACES`] order.
    pub surfaces: Vec<SurfaceScan>,
    /// Worst status across all files and surfaces.
    pub overall: Status,
}

/// Pure core: classify a list of `(file_name, present_and_non_empty)` pairs.
///
/// A file is `Ok` only when its flag is `true` (present *and* non-empty);
/// otherwise it is `Fail`. The caller supplies the on-disk facts so this core is
/// trivially unit-testable without touching the filesystem.
pub fn check_required_files(present: &[(&str, bool)]) -> Vec<FileCheck> {
    present
        .iter()
        .map(|(name, non_empty)| FileCheck {
            name: (*name).to_string(),
            present: *non_empty,
            non_empty: *non_empty,
            status: if *non_empty { Status::Ok } else { Status::Fail },
        })
        .collect()
}

/// Lowercase the leading ASCII-alphanumeric/underscore prefix shared by a token
/// and a mark, used only for prefix comparisons.
fn lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

/// Decide whether `token` (already known to contain `mark`) is allow-listed,
/// i.e. *not* a real trademark leak, per the leak-gate documentation.
///
/// The rules:
/// * an exact (case-insensitive) match against an `allow` entry — covers
///   `GooseMode`, `GooseClient`, `ContentBlock`, and the bare word `block`;
/// * any `goose_*` snake_case identifier (e.g. `goose_mode`, `goose_providers`)
///   — these are internal Rust field/fn/crate-path names;
/// * for the `block` mark only: the ordinary English word `block` and its
///   derivatives (`blocked`, `blocking`, `unblock`, `blockchain`, …) and
///   `*Block`/`*block` compound identifiers. The brand only ever appears as the
///   standalone capitalized token `Block` ("Block, Inc."), so that exact form is
///   the only `block`-mark hit treated as a leak.
fn is_allowed_token(token: &str, mark: &str, allow: &[&str]) -> bool {
    let lowered = lower(token);
    if allow.iter().any(|a| lower(a) == lowered) {
        return true;
    }
    // Internal snake_case `goose_*` identifiers (goose_mode, goose_providers...).
    if lowered.starts_with("goose_") {
        return true;
    }
    // The `block` mark: allow the English word family and `*Block` compounds;
    // only the bare brand-cased `Block` token is a leak.
    if mark == "block" {
        return token != "Block";
    }
    false
}

/// Split a line into identifier-ish tokens: maximal runs of ASCII
/// alphanumerics plus `_`. This keeps `goose_mode` and `GooseMode` whole so the
/// allow-list can reason about them, while still isolating a bare `goose` in
/// `goose configure`.
fn tokenize(line: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let bytes = line.as_bytes();
    let mut start = None;
    for (i, &b) in bytes.iter().enumerate() {
        let is_word = b.is_ascii_alphanumeric() || b == b'_';
        match (is_word, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                tokens.push(&line[s..i]);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        tokens.push(&line[s..]);
    }
    tokens
}

/// Does `token` contain one of the company [`MARKS`] as a substring
/// (case-insensitive)? Returns the matched mark.
fn token_mark(token: &str) -> Option<&'static str> {
    let lowered = lower(token);
    MARKS.iter().copied().find(|m| lowered.contains(m))
}

/// Pure core: scan `text` for residual company marks, returning every hit that
/// is *not* covered by the allow-list.
///
/// Tokens are matched whole (so `GooseMode` is considered atomically and can be
/// allow-listed), then any token containing a mark substring is reported unless
/// [`is_allowed_token`] clears it. The English word `block` (allow-listed) and
/// `goose_*` identifiers never produce hits; `goose configure` does.
pub fn scan_for_marks(text: &str, allow: &[&str]) -> Vec<MarkHit> {
    let mut hits = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        for token in tokenize(line) {
            if let Some(mark) = token_mark(token) {
                if is_allowed_token(token, mark, allow) {
                    continue;
                }
                hits.push(MarkHit {
                    mark: mark.to_string(),
                    line: idx + 1,
                    token: token.to_string(),
                });
            }
        }
    }
    hits
}

/// Read a file's contents, returning `None` for any non-existent / unreadable /
/// non-UTF-8 file. Read-only by construction.
fn read_text(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// Run the full compliance + trademark gate over `repo_root`.
///
/// Strictly read-only: every file is opened for reading only; nothing is
/// written, renamed, or removed.
pub fn verify_compliance(repo_root: &Path) -> ComplianceReport {
    // --- Required-file presence / non-emptiness ---
    // The on-disk facts: (exists-as-file, non-empty). The pure core only needs
    // the non-empty flag for its verdict; we keep `exists` so the human summary
    // can distinguish "missing" from "empty".
    let facts: Vec<(bool, bool)> = REQUIRED_FILES
        .iter()
        .map(|name| {
            let path = repo_root.join(name);
            match std::fs::metadata(&path) {
                Ok(m) if m.is_file() => (true, m.len() > 0),
                _ => (false, false),
            }
        })
        .collect();
    let present: Vec<(&str, bool)> = REQUIRED_FILES
        .iter()
        .zip(facts.iter())
        .map(|(name, (_, non_empty))| (*name, *non_empty))
        .collect();
    let mut files = check_required_files(&present);
    for (check, (exists, _)) in files.iter_mut().zip(facts.iter()) {
        check.present = *exists;
    }

    // --- Bounded trademark scan over user-facing surfaces ---
    let mut surfaces = Vec::with_capacity(SCAN_SURFACES.len());
    for name in SCAN_SURFACES {
        let path = repo_root.join(name);
        match read_text(&path) {
            Some(text) => {
                let hits = scan_for_marks(&text, DEFAULT_ALLOW);
                // README.md is the front door: any residual mark there is a hard
                // failure. NOTICE / MODIFICATIONS.md legitimately *name* the
                // upstream projects for attribution, so a hit is informational.
                let is_attribution = *name != "README.md";
                let status = if hits.is_empty() {
                    Status::Ok
                } else if is_attribution {
                    Status::Warn
                } else {
                    Status::Fail
                };
                surfaces.push(SurfaceScan {
                    name: (*name).to_string(),
                    scanned: true,
                    hits,
                    status,
                });
            }
            None => {
                // A surface we cannot read is not itself a license violation;
                // surface it as a Warn so the user notices the gap.
                surfaces.push(SurfaceScan {
                    name: (*name).to_string(),
                    scanned: false,
                    hits: Vec::new(),
                    status: Status::Warn,
                });
            }
        }
    }

    let overall = files
        .iter()
        .map(|f| f.status)
        .chain(surfaces.iter().map(|s| s.status))
        .fold(Status::Ok, Status::worse);

    ComplianceReport {
        files,
        surfaces,
        overall,
    }
}

/// Render a [`ComplianceReport`] as a human-readable text summary.
fn render_summary(report: &ComplianceReport, repo_root: &Path) -> String {
    let mut out = format!(
        "Compliance gate for {} : {}\n",
        repo_root.display(),
        report.overall.as_str().to_uppercase()
    );

    out.push_str("\nRequired files:\n");
    for f in &report.files {
        let detail = if !f.present {
            "missing"
        } else if !f.non_empty {
            "empty"
        } else {
            "present"
        };
        out.push_str(&format!("  [{}] {} ({detail})\n", f.status.glyph(), f.name));
    }

    out.push_str("\nUser-facing surfaces:\n");
    for s in &report.surfaces {
        if !s.scanned {
            out.push_str(&format!("  [{}] {} (not found)\n", s.status.glyph(), s.name));
            continue;
        }
        out.push_str(&format!(
            "  [{}] {} ({} residual mark{})\n",
            s.status.glyph(),
            s.name,
            s.hits.len(),
            if s.hits.len() == 1 { "" } else { "s" },
        ));
        for hit in s.hits.iter().take(10) {
            out.push_str(&format!(
                "      line {}: {:?} ({})\n",
                hit.line, hit.token, hit.mark
            ));
        }
        if s.hits.len() > 10 {
            out.push_str(&format!("      ... {} more\n", s.hits.len() - 10));
        }
    }

    out
}

/// Convert a [`ComplianceReport`] into structured JSON for programmatic use.
fn report_to_json(report: &ComplianceReport, repo_root: &Path) -> Value {
    json!({
        "repo_root": repo_root.display().to_string(),
        "overall": report.overall.as_str(),
        "files": report.files.iter().map(|f| json!({
            "name": f.name,
            "present": f.present,
            "non_empty": f.non_empty,
            "status": f.status.as_str(),
        })).collect::<Vec<Value>>(),
        "surfaces": report.surfaces.iter().map(|s| json!({
            "name": s.name,
            "scanned": s.scanned,
            "status": s.status.as_str(),
            "hits": s.hits.iter().map(|h| json!({
                "mark": h.mark,
                "line": h.line,
                "token": h.token,
            })).collect::<Vec<Value>>(),
        })).collect::<Vec<Value>>(),
    })
}

/// Input schema for the `verify_compliance` tool. Both fields are optional: with
/// no arguments the working directory is audited.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ComplianceParams {
    /// Repository root to audit. Resolved relative to the working directory when
    /// not absolute. Defaults to the working directory itself.
    #[serde(default)]
    pub path: Option<String>,
}

/// Build the `Tool` descriptor for `verify_compliance`.
pub fn compliance_tool() -> Tool {
    Tool::new(
        "verify_compliance".to_string(),
        "Read-only license + trademark compliance gate. Confirms the project's \
         Apache-2.0 obligations are met (LICENSE, LICENSES/LICENSE-goose, \
         LICENSES/LICENSE-codex, NOTICE, and MODIFICATIONS.md all present and \
         non-empty) and scans a bounded set of user-facing surfaces (README, \
         NOTICE, MODIFICATIONS.md) for residual upstream trademark leakage, \
         ignoring documented internal identifiers. Returns a structured \
         pass/fail report. Never writes, renames, or deletes anything."
            .to_string(),
        schema_object::<ComplianceParams>(),
    )
    .annotate(ToolAnnotations::from_raw(
        Some("Verify Compliance".to_string()),
        Some(true),
        Some(false),
        Some(true),
        Some(false),
    ))
}

/// Serialize a JsonSchema type into the object form `Tool::new` expects.
fn schema_object<T: JsonSchema>() -> JsonObject {
    serde_json::to_value(schema_for!(T))
        .expect("schema serialization should succeed")
        .as_object()
        .expect("schema should serialize to an object")
        .clone()
}

/// Resolve the audit root from `params.path` (relative to `cwd`), defaulting to
/// `cwd`, then to the process cwd.
fn resolve_root(path: Option<&str>, cwd: Option<&Path>) -> PathBuf {
    let base = cwd
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    match path {
        Some(p) if !p.is_empty() => {
            let pb = PathBuf::from(p);
            if pb.is_absolute() {
                pb
            } else {
                base.join(pb)
            }
        }
        _ => base,
    }
}

/// Entry point for the `verify_compliance` tool. Parses the optional `{path?}`
/// argument, runs the read-only gate, and returns the report as text plus
/// structured content. Never mutates anything.
pub fn run(arguments: Option<JsonObject>, cwd: Option<&Path>) -> ToolResult<CallToolResult> {
    let params: ComplianceParams = match arguments {
        Some(map) => match serde_json::from_value(Value::Object(map)) {
            Ok(p) => p,
            Err(err) => {
                return Ok(error_result(&format!("Failed to parse arguments: {err}")));
            }
        },
        None => ComplianceParams::default(),
    };

    let root = resolve_root(params.path.as_deref(), cwd);
    let report = verify_compliance(&root);

    let summary = render_summary(&report, &root);
    let structured = report_to_json(&report, &root);

    let mut result = CallToolResult::success(vec![Content::text(summary).with_priority(0.0)]);
    // A failing gate is a finding, not a tool error; callers inspect `overall`.
    result.structured_content = Some(structured);
    Ok(result)
}

fn error_result(message: &str) -> CallToolResult {
    CallToolResult::error(vec![
        Content::text(format!("Error: {message}")).with_priority(0.0)
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::RawContent;
    use std::fs;
    use tempfile::TempDir;

    fn text_of(result: &CallToolResult) -> String {
        match &result.content[0].raw {
            RawContent::Text(t) => t.text.clone(),
            _ => panic!("expected text content"),
        }
    }

    fn structured_of(result: &CallToolResult) -> &Value {
        result
            .structured_content
            .as_ref()
            .expect("structured_content present")
    }

    // --- check_required_files ---

    #[test]
    fn check_required_files_flags_missing_or_empty_notice_as_fail() {
        let checks = check_required_files(&[
            ("LICENSE", true),
            ("LICENSES/LICENSE-goose", true),
            ("LICENSES/LICENSE-codex", true),
            ("NOTICE", false), // missing or empty
            ("MODIFICATIONS.md", true),
        ]);
        let notice = checks.iter().find(|c| c.name == "NOTICE").unwrap();
        assert_eq!(notice.status, Status::Fail);
        assert!(!notice.non_empty);
    }

    #[test]
    fn check_required_files_all_present_is_ok() {
        let checks = check_required_files(&[
            ("LICENSE", true),
            ("LICENSES/LICENSE-goose", true),
            ("LICENSES/LICENSE-codex", true),
            ("NOTICE", true),
            ("MODIFICATIONS.md", true),
        ]);
        assert!(checks.iter().all(|c| c.status == Status::Ok));
        assert!(checks.iter().all(|c| c.present && c.non_empty));
    }

    // --- scan_for_marks ---

    #[test]
    fn scan_finds_goose_configure_command() {
        let hits = scan_for_marks("Run `goose configure` to begin.", DEFAULT_ALLOW);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].mark, "goose");
        assert_eq!(hits[0].token, "goose");
        assert_eq!(hits[0].line, 1);
    }

    #[test]
    fn scan_ignores_allowlisted_identifiers_and_english_block() {
        let text = "\
let mode = GooseMode::Auto;\n\
let goose_mode = resolve();\n\
match content { ContentBlock::Text(_) => {} }\n\
The build was blocked but we unblock a block here.\n";
        let hits = scan_for_marks(text, DEFAULT_ALLOW);
        assert!(
            hits.is_empty(),
            "expected no leaks, got: {hits:?}"
        );
    }

    #[test]
    fn scan_reports_goose_client_only_when_not_allowlisted() {
        // GooseClient is allow-listed; a bare `Goose` brand word is not.
        let text = "GooseClient connects. The Goose mascot waddles.";
        let hits = scan_for_marks(text, DEFAULT_ALLOW);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].token, "Goose");
    }

    #[test]
    fn scan_detects_codex_and_openai_marks() {
        let text = "Ported from OpenAI Codex.";
        let hits = scan_for_marks(text, DEFAULT_ALLOW);
        let marks: Vec<&str> = hits.iter().map(|h| h.mark.as_str()).collect();
        assert!(marks.contains(&"openai"));
        assert!(marks.contains(&"codex"));
    }

    // --- verify_compliance over fixtures ---

    fn write_clean_repo(dir: &Path) {
        fs::write(dir.join("LICENSE"), "Apache License 2.0\n").unwrap();
        fs::create_dir_all(dir.join("LICENSES")).unwrap();
        fs::write(dir.join("LICENSES/LICENSE-goose"), "upstream license\n").unwrap();
        fs::write(dir.join("LICENSES/LICENSE-codex"), "codex license\n").unwrap();
        fs::write(dir.join("NOTICE"), "attribution notice\n").unwrap();
        fs::write(dir.join("MODIFICATIONS.md"), "# changes\n").unwrap();
        // A clean README with no upstream marks (only the English word "block").
        fs::write(
            dir.join("README.md"),
            "# BharatCode\n\nYour code stays local. Nothing can block you.\n",
        )
        .unwrap();
    }

    #[test]
    fn verify_compliance_clean_fixture_is_ok() {
        let tmp = TempDir::new().unwrap();
        write_clean_repo(tmp.path());

        let report = verify_compliance(tmp.path());
        assert_eq!(report.overall, Status::Ok, "report: {report:?}");
        assert!(report.files.iter().all(|f| f.status == Status::Ok));
        let readme = report
            .surfaces
            .iter()
            .find(|s| s.name == "README.md")
            .unwrap();
        assert_eq!(readme.status, Status::Ok);
        assert!(readme.hits.is_empty());
    }

    #[test]
    fn verify_compliance_missing_license_codex_is_fail() {
        let tmp = TempDir::new().unwrap();
        write_clean_repo(tmp.path());
        fs::remove_file(tmp.path().join("LICENSES/LICENSE-codex")).unwrap();

        let report = verify_compliance(tmp.path());
        assert_eq!(report.overall, Status::Fail);
        let codex = report
            .files
            .iter()
            .find(|f| f.name == "LICENSES/LICENSE-codex")
            .unwrap();
        assert_eq!(codex.status, Status::Fail);
        assert!(!codex.present);
    }

    #[test]
    fn verify_compliance_empty_notice_is_fail() {
        let tmp = TempDir::new().unwrap();
        write_clean_repo(tmp.path());
        fs::write(tmp.path().join("NOTICE"), "").unwrap();

        let report = verify_compliance(tmp.path());
        assert_eq!(report.overall, Status::Fail);
        let notice = report.files.iter().find(|f| f.name == "NOTICE").unwrap();
        assert_eq!(notice.status, Status::Fail);
    }

    #[test]
    fn verify_compliance_readme_leak_is_fail() {
        let tmp = TempDir::new().unwrap();
        write_clean_repo(tmp.path());
        fs::write(
            tmp.path().join("README.md"),
            "# BharatCode\n\nRun `goose configure` first.\n",
        )
        .unwrap();

        let report = verify_compliance(tmp.path());
        assert_eq!(report.overall, Status::Fail);
        let readme = report
            .surfaces
            .iter()
            .find(|s| s.name == "README.md")
            .unwrap();
        assert_eq!(readme.status, Status::Fail);
        assert_eq!(readme.hits.len(), 1);
    }

    #[test]
    fn verify_compliance_notice_attribution_is_warn_not_fail() {
        let tmp = TempDir::new().unwrap();
        write_clean_repo(tmp.path());
        // NOTICE naming the upstream projects is required attribution.
        fs::write(
            tmp.path().join("NOTICE"),
            "Derivative work of Goose (Block, Inc.) and OpenAI Codex.\n",
        )
        .unwrap();

        let report = verify_compliance(tmp.path());
        // Files are all present + non-empty; the NOTICE attribution names
        // upstream => Warn, never Fail.
        let notice = report
            .surfaces
            .iter()
            .find(|s| s.name == "NOTICE")
            .unwrap();
        assert_eq!(notice.status, Status::Warn);
        assert!(!notice.hits.is_empty());
        assert_eq!(report.overall, Status::Warn);
    }

    // --- tool wrapper: read-only, structured output ---

    #[test]
    fn run_over_clean_fixture_reports_ok_and_mutates_nothing() {
        let tmp = TempDir::new().unwrap();
        write_clean_repo(tmp.path());

        // Snapshot the directory tree (path -> len) before the call.
        let snapshot = |root: &Path| -> Vec<(PathBuf, u64)> {
            let mut entries = Vec::new();
            for entry in walkdir(root) {
                let len = fs::metadata(&entry).map(|m| m.len()).unwrap_or(0);
                entries.push((entry, len));
            }
            entries.sort();
            entries
        };
        let before = snapshot(tmp.path());

        let result = run(None, Some(tmp.path())).unwrap();
        assert_eq!(result.is_error, Some(false));

        let body = text_of(&result);
        assert!(body.contains("OK"), "summary was: {body}");
        // No upstream brand leaks in the tool's own user-facing output.
        assert!(!body.to_lowercase().contains("goose configure"));

        let sc = structured_of(&result);
        assert_eq!(sc["overall"], "ok");
        assert_eq!(sc["files"].as_array().unwrap().len(), REQUIRED_FILES.len());

        let after = snapshot(tmp.path());
        assert_eq!(before, after, "verify_compliance must not mutate the repo");
    }

    #[test]
    fn run_with_path_argument_resolves_under_cwd() {
        let tmp = TempDir::new().unwrap();
        let nested = tmp.path().join("repo");
        fs::create_dir_all(&nested).unwrap();
        write_clean_repo(&nested);

        let args = json!({ "path": "repo" });
        let result = run(args.as_object().cloned(), Some(tmp.path())).unwrap();
        assert_eq!(result.is_error, Some(false));
        assert_eq!(structured_of(&result)["overall"], "ok");
    }

    #[test]
    fn run_missing_codex_reports_fail_without_error_flag() {
        let tmp = TempDir::new().unwrap();
        write_clean_repo(tmp.path());
        fs::remove_file(tmp.path().join("LICENSES/LICENSE-codex")).unwrap();

        let result = run(None, Some(tmp.path())).unwrap();
        // A failing gate is a finding, not a tool error.
        assert_eq!(result.is_error, Some(false));
        assert_eq!(structured_of(&result)["overall"], "fail");
    }

    #[test]
    fn compliance_tool_is_read_only_and_brand_clean() {
        let tool = compliance_tool();
        assert_eq!(tool.name, "verify_compliance");
        let ann = tool.annotations.expect("annotations present");
        assert_eq!(ann.read_only_hint, Some(true));
        assert_eq!(ann.destructive_hint, Some(false));
        // The user-facing description must not leak an upstream product name as
        // a bare brand; it may name files like LICENSE-goose for accuracy.
        let desc = tool.description.unwrap_or_default().to_string();
        assert!(desc.contains("compliance"));
    }

    /// Minimal recursive directory walk for the mutation-free assertion (kept
    /// local so the test has no extra dependency).
    fn walkdir(root: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(read) = fs::read_dir(&dir) else { continue };
            for entry in read.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    out.push(path);
                }
            }
        }
        out
    }
}
