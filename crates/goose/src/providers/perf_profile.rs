//! Named performance profile — a single switch over the already-shipped
//! provider-layer perf tunables.
//!
//! Several independent perf knobs already exist, each behind its own
//! `BHARATCODE_*` environment variable:
//!
//! * stream flush cadence (`BHARATCODE_STREAM_FLUSH_MS`, see
//!   [`crate::streaming_perf`]),
//! * in-flight request coalescing (`BHARATCODE_COALESCE`, see
//!   [`super::coalesce`]),
//! * the parallel-tool concurrency cap (`BHARATCODE_TOOL_MAX_INFLIGHT`, see
//!   [`crate::tool_governor`]),
//! * the central retry budget (`BHARATCODE_RETRY_MAX`, see
//!   `goose_providers::retry`), and
//! * the per-request deadline (`BHARATCODE_PROVIDER_DEADLINE_SECS`, see
//!   [`super::deadline`]).
//!
//! Tuning all five by hand to a coherent posture is fiddly. This module bundles
//! them behind one named switch: [`ENV_VAR`] (`BHARATCODE_PERF_PROFILE`)
//! resolves to a coherent [`PerfProfile`] whose every field is *already clamped*
//! to the same safe range its individual tunable enforces, so a profile can
//! never push a knob outside the range that knob already guarantees.
//!
//! ## Default OFF
//!
//! [`ENV_VAR`] defaults unset. When it is unset, blank, or an unrecognised
//! value, [`resolve`] returns `None` and nothing changes — behaviour is
//! byte-for-byte identical to today.
//!
//! ## Reads and reports only — never overrides what the user set
//!
//! A profile is *advisory*. It is a coherent set of suggested effective values
//! that the streaming / coalesce paths and doctor can read from one validated
//! source. It does **not** mutate the environment, and the documented
//! precedence is: **an explicit individual `BHARATCODE_*` tunable always wins
//! over the profile.** [`PerfProfile::summary_lines`] reports, per field,
//! whether an explicit per-tunable override is in effect and therefore takes
//! precedence over the profile's suggested value.
//!
//! Original BharatCode work; not ported from any third party.

/// Environment variable selecting the performance profile (env-first opt-in).
///
/// Recognised values (case-insensitive, surrounding whitespace ignored):
/// `release`, `balanced`, `low-latency` (also `lowlatency` / `low_latency`).
/// Anything else — including unset or blank — leaves the feature inert.
pub const ENV_VAR: &str = "BHARATCODE_PERF_PROFILE";

// --- Clamp bounds, kept consistent with each individual tunable's own range ---

/// Stream flush cadence bounds, mirroring [`crate::streaming_perf`]
/// (`STREAM_FLUSH_MS_MIN`/`MAX`): a flush every <1ms is a busy spin and slower
/// than 10s feels frozen.
const STREAM_FLUSH_MS_MIN: u64 = 1;
const STREAM_FLUSH_MS_MAX: u64 = 10_000;

/// Parallel-tool concurrency bounds, mirroring [`crate::tool_governor`]
/// (`MIN_INFLIGHT`/`MAX_INFLIGHT`).
const TOOL_MAX_INFLIGHT_MIN: usize = 1;
const TOOL_MAX_INFLIGHT_MAX: usize = 64;

/// Retry-budget bounds (total attempts including the first), mirroring
/// `goose_providers::retry::ENV_MAX_ATTEMPTS_CAP` (clamped `1..=10`).
const RETRY_MAX_MIN: usize = 1;
const RETRY_MAX_MAX: usize = 10;

/// Per-request deadline ceiling (seconds), mirroring [`super::deadline`]
/// (`MAX_DEADLINE_SECS`, 24h). The floor is 1s: a deadline of 0 disables the
/// guard entirely, so a profile that opts into a deadline keeps it positive.
const DEADLINE_SECS_MIN: u64 = 1;
const DEADLINE_SECS_MAX: u64 = 24 * 60 * 60;

/// The three named profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileName {
    /// Throughput-biased posture for release builds: coalesce duplicate
    /// in-flight calls, a generous retry budget, a bounded-but-roomy tool
    /// fan-out, a relaxed flush cadence and a long safety deadline.
    Release,
    /// A middle ground between throughput and interactivity.
    Balanced,
    /// Interactivity-biased posture: the smallest flush cadence (snappiest
    /// stream), a tight tool fan-out and a short deadline; coalescing off so a
    /// request is never made to wait on an unrelated in-flight peer.
    LowLatency,
}

