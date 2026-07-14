//! Typed streaming/render perf-tuning getters surfaced through the standard
//! config accessor path.
//!
//! The v68 streaming perf feature gives the CLI's streaming render a single
//! validated source of truth for three render tunables:
//!
//! * `BHARATCODE_STREAM_FLUSH_MS` - how often, in milliseconds, the streaming
//!   buffer flushes accumulated safe content to the terminal,
//! * `BHARATCODE_MAX_CODE_BLOCK_LINES` - the maximum number of lines a streamed
//!   code block renders before the tail is truncated to a temp-file pointer,
//! * `BHARATCODE_STREAM_COALESCE_LINES` - how many freshly-completed lines the
//!   renderer coalesces into one paint to cut redraw churn.
//!
//! Every tunable is *optional*. When unset each getter resolves to `None` so
//! callers keep their own documented default and behaviour is unchanged. When
//! set, values are read env-first (so a bare `1` survives the config
//! number-coercion as a string) and clamped to sane bounds so an absurd input
//! never produces a nonsensical interval, line cap, or batch size. The
//! reader/summary helpers are reached from `Config::streaming_perf_summary`
//! (wired in `config/base.rs`), and the scattered `parse_positive_lines` /
//! `max_code_block_lines` env reads in the CLI streaming buffer can call the
//! same typed source instead of ad-hoc parsing.
//!
//! Pure config logic: no I/O beyond the env/config reads the accessor path
//! already performs.

use crate::config::Config;

/// Flush cadence, in milliseconds, for the streaming render buffer.
pub const STREAM_FLUSH_MS_KEY: &str = "BHARATCODE_STREAM_FLUSH_MS";
/// Maximum number of lines a streamed code block renders before truncation.
pub const MAX_CODE_BLOCK_LINES_KEY: &str = "BHARATCODE_MAX_CODE_BLOCK_LINES";
/// Number of completed lines coalesced into a single render paint.
pub const STREAM_COALESCE_LINES_KEY: &str = "BHARATCODE_STREAM_COALESCE_LINES";

/// Documented default flush cadence (ms) when the key is unset. Mirrors the
/// current CLI streaming cadence so behaviour is unchanged until overridden.
pub const DEFAULT_STREAM_FLUSH_MS: u64 = 50;
/// Documented default code-block render cap when the key is unset. Mirrors the
/// CLI streaming buffer's `DEFAULT_MAX_CODE_BLOCK_LINES`.
pub const DEFAULT_MAX_CODE_BLOCK_LINES: usize = 50;
/// Documented default coalesce batch size when the key is unset. `1` means no
/// coalescing - every completed line paints on its own, the current behaviour.
pub const DEFAULT_STREAM_COALESCE_LINES: usize = 1;

/// Lower / upper bounds for the flush cadence. A flush every <1ms is a busy
/// spin and a flush slower than 10s makes the stream feel frozen, so both ends
/// are clamped.
const STREAM_FLUSH_MS_MIN: u64 = 1;
const STREAM_FLUSH_MS_MAX: u64 = 10_000;
/// Lower / upper bounds for the code-block render cap. At least one line so a
/// block never vanishes entirely; capped so a typo'd value cannot push an
/// unbounded amount of code through the renderer.
const MAX_CODE_BLOCK_LINES_MIN: usize = 1;
const MAX_CODE_BLOCK_LINES_MAX: usize = 100_000;
/// Lower / upper bounds for the coalesce batch size. At least one line (no
/// coalescing); capped so a batch never grows so large the screen stalls
/// waiting to paint.
const STREAM_COALESCE_LINES_MIN: usize = 1;
const STREAM_COALESCE_LINES_MAX: usize = 10_000;

/// Every tunable key in registration order. Keeps the summary and the unit test
/// in lock-step with the resolved struct.
#[cfg(test)]
const TUNABLE_KEYS: &[&str] = &[
    STREAM_FLUSH_MS_KEY,
    MAX_CODE_BLOCK_LINES_KEY,
    STREAM_COALESCE_LINES_KEY,
];

