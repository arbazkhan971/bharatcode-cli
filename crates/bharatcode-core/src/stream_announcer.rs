//! Localized, screen-reader-friendly streaming progress announcer (opt-in a11y).
//!
//! During a long model stream the terminal normally shows only an animated
//! spinner. That spinner is invisible to screen readers and to `NO_COLOR` /
//! plain-pipe consumers, so users relying on linear, textual output get no sign
//! that work is still progressing. This module emits a short, localized progress
//! line — e.g. `Working… 12s elapsed` — at most once per interval, as ordinary
//! assistant-status text, so that feedback is announced rather than animated.
//!
//! The type is pure and time-injectable: [`Announcer::tick`] takes the clock as
//! an argument, so the throttling logic is fully unit-testable with a fixed
//! clock and never reads the wall clock itself.
//!
//! The feature is opt-in behind the `BHARATCODE_A11Y` environment variable. When
//! it is unset — the default — the call site never constructs an [`Announcer`]
//! and [`announcer_enabled`] returns `false`, so no extra output is produced and
//! the visual spinner path is entirely unchanged.

use std::time::{Duration, Instant};

/// Environment variable that opts the accessible announcer in. Default (unset)
/// is OFF: no progress lines are emitted and the visual spinner is unchanged.
const ENV_VAR: &str = "BHARATCODE_A11Y";

/// Minimum wall time between two emitted progress lines. Chosen so the
/// announcements stay informative without flooding a screen reader.
const DEFAULT_INTERVAL: Duration = Duration::from_secs(10);

/// Returns `true` when the accessible streaming announcer has been opted in via
/// the `BHARATCODE_A11Y` environment variable.
///
/// Any truthy value (`1`, `true`, `yes`, `on`, `enable`, `enabled`,
/// case-insensitive) turns it on. Unset or unrecognised resolves to "off", so
/// default behaviour — the silent visual spinner — is unchanged.
pub fn announcer_enabled() -> bool {
    std::env::var(ENV_VAR)
        .ok()
        .map(|raw| is_truthy(&raw))
        .unwrap_or(false)
}

fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on" | "enable" | "enabled"
    )
}

/// Supported locales for the progress line. Mirrors the small regional set used
/// elsewhere in this crate so the announcement speaks the user's configured
/// language.
#[derive(Debug, Clone, Copy)]
enum Locale {
    En,
    Hi,
    Ta,
}

fn normalize_locale(raw: &str) -> Locale {
    let lowered = raw.trim().to_ascii_lowercase();
    let primary = lowered.split(['_', '-', '.']).next().unwrap_or("");
    match primary {
        "hi" => Locale::Hi,
        "ta" => Locale::Ta,
        _ => Locale::En,
    }
}

/// Resolve the active locale, mirroring the resolution order used by the other
/// localized status emitters in this crate (env override, config, `LANG`, then
/// English).
fn active_locale() -> Locale {
    if let Some(loc) = std::env::var("BHARATCODE_LANG")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        return normalize_locale(&loc);
    }
    if let Some(loc) = crate::config::Config::global()
        .get_param::<String>("bharatcode_lang")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        return normalize_locale(&loc);
    }
    if let Some(loc) = std::env::var("LANG").ok().filter(|s| !s.trim().is_empty()) {
        return normalize_locale(&loc);
    }
    Locale::En
}

/// Build the localized progress line for the given elapsed seconds, e.g.
/// `Working… 12s elapsed`. Kept locale-aware but free of any product/brand
/// identifiers so the announcement never leaks an upstream name.
fn progress_line(locale: Locale, elapsed_secs: u64) -> String {
    match locale {
        Locale::En => format!("Working… {elapsed_secs}s elapsed"),
        Locale::Hi => format!("काम जारी है… {elapsed_secs}से बीत गए"),
        Locale::Ta => format!("வேலை நடக்கிறது… {elapsed_secs}வி கடந்தது"),
    }
}

/// A throttled, time-injectable progress announcer for a single model stream.
///
/// Construct one per stream (only when [`announcer_enabled`] is true) and call
/// [`Announcer::tick`] on every streamed chunk. At most one [`Some`] line is
/// produced per [`interval`](Self::with_interval) of wall time; all other ticks
/// return [`None`].
pub struct Announcer {
    /// Wall time of the most recent emitted line, or `None` until the first one.
    last_emit: Option<Instant>,
    /// Minimum spacing between two emitted lines.
    interval: Duration,
}

impl Default for Announcer {
    fn default() -> Self {
        Self::new()
    }
}

