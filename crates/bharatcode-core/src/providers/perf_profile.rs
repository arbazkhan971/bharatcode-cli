//! Perf-release runtime profile — opt-in HTTP connection-pool / concurrency
//! tuning for the shared provider client.
//!
//! The provider layer ships a single, conservative reqwest connection pool.
//! That posture is right for an interactive desktop session, but a throughput-
//! biased *release* deployment (batch evals, server fan-out) benefits from a
//! larger keep-alive pool and a higher request-concurrency hint.
//!
//! This module resolves one named switch — [`ENV_VAR`]
//! (`BHARATCODE_PERF_PROFILE`) — into a fully *clamped* [`PerfProfile`]:
//!
//! * [`Profile::Balanced`] (the default, and the value for an unset / blank /
//!   unrecognised variable) reproduces today's conservative defaults *exactly*,
//!   so behaviour is byte-for-byte unchanged unless the operator opts in.
//! * [`Profile::Release`] raises the pool size, keep-alive idle timeout and the
//!   advisory request-concurrency hint to throughput-friendly values.
//!
//! Two per-knob overrides let an operator tune a single dial without selecting
//! a whole profile:
//!
//! * `BHARATCODE_HTTP_POOL_MAX`  — overrides `pool_max_idle` (per host).
//! * `BHARATCODE_HTTP_IDLE_SECS` — overrides `pool_idle_timeout_secs`.
//!
//! Every resolved value is **clamped to a sane range** ([`POOL_MIN`]..=
//! [`POOL_MAX`] etc.) so neither a profile nor an out-of-range override can push
//! a knob into a pathological setting. Resolution is *pure config*: it reads the
//! environment and returns a value — it performs no I/O and mutates nothing.
//!
//! The consumer is the central shared provider client (see
//! [`super`]: the providers module reads [`resolve`] when building the shared
//! reqwest client and feeds `pool_max_idle`/`pool_idle_timeout_secs` into the
//! `reqwest::ClientBuilder`, diverging from reqwest's defaults only when the
//! profile is not `balanced`).
//!
//! Original BharatCode work; not ported from any third party.

use std::time::Duration;

/// Environment variable selecting the runtime performance profile (opt-in).
///
/// Recognised values (case-insensitive, surrounding whitespace ignored):
/// `balanced` (the default) and `release`. Anything else — including unset or
/// blank — resolves to [`Profile::Balanced`], i.e. today's behaviour.
pub const ENV_VAR: &str = "BHARATCODE_PERF_PROFILE";

/// Per-knob override for the connection pool's max idle connections per host.
pub const POOL_MAX_ENV: &str = "BHARATCODE_HTTP_POOL_MAX";

/// Per-knob override for the pool's keep-alive idle timeout, in seconds.
pub const IDLE_SECS_ENV: &str = "BHARATCODE_HTTP_IDLE_SECS";

// --- Clamp bounds -----------------------------------------------------------

/// Lower / upper bound for `pool_max_idle` (idle connections kept per host).
/// A pool of zero would disable keep-alive entirely; 256 is far above any
/// realistic provider fan-out and guards against a runaway override.
pub const POOL_MIN: usize = 1;
pub const POOL_MAX: usize = 256;

/// Lower / upper bound for `pool_idle_timeout_secs`. A 1s floor keeps at least
/// a brief keep-alive window; the 3600s (1h) ceiling stops an idle socket from
/// being pinned open indefinitely.
pub const IDLE_SECS_MIN: u64 = 1;
pub const IDLE_SECS_MAX: u64 = 3600;

/// Lower / upper bound for the advisory `max_concurrency` hint surfaced to the
/// provider layer. At least one in-flight request; 1024 is a generous ceiling.
pub const CONCURRENCY_MIN: usize = 1;
pub const CONCURRENCY_MAX: usize = 1024;

// --- Documented per-profile defaults ---------------------------------------

/// Conservative (`balanced`) defaults — these reproduce today's behaviour.
/// `pool_max_idle` mirrors reqwest's own default keep-alive posture, the idle
/// timeout matches a typical 90s keep-alive, and the concurrency hint is modest.
pub const BALANCED_POOL_MAX_IDLE: usize = 8;
pub const BALANCED_POOL_IDLE_TIMEOUT_SECS: u64 = 90;
pub const BALANCED_MAX_CONCURRENCY: usize = 8;

