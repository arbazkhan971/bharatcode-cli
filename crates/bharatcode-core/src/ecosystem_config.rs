//! Ecosystem-extensibility toggles surfaced through the standard config path.
//!
//! The wave-v71-v80 ecosystem features (block catalog, project scripts, CI
//! integration, external-advisory checks, recipe lockfiles, cost extensions)
//! are each driven by a `BHARATCODE_*` environment variable / config key and
//! default OFF so behaviour is unchanged when nothing is set. This module gives
//! `bharatcode configure`/doctor a single place to *read* those toggles through
//! the typed `get_bharatcode_*` getters registered in `config/base.rs` (which
//! already layer env over the config file), rather than scattering raw
//! `std::env::var` lookups across the codebase.
//!
//! The resolution mirrors the structured-feature accessors in `config::base`
//! and the proven v50 `agent_caps` reader: every value is read via `read_key`,
//! which checks the uppercase environment variable first (so a bare `1`/`true`
//! survives as a string rather than being coerced to a number by the config
//! parser) and then the merged config file. A missing key resolves to the
//! default (off / empty).

use crate::config::Config;

/// Toggle: enable the local block/recipe catalog surface.
pub const CATALOG_KEY: &str = "BHARATCODE_CATALOG";
/// Toggle: enable project-defined scripts.
pub const SCRIPTS_KEY: &str = "BHARATCODE_SCRIPTS";
/// Toggle: enable CI integration hooks.
pub const CI_KEY: &str = "BHARATCODE_CI";
/// Toggle: enable external-advisory (vulnerability) checks.
pub const EXT_ADVISORY_KEY: &str = "BHARATCODE_EXT_ADVISORY";
/// Toggle: enable recipe lockfile pinning.
pub const RECIPE_LOCK_KEY: &str = "BHARATCODE_RECIPE_LOCK";
/// Toggle: enable cost-tracking extensions.
pub const COST_EXTENSIONS_KEY: &str = "BHARATCODE_COST_EXTENSIONS";

/// Every ecosystem key in registration order. Used for summary rendering and to
/// keep the unit test in lock-step with the resolved struct.
pub const ECOSYSTEM_KEYS: &[&str] = &[
    CATALOG_KEY,
    SCRIPTS_KEY,
    CI_KEY,
    EXT_ADVISORY_KEY,
    RECIPE_LOCK_KEY,
    COST_EXTENSIONS_KEY,
];

/// Resolved snapshot of the ecosystem-extensibility toggles.
///
/// Every toggle defaults to `false`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EcosystemConfig {
    pub catalog: bool,
    pub scripts: bool,
    pub ci: bool,
    pub ext_advisory: bool,
    pub recipe_lock: bool,
    pub cost_extensions: bool,
}

/// Interpret a raw config/env string as a boolean toggle. Anything not
/// recognised as "on" (and a blank value) reads as off, so a stray value never
/// silently enables a feature.
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
/// to an empty string. Mirrors the env-first pattern in `agent_caps::read_key`.
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

impl EcosystemConfig {
    /// Resolve the toggles from a specific config (used by tests and callers
    /// that already hold a `Config`).
    pub fn from_config(config: &Config) -> Self {
        EcosystemConfig {
            catalog: read_toggle(config, CATALOG_KEY),
            scripts: read_toggle(config, SCRIPTS_KEY),
            ci: read_toggle(config, CI_KEY),
            ext_advisory: read_toggle(config, EXT_ADVISORY_KEY),
            recipe_lock: read_toggle(config, RECIPE_LOCK_KEY),
            cost_extensions: read_toggle(config, COST_EXTENSIONS_KEY),
        }
    }

    /// Whether a given ecosystem key reads as enabled in this snapshot.
    pub fn is_enabled(&self, key: &str) -> bool {
        match key {
            CATALOG_KEY => self.catalog,
            SCRIPTS_KEY => self.scripts,
            CI_KEY => self.ci,
            EXT_ADVISORY_KEY => self.ext_advisory,
            RECIPE_LOCK_KEY => self.recipe_lock,
            COST_EXTENSIONS_KEY => self.cost_extensions,
            _ => false,
        }
    }
}

/// Resolve the ecosystem-extensibility toggles from the global config.
pub fn resolve() -> EcosystemConfig {
    EcosystemConfig::from_config(Config::global())
}

