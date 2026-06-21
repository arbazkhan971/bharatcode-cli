//! `bharatcode-bench` — a deterministic, offline benchmark / eval harness.
//!
//! This module ships a small, fully-offline eval harness whose entire purpose
//! is to make regressions in core helpers *catchable*. It runs a fixed,
//! embedded [`SUITE`] of scorable [`BenchCase`]s, each of which exercises a
//! shipped pure function (no model call, no network, no file writes) and checks
//! the result against an expected output. [`run_suite`] scores the cases and
//! returns a [`BenchReport`] with `{ total, passed, score }`.
//!
//! The design is deliberately *data vs. execution*: the cases are plain data,
//! the checker per [`CaseKind`] is a pure function over shipped code, and the
//! report renderer is the only thing that touches styling. Nothing here reads
//! the network or mutates state, so the whole harness is exercised by this
//! module's own unit tests.
//!
//! Why these cases? Each one re-runs a *shipped* pure helper so a regression in
//! the real code path fails the bench:
//!
//!   * `exec_policy::ExecPolicy::{from_json, check}` — the allow/deny command
//!     splitter that screens shell commands (deny-prefix match, allow-list
//!     miss/hit, chained-segment boundaries).
//!   * `cost_ledger::format_inr` — the ₹ formatter, including the paise-carry
//!     boundary (`0.999 -> ₹1.00`) and Indian digit grouping.
//!   * `cost_ledger::format_inr_compact` — the compact ₹ magnitude *bucket*
//!     boundaries (k / L / Cr thresholds), the offline analog of a token-bucket
//!     boundary table.
//!
//! By default the harness is *parse-only*: no model is ever invoked. The
//! `--live` flag is reserved for future model-backed scoring; today it is a
//! documented stub that reports every case as [`CaseStatus::Skipped`] without
//! contacting any provider. `--list` prints the case ids and exits without
//! running anything.
//!
//! There is no env gate: the harness is read-only and has no side effects, so
//! running it never changes default behavior. It is surfaced through the thin
//! `bharatcode-bench` binary, which calls [`handle_bench`].

use std::sync::LazyLock;

use anyhow::Result;

use bharatcode_core::exec_policy::{Decision, ExecPolicy};

use crate::commands::cost_ledger::{format_inr, format_inr_compact};

/// Which shipped pure helper a [`BenchCase`] exercises.
///
/// Every variant maps to a single offline, deterministic function in the
/// shipped code base; this is what makes a bench failure a real regression
/// signal rather than a synthetic one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseKind {
    /// Screen `input` (a `policy_json || command_line` pair, split on the first
    /// ` || `) through [`ExecPolicy::check`] and compare the [`Decision`]
    /// against `expected` (`"allow"` or `"deny"`).
    ExecPolicy,
    /// Format `input` (a rupee amount, parsed as `f64`) through [`format_inr`]
    /// and compare the string against `expected`.
    CostInr,
    /// Format `input` (a rupee amount, parsed as `f64`) through
    /// [`format_inr_compact`] and compare the string against `expected`.
    CostInrCompact,
}

impl CaseKind {
    /// Stable, lowercase tag used in reports and JSON.
    pub fn tag(self) -> &'static str {
        match self {
            CaseKind::ExecPolicy => "exec_policy",
            CaseKind::CostInr => "cost_inr",
            CaseKind::CostInrCompact => "cost_inr_compact",
        }
    }
}

/// A single embedded, scorable benchmark case.
///
/// A case is plain data: a stable `id`, the `kind` (which shipped helper to
/// run), the `input` it is fed, and the `expected` output its checker must
/// produce. The checker itself lives in [`check_case`] so cases stay
/// declarative.
#[derive(Debug, Clone)]
pub struct BenchCase {
    /// Stable, unique identifier used by `--list` and report rows.
    pub id: &'static str,
    /// Which shipped pure helper this case exercises.
    pub kind: CaseKind,
    /// The input fed to the helper (encoding depends on `kind`).
    pub input: &'static str,
    /// The expected output the checker compares against.
    pub expected: &'static str,
}

