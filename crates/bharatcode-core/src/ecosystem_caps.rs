//! Ecosystem config surface: typed extensibility toggles surfaced through the
//! standard config accessor path.
//!
//! This wave's ecosystem-extensibility settings (plugin summary surface, git
//! context capture, external-extension digest, MCP-registry pin, automation
//! default-script path) are each driven by a `BHARATCODE_*` environment variable
//! / config key and default to empty/off so behaviour is unchanged when nothing
//! is set. This module gives doctor/privacy a single place to *read* those
//! settings through the typed `get_bharatcode_*` getters registered in
//! `config/base.rs` (which already layer env over the config file), rather than
//! scattering raw `std::env::var` lookups across the codebase.
//!
//! The resolution mirrors the proven `agent_caps` / `resource_limits` readers:
//! every value is read via `read_key`, which checks the uppercase environment
//! variable first (so a bare `1`/`true` survives as a string rather than being
//! coerced to a number by the config parser - the v50/v66 fix) and then the
//! merged config file. A missing key resolves to the default (off / empty).

use crate::config::Config;

/// Toggle: surface the installed-plugin summary in doctor/info output.
pub const PLUGIN_SUMMARY_KEY: &str = "BHARATCODE_PLUGIN_SUMMARY";
/// Toggle: capture git context (branch / dirty state) for the session.
pub const GIT_CONTEXT_KEY: &str = "BHARATCODE_GIT_CONTEXT";
/// Toggle: compute a digest of external extensions for change detection.
pub const EXT_DIGEST_KEY: &str = "BHARATCODE_EXT_DIGEST";
/// Pin: MCP-registry revision / URL to resolve extensions against (empty = unset).
pub const MCP_REGISTRY_PIN_KEY: &str = "BHARATCODE_MCP_REGISTRY_PIN";
/// Path: default automation script to load (empty = unset).
pub const AUTOMATION_SCRIPT_KEY: &str = "BHARATCODE_AUTOMATION_SCRIPT";

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
/// consulted first (so a bare `1`/`true` or a path survives verbatim as a string
/// rather than being coerced by the config parser), then the merged config file
/// via the standard accessor path. Returns `None` when the key is absent or
/// resolves to an empty string. Mirrors the env-first pattern in
/// `agent_caps::read_key` / `resource_limits::read_key` (the v50/v66 fix).
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