/// One `key = on/off` row per ecosystem toggle, in registration order, for
/// display in `bharatcode configure`/doctor.
pub fn summary_lines() -> Vec<String> {
    summary_lines_for(&resolve())
}

/// Like [`summary_lines`] but resolved from a specific `Config` rather than the
/// global one. This is the real call site reached from `Config`'s public API
/// (`Config::ecosystem_summary`).
pub fn summary_lines_for_config(config: &Config) -> Vec<String> {
    summary_lines_for(&EcosystemConfig::from_config(config))
}

fn summary_lines_for(eco: &EcosystemConfig) -> Vec<String> {
    ECOSYSTEM_KEYS
        .iter()
        .map(|key| {
            let state = if eco.is_enabled(key) { "on" } else { "off" };
            format!("{key} = {state}")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_keys_unset() -> [(&'static str, Option<&'static str>); 6] {
        [
            (CATALOG_KEY, None),
            (SCRIPTS_KEY, None),
            (CI_KEY, None),
            (EXT_ADVISORY_KEY, None),
            (RECIPE_LOCK_KEY, None),
            (COST_EXTENSIONS_KEY, None),
        ]
    }

    #[test]
    fn resolve_defaults_to_all_off() {
        let _guard = env_lock::lock_env(all_keys_unset());
        let eco = resolve();
        assert_eq!(eco, EcosystemConfig::default());
        assert!(!eco.catalog);
        assert!(!eco.scripts);
        assert!(!eco.ci);
        assert!(!eco.ext_advisory);
        assert!(!eco.recipe_lock);
        assert!(!eco.cost_extensions);
    }

    #[test]
    fn summary_rows_all_off_when_unset() {
        let _guard = env_lock::lock_env(all_keys_unset());
        let lines = summary_lines();
        assert_eq!(lines.len(), ECOSYSTEM_KEYS.len());
        for key in ECOSYSTEM_KEYS {
            assert!(
                lines.iter().any(|line| line.starts_with(key)),
                "summary missing key {key}"
            );
        }
        assert!(
            lines.iter().all(|line| line.ends_with("off")),
            "expected every row off when unset: {lines:?}"
        );
    }

    #[test]
    fn ci_env_flips_only_its_row() {
        let mut keys = all_keys_unset();
        // Index 2 is CI_KEY in registration order. A bare `1` must be honored
        // as a string toggle, not coerced to a number.
        keys[2] = (CI_KEY, Some("1"));
        let _guard = env_lock::lock_env(keys);

        let eco = resolve();
        assert!(eco.ci, "CI toggle should be on when BHARATCODE_CI=1");
        // Every other toggle stays off.
        assert!(!eco.catalog);
        assert!(!eco.scripts);
        assert!(!eco.ext_advisory);
        assert!(!eco.recipe_lock);
        assert!(!eco.cost_extensions);

        let lines = summary_lines();
        assert!(
            lines.iter().any(|line| line == &format!("{CI_KEY} = on")),
            "CI row should read on: {lines:?}"
        );
        // All non-CI rows stay off.
        for line in &lines {
            if line.starts_with(CI_KEY) {
                continue;
            }
            assert!(line.ends_with("off"), "non-CI row unexpectedly on: {line}");
        }
    }

    #[test]
    fn bare_one_is_honored_as_string_not_number() {
        let mut keys = all_keys_unset();
        keys[3] = (EXT_ADVISORY_KEY, Some("1"));
        let _guard = env_lock::lock_env(keys);
        // read_key returns the raw string "1" (env-first), and parse_toggle
        // recognises it as on. A numeric coercion would have rejected the bare
        // value before it reached the toggle parser.
        assert_eq!(
            read_key(&Config::global(), EXT_ADVISORY_KEY).as_deref(),
            Some("1")
        );
        assert!(resolve().ext_advisory);
    }

    #[test]
    fn unrecognised_value_reads_as_off() {
        let mut keys = all_keys_unset();
        keys[1] = (SCRIPTS_KEY, Some("maybe"));
        let _guard = env_lock::lock_env(keys);
        assert!(!resolve().scripts);
    }

    #[test]
    fn no_vendor_leakage_in_labels() {
        let _guard = env_lock::lock_env(all_keys_unset());
        for line in summary_lines() {
            let lower = line.to_ascii_lowercase();
            assert!(
                !lower.contains("goose"),
                "row label leaks vendor token: {line}"
            );
            assert!(
                !line.contains("Block"),
                "row label leaks vendor token: {line}"
            );
        }
    }
}