/// The embedded benchmark suite.
///
/// Cases are small, self-contained, and brand-free. Each one drives a shipped
/// pure function across a meaningful boundary so a regression in that function
/// fails the bench. Ids are unique (asserted in the tests).
///
/// `BenchCase` is `'static`-data only, so this could be a plain `static`; it is
/// built once via [`LazyLock`] purely to keep the table next to its checker and
/// leave room for owned data later. It is immutable and side-effect free.
pub static SUITE: LazyLock<Vec<BenchCase>> = LazyLock::new(|| {
    vec![
        // ---- exec_policy splitter / allow-deny boundaries ----
        BenchCase {
            id: "exec-deny-prefix",
            kind: CaseKind::ExecPolicy,
            input: r#"{"deny":["rm -rf"]} || rm -rf /tmp/x"#,
            expected: "deny",
        },
        BenchCase {
            id: "exec-allow-miss",
            kind: CaseKind::ExecPolicy,
            input: r#"{"allow":["ls"]} || cat /etc/hosts"#,
            expected: "deny",
        },
        BenchCase {
            id: "exec-allow-hit",
            kind: CaseKind::ExecPolicy,
            input: r#"{"allow":["ls"]} || ls -la"#,
            expected: "allow",
        },
        BenchCase {
            id: "exec-chained-segment-deny",
            kind: CaseKind::ExecPolicy,
            input: r#"{"deny":["curl"]} || echo hi && curl http://x"#,
            expected: "deny",
        },
        BenchCase {
            id: "exec-empty-policy-allows",
            kind: CaseKind::ExecPolicy,
            input: r#"{} || anything goes here"#,
            expected: "allow",
        },
        // ---- cost ledger ₹ rounding boundaries ----
        BenchCase {
            id: "inr-paise-carry",
            kind: CaseKind::CostInr,
            // 0.999 rupees rounds paise to 100, which must carry into ₹1.00.
            input: "0.999",
            expected: "₹1.00",
        },
        BenchCase {
            id: "inr-indian-grouping",
            kind: CaseKind::CostInr,
            input: "1234567.5",
            expected: "₹12,34,567.50",
        },
        BenchCase {
            id: "inr-small-amount",
            kind: CaseKind::CostInr,
            input: "42.25",
            expected: "₹42.25",
        },
        // ---- cost ledger compact magnitude (bucket) boundaries ----
        BenchCase {
            id: "inr-compact-thousand",
            kind: CaseKind::CostInrCompact,
            input: "1500",
            expected: "₹1.5k",
        },
        BenchCase {
            id: "inr-compact-lakh",
            kind: CaseKind::CostInrCompact,
            input: "250000",
            expected: "₹2.50L",
        },
        BenchCase {
            id: "inr-compact-crore",
            kind: CaseKind::CostInrCompact,
            input: "30000000",
            expected: "₹3.00Cr",
        },
    ]
});

/// The status of a single case after scoring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseStatus {
    /// The checker produced the expected output.
    Passed,
    /// The checker produced something other than the expected output.
    Failed,
    /// The case was not run (e.g. `--live` model-backed scoring, not yet
    /// implemented). Skipped cases never count toward the score.
    Skipped,
}

impl CaseStatus {
    /// Stable, lowercase tag for JSON / diagnostics.
    pub fn tag(self) -> &'static str {
        match self {
            CaseStatus::Passed => "passed",
            CaseStatus::Failed => "failed",
            CaseStatus::Skipped => "skipped",
        }
    }
}

/// The scored outcome of a single case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseResult {
    /// The id of the case this result is for.
    pub id: String,
    /// The kind tag (which shipped helper ran).
    pub kind: &'static str,
    /// The scoring status.
    pub status: CaseStatus,
    /// What the checker actually produced (empty for skipped cases).
    pub actual: String,
    /// What the case expected.
    pub expected: String,
}

/// A scored benchmark report: per-case results plus aggregates.
///
/// `score` is the pass fraction over the cases that actually *ran* (skipped
/// cases are excluded from the denominator), in `0.0..=1.0`. A run with no
/// runnable cases scores `0.0`.
#[derive(Debug, Clone, PartialEq)]
pub struct BenchReport {
    /// One [`CaseResult`] per case, in suite order.
    pub results: Vec<CaseResult>,
    /// Total number of cases in the run.
    pub total: usize,
    /// Number of cases that passed.
    pub passed: usize,
    /// Pass fraction over non-skipped cases, in `0.0..=1.0`.
    pub score: f64,
}

/// Run the [`ExecPolicy`] checker for an `exec_policy`-kind case.
///
/// `input` is `policy_json || command_line`. Returns `"allow"` / `"deny"` to
/// match against `expected`, or `Err` if the embedded policy JSON is malformed
/// (which would itself be a regression worth catching).
fn check_exec_policy(input: &str) -> std::result::Result<&'static str, String> {
    let (policy_json, command) = input
        .split_once(" || ")
        .ok_or_else(|| "exec_policy case input must be `policy_json || command`".to_string())?;
    let policy = ExecPolicy::from_json(policy_json.trim())?;
    Ok(match policy.check(command.trim()) {
        Decision::Allow => "allow",
        Decision::Deny { .. } => "deny",
    })
}