/// Throughput-biased (`release`) defaults: a larger keep-alive pool, a longer
/// idle window so warm connections survive bursty gaps, and a higher
/// request-concurrency hint.
pub const RELEASE_POOL_MAX_IDLE: usize = 64;
pub const RELEASE_POOL_IDLE_TIMEOUT_SECS: u64 = 300;
pub const RELEASE_MAX_CONCURRENCY: usize = 64;

/// The named runtime profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// Today's conservative defaults. The value for unset / blank / unrecognised
    /// `BHARATCODE_PERF_PROFILE`, so default behaviour is unchanged.
    Balanced,
    /// Throughput-biased posture: larger pool, longer keep-alive, higher
    /// concurrency hint.
    Release,
}

impl Profile {
    /// Parse a raw profile name (case-insensitive, whitespace-trimmed).
    ///
    /// Unrecognised / blank input falls back to [`Profile::Balanced`] so the
    /// caller stays on today's behaviour.
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "release" => Self::Release,
            _ => Self::Balanced,
        }
    }

    /// Stable, brand-free label for this profile.
    pub fn label(self) -> &'static str {
        match self {
            Self::Balanced => "balanced",
            Self::Release => "release",
        }
    }

    /// Whether this profile diverges from the conservative `balanced` defaults.
    ///
    /// The shared client build uses this to leave its `reqwest::ClientBuilder`
    /// completely untouched on the default path.
    pub fn diverges_from_default(self) -> bool {
        self != Self::Balanced
    }

    /// The base (pre-override) tuned values for this profile.
    fn defaults(self) -> PerfProfile {
        match self {
            Self::Balanced => PerfProfile {
                profile: self,
                pool_max_idle: BALANCED_POOL_MAX_IDLE,
                pool_idle_timeout_secs: BALANCED_POOL_IDLE_TIMEOUT_SECS,
                max_concurrency: BALANCED_MAX_CONCURRENCY,
            },
            Self::Release => PerfProfile {
                profile: self,
                pool_max_idle: RELEASE_POOL_MAX_IDLE,
                pool_idle_timeout_secs: RELEASE_POOL_IDLE_TIMEOUT_SECS,
                max_concurrency: RELEASE_MAX_CONCURRENCY,
            },
        }
    }
}

/// A fully-resolved, fully-clamped snapshot of the runtime perf tunables.
///
/// Every field is guaranteed to sit within its documented clamp range, so a
/// consumer can feed these values straight into the HTTP client builder without
/// re-validating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerfProfile {
    /// Which named profile produced this snapshot.
    profile: Profile,
    /// Max idle keep-alive connections per host. Clamped to
    /// [`POOL_MIN`]..=[`POOL_MAX`].
    pub pool_max_idle: usize,
    /// Keep-alive idle timeout, seconds. Clamped to
    /// [`IDLE_SECS_MIN`]..=[`IDLE_SECS_MAX`].
    pub pool_idle_timeout_secs: u64,
    /// Advisory request-concurrency hint surfaced to the provider layer.
    /// Clamped to [`CONCURRENCY_MIN`]..=[`CONCURRENCY_MAX`].
    pub max_concurrency: usize,
}

impl PerfProfile {
    /// The named profile this snapshot came from.
    pub fn profile(&self) -> Profile {
        self.profile
    }

    /// Whether this snapshot diverges from the conservative `balanced` defaults.
    pub fn diverges_from_default(&self) -> bool {
        self.profile.diverges_from_default()
    }

    /// The keep-alive idle timeout as a [`Duration`], ready for
    /// `reqwest::ClientBuilder::pool_idle_timeout`.
    pub fn pool_idle_timeout(&self) -> Duration {
        Duration::from_secs(self.pool_idle_timeout_secs)
    }

    /// One human-readable, brand-free row per knob for doctor / info output.
    pub fn summary_lines(&self) -> Vec<String> {
        vec![
            format!("{ENV_VAR} = {}", self.profile.label()),
            format!("{POOL_MAX_ENV} = {}", self.pool_max_idle),
            format!("{IDLE_SECS_ENV} = {}s", self.pool_idle_timeout_secs),
            format!("max_concurrency = {}", self.max_concurrency),
        ]
    }
}

