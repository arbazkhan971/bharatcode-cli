//! GA release-profile footer for `bharatcode cost` (BharatCode v97).
//!
//! Renders ONE read-only, themed footer line carrying the crate version (the
//! compile-time `CARGO_PKG_VERSION`), the build profile (debug/release, derived
//! from `cfg!(debug_assertions)` so it reflects the actual running binary), and
//! a fixed "GA" release-channel marker:
//!
//! ```text
//! Release profile: v<ver> · <profile> build · GA channel
//! ```
//!
//! Opt-in behind `BHARATCODE_COST_RELEASE` (default OFF). With the env var unset
//! or falsey, [`release_footer`] returns `None`, so the `cost` output is
//! byte-identical to before.

/// Opt-in env var gating the release-profile footer.
pub const COST_RELEASE_ENABLED_KEY: &str = "BHARATCODE_COST_RELEASE";

/// Whether the footer is enabled. Reads the raw env string so a bare `1` is
/// honoured (mirrors [`super::cost_extensions::is_enabled`]).
pub fn is_enabled() -> bool {
    match std::env::var(COST_RELEASE_ENABLED_KEY) {
        Ok(raw) => is_truthy(&raw),
        Err(_) => false,
    }
}

fn is_truthy(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Look up a user-facing label through the i18n layer, falling back to the
/// English `default` when the active locale table has no entry for `key`.
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// The build profile of the running binary.
fn profile() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

/// Pure renderer for the footer line. No I/O, no env, no styling.
pub fn render_release_footer(version: &str, profile: &str) -> String {
    let prefix = label("cost.release.label", "Release profile");
    let channel = label("cost.release.channel", "GA channel");
    format!("{prefix}: v{version} \u{00b7} {profile} build \u{00b7} {channel}")
}

/// The wired entry point: returns `Some(line)` only when the footer is enabled.
pub fn release_footer() -> Option<String> {
    if !is_enabled() {
        return None;
    }
    Some(render_release_footer(env!("CARGO_PKG_VERSION"), profile()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truthy_spellings_enable() {
        for v in ["1", "true", "yes", "on", " ON ", "True"] {
            assert!(is_truthy(v), "{v} should be truthy");
        }
        for v in ["0", "false", "no", "off", ""] {
            assert!(!is_truthy(v), "{v} should be falsey");
        }
    }

    #[test]
    fn disabled_by_default() {
        let _guard = env_lock::lock_env([(COST_RELEASE_ENABLED_KEY, None::<&str>)]);
        assert!(!is_enabled());
        assert!(release_footer().is_none());
    }

    #[test]
    fn enabled_yields_some() {
        let _guard = env_lock::lock_env([(COST_RELEASE_ENABLED_KEY, Some("1"))]);
        assert!(is_enabled());
        assert!(release_footer().is_some());
    }

    #[test]
    fn footer_is_single_line_with_version_profile_channel() {
        let line = render_release_footer("9.9.9", "release");
        assert_eq!(line.lines().count(), 1);
        assert!(line.contains("v9.9.9"), "version: {line}");
        assert!(line.contains("release build"), "profile: {line}");
        assert!(line.contains("GA"), "channel: {line}");
    }

    #[test]
    fn both_profiles_render() {
        for p in ["debug", "release"] {
            let line = render_release_footer("1.0.0", p);
            assert!(line.contains(&format!("{p} build")), "{line}");
        }
    }

    #[test]
    fn no_upstream_branding() {
        let line = render_release_footer("1.2.3", "release").to_lowercase();
        assert!(!line.contains("goose"), "brand leak: {line}");
        assert!(!line.contains("block"), "brand leak: {line}");
    }
}
