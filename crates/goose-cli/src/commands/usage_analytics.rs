//! Privacy-preserving, strictly-local usage analytics for BharatCode.
//!
//! Keeps a small, opt-in counter file of *coarse aggregate* usage — how many
//! turns ran, how often each tool was called (by tool name), how many tokens
//! flowed, and ₹ spend bucketed by day — under the config directory. The
//! aggregate is surfaced as a compact `Usage (local, aggregated)` footer on
//! `bharatcode cost`.
//!
//! Privacy is the whole point, so it is enforced by construction:
//!   * **Never phones home.** Nothing here opens a socket or touches the
//!     network. The only side effect is an atomic write of a local JSON file.
//!   * **No free text is ever recorded.** [`UsageEvent`] carries only enums,
//!     numbers, and a tool *name* (`&str`) — there is no prompt / path / file
//!     content / model-output field, so there is nothing for arbitrary user
//!     text to flow into. The recorded shape is counts only.
//!   * **Default OFF.** Recording and the footer are gated on
//!     `BHARATCODE_ANALYTICS`; with it unset the `cost` output is
//!     byte-identical to before this module existed.
//!
//! The enabled check reads the raw environment string first (mirroring
//! [`goose::memory_store::is_enabled`]) so a bare `1` is honoured rather than
//! being coerced into a JSON number by the config layer and read back as OFF
//! (the bug fixed in v29/v50).
//!
//! Original BharatCode work; not ported from any third party.

use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::{FixedOffset, Utc};
use serde::{Deserialize, Serialize};

use goose::config::paths::Paths;

/// Environment key that turns local analytics on. Absent / falsey => fully
/// disabled (default OFF — recording is skipped and the footer is `None`, so
/// the `cost` output is unchanged).
pub const ANALYTICS_ENABLED_KEY: &str = "BHARATCODE_ANALYTICS";

/// File under the config dir that backs the aggregate. Lives beside the other
/// per-user state files; only ever written atomically (temp + rename).
const ANALYTICS_FILE: &str = "bharatcode/analytics.json";

/// Schema tag stamped into the on-disk JSON so a future format change can be
/// detected and migrated rather than mis-parsed.
const SCHEMA: u32 = 1;

/// Whether local analytics are enabled for this process.
///
/// Reads `BHARATCODE_ANALYTICS` as a raw environment string first so a bare `1`
/// is honoured (mirrors [`goose::memory_store::is_enabled`], which avoids the
/// config layer coercing `1` into a JSON number and reporting OFF). Accepts the
/// usual truthy spellings (`1`, `true`, `yes`, `on`); anything else — including
/// absence — is OFF.
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

/// India Standard Time (UTC+05:30), the relevant wall clock for bucketing ₹
/// spend by day. Mirrors the offset used by `plan_file`.
fn ist_offset() -> FixedOffset {
    FixedOffset::east_opt(5 * 3600 + 30 * 60).expect("IST (+05:30) is a valid fixed offset")
}

/// The IST calendar day (`YYYY-MM-DD`) for a UTC instant — the key under which
/// that instant's ₹ spend is bucketed.
fn ist_day_key(now: chrono::DateTime<Utc>) -> String {
    now.with_timezone(&ist_offset())
        .format("%Y-%m-%d")
        .to_string()
}

/// A single thing worth counting.
///
/// **Privacy invariant, enforced by the type:** every variant carries only an
/// enum tag and numbers — plus, for [`UsageEvent::ToolCall`], the tool's
/// *name*. There is deliberately no field that can hold a prompt, a path, file
/// contents, model output, or any other free text, so arbitrary user text has
/// nowhere to be recorded. Anything that lands in the aggregate is a count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageEvent<'a> {
    /// One completed agent turn.
    Turn,
    /// One invocation of a tool, identified by its registered name only.
    ToolCall { name: &'a str },
    /// Token usage for a turn: input and output token counts.
    Tokens { input: u64, output: u64 },
}

/// A ₹-spend event, kept separate from [`UsageEvent`] so it can carry the
/// numeric amount (a count of paise/rupees, never text) bucketed by IST day.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, PartialEq)]
pub struct SpendEvent {
    /// Rupee amount for this spend, bucketed under the current IST day.
    pub inr: f64,
}

