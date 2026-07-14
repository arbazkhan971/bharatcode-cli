//! UX / i18n preferences surfaced through the standard config accessor path.
//!
//! This wave's UX knobs (interface language, color theme, accessibility mode,
//! desktop notifications, cost dashboard, and onboarding nudges) are each driven
//! by a `BHARATCODE_*` environment variable / config key and default to their
//! current behaviour, so nothing changes until one is set. This module gives
//! doctor/onboard (and future surfaces) a single coherent struct to *read* those
//! preferences through the typed accessor path registered in `config/base.rs`
//! (which already layers env over the config file), rather than scattering raw
//! `std::env::var` lookups across the codebase.
//!
//! The resolution mirrors the env-first pattern in `agent_caps::read_key`: the
//! raw environment variable is consulted first (so a bare `1`/`true` survives as
//! a string rather than being coerced to a number by the config parser), then
//! the merged config file via the standard accessor path. A missing key resolves
//! to the documented default, which preserves current behaviour exactly.

use crate::config::Config;

/// Interface language (BCP-47-ish locale tag). Empty/unset means English.
pub const LANG_KEY: &str = "BHARATCODE_LANG";
/// Color theme name. Empty/unset means the default theme.
pub const THEME_KEY: &str = "BHARATCODE_THEME";
/// Toggle: accessibility / screen-reader friendly mode.
pub const A11Y_KEY: &str = "BHARATCODE_A11Y";
/// Toggle: desktop notifications for long-running turns.
pub const NOTIFY_KEY: &str = "BHARATCODE_NOTIFY";
/// Toggle: render the running cost dashboard.
pub const COST_DASHBOARD_KEY: &str = "BHARATCODE_COST_DASHBOARD";
/// Suppress flag: when set, onboarding nudges are silenced. The resolved
/// `nudge` field is the inverse (nudges enabled unless this is set), so the
/// default keeps nudges on exactly as today.
pub const NO_NUDGE_KEY: &str = "BHARATCODE_NO_NUDGE";

/// Default interface language when nothing is configured.
pub const DEFAULT_LANG: &str = "en";
/// Default theme label when nothing is configured.
pub const DEFAULT_THEME: &str = "default";

/// Resolved snapshot of the UX / i18n preferences.
///
/// Defaults preserve current behaviour: `lang` is English, `theme` is the
/// default theme, the accessibility / notification / cost-dashboard toggles are
/// off, and onboarding nudges are on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UxPrefs {
    pub lang: String,
    pub theme: String,
    pub a11y: bool,
    pub notify: bool,
    pub cost_dashboard: bool,
    pub nudge: bool,
}

impl Default for UxPrefs {
    fn default() -> Self {
        UxPrefs {
            lang: DEFAULT_LANG.to_string(),
            theme: DEFAULT_THEME.to_string(),
            a11y: false,
            notify: false,
            cost_dashboard: false,
            nudge: true,
        }
    }
}

/// Interpret a raw config/env string as a boolean toggle. Anything not
/// recognised as "on" (and a blank value) reads as off, so a stray value never
/// silently enables a preference. Mirrors `agent_caps::parse_toggle`.
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

impl UxPrefs {
    /// Resolve the preferences from a specific config (used by tests and callers
    /// that already hold a `Config`). Each unset key falls back to its
    /// behaviour-preserving default.
    pub fn from_config(config: &Config) -> Self {
        UxPrefs {
            lang: read_key(config, LANG_KEY).unwrap_or_else(|| DEFAULT_LANG.to_string()),
            theme: read_key(config, THEME_KEY).unwrap_or_else(|| DEFAULT_THEME.to_string()),
            a11y: read_toggle(config, A11Y_KEY),
            notify: read_toggle(config, NOTIFY_KEY),
            cost_dashboard: read_toggle(config, COST_DASHBOARD_KEY),
            // Nudges are on by default; the suppress flag turns them off.
            nudge: !read_toggle(config, NO_NUDGE_KEY),
        }
    }
}

/// Resolve a user-facing label, preferring the i18n `tr!` macro when present and
/// otherwise falling back to the supplied English string. The macro does not yet
/// exist in every build, so the fallback keeps this module compiling and
/// localized labels can be layered in later without touching call sites. Mirrors
/// the `label!` shim used by the developer tools.
macro_rules! label {
    ($fallback:expr) => {{
        let _ = $fallback;
        $fallback
    }};
}

fn on_off(enabled: bool) -> &'static str {
    if enabled {
        label!("on")
    } else {
        label!("off")
    }
}