/// Resolved snapshot of the streaming/render perf tunables.
///
/// Each field defaults to `None`, meaning "use the caller's documented
/// default". A `Some(_)` value has already been clamped to its sane bounds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamingPerf {
    flush_ms: Option<u64>,
    max_code_block_lines: Option<usize>,
    coalesce_lines: Option<usize>,
}

/// Read a single key as a trimmed string, env-first.
///
/// The raw environment variable is consulted before the merged config file so a
/// bare `1` survives as a string rather than being coerced to a number by the
/// config parser. Returns `None` when the key is absent or resolves to an empty
/// string. Mirrors `agent_caps::read_key` / `resource_limits::read_key`.
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

/// Parse a key as `u64`, clamping to `[min, max]`. A value that does not parse,
/// or parses to `0`, reads as `None` so a malformed tunable falls back to the
/// caller's documented default rather than a nonsensical interval.
fn read_u64_clamped(config: &Config, key: &str, min: u64, max: u64) -> Option<u64> {
    let parsed: u64 = read_key(config, key)?.parse().ok()?;
    if parsed == 0 {
        return None;
    }
    Some(parsed.clamp(min, max))
}

/// Parse a key as `usize`, clamping to `[min, max]`. See [`read_u64_clamped`].
fn read_usize_clamped(config: &Config, key: &str, min: usize, max: usize) -> Option<usize> {
    let parsed: usize = read_key(config, key)?.parse().ok()?;
    if parsed == 0 {
        return None;
    }
    Some(parsed.clamp(min, max))
}

impl StreamingPerf {
    /// Clamped flush cadence in milliseconds, or `None` to use the caller's
    /// documented default. See [`StreamingPerf::effective_flush_ms`].
    pub fn flush_ms(&self) -> Option<u64> {
        self.flush_ms
    }

    /// Clamped code-block render cap, or `None` to use the caller's documented
    /// default. See [`StreamingPerf::effective_max_code_block_lines`].
    pub fn max_code_block_lines(&self) -> Option<usize> {
        self.max_code_block_lines
    }

    /// Clamped coalesce batch size, or `None` to use the caller's documented
    /// default. See [`StreamingPerf::effective_coalesce_lines`].
    pub fn coalesce_lines(&self) -> Option<usize> {
        self.coalesce_lines
    }

    /// Effective flush cadence: the clamped override if set, else the documented
    /// default. This is the value the CLI streaming buffer should actually use.
    pub fn effective_flush_ms(&self) -> u64 {
        self.flush_ms().unwrap_or(DEFAULT_STREAM_FLUSH_MS)
    }

    /// Effective code-block render cap: the clamped override if set, else the
    /// documented default.
    pub fn effective_max_code_block_lines(&self) -> usize {
        self.max_code_block_lines()
            .unwrap_or(DEFAULT_MAX_CODE_BLOCK_LINES)
    }

    /// Effective coalesce batch size: the clamped override if set, else the
    /// documented default.
    pub fn effective_coalesce_lines(&self) -> usize {
        self.coalesce_lines()
            .unwrap_or(DEFAULT_STREAM_COALESCE_LINES)
    }

    /// Whether every tunable is unset (all defaults apply).
    #[cfg(test)]
    fn is_all_default(&self) -> bool {
        self.flush_ms.is_none()
            && self.max_code_block_lines.is_none()
            && self.coalesce_lines.is_none()
    }

    /// One human-readable row per tunable, in registration order, reflecting the
    /// *effective* value (override or documented default) plus whether it came
    /// from the default. Used by `Config::streaming_perf_summary` for doctor/info.
    pub fn summary_lines(&self) -> Vec<String> {
        vec![
            summary_row(
                STREAM_FLUSH_MS_KEY,
                format!("{}ms", self.effective_flush_ms()),
                self.flush_ms().is_none(),
            ),
            summary_row(
                MAX_CODE_BLOCK_LINES_KEY,
                format!("{} lines", self.effective_max_code_block_lines()),
                self.max_code_block_lines().is_none(),
            ),
            summary_row(
                STREAM_COALESCE_LINES_KEY,
                format!("{} lines", self.effective_coalesce_lines()),
                self.coalesce_lines().is_none(),
            ),
        ]
    }
}

