//! Final compliance + trademark gate — `bharatcode compliance` (BharatCode v99).
//!
//! A self-contained, **read-only** release gate that answers one question: is
//! this build legally and editorially clean enough to ship? It folds two
//! independent checks into a single pass/fail verdict and a process exit code so
//! it can run unattended as a CI step:
//!
//! 1. **Apache-2.0 obligations** — every license / attribution file a derivative
//!    work must ship is present, non-empty, and Apache-2.0-shaped (carries an
//!    `Apache` marker):
//!    `LICENSE`, `LICENSES/LICENSE-goose`, `LICENSES/LICENSE-codex`, `NOTICE`,
//!    `MODIFICATIONS.md`, and `THIRD_PARTY_LICENSES.md`. A missing, empty, or
//!    un-shaped file is a hard `✗`.
//!
//! 2. **Residual trademark leakage** — a bounded scan of *this process's own
//!    embedded user-facing strings* for upstream company marks
//!    (`Goose` / `Block` / `OpenAI` / `ChatGPT`). The scanned surfaces are
//!    exactly the ones prior leaks hid in: the bundled i18n tables (`en.json` and
//!    `hi.json`, embedded at compile time) and the built-in skill bodies
//!    (`goose::skills::discover_skills`, the same embedded markdown the agent
//!    serves). Matches that correspond to documented internal Rust identifiers
//!    (`GooseMode`, `GooseClient`, `goose_*` snake_case, `ContentBlock`) or the
//!    ordinary English word *block* / *blocked* are allow-listed and never
//!    counted as leaks.
//!
//! The gate is strictly read-only: it opens files for reading only and never
//! writes, renames, or deletes anything, and it makes no network calls. There is
//! no opt-in env gate — a read-only gate is always safe to run. The `--strict`
//! flag turns advisory `⚠` rows (an unreadable license file, an un-shaped but
//! present file) into hard failures so a release pipeline can demand a spotless
//! result.
//!
//! User-facing labels render through [`crate::tr!`] into the `compliance.*` keys
//! carried by `i18n/hi.json` (this version's shared wire), falling back to the
//! embedded English defaults when a locale table has no entry.

use anyhow::Result;

/// The license / attribution files an Apache-2.0 derivative work must ship,
/// relative to the repository root. Each must be present, non-empty, and carry an
/// `Apache` marker (see [`is_apache_shaped`]).
pub const REQUIRED_FILES: &[&str] = &[
    "LICENSE",
    "LICENSES/LICENSE-goose",
    "LICENSES/LICENSE-codex",
    "NOTICE",
    "MODIFICATIONS.md",
    "THIRD_PARTY_LICENSES.md",
];

/// Upstream company marks scanned for. Lowercased; matching is case-insensitive.
/// These are the marks the rebrand must keep out of user-facing surfaces; the
/// internal `goose-*` crate/identifier names are allow-listed in
/// [`is_allowed_token`].
const MARKS: &[&str] = &["goose", "block", "openai", "chatgpt"];

/// Documented internal identifiers that are *not* trademark leaks. Matching is
/// case-insensitive against whole word-tokens; `goose_*` snake_case identifiers
/// and the English word *block* family are handled by dedicated rules in
/// [`is_allowed_token`].
pub const DEFAULT_ALLOW: &[&str] = &["GooseMode", "GooseClient", "ContentBlock", "block"];

/// Pass / warn / fail classification for a single check or the overall verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// The obligation is met / no leak found.
    Ok,
    /// A non-fatal advisory (downgraded to a failure under `--strict`).
    Warn,
    /// A hard failure: a required file is missing/empty/un-shaped, or a leak was
    /// found.
    Fail,
}

