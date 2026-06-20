//! CI-friendly footer for `bharatcode cost`.
//!
//! When `bharatcode cost` runs inside a CI job, a human-readable table is hard
//! to consume: pipelines want a single machine-parseable line, and GitHub
//! Actions wants its own `::notice::` / `::error::` workflow annotations so the
//! spend (and whether it breached the configured budget) shows up in the run
//! summary. This module supplies those two extra outputs.
//!
//! Design:
//!   * **Default OFF for normal terminals.** The extra lines are only emitted
//!     when [`is_ci`] reports a CI environment, so an interactive `cost` run is
//!     byte-identical to before this module existed.
//!   * **CI detection is raw-env only.** [`is_ci`] reads `CI`,
//!     `GITHUB_ACTIONS` and the explicit `BHARATCODE_CI` override directly from
//!     the process environment (mirroring [`crate::commands::cost_extensions`]),
//!     so a bare `1` or `true` is honoured without the config layer coercing it.
//!   * **Rendering is pure.** [`ci_footer_json`] and [`github_annotation`] are
//!     side-effect-free string builders, unit-tested below.
//!
//! Original BharatCode work; not ported from any third party.

/// Environment key that forces the CI footer on regardless of the usual CI
/// signals. Useful for testing the footer locally.
pub const BHARATCODE_CI_KEY: &str = "BHARATCODE_CI";
/// Generic CI signal honoured by virtually every CI provider.
pub const CI_KEY: &str = "CI";
/// GitHub Actions signal; also gates workflow-command annotations.
pub const GITHUB_ACTIONS_KEY: &str = "GITHUB_ACTIONS";