impl Announcer {
    /// Create an announcer with the default 10s interval.
    pub fn new() -> Self {
        Self {
            last_emit: None,
            interval: DEFAULT_INTERVAL,
        }
    }

    /// Create an announcer with an explicit interval (used by tests to drive a
    /// deterministic, fixed-clock schedule).
    #[cfg(test)]
    fn with_interval(interval: Duration) -> Self {
        Self {
            last_emit: None,
            interval,
        }
    }

    /// Advance the announcer with the current clock `now` and the stream's total
    /// `elapsed` time.
    ///
    /// Returns `Some(line)` with a localized progress announcement the first time
    /// it is called and again once at least `interval` has passed since the last
    /// emitted line; otherwise returns `None`. The clock is passed in (never read
    /// internally) so the throttle is fully deterministic under test.
    pub fn tick(&mut self, now: Instant, elapsed: Duration) -> Option<String> {
        let due = match self.last_emit {
            None => true,
            Some(prev) => now.duration_since(prev) >= self.interval,
        };
        if !due {
            return None;
        }
        self.last_emit = Some(now);
        Some(progress_line(active_locale(), elapsed.as_secs()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn announcer_enabled_defaults_off_when_unset() {
        // The integrate step runs tests with the var unset, so the default is
        // OFF. Toggle the raw env within this test to exercise the on-state.
        let prev = std::env::var(ENV_VAR).ok();

        std::env::remove_var(ENV_VAR);
        assert!(!announcer_enabled(), "must default OFF when unset");

        std::env::set_var(ENV_VAR, "1");
        assert!(announcer_enabled(), "truthy value must enable");

        std::env::set_var(ENV_VAR, "false");
        assert!(!announcer_enabled(), "non-truthy value stays disabled");

        match prev {
            Some(v) => std::env::set_var(ENV_VAR, v),
            None => std::env::remove_var(ENV_VAR),
        }
    }

    #[test]
    fn two_ticks_within_interval_yield_exactly_one_some() {
        let interval = Duration::from_secs(10);
        let mut announcer = Announcer::with_interval(interval);
        let start = Instant::now();

        // First tick always emits.
        let first = announcer.tick(start, Duration::from_secs(0));
        assert!(first.is_some(), "the first tick must emit a line");

        // A second tick less than `interval` later must be throttled.
        let second = announcer.tick(start + Duration::from_secs(3), Duration::from_secs(3));
        assert!(
            second.is_none(),
            "a tick within the interval must be suppressed"
        );
    }

    #[test]
    fn tick_after_interval_yields_another_some() {
        let interval = Duration::from_secs(10);
        let mut announcer = Announcer::with_interval(interval);
        let start = Instant::now();

        assert!(
            announcer.tick(start, Duration::from_secs(0)).is_some(),
            "first tick emits"
        );
        assert!(
            announcer
                .tick(start + Duration::from_secs(4), Duration::from_secs(4))
                .is_none(),
            "second tick within the interval is suppressed"
        );

        // A tick at exactly the interval boundary re-arms and emits again.
        let third = announcer.tick(start + interval, Duration::from_secs(10));
        assert!(third.is_some(), "a tick after the interval emits again");
    }

    #[test]
    fn emitted_line_reports_elapsed_seconds_and_no_brand() {
        let mut announcer = Announcer::with_interval(Duration::from_secs(10));
        let line = announcer
            .tick(Instant::now(), Duration::from_secs(12))
            .expect("first tick emits");

        assert!(line.contains("12"), "line reports elapsed seconds: {line}");
        let lowered = line.to_ascii_lowercase();
        assert!(
            !lowered.contains("goose") && !lowered.contains("block"),
            "announcement must not leak an upstream brand: {line}"
        );
    }

    #[test]
    fn localized_lines_render_for_each_supported_locale() {
        // The line builder is pure given a locale, so exercise each branch
        // directly without depending on process-wide env state.
        assert!(progress_line(Locale::En, 7).contains('7'));
        assert!(progress_line(Locale::Hi, 7).contains('7'));
        assert!(progress_line(Locale::Ta, 7).contains('7'));
    }

    #[test]
    fn locale_normalization_maps_regional_tokens() {
        assert!(matches!(normalize_locale("hi_IN.UTF-8"), Locale::Hi));
        assert!(matches!(normalize_locale("ta-IN"), Locale::Ta));
        assert!(matches!(normalize_locale("en_US.UTF-8"), Locale::En));
        assert!(matches!(normalize_locale("fr"), Locale::En));
    }
}