/// Run the checker for a single case and return its concrete output.
///
/// This is the pure heart of the harness: it dispatches on [`CaseKind`], calls
/// the shipped helper, and yields the string the report compares against
/// `expected`. It never performs I/O.
pub fn check_case(case: &BenchCase) -> std::result::Result<String, String> {
    match case.kind {
        CaseKind::ExecPolicy => check_exec_policy(case.input).map(|s| s.to_string()),
        CaseKind::CostInr => {
            let amount: f64 = case
                .input
                .trim()
                .parse()
                .map_err(|_| format!("cost_inr case input is not a number: {}", case.input))?;
            Ok(format_inr(amount))
        }
        CaseKind::CostInrCompact => {
            let amount: f64 = case.input.trim().parse().map_err(|_| {
                format!(
                    "cost_inr_compact case input is not a number: {}",
                    case.input
                )
            })?;
            Ok(format_inr_compact(amount))
        }
    }
}

/// Score an embedded suite of cases, returning a [`BenchReport`].
///
/// This is the central, deterministic entry point named by the spec: given a
/// slice of cases it runs each checker, compares the output to `expected`, and
/// aggregates `{ total, passed, score }`. It performs no I/O and is fully
/// unit-testable; a checker error fails its case rather than aborting the run.
pub fn run_suite(cases: &[BenchCase]) -> BenchReport {
    let mut results = Vec::with_capacity(cases.len());
    let mut passed = 0usize;

    for case in cases {
        let (status, actual) = match check_case(case) {
            Ok(out) if out == case.expected => {
                passed += 1;
                (CaseStatus::Passed, out)
            }
            Ok(out) => (CaseStatus::Failed, out),
            Err(err) => (CaseStatus::Failed, err),
        };
        results.push(CaseResult {
            id: case.id.to_string(),
            kind: case.kind.tag(),
            status,
            actual,
            expected: case.expected.to_string(),
        });
    }

    let total = cases.len();
    let ran = results
        .iter()
        .filter(|r| r.status != CaseStatus::Skipped)
        .count();
    let score = if ran == 0 {
        0.0
    } else {
        passed as f64 / ran as f64
    };

    BenchReport {
        results,
        total,
        passed,
        score,
    }
}

/// Build a report for a `--live` run.
///
/// Model-backed scoring is not implemented yet, so every case is reported as
/// [`CaseStatus::Skipped`] and the score is `0.0`. This keeps the `--live`
/// surface documented and reachable without ever contacting a provider.
fn run_suite_live(cases: &[BenchCase]) -> BenchReport {
    let results = cases
        .iter()
        .map(|case| CaseResult {
            id: case.id.to_string(),
            kind: case.kind.tag(),
            status: CaseStatus::Skipped,
            actual: String::new(),
            expected: case.expected.to_string(),
        })
        .collect();
    BenchReport {
        results,
        total: cases.len(),
        passed: 0,
        score: 0.0,
    }
}

impl BenchReport {
    /// Render the report as a styled, human-readable block.
    ///
    /// Styling is routed through [`crate::theme`] so the harness honors the
    /// active palette. Failing rows show the expected-vs-actual diff for quick
    /// triage. Output is deterministic for a given suite.
    pub fn render_report(&self) -> String {
        use crate::theme;

        let mut out = String::new();
        out.push_str(&format!(
            "{}\n",
            theme::heading(crate::tr!("bench.report.title"))
        ));

        let id_width = self
            .results
            .iter()
            .map(|r| r.id.len())
            .max()
            .unwrap_or(0)
            .max(crate::tr!("bench.col.case").len());

        for r in &self.results {
            let (mark, styled_id) = match r.status {
                CaseStatus::Passed => (
                    "PASS",
                    theme::success(format!("{:<id_width$}", r.id)).to_string(),
                ),
                CaseStatus::Failed => (
                    "FAIL",
                    theme::error(format!("{:<id_width$}", r.id)).to_string(),
                ),
                CaseStatus::Skipped => (
                    "SKIP",
                    theme::muted(format!("{:<id_width$}", r.id)).to_string(),
                ),
            };
            out.push_str(&format!("  {mark}  {styled_id}  {}", theme::muted(r.kind)));
            if r.status == CaseStatus::Failed {
                out.push_str(&format!(
                    "  ({}={:?} {}={:?})",
                    crate::tr!("bench.col.expected"),
                    r.expected,
                    crate::tr!("bench.col.actual"),
                    r.actual,
                ));
            }
            out.push('\n');
        }

        out.push('\n');
        let summary = format!(
            "{}: {}/{}  {}: {:.0}%",
            crate::tr!("bench.passed"),
            self.passed,
            self.total,
            crate::tr!("bench.score"),
            self.score * 100.0,
        );
        if self.passed == self.total {
            out.push_str(&format!("{}\n", theme::success(summary)));
        } else {
            out.push_str(&format!("{}\n", theme::accent(summary)));
        }
        out
    }

