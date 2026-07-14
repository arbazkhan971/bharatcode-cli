//! Privacy-preserving, strictly-local usage analytics for BharatCode (v92).
//!
//! Maintains a single rolling JSON *aggregate* of coarse, monotonic usage
//! counters — how many turns ran, how many tool calls were made, how many
//! sessions were seen, how many tokens flowed in total, and how many distinct
//! calendar days the user was active — under the config directory at
//! `<config_dir>/bharatcode/usage_aggregate.json`. The aggregate is surfaced as
//! exactly ONE footer line on `bharatcode cost`.
//!
//! Privacy is the whole point, so it is enforced by construction:
//!   * **Never phones home.** Nothing here opens a socket or touches the
//!     network. The only side effect is an atomic write of a local JSON file
//!     (sibling temp file + rename, so a crash mid-write cannot tear the JSON).
//!   * **Counts only — no per-event detail.** The on-disk [`UsageAggregate`]
//!     carries only `u64` counters and a set of bare ISO `YYYY-MM-DD` day
//!     strings. There is deliberately no field that can hold a prompt, a path,
//!     file contents, model output, or any other free text, so arbitrary user
//!     text has nowhere to be recorded.
//!   * **Default OFF.** Recording and the footer are gated on
//!     `BHARATCODE_ANALYTICS_LOCAL`; with it unset the `cost` output is
//!     byte-identical to before this module existed and there is zero I/O.
//!
//! The enabled check reads the raw environment string first (mirroring the
//! sibling footers) so a bare `1` is honoured rather than being coerced into a
//! JSON number by the config layer and read back as OFF.
//!
//! Original BharatCode work; not ported from any third party.

use std::collections::BTreeSet;
use std::path::PathBuf;

use chrono::{FixedOffset, Utc};
use serde::{Deserialize, Serialize};

use bharatcode_core::config::paths::Paths;

/// Environment key that turns local usage analytics on. Absent / falsey =>
/// fully disabled (default OFF — recording is a no-op and the footer is `None`,
/// so the `cost` output is unchanged and no file is touched).
pub const ANALYTICS_LOCAL_ENABLED_KEY: &str = "BHARATCODE_ANALYTICS_LOCAL";

/// File under the config dir that backs the rolling aggregate. Lives beside the
/// other per-user state files; only ever written atomically (temp + rename).
const AGGREGATE_FILE: &str = "bharatcode/usage_aggregate.json";

/// Schema tag stamped into the on-disk JSON so a future format change can be
/// detected and migrated rather than mis-parsed.
const SCHEMA: u32 = 1;

/// Whether local usage analytics are enabled for this process.
///
/// Reads `BHARATCODE_ANALYTICS_LOCAL` as a raw environment string first so a
/// bare `1` is honoured (the config layer would otherwise coerce `1` into a
/// JSON number and report OFF). Accepts the usual truthy spellings (`1`,
/// `true`, `yes`, `on`); anything else — including absence — is OFF.
pub fn is_enabled() -> bool {
    match std::env::var(ANALYTICS_LOCAL_ENABLED_KEY) {
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

/// Look up a user-facing label through the i18n layer, falling back to the
/// English `default` when the active locale table has no entry for `key`.
///
/// `tr!` echoes the key back when it is missing, so an unchanged key is treated
/// as "untranslated" and the English default is used.
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// India Standard Time (UTC+05:30) — the wall clock used to attribute an
/// instant to a calendar day for the "days active" counter.
fn ist_offset() -> FixedOffset {
    FixedOffset::east_opt(5 * 3600 + 30 * 60).expect("IST (+05:30) is a valid fixed offset")
}

/// The IST calendar day (`YYYY-MM-DD`) for a UTC instant — the key recorded in
/// the active-days set. A bare ISO date, never free text.
fn ist_day_key(now: chrono::DateTime<Utc>) -> String {
    now.with_timezone(&ist_offset())
        .format("%Y-%m-%d")
        .to_string()
}

/// The full on-disk aggregate. Monotonic counters only, plus a set of bare ISO
/// day strings for the "days active" count.
///
/// **Privacy invariant, enforced by the type:** every field is a `u64` counter,
/// a `u32` schema tag, or a `BTreeSet<String>` of `YYYY-MM-DD` dates. There is
/// no field that can hold a prompt, path, file content, or model output, so
/// arbitrary user text has nowhere to be recorded.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageAggregate {
    #[serde(default)]
    pub schema: u32,
    /// Completed agent turns.
    #[serde(default)]
    pub turns: u64,
    /// Tool invocations (count only — not which tools).
    #[serde(default)]
    pub tool_calls: u64,
    /// Sessions observed.
    #[serde(default)]
    pub sessions: u64,
    /// Total tokens (input + output) seen across all turns.
    #[serde(default)]
    pub total_tokens: u64,
    /// Distinct IST calendar days (`YYYY-MM-DD`) on which any activity occurred.
    #[serde(default)]
    pub active_days: BTreeSet<String>,
}

/// One monotonic counter to bump. Each variant carries only a small `u64`
/// payload (or nothing) — there is no free-text field, by construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Counter {
    /// One completed agent turn.
    Turn,
    /// `n` tool invocations.
    ToolCalls(u64),
    /// One session observed.
    Session,
    /// `n` tokens (input + output) seen.
    Tokens(u64),
}