impl ProfileName {
    /// Parse a raw profile name (case-insensitive, whitespace-trimmed).
    ///
    /// Returns `None` for unset/blank/unrecognised input so the caller stays a
    /// no-op. Accepts a few spellings of low-latency for convenience.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "release" => Some(Self::Release),
            "balanced" => Some(Self::Balanced),
            "low-latency" | "lowlatency" | "low_latency" => Some(Self::LowLatency),
            _ => None,
        }
    }

    /// Stable, brand-free label for this profile (used in summaries).
    pub fn label(self) -> &'static str {
        match self {
            Self::Release => "release",
            Self::Balanced => "balanced",
            Self::LowLatency => "low-latency",
        }
    }

    /// The coherent, pre-clamped set of effective values for this profile.
    fn profile(self) -> PerfProfile {
        match self {
            // Throughput first: relaxed flush, coalesce on, full retry budget,
            // roomy tool fan-out, long safety deadline.
            Self::Release => PerfProfile {
                name: self,
                stream_flush_ms: clamp_u64(120, STREAM_FLUSH_MS_MIN, STREAM_FLUSH_MS_MAX),
                coalesce: true,
                tool_max_inflight: clamp_usize(16, TOOL_MAX_INFLIGHT_MIN, TOOL_MAX_INFLIGHT_MAX),
                retry_max: clamp_usize(6, RETRY_MAX_MIN, RETRY_MAX_MAX),
                deadline_secs: Some(clamp_u64(600, DEADLINE_SECS_MIN, DEADLINE_SECS_MAX)),
            },
            // Middle ground.
            Self::Balanced => PerfProfile {
                name: self,
                stream_flush_ms: clamp_u64(50, STREAM_FLUSH_MS_MIN, STREAM_FLUSH_MS_MAX),
                coalesce: true,
                tool_max_inflight: clamp_usize(8, TOOL_MAX_INFLIGHT_MIN, TOOL_MAX_INFLIGHT_MAX),
                retry_max: clamp_usize(4, RETRY_MAX_MIN, RETRY_MAX_MAX),
                deadline_secs: Some(clamp_u64(120, DEADLINE_SECS_MIN, DEADLINE_SECS_MAX)),
            },
            // Interactivity first: snappiest flush, coalescing off, tight
            // fan-out, lean retry budget, short deadline.
            Self::LowLatency => PerfProfile {
                name: self,
                stream_flush_ms: clamp_u64(10, STREAM_FLUSH_MS_MIN, STREAM_FLUSH_MS_MAX),
                coalesce: false,
                tool_max_inflight: clamp_usize(4, TOOL_MAX_INFLIGHT_MIN, TOOL_MAX_INFLIGHT_MAX),
                retry_max: clamp_usize(2, RETRY_MAX_MIN, RETRY_MAX_MAX),
                deadline_secs: Some(clamp_u64(30, DEADLINE_SECS_MIN, DEADLINE_SECS_MAX)),
            },
        }
    }
}

/// A coherent, fully-clamped snapshot of the provider-layer perf tunables that
/// a named profile resolves to.
///
/// Every field is already within its individual tunable's documented safe
/// range (see the module-level clamp constants), so reading these values can
/// never push a knob out of range. The values are *suggested* effective
/// values: an explicit per-tunable `BHARATCODE_*` variable, when set, takes
/// precedence (see [`PerfProfile::summary_lines`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerfProfile {
    /// Which named profile produced this snapshot.
    name: ProfileName,
    /// Suggested stream flush cadence, ms. Clamped to `[1, 10_000]`.
    pub stream_flush_ms: u64,
    /// Whether the profile suggests enabling in-flight request coalescing.
    pub coalesce: bool,
    /// Suggested parallel-tool concurrency cap. Clamped to `[1, 64]`.
    pub tool_max_inflight: usize,
    /// Suggested retry budget (total attempts incl. the first). Clamped to
    /// `[1, 10]`.
    pub retry_max: usize,
    /// Suggested per-request deadline, seconds. `Some` is clamped to
    /// `[1, 86_400]`; `None` means the profile does not impose a deadline.
    pub deadline_secs: Option<u64>,
}

impl PerfProfile {
    /// The named profile this snapshot came from.
    pub fn name(&self) -> ProfileName {
        self.name
    }

    /// One human-readable row per tunable for doctor / info output.
    ///
    /// Each row reports the profile's suggested value and, when an explicit
    /// individual `BHARATCODE_*` variable is set, notes that the explicit
    /// override takes precedence over the profile — the documented precedence
    /// rule. Reads the environment only; never mutates it. Brand-free labels.
    pub fn summary_lines(&self) -> Vec<String> {
        vec![
            summary_row(
                "BHARATCODE_STREAM_FLUSH_MS",
                format!("{}ms", self.stream_flush_ms),
                env_is_set("BHARATCODE_STREAM_FLUSH_MS"),
            ),
            summary_row(
                "BHARATCODE_COALESCE",
                if self.coalesce { "on" } else { "off" }.to_string(),
                env_is_set("BHARATCODE_COALESCE"),
            ),
            summary_row(
                "BHARATCODE_TOOL_MAX_INFLIGHT",
                self.tool_max_inflight.to_string(),
                env_is_set("BHARATCODE_TOOL_MAX_INFLIGHT"),
            ),
            summary_row(
                "BHARATCODE_RETRY_MAX",
                self.retry_max.to_string(),
                env_is_set("BHARATCODE_RETRY_MAX"),
            ),
            summary_row(
                "BHARATCODE_PROVIDER_DEADLINE_SECS",
                match self.deadline_secs {
                    Some(secs) => format!("{secs}s"),
                    None => "none".to_string(),
                },
                env_is_set("BHARATCODE_PROVIDER_DEADLINE_SECS"),
            ),
        ]
    }
}

