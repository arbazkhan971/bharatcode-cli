//! `bharatcode bench` — an offline benchmark / eval harness.
//!
//! This module ships a deterministic, fully-offline eval harness. A fixed set
//! of embedded [`BenchCase`]s — each an `id`, a `prompt`, and an objective
//! [`Grader`] — is run through one headless agent turn against the active local
//! provider. The assistant text for each case is graded (substring-present,
//! regex-match, or non-empty), the turn is timed, and a scored [`BenchReport`]
//! is produced: pass/fail per case, an aggregate pass-rate, and p50/p95
//! wall-times.
//!
//! The deliberate split here is *data vs. execution*. The cases, the graders,
//! the percentile math, the pass-rate computation, and both report renderers
//! are pure and provider-independent, so the whole grading surface is
//! unit-testable without ever spinning up a provider. Only [`handle_bench`]
//! touches the agent.
//!
//! Per-case wall-time is clamped by the `BHARATCODE_BENCH_TIMEOUT_SECS`
//! environment variable (default 60s). Grading itself is ungated and has no
//! side effects.
//!
//! This is also the library entry point for the serve-sessions / eval
//! consumers, which call [`handle_bench`] directly; the embedded
//! `recipes/bench.yaml` data artifact documents the contract and surfaces the
//! harness the same way the recipe library surfaces its templates.

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::session::{build_session, SessionBuilderConfig};

/// The embedded `recipes/bench.yaml` data artifact, surfaced so the recipe
/// contract that documents this harness is reachable as crate API (mirroring
/// how the recipe library embeds its templates via `include_str!`).
pub const BENCH_RECIPE_YAML: &str = include_str!("../../../../recipes/bench.yaml");

/// Default per-case wall-time clamp, in seconds, when
/// `BHARATCODE_BENCH_TIMEOUT_SECS` is unset or unparseable.
const DEFAULT_TIMEOUT_SECS: u64 = 60;

/// Upper bound for the per-case wall-time clamp. A benchmark case should never
/// be allowed to hang a run for more than a few minutes regardless of what the
/// environment requests.
const MAX_TIMEOUT_SECS: u64 = 600;

/// An objective, deterministic grader for a single benchmark case.
///
/// Every variant is a pure predicate over the captured assistant text; given
/// the same output, a grader always returns the same verdict. This is what
/// keeps the harness reproducible and testable without a provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Grader {
    /// Pass when the output contains the given substring (case-sensitive).
    SubstringPresent(String),
    /// Pass when the output matches the given regular expression anywhere.
    ///
    /// An invalid pattern never panics: it simply fails the case.
    RegexMatch(String),
    /// Pass when the output, trimmed of surrounding whitespace, is non-empty.
    NonEmpty,
}

/// A single embedded benchmark case: a stable id, the prompt sent through one
/// headless agent turn, and the objective grader applied to the result.
#[derive(Debug, Clone)]
pub struct BenchCase {
    /// Stable, unique identifier used for filtering (`--case <id>`) and report
    /// rows.
    pub id: &'static str,
    /// The prompt sent to the agent for this case.
    pub prompt: &'static str,
    /// The objective grader applied to the captured assistant text.
    pub grader: Grader,
}

