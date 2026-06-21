//! Explicit plan-mode planner block.
//!
//! When opted in, this module contributes a compact `# Plan First` directive to
//! the system prompt. The directive instructs the model to emit a numbered,
//! file-scoped plan and wait for the user's confirmation before invoking any
//! mutating tool (writes, edits, shell commands that change state).
//!
//! This deepens the existing `/plan` slash-command flow rather than duplicating
//! it: the slash command lets a user enter plan mode interactively, while this
//! block makes the *planner contract* explicit and stable in the prompt so the
//! model plans before mutating even when the flag is the only signal.
//!
//! The feature is opt-in and **off by default**: when `BHARATCODE_PLAN` is unset
//! (or falsey) [`plan_block`] returns `None` and the assembled prompt is
//! byte-identical to the unflagged build. Any truthy-ish value (`1`, `true`,
//! `yes`, `on`) enables it. This module is original work; nothing here is ported
//! from third-party sources.

use indoc::indoc;

/// Opt-in toggle name, read from the process environment.
const ENABLE_KEY: &str = "BHARATCODE_PLAN";

/// Stable directive injected into the system prompt when the planner is enabled.
/// The `# Plan First` heading is a stable anchor relied on by tests and callers.
const PLAN_DIRECTIVE: &str = indoc! {"
    # Plan First

    Plan before you mutate. Before invoking any tool that writes, edits, or
    otherwise changes state (file writes, patches, shell commands with side
    effects), first present a plan and wait for explicit user confirmation.

    The plan MUST:
    1. Be a numbered list of concrete, ordered steps.
    2. Name every file you intend to create, modify, or delete (file-scoped).
    3. Note any commands you intend to run and their effect.
    4. End by asking the user to confirm before you proceed.

    Do not perform any write or other mutating action until the user confirms
    the plan. Read-only exploration (reading files, searching) needs no plan.
"};

/// Whether the explicit planner is enabled. Opt-in via the `BHARATCODE_PLAN`
/// environment variable; any truthy-ish value (`1`, `true`, `yes`, `on`)
/// enables it. Defaults to `false` when unset.
pub fn is_enabled() -> bool {
    std::env::var(ENABLE_KEY)
        .map(|raw| is_truthy(&raw))
        .unwrap_or(false)
}

fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// The planner directive to inject into the system prompt, or `None` when the
/// feature is disabled. Callers wire this in with a single `if let` so the
/// prompt is unchanged unless the flag is set.
pub fn plan_block() -> Option<String> {
    if !is_enabled() {
        return None;
    }
    Some(PLAN_DIRECTIVE.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialise tests that mutate the shared process env so the
    /// `BHARATCODE_PLAN` toggle does not race across threads.
    fn env_guard(value: Option<&str>) -> env_lock::EnvGuard<'_> {
        env_lock::lock_env([(ENABLE_KEY, value)])
    }

    #[test]
    fn disabled_yields_none_when_unset() {
        let _guard = env_guard(None);
        assert!(!is_enabled());
        assert!(plan_block().is_none());
    }

    #[test]
    fn enabled_yields_block_with_stable_anchor() {
        let _guard = env_guard(Some("1"));
        assert!(is_enabled());

        let block = plan_block().expect("flag set yields a block");
        assert!(block.contains("Plan First"));
        // Planner contract surfaces the file-scoped, numbered, confirm-first cues.
        assert!(block.contains("numbered"));
        assert!(block.contains("file"));
        assert!(block.to_ascii_lowercase().contains("confirm"));
    }

    #[test]
    fn falsey_value_stays_disabled() {
        let _guard = env_guard(Some("0"));
        assert!(!is_enabled());
        assert!(plan_block().is_none());
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