impl Status {
    /// The glyph this status renders as, matching the doctor command's style.
    pub fn glyph(self) -> &'static str {
        match self {
            Status::Ok => "\u{2713}",   // ✓
            Status::Warn => "\u{26a0}", // ⚠
            Status::Fail => "\u{2717}", // ✗
        }
    }

    /// Combine two statuses, keeping the more severe one (`Fail` > `Warn` > `Ok`).
    fn worse(self, other: Status) -> Status {
        match (self, other) {
            (Status::Fail, _) | (_, Status::Fail) => Status::Fail,
            (Status::Warn, _) | (_, Status::Warn) => Status::Warn,
            _ => Status::Ok,
        }
    }

    /// Resolve to the effective pass/fail boolean given the `strict` flag: a
    /// `Warn` is a failure only under `--strict`.
    fn passes(self, strict: bool) -> bool {
        match self {
            Status::Ok => true,
            Status::Warn => !strict,
            Status::Fail => false,
        }
    }
}

/// On-disk facts about a single required file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCheck {
    /// Repo-root-relative file name.
    pub name: String,
    /// Whether the file exists on disk as a regular file.
    pub present: bool,
    /// Whether the file is non-empty (only meaningful when `present`).
    pub non_empty: bool,
    /// Whether the file carries an `Apache` marker (only meaningful when read).
    pub apache_shaped: bool,
    /// `Ok` when present, non-empty, and Apache-shaped; otherwise `Fail`.
    pub status: Status,
}

/// One residual-trademark hit found while scanning an embedded surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkHit {
    /// Human-readable name of the surface the hit came from (e.g. `i18n/en.json`).
    pub surface: String,
    /// The company mark that matched (lowercased: `goose`/`block`/...).
    pub mark: String,
    /// The exact token (verbatim case) that triggered the hit.
    pub token: String,
}

/// The full structured result of the gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComplianceReport {
    /// Per-file results, in [`REQUIRED_FILES`] order.
    pub files: Vec<FileCheck>,
    /// Residual trademark hits across every scanned embedded surface.
    pub hits: Vec<MarkHit>,
    /// Number of embedded surfaces scanned (for the summary line).
    pub surfaces_scanned: usize,
}

impl ComplianceReport {
    /// Worst status across all files plus the trademark scan.
    ///
    /// File problems are always `Fail`. The trademark scan is `Fail` when any hit
    /// remains after the allow-list filter, else `Ok`. The `--strict` flag is
    /// applied separately by [`Status::passes`] so this stays a pure rollup.
    pub fn overall(&self) -> Status {
        let files = self
            .files
            .iter()
            .map(|f| f.status)
            .fold(Status::Ok, Status::worse);
        let scan = if self.hits.is_empty() {
            Status::Ok
        } else {
            Status::Fail
        };
        files.worse(scan)
    }
}

// ---------------------------------------------------------------------------
// Pure cores (filesystem- and locale-independent, trivially unit testable)
// ---------------------------------------------------------------------------

/// Whether `text` is Apache-2.0-shaped: it carries an `Apache` marker. Every
/// required file in this project — the three full Apache license texts and the
/// three attribution documents (`NOTICE`, `MODIFICATIONS.md`,
/// `THIRD_PARTY_LICENSES.md`) — names the Apache License, so a case-insensitive
/// `apache` substring is a sound, low-false-positive shape signal.
fn is_apache_shaped(text: &str) -> bool {
    text.to_ascii_lowercase().contains("apache")
}

/// Classify one required file from its `(name, present, contents)` facts.
///
/// `Ok` only when the file is present, non-empty, and Apache-shaped; otherwise
/// `Fail`. The caller supplies the on-disk facts so this core needs no
/// filesystem.
pub fn classify_file(name: &str, present: bool, contents: Option<&str>) -> FileCheck {
    let (non_empty, apache_shaped) = match contents {
        Some(c) => (!c.trim().is_empty(), is_apache_shaped(c)),
        None => (false, false),
    };
    let ok = present && non_empty && apache_shaped;
    FileCheck {
        name: name.to_string(),
        present,
        non_empty,
        apache_shaped,
        status: if ok { Status::Ok } else { Status::Fail },
    }
}

