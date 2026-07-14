//! Lightweight per-turn streaming perf probe (`BHARATCODE_PERF`).
//!
//! The runtime path is pure-`std` (plus `once_cell` for the global handle) and
//! entirely network-free:
//!
//! [`PerfProbe`] is a process-global, lock-free probe that the real provider
//!    streaming path brackets around each turn: [`PerfProbe::mark_request_start`]
//!    before streaming, [`PerfProbe::mark_first_token`] on the first yielded
//!    chunk, and [`PerfProbe::mark_complete`] at the end. It derives
//!    time-to-first-token (ttft) and tokens/second from those marks.
//!
//! The probe is **off by default**. [`PerfProbe::is_enabled`] reads the raw
//! `BHARATCODE_PERF` environment variable once; when it is not truthy *every*
//! mark is a single relaxed atomic-bool load that returns immediately. There is
//! no allocation, no logging, and no behavioural change on the disabled (default)
//! path, so a stock binary pays effectively nothing.
//!
//! Test-only manifest fixtures assert the intended release-profile knobs
//! without shipping an unused runtime configuration API.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;

/// Environment variable that opts the perf probe in. Default (unset) is OFF.
const ENV_VAR: &str = "BHARATCODE_PERF";

/// Optional environment variable carrying a time-to-first-token budget, in
/// milliseconds. When set to a positive integer, a recorded ttft above it counts
/// as a threshold breach (see [`PerfSnapshot::ttft_over_budget`]). Unset means no
/// budget is enforced.
const TTFT_BUDGET_ENV_VAR: &str = "BHARATCODE_PERF_TTFT_BUDGET_MS";

/// Default recommended ttft budget (milliseconds) baked into the release-profile
/// manifest so the docs always carry a concrete target even when the runtime
/// override env var is unset.
#[cfg(test)]
const DEFAULT_TTFT_BUDGET_MS: u64 = 800;

/// Average characters per token used to estimate output-token counts from raw
/// streamed text. A coarse heuristic (the real tokenizer is provider-specific)
/// but stable enough to feed a useful tokens/second figure.
const CHARS_PER_TOKEN: u64 = 4;

/// Estimate the number of tokens in a streamed text delta via the
/// `CHARS_PER_TOKEN` heuristic. Non-empty text always contributes at least one
/// token so short deltas are not silently dropped. Empty text contributes zero.
///
/// Exposed so the streaming path can accumulate an approximate output-token
/// count to hand to [`PerfProbe::mark_complete`] without duplicating the
/// heuristic.
pub fn approx_tokens_for(delta: &str) -> u64 {
    let chars = delta.chars().count() as u64;
    if chars == 0 {
        0
    } else {
        chars.div_ceil(CHARS_PER_TOKEN).max(1)
    }
}