/// Human-readable `label: value` rows for the ecosystem config surface,
/// resolved env-first from a specific `Config`. Only settings that are actually
/// set produce a row; when nothing is set a single "all ecosystem features
/// default-off" line is returned so the summary is never empty-but-silent. This
/// is the real call site reached from `Config::ecosystem_caps_summary`, giving
/// doctor/privacy one source of truth for these toggles.
pub fn summary_lines_for_config(config: &Config) -> Vec<String> {
    let mut lines = Vec::new();

    if read_key(config, PLUGIN_SUMMARY_KEY).is_some_and(|v| parse_toggle(&v)) {
        lines.push("plugin summary: on".to_string());
    }
    if read_key(config, GIT_CONTEXT_KEY).is_some_and(|v| parse_toggle(&v)) {
        lines.push("git context: on".to_string());
    }
    if read_key(config, EXT_DIGEST_KEY).is_some_and(|v| parse_toggle(&v)) {
        lines.push("ext digest: on".to_string());
    }
    if let Some(pin) = read_key(config, MCP_REGISTRY_PIN_KEY) {
        lines.push(format!("mcp registry pin: {pin}"));
    }
    if let Some(path) = read_key(config, AUTOMATION_SCRIPT_KEY) {
        lines.push(format!("automation script: {path}"));
    }

    if lines.is_empty() {
        lines.push("all ecosystem features default-off".to_string());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All five keys cleared, so `summary_lines_for_config(Config::global())`
    /// resolves purely from defaults.
    fn all_keys_unset() -> [(&'static str, Option<&'static str>); 5] {
        [
            (PLUGIN_SUMMARY_KEY, None),
            (GIT_CONTEXT_KEY, None),
            (EXT_DIGEST_KEY, None),
            (MCP_REGISTRY_PIN_KEY, None),
            (AUTOMATION_SCRIPT_KEY, None),
        ]
    }

    fn assert_no_vendor_leak(lines: &[String]) {
        for line in lines {
            let lower = line.to_ascii_lowercase();
            assert!(!lower.contains("goose"), "row leaks vendor token: {line}");
            assert!(!line.contains("Block"), "row leaks vendor token: {line}");
        }
    }

    #[test]
    fn empty_config_reports_all_off_line() {
        let _guard = env_lock::lock_env(all_keys_unset());
        let lines = summary_lines_for_config(Config::global());
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "all ecosystem features default-off");
        assert_no_vendor_leak(&lines);
    }

    #[test]
    fn git_context_bare_one_survives_and_reads_on() {
        let mut keys = all_keys_unset();
        // A bare `1` must survive env coercion as a string and read as on.
        keys[1] = (GIT_CONTEXT_KEY, Some("1"));
        let _guard = env_lock::lock_env(keys);

        let lines = summary_lines_for_config(Config::global());
        assert!(
            lines.iter().any(|l| l == "git context: on"),
            "git context row should read on: {lines:?}"
        );
        // The all-off line must not appear once a setting is active.
        assert!(!lines.iter().any(|l| l.contains("default-off")));
        assert_no_vendor_leak(&lines);
    }

    #[test]
    fn automation_script_path_appears_verbatim() {
        let mut keys = all_keys_unset();
        keys[4] = (AUTOMATION_SCRIPT_KEY, Some("/tmp/x.jsonl"));
        let _guard = env_lock::lock_env(keys);

        let lines = summary_lines_for_config(Config::global());
        assert!(
            lines.iter().any(|l| l == "automation script: /tmp/x.jsonl"),
            "automation script path should appear verbatim: {lines:?}"
        );
        assert_no_vendor_leak(&lines);
    }

    #[test]
    fn mcp_registry_pin_appears_verbatim() {
        let mut keys = all_keys_unset();
        keys[3] = (MCP_REGISTRY_PIN_KEY, Some("rev-2026-06"));
        let _guard = env_lock::lock_env(keys);

        let lines = summary_lines_for_config(Config::global());
        assert!(
            lines.iter().any(|l| l == "mcp registry pin: rev-2026-06"),
            "mcp registry pin should appear verbatim: {lines:?}"
        );
        assert_no_vendor_leak(&lines);
    }

    #[test]
    fn unrecognised_toggle_value_reads_as_off() {
        let mut keys = all_keys_unset();
        keys[0] = (PLUGIN_SUMMARY_KEY, Some("maybe"));
        let _guard = env_lock::lock_env(keys);
        let lines = summary_lines_for_config(Config::global());
        assert_eq!(
            lines,
            vec!["all ecosystem features default-off".to_string()]
        );
    }

    #[test]
    fn multiple_settings_each_get_a_row() {
        let keys = [
            (PLUGIN_SUMMARY_KEY, Some("on")),
            (GIT_CONTEXT_KEY, Some("true")),
            (EXT_DIGEST_KEY, None),
            (MCP_REGISTRY_PIN_KEY, None),
            (AUTOMATION_SCRIPT_KEY, Some("/tmp/run.jsonl")),
        ];
        let _guard = env_lock::lock_env(keys);
        let lines = summary_lines_for_config(Config::global());
        assert!(lines.iter().any(|l| l == "plugin summary: on"));
        assert!(lines.iter().any(|l| l == "git context: on"));
        assert!(lines
            .iter()
            .any(|l| l == "automation script: /tmp/run.jsonl"));
        assert!(!lines.iter().any(|l| l.contains("default-off")));
        assert_no_vendor_leak(&lines);
    }
}
