//! Opt-in, **local-only** aggregated usage counters (privacy-preserving).
//!
//! This module keeps a tiny rolling tally of *how much* the agent was used —
//! never *what* it was used for. It records two counters, bucketed by the IST
//! (+05:30) calendar day:
//!
//!   * `turns`      — number of completed agent turns, and
//!   * `tool_calls` — number of tool requests issued across those turns.
//!
//! Crucially it records **counts only**. No prompt text, no model output, no
//! file paths, no tool names, no arguments — nothing about *content* — ever
//! reaches this file. The on-disk shape is a flat `{ "by_day": { "YYYY-MM-DD":
//! { "turns": N, "tool_calls": M } }, ... }` JSON object under the config dir.
//!
//! Design (mirrors the recovery sidecar / turn-checkpoint conventions):
//!   * **Default OFF.** Nothing is written and no I/O happens unless
//!     `BHARATCODE_ANALYTICS` is set to a truthy value (`1`/`true`/`yes`/`on`).
//!     With it unset [`record_turn`] is a no-op (zero I/O), so behavior is
//!     byte-identical to before — no file is created.
//!   * **Local only.** The tally lives at
//!     `<config_dir>/bharatcode/usage_analytics.json`. It is never transmitted
//!     anywhere; there is no network code in this module by construction.
//!   * **Atomic, read-modify-write.** Each enabled turn loads the current
//!     tally, increments the counters for today's IST bucket, and writes it
//!     back via a unique temp file + rename so a concurrent reader (or an
//!     interrupted write) never observes a half-written file.
//!   * **Best-effort.** [`record_turn`] never surfaces an error or blocks the
//!     turn; any I/O / parse failure is swallowed (logged at `debug`).
//!   * **IST day buckets.** Days roll over on the Indian wall clock, matching
//!     the recovery/audit/checkpoint conventions so an Indian operator reads a
//!     familiar local calendar.
//!
//! Original BharatCode work; not ported from any third party. std + chrono +
//! serde_json only — no telemetry, no network, no PII.

use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};

use crate::config::paths::Paths;

/// Environment key that turns the local analytics tally on. Absent / falsey =>
/// fully disabled (default OFF — [`record_turn`] is a no-op, no file written).
pub const ANALYTICS_ENABLED_KEY: &str = "BHARATCODE_ANALYTICS";

/// Sub-directory (under the config dir) holding the tally. Shared with the
/// recovery sidecar / checkpoint so the local progress/usage markers live side
/// by side.
const ANALYTICS_SUBDIR: &str = "bharatcode";

/// File name of the single rolling counter file.
const ANALYTICS_FILE: &str = "usage_analytics.json";

/// India Standard Time (UTC+05:30). Day buckets roll over on this wall clock.
fn ist_offset() -> FixedOffset {
    FixedOffset::east_opt(5 * 3600 + 30 * 60).expect("IST (+05:30) is a valid fixed offset")
}

/// Whether the local usage tally is enabled for this process.
///
/// Reads `BHARATCODE_ANALYTICS` straight from the environment and accepts the
/// usual truthy spellings (`1`, `true`, `yes`, `on`); anything else — including
/// absence — is OFF. The raw-env read (rather than the typed config layer)
/// mirrors the recovery sidecar / checkpoint gate.
pub fn is_enabled() -> bool {
    match std::env::var(ANALYTICS_ENABLED_KEY) {
        Ok(raw) => is_truthy(&raw),
        Err(_) => false,
    }
}

fn is_truthy(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Absolute path of the rolling tally
/// (`<config_dir>/bharatcode/usage_analytics.json`).
pub fn analytics_path() -> PathBuf {
    Paths::config_dir()
        .join(ANALYTICS_SUBDIR)
        .join(ANALYTICS_FILE)
}

/// Today's IST calendar day as `YYYY-MM-DD`, the bucket key for new counts.
fn today_ist() -> String {
    let now: DateTime<Utc> = Utc::now();
    now.with_timezone(&ist_offset())
        .format("%Y-%m-%d")
        .to_string()
}

/// Per-day counters. Counts only — never any content.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DayCounters {
    /// Completed agent turns on this IST day.
    #[serde(default)]
    pub turns: u64,
    /// Tool requests issued across those turns on this IST day.
    #[serde(default)]
    pub tool_calls: u64,
}

