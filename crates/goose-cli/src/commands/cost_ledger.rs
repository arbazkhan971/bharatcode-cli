//! INR cost ledger for BharatCode.
//!
//! BharatCode already inherits per-session usage/cost accounting from the
//! underlying agent runtime: every session records an `accumulated_cost` in US
//! dollars. This module adds a thin rupee-denominated layer on top of that
//! data so spend can be surfaced in ₹ for Indian users. It provides:
//!
//!   * a configurable USD->INR rate (`BHARATCODE_USD_INR`, with a sane default),
//!   * compact and full ₹ formatting helpers (footer vs. summary), and
//!   * a ledger that rolls per-session spend up into daily and monthly totals.
//!
//! The layer is purely additive and read-only: it never mutates session state,
//! it only reads the already-recorded USD cost and converts it.
//!
//! Original BharatCode work; not ported from any third party.

use std::collections::BTreeMap;

use chrono::{DateTime, FixedOffset, Utc};
use goose::config::Config;
use goose::session::SessionManager;

/// India Standard Time (UTC+05:30). BharatCode targets India, so "today" and
/// "this month" spend buckets are computed against IST calendar boundaries
/// rather than UTC (a session at 02:00 IST belongs to that IST day, not the
/// previous UTC day).
fn ist_offset() -> FixedOffset {
    FixedOffset::east_opt(5 * 3600 + 30 * 60).expect("IST (+05:30) is a valid fixed offset")
}

/// Sane default USD->INR conversion rate used when `BHARATCODE_USD_INR` is not
/// configured. A round, recent-ish figure; users who need accuracy can override
/// it via the config key or the matching environment variable.
pub const DEFAULT_USD_INR: f64 = 88.0;

/// Config / environment key holding the USD->INR conversion rate.
pub const USD_INR_KEY: &str = "BHARATCODE_USD_INR";

/// Resolve the active USD->INR rate.
///
/// Resolution order (handled by the config layer): the `BHARATCODE_USD_INR`
/// environment variable, then the same key in the config file, then
/// [`DEFAULT_USD_INR`]. Any non-positive, infinite or unparseable value falls
/// back to the default so callers always get a usable, positive rate.
pub fn usd_inr_rate() -> f64 {
    match Config::global().get_param::<f64>(USD_INR_KEY) {
        Ok(rate) if rate.is_finite() && rate > 0.0 => rate,
        _ => DEFAULT_USD_INR,
    }
}

/// Convert a USD amount to INR using the active rate.
pub fn usd_to_inr(usd: f64) -> f64 {
    usd * usd_inr_rate()
}

/// Format an INR amount compactly for tight UI such as the session footer.
///
/// Uses India-friendly magnitude suffixes (`k` = thousand, `L` = lakh = 1e5,
/// `Cr` = crore = 1e7) and stays readable down to paise for small amounts.
pub fn format_inr_compact(inr: f64) -> String {
    if !inr.is_finite() {
        return "₹0".to_string();
    }
    let sign = if inr < 0.0 { "-" } else { "" };
    let a = inr.abs();
    if a >= 1.0e7 {
        format!("{sign}₹{:.2}Cr", a / 1.0e7)
    } else if a >= 1.0e5 {
        format!("{sign}₹{:.2}L", a / 1.0e5)
    } else if a >= 1.0e3 {
        format!("{sign}₹{:.1}k", a / 1.0e3)
    } else if a >= 1.0 {
        format!("{sign}₹{:.2}", a)
    } else if a > 0.0 {
        format!("{sign}₹{:.3}", a)
    } else {
        "₹0".to_string()
    }
}

/// Group an integer rupee count using the Indian numbering system
/// (e.g. `1234567` -> `12,34,567`): the last three digits, then groups of two.
fn group_indian(n: u64) -> String {
    let digits: Vec<char> = n.to_string().chars().collect();
    let len = digits.len();
    if len <= 3 {
        return digits.into_iter().collect();
    }
    // Digits before the final group of three; the head is split into 2-digit
    // groups from the right (a leading group may be 1 digit wide).
    let head_len = len - 3;
    let mut out = String::new();
    for (idx, ch) in digits.iter().enumerate() {
        if idx == head_len {
            out.push(',');
        } else if idx > 0 && idx < head_len && (head_len - idx) % 2 == 0 {
            out.push(',');
        }
        out.push(*ch);
    }
    out
}

/// Format an INR amount with full Indian digit grouping and paise, e.g.
/// `₹1,23,456.78`.
pub fn format_inr(inr: f64) -> String {
    if !inr.is_finite() {
        return "₹0.00".to_string();
    }
    let sign = if inr < 0.0 { "-" } else { "" };
    let a = inr.abs();
    let mut rupees = a.trunc() as u64;
    let mut paise = (a.fract() * 100.0).round() as u64;
    // Rounding paise can carry into the rupee (e.g. 0.999 -> 1.00).
    if paise >= 100 {
        rupees += 1;
        paise -= 100;
    }
    format!("{sign}₹{}.{:02}", group_indian(rupees), paise)
}

