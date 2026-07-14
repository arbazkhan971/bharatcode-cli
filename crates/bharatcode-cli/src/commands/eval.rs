//! `bharatcode eval` — an offline-capable benchmark / eval harness.
//!
//! This command runs a small, embedded suite of deterministic coding tasks
//! through the **shared headless agent path** (the same `build_session` +
//! single-turn `headless` flow used by `review-diff` and `gen-tests`) and emits
//! a pass/fail + latency + ₹-cost scorecard. Its purpose is to make regressions
//! across model/provider swaps *measurable*: run the same suite before and after
//! a swap and compare pass rate, p50/p95 latency, and rupee spend.
//!
//! The design deliberately splits the harness into two layers:
//!
//!   * a **pure, offline core** — the embedded [`EvalCase`] suite, the
//!     [`Grader`] (substring / regex / shell), the [`grade`] function, the
//!     per-case [`EvalResult`], and the scorecard JSON. None of these touch the
//!     network or a model, so `bharatcode eval --list` and *grading itself* work
//!     fully offline and are exercised by this module's own unit tests.
//!   * a **thin online driver** — [`run_case`], which builds a session, runs one
//!     headless turn to obtain the assistant reply, times it, and reads the
//!     already-recorded session cost (USD) and converts it to ₹ via the existing
//!     [`crate::commands::cost_ledger`]. This is the only part that needs a
//!     provider; it is never reached by `--list` or by the unit tests.
//!
//! Per-case wall time is clamped by `BHARATCODE_EVAL_TIMEOUT_SECS` (a positive
//! integer; clamped to a sane range). No environment variable is required to run
//! `--list` or to grade offline — default behaviour is unchanged.
//!
//! User-facing labels route through the i18n layer via [`label`], falling back
//! to English when the active locale has no entry for a key, so English output
//! stays stable while translations can land later.
//!
//! Original BharatCode work; not ported from any third party.

use std::io::Write as _;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::commands::cost_ledger;
use crate::session::{build_session, SessionBuilderConfig};

/// Default per-case wall-time budget, in seconds, when
/// `BHARATCODE_EVAL_TIMEOUT_SECS` is unset or unparseable.
const DEFAULT_TIMEOUT_SECS: u64 = 120;

/// Lower clamp for the per-case wall-time budget. A model turn needs at least a
/// few seconds to be meaningful, so absurdly small requests are raised to this.
const MIN_TIMEOUT_SECS: u64 = 5;

/// Upper clamp for the per-case wall-time budget. A single eval case should
/// never be allowed to run unbounded regardless of what the environment asks.
const MAX_TIMEOUT_SECS: u64 = 1_800;

/// The embedded eval-case suite, compiled into the binary via [`include_str!`]
/// so `--list` and grading work with no filesystem and no network. Each entry is
/// the raw YAML of one case; [`load_cases`] parses them.
const EMBEDDED_CASE_YAML: &[&str] = &[
    include_str!("eval_cases/hello_world.yaml"),
    include_str!("eval_cases/sum_two_numbers.yaml"),
    include_str!("eval_cases/json_field.yaml"),
];

/// Options accepted by [`handle_eval`].
#[derive(Debug, Clone, Default)]
pub struct EvalOptions {
    /// List the embedded cases (offline; no model call) and return.
    pub list: bool,
    /// Run only the case with this id; when `None`, run the whole suite.
    pub case: Option<String>,
    /// Emit the machine-readable JSON scorecard instead of the table.
    pub json: bool,
}

/// A deterministic grader for an assistant reply.
///
/// All three variants are pure with respect to the harness: `Substring` and
/// `Regex` inspect the reply text directly; `Shell` runs a fixed command with
/// the reply exposed at `$REPLY_FILE` and treats exit code 0 as a pass. None of
/// them call a model, so [`Grader::grade`] is fully offline and unit-tested.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Grader {
    /// Pass when `value` appears in the reply.
    Substring {
        /// The needle to look for.
        value: String,
        /// When `true`, compare case-insensitively.
        #[serde(default)]
        ignore_case: bool,
    },
    /// Pass when `value` (a regular expression) matches anywhere in the reply.
    Regex {
        /// The pattern to match.
        value: String,
    },
    /// Pass when the shell command `value` exits 0. The reply is written to a
    /// temp file whose path is exported as `REPLY_FILE`.
    Shell {
        /// The check command, run via `sh -c`.
        value: String,
    },
}

