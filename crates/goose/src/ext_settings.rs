//! Extension/plugin ecosystem config getters surfaced through the standard
//! config accessor path.
//!
//! This wave adds a handful of ecosystem toggles that the plugin/MCP/recipe
//! tooling consults: whether plugins auto-update, whether the MCP registry
//! banner is shown, the default output directory for shared recipes, and the
//! default automation-mode switch. Each is driven by a `BHARATCODE_*`
//! environment variable / config key and defaults to its *current* behaviour
//! (auto-update off, banner off, recipe output in the current directory,
//! automation off) so nothing changes until a key is set.
//!
//! Resolution mirrors the structured-feature accessors in `agent_caps`: the raw
//! environment variable is consulted first (so a bare `1`/`true` survives as a
//! string rather than being coerced by the config parser), then the merged
//! config file via the standard accessor path. This module gives doctor/info a
//! single source of truth for these settings by rendering one
//! `key = value (source: env|config|default)` row per setting through
//! [`summary_lines_for_config`].

use crate::config::Config;

/// Toggle: automatically update installed plugins on a fixed interval.
pub const PLUGIN_AUTO_UPDATE_KEY: &str = "BHARATCODE_PLUGIN_AUTO_UPDATE";
/// Toggle: show the MCP registry banner at startup.
pub const MCP_REGISTRY_BANNER_KEY: &str = "BHARATCODE_MCP_REGISTRY_BANNER";
/// Override: default output directory for shared/exported recipes.
pub const RECIPE_OUT_DIR_KEY: &str = "BHARATCODE_RECIPE_OUT_DIR";
/// Toggle: enable automation mode by default.
pub const AUTOMATION_KEY: &str = "BHARATCODE_AUTOMATION";

/// Default output directory for shared recipes when no override is set: the
/// current working directory. Preserving the existing behaviour.
const RECIPE_OUT_DIR_DEFAULT: &str = ".";

/// Where a resolved setting value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Env,
    Config,
    Default,
}

impl Source {
    fn label(self) -> &'static str {
        match self {
            Source::Env => "env",
            Source::Config => "config",
            Source::Default => "default",
        }
    }
}

/// Interpret a raw config/env string as a boolean toggle. Anything not
/// recognised as "on" (and a blank value) reads as off, so a stray value never
/// silently enables a setting. Mirrors `agent_caps::parse_toggle`.
fn parse_toggle(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "on" | "yes" | "enabled"
    )
}

