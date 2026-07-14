//! Offline benchmark / eval harness (scenario runner public API) — BharatCode v93.
//!
//! A deterministic, fully *offline* benchmark/eval harness exposed as reachable
//! public library API. The harness has no provider hardwired: a caller supplies
//! an async *turn function* (`prompt -> reply`), and the harness scores the
//! reply against a named suite of scenarios. This lets CI and the run/recipe
//! path drive parity benchmarks across model/provider swaps by running the same
//! suite before and after a change and comparing pass rates and latency.
//!
//! The shape is intentionally two-layer:
//!
//!   * a **pure, offline core** — [`EvalSuite`] / [`EvalScenario`] parsing
//!     (JSON *or* YAML via the same serde model), the per-scenario matching
//!     rules ([`Expect`]: plain substrings, regular expressions, and optional
//!     tool-name expectations), and the deterministic [`EvalReport`] /
//!     [`ScenarioOutcome`] scoring. None of this touches the network or a model.
//!   * a **caller-driven driver** — [`EvalSuite::run`], which awaits the
//!     caller-supplied turn function once per scenario, times it, and grades the
//!     returned [`TurnOutput`]. The harness never constructs a provider itself.
//!
//! No environment variable gates this module: it is a library API driven
//! entirely by its caller, so default binary behaviour is unchanged (nothing
//! runs until a caller invokes [`EvalSuite::run`]).
//!
//! Regex matching uses the `regex` crate already in the tree; substring matching
//! is plain `str::contains`. An invalid regex in a scenario is surfaced as a
//! parse/compile error rather than silently passing.
//!
//! Original BharatCode work; not ported from any third party.

use std::future::Future;
use std::time::Instant;

use regex::Regex;
use serde::{Deserialize, Serialize};

/// Local English-fallback label helper. The `tr!` macro lives in the CLI crate,
/// not in this library crate, so the few user-facing strings the harness emits
/// (only [`EvalReport::summary_line`]) are inlined in English here. This mirrors
/// the established sibling pattern (`security_audit` / `planner_presets`) so
/// localized labels can layer in later without touching call sites.
macro_rules! label {
    ($_key:expr, $default:expr) => {
        $default
    };
}

/// The reply produced by a caller-supplied turn function for one scenario.
///
/// The harness is provider-agnostic: a caller adapts whatever its real turn
/// path returns into this small, offline-friendly shape. `text` is the
/// assistant's final reply; `tool_names` is the ordered list of tool/function
/// names the turn invoked (empty when the turn used no tools).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TurnOutput {
    /// The assistant's final reply text.
    pub text: String,
    /// Names of tools/functions invoked during the turn, in call order.
    pub tool_names: Vec<String>,
}

impl TurnOutput {
    /// Build a text-only turn output (no tools invoked).
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tool_names: Vec::new(),
        }
    }

    /// Attach an expected/observed tool name to this output.
    pub fn with_tool(mut self, name: impl Into<String>) -> Self {
        self.tool_names.push(name.into());
        self
    }
}

/// What a scenario expects of a turn's output.
///
/// All populated expectations must hold for the scenario to pass:
///   * every entry in `contains` must appear as a substring of the reply text;
///   * every entry in `regex` must match the reply text;
///   * every entry in `tools` must appear in the turn's `tool_names`.
///
/// An empty/omitted field imposes no constraint; an [`Expect`] with no
/// populated fields trivially passes (it asserts nothing).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Expect {
    /// Plain substrings that must all appear in the reply text.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contains: Vec<String>,
    /// Regular expressions that must all match the reply text.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub regex: Vec<String>,
    /// Tool/function names that must all have been invoked by the turn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
}

/// A single named scenario: a prompt plus its expectations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvalScenario {
    /// Stable, human-readable scenario name (used in the report).
    pub name: String,
    /// The prompt handed to the caller-supplied turn function.
    pub prompt: String,
    /// What the turn's output must satisfy to pass.
    #[serde(default)]
    pub expect: Expect,
}