    /// Render the report as a stable, machine-readable JSON object.
    pub fn render_json(&self) -> String {
        let cases: Vec<serde_json::Value> = self
            .results
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.id,
                    "kind": r.kind,
                    "status": r.status.tag(),
                    "expected": r.expected,
                    "actual": r.actual,
                })
            })
            .collect();
        let value = serde_json::json!({
            "results": cases,
            "total": self.total,
            "passed": self.passed,
            "score": self.score,
        });
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
    }
}

/// Options accepted by [`handle_bench`].
///
/// Defaults to the offline, parse-only run. There is no env gate: every field
/// defaults to off and the harness is read-only.
#[derive(Debug, Clone, Default)]
pub struct BenchOptions {
    /// When `true`, print the case ids and exit without running anything.
    pub list: bool,
    /// When `true`, request future model-backed scoring. Today this is a
    /// documented stub that reports every case as `Skipped` and contacts no
    /// provider.
    pub live: bool,
    /// When `true`, emit the machine-readable JSON report instead of the styled
    /// table.
    pub json: bool,
}

/// Public entry point for the `bharatcode-bench` binary.
///
/// Resolves [`BenchOptions`] into one of three behaviors: `--list` prints the
/// embedded case ids and returns; `--live` runs the (stubbed) model-backed path
/// that skips every case; otherwise the default offline parse-only suite is
/// scored via [`run_suite`]. The report is printed as a styled table or JSON.
pub fn handle_bench(opts: BenchOptions) -> Result<()> {
    if opts.list {
        for case in SUITE.iter() {
            println!("{}\t{}", case.id, case.kind.tag());
        }
        return Ok(());
    }

    let report = if opts.live {
        run_suite_live(&SUITE)
    } else {
        run_suite(&SUITE)
    };

    if opts.json {
        println!("{}", report.render_json());
    } else {
        print!("{}", report.render_report());
    }

    Ok(())
}

