//! Agent-capability toggles surfaced through the standard config accessor path.
//!
//! The v41-v49 agent-capability features are each driven by a `BHARATCODE_*`
//! environment variable / config key and default OFF so behaviour is unchanged
//! when nothing is set. This module gives `bharatcode configure`/doctor a single
//! place to *read* those toggles through the typed `get_bharatcode_*` getters
//! registered in `config/base.rs` (which already layer env over the config file),
//! rather than scattering raw `std::env::var` lookups across the codebase.
//!
//! The resolution mirrors the structured-feature accessors in `config::base`:
//! every value is read via `Config::global().get_param::<String>()`, which
//! checks the uppercase environment variable first and then the merged config
//! file. A missing key resolves to the default (off / empty).

use crate::config::Config;

/// Toggle: spawn dedicated subagents for delegated work.
pub const SUBAGENTS_KEY: &str = "BHARATCODE_SUBAGENTS";
/// Toggle: persist the working plan to a plan file.
pub const PLAN_FILE_KEY: &str = "BHARATCODE_PLAN_FILE";
/// Toggle: build/consult a codebase index for retrieval.
pub const CODEBASE_INDEX_KEY: &str = "BHARATCODE_CODEBASE_INDEX";
/// Toggle: render compact diffs in tool output.
pub const DIFF_COMPACT_KEY: &str = "BHARATCODE_DIFF_COMPACT";
/// Override: model used for the planning step (empty = use the main model).
pub const PLANNER_MODEL_KEY: &str = "BHARATCODE_PLANNER_MODEL";
/// Toggle: run config/data migrations on startup.
pub const MIGRATE_KEY: &str = "BHARATCODE_MIGRATE";

/// Every capability key in registration order. Used for summary rendering and
/// to keep the unit test in lock-step with the resolved struct.
pub const CAP_KEYS: &[&str] = &[
    SUBAGENTS_KEY,
    PLAN_FILE_KEY,
    CODEBASE_INDEX_KEY,
    DIFF_COMPACT_KEY,
    PLANNER_MODEL_KEY,
    MIGRATE_KEY,
];

/// Resolved snapshot of the agent-capability toggles.
///
/// Boolean toggles default to `false`; `planner_model` defaults to `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentCaps {
    pub subagents: bool,
    pub plan_file: bool,
    pub codebase_index: bool,
    pub diff_compact: bool,
    pub planner_model: Option<String>,
    pub migrate: bool,
}

/// Interpret a raw config/env string as a boolean toggle. Anything not
/// recognised as "on" (and a blank value) reads as off, so a stray value never
/// silently enables a capability.
fn parse_toggle(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "on" | "yes" | "enabled"
    )
}