impl Grader {
    /// Grade `reply` deterministically, returning `Ok(true)` for a pass.
    ///
    /// A malformed regex or a shell that cannot be spawned is an `Err`, not a
    /// silent fail, so harness bugs are distinguishable from genuine case
    /// failures.
    pub fn grade(&self, reply: &str) -> Result<bool> {
        match self {
            Grader::Substring { value, ignore_case } => {
                if *ignore_case {
                    Ok(reply.to_lowercase().contains(&value.to_lowercase()))
                } else {
                    Ok(reply.contains(value))
                }
            }
            Grader::Regex { value } => {
                let re = regex::Regex::new(value)
                    .with_context(|| format!("invalid regex grader pattern: {value}"))?;
                Ok(re.is_match(reply))
            }
            Grader::Shell { value } => run_shell_grader(value, reply),
        }
    }
}

/// Run a shell grader: write `reply` to a temp file, export its path as
/// `REPLY_FILE`, run `sh -c <command>`, and return whether it exited 0.
fn run_shell_grader(command: &str, reply: &str) -> Result<bool> {
    let mut file = tempfile::NamedTempFile::new().context("creating reply temp file")?;
    file.write_all(reply.as_bytes())
        .context("writing reply to temp file")?;
    file.flush().context("flushing reply temp file")?;

    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .env("REPLY_FILE", file.path())
        .status()
        .context("spawning shell grader")?;
    Ok(status.success())
}

/// Grade `reply` with `grader`, returning `Ok(true)` for a pass.
///
/// A thin free-function wrapper over [`Grader::grade`] so callers (and the CLI
/// dispatch) can grade without naming the method. Pure and offline.
pub fn grade_reply(grader: &Grader, reply: &str) -> Result<bool> {
    grader.grade(reply)
}

/// One embedded eval case: a stable id, human-readable metadata, the prompt sent
/// to the agent, and the deterministic [`Grader`] applied to the reply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvalCase {
    /// Stable id used to select the case on the command line and as a row label.
    pub id: String,
    /// Short human-readable title for the listing.
    pub title: String,
    /// Longer description of what the case checks (optional in YAML).
    #[serde(default)]
    pub description: String,
    /// The prompt sent to the agent as a single headless turn.
    pub prompt: String,
    /// The deterministic grader applied to the assistant reply.
    pub grader: Grader,
}

/// Parse the embedded YAML suite into [`EvalCase`]s.
///
/// Pure and offline: this is what `--list` and the unit tests rely on. A parse
/// error names the failing case so a broken embedded file is easy to spot.
pub fn load_cases() -> Result<Vec<EvalCase>> {
    let mut cases = Vec::with_capacity(EMBEDDED_CASE_YAML.len());
    for (idx, yaml) in EMBEDDED_CASE_YAML.iter().enumerate() {
        let case: EvalCase = serde_yaml::from_str(yaml)
            .with_context(|| format!("parsing embedded eval case #{idx}"))?;
        cases.push(case);
    }
    Ok(cases)
}

/// The graded outcome of running one [`EvalCase`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalResult {
    /// Id of the case this result is for.
    pub case_id: String,
    /// Whether the grader passed.
    pub passed: bool,
    /// Wall-clock time the turn took, in milliseconds.
    pub latency_ms: u64,
    /// Rupee cost attributed to this case (delta in session spend, converted).
    pub cost_inr: f64,
    /// A short diagnostic note (e.g. an error, or `timeout`); empty on a clean
    /// pass.
    #[serde(default)]
    pub note: String,
}

