//! Opt-in GA release-identity + readiness banner for the system prompt
//! (BharatCode v100).
//!
//! When enabled via `BHARATCODE_GA_IDENTITY`, [`release_context_block`] renders
//! one compact "# Release context" block stating the shipped product identity
//! (BharatCode 1.0 GA), the release channel, an Apache-2.0 + local-first posture
//! line, and a current date-stamp, plus a one-line readiness summary so the
//! agent self-identifies with the GA 1.0 release. Disabled by default, in which
//! case [`release_context_block`] is `None` and the prompt is byte-identical
//! (no extra tokens).
//!
//! This module is original work; nothing here is ported from third-party
//! sources. It compiles standalone: it carries its own [`GA_VERSION`] fallback
//! and does not depend on any sibling release module.

/// Opt-in toggle name, read raw from the process environment. Default OFF.
const ENABLE_KEY: &str = "BHARATCODE_GA_IDENTITY";

/// Optional release-channel override. A blank/unset value falls back to
/// [`DEFAULT_CHANNEL`].
const CHANNEL_KEY: &str = "BHARATCODE_GA_CHANNEL";

/// Default release channel label for the GA wave.
const DEFAULT_CHANNEL: &str = "stable";

/// Shipped GA marketing version. Kept as a standalone const so this module
/// compiles without any sibling release module; if a `release::GA_VERSION`
/// lands upstream it is expected to carry the same `1.0` GA value.
const GA_VERSION: &str = "1.0";

/// Whether the GA identity block is enabled. Opt-in via `BHARATCODE_GA_IDENTITY`;
/// any truthy value (`1`, `true`, `yes`, `on`) enables it. Defaults to `false`,
/// so an unset or falsey (`0`/`off`/`false`/`no`) value leaves it off.
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

/// The release channel label. Overridable via `BHARATCODE_GA_CHANNEL`; a blank
/// or unset value falls back to [`DEFAULT_CHANNEL`].
fn channel() -> String {
    std::env::var(CHANNEL_KEY)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_CHANNEL.to_string())
}

/// Pure renderer for the GA release-context block. `date` is the current
/// date-stamp (`YYYY-MM-DD`). Caps the block at ~400 bytes.
fn render(version: &str, channel: &str, date: &str) -> String {
    format!(
        "# Release context\n\n\
         You are BharatCode {version} (GA, {channel} channel; released, production-ready). \
         Apache-2.0 licensed, local-first. Current date: {date}. \
         When asked which version you are, state this GA {version} release.\n"
    )
}

/// The GA release-identity + readiness block injected into the system prompt
/// when enabled, or `None` when disabled (leaving the prompt byte-identical and
/// adding no extra tokens). The block names `BharatCode`, `1.0`, and
/// `Apache-2.0`, and stays under a 400-byte cap.
pub fn release_context_block() -> Option<String> {
    if !is_enabled() {
        return None;
    }
    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    Some(render(GA_VERSION, &channel(), &date))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialise tests that mutate the shared process env so the toggle does not
    /// race across threads.
    fn env_guard<'a>(enabled: Option<&'a str>, chan: Option<&'a str>) -> env_lock::EnvGuard<'a> {
        env_lock::lock_env([(ENABLE_KEY, enabled), (CHANNEL_KEY, chan)])
    }

    #[test]
    fn disabled_when_unset_yields_none() {
        let _guard = env_guard(None, None);
        assert!(!is_enabled());
        assert!(release_context_block().is_none());
    }

    #[test]
    fn falsey_zero_stays_off() {
        for off in ["0", "off", "false", "no", "  "] {
            let _guard = env_guard(Some(off), None);
            assert!(!is_enabled(), "expected {off:?} to stay off");
            assert!(release_context_block().is_none(), "expected None for {off:?}");
        }
    }

    #[test]
    fn enabled_yields_ga_identity_block() {
        let _guard = env_guard(Some("1"), None);
        let block = release_context_block().expect("present when enabled");
        assert!(block.contains("BharatCode"), "{block}");
        assert!(block.contains("1.0"), "{block}");
        assert!(block.contains("Apache-2.0"), "{block}");
        assert!(block.contains("GA"), "{block}");
    }

    #[test]
    fn block_is_brand_leak_clean() {
        let _guard = env_guard(Some("true"), None);
        let block = release_context_block().unwrap();
        let lower = block.to_lowercase();
        // No upstream product name and no upstream company name leak.
        assert!(!lower.contains("goose"), "brand leak: {block}");
        assert!(!lower.contains("block"), "brand leak: {block}");
    }

    #[test]
    fn block_under_byte_cap() {
        let _guard = env_guard(Some("on"), None);
        let block = release_context_block().unwrap();
        assert!(block.len() <= 400, "expected <=400 bytes, got {}: {block}", block.len());
    }

    #[test]
    fn truthy_values_enable() {
        for on in ["1", "true", "YES", " on "] {
            let _guard = env_guard(Some(on), None);
            assert!(is_enabled(), "expected {on:?} to enable");
        }
    }

    #[test]
    fn channel_override_is_honoured() {
        let _guard = env_guard(Some("1"), Some("RC"));
        assert_eq!(channel(), "RC");
        let block = release_context_block().unwrap();
        assert!(block.contains("RC"), "{block}");
        assert!(block.len() <= 400, "{}", block.len());
    }

    #[test]
    fn blank_channel_falls_back_to_default() {
        let _guard = env_guard(Some("1"), Some("   "));
        assert_eq!(channel(), DEFAULT_CHANNEL);
    }

    #[test]
    fn render_is_pure() {
        let out = render("1.0", "stable", "2026-06-20");
        assert!(out.contains("BharatCode 1.0"));
        assert!(out.contains("stable"));
        assert!(out.contains("2026-06-20"));
        assert!(out.contains("Apache-2.0"));
        assert!(out.len() <= 400, "render cap: {}", out.len());
    }
}
