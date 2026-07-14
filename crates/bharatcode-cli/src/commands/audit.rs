//! DPDP audit log for BharatCode.
//!
//! India's Digital Personal Data Protection Act (DPDP) expects organisations to
//! be able to show *what* automated processing happened, *when*, and *at whose
//! cost*. For a coding agent that means: which provider/model was called, at
//! what time, how many tokens flowed, and what it cost in ₹. This module keeps a
//! **local, append-only** record of exactly that — one JSON object per model
//! turn — so a team can demonstrate an auditable trail without shipping any
//! prompt content off the machine.
//!
//! Design:
//!   * **Default OFF.** The log is only written when `BHARATCODE_AUDIT` is on
//!     (env var or config key). With it unset, behaviour is unchanged.
//!   * **Append-only JSONL** under the config dir (`audit.jsonl`), one record
//!     per turn. We never read-modify-write the file, so an interrupted write
//!     can at worst drop the trailing record, never corrupt earlier history.
//!   * **No prompt content.** Only metadata (provider, model, timestamp,
//!     token counts, ₹ cost, session id) is recorded — never message text.
//!   * **Best-effort.** [`record_turn`] never returns an error to the turn
//!     loop; a failure to audit must not break a user's session.
//!
//! The matching viewer ([`handle_audit`]) renders the most recent records with
//! IST timestamps and a ₹ roll-up.
//!
//! Original BharatCode work; not ported from any third party.

use std::fs::OpenOptions;
use std::io::{BufRead, Write};
use std::path::PathBuf;

use bharatcode_core::config::paths::Paths;
use bharatcode_core::config::Config;
use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};

use crate::commands::cost_ledger::{format_inr, format_inr_compact, usd_to_inr};

/// Config / environment key that turns the audit log on. Absent / falsey =>
/// fully disabled (default OFF — behaviour unchanged).
pub const AUDIT_ENABLED_KEY: &str = "BHARATCODE_AUDIT";

/// File name (under the config dir) holding the append-only JSONL audit log.
pub const AUDIT_LOG_FILE: &str = "audit.jsonl";

/// India Standard Time (UTC+05:30). DPDP is an Indian statute, so audit
/// timestamps are surfaced against the IST wall clock rather than UTC.
fn ist_offset() -> FixedOffset {
    FixedOffset::east_opt(5 * 3600 + 30 * 60).expect("IST (+05:30) is a valid fixed offset")
}

/// Whether the audit log is enabled for this process.
///
/// Reads `BHARATCODE_AUDIT` through the config layer (so it honours either the
/// environment variable or the config file). Accepts the usual truthy spellings
/// (`1`, `true`, `yes`, `on`); anything else — including absence — is OFF.
pub fn is_enabled() -> bool {
    // Read the raw environment string first. Going through the config layer's
    // typed `get_param::<String>` would fail to deserialize a value like `1`
    // (which the config layer coerces to a JSON number), reporting the flag as
    // OFF even when explicitly set. Reading the raw string sidesteps that.
    if let Ok(raw) = std::env::var(AUDIT_ENABLED_KEY) {
        return is_truthy(&raw);
    }
    match Config::global().get_param::<String>(AUDIT_ENABLED_KEY) {
        Ok(v) => is_truthy(&v),
        Err(_) => false,
    }
}

fn is_truthy(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Absolute path of the append-only audit log.
pub fn audit_log_path() -> PathBuf {
    Paths::in_config_dir(AUDIT_LOG_FILE)
}

/// One audited model turn. Serialized as a single JSON line (JSONL).
///
/// Deliberately metadata-only: no prompt or completion text is ever stored, so
/// the log can be retained and inspected without exposing personal data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRecord {
    /// RFC3339 timestamp in UTC (sortable, unambiguous).
    pub ts_utc: String,
    /// Same instant rendered in IST (`YYYY-MM-DD HH:MM:SS`) for human reading.
    pub ts_ist: String,
    /// Provider that served the turn (e.g. `anthropic`, `openai`).
    pub provider: String,
    /// Model that served the turn.
    pub model: String,
    /// Session id the turn belonged to.
    pub session_id: String,
    /// Input (prompt) tokens for this turn, if reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<i32>,
    /// Output (completion) tokens for this turn, if reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<i32>,
    /// Total tokens for this turn, if reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<i32>,
    /// Session-cumulative spend at the time of this turn, in INR.
    pub cost_inr: f64,
}

impl AuditRecord {
    /// Build a record for the current instant.
    pub fn now(
        provider: impl Into<String>,
        model: impl Into<String>,
        session_id: impl Into<String>,
        input_tokens: Option<i32>,
        output_tokens: Option<i32>,
        total_tokens: Option<i32>,
        cost_inr: f64,
    ) -> Self {
        let now: DateTime<Utc> = Utc::now();
        Self {
            ts_utc: now.to_rfc3339(),
            ts_ist: now
                .with_timezone(&ist_offset())
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
            provider: provider.into(),
            model: model.into(),
            session_id: session_id.into(),
            input_tokens,
            output_tokens,
            total_tokens,
            cost_inr,
        }
    }
}