/// Read a single key as a trimmed string together with where it came from. The
/// raw environment variable is consulted first (so a bare `1`/`true` survives as
/// a string rather than being coerced to a number by the config parser), then
/// the merged config file via the standard accessor path. Returns `None` when
/// the key is absent or resolves to an empty string. Mirrors the env-first
/// pattern in `agent_caps::read_key`.
fn read_key(config: &Config, key: &str) -> Option<(String, Source)> {
    if let Ok(raw) = std::env::var(key) {
        let trimmed = raw.trim().to_string();
        return if trimmed.is_empty() {
            None
        } else {
            Some((trimmed, Source::Env))
        };
    }
    config
        .get_param::<String>(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .map(|v| (v, Source::Config))
}

/// Render one `key = value (source: ...)` row for a boolean toggle that defaults
/// to off, preserving current behaviour.
fn toggle_line(config: &Config, key: &str) -> String {
    match read_key(config, key) {
        Some((raw, source)) => {
            format!(
                "{key} = {} (source: {})",
                parse_toggle(&raw),
                source.label()
            )
        }
        None => format!("{key} = false (source: {})", Source::Default.label()),
    }
}

/// Render the `key = value (source: ...)` row for the recipe output directory,
/// which defaults to the current directory (`.`).
fn recipe_out_dir_line(config: &Config) -> String {
    match read_key(config, RECIPE_OUT_DIR_KEY) {
        Some((dir, source)) => format!("{RECIPE_OUT_DIR_KEY} = {dir} (source: {})", source.label()),
        None => format!(
            "{RECIPE_OUT_DIR_KEY} = {RECIPE_OUT_DIR_DEFAULT} (source: {})",
            Source::Default.label()
        ),
    }
}

/// One `key = value (source: env|config|default)` row per ecosystem setting, in
/// a stable order, for display in doctor/info. This is the real call site
/// reached from `Config`'s public API (`Config::extension_settings_summary`).
/// Pure read: never mutates config.
pub fn summary_lines_for_config(config: &Config) -> Vec<String> {
    vec![
        toggle_line(config, PLUGIN_AUTO_UPDATE_KEY),
        toggle_line(config, MCP_REGISTRY_BANNER_KEY),
        recipe_out_dir_line(config),
        toggle_line(config, AUTOMATION_KEY),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_KEYS: &[&str] = &[
        PLUGIN_AUTO_UPDATE_KEY,
        MCP_REGISTRY_BANNER_KEY,
        RECIPE_OUT_DIR_KEY,
        AUTOMATION_KEY,
    ];

    fn all_keys_unset() -> [(&'static str, Option<&'static str>); 4] {
        [
            (PLUGIN_AUTO_UPDATE_KEY, None),
            (MCP_REGISTRY_BANNER_KEY, None),
            (RECIPE_OUT_DIR_KEY, None),
            (AUTOMATION_KEY, None),
        ]
    }

    #[test]
    fn defaults_list_every_key_with_current_behaviour() {
        let _guard = env_lock::lock_env(all_keys_unset());
        let lines = summary_lines_for_config(Config::global());

        assert_eq!(lines.len(), ALL_KEYS.len());
        for key in ALL_KEYS {
            assert!(
                lines.iter().any(|line| line.starts_with(key)),
                "summary missing key {key}"
            );
        }
        // Every line resolves to its default source.
        assert!(
            lines.iter().all(|line| line.ends_with("(source: default)")),
            "expected all-default sources, got: {lines:?}"
        );
        // Current-behaviour values: toggles off, recipe dir = cwd.
        assert!(lines
            .iter()
            .any(|line| line == &format!("{PLUGIN_AUTO_UPDATE_KEY} = false (source: default)")));
        assert!(lines
            .iter()
            .any(|line| line == &format!("{MCP_REGISTRY_BANNER_KEY} = false (source: default)")));
        assert!(lines
            .iter()
            .any(|line| line == &format!("{RECIPE_OUT_DIR_KEY} = . (source: default)")));
        assert!(lines
            .iter()
            .any(|line| line == &format!("{AUTOMATION_KEY} = false (source: default)")));
    }

    #[test]
    fn automation_env_flips_line_to_env_true() {
        let mut keys = all_keys_unset();
        keys[3] = (AUTOMATION_KEY, Some("1"));
        let _guard = env_lock::lock_env(keys);
        let lines = summary_lines_for_config(Config::global());

        assert!(
            lines
                .iter()
                .any(|line| line == &format!("{AUTOMATION_KEY} = true (source: env)")),
            "automation line not flipped: {lines:?}"
        );
        // The other settings remain at their defaults.
        assert!(lines
            .iter()
            .any(|line| line == &format!("{PLUGIN_AUTO_UPDATE_KEY} = false (source: default)")));
    }

    #[test]
    fn recipe_out_dir_env_override_is_reported() {
        let mut keys = all_keys_unset();
        keys[2] = (RECIPE_OUT_DIR_KEY, Some("/tmp/recipes"));
        let _guard = env_lock::lock_env(keys);
        let lines = summary_lines_for_config(Config::global());

        assert!(lines
            .iter()
            .any(|line| line == &format!("{RECIPE_OUT_DIR_KEY} = /tmp/recipes (source: env)")));
    }

    #[test]
    fn unrecognised_toggle_value_reads_as_false() {
        let mut keys = all_keys_unset();
        keys[0] = (PLUGIN_AUTO_UPDATE_KEY, Some("maybe"));
        let _guard = env_lock::lock_env(keys);
        let lines = summary_lines_for_config(Config::global());

        assert!(lines
            .iter()
            .any(|line| line == &format!("{PLUGIN_AUTO_UPDATE_KEY} = false (source: env)")));
    }

    #[test]
    fn no_upstream_brand_leak_in_any_line() {
        let _guard = env_lock::lock_env(all_keys_unset());
        let lines = summary_lines_for_config(Config::global());
        for line in &lines {
            let lower = line.to_ascii_lowercase();
            assert!(
                !lower.contains("goose"),
                "unexpected brand leak in summary line: {line}"
            );
            assert!(
                !lower.contains("block"),
                "unexpected brand leak in summary line: {line}"
            );
        }
    }
}
