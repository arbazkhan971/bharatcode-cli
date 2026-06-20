//! Named perf-release profile selector: a single validated source of truth that
//! resolves one profile name into concrete advisory perf knobs.
//!
//! The v97 perf-release feature gives doctor/info a single typed way to pick a
//! *named* performance posture rather than tuning a handful of unrelated knobs by
//! hand. A profile is selected through one optional key:
//!
//! * `BHARATCODE_PERF_PROFILE` - one of `balanced`, `low-latency`, `throughput`,
//!   `battery`.
//!
//! The selector is purely *advisory*: it resolves a profile name into three
//! concrete knobs (a stream-flush cadence in ms, a max-concurrent-tools hint, and
//! a context-budget hint) that callers may read, but it changes no default on its
//! own. When the key is unset the profile resolves to `balanced`, whose knobs are
//! exactly the current defaults, so selecting nothing keeps behaviour identical.
//! An unrecognised name also falls back to `balanced` (never a panic), so a typo
//! degrades gracefully rather than breaking startup.
//!
//! Values are read env-first (so a bare profile name survives the config
//! number-coercion as a string). The reader/summary helpers are reached from
//! `Config::perf_release_summary` (wired in `config/base.rs`), mirroring the
//! shipped `resource_limits` / `streaming_perf` surfaces.
//!
//! Pure config logic: no I/O beyond the env/config reads the accessor path
//! already performs.

use crate::config::Config;

/// Name of the optional key that selects a named perf-release profile.
pub const PERF_PROFILE_KEY: &str = "BHARATCODE_PERF_PROFILE";

/// Stream-flush cadence (ms) the `balanced` profile advises. Mirrors the current
/// streaming default so the unset/`balanced` posture keeps behaviour unchanged.
const BALANCED_FLUSH_MS: u64 = 50;
/// Max-concurrent-tools hint the `balanced` profile advises.
const BALANCED_MAX_CONCURRENT_TOOLS: u32 = 4;
/// Context-budget hint (tokens) the `balanced` profile advises.
const BALANCED_CONTEXT_BUDGET_HINT: u64 = 128_000;

/// A named perf-release profile.
///
/// `Balanced` is the default posture and its knobs equal the current defaults,
/// so an unset (or unrecognised) selection keeps behaviour identical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProfileName {
    /// Current defaults; selecting nothing resolves here.
    #[default]
    Balanced,
    /// Tighter flush cadence for snappier streaming at some throughput cost.
    LowLatency,
    /// Higher concurrency / larger budgets for batch-style throughput.
    Throughput,
    /// Conservative cadence and budgets to reduce wakeups / work on battery.
    Battery,
}

impl ProfileName {
    /// Parse a profile name, tolerant of case, surrounding whitespace, and either
    /// `-` or `_` as the word separator. An unrecognised name resolves to
    /// [`ProfileName::Balanced`] rather than panicking, so a typo degrades
    /// gracefully to the default posture.
    pub fn parse(raw: &str) -> ProfileName {
        match raw.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "balanced" => ProfileName::Balanced,
            "low-latency" => ProfileName::LowLatency,
            "throughput" => ProfileName::Throughput,
            "battery" => ProfileName::Battery,
            _ => ProfileName::Balanced,
        }
    }

    /// The canonical lower-case profile name, suitable for summary rows.
    pub fn as_str(self) -> &'static str {
        match self {
            ProfileName::Balanced => "balanced",
            ProfileName::LowLatency => "low-latency",
            ProfileName::Throughput => "throughput",
            ProfileName::Battery => "battery",
        }
    }

    /// Resolve this profile into its concrete advisory knobs.
    pub fn knobs(self) -> ProfileKnobs {
        match self {
            ProfileName::Balanced => ProfileKnobs {
                profile: self,
                stream_flush_ms: BALANCED_FLUSH_MS,
                max_concurrent_tools: BALANCED_MAX_CONCURRENT_TOOLS,
                context_budget_hint: BALANCED_CONTEXT_BUDGET_HINT,
            },
            // Tighter flush for snappier streaming; concurrency/budget unchanged.
            ProfileName::LowLatency => ProfileKnobs {
                profile: self,
                stream_flush_ms: 16,
                max_concurrent_tools: BALANCED_MAX_CONCURRENT_TOOLS,
                context_budget_hint: BALANCED_CONTEXT_BUDGET_HINT,
            },
            // Raise concurrency and budget for batch throughput; relax cadence.
            ProfileName::Throughput => ProfileKnobs {
                profile: self,
                stream_flush_ms: 120,
                max_concurrent_tools: 16,
                context_budget_hint: 256_000,
            },
            // Slow cadence and trim budgets to reduce wakeups / work.
            ProfileName::Battery => ProfileKnobs {
                profile: self,
                stream_flush_ms: 250,
                max_concurrent_tools: 2,
                context_budget_hint: 64_000,
            },
        }
    }
}