/// A named suite of scenarios, parseable from JSON or YAML.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvalSuite {
    /// Suite name (used in the report's summary line).
    pub name: String,
    /// The scenarios to run, in order.
    pub scenarios: Vec<EvalScenario>,
}

/// A compiled regular expression bound to its source pattern, so a match
/// failure can report which expectation was unmet.
struct CompiledRegex {
    source: String,
    regex: Regex,
}

impl Expect {
    /// Pre-compile every regex in this expectation, failing fast on the first
    /// invalid pattern. Pulled out so [`EvalSuite::run`] can surface a malformed
    /// regex as a typed error instead of treating it as a (silent) failure.
    fn compile_regexes(&self) -> Result<Vec<CompiledRegex>, EvalError> {
        self.regex
            .iter()
            .map(|src| {
                Regex::new(src)
                    .map(|regex| CompiledRegex {
                        source: src.clone(),
                        regex,
                    })
                    .map_err(|e| EvalError::InvalidRegex {
                        pattern: src.clone(),
                        message: e.to_string(),
                    })
            })
            .collect()
    }

    /// Grade `output` against this expectation, returning the list of unmet
    /// expectation descriptions (empty == pass). `compiled` must correspond to
    /// `self.regex` (produced by [`Expect::compile_regexes`]).
    fn unmet(&self, output: &TurnOutput, compiled: &[CompiledRegex]) -> Vec<String> {
        let mut failures = Vec::new();
        for needle in &self.contains {
            if !output.text.contains(needle) {
                failures.push(format!("missing substring {needle:?}"));
            }
        }
        for cr in compiled {
            if !cr.regex.is_match(&output.text) {
                failures.push(format!("regex did not match {:?}", cr.source));
            }
        }
        for tool in &self.tools {
            if !output.tool_names.iter().any(|t| t == tool) {
                failures.push(format!("tool {tool:?} was not invoked"));
            }
        }
        failures
    }
}

/// The outcome of running one scenario.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioOutcome {
    /// The scenario's name.
    pub name: String,
    /// Whether every populated expectation held.
    pub passed: bool,
    /// Milliseconds the turn function took for this scenario.
    pub latency_ms: u128,
    /// Human-readable descriptions of unmet expectations (empty on pass).
    pub failures: Vec<String>,
}

/// The aggregate result of running a whole suite.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvalReport {
    /// The suite's name (echoed for the summary line).
    pub suite: String,
    /// Per-scenario outcomes, in suite order.
    pub outcomes: Vec<ScenarioOutcome>,
}

impl EvalReport {
    /// Number of scenarios that passed.
    pub fn passed(&self) -> usize {
        self.outcomes.iter().filter(|o| o.passed).count()
    }

    /// Total number of scenarios in the report.
    pub fn total(&self) -> usize {
        self.outcomes.len()
    }

    /// Fraction of scenarios that passed, in `0.0..=1.0`. An empty report has a
    /// pass rate of `0.0` (nothing passed).
    pub fn pass_rate(&self) -> f64 {
        let total = self.total();
        if total == 0 {
            return 0.0;
        }
        self.passed() as f64 / total as f64
    }

    /// Total latency across all scenarios, in milliseconds.
    pub fn total_latency_ms(&self) -> u128 {
        self.outcomes.iter().map(|o| o.latency_ms).sum()
    }

    /// A single, brand-free summary line carrying the pass count, total, and
    /// pass-rate percentage. Always one line (no embedded newline).
    pub fn summary_line(&self) -> String {
        let label = label!("eval.summary", "eval");
        format!(
            "{label} {suite}: {passed}/{total} passed ({pct:.0}%)",
            suite = self.suite,
            passed = self.passed(),
            total = self.total(),
            pct = self.pass_rate() * 100.0,
        )
    }
}