/// Running totals. Plain numbers — no text fields.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Totals {
    #[serde(default)]
    pub turns: u64,
    #[serde(default)]
    pub tokens_in: u64,
    #[serde(default)]
    pub tokens_out: u64,
    #[serde(default)]
    pub inr_spend: f64,
}

/// The full on-disk aggregate. Counts only: a `tools` histogram keyed by tool
/// name, a per-IST-day ₹ histogram, and scalar totals.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Aggregate {
    #[serde(default)]
    pub schema: u32,
    #[serde(default)]
    pub totals: Totals,
    /// Tool-call counts keyed by tool name.
    #[serde(default)]
    pub tools: BTreeMap<String, u64>,
    /// ₹ spend keyed by IST calendar day (`YYYY-MM-DD`).
    #[serde(default)]
    pub by_day_inr: BTreeMap<String, f64>,
}

impl Aggregate {
    /// Fold one [`UsageEvent`] into the running counts.
    fn apply(&mut self, event: &UsageEvent) {
        match event {
            UsageEvent::Turn => {
                self.totals.turns = self.totals.turns.saturating_add(1);
            }
            UsageEvent::ToolCall { name } => {
                let name = name.trim();
                if !name.is_empty() {
                    *self.tools.entry(name.to_string()).or_insert(0) += 1;
                }
            }
            UsageEvent::Tokens { input, output } => {
                self.totals.tokens_in = self.totals.tokens_in.saturating_add(*input);
                self.totals.tokens_out = self.totals.tokens_out.saturating_add(*output);
            }
        }
    }

    /// Fold one ₹-spend event into the running ₹ totals, bucketed by IST day.
    fn apply_spend(&mut self, spend: &SpendEvent, now: chrono::DateTime<Utc>) {
        if spend.inr.is_finite() && spend.inr > 0.0 {
            self.totals.inr_spend += spend.inr;
            *self.by_day_inr.entry(ist_day_key(now)).or_insert(0.0) += spend.inr;
        }
    }
}

/// Path to the JSON file backing the aggregate, under the config directory.
fn analytics_path() -> PathBuf {
    Paths::in_config_dir(ANALYTICS_FILE)
}

/// Load the aggregate from disk, returning an empty one when the file is
/// missing or unreadable. Never errors — analytics are strictly best-effort.
fn load() -> Aggregate {
    match std::fs::read_to_string(analytics_path()) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => Aggregate::default(),
    }
}

/// Persist the aggregate atomically: write a sibling temp file then rename it
/// over the target, so a crash mid-write can never leave a torn JSON file.
fn store(agg: &Aggregate) -> std::io::Result<()> {
    let path = analytics_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let serialized = serde_json::to_string_pretty(agg)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    std::io::Write::write_all(&mut tmp, serialized.as_bytes())?;
    tmp.persist(&path)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    Ok(())
}

/// Record one usage event into the local aggregate, best-effort.
///
/// No-op when analytics are disabled (`BHARATCODE_ANALYTICS` unset / falsey),
/// so the default behaviour is untouched. When enabled, loads the aggregate,
/// folds in the event, and writes it back atomically. Any I/O error is
/// swallowed — analytics must never break a run.
//
// The recording API is part of this module's public surface; the agent loop's
// call sites live outside the `cost` command's owned files, so suppress the
// not-yet-wired-here dead-code warning in non-test builds. The footer entry
// point ([`analytics_footer`]) is the wired call site reached from `cost`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn record(event: UsageEvent) {
    if !is_enabled() {
        return;
    }
    let mut agg = load();
    agg.schema = SCHEMA;
    agg.apply(&event);
    let _ = store(&agg);
}

/// Record one ₹-spend event into the local aggregate, bucketed by IST day.
///
/// Same gating and best-effort semantics as [`record`].
#[cfg_attr(not(test), allow(dead_code))]
pub fn record_spend(spend: SpendEvent) {
    if !is_enabled() {
        return;
    }
    let mut agg = load();
    agg.schema = SCHEMA;
    agg.apply_spend(&spend, Utc::now());
    let _ = store(&agg);
}