/// Resolve the effective runtime perf profile from the environment.
///
/// Reads [`ENV_VAR`] to pick the base profile (defaulting to
/// [`Profile::Balanced`]), then applies the optional per-knob overrides
/// [`POOL_MAX_ENV`] / [`IDLE_SECS_ENV`]. Every field of the returned
/// [`PerfProfile`] is clamped to its documented bound. Pure config resolution:
/// reads the environment, performs no I/O, mutates nothing.
///
/// With no environment set this returns the exact `balanced` defaults, so the
/// shared client build is byte-for-byte unchanged.
pub fn resolve() -> PerfProfile {
    let profile = match std::env::var(ENV_VAR) {
        Ok(raw) => Profile::parse(&raw),
        Err(_) => Profile::Balanced,
    };
    let pool_override = env_usize(POOL_MAX_ENV);
    let idle_override = env_u64(IDLE_SECS_ENV);
    resolve_from(profile, pool_override, idle_override)
}

/// Pure resolver over explicit inputs (testable without touching the env).
///
/// Mirrors [`resolve`]: starts from the profile defaults, applies any present
/// per-knob override, and clamps every field to its documented range.
pub fn resolve_from(
    profile: Profile,
    pool_override: Option<usize>,
    idle_override: Option<u64>,
) -> PerfProfile {
    let mut p = profile.defaults();

    if let Some(pool) = pool_override {
        p.pool_max_idle = pool;
    }
    if let Some(idle) = idle_override {
        p.pool_idle_timeout_secs = idle;
    }

    // Clamp every field, regardless of whether it came from a profile default
    // or an override, so nothing can escape its documented bound.
    p.pool_max_idle = p.pool_max_idle.clamp(POOL_MIN, POOL_MAX);
    p.pool_idle_timeout_secs = p.pool_idle_timeout_secs.clamp(IDLE_SECS_MIN, IDLE_SECS_MAX);
    p.max_concurrency = p.max_concurrency.clamp(CONCURRENCY_MIN, CONCURRENCY_MAX);
    p
}

/// Read a `usize` from `key`, returning `None` for unset / blank / unparsable.
fn env_usize(key: &str) -> Option<usize> {
    let raw = std::env::var(key).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<usize>().ok()
}