/// Errors from parsing a suite or compiling a scenario's expectations.
#[derive(Debug)]
pub enum EvalError {
    /// The suite text was not valid JSON nor valid YAML.
    Parse {
        /// The serde_yaml error (YAML is a superset of JSON, so this also
        /// covers JSON-shaped input).
        message: String,
    },
    /// A scenario carried a regex that failed to compile.
    InvalidRegex {
        /// The offending pattern.
        pattern: String,
        /// The compiler's error message.
        message: String,
    },
}

impl std::fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvalError::Parse { message } => {
                write!(f, "could not parse eval suite: {message}")
            }
            EvalError::InvalidRegex { pattern, message } => {
                write!(f, "invalid regex {pattern:?}: {message}")
            }
        }
    }
}

impl std::error::Error for EvalError {}

impl std::str::FromStr for EvalSuite {
    type Err = EvalError;

    /// Parse a suite from JSON or YAML. YAML 1.1 is a superset of JSON, so a
    /// single `serde_yaml` pass accepts both shapes.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_yaml::from_str(s).map_err(|e| EvalError::Parse {
            message: e.to_string(),
        })
    }
}

impl EvalSuite {
    /// Parse a suite from a JSON or YAML string.
    pub fn parse(s: &str) -> Result<Self, EvalError> {
        <Self as std::str::FromStr>::from_str(s)
    }