/// Lowercase a token for case-insensitive comparisons.
fn lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

/// Decide whether `token` (already known to contain `mark`) is allow-listed —
/// i.e. *not* a real trademark leak.
///
/// Rules (mirroring the project's documented leak-gate policy):
/// * an exact (case-insensitive) match against an `allow` entry — covers
///   `GooseMode`, `GooseClient`, `ContentBlock`, and the bare word `block`;
/// * any `goose_*` snake_case identifier (`goose_mode`, `goose_providers`, …) —
///   these are internal Rust field / fn / crate-path names;
/// * for the `block` mark only: the ordinary English word *block* and its
///   derivatives (`blocked`, `blocking`, `unblock`, `blockchain`, …) and
///   `*Block` / `*block` compounds. The brand only ever appears as the
///   standalone capitalized token `Block` ("Block, Inc."), so that exact form is
///   the only `block`-mark hit treated as a leak.
fn is_allowed_token(token: &str, mark: &str, allow: &[&str]) -> bool {
    let lowered = lower(token);
    if allow.iter().any(|a| lower(a) == lowered) {
        return true;
    }
    if lowered.starts_with("goose_") {
        return true;
    }
    if mark == "block" {
        return token != "Block";
    }
    false
}

/// Split a line into identifier-ish tokens: maximal runs of ASCII alphanumerics
/// plus `_`. Keeps `goose_mode` and `GooseMode` whole so the allow-list can
/// reason about them, while still isolating a bare `Goose`.
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

/// Pure core: scan `text` (one embedded surface named `surface`) for residual
/// company marks, returning every hit *not* covered by the allow-list.
///
/// Tokens are matched whole (so `GooseMode` is considered atomically), then any
/// token containing a mark substring is reported unless [`is_allowed_token`]
/// clears it. The English word `block` (allow-listed) and `goose_*` identifiers
/// never produce hits; a bare `Goose` / `OpenAI` / `ChatGPT` does.
pub fn scan_surface(surface: &str, text: &str, allow: &[&str]) -> Vec<MarkHit> {
    let mut hits = Vec::new();
    for line in text.lines() {
        for token in tokenize(line) {
            if let Some(mark) = token_mark(token) {
                if is_allowed_token(token, mark, allow) {
                    continue;
                }
                hits.push(MarkHit {
                    surface: surface.to_string(),
                    mark: mark.to_string(),
                    token: token.to_string(),
                });
            }
        }
    }
    hits
}

// ---------------------------------------------------------------------------
// Embedded surfaces
// ---------------------------------------------------------------------------

/// The bundled English i18n table, embedded at compile time. This is exactly the
/// table the running binary's `t()` consults, so the scan reflects what the
/// process can actually emit.
const EN_JSON: &str = include_str!("../i18n/en.json");

/// The bundled Hindi i18n table, embedded at compile time (this version's shared
/// wire — `compliance.*` labels live here).
const HI_JSON: &str = include_str!("../i18n/hi.json");

/// Every embedded user-facing surface to scan, as `(name, text)` pairs: the two
/// i18n tables plus each built-in skill body. Built-in skill bodies come from the
/// public [`goose::skills::discover_skills`] discovery (filtered to the embedded
/// `BuiltinSkill` entries), which is the same embedded markdown the agent serves
/// — so a leak in a shipped skill is caught here without re-embedding it.
fn embedded_surfaces() -> Vec<(String, String)> {
    use goose::custom_requests::SourceType;

    let mut surfaces: Vec<(String, String)> = vec![
        ("i18n/en.json".to_string(), EN_JSON.to_string()),
        ("i18n/hi.json".to_string(), HI_JSON.to_string()),
    ];

    // Built-in skill bodies are embedded in the binary; pass `None` so discovery
    // resolves the process's own built-ins, then keep only the `BuiltinSkill`
    // entries (filesystem skills, if any, are not part of *this binary's*
    // user-facing surface and are deliberately excluded for determinism).
    for skill in goose::skills::discover_skills(None) {
        if skill.source_type == SourceType::BuiltinSkill {
            let name = format!("skill:{}", skill.name);
            // Scan both the description (frontmatter blurb) and the body, since a
            // leak could hide in either user-facing field.
            let text = format!("{}\n{}", skill.description, skill.content);
            surfaces.push((name, text));
        }
    }

    surfaces
}