impl UsageAggregate {
    /// Fold one [`Counter`] into the running totals and mark the current IST
    /// day active. All increments saturate, so the counters are strictly
    /// monotonic and can never wrap.
    fn bump_counter(&mut self, counter: Counter, now: chrono::DateTime<Utc>) {
        match counter {
            Counter::Turn => self.turns = self.turns.saturating_add(1),
            Counter::ToolCalls(n) => self.tool_calls = self.tool_calls.saturating_add(n),
            Counter::Session => self.sessions = self.sessions.saturating_add(1),
            Counter::Tokens(n) => self.total_tokens = self.total_tokens.saturating_add(n),
        }
        self.active_days.insert(ist_day_key(now));
    }

    /// Number of distinct calendar days with recorded activity.
    fn days_active(&self) -> usize {
        self.active_days.len()
    }

    /// True when nothing has been recorded yet — used to suppress an empty
    /// footer so an enabled-but-unused install prints nothing.
    fn is_empty(&self) -> bool {
        self.turns == 0
            && self.tool_calls == 0
            && self.sessions == 0
            && self.total_tokens == 0
            && self.active_days.is_empty()
    }
}

/// Path to the JSON file backing the aggregate, under the config directory.
fn aggregate_path() -> PathBuf {
    Paths::in_config_dir(AGGREGATE_FILE)
}

/// Load the aggregate from disk, returning a zeroed one when the file is
/// missing, unreadable, or corrupt. Never errors and never panics — analytics
/// are strictly best-effort, so a torn or hand-edited file simply reads as
/// empty rather than taking down `bharatcode cost`.
fn load() -> UsageAggregate {
    match std::fs::read_to_string(aggregate_path()) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => UsageAggregate::default(),
    }
}

/// Persist the aggregate atomically: write a sibling temp file then rename it
/// over the target, so a crash mid-write can never leave a torn JSON file.
fn store(agg: &UsageAggregate) -> std::io::Result<()> {
    let path = aggregate_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let serialized = serde_json::to_string_pretty(agg)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    std::io::Write::write_all(&mut tmp, serialized.as_bytes())?;
    tmp.persist(&path).map_err(std::io::Error::other)?;
    Ok(())
}

/// Bump one counter in the local aggregate, best-effort.
///
/// No-op when analytics are disabled (`BHARATCODE_ANALYTICS_LOCAL` unset /
/// falsey), so the default behaviour — and the default I/O profile (none) — is
/// untouched. When enabled, loads the aggregate, folds in the counter, marks the
/// current day active, and writes it back atomically. Any I/O error is swallowed
/// — analytics must never break a run.
//
pub fn bump(counter: Counter) {
    if !is_enabled() {
        return;
    }
    let mut agg = load();
    agg.schema = SCHEMA;
    agg.bump_counter(counter, Utc::now());
    let _ = store(&agg);
}