/// The whole local tally: counts bucketed by IST calendar day.
///
/// `by_day` is a `BTreeMap` so the on-disk JSON is deterministically ordered by
/// date (stable diffs, byte-stable round-trips). A schema tag lets a future
/// format migrate without misreading an old file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageTally {
    /// On-disk schema version of this tally.
    #[serde(default = "default_schema")]
    pub schema: u32,
    /// Counters keyed by IST day (`YYYY-MM-DD`).
    #[serde(default)]
    pub by_day: BTreeMap<String, DayCounters>,
}

fn default_schema() -> u32 {
    1
}

impl Default for UsageTally {
    fn default() -> Self {
        Self {
            schema: default_schema(),
            by_day: BTreeMap::new(),
        }
    }
}

impl UsageTally {
    /// Total turns across every recorded day.
    pub fn total_turns(&self) -> u64 {
        self.by_day.values().map(|d| d.turns).sum()
    }

    /// Total tool calls across every recorded day.
    pub fn total_tool_calls(&self) -> u64 {
        self.by_day.values().map(|d| d.tool_calls).sum()
    }

    /// Apply one completed turn (and its `tool_calls`) to the given IST `day`.
    ///
    /// Pure and day-injected so the increment is trivially testable without a
    /// clock. Counts saturate rather than wrap, so a pathological run can never
    /// silently roll a counter back to zero.
    pub fn apply_turn(&mut self, day: &str, tool_calls: u64) {
        let entry = self.by_day.entry(day.to_string()).or_default();
        entry.turns = entry.turns.saturating_add(1);
        entry.tool_calls = entry.tool_calls.saturating_add(tool_calls);
    }

    /// A one-line, plain-text summary of the whole tally. Carries no
    /// Goose/Block identifiers and, by construction, no content — counts only.
    pub fn summary_line(&self) -> String {
        format!(
            "usage: {} turns · {} tool calls across {} day(s)",
            self.total_turns(),
            self.total_tool_calls(),
            self.by_day.len()
        )
    }
}

/// Record one completed turn into the local tally (best-effort, IST-bucketed).
///
/// No-op (zero I/O, no file created) when analytics is disabled. When enabled,
/// it loads the current tally, increments today's IST bucket by one turn and by
/// `tool_calls` tool requests, and writes it back atomically. `tool_calls` is a
/// **count** the caller derived from this turn — never a tool name or argument.
/// Any I/O / parse failure is swallowed (logged at `debug`) so an analytics
/// failure never breaks or blocks the user's turn.
pub fn record_turn(tool_calls: u64) {
    if !is_enabled() {
        return;
    }
    if let Err(e) = record_turn_at(&today_ist(), tool_calls) {
        tracing::debug!(target: "usage_analytics", error = %e, "failed to record local usage tally");
    }
}

/// Day-injected core of [`record_turn`]: read → increment `day` → write.
/// Separated so tests exercise the load/increment/persist round-trip with a
/// fixed day and no clock dependency.
fn record_turn_at(day: &str, tool_calls: u64) -> std::io::Result<()> {
    let mut tally = load().unwrap_or_default();
    tally.apply_turn(day, tool_calls);
    write_tally(&tally)
}