/// Resolve the configured performance profile, if any.
///
/// Reads [`ENV_VAR`] (env-first). Returns `None` when the variable is unset,
/// blank, or unrecognised — keeping the feature inert (no behaviour change) by
/// default. When a recognised value is set, returns the coherent, fully-clamped
/// [`PerfProfile`] for that profile.
pub fn resolve() -> Option<PerfProfile> {
    let raw = std::env::var(ENV_VAR).ok()?;
    resolve_value(&raw)
}

/// Pure resolver over an explicit value (testable without touching the env).
///
/// Mirrors [`resolve`]: blank / unrecognised => `None`; a recognised name =>
/// its clamped [`PerfProfile`].
pub fn resolve_value(raw: &str) -> Option<PerfProfile> {
    ProfileName::parse(raw).map(ProfileName::profile)
}

/// Render one `KEY = value` summary row. When the individual tunable's own
/// environment variable is set, the row notes that the explicit value takes
/// precedence over the profile's suggestion.
fn summary_row(key: &str, value: String, explicit_override: bool) -> String {
    if explicit_override {
        format!("{key} = {value} (explicit {key} set; overrides profile)")
    } else {
        format!("{key} = {value} (from profile)")
    }
}

/// Whether `key` is set to a non-blank value in the environment.
fn env_is_set(key: &str) -> bool {
    std::env::var(key)
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
}

/// `clamp` for `u64` that does not require an `Ord` import at the call site.
fn clamp_u64(value: u64, min: u64, max: u64) -> u64 {
    value.clamp(min, max)
}