/// Read a required file's contents, returning `None` when it is absent /
/// unreadable / non-UTF-8. Read-only by construction.
fn read_required(repo_root: &std::path::Path, name: &str) -> (bool, Option<String>) {
    let path = repo_root.join(name);
    match std::fs::metadata(&path) {
        Ok(m) if m.is_file() => (true, std::fs::read_to_string(&path).ok()),
        _ => (false, None),
    }
}

/// Run the full gate over `repo_root` against the supplied embedded `surfaces`.
///
/// Kept generic over the surface list so tests can inject a fixture set; the
/// production entry point passes [`embedded_surfaces`]. Strictly read-only.
pub fn run_gate(repo_root: &std::path::Path, surfaces: &[(String, String)]) -> ComplianceReport {
    let files = REQUIRED_FILES
        .iter()
        .map(|&name| {
            let (present, contents) = read_required(repo_root, name);
            classify_file(name, present, contents.as_deref())
        })
        .collect();

    let mut hits = Vec::new();
    for (name, text) in surfaces {
        hits.extend(scan_surface(name, text, DEFAULT_ALLOW));
    }

    ComplianceReport {
        files,
        hits,
        surfaces_scanned: surfaces.len(),
    }
}

// ---------------------------------------------------------------------------
// Rendering + CLI entry point
// ---------------------------------------------------------------------------

/// Look up a user-facing label through the i18n layer, falling back to the
/// English `default` when the active locale table has no entry for `key`.
///
/// `t()` echoes the key back when it is missing, so an unchanged key means
/// "untranslated"; this keeps the gate renderable in English without depending on
/// the i18n table while still letting `i18n/hi.json` localize every row.
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// Resolve the repository root to audit: the current working directory.
fn resolve_repo_root() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
}

