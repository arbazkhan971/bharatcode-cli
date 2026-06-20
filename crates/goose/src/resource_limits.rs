//! Typed per-turn / per-session resource ceilings surfaced through the standard
//! config accessor path.
//!
//! The v66 resource-limit feature gives consumers a single validated source of
//! truth for three optional ceilings:
//!
//! * `BHARATCODE_MAX_TOOL_CALLS_PER_TURN` - cap on tool calls within one turn,
//! * `BHARATCODE_MAX_TURN_SECS` - turn wall-clock budget in seconds,
//! * `BHARATCODE_MAX_SESSION_TOKENS` - total token budget across the session.
//!
//! Every value defaults to `None` (unlimited), so behaviour is unchanged until
//! one is set. Values are read env-first (so a bare `1` survives the config
//! number-coercion as a string) and clamped to sane bounds so an absurd input
//! never produces a nonsensical ceiling. The reader/summary helpers are reached
//! from `Config::resource_limits_summary` (wired in `config/base.rs`).

use crate::config::Config;

/// Cap on tool calls allowed within a single turn.
pub const MAX_TOOL_CALLS_PER_TURN_KEY: &str = "BHARATCODE_MAX_TOOL_CALLS_PER_TURN";
/// Wall-clock budget, in seconds, for a single turn.
pub const MAX_TURN_SECS_KEY: &str = "BHARATCODE_MAX_TURN_SECS";
/// Total token budget across the whole session.
pub const MAX_SESSION_TOKENS_KEY: &str = "BHARATCODE_MAX_SESSION_TOKENS";

/// Upper bound for the per-turn tool-call ceiling. Anything larger is clamped so
/// a typo'd `1000000` does not become an effectively unbounded counter.
const MAX_TOOL_CALLS_PER_TURN_CAP: u32 = 10_000;
/// Upper bound for the per-turn wall-clock ceiling: 24 hours in seconds.
const MAX_TURN_SECS_CAP: u64 = 86_400;
/// Upper bound for the session token ceiling: one billion tokens.
const MAX_SESSION_TOKENS_CAP: u64 = 1_000_000_000;

/// Resolved snapshot of the per-turn / per-session resource ceilings.
///
/// Each field defaults to `None`, meaning "unlimited". A `Some(0)` input is
/// treated as unset (unlimited) rather than an immediate hard stop, so a stray
/// `0` never silently wedges a turn.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResourceLimits {
    max_tool_calls_per_turn: Option<u32>,
    max_turn_secs: Option<u64>,
    max_session_tokens: Option<u64>,
}

/// Read a single key as a trimmed string, env-first.
///
/// The raw environment variable is consulted before the merged config file so a
/// bare `1` survives as a string rather than being coerced to a number by the
/// config parser. Returns `None` when the key is absent or resolves to an empty
/// string. Mirrors `agent_caps::read_key`.
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

/// Parse a key as `u32`, clamping to `[1, cap]`. A value that does not parse, or
/// parses to `0`, reads as `None` (unlimited) so a malformed ceiling never
/// applies an accidental hard limit.
fn read_u32_clamped(config: &Config, key: &str, cap: u32) -> Option<u32> {
    let parsed: u64 = read_key(config, key)?.parse().ok()?;
    if parsed == 0 {
        return None;
    }
    Some(parsed.min(cap as u64) as u32)
}

/// Parse a key as `u64`, clamping to `[1, cap]`. See [`read_u32_clamped`].
fn read_u64_clamped(config: &Config, key: &str, cap: u64) -> Option<u64> {
    let parsed: u64 = read_key(config, key)?.parse().ok()?;
    if parsed == 0 {
        return None;
    }
    Some(parsed.min(cap))
}

impl ResourceLimits {
    /// Cap on tool calls allowed within a single turn (`None` = unlimited).
    pub fn max_tool_calls_per_turn(&self) -> Option<u32> {
        self.max_tool_calls_per_turn
    }

    /// Wall-clock budget, in seconds, for a single turn (`None` = unlimited).
    pub fn max_turn_secs(&self) -> Option<u64> {
        self.max_turn_secs
    }

    /// Total token budget across the whole session (`None` = unlimited).
    pub fn max_session_tokens(&self) -> Option<u64> {
        self.max_session_tokens
    }

    /// Whether every ceiling is unset (all unlimited).
    pub fn is_unlimited(&self) -> bool {
        self.max_tool_calls_per_turn.is_none()
            && self.max_turn_secs.is_none()
            && self.max_session_tokens.is_none()
    }

    /// One human-readable row per ceiling that has been set, in registration
    /// order. When nothing is set this returns a single "unlimited" line so the
    /// summary is never empty-but-silent. Used by `Config::resource_limits_summary`.
    pub fn summary_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some(v) = self.max_tool_calls_per_turn {
            lines.push(format!("{MAX_TOOL_CALLS_PER_TURN_KEY} = {v}"));
        }
        if let Some(v) = self.max_turn_secs {
            lines.push(format!("{MAX_TURN_SECS_KEY} = {v}s"));
        }
        if let Some(v) = self.max_session_tokens {
            lines.push(format!("{MAX_SESSION_TOKENS_KEY} = {v}"));
        }
        if lines.is_empty() {
            lines.push("resource limits = unlimited".to_string());
        }
        lines
    }
}