    /// Run every scenario through the caller-supplied async `turn` function and
    /// produce an [`EvalReport`].
    ///
    /// `turn` is awaited once per scenario, in order, receiving the scenario by
    /// reference and returning the turn's [`TurnOutput`]. The harness times each
    /// call, grades the output against the scenario's [`Expect`], and records a
    /// [`ScenarioOutcome`]. The harness is fully offline: it never constructs a
    /// provider or performs any I/O of its own.
    ///
    /// Returns an error only if a scenario's expectation carries an invalid
    /// regex (surfaced before the turn runs); a turn that simply fails its
    /// expectations is a *failing scenario*, not an error.
    pub async fn run<F, Fut>(&self, mut turn: F) -> Result<EvalReport, EvalError>
    where
        F: FnMut(EvalScenario) -> Fut,
        Fut: Future<Output = TurnOutput>,
    {
        let mut outcomes = Vec::with_capacity(self.scenarios.len());
        for scenario in &self.scenarios {
            // Compile regexes up front so a malformed pattern is a typed error,
            // not a silent scenario failure.
            let compiled = scenario.expect.compile_regexes()?;

            let started = Instant::now();
            let output = turn(scenario.clone()).await;
            let latency_ms = started.elapsed().as_millis();

            let failures = scenario.expect.unmet(&output, &compiled);
            outcomes.push(ScenarioOutcome {
                name: scenario.name.clone(),
                passed: failures.is_empty(),
                latency_ms,
                failures,
            });
        }
        Ok(EvalReport {
            suite: self.name.clone(),
            outcomes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2-scenario suite where an "echo" turn (reply == prompt) passes the
    /// first scenario and fails the second.
    const TWO_SCENARIO_JSON: &str = r#"
    {
      "name": "parity-smoke",
      "scenarios": [
        {
          "name": "greets",
          "prompt": "please say hello world to the user",
          "expect": { "contains": ["hello world"] }
        },
        {
          "name": "computes",
          "prompt": "add the numbers",
          "expect": { "contains": ["the answer is 4"] }
        }
      ]
    }
    "#;

    /// An echoing turn function: the reply is exactly the prompt. With the suite
    /// above this passes scenario 1 (prompt contains "hello world") and fails
    /// scenario 2 (prompt does not contain "the answer is 4").
    async fn echo_turn(scenario: EvalScenario) -> TurnOutput {
        TurnOutput::text(scenario.prompt.clone())
    }

    #[tokio::test]
    async fn parses_runs_and_scores_half() {
        let suite = EvalSuite::parse(TWO_SCENARIO_JSON).expect("suite should parse");
        assert_eq!(suite.scenarios.len(), 2);

        let report = suite.run(echo_turn).await.expect("run should succeed");
        assert_eq!(report.total(), 2);
        assert_eq!(report.passed(), 1);
        assert!((report.pass_rate() - 0.5).abs() < f64::EPSILON);

        // Scenario order is preserved and grading is correct.
        assert!(
            report.outcomes[0].passed,
            "echo passes the hello-world case"
        );
        assert!(!report.outcomes[1].passed, "echo fails the arithmetic case");
        assert!(!report.outcomes[1].failures.is_empty());
    }

    #[tokio::test]
    async fn summary_line_is_one_line_brand_free_with_pass_count() {
        let suite = EvalSuite::parse(TWO_SCENARIO_JSON).unwrap();
        let report = suite.run(echo_turn).await.unwrap();

        let line = report.summary_line();
        assert!(!line.contains('\n'), "summary must be a single line");
        // Carries the pass count.
        assert!(
            line.contains("1/2"),
            "summary should carry the pass count: {line}"
        );
        // Brand-free.
        let lower = line.to_ascii_lowercase();
        assert!(
            !lower.contains("goose"),
            "summary leaked a brand name: {line}"
        );
        assert!(
            !lower.contains("block"),
            "summary leaked a brand name: {line}"
        );
    }

    #[tokio::test]
    async fn regex_expectation_matches() {
        let yaml = r#"
name: regex-suite
scenarios:
  - name: has-digits
    prompt: "the order id is 12345 confirmed"
    expect:
      regex:
        - "id is [0-9]+"
"#;
        let suite = EvalSuite::parse(yaml).expect("yaml suite should parse");
        let report = suite.run(echo_turn).await.expect("run should succeed");
        assert_eq!(report.passed(), 1);
        assert!((report.pass_rate() - 1.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn tool_expectation_is_honoured() {
        let json = r#"
        {
          "name": "tool-suite",
          "scenarios": [
            { "name": "uses-tool", "prompt": "search the web", "expect": { "tools": ["web_search"] } }
          ]
        }
        "#;
        let suite = EvalSuite::parse(json).unwrap();

        // Turn that DID invoke the expected tool -> pass.
        let with_tool = suite
            .run(|s: EvalScenario| async move {
                TurnOutput::text(s.prompt.clone()).with_tool("web_search")
            })
            .await
            .unwrap();
        assert_eq!(with_tool.passed(), 1);

        // Turn that did NOT invoke it -> fail.
        let without_tool = suite.run(echo_turn).await.unwrap();
        assert_eq!(without_tool.passed(), 0);
    }

    #[tokio::test]
    async fn invalid_regex_is_a_typed_error_not_a_silent_fail() {
        let yaml = r#"
name: bad-regex
scenarios:
  - name: broken
    prompt: anything
    expect:
      regex:
        - "("
"#;
        let suite = EvalSuite::parse(yaml).unwrap();
        let err = suite
            .run(echo_turn)
            .await
            .expect_err("invalid regex must error");
        assert!(matches!(err, EvalError::InvalidRegex { .. }));
    }

    #[test]
    fn malformed_suite_is_err() {
        // Neither valid JSON nor valid YAML mapping with the required fields.
        let err = EvalSuite::parse("name: [unterminated").unwrap_err();
        assert!(matches!(err, EvalError::Parse { .. }));
        // A structurally-valid YAML scalar that is not a suite mapping also fails.
        assert!(EvalSuite::parse("just a string").is_err());
    }

    #[test]
    fn empty_report_pass_rate_is_zero() {
        let report = EvalReport {
            suite: "empty".to_string(),
            outcomes: Vec::new(),
        };
        assert_eq!(report.pass_rate(), 0.0);
        assert!(!report.summary_line().contains('\n'));
    }

    #[test]
    fn from_str_and_parse_trait_agree() {
        let direct = EvalSuite::parse(TWO_SCENARIO_JSON).unwrap();
        let viaparse: EvalSuite = TWO_SCENARIO_JSON.parse().unwrap();
        assert_eq!(direct, viaparse);
    }
}