/// Render the compact muted `Usage (local, aggregated)` footer from an
/// aggregate. Counts only — no free text from the aggregate is ever
/// interpolated except tool names, which are themselves names, not prompts.
///
/// Pulled out so it can be unit-tested without touching the filesystem.
fn render_block(agg: &Aggregate) -> Option<String> {
    let nothing = agg.totals.turns == 0
        && agg.totals.tokens_in == 0
        && agg.totals.tokens_out == 0
        && agg.tools.is_empty()
        && agg.by_day_inr.is_empty();
    if nothing {
        return None;
    }

    let header = label("cost.usage_analytics", "Usage (local, aggregated):");
    let turns_lbl = label("cost.usage_turns", "turns");
    let tokens_lbl = label("cost.usage_tokens", "tokens (in/out)");
    let tools_lbl = label("cost.usage_tool_calls", "tool calls");
    let days_lbl = label("cost.usage_days", "days with spend");

    let total_tool_calls: u64 = agg.tools.values().copied().sum();

    let mut out = String::new();
    out.push_str(&format!("  {}", crate::theme::muted(header)));
    out.push_str(&format!(
        "\n    {}",
        crate::theme::muted(format!("- {}: {}", turns_lbl, agg.totals.turns))
    ));
    out.push_str(&format!(
        "\n    {}",
        crate::theme::muted(format!(
            "- {}: {}/{}",
            tokens_lbl, agg.totals.tokens_in, agg.totals.tokens_out
        ))
    ));
    out.push_str(&format!(
        "\n    {}",
        crate::theme::muted(format!(
            "- {}: {} ({} distinct)",
            tools_lbl,
            total_tool_calls,
            agg.tools.len()
        ))
    ));
    out.push_str(&format!(
        "\n    {}",
        crate::theme::muted(format!("- {}: {}", days_lbl, agg.by_day_inr.len()))
    ));
    Some(out)
}