/// The ids of every embedded case, in suite order.
///
/// Exposed so `--list` and tests share one definition of "all ids".
pub fn case_ids() -> Vec<&'static str> {
    SUITE.iter().map(|c| c.id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suite_is_known_good_and_fully_passes() {
        // The headline invariant: on the embedded, known-good suite, every case
        // passes and the score is a perfect 1.0.
        let report = run_suite(&SUITE);
        assert_eq!(report.total, SUITE.len());
        assert_eq!(
            report.passed,
            report.total,
            "known-good suite must fully pass; failing rows: {:?}",
            report
                .results
                .iter()
                .filter(|r| r.status != CaseStatus::Passed)
                .collect::<Vec<_>>()
        );
        // score >= a comfortable threshold (and in fact exactly 1.0).
        assert!(
            report.score >= 0.99,
            "score below threshold: {}",
            report.score
        );
        assert!((report.score - 1.0).abs() < 1e-9);
    }

    #[test]
    fn list_yields_all_case_ids() {
        let ids = case_ids();
        assert_eq!(ids.len(), SUITE.len());
        for case in SUITE.iter() {
            assert!(ids.contains(&case.id), "missing id from list: {}", case.id);
        }
    }

    #[test]
    fn case_ids_are_unique_and_non_empty() {
        let mut seen = std::collections::HashSet::new();
        for case in SUITE.iter() {
            assert!(!case.id.is_empty(), "empty case id");
            assert!(seen.insert(case.id), "duplicate case id: {}", case.id);
            assert!(!case.expected.is_empty(), "empty expected for {}", case.id);
        }
    }

    #[test]
    fn exec_policy_cases_exercise_shipped_splitter() {
        // Directly assert the shipped helper agrees with the embedded expected
        // outputs, so a regression in ExecPolicy is what fails the bench.
        assert_eq!(
            check_exec_policy(r#"{"deny":["rm -rf"]} || rm -rf /tmp/x"#).unwrap(),
            "deny"
        );
        assert_eq!(
            check_exec_policy(r#"{"allow":["ls"]} || ls -la"#).unwrap(),
            "allow"
        );
        assert_eq!(
            check_exec_policy(r#"{"allow":["ls"]} || cat /etc/hosts"#).unwrap(),
            "deny"
        );
        assert_eq!(check_exec_policy(r#"{} || anything"#).unwrap(), "allow");
        // A chained `&&` segment that hits a denied prefix must deny.
        assert_eq!(
            check_exec_policy(r#"{"deny":["curl"]} || echo hi && curl http://x"#).unwrap(),
            "deny"
        );
    }

    #[test]
    fn cost_inr_paise_carry_boundary() {
        // The paise-carry boundary in format_inr: 0.999 -> ₹1.00.
        assert_eq!(format_inr(0.999), "₹1.00");
        // And the case fed to the suite produces exactly the expected string.
        let case = SUITE.iter().find(|c| c.id == "inr-paise-carry").unwrap();
        assert_eq!(check_case(case).unwrap(), case.expected);
    }

    #[test]
    fn compact_magnitude_bucket_boundaries() {
        // k / L / Cr thresholds — the offline analog of a bucket-boundary table.
        assert_eq!(format_inr_compact(1500.0), "₹1.5k");
        assert_eq!(format_inr_compact(250000.0), "₹2.50L");
        assert_eq!(format_inr_compact(30000000.0), "₹3.00Cr");
    }

    #[test]
    fn failing_case_is_scored_failed_not_panicking() {
        // A deliberately-wrong expected value must score Failed, not panic, and
        // must drag the score below 1.0.
        let bad = vec![BenchCase {
            id: "intentionally-wrong",
            kind: CaseKind::CostInr,
            input: "42.25",
            expected: "₹999.99",
        }];
        let report = run_suite(&bad);
        assert_eq!(report.total, 1);
        assert_eq!(report.passed, 0);
        assert_eq!(report.results[0].status, CaseStatus::Failed);
        assert_eq!(report.results[0].actual, "₹42.25");
        assert!((report.score - 0.0).abs() < 1e-9);
    }

    #[test]
    fn malformed_exec_policy_input_fails_closed() {
        // A checker error (bad input encoding) fails the case rather than
        // aborting the whole run.
        let bad = vec![BenchCase {
            id: "no-separator",
            kind: CaseKind::ExecPolicy,
            input: "missing the separator",
            expected: "allow",
        }];
        let report = run_suite(&bad);
        assert_eq!(report.results[0].status, CaseStatus::Failed);
    }

    #[test]
    fn live_run_skips_every_case_and_scores_zero() {
        // --live is a documented stub: nothing runs, score is 0.0, every case is
        // Skipped, and skipped cases are excluded from passing math.
        let report = run_suite_live(&SUITE);
        assert_eq!(report.total, SUITE.len());
        assert_eq!(report.passed, 0);
        assert!((report.score - 0.0).abs() < 1e-9);
        assert!(report
            .results
            .iter()
            .all(|r| r.status == CaseStatus::Skipped));
    }

    #[test]
    fn render_report_marks_pass_and_summary() {
        let report = run_suite(&SUITE);
        let text = report.render_report();
        assert!(text.contains("PASS"));
        // Full pass -> 100%.
        assert!(text.contains("100%"), "summary should show 100%: {text}");
        // Every case id appears.
        for case in SUITE.iter() {
            assert!(text.contains(case.id), "report missing id {}", case.id);
        }
    }

    #[test]
    fn render_json_is_valid_and_complete() {
        let report = run_suite(&SUITE);
        let json = report.render_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["total"], SUITE.len());
        assert_eq!(parsed["passed"], SUITE.len());
        assert!((parsed["score"].as_f64().unwrap() - 1.0).abs() < 1e-9);
        assert_eq!(parsed["results"].as_array().unwrap().len(), SUITE.len());
        assert_eq!(parsed["results"][0]["status"], "passed");
    }

    #[test]
    fn cases_are_brand_free() {
        // No user-facing leakage of upstream framework brands in case data.
        let banned = ["goose", "block", "anthropic", "openai", "codex"];
        for case in SUITE.iter() {
            let hay = format!("{} {} {}", case.id, case.input, case.expected).to_lowercase();
            for b in banned {
                assert!(!hay.contains(b), "case '{}' leaks brand '{}'", case.id, b);
            }
        }
    }

    #[test]
    fn handle_bench_paths_do_not_error() {
        // Smoke: the handler paths return Ok and never touch the network.
        handle_bench(BenchOptions {
            list: true,
            ..BenchOptions::default()
        })
        .unwrap();
        handle_bench(BenchOptions::default()).unwrap();
        handle_bench(BenchOptions {
            live: true,
            ..BenchOptions::default()
        })
        .unwrap();
        handle_bench(BenchOptions {
            json: true,
            ..BenchOptions::default()
        })
        .unwrap();
    }
}