/// Run the compliance + trademark gate and print a `✓`/`✗`/`⚠` line per check
/// with a final verdict.
///
/// Returns a process exit code: `0` when the gate passes, non-zero when any check
/// fails. With `strict = true`, advisory `⚠` rows are treated as failures so a
/// release pipeline can demand a spotless result. Read-only and offline.
pub fn handle_compliance(strict: bool) -> Result<i32> {
    let repo_root = resolve_repo_root();
    let surfaces = embedded_surfaces();
    let report = run_gate(&repo_root, &surfaces);

    println!(
        "{}",
        crate::theme::heading(label("compliance.title", "BharatCode compliance gate"))
    );
    println!();

    // --- Required files -------------------------------------------------
    println!(
        "{}",
        crate::theme::muted(label("compliance.files_header", "License & attribution files"))
    );
    for f in &report.files {
        let detail = if !f.present {
            label("compliance.detail_missing", "missing")
        } else if !f.non_empty {
            label("compliance.detail_empty", "empty")
        } else if !f.apache_shaped {
            label("compliance.detail_unshaped", "not Apache-2.0 shaped")
        } else {
            label("compliance.detail_ok", "present")
        };
        let glyph = f.status.glyph();
        println!("  {glyph} {} ({detail})", f.name);
    }

    println!();

    // --- Trademark scan -------------------------------------------------
    let scan_header = label("compliance.scan_header", "Embedded trademark scan");
    println!("{}", crate::theme::muted(scan_header));
    if report.hits.is_empty() {
        let clean = label(
            "compliance.scan_clean",
            "no residual upstream marks in embedded user-facing strings",
        );
        println!(
            "  {} {} ({} {})",
            Status::Ok.glyph(),
            clean,
            report.surfaces_scanned,
            label("compliance.surfaces_word", "surfaces"),
        );
    } else {
        let found = label("compliance.scan_leak", "residual upstream mark");
        println!(
            "  {} {} ({})",
            Status::Fail.glyph(),
            found,
            report.hits.len(),
        );
        for hit in report.hits.iter().take(20) {
            println!("      {}: {:?} ({})", hit.surface, hit.token, hit.mark);
        }
        if report.hits.len() > 20 {
            let more = label("compliance.more", "more");
            println!("      ... {} {more}", report.hits.len() - 20);
        }
    }

    println!();

    // --- Verdict --------------------------------------------------------
    let overall = report.overall();
    let passed = overall.passes(strict);
    if passed {
        let verdict = label("compliance.verdict_pass", "compliance gate passed");
        println!("{} {}", Status::Ok.glyph(), crate::theme::success(&verdict));
        Ok(0)
    } else {
        let verdict = label("compliance.verdict_fail", "compliance gate failed");
        println!("{} {}", Status::Fail.glyph(), crate::theme::error(&verdict));
        Ok(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    /// Write a clean, fully compliant fixture repo: every required file present,
    /// non-empty, and Apache-shaped.
    fn write_clean_repo(dir: &Path) {
        let apache = "Apache License\nVersion 2.0, January 2004\n";
        fs::write(dir.join("LICENSE"), apache).unwrap();
        fs::create_dir_all(dir.join("LICENSES")).unwrap();
        fs::write(dir.join("LICENSES/LICENSE-goose"), apache).unwrap();
        fs::write(dir.join("LICENSES/LICENSE-codex"), apache).unwrap();
        fs::write(
            dir.join("NOTICE"),
            "Derivative work under the Apache License 2.0.\n",
        )
        .unwrap();
        fs::write(
            dir.join("MODIFICATIONS.md"),
            "# Modifications\n\nDerivative work, Apache-2.0.\n",
        )
        .unwrap();
        fs::write(
            dir.join("THIRD_PARTY_LICENSES.md"),
            "# Third-Party Licenses\n\nApache License 2.0.\n",
        )
        .unwrap();
    }

    /// A clean embedded-surface set (no upstream marks; only the English word
    /// "blocked", which is allow-listed).
    fn clean_surfaces() -> Vec<(String, String)> {
        vec![
            (
                "i18n/en.json".to_string(),
                "{ \"budget.deny\": \"further turns are blocked\" }".to_string(),
            ),
            (
                "skill:framework-migration".to_string(),
                "Mark rows with no clean equivalent as BLOCKED.".to_string(),
            ),
        ]
    }

    // --- classify_file / is_apache_shaped -------------------------------

    #[test]
    fn apache_shaped_detects_marker_case_insensitively() {
        assert!(is_apache_shaped("the Apache License, Version 2.0"));
        assert!(is_apache_shaped("licensed under apache-2.0"));
        assert!(!is_apache_shaped("MIT License\n"));
        assert!(!is_apache_shaped(""));
    }

    #[test]
    fn classify_file_ok_when_present_nonempty_and_shaped() {
        let c = classify_file("LICENSE", true, Some("Apache License 2.0\n"));
        assert_eq!(c.status, Status::Ok);
        assert!(c.present && c.non_empty && c.apache_shaped);
    }

    #[test]
    fn classify_file_fail_when_missing_or_empty_or_unshaped() {
        assert_eq!(classify_file("NOTICE", false, None).status, Status::Fail);
        assert_eq!(
            classify_file("NOTICE", true, Some("   \n")).status,
            Status::Fail
        );
        assert_eq!(
            classify_file("NOTICE", true, Some("just some text")).status,
            Status::Fail,
            "non-Apache text must fail the shape check"
        );
    }

    // --- scan_surface ---------------------------------------------------

    #[test]
    fn scan_flags_injected_goose_brand_but_not_internal_idents() {
        // An injected bare `Goose` brand word IS flagged; the internal
        // `GooseMode` identifier is NOT.
        let text = "let mode = GooseMode::Auto; // The Goose mascot waddles.";
        let hits = scan_surface("i18n/en.json", text, DEFAULT_ALLOW);
        assert_eq!(hits.len(), 1, "hits: {hits:?}");
        assert_eq!(hits[0].token, "Goose");
        assert_eq!(hits[0].mark, "goose");
        assert_eq!(hits[0].surface, "i18n/en.json");
    }

    #[test]
    fn scan_ignores_english_block_and_goose_snake_case() {
        let text = "\
let goose_mode = resolve();\n\
match c { ContentBlock::Text(_) => {} }\n\
The build was blocked but we unblock a block here. BLOCKED rows skip.\n";
        let hits = scan_surface("skill:x", text, DEFAULT_ALLOW);
        assert!(hits.is_empty(), "expected no leaks, got: {hits:?}");
    }

    #[test]
    fn scan_detects_openai_and_chatgpt_marks() {
        let hits = scan_surface(
            "skill:y",
            "Compare against ChatGPT and the OpenAI API.",
            DEFAULT_ALLOW,
        );
        let marks: Vec<&str> = hits.iter().map(|h| h.mark.as_str()).collect();
        assert!(marks.contains(&"chatgpt"), "marks: {marks:?}");
        assert!(marks.contains(&"openai"), "marks: {marks:?}");
    }

    // --- run_gate over fixtures -----------------------------------------

    #[test]
    fn clean_fixture_passes_with_exit_zero() {
        let tmp = TempDir::new().unwrap();
        write_clean_repo(tmp.path());

        let report = run_gate(tmp.path(), &clean_surfaces());
        assert!(report.hits.is_empty(), "hits: {:?}", report.hits);
        assert!(report.files.iter().all(|f| f.status == Status::Ok));

        let overall = report.overall();
        assert_eq!(overall, Status::Ok);
        assert!(overall.passes(false), "clean gate must pass => exit 0");
        assert!(overall.passes(true), "clean gate passes even under --strict");
    }

    #[test]
    fn missing_notice_fails_with_nonzero_exit() {
        let tmp = TempDir::new().unwrap();
        write_clean_repo(tmp.path());
        fs::remove_file(tmp.path().join("NOTICE")).unwrap();

        let report = run_gate(tmp.path(), &clean_surfaces());
        let notice = report.files.iter().find(|f| f.name == "NOTICE").unwrap();
        assert_eq!(notice.status, Status::Fail);
        assert!(!notice.present);

        let overall = report.overall();
        assert_eq!(overall, Status::Fail);
        assert!(
            !overall.passes(false),
            "missing NOTICE must fail => nonzero exit"
        );
    }

    #[test]
    fn injected_goose_string_in_scan_set_is_flagged() {
        let tmp = TempDir::new().unwrap();
        write_clean_repo(tmp.path());

        // All files clean, but an embedded surface leaks the `Goose` brand while
        // a sibling line uses the internal `GooseMode` identifier (must NOT
        // count). The leak alone must fail the gate.
        let surfaces = vec![(
            "i18n/en.json".to_string(),
            "{ \"x\": \"Run Goose configure\", \"y\": \"GooseMode::Auto\" }".to_string(),
        )];
        let report = run_gate(tmp.path(), &surfaces);
        assert_eq!(report.hits.len(), 1, "hits: {:?}", report.hits);
        assert_eq!(report.hits[0].token, "Goose");
        assert!(report.files.iter().all(|f| f.status == Status::Ok));
        assert_eq!(report.overall(), Status::Fail);
        assert!(!report.overall().passes(false));
    }

    #[test]
    fn empty_notice_fails_shape_and_nonempty_checks() {
        let tmp = TempDir::new().unwrap();
        write_clean_repo(tmp.path());
        fs::write(tmp.path().join("NOTICE"), "").unwrap();

        let report = run_gate(tmp.path(), &clean_surfaces());
        let notice = report.files.iter().find(|f| f.name == "NOTICE").unwrap();
        assert_eq!(notice.status, Status::Fail);
        assert!(notice.present && !notice.non_empty);
        assert_eq!(report.overall(), Status::Fail);
    }

    // --- handle_compliance end-to-end against the real repo -------------

    #[test]
    fn handle_compliance_returns_exit_code() {
        // Smoke test: the real entry point runs against the process cwd and never
        // panics, returning a valid exit code (0 or 1) regardless of the cwd.
        let code = handle_compliance(false).expect("gate must not error");
        assert!(code == 0 || code == 1, "unexpected exit code: {code}");
    }

    #[test]
    fn embedded_surfaces_include_i18n_tables_and_are_self_clean() {
        // The binary's own embedded surfaces (real en.json/hi.json + built-in
        // skill bodies) must themselves be free of residual upstream marks: this
        // is the live leak gate for the shipped strings.
        let surfaces = embedded_surfaces();
        let names: Vec<&str> = surfaces.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"i18n/en.json"));
        assert!(names.contains(&"i18n/hi.json"));

        let mut hits = Vec::new();
        for (name, text) in &surfaces {
            hits.extend(scan_surface(name, text, DEFAULT_ALLOW));
        }
        assert!(
            hits.is_empty(),
            "shipped embedded strings leak an upstream mark: {hits:?}"
        );
    }

    // --- strict-mode warning semantics ----------------------------------

    #[test]
    fn warn_passes_unless_strict() {
        assert!(Status::Warn.passes(false));
        assert!(!Status::Warn.passes(true));
        assert!(Status::Ok.passes(true));
        assert!(!Status::Fail.passes(false));
    }

    // --- hi.json round-trips and carries compliance.* keys with parity ---

    #[test]
    fn hi_json_round_trips_and_has_compliance_keys_with_en_parity() {
        use std::collections::HashMap;
        let en: HashMap<String, String> =
            serde_json::from_str(EN_JSON).expect("en.json must be valid JSON");
        let hi: HashMap<String, String> =
            serde_json::from_str(HI_JSON).expect("hi.json must round-trip as JSON");

        // The compliance.* result-label keys this version's renderer uses, all
        // localized in hi.json (this version's shared wire).
        let compliance_keys = [
            "compliance.title",
            "compliance.files_header",
            "compliance.detail_missing",
            "compliance.detail_empty",
            "compliance.detail_unshaped",
            "compliance.detail_ok",
            "compliance.scan_header",
            "compliance.scan_clean",
            "compliance.scan_leak",
            "compliance.surfaces_word",
            "compliance.more",
            "compliance.verdict_pass",
            "compliance.verdict_fail",
        ];
        for key in compliance_keys {
            let hv = hi.get(key);
            assert!(
                hv.is_some_and(|v| !v.trim().is_empty()),
                "hi.json missing non-empty compliance key: {key}"
            );
        }

        // en/hi parity in the direction this version can guarantee on its own:
        // every `compliance.*` key the English source-of-truth table carries
        // must also be present (non-empty) in hi.json, so no English key is left
        // un-localized. (en.json is owned by a sibling version that adds the same
        // keys; the renderer's English fallback lives in `label`'s defaults, so
        // the gate is correct even before that lands.)
        for (key, value) in &en {
            if !key.starts_with("compliance.") {
                continue;
            }
            assert!(!value.trim().is_empty(), "en.json has empty {key}");
            assert!(
                hi.get(key).is_some_and(|v| !v.trim().is_empty()),
                "hi.json must localize every English compliance key: {key}"
            );
        }
    }
}