/// Record one completed agent turn — the most common bump, given its own helper
/// so call sites read cleanly. Same gating and best-effort semantics as
/// [`bump`].
pub fn record_turn() {
    bump(Counter::Turn);
}

/// Render the single muted footer line from an aggregate. Counts only — no free
/// text from the aggregate is ever interpolated; only its `u64` totals and the
/// `usize` day count appear.
///
/// Pulled out so it can be unit-tested without touching the filesystem. Returns
/// `None` for an empty aggregate so an enabled-but-unused install prints
/// nothing.
fn render_line(agg: &UsageAggregate) -> Option<String> {
    if agg.is_empty() {
        return None;
    }

    let header = label("cost.usage_local", "Usage (local, aggregated)");
    let turns_lbl = label("cost.usage_local_turns", "turns");
    let tools_lbl = label("cost.usage_local_tools", "tool calls");
    let sessions_lbl = label("cost.usage_local_sessions", "sessions");
    let tokens_lbl = label("cost.usage_local_tokens", "tokens");
    let days_lbl = label("cost.usage_local_days", "days active");

    Some(format!(
        "{header}: {turns} {turns_lbl}, {tools} {tools_lbl}, {sessions} {sessions_lbl}, {tokens} {tokens_lbl}, {days} {days_lbl}",
        turns = agg.turns,
        tools = agg.tool_calls,
        sessions = agg.sessions,
        tokens = agg.total_tokens,
        days = agg.days_active(),
    ))
}