/// Append a single audit record to the log (best-effort).
///
/// No-op when the audit log is disabled. Any IO error is swallowed (logged at
/// `warn`) so an auditing failure never breaks the user's turn — auditing is a
/// side channel, not part of the critical path.
pub fn record(record: &AuditRecord) {
    if !is_enabled() {
        return;
    }
    if let Err(e) = append_record(record) {
        tracing::warn!("DPDP audit: failed to append record: {e}");
    }
}

/// Convenience wrapper used by the session turn loop.
///
/// Converts the recorded session-cumulative USD cost into INR and appends a
/// record. Off by default and fully best-effort; safe to call unconditionally.
#[allow(clippy::too_many_arguments)]
pub fn record_turn(
    provider: &str,
    model: &str,
    session_id: &str,
    input_tokens: Option<i32>,
    output_tokens: Option<i32>,
    total_tokens: Option<i32>,
    session_cost_usd: f64,
) {
    if !is_enabled() {
        return;
    }
    let cost_inr = if session_cost_usd.is_finite() && session_cost_usd > 0.0 {
        usd_to_inr(session_cost_usd)
    } else {
        0.0
    };
    let rec = AuditRecord::now(
        provider,
        model,
        session_id,
        input_tokens,
        output_tokens,
        total_tokens,
        cost_inr,
    );
    record(&rec);
}

fn append_record(record: &AuditRecord) -> anyhow::Result<()> {
    let path = audit_log_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let line = serde_json::to_string(record)?;
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    writeln!(file, "{line}")?;
    Ok(())
}

/// Read all audit records, oldest first. Malformed lines are skipped so a single
/// bad line can never make the whole log unreadable. Returns an empty vec when
/// the log does not exist yet.
pub fn read_records() -> anyhow::Result<Vec<AuditRecord>> {
    let path = audit_log_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = std::fs::File::open(&path)?;
    let reader = std::io::BufReader::new(file);
    let mut records = Vec::new();
    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(rec) = serde_json::from_str::<AuditRecord>(trimmed) {
            records.push(rec);
        }
    }
    Ok(records)
}

/// Options for the `audit` viewer subcommand.
pub struct AuditOptions {
    /// Show every record instead of just the most recent.
    pub all: bool,
    /// Maximum number of records to list when not showing all.
    pub limit: usize,
}

impl Default for AuditOptions {
    fn default() -> Self {
        Self {
            all: false,
            limit: 20,
        }
    }
}

/// Look up a user-facing label through the i18n layer, falling back to the
/// English `default` when the active locale table has no entry for `key`.
///
/// Mirrors the pattern in [`crate::commands::cost`]: `tr!` echoes the key back
/// when it is missing, so an unchanged key is treated as "untranslated" and the
/// English default is used. This keeps English output stable while leaving room
/// for Hindi (and other locales) to add the keys later.
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// Entry point for `bharatcode audit`: render the local DPDP audit log.
pub async fn handle_audit(opts: AuditOptions) -> anyhow::Result<()> {
    let path = audit_log_path();
    let records = read_records()?;

    println!();
    println!(
        "  {}",
        crate::theme::heading(label("audit.title", "BharatCode DPDP audit log"))
    );
    println!(
        "  {}",
        crate::theme::muted(
            label("audit.path", "Log: {path}").replace("{path}", &path.display().to_string())
        )
    );
    if !is_enabled() {
        println!(
            "  {}",
            crate::theme::muted(label(
                "audit.disabled",
                "Auditing is OFF. Set BHARATCODE_AUDIT=1 to record future turns.",
            ))
        );
    }
    println!();

    if records.is_empty() {
        println!(
            "  {}",
            crate::theme::muted(label(
                "audit.empty",
                "No audit records yet. Enable BHARATCODE_AUDIT and run a turn.",
            ))
        );
        return Ok(());
    }

    let total_inr: f64 = records.last().map(|r| r.cost_inr).unwrap_or(0.0);
    let total_tokens: i64 = records
        .iter()
        .filter_map(|r| r.total_tokens.map(i64::from))
        .sum();

    println!(
        "  {}",
        label(
            "audit.summary",
            "{count} records · {tokens} tokens · {inr} (latest session)"
        )
        .replace("{count}", &records.len().to_string())
        .replace("{tokens}", &total_tokens.to_string())
        .replace("{inr}", &format_inr(total_inr))
    );
    println!();

    // Most recent first.
    let mut ordered: Vec<&AuditRecord> = records.iter().collect();
    ordered.reverse();
    let shown: Vec<&AuditRecord> = if opts.all {
        ordered.clone()
    } else {
        ordered.iter().take(opts.limit.max(1)).copied().collect()
    };

    for r in &shown {
        let tokens = r
            .total_tokens
            .map(|t| t.to_string())
            .unwrap_or_else(|| "-".to_string());
        println!(
            "    {}  {:<10} {:<24} {:>8} tok  {}",
            crate::theme::muted(r.ts_ist.clone()),
            r.provider,
            bharatcode_core::utils::safe_truncate(&r.model, 24),
            tokens,
            crate::theme::success(format_inr_compact(r.cost_inr)),
        );
    }

    if !opts.all && ordered.len() > shown.len() {
        println!(
            "    {}",
            crate::theme::muted(
                label(
                    "audit.more",
                    "... {count} older (use --all to show every record)"
                )
                .replace("{count}", &(ordered.len() - shown.len()).to_string())
            )
        );
    }

    Ok(())
}