/// Whether a raw environment value is a truthy spelling (`1`, `true`, `yes`,
/// `on`). Anything else — including absence — is falsey.
fn env_truthy(key: &str) -> bool {
    match std::env::var(key) {
        Ok(raw) => matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
}

/// Whether this process is running under CI.
///
/// True when any of `CI`, `GITHUB_ACTIONS`, or the explicit `BHARATCODE_CI`
/// override is set to a truthy value. With all three unset (or falsey) this is
/// `false`, so the `cost` command keeps its default human-only output.
pub fn is_ci() -> bool {
    env_truthy(CI_KEY) || env_truthy(GITHUB_ACTIONS_KEY) || env_truthy(BHARATCODE_CI_KEY)
}

/// Whether this process is specifically running under GitHub Actions, which is
/// the only environment where `::notice::` / `::error::` workflow commands mean
/// anything.
fn is_github_actions() -> bool {
    env_truthy(GITHUB_ACTIONS_KEY)
}

/// Whether `day_inr` breaches the configured `budget_inr` cap.
///
/// With no budget configured (`None`) or a non-positive / non-finite cap there
/// is nothing to breach, so this is always `false`.
fn is_over_budget(day_inr: f64, budget_inr: Option<f64>) -> bool {
    match budget_inr {
        Some(cap) if cap.is_finite() && cap > 0.0 && day_inr.is_finite() => day_inr > cap,
        _ => false,
    }
}

/// Build the single-line machine-readable JSON footer.
///
/// The object always carries `day_inr`, `month_inr`, `budget_inr` (JSON `null`
/// when no cap is configured) and an `over_budget` boolean. It is emitted on its
/// own line so a CI step can `grep` for it and pipe it into `jq`.
pub fn ci_footer_json(day_inr: f64, month_inr: f64, budget_inr: Option<f64>) -> String {
    let over_budget = is_over_budget(day_inr, budget_inr);
    let value = serde_json::json!({
        "day_inr": day_inr,
        "month_inr": month_inr,
        "budget_inr": budget_inr,
        "over_budget": over_budget,
    });
    value.to_string()
}

/// Build a GitHub Actions workflow annotation for the given spend message.
///
/// Returns `None` unless running under GitHub Actions (so non-GHA CI gets only
/// the JSON footer). Under GHA it returns an `::error::` line when over budget
/// and a `::notice::` line otherwise, with `msg` carried as the annotation
/// body.
pub fn github_annotation(over_budget: bool, msg: &str) -> Option<String> {
    if !is_github_actions() {
        return None;
    }
    let level = if over_budget { "error" } else { "notice" };
    Some(format!("::{level}::{msg}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialize CI-env mutation so concurrently-running tests in this module do
    /// not clobber each other's view of the process environment.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn clear_ci_env() {
        std::env::remove_var(CI_KEY);
        std::env::remove_var(GITHUB_ACTIONS_KEY);
        std::env::remove_var(BHARATCODE_CI_KEY);
    }

    #[test]
    fn is_ci_false_when_all_unset() {
        let _g = env_lock();
        clear_ci_env();
        assert!(!is_ci());
        assert!(github_annotation(false, "x").is_none());
    }

    #[test]
    fn is_ci_true_for_each_signal() {
        let _g = env_lock();
        for key in [CI_KEY, GITHUB_ACTIONS_KEY, BHARATCODE_CI_KEY] {
            clear_ci_env();
            std::env::set_var(key, "true");
            assert!(is_ci(), "{key}=true should be CI");
        }
        clear_ci_env();
    }

    #[test]
    fn footer_round_trips_to_object_with_expected_keys() {
        let s = ci_footer_json(12.5, 340.0, Some(500.0));
        let v: serde_json::Value = serde_json::from_str(&s).expect("footer is valid JSON");
        assert!(v.is_object());
        let obj = v.as_object().unwrap();
        for key in ["day_inr", "month_inr", "budget_inr", "over_budget"] {
            assert!(obj.contains_key(key), "missing key {key}");
        }
        assert_eq!(obj["day_inr"], serde_json::json!(12.5));
        assert_eq!(obj["over_budget"], serde_json::json!(false));
        // Single line: no embedded newlines so CI can grep one line.
        assert!(!s.contains('\n'));
    }

    #[test]
    fn over_budget_true_when_day_exceeds_cap() {
        let s = ci_footer_json(600.0, 600.0, Some(500.0));
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["over_budget"], serde_json::json!(true));
        assert_eq!(v["budget_inr"], serde_json::json!(500.0));
    }

    #[test]
    fn over_budget_false_without_cap() {
        let s = ci_footer_json(9_999.0, 9_999.0, None);
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["over_budget"], serde_json::json!(false));
        assert_eq!(v["budget_inr"], serde_json::Value::Null);
    }

    #[test]
    fn annotation_none_unless_github_actions() {
        let _g = env_lock();
        clear_ci_env();
        // Even with generic CI set, a non-GHA runner gets no annotation.
        std::env::set_var(CI_KEY, "true");
        assert!(github_annotation(true, "over").is_none());
        clear_ci_env();
    }

    #[test]
    fn annotation_levels_under_github_actions() {
        let _g = env_lock();
        clear_ci_env();
        std::env::set_var(GITHUB_ACTIONS_KEY, "true");

        let err = github_annotation(true, "over budget").expect("GHA emits annotation");
        assert!(err.starts_with("::error::"), "got {err}");
        assert!(err.contains("over budget"));

        let note = github_annotation(false, "within budget").expect("GHA emits annotation");
        assert!(note.starts_with("::notice::"), "got {note}");

        clear_ci_env();
    }

    #[test]
    fn output_never_mentions_internal_brand() {
        // The machine-readable surface must not leak the upstream project name.
        let footer = ci_footer_json(1.0, 2.0, Some(3.0)).to_lowercase();
        assert!(!footer.contains("goose"));
        let _g = env_lock();
        clear_ci_env();
        std::env::set_var(GITHUB_ACTIONS_KEY, "true");
        let annotation = github_annotation(true, "spend report")
            .unwrap()
            .to_lowercase();
        assert!(!annotation.contains("goose"));
        clear_ci_env();
    }
}