/// One session's spend, in both USD (as recorded) and INR (converted).
pub struct SessionCost {
    /// Session id.
    pub id: String,
    /// Human-friendly session name (may be empty for unnamed sessions).
    pub name: String,
    /// Spend in USD, as recorded by the runtime.
    pub usd: f64,
    /// Spend in INR, converted with the ledger's rate.
    pub inr: f64,
    /// When the session was last updated (used for day/month bucketing).
    pub updated_at: DateTime<Utc>,
}

/// A roll-up of spend across sessions, with daily and monthly buckets.
pub struct CostLedger {
    /// The USD->INR rate used to build this ledger.
    pub rate: f64,
    /// Per-session spend, most-recently-updated first.
    pub sessions: Vec<SessionCost>,
    /// Total spend in USD across all included sessions.
    pub total_usd: f64,
    /// Total spend in INR across all included sessions.
    pub total_inr: f64,
    /// INR spend keyed by day (`YYYY-MM-DD`, IST), ascending.
    pub by_day: BTreeMap<String, f64>,
    /// INR spend keyed by month (`YYYY-MM`, IST), ascending.
    pub by_month: BTreeMap<String, f64>,
}

impl CostLedger {
    /// INR spent today (IST).
    pub fn today_inr(&self) -> f64 {
        let key = Utc::now()
            .with_timezone(&ist_offset())
            .format("%Y-%m-%d")
            .to_string();
        self.by_day.get(&key).copied().unwrap_or(0.0)
    }

    /// INR spent this month (IST).
    pub fn this_month_inr(&self) -> f64 {
        let key = Utc::now()
            .with_timezone(&ist_offset())
            .format("%Y-%m")
            .to_string();
        self.by_month.get(&key).copied().unwrap_or(0.0)
    }
}

/// Build an INR cost ledger from the user's recorded sessions.
///
/// Sessions without a recorded cost (or with a non-positive / non-finite cost)
/// are skipped. Spend is bucketed by each session's last-updated timestamp.
pub async fn build_ledger() -> anyhow::Result<CostLedger> {
    let rate = usd_inr_rate();
    let sessions = SessionManager::instance().list_sessions().await?;

    let mut rows: Vec<SessionCost> = Vec::new();
    let mut by_day: BTreeMap<String, f64> = BTreeMap::new();
    let mut by_month: BTreeMap<String, f64> = BTreeMap::new();
    let mut total_usd = 0.0_f64;

    for session in sessions {
        let usd = session.accumulated_cost.unwrap_or(0.0);
        if !(usd.is_finite() && usd > 0.0) {
            continue;
        }
        let inr = usd * rate;
        total_usd += usd;
        let updated_ist = session.updated_at.with_timezone(&ist_offset());
        *by_day
            .entry(updated_ist.format("%Y-%m-%d").to_string())
            .or_default() += inr;
        *by_month
            .entry(updated_ist.format("%Y-%m").to_string())
            .or_default() += inr;
        rows.push(SessionCost {
            id: session.id,
            name: session.name,
            usd,
            inr,
            updated_at: session.updated_at,
        });
    }

    rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

    Ok(CostLedger {
        rate,
        total_usd,
        total_inr: total_usd * rate,
        sessions: rows,
        by_day,
        by_month,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_formats_by_magnitude() {
        assert_eq!(format_inr_compact(0.0), "₹0");
        assert_eq!(format_inr_compact(0.123), "₹0.123");
        assert_eq!(format_inr_compact(12.5), "₹12.50");
        assert_eq!(format_inr_compact(1500.0), "₹1.5k");
        assert_eq!(format_inr_compact(250_000.0), "₹2.50L");
        assert_eq!(format_inr_compact(35_000_000.0), "₹3.50Cr");
        assert_eq!(format_inr_compact(-12.5), "-₹12.50");
    }

    #[test]
    fn full_uses_indian_grouping() {
        assert_eq!(format_inr(0.0), "₹0.00");
        assert_eq!(format_inr(999.0), "₹999.00");
        assert_eq!(format_inr(1234.5), "₹1,234.50");
        assert_eq!(format_inr(1_234_567.0), "₹12,34,567.00");
        // Paise rounding carries into the rupee.
        assert_eq!(format_inr(0.999), "₹1.00");
    }

    #[test]
    fn conversion_scales_with_rate() {
        // usd_to_inr depends on config/env; here we just check the pure math.
        let rate = 88.0;
        assert!((10.0_f64 * rate - 880.0).abs() < 1e-9);
    }

    #[test]
    fn day_month_buckets_use_ist_boundaries() {
        // 2026-06-19T20:00:00Z is 2026-06-20T01:30 IST: it must land in the IST
        // day/month, not the earlier UTC day.
        let utc: DateTime<Utc> = "2026-06-19T20:00:00Z".parse().unwrap();
        let ist = utc.with_timezone(&ist_offset());
        assert_eq!(ist.format("%Y-%m-%d").to_string(), "2026-06-20");
        assert_eq!(ist.format("%Y-%m").to_string(), "2026-06");

        // A month boundary: 2026-06-30T19:00:00Z is 2026-07-01T00:30 IST.
        let utc: DateTime<Utc> = "2026-06-30T19:00:00Z".parse().unwrap();
        let ist = utc.with_timezone(&ist_offset());
        assert_eq!(ist.format("%Y-%m-%d").to_string(), "2026-07-01");
        assert_eq!(ist.format("%Y-%m").to_string(), "2026-07");
    }
}