/// The embedded benchmark suite.
///
/// Cases are intentionally small, self-contained, and brand-free: each one asks
/// the model to do something with an objectively checkable answer (echo a
/// token, perform trivial arithmetic, follow a simple format instruction) so a
/// regex / substring / non-empty grader can verify it deterministically. Ids
/// are unique; this is asserted in the tests.
///
/// `Grader` carries owned `String` patterns, so the table is built once on
/// first access via [`LazyLock`] rather than as a plain `const`/`static`
/// initializer (which cannot allocate). It is otherwise immutable, embedded,
/// and side-effect free — the same shape callers expect from a `static` slice.
pub static CASES: LazyLock<Vec<BenchCase>> = LazyLock::new(|| {
    vec![
        BenchCase {
            id: "echo-token",
            prompt: "Reply with exactly this token and nothing else: ALPHA-7421",
            grader: Grader::SubstringPresent("ALPHA-7421".to_string()),
        },
        BenchCase {
            id: "arithmetic-sum",
            prompt: "What is 17 plus 25? Answer with just the number.",
            grader: Grader::RegexMatch(r"\b42\b".to_string()),
        },
        BenchCase {
            id: "yes-no-format",
            prompt: "Is the integer 10 an even number? Answer with a single word: Yes or No.",
            grader: Grader::RegexMatch(r"(?i)\byes\b".to_string()),
        },
        BenchCase {
            id: "list-three-colors",
            prompt: "List exactly three primary colors, separated by commas.",
            grader: Grader::RegexMatch(r"(?i)red".to_string()),
        },
        BenchCase {
            id: "non-empty-reply",
            prompt: "Write one short sentence describing what a compiler does.",
            grader: Grader::NonEmpty,
        },
        BenchCase {
            id: "json-key",
            prompt:
                "Output a JSON object with a single key \"status\" whose value is the string \"ok\".",
            grader: Grader::RegexMatch(r#"(?i)"status"\s*:\s*"ok""#.to_string()),
        },
    ]
});

/// Options accepted by [`handle_bench`].
#[derive(Debug, Clone, Default)]
pub struct BenchOptions {
    /// When `Some`, run only the case whose id matches; otherwise run the full
    /// suite.
    pub case: Option<String>,
    /// When `true`, emit the machine-readable JSON report; otherwise render the
    /// human-readable table.
    pub json: bool,
}

/// The graded outcome of a single benchmark case.
#[derive(Debug, Clone, PartialEq)]
pub struct CaseResult {
    /// The id of the case this result is for.
    pub id: String,
    /// Whether the case passed its grader.
    pub passed: bool,
    /// Wall-time of the headless turn, in milliseconds.
    pub duration_ms: u64,
    /// An optional note (e.g. an error or a timeout) for diagnostics. Empty on
    /// a clean pass/fail.
    pub note: String,
}

/// A scored benchmark report: per-case results plus aggregates.
#[derive(Debug, Clone, PartialEq)]
pub struct BenchReport {
    /// One [`CaseResult`] per case that ran, in run order.
    pub results: Vec<CaseResult>,
    /// Fraction of cases that passed, in `0.0..=1.0`. `0.0` for an empty run.
    pub pass_rate: f64,
    /// 50th-percentile wall-time across cases, in milliseconds.
    pub p50_ms: u64,
    /// 95th-percentile wall-time across cases, in milliseconds.
    pub p95_ms: u64,
}

/// Pure grader: returns whether `output` satisfies grader `g`.
///
/// This is the single source of truth for case verdicts and is deliberately
/// free of I/O so it can be exhaustively unit-tested. An invalid regex pattern
/// fails closed (returns `false`) rather than panicking.
pub fn grade(output: &str, g: &Grader) -> bool {
    match g {
        Grader::SubstringPresent(needle) => output.contains(needle.as_str()),
        Grader::RegexMatch(pattern) => regex::Regex::new(pattern)
            .map(|re| re.is_match(output))
            .unwrap_or(false),
        Grader::NonEmpty => !output.trim().is_empty(),
    }
}

/// Compute the `pct`-th percentile (e.g. `50.0`, `95.0`) over a slice of
/// durations, in milliseconds, using the nearest-rank method.
///
/// Returns `0` for an empty input. The slice is sorted internally, so callers
/// need not pre-sort. Nearest-rank keeps the result a value that actually
/// occurred, which is the right shape for "p50/p95 wall-time" reporting.
pub fn percentile_ms(durations: &[u64], pct: f64) -> u64 {
    if durations.is_empty() {
        return 0;
    }
    let mut sorted: Vec<u64> = durations.to_vec();
    sorted.sort_unstable();
    let n = sorted.len();
    // Nearest-rank: rank = ceil(pct/100 * n), 1-based, clamped to [1, n].
    let rank = ((pct / 100.0) * n as f64).ceil() as usize;
    let idx = rank.clamp(1, n) - 1;
    sorted[idx]
}

/// Compute the pass-rate (fraction of passing results) over `results`.
///
/// Returns `0.0` for an empty slice.
pub fn pass_rate(results: &[CaseResult]) -> f64 {
    if results.is_empty() {
        return 0.0;
    }
    let passed = results.iter().filter(|r| r.passed).count();
    passed as f64 / results.len() as f64
}

impl BenchReport {
    /// Build a report from a vector of per-case results, computing the
    /// aggregates (pass-rate and p50/p95 wall-time).
    pub fn from_results(results: Vec<CaseResult>) -> Self {
        let durations: Vec<u64> = results.iter().map(|r| r.duration_ms).collect();
        let pass_rate = pass_rate(&results);
        let p50_ms = percentile_ms(&durations, 50.0);
        let p95_ms = percentile_ms(&durations, 95.0);
        BenchReport {
            results,
            pass_rate,
            p50_ms,
            p95_ms,
        }
    }

    /// Number of cases that passed.
    pub fn passed_count(&self) -> usize {
        self.results.iter().filter(|r| r.passed).count()
    }

    /// Render the report as a human-readable, aligned table.
    pub fn render_table(&self) -> String {
        let mut out = String::new();
        let header_case = crate::tr!("bench.col.case");
        let header_result = crate::tr!("bench.col.result");
        let header_ms = crate::tr!("bench.col.ms");

        // Width the id column to the longest id (or the header), so the table
        // stays aligned regardless of which cases ran.
        let id_width = self
            .results
            .iter()
            .map(|r| r.id.len())
            .chain(std::iter::once(header_case.len()))
            .max()
            .unwrap_or(header_case.len());

        out.push_str(&format!(
            "{:<id_width$}  {:<6}  {:>8}\n",
            header_case,
            header_result,
            header_ms,
            id_width = id_width,
        ));
        out.push_str(&format!("{}\n", "-".repeat(id_width + 2 + 6 + 2 + 8)));

        for r in &self.results {
            let verdict = if r.passed {
                crate::tr!("bench.pass")
            } else {
                crate::tr!("bench.fail")
            };
            out.push_str(&format!(
                "{:<id_width$}  {:<6}  {:>8}",
                r.id,
                verdict,
                r.duration_ms,
                id_width = id_width,
            ));
            if !r.note.is_empty() {
                out.push_str(&format!("  ({})", r.note));
            }
            out.push('\n');
        }

        out.push('\n');
        out.push_str(&format!(
            "{}: {}/{} ({:.0}%)\n",
            crate::tr!("bench.pass_rate"),
            self.passed_count(),
            self.results.len(),
            self.pass_rate * 100.0,
        ));
        out.push_str(&format!(
            "{}: p50={}ms  p95={}ms\n",
            crate::tr!("bench.walltime"),
            self.p50_ms,
            self.p95_ms,
        ));
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
                    "passed": r.passed,
                    "duration_ms": r.duration_ms,
                    "note": r.note,
                })
            })
            .collect();
        let value = serde_json::json!({
            "results": cases,
            "passed": self.passed_count(),
            "total": self.results.len(),
            "pass_rate": self.pass_rate,
            "p50_ms": self.p50_ms,
            "p95_ms": self.p95_ms,
        });
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
    }
}