/// Resolved advisory perf knobs for one named profile.
///
/// Every field is advisory: a caller may consult it, but the selector changes no
/// default on its own. `Balanced` (the default) carries exactly the current
/// defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileKnobs {
    profile: ProfileName,
    stream_flush_ms: u64,
    max_concurrent_tools: u32,
    context_budget_hint: u64,
}

impl ProfileKnobs {
    /// The named profile these knobs were resolved from.
    pub fn profile(&self) -> ProfileName {
        self.profile
    }

    /// Advised stream-flush cadence, in milliseconds.
    pub fn stream_flush_ms(&self) -> u64 {
        self.stream_flush_ms
    }

    /// Advised maximum number of concurrently-running tools.
    pub fn max_concurrent_tools(&self) -> u32 {
        self.max_concurrent_tools
    }

    /// Advised context budget hint, in tokens.
    pub fn context_budget_hint(&self) -> u64 {
        self.context_budget_hint
    }

    /// Whether this is the default (`balanced`) posture with all-default knobs.
    pub fn is_default(&self) -> bool {
        self.profile == ProfileName::Balanced
    }

    /// Human-readable rows describing the selected profile and each resolved
    /// knob. The first row names the profile (`profile: <name>`); the remaining
    /// rows list each advisory knob. Used by `Config::perf_release_summary` for
    /// doctor/info.
    pub fn summary_lines(&self) -> Vec<String> {
        let default_tag = if self.is_default() { " (default)" } else { "" };
        vec![
            format!("profile: {}{}", self.profile.as_str(), default_tag),
            format!("stream-flush: {}ms", self.stream_flush_ms),
            format!("max-concurrent-tools: {}", self.max_concurrent_tools),
            format!("context-budget-hint: {} tokens", self.context_budget_hint),
        ]
    }
}

/// Read a single key as a trimmed string, env-first.
///
/// The raw environment variable is consulted before the merged config file so a
/// bare profile name survives as a string rather than being coerced by the
/// config parser. Returns `None` when the key is absent or empty. Mirrors
/// `streaming_perf::read_key` / `resource_limits::read_key`.
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

/// Resolve the selected [`ProfileName`] from a specific config, reading
/// env-first. When the key is unset (or empty) this is [`ProfileName::Balanced`];
/// an unrecognised name also falls back to `balanced` (never panics).
pub fn profile_for_config(config: &Config) -> ProfileName {
    match read_key(config, PERF_PROFILE_KEY) {
        Some(raw) => ProfileName::parse(&raw),
        None => ProfileName::default(),
    }
}

/// Resolve the selected profile's concrete advisory knobs from a specific config.
/// This is the real call site reached from `Config::perf_release_summary`, and the
/// single typed source advisory perf consumers can read.
pub fn knobs_for_config(config: &Config) -> ProfileKnobs {
    profile_for_config(config).knobs()
}

/// Resolve the selected profile from the global config.
pub fn resolve() -> ProfileKnobs {
    knobs_for_config(Config::global())
}