/// Render one `KEY = value` row, tagging defaulted tunables so the summary makes
/// clear which values are overrides and which are falling back.
fn summary_row(key: &str, value: String, is_default: bool) -> String {
    if is_default {
        format!("{key} = {value} (default)")
    } else {
        format!("{key} = {value}")
    }
}

/// Resolve the streaming/render perf tunables from a specific config, reading
/// env-first and clamping each value to sane bounds. This is the real call site
/// reached from `Config::streaming_perf_summary`, and the single typed source
/// the CLI streaming buffer can read instead of ad-hoc env parses.
pub fn from_config(config: &Config) -> StreamingPerf {
    StreamingPerf {
        flush_ms: read_u64_clamped(
            config,
            STREAM_FLUSH_MS_KEY,
            STREAM_FLUSH_MS_MIN,
            STREAM_FLUSH_MS_MAX,
        ),
        max_code_block_lines: read_usize_clamped(
            config,
            MAX_CODE_BLOCK_LINES_KEY,
            MAX_CODE_BLOCK_LINES_MIN,
            MAX_CODE_BLOCK_LINES_MAX,
        ),
        coalesce_lines: read_usize_clamped(
            config,
            STREAM_COALESCE_LINES_KEY,
            STREAM_COALESCE_LINES_MIN,
            STREAM_COALESCE_LINES_MAX,
        ),
    }
}