/// Resolve the per-case wall-time clamp from `BHARATCODE_BENCH_TIMEOUT_SECS`,
/// falling back to the default and clamping to a sane upper bound.
fn resolve_timeout() -> Duration {
    let secs = std::env::var("BHARATCODE_BENCH_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|&v| v > 0)
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
        .min(MAX_TIMEOUT_SECS);
    Duration::from_secs(secs)
}

/// The subset of cases to run, given an optional id filter.
///
/// Returns the matching cases in suite order. An unknown id yields an empty
/// slice, which the caller reports as such.
fn select_cases(filter: Option<&str>) -> Vec<&'static BenchCase> {
    match filter {
        Some(id) => CASES.iter().filter(|c| c.id == id).collect(),
        None => CASES.iter().collect(),
    }
}

/// Run a single case through one headless agent turn and capture the assistant
/// text, clamped to `timeout`.
///
/// Returns the captured text on a clean turn, or a diagnostic note on
/// timeout/error. This is the only provider-touching function in the module.
async fn run_case(case: &BenchCase, timeout: Duration) -> std::result::Result<String, String> {
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

    let turn = tokio::time::timeout(timeout, session.headless(case.prompt.to_string())).await;

    match turn {
        Err(_) => Err(crate::tr!("bench.timeout")),
        Ok(Err(e)) => Err(e.to_string()),
        Ok(Ok(())) => {
            let history = session.message_history();
            let text = history
                .iter()
                .rev()
                .find(|m| m.role == rmcp::model::Role::Assistant)
                .map(|m| m.as_concat_text())
                .unwrap_or_default();
            Ok(text)
        }
    }
}