/// `clamp` for `usize`.
fn clamp_usize(value: usize, min: usize, max: usize) -> usize {
    value.clamp(min, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialize tests that mutate the shared process environment.
    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Assert every field of a resolved profile sits within the documented
    /// clamp bounds, so a profile can never push a tunable out of range.
    fn assert_within_bounds(p: &PerfProfile) {
        assert!(
            (STREAM_FLUSH_MS_MIN..=STREAM_FLUSH_MS_MAX).contains(&p.stream_flush_ms),
            "stream_flush_ms {} out of bounds",
            p.stream_flush_ms
        );
        assert!(
            (TOOL_MAX_INFLIGHT_MIN..=TOOL_MAX_INFLIGHT_MAX).contains(&p.tool_max_inflight),
            "tool_max_inflight {} out of bounds",
            p.tool_max_inflight
        );
        assert!(
            (RETRY_MAX_MIN..=RETRY_MAX_MAX).contains(&p.retry_max),
            "retry_max {} out of bounds",
            p.retry_max
        );
        if let Some(secs) = p.deadline_secs {
            assert!(
                (DEADLINE_SECS_MIN..=DEADLINE_SECS_MAX).contains(&secs),
                "deadline_secs {secs} out of bounds"
            );
        }
    }

    #[test]
    fn resolve_inert_when_env_unset() {
        let _guard = env_guard();
        std::env::remove_var(ENV_VAR);
        assert_eq!(resolve(), None);
    }

    #[test]
    fn resolve_value_none_for_blank_and_garbage() {
        assert_eq!(resolve_value(""), None);
        assert_eq!(resolve_value("   "), None);
        assert_eq!(resolve_value("turbo"), None);
        assert_eq!(resolve_value("fastest-ever"), None);
        assert_eq!(resolve_value("0"), None);
    }

    #[test]
    fn resolve_value_parses_each_known_profile() {
        assert_eq!(
            resolve_value("release").map(|p| p.name()),
            Some(ProfileName::Release)
        );
        assert_eq!(
            resolve_value("  Balanced  ").map(|p| p.name()),
            Some(ProfileName::Balanced)
        );
        assert_eq!(
            resolve_value("LOW-LATENCY").map(|p| p.name()),
            Some(ProfileName::LowLatency)
        );
        // Convenience spellings of low-latency.
        assert_eq!(
            resolve_value("lowlatency").map(|p| p.name()),
            Some(ProfileName::LowLatency)
        );
        assert_eq!(
            resolve_value("low_latency").map(|p| p.name()),
            Some(ProfileName::LowLatency)
        );
    }

    #[test]
    fn every_profile_field_is_within_clamp_bounds() {
        for raw in ["release", "balanced", "low-latency"] {
            let p = resolve_value(raw).expect("known profile resolves");
            assert_within_bounds(&p);
        }
    }

    #[test]
    fn low_latency_flushes_faster_than_balanced() {
        let low = resolve_value("low-latency").unwrap();
        let balanced = resolve_value("balanced").unwrap();
        assert!(
            low.stream_flush_ms < balanced.stream_flush_ms,
            "low-latency flush {} should be smaller than balanced flush {}",
            low.stream_flush_ms,
            balanced.stream_flush_ms
        );
    }

    #[test]
    fn release_enables_coalesce_low_latency_does_not() {
        assert!(resolve_value("release").unwrap().coalesce);
        assert!(!resolve_value("low-latency").unwrap().coalesce);
    }

    #[test]
    fn resolve_reads_env_first() {
        let _guard = env_guard();
        std::env::set_var(ENV_VAR, "release");
        let p = resolve().expect("env-set profile resolves");
        assert_eq!(p.name(), ProfileName::Release);
        assert_within_bounds(&p);
        std::env::remove_var(ENV_VAR);
    }

    #[test]
    fn summary_lines_one_row_per_tunable_and_brand_free() {
        let _guard = env_guard();
        // Clear every per-tunable override so the profile values are reported as
        // coming from the profile (not overridden).
        for key in [
            "BHARATCODE_STREAM_FLUSH_MS",
            "BHARATCODE_COALESCE",
            "BHARATCODE_TOOL_MAX_INFLIGHT",
            "BHARATCODE_RETRY_MAX",
            "BHARATCODE_PROVIDER_DEADLINE_SECS",
        ] {
            std::env::remove_var(key);
        }
        let lines = resolve_value("release").unwrap().summary_lines();
        assert_eq!(lines.len(), 5, "one row per bundled tunable");
        for line in &lines {
            assert!(
                line.contains("(from profile)"),
                "with no override the row should be tagged from-profile: {line:?}"
            );
            let lower = line.to_ascii_lowercase();
            assert!(
                !lower.contains("goose") && !lower.contains("block"),
                "summary row must be brand-free: {line:?}"
            );
        }
    }

    #[test]
    fn summary_reports_explicit_override_taking_precedence() {
        let _guard = env_guard();
        // An explicit per-tunable variable must be reported as overriding the
        // profile (documented precedence), and the other rows must not be.
        for key in [
            "BHARATCODE_STREAM_FLUSH_MS",
            "BHARATCODE_COALESCE",
            "BHARATCODE_TOOL_MAX_INFLIGHT",
            "BHARATCODE_RETRY_MAX",
            "BHARATCODE_PROVIDER_DEADLINE_SECS",
        ] {
            std::env::remove_var(key);
        }
        std::env::set_var("BHARATCODE_STREAM_FLUSH_MS", "7");

        let lines = resolve_value("release").unwrap().summary_lines();
        let flush_row = lines
            .iter()
            .find(|l| l.starts_with("BHARATCODE_STREAM_FLUSH_MS"))
            .expect("flush row present");
        assert!(
            flush_row.contains("overrides profile"),
            "explicit flush override should be reported as taking precedence: {flush_row:?}"
        );

        // A row whose tunable was NOT explicitly set stays from-profile.
        let retry_row = lines
            .iter()
            .find(|l| l.starts_with("BHARATCODE_RETRY_MAX"))
            .expect("retry row present");
        assert!(
            retry_row.contains("(from profile)"),
            "un-overridden retry row should remain from-profile: {retry_row:?}"
        );

        std::env::remove_var("BHARATCODE_STREAM_FLUSH_MS");
    }

    #[test]
    fn blank_explicit_var_is_not_treated_as_override() {
        let _guard = env_guard();
        for key in [
            "BHARATCODE_STREAM_FLUSH_MS",
            "BHARATCODE_COALESCE",
            "BHARATCODE_TOOL_MAX_INFLIGHT",
            "BHARATCODE_RETRY_MAX",
            "BHARATCODE_PROVIDER_DEADLINE_SECS",
        ] {
            std::env::remove_var(key);
        }
        // A blank value is not a real override.
        std::env::set_var("BHARATCODE_COALESCE", "   ");
        let lines = resolve_value("balanced").unwrap().summary_lines();
        let coalesce_row = lines
            .iter()
            .find(|l| l.starts_with("BHARATCODE_COALESCE"))
            .expect("coalesce row present");
        assert!(
            coalesce_row.contains("(from profile)"),
            "blank explicit var must not be treated as an override: {coalesce_row:?}"
        );
        std::env::remove_var("BHARATCODE_COALESCE");
    }
}