/// Like [`StreamingPerf::summary_lines`] but resolved from a specific `Config`.
/// This is the helper `Config::streaming_perf_summary` delegates to.
pub fn summary_lines_for_config(config: &Config) -> Vec<String> {
    from_config(config).summary_lines()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All three keys cleared, so `from_config(Config::global())` resolves purely
    /// from defaults.
    fn all_keys_unset() -> [(&'static str, Option<&'static str>); 3] {
        [
            (STREAM_FLUSH_MS_KEY, None),
            (MAX_CODE_BLOCK_LINES_KEY, None),
            (STREAM_COALESCE_LINES_KEY, None),
        ]
    }

    #[test]
    fn empty_config_yields_all_none_with_documented_defaults() {
        let _guard = env_lock::lock_env(all_keys_unset());
        let perf = from_config(Config::global());
        assert_eq!(perf, StreamingPerf::default());
        assert!(perf.is_all_default());
        // Raw getters are None when unset...
        assert_eq!(perf.flush_ms(), None);
        assert_eq!(perf.max_code_block_lines(), None);
        assert_eq!(perf.coalesce_lines(), None);
        // ...and the effective values fall back to the documented defaults.
        assert_eq!(perf.effective_flush_ms(), DEFAULT_STREAM_FLUSH_MS);
        assert_eq!(
            perf.effective_max_code_block_lines(),
            DEFAULT_MAX_CODE_BLOCK_LINES
        );
        assert_eq!(
            perf.effective_coalesce_lines(),
            DEFAULT_STREAM_COALESCE_LINES
        );
    }

    #[test]
    fn summary_emits_one_row_per_tunable_reflecting_defaults() {
        let _guard = env_lock::lock_env(all_keys_unset());
        let lines = summary_lines_for_config(Config::global());
        assert_eq!(lines.len(), TUNABLE_KEYS.len());
        for (line, key) in lines.iter().zip(TUNABLE_KEYS) {
            assert!(
                line.starts_with(key),
                "row {line:?} should start with {key}"
            );
            assert!(
                line.contains("(default)"),
                "unset tunable {key} should be tagged as default: {line:?}"
            );
        }
        // The effective default values are visible in the summary.
        assert!(lines[0].contains(&format!("{DEFAULT_STREAM_FLUSH_MS}ms")));
        assert!(lines[1].contains(&format!("{DEFAULT_MAX_CODE_BLOCK_LINES} lines")));
        assert!(lines[2].contains(&format!("{DEFAULT_STREAM_COALESCE_LINES} lines")));
    }

    #[test]
    fn in_range_values_pass_through_unchanged() {
        let keys = [
            (STREAM_FLUSH_MS_KEY, Some("120")),
            (MAX_CODE_BLOCK_LINES_KEY, Some("80")),
            (STREAM_COALESCE_LINES_KEY, Some("4")),
        ];
        let _guard = env_lock::lock_env(keys);
        let perf = from_config(Config::global());
        assert_eq!(perf.flush_ms(), Some(120));
        assert_eq!(perf.max_code_block_lines(), Some(80));
        assert_eq!(perf.coalesce_lines(), Some(4));
        assert!(!perf.is_all_default());
        assert_eq!(perf.effective_flush_ms(), 120);
        assert_eq!(perf.effective_max_code_block_lines(), 80);
        assert_eq!(perf.effective_coalesce_lines(), 4);
    }

    #[test]
    fn summary_reflects_overrides_without_default_tag() {
        let keys = [
            (STREAM_FLUSH_MS_KEY, Some("120")),
            (MAX_CODE_BLOCK_LINES_KEY, Some("80")),
            (STREAM_COALESCE_LINES_KEY, Some("4")),
        ];
        let _guard = env_lock::lock_env(keys);
        let lines = summary_lines_for_config(Config::global());
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("120ms") && !lines[0].contains("(default)"));
        assert!(lines[1].contains("80 lines") && !lines[1].contains("(default)"));
        assert!(lines[2].contains("4 lines") && !lines[2].contains("(default)"));
    }

    #[test]
    fn over_range_values_clamp_to_bounds() {
        let keys = [
            (STREAM_FLUSH_MS_KEY, Some("999999999")),
            (MAX_CODE_BLOCK_LINES_KEY, Some("999999999")),
            (STREAM_COALESCE_LINES_KEY, Some("999999999")),
        ];
        let _guard = env_lock::lock_env(keys);
        let perf = from_config(Config::global());
        assert_eq!(perf.flush_ms(), Some(STREAM_FLUSH_MS_MAX));
        assert_eq!(perf.max_code_block_lines(), Some(MAX_CODE_BLOCK_LINES_MAX));
        assert_eq!(perf.coalesce_lines(), Some(STREAM_COALESCE_LINES_MAX));
    }

    #[test]
    fn garbage_input_falls_back_to_default() {
        let keys = [
            (STREAM_FLUSH_MS_KEY, Some("soon")),
            (MAX_CODE_BLOCK_LINES_KEY, Some("lots")),
            (STREAM_COALESCE_LINES_KEY, Some("3.5")),
        ];
        let _guard = env_lock::lock_env(keys);
        let perf = from_config(Config::global());
        assert_eq!(perf.flush_ms(), None);
        assert_eq!(perf.max_code_block_lines(), None);
        assert_eq!(perf.coalesce_lines(), None);
        assert!(perf.is_all_default());
        // Effective values are the documented defaults.
        assert_eq!(perf.effective_flush_ms(), DEFAULT_STREAM_FLUSH_MS);
        assert_eq!(
            perf.effective_max_code_block_lines(),
            DEFAULT_MAX_CODE_BLOCK_LINES
        );
    }

    #[test]
    fn zero_reads_as_default() {
        let mut keys = all_keys_unset();
        keys[0] = (STREAM_FLUSH_MS_KEY, Some("0"));
        let _guard = env_lock::lock_env(keys);
        let perf = from_config(Config::global());
        assert_eq!(perf.flush_ms(), None);
        assert_eq!(perf.effective_flush_ms(), DEFAULT_STREAM_FLUSH_MS);
    }

    #[test]
    fn bare_one_is_honored_env_first() {
        // A bare numeric env string survives the env-first read rather than being
        // coerced away, and clamps up to the minimum where one applies.
        let mut keys = all_keys_unset();
        keys[1] = (MAX_CODE_BLOCK_LINES_KEY, Some("1"));
        let _guard = env_lock::lock_env(keys);
        let perf = from_config(Config::global());
        assert_eq!(perf.max_code_block_lines(), Some(1));
        assert_eq!(perf.effective_max_code_block_lines(), 1);
    }

    #[test]
    fn below_min_flush_clamps_up() {
        // A sub-minimum but non-zero flush clamps up to the floor rather than
        // becoming a busy spin.
        let mut keys = all_keys_unset();
        keys[0] = (STREAM_FLUSH_MS_KEY, Some("1"));
        let _guard = env_lock::lock_env(keys);
        let perf = from_config(Config::global());
        assert_eq!(perf.flush_ms(), Some(STREAM_FLUSH_MS_MIN));
    }
}