/// Read a `u64` from `key`, returning `None` for unset / blank / unparsable.
fn env_u64(key: &str) -> Option<u64> {
    let raw = std::env::var(key).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<u64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialize tests that mutate the shared process environment.
    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn clear_all_env() {
        std::env::remove_var(ENV_VAR);
        std::env::remove_var(POOL_MAX_ENV);
        std::env::remove_var(IDLE_SECS_ENV);
    }

    fn assert_within_bounds(p: &PerfProfile) {
        assert!(
            (POOL_MIN..=POOL_MAX).contains(&p.pool_max_idle),
            "pool_max_idle {} out of bounds",
            p.pool_max_idle
        );
        assert!(
            (IDLE_SECS_MIN..=IDLE_SECS_MAX).contains(&p.pool_idle_timeout_secs),
            "pool_idle_timeout_secs {} out of bounds",
            p.pool_idle_timeout_secs
        );
        assert!(
            (CONCURRENCY_MIN..=CONCURRENCY_MAX).contains(&p.max_concurrency),
            "max_concurrency {} out of bounds",
            p.max_concurrency
        );
    }

    #[test]
    fn resolve_with_no_env_returns_balanced_defaults() {
        let _guard = env_guard();
        clear_all_env();

        let p = resolve();
        // Exactly the pre-existing conservative defaults.
        assert_eq!(p.profile(), Profile::Balanced);
        assert_eq!(p.pool_max_idle, BALANCED_POOL_MAX_IDLE);
        assert_eq!(p.pool_idle_timeout_secs, BALANCED_POOL_IDLE_TIMEOUT_SECS);
        assert_eq!(p.max_concurrency, BALANCED_MAX_CONCURRENCY);
        assert!(!p.diverges_from_default());
        assert_within_bounds(&p);
    }

    #[test]
    fn balanced_yields_exact_preexisting_defaults() {
        // Independent of the environment: the balanced profile is the documented
        // conservative baseline and must never change without an explicit bump.
        let p = resolve_from(Profile::Balanced, None, None);
        assert_eq!(p.pool_max_idle, 8);
        assert_eq!(p.pool_idle_timeout_secs, 90);
        assert_eq!(p.max_concurrency, 8);
        assert_eq!(p.pool_idle_timeout(), Duration::from_secs(90));
    }

    #[test]
    fn release_profile_returns_higher_pool() {
        let _guard = env_guard();
        clear_all_env();
        std::env::set_var(ENV_VAR, "release");

        let p = resolve();
        assert_eq!(p.profile(), Profile::Release);
        assert!(
            p.pool_max_idle > resolve_from(Profile::Balanced, None, None).pool_max_idle,
            "release pool {} should exceed balanced pool",
            p.pool_max_idle
        );
        assert_eq!(p.pool_max_idle, RELEASE_POOL_MAX_IDLE);
        assert!(p.diverges_from_default());
        assert_within_bounds(&p);

        clear_all_env();
    }

    #[test]
    fn release_is_case_and_whitespace_insensitive() {
        let _guard = env_guard();
        clear_all_env();
        std::env::set_var(ENV_VAR, "  RELEASE  ");
        assert_eq!(resolve().profile(), Profile::Release);
        clear_all_env();
    }

    #[test]
    fn out_of_range_pool_override_clamps_to_max() {
        let _guard = env_guard();
        clear_all_env();
        std::env::set_var(POOL_MAX_ENV, "99999");

        let p = resolve();
        assert_eq!(
            p.pool_max_idle, POOL_MAX,
            "an out-of-range pool override must clamp to the max bound"
        );
        assert_within_bounds(&p);

        clear_all_env();
    }

    #[test]
    fn zero_pool_override_clamps_to_min() {
        let p = resolve_from(Profile::Release, Some(0), None);
        assert_eq!(p.pool_max_idle, POOL_MIN);
        assert_within_bounds(&p);
    }

    #[test]
    fn idle_override_applies_and_clamps() {
        // An in-range override is taken verbatim.
        let mid = resolve_from(Profile::Balanced, None, Some(120));
        assert_eq!(mid.pool_idle_timeout_secs, 120);
        // A wildly large override clamps to the ceiling.
        let high = resolve_from(Profile::Balanced, None, Some(u64::MAX));
        assert_eq!(high.pool_idle_timeout_secs, IDLE_SECS_MAX);
        // Zero clamps up to the floor.
        let low = resolve_from(Profile::Balanced, None, Some(0));
        assert_eq!(low.pool_idle_timeout_secs, IDLE_SECS_MIN);
    }

    #[test]
    fn unrecognised_profile_falls_back_to_balanced() {
        assert_eq!(Profile::parse("turbo"), Profile::Balanced);
        assert_eq!(Profile::parse(""), Profile::Balanced);
        assert_eq!(Profile::parse("   "), Profile::Balanced);
    }

    #[test]
    fn blank_or_garbage_override_is_ignored() {
        let _guard = env_guard();
        clear_all_env();
        std::env::set_var(POOL_MAX_ENV, "   ");
        std::env::set_var(IDLE_SECS_ENV, "not-a-number");

        let p = resolve();
        // Falls back to balanced defaults — the overrides are not applied.
        assert_eq!(p.pool_max_idle, BALANCED_POOL_MAX_IDLE);
        assert_eq!(p.pool_idle_timeout_secs, BALANCED_POOL_IDLE_TIMEOUT_SECS);

        clear_all_env();
    }

    #[test]
    fn summary_lines_are_brand_free() {
        let lines = resolve_from(Profile::Release, None, None).summary_lines();
        assert!(!lines.is_empty());
        for line in &lines {
            let lower = line.to_ascii_lowercase();
            assert!(
                !lower.contains("goose") && !lower.contains("block"),
                "summary row must be brand-free: {line:?}"
            );
        }
    }

    #[test]
    fn override_wins_over_profile_default() {
        // A per-knob override applies even when a profile is selected.
        let p = resolve_from(Profile::Release, Some(20), Some(45));
        assert_eq!(p.pool_max_idle, 20);
        assert_eq!(p.pool_idle_timeout_secs, 45);
        // The un-overridden knob keeps the profile's value.
        assert_eq!(p.max_concurrency, RELEASE_MAX_CONCURRENCY);
    }
}