/// Entry point for the `bharatcode bench` surface and for the serve-sessions /
/// eval consumers.
///
/// Runs each (filtered) embedded case through one headless agent turn, grades
/// the captured assistant text, times the turn, and prints the scored report
/// (table or JSON per `opts.json`).
pub async fn handle_bench(opts: BenchOptions) -> Result<()> {
    let timeout = resolve_timeout();
    let cases = select_cases(opts.case.as_deref());

    if cases.is_empty() {
        // An explicit filter that matched nothing is a usage error worth
        // surfacing rather than printing an empty, confusing report.
        if let Some(id) = opts.case.as_deref() {
            anyhow::bail!("{}: {}", crate::tr!("bench.unknown_case"), id);
        }
        anyhow::bail!("{}", crate::tr!("bench.no_cases"));
    }

    let mut results = Vec::with_capacity(cases.len());
    for case in cases {
        let start = Instant::now();
        let outcome = run_case(case, timeout).await;
        let duration_ms = start.elapsed().as_millis() as u64;

        let (passed, note) = match outcome {
            Ok(text) => (grade(&text, &case.grader), String::new()),
            Err(note) => (false, note),
        };

        results.push(CaseResult {
            id: case.id.to_string(),
            passed,
            duration_ms,
            note,
        });
    }

    let report = BenchReport::from_results(results);

    if opts.json {
        println!("{}", report.render_json());
    } else {
        print!("{}", report.render_table());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(id: &str, passed: bool, ms: u64) -> CaseResult {
        CaseResult {
            id: id.to_string(),
            passed,
            duration_ms: ms,
            note: String::new(),
        }
    }

    // ---- Grader truth-table: one assertion per variant, both verdicts. ----

    #[test]
    fn grade_substring_present() {
        let g = Grader::SubstringPresent("ALPHA-7421".to_string());
        assert!(grade("the answer is ALPHA-7421 indeed", &g));
        assert!(!grade("no token here", &g));
        // Case-sensitive by design.
        assert!(!grade("alpha-7421", &g));
    }

    #[test]
    fn grade_regex_match() {
        let g = Grader::RegexMatch(r"\b42\b".to_string());
        assert!(grade("the result is 42 exactly", &g));
        assert!(!grade("the result is 420", &g));
        assert!(!grade("no number", &g));
    }

    #[test]
    fn grade_regex_case_insensitive_flag() {
        let g = Grader::RegexMatch(r"(?i)\byes\b".to_string());
        assert!(grade("Yes", &g));
        assert!(grade("the answer is YES", &g));
        assert!(!grade("no", &g));
    }

    #[test]
    fn grade_regex_invalid_pattern_fails_closed() {
        // An unbalanced group is an invalid regex; it must fail, not panic.
        let g = Grader::RegexMatch(r"(unterminated".to_string());
        assert!(!grade("anything at all", &g));
    }

    #[test]
    fn grade_non_empty() {
        let g = Grader::NonEmpty;
        assert!(grade("x", &g));
        assert!(grade("  hello  ", &g));
        assert!(!grade("", &g));
        assert!(!grade("   \n\t  ", &g));
    }

    // ---- Percentile math: feed known durations, assert p50/p95. ----

    #[test]
    fn percentile_known_durations() {
        // 1..=10, nearest-rank: p50 -> rank ceil(0.5*10)=5 -> value 5;
        // p95 -> rank ceil(0.95*10)=10 -> value 10.
        let d: Vec<u64> = (1..=10).collect();
        assert_eq!(percentile_ms(&d, 50.0), 5);
        assert_eq!(percentile_ms(&d, 95.0), 10);
        assert_eq!(percentile_ms(&d, 100.0), 10);
    }

    #[test]
    fn percentile_unsorted_input() {
        // Sorting is internal; order should not matter.
        let d = vec![50, 10, 30, 20, 40];
        assert_eq!(percentile_ms(&d, 50.0), 30);
        assert_eq!(percentile_ms(&d, 95.0), 50);
    }

    #[test]
    fn percentile_single_and_empty() {
        assert_eq!(percentile_ms(&[7], 50.0), 7);
        assert_eq!(percentile_ms(&[7], 95.0), 7);
        assert_eq!(percentile_ms(&[], 50.0), 0);
        assert_eq!(percentile_ms(&[], 95.0), 0);
    }

    // ---- pass_rate over a fixed CaseResult vec. ----

    #[test]
    fn pass_rate_computation() {
        let results = vec![
            result("a", true, 10),
            result("b", false, 20),
            result("c", true, 30),
            result("d", true, 40),
        ];
        // 3 of 4 passed.
        assert!((pass_rate(&results) - 0.75).abs() < 1e-9);
    }

    #[test]
    fn pass_rate_all_and_none_and_empty() {
        assert!((pass_rate(&[result("a", true, 1)]) - 1.0).abs() < 1e-9);
        assert!((pass_rate(&[result("a", false, 1)]) - 0.0).abs() < 1e-9);
        assert!((pass_rate(&[]) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn report_aggregates_wired() {
        let results = vec![
            result("a", true, 10),
            result("b", false, 20),
            result("c", true, 30),
        ];
        let report = BenchReport::from_results(results);
        assert_eq!(report.passed_count(), 2);
        assert!((report.pass_rate - (2.0 / 3.0)).abs() < 1e-9);
        // durations sorted [10,20,30]: p50 rank ceil(1.5)=2 -> 20; p95 rank
        // ceil(2.85)=3 -> 30.
        assert_eq!(report.p50_ms, 20);
        assert_eq!(report.p95_ms, 30);
    }

    // ---- Renderers stay non-empty and consistent with the data. ----

    #[test]
    fn render_table_contains_aggregates() {
        let report = BenchReport::from_results(vec![
            result("alpha", true, 12),
            result("beta", false, 34),
        ]);
        let table = report.render_table();
        assert!(table.contains("alpha"));
        assert!(table.contains("beta"));
        // 1 of 2 passed -> 50%.
        assert!(table.contains("50%"));
        assert!(table.contains("p50="));
        assert!(table.contains("p95="));
    }

    #[test]
    fn render_json_is_valid_and_complete() {
        let report = BenchReport::from_results(vec![
            result("alpha", true, 12),
            result("beta", false, 34),
        ]);
        let json = report.render_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["total"], 2);
        assert_eq!(parsed["passed"], 1);
        assert_eq!(parsed["results"].as_array().unwrap().len(), 2);
        assert!((parsed["pass_rate"].as_f64().unwrap() - 0.5).abs() < 1e-9);
        assert_eq!(parsed["results"][0]["id"], "alpha");
        assert_eq!(parsed["results"][0]["passed"], true);
    }

    // ---- CASES table invariants: unique ids, brand-free prompts. ----

    #[test]
    fn cases_have_unique_ids() {
        let mut seen = std::collections::HashSet::new();
        for c in CASES.iter() {
            assert!(!c.id.is_empty(), "case id must not be empty");
            assert!(seen.insert(c.id), "duplicate case id: {}", c.id);
        }
    }

    #[test]
    fn cases_are_brand_free() {
        // No user-facing leakage of the upstream framework brands.
        let banned = ["goose", "block", "anthropic", "openai", "codex"];
        for c in CASES.iter() {
            let p = c.prompt.to_lowercase();
            for b in banned {
                assert!(
                    !p.contains(b),
                    "case '{}' prompt leaks banned brand '{}'",
                    c.id,
                    b
                );
            }
        }
    }

    #[test]
    fn cases_graders_are_well_formed() {
        // Every embedded regex grader must compile; substrings must be
        // non-empty.
        for c in CASES.iter() {
            match &c.grader {
                Grader::RegexMatch(pat) => {
                    assert!(
                        regex::Regex::new(pat).is_ok(),
                        "case '{}' has invalid regex: {}",
                        c.id,
                        pat
                    );
                }
                Grader::SubstringPresent(s) => {
                    assert!(!s.is_empty(), "case '{}' has empty substring grader", c.id);
                }
                Grader::NonEmpty => {}
            }
            assert!(!c.prompt.is_empty(), "case '{}' has empty prompt", c.id);
        }
    }

    // ---- Case selection / timeout resolution. ----

    #[test]
    fn select_cases_filters_by_id() {
        let one = select_cases(Some("echo-token"));
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].id, "echo-token");

        let none = select_cases(Some("does-not-exist"));
        assert!(none.is_empty());

        let all = select_cases(None);
        assert_eq!(all.len(), CASES.len());
    }

    #[test]
    fn timeout_clamps_and_defaults() {
        // Default when unset is covered indirectly; here assert the clamp bound.
        // We can't easily mutate process env safely across threads, so assert
        // the constants the resolver relies on.
        assert_eq!(DEFAULT_TIMEOUT_SECS, 60);
        assert!(MAX_TIMEOUT_SECS >= DEFAULT_TIMEOUT_SECS);
    }

    #[test]
    fn recipe_yaml_is_embedded_and_branded_cleanly() {
        // The embedded recipe artifact must be present and brand-free.
        assert!(BENCH_RECIPE_YAML.contains("Offline Benchmark"));
        let lower = BENCH_RECIPE_YAML.to_lowercase();
        for b in ["goose", "block "] {
            assert!(!lower.contains(b), "recipe yaml leaks brand: {}", b);
        }
    }
}