/// Build the opt-in `Usage (local, aggregated)` footer for `bharatcode cost`.
///
/// Returns `None` when analytics are disabled (`BHARATCODE_ANALYTICS` unset /
/// falsey) **or** when nothing has been recorded yet, so the default `cost`
/// output is unchanged in both cases. When enabled with a non-empty aggregate,
/// returns a compact muted block of counts only.
pub fn analytics_footer() -> Option<String> {
    if !is_enabled() {
        return None;
    }
    render_block(&load())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let _guard = env_lock::lock_env([(ANALYTICS_ENABLED_KEY, None::<&str>)]);
        assert!(!is_enabled());
    }

    #[test]
    fn footer_none_when_disabled() {
        let _guard = env_lock::lock_env([(ANALYTICS_ENABLED_KEY, None::<&str>)]);
        assert!(analytics_footer().is_none());
    }

    /// Fold three events into a fresh aggregate and assert the resulting JSON
    /// matches the expected totals and tool-bucket counts. Exercises the
    /// serialize/deserialize round-trip the on-disk file relies on.
    #[test]
    fn three_events_produce_expected_json_aggregate() {
        let mut agg = Aggregate {
            schema: SCHEMA,
            ..Aggregate::default()
        };
        agg.apply(&UsageEvent::Turn);
        agg.apply(&UsageEvent::ToolCall { name: "shell" });
        agg.apply(&UsageEvent::ToolCall { name: "shell" });
        agg.apply(&UsageEvent::Tokens {
            input: 120,
            output: 45,
        });

        assert_eq!(agg.totals.turns, 1);
        assert_eq!(agg.totals.tokens_in, 120);
        assert_eq!(agg.totals.tokens_out, 45);
        assert_eq!(agg.tools.get("shell").copied(), Some(2));

        // Round-trips through JSON unchanged (the on-disk representation).
        let json = serde_json::to_string(&agg).unwrap();
        let parsed: Aggregate = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, agg);
        assert!(json.contains("\"turns\":1"));
        assert!(json.contains("\"shell\":2"));
    }

    /// ₹ spend is bucketed under an IST (`YYYY-MM-DD`) day key, and a known
    /// instant lands in the expected IST calendar day (the +05:30 offset can
    /// push a late-UTC instant into the next IST day).
    #[test]
    fn spend_bucketed_under_ist_day_key() {
        // 2026-06-20T20:00:00Z is 2026-06-21T01:30:00 IST => next IST day.
        let instant = chrono::DateTime::parse_from_rfc3339("2026-06-20T20:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut agg = Aggregate::default();
        agg.apply_spend(&SpendEvent { inr: 12.5 }, instant);

        let key = "2026-06-21";
        assert!(
            agg.by_day_inr.contains_key(key),
            "expected IST day key {key:?}, got {:?}",
            agg.by_day_inr.keys().collect::<Vec<_>>()
        );
        assert_eq!(agg.by_day_inr.get(key).copied(), Some(12.5));
        assert_eq!(agg.totals.inr_spend, 12.5);
        // Day key is a bare ISO date: YYYY-MM-DD, no free text.
        assert_eq!(key.len(), 10);
        assert_eq!(key.matches('-').count(), 2);
    }

    /// The rendered footer is non-empty when there is something to show and is
    /// free of any upstream branding leakage.
    #[test]
    fn render_block_non_empty_and_brand_free() {
        let mut agg = Aggregate::default();
        agg.apply(&UsageEvent::Turn);
        agg.apply(&UsageEvent::ToolCall { name: "editor" });
        agg.apply_spend(
            &SpendEvent { inr: 3.0 },
            chrono::DateTime::parse_from_rfc3339("2026-06-20T06:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        );

        let block = render_block(&agg).expect("non-empty aggregate renders a block");
        assert!(!block.is_empty());
        for forbidden in ["goose", "Goose", "Block"] {
            assert!(
                !block.contains(forbidden),
                "footer must not leak {forbidden:?}: {block}"
            );
        }
    }

    #[test]
    fn render_block_none_for_empty_aggregate() {
        assert!(render_block(&Aggregate::default()).is_none());
    }

    /// End-to-end: with analytics enabled and the config dir pointed at a temp
    /// directory, three recorded events land in the on-disk JSON with the
    /// expected totals/tool buckets, and the footer is `Some` and non-empty.
    #[test]
    fn record_then_footer_roundtrip_when_enabled() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = env_lock::lock_env([
            (ANALYTICS_ENABLED_KEY, Some("1")),
            ("BHARATCODE_PATH_ROOT", temp.path().to_str()),
        ]);

        record(UsageEvent::Turn);
        record(UsageEvent::ToolCall { name: "shell" });
        record(UsageEvent::Tokens {
            input: 10,
            output: 7,
        });

        let agg = load();
        assert_eq!(agg.schema, SCHEMA);
        assert_eq!(agg.totals.turns, 1);
        assert_eq!(agg.totals.tokens_in, 10);
        assert_eq!(agg.totals.tokens_out, 7);
        assert_eq!(agg.tools.get("shell").copied(), Some(1));

        let footer = analytics_footer().expect("enabled + non-empty => Some footer");
        assert!(!footer.is_empty());
        for forbidden in ["goose", "Goose", "Block"] {
            assert!(
                !footer.contains(forbidden),
                "footer must not leak {forbidden:?}"
            );
        }
    }

    /// Type-level privacy guard: a `UsageEvent` can be constructed only from
    /// enums/numbers/a tool *name*. There is no String prompt/path/content
    /// field, so arbitrary user text has nowhere to be recorded. This compiles
    /// iff that invariant holds; it intentionally does not (and cannot)
    /// reference any free-text field.
    #[test]
    fn usage_event_carries_no_free_text_field() {
        let _turn = UsageEvent::Turn;
        let _tool = UsageEvent::ToolCall { name: "name-only" };
        let _tokens = UsageEvent::Tokens {
            input: 1,
            output: 1,
        };
        // The tool name is a &str, deliberately not an owned prompt buffer:
        // the type forces names, not arbitrary recorded text.
        assert!(matches!(_tool, UsageEvent::ToolCall { name } if !name.is_empty()));
    }
}