/// Build the opt-in single-line usage footer for `bharatcode cost`.
///
/// Returns `None` when analytics are disabled (`BHARATCODE_ANALYTICS_LOCAL`
/// unset / falsey) **or** when nothing has been recorded yet, so the default
/// `cost` output is unchanged in both cases. When enabled with a non-empty
/// aggregate, returns exactly one line of counts.
pub fn usage_footer() -> Option<String> {
    if !is_enabled() {
        return None;
    }
    render_line(&load())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instant(rfc3339: &str) -> chrono::DateTime<Utc> {
        chrono::DateTime::parse_from_rfc3339(rfc3339)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn is_truthy_accepts_common_spellings() {
        for v in ["1", "true", "TRUE", "yes", "on", "  On  "] {
            assert!(is_truthy(v), "expected {v:?} to be truthy");
        }
        for v in ["", "0", "false", "no", "off", "nope"] {
            assert!(!is_truthy(v), "expected {v:?} to be falsey");
        }
    }

    #[test]
    fn is_enabled_false_when_env_unset() {
        let _guard = env_lock::lock_env([(ANALYTICS_LOCAL_ENABLED_KEY, None::<&str>)]);
        assert!(!is_enabled());
    }

    #[test]
    fn footer_none_when_disabled() {
        let _guard = env_lock::lock_env([(ANALYTICS_LOCAL_ENABLED_KEY, None::<&str>)]);
        assert!(usage_footer().is_none());
    }

    #[test]
    fn render_line_none_for_empty_aggregate() {
        assert!(render_line(&UsageAggregate::default()).is_none());
    }

    /// The rendered footer is a single line that surfaces both the turns and
    /// tokens counts and leaks no upstream branding.
    #[test]
    fn footer_is_single_line_with_turns_and_tokens_and_brand_free() {
        let mut agg = UsageAggregate::default();
        agg.bump_counter(Counter::Turn, instant("2026-06-20T06:00:00Z"));
        agg.bump_counter(Counter::ToolCalls(3), instant("2026-06-20T06:00:00Z"));
        agg.bump_counter(Counter::Tokens(165), instant("2026-06-20T06:00:00Z"));

        let line = render_line(&agg).expect("non-empty aggregate renders a line");

        assert_eq!(line.lines().count(), 1, "footer must be one line: {line}");
        assert!(
            line.contains('1') && line.contains("turns"),
            "turns: {line}"
        );
        assert!(
            line.contains("165") && line.contains("tokens"),
            "tokens: {line}"
        );
        for forbidden in ["goose", "Goose", "Block"] {
            assert!(
                !line.contains(forbidden),
                "footer must not leak {forbidden:?}: {line}"
            );
        }
    }

    /// Increments saturate and the active-days set tracks distinct calendar
    /// days (the +05:30 offset can push a late-UTC instant into the next IST
    /// day).
    #[test]
    fn bump_counter_accumulates_and_tracks_distinct_days() {
        let mut agg = UsageAggregate::default();
        // Two instants on the same IST day, one on the next.
        agg.bump_counter(Counter::Turn, instant("2026-06-20T06:00:00Z"));
        agg.bump_counter(Counter::Tokens(100), instant("2026-06-20T07:00:00Z"));
        // 2026-06-20T20:00:00Z is 2026-06-21T01:30:00 IST => next IST day.
        agg.bump_counter(Counter::Tokens(50), instant("2026-06-20T20:00:00Z"));

        assert_eq!(agg.turns, 1);
        assert_eq!(agg.total_tokens, 150);
        assert_eq!(agg.days_active(), 2, "two distinct IST days expected");
        assert!(agg.active_days.contains("2026-06-20"));
        assert!(agg.active_days.contains("2026-06-21"));
        // Day keys are bare ISO dates: YYYY-MM-DD, no free text.
        for day in &agg.active_days {
            assert_eq!(day.len(), 10);
            assert_eq!(day.matches('-').count(), 2);
        }
    }

    /// A corrupt aggregate file loads as a zeroed aggregate and never panics.
    #[test]
    fn corrupt_aggregate_loads_as_zeroed() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = env_lock::lock_env([
            (ANALYTICS_LOCAL_ENABLED_KEY, Some("1")),
            ("BHARATCODE_PATH_ROOT", temp.path().to_str()),
        ]);

        let path = aggregate_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ this is not valid json ]]]").unwrap();

        let agg = load();
        assert_eq!(agg, UsageAggregate::default());
        // The disabled-by-content path still yields no footer when the file is
        // corrupt (loads empty => render_line None).
        assert!(usage_footer().is_none());
    }

    /// End-to-end: with analytics enabled and the config dir pointed at a temp
    /// directory, `bump`/`record_turn` increment and round-trip through the
    /// on-disk JSON, and the footer is `Some`, a single line, and brand-free.
    #[test]
    fn bump_increments_and_roundtrips_through_temp_json() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = env_lock::lock_env([
            (ANALYTICS_LOCAL_ENABLED_KEY, Some("1")),
            ("BHARATCODE_PATH_ROOT", temp.path().to_str()),
        ]);

        record_turn();
        bump(Counter::ToolCalls(2));
        bump(Counter::Session);
        bump(Counter::Tokens(42));

        // Re-read from disk to prove the round-trip through the JSON file.
        let agg = load();
        assert_eq!(agg.schema, SCHEMA);
        assert_eq!(agg.turns, 1);
        assert_eq!(agg.tool_calls, 2);
        assert_eq!(agg.sessions, 1);
        assert_eq!(agg.total_tokens, 42);
        assert!(agg.days_active() >= 1);

        // The on-disk file actually exists and parses back identically.
        let raw = std::fs::read_to_string(aggregate_path()).expect("aggregate file written");
        let parsed: UsageAggregate = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed, agg);

        let footer = usage_footer().expect("enabled + non-empty => Some footer");
        assert_eq!(footer.lines().count(), 1);
        assert!(footer.contains("turns"));
        assert!(footer.contains("tokens"));
        for forbidden in ["goose", "Goose", "Block"] {
            assert!(
                !footer.contains(forbidden),
                "footer must not leak {forbidden:?}"
            );
        }
    }
}