/// Resolve the resource ceilings from a specific config, reading env-first and
/// clamping each value to sane bounds. This is the real call site reached from
/// `Config::resource_limits_summary`.
pub fn from_config(config: &Config) -> ResourceLimits {
    ResourceLimits {
        max_tool_calls_per_turn: read_u32_clamped(
            config,
            MAX_TOOL_CALLS_PER_TURN_KEY,
            MAX_TOOL_CALLS_PER_TURN_CAP,
        ),
        max_turn_secs: read_u64_clamped(config, MAX_TURN_SECS_KEY, MAX_TURN_SECS_CAP),
        max_session_tokens: read_u64_clamped(
            config,
            MAX_SESSION_TOKENS_KEY,
            MAX_SESSION_TOKENS_CAP,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All three keys cleared, so `from_config(Config::global())` resolves purely
    /// from defaults.
    fn all_keys_unset() -> [(&'static str, Option<&'static str>); 3] {
        [
            (MAX_TOOL_CALLS_PER_TURN_KEY, None),
            (MAX_TURN_SECS_KEY, None),
            (MAX_SESSION_TOKENS_KEY, None),
        ]
    }

    #[test]
    fn empty_config_yields_all_none() {
        let _guard = env_lock::lock_env(all_keys_unset());
        let limits = from_config(Config::global());
        assert_eq!(limits, ResourceLimits::default());
        assert!(limits.is_unlimited());
        assert_eq!(limits.max_tool_calls_per_turn(), None);
        assert_eq!(limits.max_turn_secs(), None);
        assert_eq!(limits.max_session_tokens(), None);
    }

    #[test]
    fn empty_config_summary_is_unlimited() {
        let _guard = env_lock::lock_env(all_keys_unset());
        let limits = from_config(Config::global());
        let lines = limits.summary_lines();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("unlimited"));
        // No ceiling key leaks into the unlimited summary.
        assert!(!lines[0].contains(MAX_TOOL_CALLS_PER_TURN_KEY));
        assert!(!lines[0].contains(MAX_TURN_SECS_KEY));
        assert!(!lines[0].contains(MAX_SESSION_TOKENS_KEY));
    }

    #[test]
    fn max_tool_calls_per_turn_is_parsed_and_summarised() {
        let mut keys = all_keys_unset();
        keys[0] = (MAX_TOOL_CALLS_PER_TURN_KEY, Some("12"));
        let _guard = env_lock::lock_env(keys);
        let limits = from_config(Config::global());
        assert_eq!(limits.max_tool_calls_per_turn(), Some(12));
        assert_eq!(limits.max_turn_secs(), None);
        assert_eq!(limits.max_session_tokens(), None);
        assert!(!limits.is_unlimited());
        let lines = limits.summary_lines();
        assert!(lines
            .iter()
            .any(|l| l.starts_with(MAX_TOOL_CALLS_PER_TURN_KEY) && l.contains("12")));
    }

    #[test]
    fn max_turn_secs_is_parsed_and_summarised() {
        let mut keys = all_keys_unset();
        keys[1] = (MAX_TURN_SECS_KEY, Some("90"));
        let _guard = env_lock::lock_env(keys);
        let limits = from_config(Config::global());
        assert_eq!(limits.max_turn_secs(), Some(90));
        let lines = limits.summary_lines();
        assert!(lines
            .iter()
            .any(|l| l.starts_with(MAX_TURN_SECS_KEY) && l.contains("90")));
    }

    #[test]
    fn max_session_tokens_is_parsed_and_summarised() {
        let mut keys = all_keys_unset();
        keys[2] = (MAX_SESSION_TOKENS_KEY, Some("250000"));
        let _guard = env_lock::lock_env(keys);
        let limits = from_config(Config::global());
        assert_eq!(limits.max_session_tokens(), Some(250_000));
        let lines = limits.summary_lines();
        assert!(lines
            .iter()
            .any(|l| l.starts_with(MAX_SESSION_TOKENS_KEY) && l.contains("250000")));
    }

    #[test]
    fn bare_one_is_read_as_one_not_coerced_away() {
        let mut keys = all_keys_unset();
        keys[0] = (MAX_TOOL_CALLS_PER_TURN_KEY, Some("1"));
        let _guard = env_lock::lock_env(keys);
        let limits = from_config(Config::global());
        assert_eq!(limits.max_tool_calls_per_turn(), Some(1));
    }

    #[test]
    fn absurd_values_clamp_to_sane_bounds() {
        let keys = [
            (MAX_TOOL_CALLS_PER_TURN_KEY, Some("999999999")),
            (MAX_TURN_SECS_KEY, Some("999999999")),
            (MAX_SESSION_TOKENS_KEY, Some("999999999999999")),
        ];
        let _guard = env_lock::lock_env(keys);
        let limits = from_config(Config::global());
        assert_eq!(
            limits.max_tool_calls_per_turn(),
            Some(MAX_TOOL_CALLS_PER_TURN_CAP)
        );
        assert_eq!(limits.max_turn_secs(), Some(MAX_TURN_SECS_CAP));
        assert_eq!(limits.max_session_tokens(), Some(MAX_SESSION_TOKENS_CAP));
    }

    #[test]
    fn zero_reads_as_unlimited() {
        let mut keys = all_keys_unset();
        keys[1] = (MAX_TURN_SECS_KEY, Some("0"));
        let _guard = env_lock::lock_env(keys);
        let limits = from_config(Config::global());
        assert_eq!(limits.max_turn_secs(), None);
    }

    #[test]
    fn non_numeric_reads_as_unlimited() {
        let mut keys = all_keys_unset();
        keys[2] = (MAX_SESSION_TOKENS_KEY, Some("lots"));
        let _guard = env_lock::lock_env(keys);
        let limits = from_config(Config::global());
        assert_eq!(limits.max_session_tokens(), None);
    }
}