/// Compact end-of-session pointer to the audit log, shown only when auditing is
/// enabled. This is the reachable surface for the viewer in the running binary
/// (the full `bharatcode audit` report lives in [`handle_audit`]): it tells the
/// user where the log is and how many records this session left behind, without
/// flooding the terminal. No-op (silent) when auditing is OFF.
pub fn print_session_summary(session_id: &str) {
    if !is_enabled() {
        return;
    }
    let records = match read_records() {
        Ok(r) => r,
        Err(_) => return,
    };
    let count = records
        .iter()
        .filter(|r| r.session_id == session_id)
        .count();
    if count == 0 {
        return;
    }
    let latest_inr = records
        .iter()
        .rev()
        .find(|r| r.session_id == session_id)
        .map(|r| r.cost_inr)
        .unwrap_or(0.0);
    println!(
        "  {}",
        crate::theme::muted(
            label(
                "audit.session_summary",
                "DPDP audit: {count} turns logged ({inr}) → run 'bharatcode audit' to view ({path})",
            )
            .replace("{count}", &count.to_string())
            .replace("{inr}", &format_inr_compact(latest_inr))
            .replace("{path}", &audit_log_path().display().to_string())
        )
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_serializes_to_single_jsonl_line() {
        let rec = AuditRecord::now(
            "anthropic",
            "claude-x",
            "sess-1",
            Some(100),
            Some(50),
            Some(150),
            12.5,
        );
        let line = serde_json::to_string(&rec).unwrap();
        assert!(!line.contains('\n'));
        let back: AuditRecord = serde_json::from_str(&line).unwrap();
        assert_eq!(back.provider, "anthropic");
        assert_eq!(back.model, "claude-x");
        assert_eq!(back.total_tokens, Some(150));
        assert!((back.cost_inr - 12.5).abs() < 1e-9);
    }

    #[test]
    fn ist_timestamp_is_five_thirty_ahead() {
        // 2026-06-19T20:00:00Z is 2026-06-20 01:30:00 IST.
        let utc: DateTime<Utc> = "2026-06-19T20:00:00Z".parse().unwrap();
        let ist = utc.with_timezone(&ist_offset());
        assert_eq!(
            ist.format("%Y-%m-%d %H:%M:%S").to_string(),
            "2026-06-20 01:30:00"
        );
    }

    #[test]
    fn round_trip_through_a_temp_log() {
        let dir = std::env::temp_dir().join(format!("bc-audit-test-{}", std::process::id()));
        // Hold the shared workspace env lock so neither BHARATCODE_PATH_ROOT nor
        // BHARATCODE_AUDIT can be changed by a concurrent test mid-round-trip.
        let _guard = env_lock::lock_env([
            ("BHARATCODE_PATH_ROOT", dir.to_str()),
            ("BHARATCODE_AUDIT", Some("1")),
        ]);

        let path = audit_log_path();
        let _ = std::fs::remove_file(&path);

        let rec = AuditRecord::now(
            "openai",
            "gpt-x",
            "sess-9",
            Some(10),
            Some(5),
            Some(15),
            3.0,
        );
        record(&rec);
        record(&rec);

        let read = read_records().unwrap();
        assert_eq!(read.len(), 2);
        assert_eq!(read[0].provider, "openai");
        assert_eq!(read[1].total_tokens, Some(15));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn disabled_record_is_a_noop() {
        let dir = std::env::temp_dir().join(format!("bc-audit-off-{}", std::process::id()));
        let _guard = env_lock::lock_env([
            ("BHARATCODE_PATH_ROOT", dir.to_str()),
            ("BHARATCODE_AUDIT", None),
        ]);

        let path = audit_log_path();
        let _ = std::fs::remove_file(&path);

        record_turn("p", "m", "s", Some(1), Some(1), Some(2), 1.0);
        assert!(!path.exists());
    }
}
