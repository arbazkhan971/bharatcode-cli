//! Scripting / automation API: a JSONL command-runner plan (BharatCode v75).
//!
//! This module gives BharatCode a small, non-interactive *scripting surface*: an
//! automation script is a list of typed steps expressed as JSON Lines (one JSON
//! object per non-blank line). [`parse_jsonl`] validates that text into a
//! [`Script`] plan, and [`execute_offline`] runs the **non-LLM** steps
//! deterministically, returning a structured [`ScriptReport`] with per-step
//! pass/fail.
//!
//! The split is deliberate. Steps come in two flavours:
//!
//! * **Pure / offline steps** — [`Step::SetEnv`], [`Step::AssertContains`], and
//!   [`Step::Comment`] are evaluated here with no I/O beyond the process
//!   environment, so the whole runner is unit-testable and free of hidden side
//!   effects.
//! * **Agent-bound steps** — [`Step::RunPrompt`] needs a live agent/model to mean
//!   anything. This module never reaches for one: `execute_offline` records such a
//!   step as *skipped* with a `requires-agent` detail and leaves the actual prompt
//!   dispatch to the CLI/headless consumer. That keeps this module a pure library.
//!
//! The API is **default-inert**: nothing here runs unless a caller explicitly
//! invokes it, so wiring it into the crate's public surface does not change any
//! default behaviour. There is no environment gate because there is nothing to
//! gate — construction and execution are entirely caller-driven.
//!
//! This module is original work; nothing here is ported from third-party sources.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

/// A single typed step in an automation script.
///
/// The wire form is a tagged JSON object whose `type` field selects the variant,
/// e.g. `{"type":"set-env","key":"MODE","value":"ci"}`. An unknown `type` is a
/// hard error at parse time (see [`parse_jsonl`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Step {
    /// Dispatch a prompt to the agent. Agent-bound: not executed offline.
    RunPrompt {
        /// The prompt text to send to the agent.
        prompt: String,
    },
    /// Set a process environment variable for subsequent steps.
    SetEnv {
        /// Environment variable name.
        key: String,
        /// Value to assign.
        value: String,
    },
    /// Assert that the last captured output contains `needle`.
    #[serde(rename = "assert-output-contains")]
    AssertContains {
        /// Substring that must be present in the last output.
        needle: String,
    },
    /// A no-op annotation carried through for readability / reporting.
    Comment {
        /// Free-form comment text.
        text: String,
    },
}

/// A parsed, validated automation plan: an ordered list of [`Step`]s.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Script {
    /// The steps to run, in order.
    pub steps: Vec<Step>,
}

impl Script {
    /// Number of steps in the plan.
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Whether the plan has no steps.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

/// Outcome of executing a single step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepResult {
    /// `true` when the step passed (or was a benign skip/no-op), `false` on a
    /// failed assertion.
    pub ok: bool,
    /// Human-readable, machine-stable detail describing the outcome.
    pub detail: String,
}

/// Structured report produced by [`execute_offline`]: one [`StepResult`] per step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ScriptReport {
    /// Per-step results, in plan order.
    pub results: Vec<StepResult>,
}

impl ScriptReport {
    /// Whether every step passed (an empty report passes vacuously).
    pub fn all_ok(&self) -> bool {
        self.results.iter().all(|r| r.ok)
    }

    /// Count of failed steps.
    pub fn failures(&self) -> usize {
        self.results.iter().filter(|r| !r.ok).count()
    }
}

/// Parse a JSONL automation script: one JSON object per non-blank line.
///
/// Blank and whitespace-only lines are ignored so scripts may be visually spaced.
/// Each remaining line must be a single tagged [`Step`] object; an unknown `type`
/// (or otherwise malformed line) is reported as an error rather than silently
/// skipped, so a typo never quietly drops a step.
pub fn parse_jsonl(input: &str) -> Result<Script> {
    let mut steps = Vec::new();
    for (idx, raw) in input.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let step: Step = serde_json::from_str(line)
            .map_err(|e| anyhow::anyhow!("line {}: invalid automation step: {e}", idx + 1))?;
        steps.push(step);
    }
    Ok(Script { steps })
}