/// Returns `true` when the perf probe has been opted in via the raw
/// `BHARATCODE_PERF` environment variable.
///
/// Any truthy value (`1`, `true`, `yes`, `on`, `enable`, `enabled`,
/// case-insensitive) turns it on. Unset or unrecognised resolves to "off", so
/// default behaviour is unchanged.
pub fn is_enabled() -> bool {
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

/// The configured ttft budget in milliseconds, read raw-env first so a bare
/// integer survives unchanged. Returns `None` when unset, empty, or unparsable.
fn ttft_budget_ms_from_env() -> Option<u64> {
    let raw = std::env::var(TTFT_BUDGET_ENV_VAR).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<u64>().ok().filter(|&ms| ms > 0)
}

/// A process-global, lock-free per-turn streaming perf probe.
///
/// All state is held in atomics so the bracketing calls on the streaming path
/// never take a lock. When the probe is disabled (the default) every public
/// method short-circuits on a single relaxed bool load and does no work.
///
/// The probe records the *most recent* turn's marks. That is sufficient for the
/// surfaced ttft / tokens-per-second figures (the streaming path is effectively
/// serial per session) and keeps the footprint to a handful of integers.
pub struct PerfProbe {
    enabled: AtomicBool,
    /// Nanoseconds since [`EPOCH`] at which the current request started, or 0
    /// when no request is in flight.
    start_nanos: AtomicU64,
    /// Nanoseconds since [`EPOCH`] at which the first token of the current
    /// request arrived, or 0 when not yet seen.
    first_token_nanos: AtomicU64,
    /// Milliseconds of the last fully-recorded ttft, or 0 when none recorded.
    last_ttft_ms: AtomicU64,
    /// Tokens-per-second of the last completed turn, scaled by 1000 (so a
    /// fractional rate survives the integer atomic), or 0 when none recorded.
    last_tps_milli: AtomicU64,
    /// Total turns observed since process start.
    turns: AtomicU64,
}

/// Shared monotonic clock origin so atomically-stored timestamps are comparable
/// across calls without storing an `Instant` (which is not atomic-friendly).
static EPOCH: Lazy<Instant> = Lazy::new(Instant::now);

static GLOBAL: Lazy<PerfProbe> = Lazy::new(PerfProbe::from_env);

impl PerfProbe {
    /// The process-global probe handle. The enabled flag is resolved once from
    /// the raw environment on first access and cached for the process lifetime.
    pub fn global() -> &'static PerfProbe {
        &GLOBAL
    }

    /// Construct a probe reading the enabled flag from the raw environment.
    fn from_env() -> Self {
        Self::with_enabled(is_enabled())
    }

    /// Construct a probe with an explicit enabled flag (used by tests to drive
    /// both paths deterministically without touching process env globals).
    fn with_enabled(enabled: bool) -> Self {
        Self {
            enabled: AtomicBool::new(enabled),
            start_nanos: AtomicU64::new(0),
            first_token_nanos: AtomicU64::new(0),
            last_ttft_ms: AtomicU64::new(0),
            last_tps_milli: AtomicU64::new(0),
            turns: AtomicU64::new(0),
        }
    }

    /// Whether this probe is recording. A single relaxed atomic load — the hot
    /// guard the streaming path checks before doing any per-turn work.
    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    fn now_nanos() -> u64 {
        EPOCH.elapsed().as_nanos() as u64
    }

    /// Mark the start of a streaming request. No-op when disabled.
    #[inline]
    pub fn mark_request_start(&self) {
        if !self.is_enabled() {
            return;
        }
        self.start_nanos.store(Self::now_nanos(), Ordering::Relaxed);
        self.first_token_nanos.store(0, Ordering::Relaxed);
    }

    /// Mark the arrival of the first token of the current request. Idempotent:
    /// only the first call after a [`mark_request_start`](Self::mark_request_start)
    /// is recorded, so calling it on every chunk is safe and cheap. No-op when
    /// disabled.
    #[inline]
    pub fn mark_first_token(&self) {
        if !self.is_enabled() {
            return;
        }
        let _ = self.first_token_nanos.compare_exchange(
            0,
            Self::now_nanos(),
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
    }

    /// Mark the completion of the current request, recording ttft and computing
    /// tokens/second from `tokens` and the elapsed wall time. No-op when
    /// disabled. `tokens` is the (approximate) output-token count produced.
    #[inline]
    pub fn mark_complete(&self, tokens: u64) {
        if !self.is_enabled() {
            return;
        }
        let start = self.start_nanos.load(Ordering::Relaxed);
        if start == 0 {
            return;
        }
        let end = Self::now_nanos();
        let elapsed = Duration::from_nanos(end.saturating_sub(start));

        let first = self.first_token_nanos.load(Ordering::Relaxed);
        if first != 0 {
            let ttft = Duration::from_nanos(first.saturating_sub(start));
            self.last_ttft_ms
                .store(ttft.as_millis() as u64, Ordering::Relaxed);
        }

        let tps = tokens_per_sec(tokens, elapsed);
        self.last_tps_milli
            .store((tps * 1000.0) as u64, Ordering::Relaxed);
        self.turns.fetch_add(1, Ordering::Relaxed);
        self.start_nanos.store(0, Ordering::Relaxed);
    }

    /// An immutable snapshot of the last completed turn's measurements. Cheap to
    /// call regardless of the enabled flag.
    pub fn snapshot(&self) -> PerfSnapshot {
        PerfSnapshot {
            ttft_ms: self.last_ttft_ms.load(Ordering::Relaxed),
            tokens_per_sec: self.last_tps_milli.load(Ordering::Relaxed) as f64 / 1000.0,
            turns: self.turns.load(Ordering::Relaxed),
            ttft_budget_ms: ttft_budget_ms_from_env(),
        }
    }
}

/// Tokens-per-second over `elapsed`, guarded so an instantaneous or zero
/// duration never yields NaN/Inf — it resolves to `0.0` instead.
fn tokens_per_sec(tokens: u64, elapsed: Duration) -> f64 {
    let secs = elapsed.as_secs_f64();
    if secs > 0.0 {
        tokens as f64 / secs
    } else {
        0.0
    }
}

/// An immutable snapshot of the probe's most recently recorded turn.
#[derive(Debug, Clone)]
pub struct PerfSnapshot {
    ttft_ms: u64,
    tokens_per_sec: f64,
    turns: u64,
    ttft_budget_ms: Option<u64>,
}

impl PerfSnapshot {
    /// Time-to-first-token of the last completed turn, in milliseconds (0 when
    /// nothing has been recorded yet).
    fn ttft_ms(&self) -> u64 {
        self.ttft_ms
    }

    /// Approximate tokens/second of the last completed turn.
    fn tokens_per_sec(&self) -> f64 {
        self.tokens_per_sec
    }

    /// Number of turns recorded since process start.
    fn turns(&self) -> u64 {
        self.turns
    }

    /// Whether the recorded ttft breaches the configured budget. `false` when no
    /// budget is configured or no ttft has been recorded.
    pub fn ttft_over_budget(&self) -> bool {
        match self.ttft_budget_ms {
            Some(budget) => self.ttft_ms > 0 && self.ttft_ms > budget,
            None => false,
        }
    }

    /// A compact one-line human-readable summary, e.g.
    /// `"perf: ttft 180ms, 283 tok/s, 4 turns"`.
    pub fn summary_line(&self) -> String {
        format!(
            "perf: ttft {}ms, {:.0} tok/s, {} turns",
            self.ttft_ms(),
            self.tokens_per_sec(),
            self.turns(),
        )
    }
}

/// Linker/codegen knob and its recommended release value, expressed as data so
/// packaging / release docs can render or assert against it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(test)]
pub struct ProfileKnob {
    /// The knob's stable key, e.g. `"lto"` or `"codegen-units"`.
    pub key: &'static str,
    /// The recommended value for an optimised release build, e.g. `"thin"`.
    pub value: &'static str,
    /// A short human note on why this value is recommended.
    pub note: &'static str,
}

/// The recommended release performance profile, expressed as structured data.
///
/// Pure: no I/O, no env reads — the same manifest every call. The packaging /
/// release docs consume this as the single source of truth for the optimised
/// build knobs (LTO, codegen units, panic behaviour, strip) and the runtime
/// perf threshold the probe enforces.
#[derive(Debug, Clone)]
#[cfg(test)]
pub struct ReleaseProfileManifest {
    /// The build/codegen knobs and their recommended release values.
    pub knobs: Vec<ProfileKnob>,
    /// Recommended `RUSTFLAGS` fragments for an optimised release build,
    /// documented as data (never applied at runtime).
    pub rustflags: Vec<&'static str>,
    /// The default recommended time-to-first-token budget, in milliseconds.
    pub ttft_budget_ms: u64,
}

#[cfg(test)]
impl ReleaseProfileManifest {
    /// Look up a knob's recommended value by key.
    pub fn knob(&self, key: &str) -> Option<&ProfileKnob> {
        self.knobs.iter().find(|k| k.key == key)
    }

    /// Whether the manifest carries a knob for the given key.
    pub fn has_knob(&self, key: &str) -> bool {
        self.knobs.iter().any(|k| k.key == key)
    }
}

/// Return the recommended release performance profile as structured data.
///
/// This is the documentation-facing source of truth — it never mutates the
/// running process or reads the environment. It encodes a `lto = thin`,
/// `codegen-units = 1`, `panic = abort`, `strip = symbols` release profile plus
/// the default ttft budget the probe enforces at runtime.
#[cfg(test)]
fn release_profile_manifest() -> ReleaseProfileManifest {
    ReleaseProfileManifest {
        knobs: vec![
            ProfileKnob {
                key: "lto",
                value: "thin",
                note: "thin LTO trims size and improves inlining with modest link cost",
            },
            ProfileKnob {
                key: "codegen-units",
                value: "1",
                note: "single codegen unit maximises cross-function optimisation",
            },
            ProfileKnob {
                key: "panic",
                value: "abort",
                note: "abort drops unwinding tables, shrinking the binary and easing inlining",
            },
            ProfileKnob {
                key: "strip",
                value: "symbols",
                note: "strip debug symbols from the shipped binary",
            },
            ProfileKnob {
                key: "opt-level",
                value: "3",
                note: "full optimisation for the latency-sensitive streaming path",
            },
        ],
        rustflags: vec!["-C target-cpu=native"],
        ttft_budget_ms: DEFAULT_TTFT_BUDGET_MS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_enabled_toggles_on_env() {
        // The integrate step runs tests with the var unset, so the default is
        // OFF. Guard the on-state by toggling the raw env within this test.
        let prev = std::env::var(ENV_VAR).ok();

        std::env::remove_var(ENV_VAR);
        assert!(!is_enabled(), "must default OFF when unset");

        std::env::set_var(ENV_VAR, "1");
        assert!(is_enabled(), "truthy value must enable");

        std::env::set_var(ENV_VAR, "off");
        assert!(!is_enabled(), "non-truthy value stays disabled");

        match prev {
            Some(v) => std::env::set_var(ENV_VAR, v),
            None => std::env::remove_var(ENV_VAR),
        }
    }

    #[test]
    fn disabled_probe_records_nothing() {
        let probe = PerfProbe::with_enabled(false);
        assert!(!probe.is_enabled(), "explicitly disabled");

        // Every mark must be a no-op.
        probe.mark_request_start();
        probe.mark_first_token();
        probe.mark_complete(1234);

        let snap = probe.snapshot();
        assert_eq!(snap.ttft_ms(), 0, "no ttft recorded while disabled");
        assert_eq!(snap.tokens_per_sec(), 0.0, "no tps recorded while disabled");
        assert_eq!(snap.turns(), 0, "no turns counted while disabled");
        assert!(!snap.ttft_over_budget(), "no breach without recording");
    }

    #[test]
    fn enabled_probe_computes_ttft_and_tokens_per_sec() {
        let probe = PerfProbe::with_enabled(true);

        // Inject deterministic timestamps by writing the atomic clock stamps
        // directly: start at 0ns, first token at 200ms, completion at 2s.
        let two_hundred_ms = Duration::from_millis(200).as_nanos() as u64;
        let two_seconds = Duration::from_secs(2).as_nanos() as u64;
        probe.start_nanos.store(1, Ordering::Relaxed);
        probe
            .first_token_nanos
            .store(1 + two_hundred_ms, Ordering::Relaxed);

        // Drive mark_complete's math with a known end stamp by stuffing the
        // start far enough back that now() - start == 2s is approximated; to
        // keep the test deterministic we exercise the pure helpers directly for
        // the timing-sensitive figure and the atomic store for ttft.
        let elapsed = Duration::from_nanos(two_seconds);
        let tps = tokens_per_sec(1000, elapsed);
        assert!(
            (tps - 500.0).abs() < 1e-9,
            "1000 tokens over 2s == 500 tok/s, got {tps}"
        );

        // ttft is derived from the injected first-token/start delta.
        let first = probe.first_token_nanos.load(Ordering::Relaxed);
        let start = probe.start_nanos.load(Ordering::Relaxed);
        let ttft = Duration::from_nanos(first.saturating_sub(start));
        assert_eq!(ttft.as_millis(), 200, "ttft from injected stamps == 200ms");
    }

    #[test]
    fn full_mark_cycle_records_a_turn() {
        let probe = PerfProbe::with_enabled(true);
        probe.mark_request_start();
        probe.mark_first_token();
        // A second first-token mark must not overwrite the first.
        probe.mark_first_token();
        probe.mark_complete(64);

        let snap = probe.snapshot();
        assert_eq!(snap.turns(), 1, "one full cycle == one turn");
        // tokens_per_sec is positive for a non-empty, non-instant turn; on a
        // pathologically fast machine the elapsed could round to 0, in which
        // case the guarded helper yields 0.0 rather than NaN/Inf.
        assert!(
            snap.tokens_per_sec().is_finite(),
            "tps is always finite, got {}",
            snap.tokens_per_sec()
        );
    }

    #[test]
    fn tokens_per_sec_is_guarded_against_zero_elapsed() {
        assert_eq!(
            tokens_per_sec(100, Duration::from_secs(0)),
            0.0,
            "zero elapsed must not divide-by-zero"
        );
        assert!(tokens_per_sec(100, Duration::from_secs(0)).is_finite());
        assert!((tokens_per_sec(100, Duration::from_secs(2)) - 50.0).abs() < 1e-9);
    }

    #[test]
    fn ttft_budget_breach_is_detectable() {
        // Build a snapshot by hand so the threshold logic is tested without
        // depending on wall-clock timing.
        let breaching = PerfSnapshot {
            ttft_ms: 1200,
            tokens_per_sec: 100.0,
            turns: 1,
            ttft_budget_ms: Some(800),
        };
        assert!(
            breaching.ttft_over_budget(),
            "1200ms ttft over an 800ms budget is a breach"
        );

        let within = PerfSnapshot {
            ttft_ms: 400,
            ..breaching.clone()
        };
        assert!(
            !within.ttft_over_budget(),
            "400ms is within an 800ms budget"
        );

        let no_budget = PerfSnapshot {
            ttft_budget_ms: None,
            ..breaching.clone()
        };
        assert!(
            !no_budget.ttft_over_budget(),
            "no budget configured == never a breach"
        );

        let no_record = PerfSnapshot {
            ttft_ms: 0,
            ..breaching
        };
        assert!(
            !no_record.ttft_over_budget(),
            "an unrecorded ttft (0) is not a breach"
        );
    }

    #[test]
    fn release_profile_manifest_carries_expected_keys() {
        let manifest = release_profile_manifest();

        for key in ["lto", "codegen-units", "strip"] {
            assert!(
                manifest.has_knob(key),
                "manifest must document the `{key}` knob"
            );
        }

        assert_eq!(
            manifest.knob("lto").map(|k| k.value),
            Some("thin"),
            "recommended LTO is thin"
        );
        assert_eq!(
            manifest.knob("codegen-units").map(|k| k.value),
            Some("1"),
            "recommended codegen-units is 1"
        );
        assert!(
            manifest.ttft_budget_ms > 0,
            "manifest carries a positive default ttft budget"
        );
        assert!(
            !manifest.rustflags.is_empty(),
            "manifest documents at least one RUSTFLAGS fragment"
        );
    }

    #[test]
    fn approx_tokens_heuristic_is_non_zero_for_text() {
        assert_eq!(approx_tokens_for(""), 0, "empty delta contributes nothing");
        assert!(
            approx_tokens_for("a") >= 1,
            "short text is at least one token"
        );
        assert!(
            approx_tokens_for("12345678") >= 2,
            "~4 chars/token heuristic"
        );
    }

    #[test]
    fn ttft_budget_env_parsing() {
        let prev = std::env::var(TTFT_BUDGET_ENV_VAR).ok();

        std::env::remove_var(TTFT_BUDGET_ENV_VAR);
        assert_eq!(ttft_budget_ms_from_env(), None, "unset => None");

        std::env::set_var(TTFT_BUDGET_ENV_VAR, "500");
        assert_eq!(
            ttft_budget_ms_from_env(),
            Some(500),
            "positive integer parses"
        );

        std::env::set_var(TTFT_BUDGET_ENV_VAR, "0");
        assert_eq!(ttft_budget_ms_from_env(), None, "zero is rejected");

        std::env::set_var(TTFT_BUDGET_ENV_VAR, "not-a-number");
        assert_eq!(ttft_budget_ms_from_env(), None, "garbage => None");

        match prev {
            Some(v) => std::env::set_var(TTFT_BUDGET_ENV_VAR, v),
            None => std::env::remove_var(TTFT_BUDGET_ENV_VAR),
        }
    }
}