/// Read the current tally back, or `None` when absent / unreadable / unparsable.
///
/// Reading is allowed regardless of the enable gate so a summary can surface a
/// tally left by a previous (enabled) run. Never errors into the caller.
pub fn load() -> Option<UsageTally> {
    let path = analytics_path();
    let bytes = std::fs::read(&path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// A one-line, plain-text usage summary for display / inspection.
///
/// Returns `None` when no tally exists yet (analytics never enabled, or enabled
/// but no turn recorded), so a caller can choose to stay silent rather than
/// print a zeroed line. Reading is gate-independent (see [`load`]).
pub fn summary() -> Option<String> {
    load().map(|t| t.summary_line())
}

fn write_tally(tally: &UsageTally) -> std::io::Result<()> {
    let path = analytics_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_vec_pretty(tally)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let tmp = path.with_extension(format!(
        "tmp.{}.{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(&tmp, &content)?;
    std::fs::rename(&tmp, &path)
}

// ---------------------------------------------------------------------------
// v92: state-dir aggregated rollup (`AnalyticsStore`)
//
// A second, fully-aggregated view of local usage that lives under the *state*
// dir (`Paths::state_dir()/usage_analytics.json`) rather than the config dir.
// Where the v91 tally above buckets turns/tool-calls by IST day, this rollup is
// a single flat set of coarse lifetime counters plus *bucketed* token and ₹
// totals — by construction it can never hold a prompt, a model output, a file
// path, a tool name, or any other content. Privacy is a structural property of
// the schema, not a runtime filter: there is simply nowhere to put content.
//
// Same gate as everything in this module (`BHARATCODE_ANALYTICS`, default OFF),
// same local-only guarantee (no network code anywhere in this file), and the
// instance id reuses `crate::instance_id` — already a locally-generated UUID
// that is never transmitted.
// ---------------------------------------------------------------------------

/// File name of the state-dir rollup (distinct dir from the v91 day tally).
const ROLLUP_FILE: &str = "usage_analytics.json";

/// Coarse bucket boundaries (inclusive upper bound, in tokens) for the
/// `token_buckets` histogram. A turn's token count is filed into the first
/// bucket whose bound it does not exceed; anything larger lands in the overflow
/// bucket. Only the *bucket counts* are stored, never the exact token figure,
/// so the rollup stays coarse and non-identifying.
const TOKEN_BUCKET_BOUNDS: [u64; 5] = [1_000, 4_000, 16_000, 64_000, 256_000];

/// Coarse bucket boundaries (inclusive upper bound, in whole ₹) for the
/// `cost_inr_buckets` histogram. As with tokens, only bucket counts are kept.
const COST_INR_BUCKET_BOUNDS: [u64; 5] = [1, 5, 25, 100, 500];

/// Index of the bucket `value` falls into for the given ascending `bounds`:
/// the first bucket whose inclusive upper bound it does not exceed, or the
/// overflow bucket (`bounds.len()`) when it exceeds every bound.
fn bucket_index(value: u64, bounds: &[u64]) -> usize {
    bounds
        .iter()
        .position(|&b| value <= b)
        .unwrap_or(bounds.len())
}

/// One unit of usage to fold into the rollup. Counts and magnitudes only — the
/// type deliberately has no field that could carry prompt/output/path content.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageEvent {
    /// Tool invocations attributed to this turn.
    pub tool_invocations: u64,
    /// Total tokens (prompt + completion) for this turn; bucketed, never stored
    /// verbatim.
    pub tokens: u64,
    /// Whole-rupee cost for this turn; bucketed, never stored verbatim.
    pub cost_inr: u64,
}

impl UsageEvent {
    pub fn new(tool_invocations: u64, tokens: u64, cost_inr: u64) -> Self {
        Self {
            tool_invocations,
            tokens,
            cost_inr,
        }
    }
}

/// The on-disk, fully-aggregated rollup. Every field is a count or a histogram
/// of counts — there is no content field anywhere, by construction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalyticsRollup {
    /// On-disk schema version of this rollup.
    #[serde(default = "default_schema")]
    pub schema: u32,
    /// Local, never-transmitted install id (a UUID from `crate::instance_id`).
    /// Stored so a *local* reader can tell two state dirs apart; it is the same
    /// id already persisted on disk and is never sent anywhere.
    #[serde(default)]
    pub instance_id: String,
    /// Completed agent turns recorded into this rollup.
    #[serde(default)]
    pub turns: u64,
    /// Tool invocations summed across those turns.
    #[serde(default)]
    pub tool_invocations: u64,
    /// Histogram of per-turn token magnitudes (counts per bucket; see
    /// [`TOKEN_BUCKET_BOUNDS`]). Length is `bounds + 1` (overflow bucket).
    #[serde(default)]
    pub token_buckets: Vec<u64>,
    /// Histogram of per-turn ₹ cost magnitudes (counts per bucket; see
    /// [`COST_INR_BUCKET_BOUNDS`]).
    #[serde(default)]
    pub cost_inr_buckets: Vec<u64>,
}

impl Default for AnalyticsRollup {
    fn default() -> Self {
        Self {
            schema: default_schema(),
            instance_id: String::new(),
            turns: 0,
            tool_invocations: 0,
            token_buckets: vec![0; TOKEN_BUCKET_BOUNDS.len() + 1],
            cost_inr_buckets: vec![0; COST_INR_BUCKET_BOUNDS.len() + 1],
        }
    }
}

impl AnalyticsRollup {
    fn ensure_bucket_widths(&mut self) {
        if self.token_buckets.len() != TOKEN_BUCKET_BOUNDS.len() + 1 {
            self.token_buckets
                .resize(TOKEN_BUCKET_BOUNDS.len() + 1, 0);
        }
        if self.cost_inr_buckets.len() != COST_INR_BUCKET_BOUNDS.len() + 1 {
            self.cost_inr_buckets
                .resize(COST_INR_BUCKET_BOUNDS.len() + 1, 0);
        }
    }

    /// Fold one [`UsageEvent`] in. Counts saturate rather than wrap so a
    /// pathological run can never silently roll a counter back to zero.
    pub fn apply(&mut self, event: &UsageEvent) {
        self.ensure_bucket_widths();
        self.turns = self.turns.saturating_add(1);
        self.tool_invocations = self.tool_invocations.saturating_add(event.tool_invocations);
        let ti = bucket_index(event.tokens, &TOKEN_BUCKET_BOUNDS);
        self.token_buckets[ti] = self.token_buckets[ti].saturating_add(1);
        let ci = bucket_index(event.cost_inr, &COST_INR_BUCKET_BOUNDS);
        self.cost_inr_buckets[ci] = self.cost_inr_buckets[ci].saturating_add(1);
    }
}

/// Local-only, opt-in aggregated usage rollup persisted under the *state* dir.
///
/// Default OFF: [`AnalyticsStore::record`] is a no-op (zero I/O, no file
/// created) unless `BHARATCODE_ANALYTICS` is truthy. The store only ever holds
/// coarse counts and bucketed magnitudes — no content, no network.
#[derive(Debug, Clone, Default)]
pub struct AnalyticsStore;

impl AnalyticsStore {
    pub fn new() -> Self {
        Self
    }

    /// Absolute path of the state-dir rollup
    /// (`<state_dir>/usage_analytics.json`).
    pub fn path() -> PathBuf {
        Paths::state_dir().join(ROLLUP_FILE)
    }

    /// Read the current rollup back, or `None` when absent / unreadable /
    /// unparsable. Gate-independent so a summary can surface a rollup left by a
    /// previous (enabled) run.
    pub fn load() -> Option<AnalyticsRollup> {
        let bytes = std::fs::read(Self::path()).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Fold one completed turn's aggregated [`UsageEvent`] into the rollup.
    ///
    /// No-op (zero I/O, no file created) while analytics is disabled. When
    /// enabled it loads → applies → writes atomically; any I/O / parse failure
    /// is swallowed (logged at `debug`) so analytics never breaks a turn.
    pub fn record(&self, event: &UsageEvent) {
        if !is_enabled() {
            return;
        }
        if let Err(e) = Self::record_inner(event) {
            tracing::debug!(target: "usage_analytics", error = %e, "failed to record aggregated usage rollup");
        }
    }

    fn record_inner(event: &UsageEvent) -> std::io::Result<()> {
        let mut rollup = Self::load().unwrap_or_default();
        if rollup.instance_id.is_empty() {
            rollup.instance_id = crate::instance_id::get_instance_id().to_string();
        }
        rollup.apply(event);
        Self::write(&rollup)
    }

    fn write(rollup: &AnalyticsRollup) -> std::io::Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_vec_pretty(rollup)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let tmp = path.with_extension(format!(
            "tmp.{}.{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::write(&tmp, &content)?;
        std::fs::rename(&tmp, &path)
    }
}

/// Convenience: fold one aggregated event into the state-dir rollup. No-op when
/// disabled. Equivalent to `AnalyticsStore::new().record(&event)`.
pub fn record(event: &UsageEvent) {
    AnalyticsStore::new().record(event);
}

/// Plain-text summary rows for the aggregated state-dir rollup, suitable for the
/// shared config-summary surface (doctor / info).
///
/// Returns a single muted "default-off" row when no rollup exists yet (analytics
/// never enabled, or enabled but nothing recorded), so the config summary always
/// has exactly one coherent line for this feature. Carries no Goose/Block
/// identifiers and, by construction, no content — counts and buckets only.
/// Gate-independent read (mirrors [`load`]).
pub fn summary_lines() -> Vec<String> {
    match AnalyticsStore::load() {
        None => vec!["Usage analytics: off (local-only, opt-in via BHARATCODE_ANALYTICS)".to_string()],
        Some(r) => {
            let busy_token_buckets = r.token_buckets.iter().filter(|&&c| c > 0).count();
            let busy_cost_buckets = r.cost_inr_buckets.iter().filter(|&&c| c > 0).count();
            vec![format!(
                "Usage analytics (local): {} turns · {} tool invocations · {} token bucket(s) · {} ₹ bucket(s)",
                r.turns, r.tool_invocations, busy_token_buckets, busy_cost_buckets
            )]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Point the config dir at a throwaway root so a test never touches a real
    /// `usage_analytics.json`. `BHARATCODE_PATH_ROOT` is honored by
    /// [`Paths::config_dir`].
    fn with_temp_root(
        keys: impl IntoIterator<Item = (&'static str, Option<&'static str>)>,
    ) -> (env_lock::EnvGuard<'static>, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_str().expect("utf8 path").to_string();
        // Leak the root string so it can live for the 'static env guard; the
        // process is short-lived under test so this is fine.
        let root: &'static str = Box::leak(root.into_boxed_str());
        let mut all: Vec<(&'static str, Option<&'static str>)> =
            vec![("BHARATCODE_PATH_ROOT", Some(root))];
        all.extend(keys);
        let guard = env_lock::lock_env(all);
        (guard, dir)
    }

    #[test]
    fn is_enabled_honors_env() {
        let _guard = env_lock::lock_env([(ANALYTICS_ENABLED_KEY, None::<&str>)]);

        std::env::remove_var(ANALYTICS_ENABLED_KEY);
        assert!(!is_enabled(), "unset must be OFF");

        for v in ["1", "true", "TRUE", "yes", "on", " On "] {
            std::env::set_var(ANALYTICS_ENABLED_KEY, v);
            assert!(is_enabled(), "expected {v:?} to be truthy");
        }
        for v in ["0", "false", "no", "off", ""] {
            std::env::set_var(ANALYTICS_ENABLED_KEY, v);
            assert!(!is_enabled(), "expected {v:?} to be falsey");
        }
        std::env::remove_var(ANALYTICS_ENABLED_KEY);
    }

    #[test]
    fn apply_turn_increments_and_buckets_by_day() {
        let mut tally = UsageTally::default();
        tally.apply_turn("2026-06-20", 3);
        tally.apply_turn("2026-06-20", 1);
        tally.apply_turn("2026-06-21", 0);

        assert_eq!(tally.by_day["2026-06-20"].turns, 2);
        assert_eq!(tally.by_day["2026-06-20"].tool_calls, 4);
        assert_eq!(tally.by_day["2026-06-21"].turns, 1);
        assert_eq!(tally.by_day["2026-06-21"].tool_calls, 0);
        assert_eq!(tally.total_turns(), 3);
        assert_eq!(tally.total_tool_calls(), 4);
    }

    #[test]
    fn tally_round_trips_through_serde() {
        let mut tally = UsageTally::default();
        tally.apply_turn("2026-06-20", 2);
        let json = serde_json::to_string(&tally).expect("serialize");
        // Counts only — the serialized form must never carry content fields.
        assert!(json.contains("turns"));
        assert!(json.contains("tool_calls"));
        let back: UsageTally = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(tally, back);
    }

    #[test]
    fn record_turn_persists_and_summary_reads_back_when_enabled() {
        let (_guard, _dir) = with_temp_root([(ANALYTICS_ENABLED_KEY, Some("1"))]);

        assert!(load().is_none(), "no tally before any recorded turn");
        assert!(summary().is_none(), "summary is None before any turn");

        // Two turns on a fixed day, exercising the read-modify-write round-trip.
        record_turn_at("2026-06-20", 2).expect("first record");
        record_turn_at("2026-06-20", 1).expect("second record");

        let tally = load().expect("tally exists after recording");
        assert_eq!(tally.total_turns(), 2);
        assert_eq!(tally.total_tool_calls(), 3);

        let line = summary().expect("summary after recording");
        assert!(line.contains("2 turns"), "summary line: {line}");
        assert!(line.contains("3 tool calls"), "summary line: {line}");
    }

    #[test]
    fn gate_off_is_a_no_op_with_no_file() {
        let (_guard, _dir) = with_temp_root([(ANALYTICS_ENABLED_KEY, None::<&str>)]);
        std::env::remove_var(ANALYTICS_ENABLED_KEY);

        // The public entry point must write nothing while disabled.
        record_turn(5);

        assert!(
            !analytics_path().exists(),
            "disabled analytics must not create a file"
        );
        assert!(load().is_none(), "disabled run leaves no tally to load");
        assert!(summary().is_none(), "disabled run yields no summary");
    }

    // ---- v92: state-dir aggregated rollup (`AnalyticsStore`) ----

    #[test]
    fn bucket_index_files_into_first_bound_and_overflow() {
        assert_eq!(bucket_index(0, &TOKEN_BUCKET_BOUNDS), 0);
        assert_eq!(bucket_index(1_000, &TOKEN_BUCKET_BOUNDS), 0);
        assert_eq!(bucket_index(1_001, &TOKEN_BUCKET_BOUNDS), 1);
        assert_eq!(
            bucket_index(u64::MAX, &TOKEN_BUCKET_BOUNDS),
            TOKEN_BUCKET_BOUNDS.len()
        );
    }

    #[test]
    fn rollup_record_writes_only_aggregated_counters() {
        let (_guard, _dir) = with_temp_root([(ANALYTICS_ENABLED_KEY, Some("1"))]);

        assert!(AnalyticsStore::load().is_none(), "no rollup before recording");

        let store = AnalyticsStore::new();
        store.record(&UsageEvent::new(3, 1_200, 7));

        let path = AnalyticsStore::path();
        assert!(path.exists(), "enabled record must create the rollup file");

        let raw = std::fs::read_to_string(&path).expect("read rollup");
        // Privacy by construction: aggregated fields present, content absent.
        assert!(raw.contains("turns"));
        assert!(raw.contains("tool_invocations"));
        assert!(raw.contains("token_buckets"));
        assert!(raw.contains("cost_inr_buckets"));
        for forbidden in [
            "prompt",
            "message",
            "content",
            "output",
            "path",
            "tool_name",
            "text",
            "arg",
        ] {
            assert!(
                !raw.to_ascii_lowercase().contains(forbidden),
                "rollup must not carry a {forbidden:?} field: {raw}"
            );
        }
    }

    #[test]
    fn rollup_two_records_accumulate_and_summary_reflects_totals() {
        let (_guard, _dir) = with_temp_root([(ANALYTICS_ENABLED_KEY, Some("1"))]);

        let store = AnalyticsStore::new();
        store.record(&UsageEvent::new(2, 500, 1));
        store.record(&UsageEvent::new(4, 90_000, 250));

        let rollup = AnalyticsStore::load().expect("rollup exists after recording");
        assert_eq!(rollup.turns, 2);
        assert_eq!(rollup.tool_invocations, 6);
        // Token 500 -> bucket 0; 90_000 -> bucket 4; two distinct busy buckets.
        assert_eq!(rollup.token_buckets[0], 1);
        assert_eq!(rollup.token_buckets[4], 1);
        // Cost 1 -> bucket 0; 250 -> bucket 4.
        assert_eq!(rollup.cost_inr_buckets[0], 1);
        assert_eq!(rollup.cost_inr_buckets[4], 1);
        // The local instance id is recorded but is the never-sent crate UUID.
        assert_eq!(rollup.instance_id, crate::instance_id::get_instance_id());

        let lines = summary_lines();
        assert_eq!(lines.len(), 1, "summary is a single rollup row");
        let line = &lines[0];
        assert!(line.contains("2 turns"), "summary line: {line}");
        assert!(line.contains("6 tool invocations"), "summary line: {line}");
    }

    #[test]
    fn rollup_disabled_is_no_op_and_summary_reports_off() {
        let (_guard, _dir) = with_temp_root([(ANALYTICS_ENABLED_KEY, None::<&str>)]);
        std::env::remove_var(ANALYTICS_ENABLED_KEY);

        record(&UsageEvent::new(9, 9_999, 99));

        assert!(
            !AnalyticsStore::path().exists(),
            "disabled rollup must not create a file"
        );
        assert!(AnalyticsStore::load().is_none(), "disabled run leaves no rollup");

        let lines = summary_lines();
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].contains("off"),
            "summary must report off when no rollup: {}",
            lines[0]
        );
    }
}
