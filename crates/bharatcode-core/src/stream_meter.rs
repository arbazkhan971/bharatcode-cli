//! Lightweight streaming throughput meter (opt-in observability).
//!
//! Wraps the provider streaming path with a cheap per-turn meter that records
//! time-to-first-token, total stream wall time, chunk count and an approximate
//! token count. The meter always runs (the bookkeeping is a couple of integer
//! adds per chunk), but it only emits a one-line summary when the
//! `BHARATCODE_STREAM_STATS` environment variable is set to a truthy value.
//!
//! The meter never inspects or alters streamed content beyond measuring it, so
//! with the switch off — the default — observable behaviour is unchanged.

use std::time::{Duration, Instant};

/// Environment variable that opts the throughput summary in. Default (unset) is
/// OFF: the meter still ticks cheaply but prints nothing.
const ENV_VAR: &str = "BHARATCODE_STREAM_STATS";

/// Average characters per token used to estimate token counts from raw text.
/// This is a coarse heuristic (the real tokenizer is provider-specific), but it
/// is stable enough to give a useful tokens/second figure for perf observation.
const CHARS_PER_TOKEN: u64 = 4;

/// Returns `true` when the streaming summary has been opted in via the
/// `BHARATCODE_STREAM_STATS` environment variable.
///
/// Any truthy value (`1`, `true`, `yes`, `on`, `enable`, `enabled`,
/// case-insensitive) turns the summary on. Unset or unrecognised resolves to
/// "off", so default behaviour is unchanged.
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

/// Estimate the number of tokens in a text delta via the `CHARS_PER_TOKEN`
/// heuristic. Non-empty text always contributes at least one token so short
/// deltas are not silently dropped from the count.
fn approx_tokens_for(delta: &str) -> u64 {
    let chars = delta.chars().count() as u64;
    if chars == 0 {
        0
    } else {
        chars.div_ceil(CHARS_PER_TOKEN).max(1)
    }
}

/// A per-turn streaming throughput meter.
///
/// Constructed before the provider stream is consumed; [`StreamMeter::tick`] is
/// called for each yielded text delta and [`StreamMeter::finish`] produces the
/// final [`StreamStats`] once the stream ends.
pub struct StreamMeter {
    start: Instant,
    first_token: Option<Instant>,
    chunks: u64,
    approx_tokens: u64,
}

impl StreamMeter {
    /// Start a meter, stamping the stream start time.
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
            first_token: None,
            chunks: 0,
            approx_tokens: 0,
        }
    }

    /// Record one streamed text delta: counts the chunk, accumulates the
    /// approximate token count, and stamps the time-to-first-token exactly once
    /// (on the first tick). The `delta` is only measured, never retained.
    pub fn tick(&mut self, delta: &str) {
        self.first_token.get_or_insert_with(Instant::now);
        self.chunks += 1;
        self.approx_tokens += approx_tokens_for(delta);
    }

    /// Consume the meter and produce the final statistics snapshot.
    pub fn finish(self) -> StreamStats {
        let elapsed = self.start.elapsed();
        let ttft = self.first_token.map(|t| t.duration_since(self.start));
        StreamStats {
            elapsed,
            ttft,
            chunks: self.chunks,
            approx_tokens: self.approx_tokens,
        }
    }
}

/// An immutable snapshot of a completed stream's throughput measurements.
pub struct StreamStats {
    elapsed: Duration,
    ttft: Option<Duration>,
    chunks: u64,
    approx_tokens: u64,
}

impl StreamStats {
    /// Time-to-first-token, or `None` if no delta was ever observed.
    fn ttft(&self) -> Option<Duration> {
        self.ttft
    }

    /// Number of streamed chunks observed.
    fn chunks(&self) -> u64 {
        self.chunks
    }

    /// Approximate token count (heuristic, see `CHARS_PER_TOKEN`).
    fn approx_tokens(&self) -> u64 {
        self.approx_tokens
    }

    /// Approximate throughput in tokens per second over the full stream wall
    /// time. Returns `0.0` (never NaN/Inf) when no measurable time elapsed.
    pub fn tokens_per_sec(&self) -> f64 {
        let secs = self.elapsed.as_secs_f64();
        if secs > 0.0 {
            self.approx_tokens as f64 / secs
        } else {
            0.0
        }
    }

    /// A compact one-line human-readable summary, e.g.
    /// `"stream: 1.2s, ttft 180ms, 24 chunks, ~340 tok, 283 tok/s"`. When no
    /// first token was seen the ttft segment reads `ttft n/a`.
    pub fn summary_line(&self) -> String {
        let ttft = match self.ttft() {
            Some(d) => format!("{}ms", d.as_millis()),
            None => "n/a".to_string(),
        };
        format!(
            "stream: {:.1}s, ttft {}, {} chunks, ~{} tok, {:.0} tok/s",
            self.elapsed.as_secs_f64(),
            ttft,
            self.chunks(),
            self.approx_tokens(),
            self.tokens_per_sec(),
        )
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

        std::env::set_var(ENV_VAR, "false");
        assert!(!is_enabled(), "non-truthy value stays disabled");

        match prev {
            Some(v) => std::env::set_var(ENV_VAR, v),
            None => std::env::remove_var(ENV_VAR),
        }
    }

    #[test]
    fn feeding_chunks_counts_and_measures() {
        let mut meter = StreamMeter::start();
        assert!(
            meter.first_token.is_none(),
            "no first token before any tick"
        );

        let chunks = ["hello ", "world", " from", " the", " stream"];
        for c in chunks {
            meter.tick(c);
        }
        assert!(
            meter.first_token.is_some(),
            "first token stamped after the first tick"
        );

        let stats = meter.finish();
        assert_eq!(stats.chunks(), chunks.len() as u64, "chunks == N");
        assert!(stats.approx_tokens() > 0, "non-empty text yields tokens");
        assert!(stats.ttft().is_some(), "Some(ttft) after the first tick");
        assert!(
            stats.tokens_per_sec() > 0.0,
            "positive tokens/sec for a non-empty stream"
        );

        let line = stats.summary_line();
        assert!(line.contains("tok/s"), "summary mentions tok/s: {line}");
        assert!(
            line.contains("stream:"),
            "summary is the stream line: {line}"
        );
    }

    #[test]
    fn empty_stream_has_no_ttft_and_no_panic() {
        let meter = StreamMeter::start();
        let stats = meter.finish();

        assert!(stats.ttft().is_none(), "empty stream has no first token");
        assert_eq!(stats.chunks(), 0, "no chunks observed");
        assert_eq!(stats.approx_tokens(), 0, "no tokens estimated");
        // Must not divide-by-zero / produce NaN.
        assert_eq!(stats.tokens_per_sec(), 0.0);
        assert!(stats.tokens_per_sec().is_finite());

        // summary_line must render without panicking even on an empty stream.
        let line = stats.summary_line();
        assert!(
            line.contains("ttft n/a"),
            "empty stream reports n/a: {line}"
        );
        assert!(
            line.contains("tok/s"),
            "summary still mentions tok/s: {line}"
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
}
