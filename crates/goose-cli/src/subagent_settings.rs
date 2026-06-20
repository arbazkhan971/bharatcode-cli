//! Resolved runtime budget for delegated sub-tasks.
//!
//! Subagents (delegated sub-tasks) inherit a small set of runtime knobs: how
//! many turns each one may take, how many may run at once, and an optional
//! model override. Historically each `TaskConfig` re-read the relevant env var
//! on its own. This module resolves those knobs **once**, at session-build
//! time, so every delegated sub-task in a session inherits a single, consistent
//! and user-tunable budget.
//!
//! Settings are read from config (env vars, or the matching config-file keys):
//!
//! - `BHARATCODE_SUBAGENT_MAX_TURNS` — max turns per sub-task. Default is the
//!   existing [`DEFAULT_SUBAGENT_MAX_TURNS`] (25), clamped to `1..=100`.
//! - `BHARATCODE_SUBAGENT_MAX_CONCURRENT` — how many sub-tasks may run at once.
//!   Default `2`, clamped to `1..=8`.
//! - `BHARATCODE_SUBAGENT_MODEL` — optional model override for sub-tasks. Unset
//!   leaves the parent's model in effect.
//!
//! With none of these set the resolved settings reproduce today's behavior
//! exactly, so this is purely additive.

use goose::config::Config;

/// Default maximum number of turns a delegated sub-task may take when unset.
///
/// Mirrors `goose::agents::subagent_task_config::DEFAULT_SUBAGENT_MAX_TURNS`
/// (which lives in a `pub(crate)` module and is not re-exported), duplicated
/// here to keep this module self-contained. Same precedent as
/// `goose::checks::DEFAULT_CHECK_TURN_LIMIT`.
pub const DEFAULT_SUBAGENT_MAX_TURNS: usize = 25;

/// Default number of sub-tasks allowed to run concurrently when unset.
pub const DEFAULT_SUBAGENT_MAX_CONCURRENT: usize = 2;

/// Inclusive bounds for `max_turns`.
const MAX_TURNS_RANGE: std::ops::RangeInclusive<usize> = 1..=100;
/// Inclusive bounds for `max_concurrent`.
const MAX_CONCURRENT_RANGE: std::ops::RangeInclusive<usize> = 1..=8;

/// Runtime budget for delegated sub-tasks, resolved once per session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentSettings {
    /// Maximum number of turns a single sub-task may take.
    pub max_turns: usize,
    /// Maximum number of sub-tasks that may run concurrently.
    pub max_concurrent: usize,
    /// Optional model override for sub-tasks; `None` keeps the parent's model.
    pub model: Option<String>,
}

impl Default for SubagentSettings {
    fn default() -> Self {
        Self {
            max_turns: DEFAULT_SUBAGENT_MAX_TURNS,
            max_concurrent: DEFAULT_SUBAGENT_MAX_CONCURRENT,
            model: None,
        }
    }
}