/// The aggregate scorecard: every per-case [`EvalResult`] plus rolled-up totals.
/// Round-trips losslessly through JSON (exercised by the unit tests).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreCard {
    /// One result per case run, in suite order.
    pub results: Vec<EvalResult>,
    /// Number of cases that passed.
    pub passed: usize,
    /// Number of cases run in total.
    pub total: usize,
    /// Sum of per-case rupee cost.
    pub total_cost_inr: f64,
}

impl ScoreCard {
    /// Build a scorecard from per-case results, computing the roll-ups.
    pub fn from_results(results: Vec<EvalResult>) -> Self {
        let total = results.len();
        let passed = results.iter().filter(|r| r.passed).count();
        let total_cost_inr = results.iter().map(|r| r.cost_inr).sum();
        ScoreCard {
            results,
            passed,
            total,
            total_cost_inr,
        }
    }

    /// Pass rate in `[0.0, 1.0]`; `0.0` for an empty card.
    pub fn pass_rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.passed as f64 / self.total as f64
        }
    }
}

/// Resolve the per-case timeout from `BHARATCODE_EVAL_TIMEOUT_SECS`, falling
/// back to the default and clamping to `[MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS]`. A
/// `0` or unparseable value falls back to the default before clamping.
pub fn resolve_timeout_secs() -> u64 {
    std::env::var("BHARATCODE_EVAL_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|&v| v > 0)
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
        .clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS)
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

/// Render the listing of embedded cases (id + title) as a string. Pure: takes
/// the cases, returns the text, prints nothing. Used by `--list`.
pub fn render_list(cases: &[EvalCase]) -> String {
    let mut out = String::new();
    out.push('\n');
    out.push_str(&format!(
        "  {}\n\n",
        crate::theme::heading(label("eval.list.title", "BharatCode eval cases"))
    ));
    let id_width = cases.iter().map(|c| c.id.len()).max().unwrap_or(0).max(2);
    for c in cases {
        out.push_str(&format!(
            "  {:<id_width$}  {}\n",
            c.id,
            crate::theme::muted(&c.title),
            id_width = id_width,
        ));
    }
    out.push('\n');
    out.push_str(&format!(
        "  {}: {}\n",
        label("eval.list.count", "cases"),
        cases.len()
    ));
    out
}

/// Render a scorecard as a clean, aligned table plus a summary line. Pure.
pub fn render_scorecard(card: &ScoreCard) -> String {
    let header_case = label("eval.col.case", "case");
    let header_result = label("eval.col.result", "result");
    let header_latency = label("eval.col.latency", "latency");
    let header_cost = label("eval.col.cost", "cost");

    let name_width = card
        .results
        .iter()
        .map(|r| r.case_id.len())
        .chain(std::iter::once(header_case.len()))
        .max()
        .unwrap_or(header_case.len());

    let pass_label = label("eval.pass", "PASS");
    let fail_label = label("eval.fail", "FAIL");

    let mut out = String::new();
    out.push('\n');
    out.push_str(&format!(
        "  {}\n\n",
        crate::theme::heading(label("eval.title", "BharatCode eval scorecard"))
    ));
    out.push_str(&format!(
        "  {:<name_width$}  {:>8}  {:>10}  {:>12}\n",
        header_case,
        header_result,
        header_latency,
        header_cost,
        name_width = name_width,
    ));
    out.push_str(&format!(
        "  {}\n",
        "-".repeat(name_width + 2 + 8 + 2 + 10 + 2 + 12)
    ));
    for r in &card.results {
        let verdict = if r.passed { &pass_label } else { &fail_label };
        out.push_str(&format!(
            "  {:<name_width$}  {:>8}  {:>8}ms  {:>12}\n",
            r.case_id,
            verdict,
            r.latency_ms,
            cost_ledger::format_inr(r.cost_inr),
            name_width = name_width,
        ));
        if !r.note.is_empty() {
            out.push_str(&format!(
                "  {:<name_width$}  {}\n",
                "",
                r.note,
                name_width = name_width
            ));
        }
    }
    out.push('\n');
    out.push_str(&format!(
        "  {}: {}/{}  •  {}: {}\n",
        label("eval.summary.passed", "passed"),
        card.passed,
        card.total,
        label("eval.summary.cost", "total cost"),
        cost_ledger::format_inr(card.total_cost_inr),
    ));
    out
}

/// Render a scorecard as a stable, machine-readable JSON object.
pub fn render_json(card: &ScoreCard) -> String {
    serde_json::to_string_pretty(card).unwrap_or_else(|_| "{}".to_string())
}

/// Extract the last assistant message's text from a finished conversation.
///
/// Returns an empty string when there is no assistant message, which a grader
/// will treat as a fail rather than a panic.
fn last_assistant_text(conversation: &bharatcode_core::conversation::Conversation) -> String {
    conversation
        .iter()
        .rev()
        .find(|m| m.role == rmcp::model::Role::Assistant)
        .map(|m| m.as_concat_text())
        .unwrap_or_default()
}

/// Run a single case through the shared headless agent path, time it, grade the
/// reply, and attribute the rupee cost.
///
/// This is the only online surface in the module; it is never reached by
/// `--list` or by the unit tests. The turn is wrapped in a wall-time budget so a
/// stuck case cannot hang the suite.
async fn run_case(case: &EvalCase, timeout: Duration) -> EvalResult {
    let mut session = build_session(SessionBuilderConfig {
        session_id: None,
        no_session: true,
        no_profile: true,
        builtins: vec!["developer".to_string()],
        quiet: true,
        output_format: "text".to_string(),
        ..SessionBuilderConfig::default()
    })
    .await;

    let cost_before = session_cost_inr(&session).await;
    let start = Instant::now();
    let turn = tokio::time::timeout(timeout, session.headless(case.prompt.clone())).await;
    let latency_ms = start.elapsed().as_millis() as u64;

    let (passed, note) = match turn {
        Err(_elapsed) => (false, label("eval.note.timeout", "timeout")),
        Ok(Err(e)) => (false, format!("error: {e}")),
        Ok(Ok(())) => {
            let reply = last_assistant_text(&session.message_history());
            match case.grader.grade(&reply) {
                Ok(true) => (true, String::new()),
                Ok(false) => (false, String::new()),
                Err(e) => (false, format!("grader error: {e}")),
            }
        }
    };

    let cost_after = session_cost_inr(&session).await;
    let cost_inr = (cost_after - cost_before).max(0.0);

    EvalResult {
        case_id: case.id.clone(),
        passed,
        latency_ms,
        cost_inr,
        note,
    }
}

/// Read the session's accumulated spend and convert it to ₹. Returns `0.0` when
/// the session has no recorded cost yet.
async fn session_cost_inr(session: &crate::session::CliSession) -> f64 {
    let usd = session
        .get_session()
        .await
        .ok()
        .and_then(|s| s.accumulated_cost)
        .unwrap_or(0.0);
    cost_ledger::usd_to_inr(usd)
}

/// Entry point for the `bharatcode eval` surface.
///
/// * `--list`            — print the embedded cases (offline) and return.
/// * `--case <id>`       — run only that case.
/// * (no case)           — run the whole suite.
///
/// `--list` never builds a session or calls a model; running cases uses the
/// shared headless agent path.
pub async fn handle_eval(opts: EvalOptions) -> Result<()> {
    let cases = load_cases()?;

    if opts.list {
        print!("{}", render_list(&cases));
        return Ok(());
    }

    let selected: Vec<&EvalCase> = match opts.case.as_deref() {
        Some(id) => {
            let found: Vec<&EvalCase> = cases.iter().filter(|c| c.id == id).collect();
            if found.is_empty() {
                anyhow::bail!("{}: {id}", label("eval.unknown_case", "unknown eval case"));
            }
            found
        }
        None => cases.iter().collect(),
    };

    let timeout = Duration::from_secs(resolve_timeout_secs());
    let mut results = Vec::with_capacity(selected.len());
    for case in selected {
        results.push(run_case(case, timeout).await);
    }
    let card = ScoreCard::from_results(results);

    if opts.json {
        println!("{}", render_json(&card));
    } else {
        print!("{}", render_scorecard(&card));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- The embedded suite parses with no network and no panic. ----

    #[test]
    fn list_parses_all_embedded_cases() {
        let cases = load_cases().expect("embedded cases must parse");
        assert_eq!(cases.len(), EMBEDDED_CASE_YAML.len());
        // Ids are unique and non-empty; every case has a prompt.
        let mut ids: Vec<&str> = cases.iter().map(|c| c.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), cases.len(), "case ids must be unique");
        for c in &cases {
            assert!(!c.id.is_empty(), "case id must be non-empty");
            assert!(!c.prompt.trim().is_empty(), "case prompt must be non-empty");
        }
    }

    #[test]
    fn each_grader_kind_is_represented_in_the_suite() {
        let cases = load_cases().unwrap();
        let mut saw_substring = false;
        let mut saw_regex = false;
        let mut saw_shell = false;
        for c in &cases {
            match c.grader {
                Grader::Substring { .. } => saw_substring = true,
                Grader::Regex { .. } => saw_regex = true,
                Grader::Shell { .. } => saw_shell = true,
            }
        }
        assert!(saw_substring && saw_regex && saw_shell);
    }

    // ---- Substring grader: known-good vs known-bad. ----

    #[test]
    fn substring_grader_scores_good_and_bad() {
        let g = Grader::Substring {
            value: "helloworld".to_string(),
            ignore_case: true,
        };
        assert!(g.grade("Sure! HELLOWORLD").unwrap());
        assert!(g.grade("helloworld").unwrap());
        assert!(!g.grade("goodbye").unwrap());
    }

    #[test]
    fn substring_grader_respects_case_sensitivity() {
        let sensitive = Grader::Substring {
            value: "OK".to_string(),
            ignore_case: false,
        };
        assert!(sensitive.grade("status OK here").unwrap());
        assert!(!sensitive.grade("status ok here").unwrap());
    }

    // ---- Regex grader: known-good vs known-bad, plus malformed pattern. ----

    #[test]
    fn regex_grader_scores_good_and_bad() {
        let g = Grader::Regex {
            value: r"\b42\b".to_string(),
        };
        assert!(g.grade("The answer is 42.").unwrap());
        assert!(!g.grade("The answer is 420.").unwrap());
        assert!(!g.grade("no number here").unwrap());
    }

    #[test]
    fn regex_grader_reports_malformed_pattern() {
        let g = Grader::Regex {
            value: "(".to_string(),
        };
        assert!(g.grade("anything").is_err());
    }

    // ---- Shell grader: known-good vs known-bad via $REPLY_FILE. ----

    #[test]
    fn shell_grader_scores_good_and_bad() {
        let g = Grader::Shell {
            value: "grep -q hello \"$REPLY_FILE\"".to_string(),
        };
        assert!(g.grade("well hello there").unwrap());
        assert!(!g.grade("nothing matching").unwrap());
    }

    #[test]
    fn shell_grader_nonzero_exit_is_a_fail() {
        let g = Grader::Shell {
            value: "exit 3".to_string(),
        };
        assert!(!g.grade("ignored").unwrap());
    }

    #[test]
    fn embedded_json_field_case_grades_its_own_reference_reply() {
        // The shipped shell case must pass a known-good reply and fail a bad one,
        // proving the embedded grader command is correct (offline).
        let cases = load_cases().unwrap();
        let case = cases.iter().find(|c| c.id == "json-field").unwrap();
        assert!(case.grader.grade(r#"{"status":"ok"}"#).unwrap());
        assert!(!case.grader.grade(r#"{"status":"error"}"#).unwrap());
    }

    // ---- Scorecard roll-ups and JSON round-trip. ----

    fn result(id: &str, passed: bool, ms: u64, inr: f64) -> EvalResult {
        EvalResult {
            case_id: id.to_string(),
            passed,
            latency_ms: ms,
            cost_inr: inr,
            note: String::new(),
        }
    }

    #[test]
    fn scorecard_rolls_up_pass_count_and_cost() {
        let card = ScoreCard::from_results(vec![
            result("a", true, 100, 1.5),
            result("b", false, 200, 0.5),
            result("c", true, 300, 2.0),
        ]);
        assert_eq!(card.total, 3);
        assert_eq!(card.passed, 2);
        assert!((card.total_cost_inr - 4.0).abs() < 1e-9);
        assert!((card.pass_rate() - 2.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn scorecard_empty_pass_rate_is_zero() {
        let card = ScoreCard::from_results(vec![]);
        assert_eq!(card.pass_rate(), 0.0);
        assert_eq!(card.total, 0);
    }

    #[test]
    fn scorecard_json_round_trips() {
        let card = ScoreCard::from_results(vec![
            result("a", true, 100, 1.5),
            result("b", false, 200, 0.5),
        ]);
        let json = render_json(&card);
        let parsed: ScoreCard = serde_json::from_str(&json).expect("valid scorecard JSON");
        assert_eq!(parsed, card);
    }

    #[test]
    fn grader_json_round_trips_each_variant() {
        for g in [
            Grader::Substring {
                value: "x".to_string(),
                ignore_case: true,
            },
            Grader::Regex {
                value: r"\d+".to_string(),
            },
            Grader::Shell {
                value: "true".to_string(),
            },
        ] {
            let json = serde_json::to_string(&g).unwrap();
            let back: Grader = serde_json::from_str(&json).unwrap();
            assert_eq!(g, back);
        }
    }

    // ---- Timeout clamps to a sane range. ----

    #[test]
    fn timeout_resolution_clamps_env() {
        // The resolver clamps its inputs into the sane range regardless of the
        // raw value; assert on the clamp math directly so the test does not
        // depend on process-wide env state.
        let clamp = |v: u64| v.clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS);
        assert_eq!(clamp(1), MIN_TIMEOUT_SECS);
        assert_eq!(clamp(u64::MAX), MAX_TIMEOUT_SECS);
        assert_eq!(clamp(DEFAULT_TIMEOUT_SECS), DEFAULT_TIMEOUT_SECS);
        // And the live resolver returns something inside the range.
        let resolved = resolve_timeout_secs();
        assert!((MIN_TIMEOUT_SECS..=MAX_TIMEOUT_SECS).contains(&resolved));
    }

    // ---- Listing renders ids/titles and the count. ----

    #[test]
    fn render_list_contains_ids_titles_and_count() {
        let cases = load_cases().unwrap();
        let listing = render_list(&cases);
        for c in &cases {
            assert!(listing.contains(&c.id), "listing missing id {}", c.id);
        }
        assert!(listing.contains(&format!("cases: {}", cases.len())));
    }

    #[test]
    fn render_scorecard_contains_verdicts_and_summary() {
        let card = ScoreCard::from_results(vec![
            result("a", true, 100, 1.5),
            result("b", false, 200, 0.5),
        ]);
        let table = render_scorecard(&card);
        assert!(table.contains("a"));
        assert!(table.contains("b"));
        assert!(table.contains("PASS"));
        assert!(table.contains("FAIL"));
        assert!(table.contains("passed"));
        assert!(table.contains("1/2"));
    }

    // ---- No user-facing upstream brand leakage in static case data. ----

    #[test]
    fn embedded_cases_are_brand_free() {
        let banned = ["goose", "block "];
        let cases = load_cases().unwrap();
        for c in &cases {
            for field in [&c.id, &c.title, &c.description, &c.prompt] {
                let lower = field.to_lowercase();
                for b in banned {
                    assert!(!lower.contains(b), "embedded case leaks brand '{}'", b);
                }
            }
        }
    }
}