/// Human-readable summary rows for the selected profile, resolved from a specific
/// `Config`. This is the helper `Config::perf_release_summary` delegates to.
pub fn summary_lines_for_config(config: &Config) -> Vec<String> {
    knobs_for_config(config).summary_lines()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The profile key cleared, so resolution falls back to `balanced`.
    fn key_unset() -> [(&'static str, Option<&'static str>); 1] {
        [(PERF_PROFILE_KEY, None)]
    }

    #[test]
    fn unset_resolves_balanced_with_default_knobs() {
        let _guard = env_lock::lock_env(key_unset());
        let knobs = knobs_for_config(Config::global());
        assert_eq!(knobs.profile(), ProfileName::Balanced);
        assert!(knobs.is_default());
        assert_eq!(knobs.stream_flush_ms(), BALANCED_FLUSH_MS);
        assert_eq!(knobs.max_concurrent_tools(), BALANCED_MAX_CONCURRENT_TOOLS);
        assert_eq!(knobs.context_budget_hint(), BALANCED_CONTEXT_BUDGET_HINT);
    }

    #[test]
    fn unset_summary_lists_profile_and_default_knobs() {
        let _guard = env_lock::lock_env(key_unset());
        let lines = summary_lines_for_config(Config::global());
        // First row names the profile and is tagged as the default posture.
        assert!(
            lines[0].starts_with("profile:"),
            "first row should name the profile: {:?}",
            lines[0]
        );
        assert!(lines[0].contains("balanced"));
        assert!(lines[0].contains("(default)"));
        // Default knob values are visible in the summary.
        assert!(lines
            .iter()
            .any(|l| l.contains(&format!("{BALANCED_FLUSH_MS}ms"))));
        assert!(lines
            .iter()
            .any(|l| l.contains(&BALANCED_MAX_CONCURRENT_TOOLS.to_string())));
        // No user-facing donor/fork leakage in any summary row.
        for line in &lines {
            let lower = line.to_ascii_lowercase();
            assert!(!lower.contains("goose"), "leak in row: {line:?}");
            assert!(!line.contains("Block"), "leak in row: {line:?}");
        }
    }

    #[test]
    fn low_latency_resolves_smaller_flush_knob() {
        let _guard = env_lock::lock_env([(PERF_PROFILE_KEY, Some("low-latency"))]);
        let knobs = knobs_for_config(Config::global());
        assert_eq!(knobs.profile(), ProfileName::LowLatency);
        assert!(!knobs.is_default());
        // A tighter flush cadence than balanced.
        assert!(
            knobs.stream_flush_ms() < BALANCED_FLUSH_MS,
            "low-latency flush {} should be < balanced {}",
            knobs.stream_flush_ms(),
            BALANCED_FLUSH_MS
        );
    }

    #[test]
    fn throughput_raises_concurrency_knob() {
        let _guard = env_lock::lock_env([(PERF_PROFILE_KEY, Some("throughput"))]);
        let knobs = knobs_for_config(Config::global());
        assert_eq!(knobs.profile(), ProfileName::Throughput);
        assert!(
            knobs.max_concurrent_tools() > BALANCED_MAX_CONCURRENT_TOOLS,
            "throughput concurrency {} should be > balanced {}",
            knobs.max_concurrent_tools(),
            BALANCED_MAX_CONCURRENT_TOOLS
        );
    }

    #[test]
    fn unrecognised_name_falls_back_to_balanced() {
        let _guard = env_lock::lock_env([(PERF_PROFILE_KEY, Some("warp-drive"))]);
        let knobs = knobs_for_config(Config::global());
        assert_eq!(knobs.profile(), ProfileName::Balanced);
        assert!(knobs.is_default());
    }

    #[test]
    fn parse_is_case_and_separator_insensitive() {
        assert_eq!(
            ProfileName::parse("  LOW_LATENCY "),
            ProfileName::LowLatency
        );
        assert_eq!(ProfileName::parse("Throughput"), ProfileName::Throughput);
        assert_eq!(ProfileName::parse("BATTERY"), ProfileName::Battery);
        assert_eq!(ProfileName::parse(""), ProfileName::Balanced);
    }

    #[test]
    fn battery_trims_cadence_and_budget() {
        let _guard = env_lock::lock_env([(PERF_PROFILE_KEY, Some("battery"))]);
        let knobs = knobs_for_config(Config::global());
        assert_eq!(knobs.profile(), ProfileName::Battery);
        assert!(knobs.stream_flush_ms() > BALANCED_FLUSH_MS);
        assert!(knobs.context_budget_hint() < BALANCED_CONTEXT_BUDGET_HINT);
    }

    #[test]
    fn summary_for_override_drops_default_tag() {
        let _guard = env_lock::lock_env([(PERF_PROFILE_KEY, Some("throughput"))]);
        let lines = summary_lines_for_config(Config::global());
        assert!(lines[0].starts_with("profile:"));
        assert!(lines[0].contains("throughput"));
        assert!(!lines[0].contains("(default)"));
    }
}
