//! Named subagent delegation profiles.
//!
//! A small, opt-in registry of purpose-built sub-task agents (e.g. a tester,
//! reviewer, or refactorer) that resolve into the existing
//! [`TaskConfig`](crate::agents::subagent_task_config::TaskConfig) plumbing.
//! A parent turn can look up a profile by name and apply its bounds (such as a
//! turn budget) before delegating a focused sub-task to a freshly configured
//! subagent.
//!
//! The feature is opt-in: [`enabled`] returns `false` unless the
//! `BHARATCODE_SUBAGENTS` config value or environment variable is set to a
//! truthy value. When disabled, callers fall back to their default behaviour
//! and the registry has no effect, so default behaviour is unchanged.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use crate::agents::subagent_task_config::TaskConfig;

/// Opt-in toggle name, shared by env var and config file.
const ENABLE_KEY: &str = "BHARATCODE_SUBAGENTS";

/// A named, purpose-built subagent definition.
///
/// Profiles are bounded delegation targets: the `instructions` describe the
/// agent's remit, `max_turns` optionally caps how long it may run, and
/// `model_hint` optionally suggests a model better suited to the task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentProfile {
    /// Stable identifier used to look the profile up (e.g. `"tester"`).
    pub name: String,
    /// System-prompt-style guidance describing the agent's focused remit.
    pub instructions: String,
    /// Optional cap on how many turns the delegated agent may take.
    pub max_turns: Option<usize>,
    /// Optional hint at a model better suited to this kind of sub-task.
    pub model_hint: Option<String>,
}

impl SubagentProfile {
    fn new(
        name: &str,
        instructions: &str,
        max_turns: Option<usize>,
        model_hint: Option<&str>,
    ) -> Self {
        Self {
            name: name.to_string(),
            instructions: instructions.to_string(),
            max_turns,
            model_hint: model_hint.map(str::to_string),
        }
    }

    /// Apply this profile's bounds onto a clone of `base`, returning a
    /// [`TaskConfig`] ready to hand to the subagent runner.
    ///
    /// Only the profile's `max_turns` is layered on (via the existing
    /// [`TaskConfig::with_max_turns`] builder); when the profile leaves
    /// `max_turns` unset the base configuration's value is preserved.
    pub fn profile_to_task_config(&self, base: &TaskConfig) -> TaskConfig {
        base.clone().with_max_turns(self.max_turns)
    }
}

/// Built-in profiles, keyed by name and kept sorted for stable listing.
static BUILTIN_PROFILES: LazyLock<BTreeMap<String, SubagentProfile>> = LazyLock::new(|| {
    let mut profiles = BTreeMap::new();
    for profile in [
        SubagentProfile::new(
            "tester",
            "You are a focused test-writing subagent. Write and run targeted \
             tests for the requested behaviour, cover edge cases, and report \
             concrete pass/fail results. Do not refactor unrelated code.",
            Some(15),
            None,
        ),
        SubagentProfile::new(
            "reviewer",
            "You are a focused code-review subagent. Review the requested diff \
             for correctness, clarity, and risk. Return concise, actionable \
             findings; do not modify files unless explicitly asked.",
            Some(10),
            None,
        ),
        SubagentProfile::new(
            "refactorer",
            "You are a focused refactoring subagent. Improve the structure and \
             readability of the requested code without changing its observable \
             behaviour, and keep edits minimal and build-safe.",
            Some(20),
            None,
        ),
    ] {
        profiles.insert(profile.name.clone(), profile);
    }
    profiles
});

/// Resolve a profile by name, returning a clone when one exists.
///
/// Returns `None` for unknown names so callers can fall back to their default
/// (non-delegated) behaviour.
pub fn resolve(name: &str) -> Option<SubagentProfile> {
    BUILTIN_PROFILES.get(name).cloned()
}

/// Whether named subagent delegation is enabled. Opt-in via the
/// `BHARATCODE_SUBAGENTS` environment variable or the config value of the same
/// name. Any truthy-ish value (`1`, `true`, `yes`, `on`) enables it.
pub fn enabled() -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::message::Message;
    use crate::providers::base::{stream_from_single_message, MessageStream, Provider};
    use async_trait::async_trait;
    use goose_providers::conversation::token_usage::{ProviderUsage, Usage};
    use goose_providers::model::ModelConfig;
    use rmcp::model::Tool;
    use std::sync::Arc;

    #[derive(Clone)]
    struct StubProvider {
        model_config: ModelConfig,
    }

    #[async_trait]
    impl Provider for StubProvider {
        fn get_name(&self) -> &str {
            "stub"
        }

        fn get_model_config(&self) -> ModelConfig {
            self.model_config.clone()
        }

        async fn stream(
            &self,
            _model_config: &ModelConfig,
            _session_id: &str,
            _system: &str,
            _messages: &[Message],
            _tools: &[Tool],
        ) -> Result<MessageStream, goose_providers::errors::ProviderError> {
            let message = Message::assistant().with_text("ok");
            let usage = ProviderUsage::new("stub".to_string(), Usage::default());
            Ok(stream_from_single_message(message, usage))
        }
    }

    fn stub_task_config() -> TaskConfig {
        let model_config = ModelConfig::new("test-model").unwrap();
        let provider = Arc::new(StubProvider { model_config });
        TaskConfig::new(
            provider,
            "parent-session",
            std::path::Path::new("/tmp"),
            vec![],
        )
        .with_max_turns(Some(99))
    }

    #[test]
    fn resolve_known_profile_has_instructions() {
        let profile = resolve("tester").expect("tester profile is built in");
        assert_eq!(profile.name, "tester");
        assert!(!profile.instructions.is_empty());
    }

    #[test]
    fn resolve_unknown_profile_is_none() {
        assert!(resolve("nope").is_none());
    }

    #[test]
    fn enabled_is_false_when_unset() {
        // The env var must be unset for the default (opt-out) path to hold.
        assert!(std::env::var(ENABLE_KEY).is_err());
        assert!(!enabled());
    }

    #[test]
    fn is_truthy_accepts_common_forms() {
        for v in ["1", "true", "YES", " on "] {
            assert!(is_truthy(v), "{v:?} should be truthy");
        }
        for v in ["0", "false", "no", ""] {
            assert!(!is_truthy(v), "{v:?} should not be truthy");
        }
    }

    #[test]
    fn profile_to_task_config_carries_max_turns() {
        let base = stub_task_config();
        assert_eq!(base.max_turns, Some(99));

        let tester = resolve("tester").expect("tester profile is built in");
        let derived = tester.profile_to_task_config(&base);

        assert_eq!(derived.max_turns, tester.max_turns);
        assert_eq!(derived.max_turns, Some(15));
        // The base config is left untouched.
        assert_eq!(base.max_turns, Some(99));
    }
}