/// Read a single key as a trimmed string. The raw environment variable is
/// consulted first (so a bare `1`/`true` survives as a string rather than being
/// coerced to a number by the config parser), then the merged config file via
/// the standard accessor path. Returns `None` when the key is absent or resolves
/// to an empty string. Mirrors the env-first pattern in `memory_store::is_enabled`.
fn read_key(config: &Config, key: &str) -> Option<String> {
    if let Ok(raw) = std::env::var(key) {
        let trimmed = raw.trim().to_string();
        return if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        };
    }
    config
        .get_param::<String>(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn read_toggle(config: &Config, key: &str) -> bool {
    read_key(config, key).is_some_and(|v| parse_toggle(&v))
}

impl AgentCaps {
    /// Resolve the toggles from a specific config (used by tests and callers
    /// that already hold a `Config`).
    pub fn from_config(config: &Config) -> Self {
        AgentCaps {
            subagents: read_toggle(config, SUBAGENTS_KEY),
            plan_file: read_toggle(config, PLAN_FILE_KEY),
            codebase_index: read_toggle(config, CODEBASE_INDEX_KEY),
            diff_compact: read_toggle(config, DIFF_COMPACT_KEY),
            planner_model: read_key(config, PLANNER_MODEL_KEY),
            migrate: read_toggle(config, MIGRATE_KEY),
        }
    }

    /// Whether a given capability key reads as enabled in this snapshot. For
    /// the planner-model override, "enabled" means a non-empty model was set.
    pub fn is_enabled(&self, key: &str) -> bool {
        match key {
            SUBAGENTS_KEY => self.subagents,
            PLAN_FILE_KEY => self.plan_file,
            CODEBASE_INDEX_KEY => self.codebase_index,
            DIFF_COMPACT_KEY => self.diff_compact,
            PLANNER_MODEL_KEY => self.planner_model.is_some(),
            MIGRATE_KEY => self.migrate,
            _ => false,
        }
    }
}

/// Resolve the agent-capability toggles from the global config.
pub fn resolve() -> AgentCaps {
    AgentCaps::from_config(Config::global())
}

/// One `key = on/off` row per capability, in registration order, for display in
/// `bharatcode configure`/doctor. The planner-model override additionally shows
/// the selected model when one is set.
pub fn summary_lines() -> Vec<String> {
    summary_lines_for(&resolve())
}

/// Like [`summary_lines`] but resolved from a specific `Config` rather than the
/// global one. This is the real call site reached from `Config`'s public API.
pub fn summary_lines_for_config(config: &Config) -> Vec<String> {
    summary_lines_for(&AgentCaps::from_config(config))
}

fn summary_lines_for(caps: &AgentCaps) -> Vec<String> {
    CAP_KEYS
        .iter()
        .map(|key| {
            let state = if caps.is_enabled(key) { "on" } else { "off" };
            if *key == PLANNER_MODEL_KEY {
                if let Some(model) = &caps.planner_model {
                    return format!("{key} = {model}");
                }
            }
            format!("{key} = {state}")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_keys_unset() -> [(&'static str, Option<&'static str>); 6] {
        [
            (SUBAGENTS_KEY, None),
            (PLAN_FILE_KEY, None),
            (CODEBASE_INDEX_KEY, None),
            (DIFF_COMPACT_KEY, None),
            (PLANNER_MODEL_KEY, None),
            (MIGRATE_KEY, None),
        ]
    }

    #[test]
    fn resolve_defaults_to_all_off() {
        let _guard = env_lock::lock_env(all_keys_unset());
        let caps = resolve();
        assert_eq!(caps, AgentCaps::default());
        assert!(!caps.subagents);
        assert!(!caps.plan_file);
        assert!(!caps.codebase_index);
        assert!(!caps.diff_compact);
        assert!(caps.planner_model.is_none());
        assert!(!caps.migrate);
    }

    #[test]
    fn subagents_env_enables_toggle() {
        let mut keys = all_keys_unset();
        keys[0] = (SUBAGENTS_KEY, Some("1"));
        let _guard = env_lock::lock_env(keys);
        let caps = resolve();
        assert!(caps.subagents);
        // Other toggles stay off.
        assert!(!caps.plan_file);
        assert!(!caps.migrate);
    }

    #[test]
    fn planner_model_override_is_captured() {
        let mut keys = all_keys_unset();
        keys[4] = (PLANNER_MODEL_KEY, Some("planner-x"));
        let _guard = env_lock::lock_env(keys);
        let caps = resolve();
        assert_eq!(caps.planner_model.as_deref(), Some("planner-x"));
    }

    #[test]
    fn summary_lines_match_keys() {
        let _guard = env_lock::lock_env(all_keys_unset());
        let lines = summary_lines();
        assert_eq!(lines.len(), CAP_KEYS.len());
        for key in CAP_KEYS {
            assert!(
                lines.iter().any(|line| line.starts_with(key)),
                "summary missing key {key}"
            );
        }
        assert!(lines.iter().all(|line| line.ends_with("off")));
    }

    #[test]
    fn summary_lines_reflect_enabled_state() {
        let caps = AgentCaps {
            subagents: true,
            planner_model: Some("planner-x".to_string()),
            ..AgentCaps::default()
        };
        let lines = summary_lines_for(&caps);
        assert_eq!(lines.len(), CAP_KEYS.len());
        assert!(lines
            .iter()
            .any(|line| line == &format!("{SUBAGENTS_KEY} = on")));
        assert!(lines
            .iter()
            .any(|line| line == &format!("{PLANNER_MODEL_KEY} = planner-x")));
    }

    #[test]
    fn unrecognised_value_reads_as_off() {
        let mut keys = all_keys_unset();
        keys[2] = (CODEBASE_INDEX_KEY, Some("maybe"));
        let _guard = env_lock::lock_env(keys);
        assert!(!resolve().codebase_index);
    }
}