impl SubagentSettings {
    /// Resolve subagent settings from config, falling back to the defaults
    /// (current behavior) for anything not set. Numeric values are clamped to
    /// their supported ranges so a misconfigured budget can never starve or
    /// runaway the delegation machinery.
    pub fn from_config(config: &Config) -> Self {
        let mut settings = Self::default();

        if let Ok(max_turns) = config.get_param::<usize>("BHARATCODE_SUBAGENT_MAX_TURNS") {
            settings.max_turns = max_turns.clamp(*MAX_TURNS_RANGE.start(), *MAX_TURNS_RANGE.end());
        }

        if let Ok(max_concurrent) = config.get_param::<usize>("BHARATCODE_SUBAGENT_MAX_CONCURRENT")
        {
            settings.max_concurrent =
                max_concurrent.clamp(*MAX_CONCURRENT_RANGE.start(), *MAX_CONCURRENT_RANGE.end());
        }

        if let Ok(model) = config.get_param::<String>("BHARATCODE_SUBAGENT_MODEL") {
            let model = model.trim();
            if !model.is_empty() {
                settings.model = Some(model.to_string());
            }
        }

        settings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    /// Serializes env-mutating tests in this module so they can't clobber each
    /// other's process-global environment.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Restores the three subagent env vars to their pre-test state on drop and
    /// holds the serialization lock for the duration of a test.
    struct EnvGuard {
        _lock: MutexGuard<'static, ()>,
        prev: Vec<(&'static str, Option<String>)>,
    }

    impl EnvGuard {
        fn new() -> Self {
            let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let keys = [
                "BHARATCODE_SUBAGENT_MAX_TURNS",
                "BHARATCODE_SUBAGENT_MAX_CONCURRENT",
                "BHARATCODE_SUBAGENT_MODEL",
            ];
            let prev = keys
                .iter()
                .map(|k| {
                    let v = std::env::var(k).ok();
                    std::env::remove_var(k);
                    (*k, v)
                })
                .collect();
            Self { _lock: lock, prev }
        }

        fn set(&self, key: &str, value: &str) {
            std::env::set_var(key, value);
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in &self.prev {
                match value {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    /// A `Config` backed by empty temp files so `get_param` resolves purely from
    /// the process environment.
    fn env_only_config() -> (Config, tempfile::NamedTempFile, tempfile::NamedTempFile) {
        let config_file = tempfile::NamedTempFile::new().expect("config temp file");
        let secrets_file = tempfile::NamedTempFile::new().expect("secrets temp file");
        let config = Config::new_with_file_secrets(config_file.path(), secrets_file.path())
            .expect("config from temp files");
        (config, config_file, secrets_file)
    }

    #[test]
    fn unset_matches_current_behavior() {
        let guard = EnvGuard::new();
        let (config, _c, _s) = env_only_config();

        let settings = SubagentSettings::from_config(&config);

        assert_eq!(settings.max_turns, DEFAULT_SUBAGENT_MAX_TURNS);
        assert_eq!(settings.max_turns, 25);
        assert_eq!(settings.max_concurrent, 2);
        assert_eq!(settings.model, None);

        drop(guard);
    }

    #[test]
    fn reads_explicit_max_turns() {
        let guard = EnvGuard::new();
        guard.set("BHARATCODE_SUBAGENT_MAX_TURNS", "3");
        let (config, _c, _s) = env_only_config();

        let settings = SubagentSettings::from_config(&config);

        assert_eq!(settings.max_turns, 3);

        drop(guard);
    }

    #[test]
    fn clamps_max_turns_to_upper_bound() {
        let guard = EnvGuard::new();
        guard.set("BHARATCODE_SUBAGENT_MAX_TURNS", "9999");
        let (config, _c, _s) = env_only_config();

        let settings = SubagentSettings::from_config(&config);

        assert_eq!(settings.max_turns, 100);

        drop(guard);
    }

    #[test]
    fn clamps_max_turns_to_lower_bound() {
        let guard = EnvGuard::new();
        guard.set("BHARATCODE_SUBAGENT_MAX_TURNS", "0");
        let (config, _c, _s) = env_only_config();

        let settings = SubagentSettings::from_config(&config);

        assert_eq!(settings.max_turns, 1);

        drop(guard);
    }

    #[test]
    fn reads_and_clamps_max_concurrent() {
        let guard = EnvGuard::new();
        guard.set("BHARATCODE_SUBAGENT_MAX_CONCURRENT", "4");
        let (config, _c, _s) = env_only_config();
        assert_eq!(SubagentSettings::from_config(&config).max_concurrent, 4);

        guard.set("BHARATCODE_SUBAGENT_MAX_CONCURRENT", "99");
        let (config, _c, _s) = env_only_config();
        assert_eq!(SubagentSettings::from_config(&config).max_concurrent, 8);

        drop(guard);
    }

    #[test]
    fn reads_model_override() {
        let guard = EnvGuard::new();
        guard.set("BHARATCODE_SUBAGENT_MODEL", "some-model");
        let (config, _c, _s) = env_only_config();

        let settings = SubagentSettings::from_config(&config);

        assert_eq!(settings.model.as_deref(), Some("some-model"));

        drop(guard);
    }
}