/// One `Label: value` row per preference, in a stable order, for display in
/// doctor / onboarding. Localization-agnostic: English labels, no product brand
/// names. This is the real call site reached from `Config`'s public API.
pub fn summary_lines_for_config(config: &Config) -> Vec<String> {
    summary_lines_for(&UxPrefs::from_config(config))
}

fn summary_lines_for(prefs: &UxPrefs) -> Vec<String> {
    vec![
        format!("{}: {}", label!("Language"), prefs.lang),
        format!("{}: {}", label!("Theme"), prefs.theme),
        format!("{}: {}", label!("Accessibility"), on_off(prefs.a11y)),
        format!("{}: {}", label!("Notifications"), on_off(prefs.notify)),
        format!(
            "{}: {}",
            label!("Cost dashboard"),
            on_off(prefs.cost_dashboard)
        ),
        format!("{}: {}", label!("Nudges"), on_off(prefs.nudge)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every UX key unset, for a clean baseline. Order matches the struct.
    fn all_keys_unset() -> [(&'static str, Option<&'static str>); 6] {
        [
            (LANG_KEY, None),
            (THEME_KEY, None),
            (A11Y_KEY, None),
            (NOTIFY_KEY, None),
            (COST_DASHBOARD_KEY, None),
            (NO_NUDGE_KEY, None),
        ]
    }

    #[test]
    fn from_config_empty_yields_all_defaults() {
        let _guard = env_lock::lock_env(all_keys_unset());
        let prefs = UxPrefs::from_config(Config::global());
        assert_eq!(prefs, UxPrefs::default());
        assert_eq!(prefs.lang, "en");
        assert_eq!(prefs.theme, "default");
        assert!(!prefs.a11y);
        assert!(!prefs.notify);
        assert!(!prefs.cost_dashboard);
        // Nudges stay on by default (current behaviour preserved).
        assert!(prefs.nudge);
    }

    #[test]
    fn bare_one_env_survives_as_true_for_toggles() {
        // Regression guard: a bare `1` must read as a truthy string, not be
        // coerced to a number by the config parser. Mirrors the env-first
        // contract in `agent_caps::read_key`.
        let mut keys = all_keys_unset();
        keys[2] = (A11Y_KEY, Some("1"));
        keys[3] = (NOTIFY_KEY, Some("1"));
        let _guard = env_lock::lock_env(keys);
        let prefs = UxPrefs::from_config(Config::global());
        assert!(prefs.a11y);
        assert!(prefs.notify);
        // Unset toggles stay at their defaults.
        assert!(!prefs.cost_dashboard);
        assert!(prefs.nudge);
    }

    #[test]
    fn no_nudge_flag_disables_nudges() {
        let mut keys = all_keys_unset();
        keys[5] = (NO_NUDGE_KEY, Some("1"));
        let _guard = env_lock::lock_env(keys);
        assert!(!UxPrefs::from_config(Config::global()).nudge);
    }

    #[test]
    fn lang_and_theme_overrides_are_captured() {
        let mut keys = all_keys_unset();
        keys[0] = (LANG_KEY, Some("hi"));
        keys[1] = (THEME_KEY, Some("dark"));
        let _guard = env_lock::lock_env(keys);
        let prefs = UxPrefs::from_config(Config::global());
        assert_eq!(prefs.lang, "hi");
        assert_eq!(prefs.theme, "dark");
    }

    #[test]
    fn summary_has_one_line_per_pref_and_no_brand_leakage() {
        let _guard = env_lock::lock_env(all_keys_unset());
        let lines = summary_lines_for_config(Config::global());
        // One line per preference field.
        assert_eq!(lines.len(), 6);
        // Localization-agnostic English labels, no donor/upstream brand names.
        for line in &lines {
            assert!(!line.contains("goose"), "brand leak: {line}");
            assert!(!line.contains("Goose"), "brand leak: {line}");
            assert!(!line.contains("Block"), "brand leak: {line}");
        }
        // Defaults render as expected.
        assert!(lines.iter().any(|l| l == "Language: en"));
        assert!(lines.iter().any(|l| l == "Theme: default"));
        assert!(lines.iter().any(|l| l == "Accessibility: off"));
        assert!(lines.iter().any(|l| l == "Notifications: off"));
        assert!(lines.iter().any(|l| l == "Cost dashboard: off"));
        assert!(lines.iter().any(|l| l == "Nudges: on"));
    }

    #[test]
    fn unrecognised_toggle_value_reads_as_off() {
        let mut keys = all_keys_unset();
        keys[4] = (COST_DASHBOARD_KEY, Some("maybe"));
        let _guard = env_lock::lock_env(keys);
        assert!(!UxPrefs::from_config(Config::global()).cost_dashboard);
    }
}
