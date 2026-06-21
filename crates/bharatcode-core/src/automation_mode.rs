//! Scripting/automation context block.
//!
//! When opted in, this module contributes a compact directive to the system
//! prompt that makes non-interactive/CI runs deterministic: the agent does not
//! ask clarifying questions, prefers non-interactive tooling, and ends with a
//! machine-parseable `STATUS:` line. This makes scripted `bharatcode run`
//! sessions reliable without changing interactive behavior.
//!
//! The feature is opt-in and **off by default**: when `BHARATCODE_AUTOMATION`
//! is unset (or falsey) [`automation_block`] returns `None` and the assembled
//! prompt is byte-identical to the unflagged build. Any truthy-ish value (`1`,
//! `true`, `yes`, `on`) enables it. This module is original work; nothing here
//! is ported from third-party sources.

use indoc::indoc;

/// Opt-in toggle name, shared by env var and config file.
const ENABLE_KEY: &str = "BHARATCODE_AUTOMATION";

/// Stable directive injected into the system prompt when automation is enabled.
/// The `# Automation Mode` heading is a stable anchor relied on by tests and
/// callers; the `STATUS:` contract gives scripts a machine-parseable marker.
const AUTOMATION_DIRECTIVE: &str = indoc! {"
    # Automation Mode

    You are running in automation mode: this is a non-interactive, scripted/CI
    run with no human watching to answer prompts. Behave deterministically.

    - Do not ask the user clarifying questions; choose sane, conventional
      defaults and proceed.
    - Prefer non-interactive tooling: pass non-interactive flags (for example
      `--yes`, `--no-input`, `--non-interactive`) and never block on a prompt.
    - Avoid commands that wait for input, open pagers, or require a TTY.
    - End your final message with a single machine-parseable status line of the
      form `STATUS: <SUCCESS|FAILURE> <one-line summary>` so scripts can parse
      the outcome.
"};

/// Whether automation mode is enabled. Opt-in via the `BHARATCODE_AUTOMATION`
/// environment variable or the config value of the same name. The raw env var
/// is read first (mirroring `memory_store::is_enabled`) so a bare `1` survives
/// without going through config parsing. Any truthy-ish value (`1`, `true`,
/// `yes`, `on`) enables it; defaults to `false` when unset.
pub fn is_enabled() -> bool {
    if let Ok(raw) = std::env::var(ENABLE_KEY) {
        return is_truthy(&raw);
    }
    crate::config::Config::global()
        .get_param::<String>(ENABLE_KEY)
        .map(|raw| is_truthy(&raw))
        .unwrap_or(false)
}

fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// The automation directive to inject into the system prompt, or `None` when
/// the feature is disabled. Callers wire this in with a single `if let` so the
/// prompt is unchanged unless the flag is set.
pub fn automation_block() -> Option<String> {
    if !is_enabled() {
        return None;
    }
    Some(AUTOMATION_DIRECTIVE.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialise tests that mutate the shared process env so the
    /// `BHARATCODE_AUTOMATION` toggle does not race across threads.
    fn env_guard(value: Option<&str>) -> env_lock::EnvGuard<'_> {
        env_lock::lock_env([(ENABLE_KEY, value)])
    }

    #[test]
    fn disabled_yields_none_when_unset() {
        let _guard = env_guard(Some("0"));
        assert!(!is_enabled());
        assert!(automation_block().is_none());
    }

    #[test]
    fn enabled_yields_block_with_markers_and_no_leak() {
        let _guard = env_guard(Some("1"));
        assert!(is_enabled());

        let block = automation_block().expect("flag set yields a block");
        let lower = block.to_ascii_lowercase();
        assert!(lower.contains("automation"));
        assert!(block.contains("STATUS:"));
        // Zero user-facing upstream-name leakage.
        assert!(!lower.contains("goose"));
        assert!(!block.contains("Block"));
    }

    #[test]
    fn falsey_value_stays_disabled() {
        let _guard = env_guard(Some("0"));
        assert!(!is_enabled());
        assert!(automation_block().is_none());
    }

    #[test]
    fn is_truthy_recognizes_common_values() {
        assert!(is_truthy("1"));
        assert!(is_truthy("TRUE"));
        assert!(is_truthy(" yes "));
        assert!(is_truthy("on"));
        assert!(!is_truthy("0"));
        assert!(!is_truthy("false"));
        assert!(!is_truthy(""));
    }
}