/// Execute the non-LLM steps of `script` deterministically against `last_output`.
///
/// * [`Step::SetEnv`] sets the variable in the process environment and passes.
/// * [`Step::AssertContains`] passes iff `last_output` contains the needle.
/// * [`Step::Comment`] is a benign no-op that always passes.
/// * [`Step::RunPrompt`] is agent-bound and is recorded as skipped
///   (`requires-agent`); the caller is responsible for dispatching it and may
///   update `last_output` between runs.
///
/// `last_output` is the rolling "last captured output" the script asserts over;
/// it is taken by mutable reference so an agent-aware caller can thread real
/// output through across invocations.
pub fn execute_offline(script: &Script, last_output: &mut String) -> ScriptReport {
    let mut results = Vec::with_capacity(script.steps.len());
    for step in &script.steps {
        let result = match step {
            Step::SetEnv { key, value } => {
                std::env::set_var(key, value);
                StepResult {
                    ok: true,
                    detail: format!("set-env: {key} set"),
                }
            }
            Step::AssertContains { needle } => {
                let ok = last_output.contains(needle.as_str());
                StepResult {
                    ok,
                    detail: if ok {
                        format!("assert-output-contains: found {needle:?}")
                    } else {
                        format!("assert-output-contains: missing {needle:?}")
                    },
                }
            }
            Step::Comment { text } => StepResult {
                ok: true,
                detail: format!("comment: {text}"),
            },
            Step::RunPrompt { .. } => StepResult {
                ok: true,
                detail: "run-prompt: skipped (requires-agent)".to_string(),
            },
        };
        results.push(result);
    }
    ScriptReport { results }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_jsonl_yields_one_step_per_line() {
        let input = "\
{\"type\":\"comment\",\"text\":\"start\"}
{\"type\":\"set-env\",\"key\":\"BHARATCODE_SCRIPT_MODE\",\"value\":\"ci\"}
{\"type\":\"run-prompt\",\"prompt\":\"summarise the diff\"}
{\"type\":\"assert-output-contains\",\"needle\":\"ok\"}
";
        let script = parse_jsonl(input).expect("valid 4-line script parses");
        assert_eq!(script.len(), 4);
        assert!(matches!(script.steps[0], Step::Comment { .. }));
        assert!(matches!(script.steps[1], Step::SetEnv { .. }));
        assert!(matches!(script.steps[2], Step::RunPrompt { .. }));
        assert!(matches!(script.steps[3], Step::AssertContains { .. }));
    }

    #[test]
    fn parse_jsonl_ignores_blank_lines() {
        let input = "\n{\"type\":\"comment\",\"text\":\"only\"}\n\n   \n";
        let script = parse_jsonl(input).expect("blank lines are skipped");
        assert_eq!(script.len(), 1);
    }

    #[test]
    fn parse_jsonl_rejects_unknown_step_type() {
        let err = parse_jsonl("{\"type\":\"bogus\"}").unwrap_err();
        // The error must name the offending content, not silently drop it.
        assert!(err.to_string().contains("invalid automation step"));
    }

    #[test]
    fn execute_offline_marks_pass_fail_and_sets_env() {
        let key = "BHARATCODE_SCRIPT_TEST_KEY";
        std::env::remove_var(key);
        let script = Script {
            steps: vec![
                Step::SetEnv {
                    key: key.to_string(),
                    value: "applied".to_string(),
                },
                Step::AssertContains {
                    needle: "present".to_string(),
                },
                Step::AssertContains {
                    needle: "absent".to_string(),
                },
            ],
        };

        let mut last_output = String::from("the value present here");
        let report = execute_offline(&script, &mut last_output);

        assert_eq!(report.results.len(), 3);
        assert!(report.results[0].ok, "set-env passes");
        assert!(report.results[1].ok, "needle present -> ok");
        assert!(!report.results[2].ok, "needle absent -> fail");
        assert_eq!(report.failures(), 1);
        assert!(!report.all_ok());

        assert_eq!(std::env::var(key).as_deref(), Ok("applied"));
        std::env::remove_var(key);
    }

    #[test]
    fn run_prompt_is_skipped_offline() {
        let script = Script {
            steps: vec![Step::RunPrompt {
                prompt: "do a thing".to_string(),
            }],
        };
        let mut out = String::new();
        let report = execute_offline(&script, &mut out);
        assert!(report.results[0].ok, "skip is benign, not a failure");
        assert!(report.results[0].detail.contains("requires-agent"));
    }

    #[test]
    fn no_user_facing_product_name_leaks_in_details() {
        let script = Script {
            steps: vec![
                Step::SetEnv {
                    key: "K".to_string(),
                    value: "v".to_string(),
                },
                Step::AssertContains {
                    needle: "x".to_string(),
                },
                Step::Comment {
                    text: "note".to_string(),
                },
                Step::RunPrompt {
                    prompt: "p".to_string(),
                },
            ],
        };
        let mut out = String::from("x");
        let report = execute_offline(&script, &mut out);
        for r in &report.results {
            let lower = r.detail.to_ascii_lowercase();
            assert!(!lower.contains("goose"), "detail leaks name: {}", r.detail);
            assert!(!lower.contains("block"), "detail leaks name: {}", r.detail);
        }
        std::env::remove_var("K");
    }
}
