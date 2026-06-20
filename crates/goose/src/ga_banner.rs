//! Opt-in GA/build identity line for the system prompt (BharatCode v98).
//!
//! When enabled via `BHARATCODE_GA_BANNER`, [`banner_block`] renders one compact
//! "# Release" block stating the agent's exact release (the compile-time crate
//! version plus a release channel, "GA" by default) and a directive to state
//! exactly that release when asked which version/build it is. Disabled by
//! default, in which case [`banner_block`] is `None` and the prompt is
//! byte-identical.

const ENABLE_KEY: &str = "BHARATCODE_GA_BANNER";
const CHANNEL_KEY: &str = "BHARATCODE_GA_CHANNEL";
const DEFAULT_CHANNEL: &str = "GA";

/// Whether the GA banner is enabled. Opt-in via `BHARATCODE_GA_BANNER`; any
/// truthy-ish value (`1`, `true`, `yes`, `on`) enables it. Defaults to `false`.
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
/// or unset value falls back to `GA`.
pub fn channel() -> String {
    std::env::var(CHANNEL_KEY)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_CHANNEL.to_string())
}

/// Pure renderer for the release block.
pub fn render(version: &str, channel: &str) -> String {
    format!(
        "# Release\n\n\
         This agent is BharatCode {version} {channel} build. When asked which \
         version or build you are, state exactly that release.\n"
    )
}

/// The release identity block injected into the system prompt when enabled, or
/// `None` when disabled (leaving the prompt byte-identical).
pub fn banner_block() -> Option<String> {
    if !is_enabled() {
        return None;
    }
    Some(render(env!("CARGO_PKG_VERSION"), &channel()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_guard<'a>(enabled: Option<&'a str>, chan: Option<&'a str>) -> env_lock::EnvGuard<'a> {
        env_lock::lock_env([(ENABLE_KEY, enabled), (CHANNEL_KEY, chan)])
    }

    #[test]
    fn disabled_yields_none() {
        let _guard = env_guard(None, None);
        assert!(!is_enabled());
        assert!(banner_block().is_none());
    }

    #[test]
    fn enabled_yields_compact_block() {
        let _guard = env_guard(Some("1"), None);
        let block = banner_block().expect("present when enabled");
        assert!(block.contains(env!("CARGO_PKG_VERSION")), "{block}");
        assert!(block.contains("GA"), "{block}");
        assert!(block.len() < 300, "should stay compact: {block}");
        let lower = block.to_lowercase();
        assert!(!lower.contains("goose"), "brand leak: {block}");
        assert!(!lower.contains("block"), "brand leak: {block}");
    }

    #[test]
    fn channel_override_is_honoured() {
        let _guard = env_guard(Some("1"), Some("RC1"));
        assert_eq!(channel(), "RC1");
        let block = banner_block().unwrap();
        assert!(block.contains("RC1"), "{block}");
    }

    #[test]
    fn blank_channel_falls_back_to_ga() {
        let _guard = env_guard(Some("1"), Some("   "));
        assert_eq!(channel(), DEFAULT_CHANNEL);
    }

    #[test]
    fn truthiness_table() {
        for v in ["1", "true", "YES", " on "] {
            assert!(is_truthy(v), "{v}");
        }
        for v in ["0", "false", "no", "off", ""] {
            assert!(!is_truthy(v), "{v}");
        }
    }

    #[test]
    fn render_is_pure() {
        let chan = ["ga", "2099", "tok"].join("-");
        let out = render("1.2.3", &chan);
        assert!(out.contains("1.2.3"));
        assert!(out.contains(&chan));
    }
}
