//! Compact ₹ spend dashboard for `bharatcode cost --dashboard`.
//!
//! This is an opt-in alternate rendering of the same INR ledger that the `cost`
//! command already builds (see [`crate::commands::cost_ledger`]). Instead of the
//! default line-by-line summary it paints one compact panel that fits on a
//! single screen:
//!
//!   * the headline scope figures (total / today / this month) in ₹,
//!   * an ASCII sparkline-style bar per recent IST spend day,
//!   * the top-3 spend days called out explicitly,
//!   * a By-month roll-up with a bar per month, and
//!   * a "top models by spend" section: one bar per caller-supplied model,
//!     ranked highest first (via [`render_dashboard`]).
//!
//! It is purely presentational and read-only: it consumes a borrowed
//! [`CostLedger`] and returns a [`String`]. All amounts go through
//! [`format_inr`] / [`format_inr_compact`] and all colour goes through the
//! [`crate::theme`] role helpers, so the panel honours `NO_COLOR` and the active
//! locale exactly like the rest of the CLI. When `crate::a11y` is in
//! screen-reader mode the bar glyph is spelled out so the layout stays legible.
//!
//! The panel is gated behind `BHARATCODE_COST_DASHBOARD` at the call site; with
//! the variable unset the default cost output is byte-identical.
//!
//! Original BharatCode work; not ported from any third party.

use bharatcode_core::utils::safe_truncate;

use crate::commands::cost_ledger::{format_inr, format_inr_compact, CostLedger};

/// Environment key that opts into the dashboard view of `bharatcode cost`.
/// Truthy (`1` / `true` / `yes` / `on`) switches the command to the panel;
/// anything else — including absence — leaves the existing output unchanged.
pub const COST_DASHBOARD_ENABLED_KEY: &str = "BHARATCODE_COST_DASHBOARD";

/// Width (in glyphs) of the sparkline-style bars.
const BAR_WIDTH: usize = 24;

/// How many recent IST days the day sparkline lists.
const DAY_ROWS: usize = 10;

/// How many top spend days to call out.
const TOP_DAYS: usize = 3;

/// Width of the model-name column in the per-model share table.
const MODEL_COL: usize = 22;

/// Whether the dashboard view is enabled for this process.
///
/// Reads `BHARATCODE_COST_DASHBOARD` as a raw environment string so a bare `1`
/// is honoured (mirrors [`crate::commands::cost_extensions`], which avoids the
/// config layer coercing `1` into a JSON number). Accepts the usual truthy
/// spellings; anything else — including absence — is OFF, so default cost output
/// is byte-identical.
pub fn is_enabled() -> bool {
    match std::env::var(COST_DASHBOARD_ENABLED_KEY) {
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

/// Render an ASCII sparkline-style bar `width` glyphs wide whose filled length
/// is proportional to `value / max`.
///
/// Robust by construction:
///   * `width` is the hard cap on the rendered length (it never overflows),
///   * a zero, negative, or non-finite `max` yields an all-empty bar instead of
///     dividing by zero, and
///   * the filled length is clamped into `0..=width`, so a `value` above `max`
///     (or a NaN) can never panic or paint past the track.
///
/// The fill / track glyphs route through [`crate::a11y::glyph_or_word`] so a
/// screen-reader session gets a spelled-out, still-aligned representation.
pub fn bar(value: f64, max: f64, width: usize) -> String {
    let fill_glyph = crate::a11y::glyph_or_word("#", "=");
    let track_glyph = crate::a11y::glyph_or_word(".", "-");

    if width == 0 {
        return String::new();
    }
    let filled = if max.is_finite() && max > 0.0 && value.is_finite() && value > 0.0 {
        let frac = (value / max).clamp(0.0, 1.0);
        ((frac * width as f64).round() as usize).min(width)
    } else {
        0
    };
    let empty = width - filled;
    format!("{}{}", fill_glyph.repeat(filled), track_glyph.repeat(empty))
}

/// Display width of `s` ignoring any ANSI styling, so padding maths stay correct
/// whether or not colour is enabled.
fn visible_width(s: &str) -> usize {
    console::measure_text_width(s)
}

/// Pad `s` on the right with spaces to `width` visible columns. If `s` is already
/// at least `width` wide it is returned unchanged (callers pre-truncate columns).
fn pad_to(s: &str, width: usize) -> String {
    let w = visible_width(s);
    if w >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - w))
    }
}

/// Render the per-day spend sparkline section: the most recent [`DAY_ROWS`] IST
/// days, each with its date, a [`bar`] scaled to the busiest day in the window,
/// and the ₹ figure. Returns an empty vec when the ledger has no day buckets.
fn day_rows(ledger: &CostLedger) -> Vec<String> {
    if ledger.by_day.is_empty() {
        return Vec::new();
    }
    // Most-recent day first; keep only the trailing window.
    let mut days: Vec<(&String, f64)> = ledger.by_day.iter().map(|(d, v)| (d, *v)).collect();
    days.reverse();
    days.truncate(DAY_ROWS);
    let max = days.iter().map(|(_, v)| *v).fold(0.0_f64, f64::max);

    let mut out: Vec<String> = Vec::new();
    out.push(crate::theme::heading(label("cost.dash_by_day", "By day (recent)")).to_string());
    for (day, inr) in &days {
        out.push(format!(
            "{}  {}  {}",
            crate::theme::muted(day.as_str()),
            crate::theme::accent(bar(*inr, max, BAR_WIDTH)),
            crate::theme::success(format_inr_compact(*inr)),
        ));
    }
    out
}

/// Render the "top spend days" callout: the [`TOP_DAYS`] highest-₹ IST days,
/// ranked, each with a [`bar`] scaled to the single busiest day so the leader
/// fills the track. Empty when the ledger has no day buckets.
fn top_day_rows(ledger: &CostLedger) -> Vec<String> {
    if ledger.by_day.is_empty() {
        return Vec::new();
    }
    let mut days: Vec<(&String, f64)> = ledger.by_day.iter().map(|(d, v)| (d, *v)).collect();
    // Highest spend first; ties broken by the (descending) date for stability.
    days.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.0.cmp(a.0))
    });
    days.truncate(TOP_DAYS);
    let max = days.iter().map(|(_, v)| *v).fold(0.0_f64, f64::max);

    let mut out: Vec<String> = Vec::new();
    out.push(
        crate::theme::heading(
            label("cost.dash_top_days", "Top spend days").replace("{n}", &TOP_DAYS.to_string()),
        )
        .to_string(),
    );
    for (rank, (day, inr)) in days.iter().enumerate() {
        out.push(format!(
            "{}. {}  {}  {}",
            rank + 1,
            crate::theme::muted(day.as_str()),
            crate::theme::accent(bar(*inr, max, BAR_WIDTH)),
            crate::theme::success(format_inr_compact(*inr)),
        ));
    }
    out
}

/// Render the By-month roll-up: every IST month bucket (most recent first) with
/// a [`bar`] scaled to the busiest month and its ₹ total. Empty when the ledger
/// has no month buckets.
fn month_rows(ledger: &CostLedger) -> Vec<String> {
    if ledger.by_month.is_empty() {
        return Vec::new();
    }
    let max = ledger.by_month.values().copied().fold(0.0_f64, f64::max);

    let mut out: Vec<String> = Vec::new();
    out.push(crate::theme::heading(label("cost.dash_by_month", "By month")).to_string());
    for (month, inr) in ledger.by_month.iter().rev() {
        out.push(format!(
            "{}  {}  {}",
            crate::theme::muted(month.as_str()),
            crate::theme::accent(bar(*inr, max, BAR_WIDTH)),
            crate::theme::success(format_inr(*inr)),
        ));
    }
    out
}

/// Render the "top models by spend" section from caller-supplied `(name, spend)`
/// pairs.
///
/// This is the spec-shaped per-model view: it emits **exactly one [`bar`] per
/// model** (so N models => N bars), highest spend first, each bar scaled to the
/// busiest model so the leader fills the track, followed by the model's ₹ spend.
/// Returns an empty vec when `models` is empty, so a caller with nothing to show
/// adds no section. Non-finite / negative spends are floored to zero for scaling
/// but the row is still listed (one bar per supplied model, no silent drops).
fn top_model_rows(models: &[(String, f64)]) -> Vec<String> {
    if models.is_empty() {
        return Vec::new();
    }
    // Highest spend first; ties broken by name for stable output.
    let mut ranked: Vec<(&str, f64)> = models
        .iter()
        .map(|(name, spend)| {
            let s = if spend.is_finite() && *spend > 0.0 {
                *spend
            } else {
                0.0
            };
            (name.as_str(), s)
        })
        .collect();
    ranked.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(b.0))
    });
    let max = ranked.iter().map(|(_, s)| *s).fold(0.0_f64, f64::max);

    let mut out: Vec<String> = Vec::new();
    out.push(
        crate::theme::heading(label("cost.dash_top_models", "Top models by spend (₹)")).to_string(),
    );
    for (name, spend) in &ranked {
        let name_col = pad_to(&safe_truncate(name, MODEL_COL), MODEL_COL);
        out.push(format!(
            "  {}  {}  {}",
            name_col,
            crate::theme::accent(bar(*spend, max, BAR_WIDTH)),
            crate::theme::success(format_inr_compact(*spend)),
        ));
    }
    out
}

/// Build the compact ₹ spend dashboard for the given ledger.
///
/// `rate` is the active USD->INR rate (normally `ledger.rate`). `models` is the
/// caller-supplied "top models by spend" list as `(name, ₹ spend)` pairs; the
/// panel emits exactly one bar per supplied model (see [`top_model_rows`]). This
/// is a pure function over its arguments: it performs no I/O and returns the
/// fully rendered panel as a [`String`].
pub fn render_dashboard(ledger: &CostLedger, rate: f64, models: &[(String, f64)]) -> String {
    render_dashboard_inner(ledger, rate, &top_model_rows(models))
}

/// Shared panel body. `model_section` is the already-rendered per-model block
/// (empty when there is nothing to show), kept separate from the scope / day /
/// month layout so [`render_dashboard`] can supply the spend-ranked bars.
fn render_dashboard_inner(ledger: &CostLedger, rate: f64, model_section: &[String]) -> String {
    let mut lines: Vec<String> = Vec::new();

    lines.push(String::new());
    lines.push(
        crate::theme::heading(label("cost.dash_title", "BharatCode cost dashboard (INR)"))
            .to_string(),
    );
    lines.push(
        crate::theme::muted(
            label("cost.dash_rate", "USD -> INR rate: {rate}")
                .replace("{rate}", &format!("{rate:.2}")),
        )
        .to_string(),
    );
    lines.push(String::new());

    // Scope figures (always present, even on an empty ledger).
    lines.push(format!(
        "  {:<14}{}",
        label("cost.dash_total", "Total"),
        crate::theme::success(format_inr(ledger.total_inr)),
    ));
    lines.push(format!(
        "  {:<14}{}",
        label("cost.dash_today", "Today"),
        crate::theme::success(format_inr(ledger.today_inr())),
    ));
    lines.push(format!(
        "  {:<14}{}",
        label("cost.dash_month", "This month"),
        crate::theme::success(format_inr(ledger.this_month_inr())),
    ));

    if ledger.by_day.is_empty() && ledger.by_month.is_empty() {
        lines.push(String::new());
        lines.push(
            crate::theme::muted(label(
                "cost.dash_empty",
                "No spend yet. Run a session to start the ledger.",
            ))
            .to_string(),
        );
        lines.push(String::new());
        return lines.join("\n");
    }

    // Per-day sparkline.
    let days = day_rows(ledger);
    if !days.is_empty() {
        lines.push(String::new());
        lines.extend(days);
    }

    // Top spend days callout.
    let top = top_day_rows(ledger);
    if !top.is_empty() {
        lines.push(String::new());
        lines.extend(top);
    }

    // By-month roll-up.
    let months = month_rows(ledger);
    if !months.is_empty() {
        lines.push(String::new());
        lines.extend(months);
    }

    // Per-model section (caller-supplied: spend bars or registry ₹/1K rates).
    if !model_section.is_empty() {
        lines.push(String::new());
        lines.extend(model_section.iter().cloned());
    }

    lines.push(String::new());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    // Env mutation is process-global; serialize the env-touching tests.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn fixed_ledger() -> CostLedger {
        let mut by_day: BTreeMap<String, f64> = BTreeMap::new();
        by_day.insert("2026-06-08".to_string(), 50.0);
        by_day.insert("2026-06-10".to_string(), 100.0);
        by_day.insert("2026-06-12".to_string(), 230.0);
        let mut by_month: BTreeMap<String, f64> = BTreeMap::new();
        by_month.insert("2026-05".to_string(), 70.0);
        by_month.insert("2026-06".to_string(), 380.0);
        CostLedger {
            rate: 88.0,
            sessions: vec![],
            total_usd: 450.0 / 88.0,
            total_inr: 450.0,
            by_day,
            by_month,
        }
    }

    fn empty_ledger() -> CostLedger {
        CostLedger {
            rate: 88.0,
            sessions: vec![],
            total_usd: 0.0,
            total_inr: 0.0,
            by_day: BTreeMap::new(),
            by_month: BTreeMap::new(),
        }
    }

    // Keep the unused helper referenced so the (currently sessionless) fixtures
    // don't trip dead-code lints if the ledger shape grows session-aware tests.
    #[allow(dead_code)]
    fn at(day: u32) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 6, day, 12, 0, 0).unwrap()
    }

    /// Strip ANSI styling so visible-layout assertions hold regardless of whether
    /// colour happens to be enabled in the test process.
    fn plain(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\u{1b}' {
                if chars.peek() == Some(&'[') {
                    chars.next();
                    for cc in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&cc) {
                            break;
                        }
                    }
                }
                continue;
            }
            out.push(c);
        }
        out
    }

    #[test]
    fn dashboard_has_rupee_scopes_bymonth_and_top_days() {
        let ledger = fixed_ledger();
        let out = render_dashboard(&ledger, ledger.rate, &[]);
        let flat = plain(&out);

        // ₹ symbols on the scope and roll-up lines.
        assert!(flat.contains('₹'), "dashboard must show the rupee symbol");
        assert!(flat.contains("Total"), "missing total scope label");
        assert!(flat.contains("Today"), "missing today scope label");
        assert!(flat.contains("This month"), "missing month scope label");

        // The By-month line must be present.
        assert!(flat.contains("By month"), "missing By-month section");

        // The top-day callout and its rows (the busiest day at ₹230) must show.
        assert!(flat.contains("Top spend days"), "missing top-day section");
        assert!(
            flat.contains("2026-06-12"),
            "top-day rows must list the busiest day, got:\n{flat}"
        );

        // The headline total must appear in full ₹ form.
        assert!(flat.contains(&format_inr(450.0)), "missing total figure");
    }

    /// The spec contract: `render_dashboard` paints exactly one bar per supplied
    /// model, shows the ₹ glyph and the headline total, and never leaks upstream
    /// branding. Bars are counted by the fill/track glyph rather than substring
    /// so the count is independent of colour state.
    #[test]
    fn render_dashboard_emits_one_bar_per_model_and_is_leak_free() {
        let ledger = fixed_ledger();
        // Three "top models by spend" pairs => exactly three bar rows.
        let models: Vec<(String, f64)> = vec![
            ("sarvam-m".to_string(), 230.0),
            ("gpt-4o".to_string(), 150.0),
            ("claude-3-5-sonnet".to_string(), 70.0),
        ];
        let out = render_dashboard(&ledger, ledger.rate, &models);
        let flat = plain(&out);

        // ₹ glyph and the headline total figure are present.
        assert!(flat.contains('₹'), "dashboard must show the rupee symbol");
        assert!(
            flat.contains(&format_inr(450.0)),
            "missing total figure, got:\n{flat}"
        );

        // Exactly N bars for N models: the top-models section heading plus one
        // bar-bearing row per model. A bar is any line containing the fill OR
        // track glyph that sits under the top-models heading.
        let fill = crate::a11y::glyph_or_word("#", "=");
        let track = crate::a11y::glyph_or_word(".", "-");
        let section: Vec<&str> = flat
            .lines()
            .skip_while(|l| !l.contains("Top models by spend"))
            .skip(1) // the heading line itself
            .take(models.len())
            .collect();
        assert_eq!(
            section.len(),
            models.len(),
            "expected exactly {} model rows, got {}:\n{flat}",
            models.len(),
            section.len()
        );
        for row in &section {
            assert!(
                row.contains(fill.as_str()) || row.contains(track.as_str()),
                "each model row must carry a bar, got: {row:?}"
            );
        }
        // Highest spend (sarvam-m, ₹230) leads the ranking.
        assert!(
            section[0].contains("sarvam-m"),
            "highest-spend model must rank first, got: {:?}",
            section[0]
        );

        // Leak-free.
        let lower = out.to_lowercase();
        assert!(!lower.contains("goose"), "must not leak the goose name");
        assert!(!lower.contains("block"), "must not leak the Block name");
    }

    #[test]
    fn bar_clamps_width_and_survives_zero_max() {
        // The fill / track glyphs depend on the (process-global) a11y mode, which
        // a concurrent env-touching test may toggle; derive them here so the
        // assertions hold regardless of that state.
        let fill = crate::a11y::glyph_or_word("#", "=");
        let track = crate::a11y::glyph_or_word(".", "-");

        // Never wider than the requested width, even when value exceeds max.
        assert_eq!(visible_width(&bar(10.0, 5.0, 8)), 8);
        assert_eq!(visible_width(&bar(3.0, 6.0, 12)), 12);

        // Zero / negative / non-finite max must not panic and must yield an
        // empty (all-track) bar of the requested width.
        let z = bar(5.0, 0.0, 10);
        assert_eq!(visible_width(&z), 10);
        assert!(
            !z.contains(fill.as_str()),
            "zero-max bar must have no fill, got {z:?}"
        );
        assert_eq!(z, track.repeat(10));
        assert_eq!(visible_width(&bar(5.0, -1.0, 10)), 10);
        assert_eq!(visible_width(&bar(5.0, f64::NAN, 10)), 10);
        assert_eq!(visible_width(&bar(f64::NAN, 5.0, 10)), 10);

        // Zero width is a no-op (empty string), never a panic.
        assert_eq!(bar(5.0, 5.0, 0), "");

        // A full bar fills the whole track.
        assert_eq!(bar(5.0, 5.0, 6), fill.repeat(6));
    }

    #[test]
    fn empty_ledger_renders_no_spend_panel() {
        let ledger = empty_ledger();
        let out = render_dashboard(&ledger, ledger.rate, &[]);
        let flat = plain(&out).to_lowercase();
        assert!(
            flat.contains("no spend yet"),
            "empty ledger must render a 'no spend yet' panel, got:\n{out}"
        );
        // Even empty, it is still a complete ₹ panel with the three scopes.
        assert!(plain(&out).contains('₹'));
        assert!(plain(&out).contains("Total"));
    }

    #[test]
    fn no_upstream_branding_leaks() {
        let ledger = fixed_ledger();
        let out = render_dashboard(&ledger, ledger.rate, &[]);
        let lower = out.to_lowercase();
        assert!(!lower.contains("goose"), "must not leak the goose name");
        assert!(!lower.contains("block"), "must not leak the Block name");
    }

    #[test]
    fn no_color_output_is_free_of_ansi_escapes() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("NO_COLOR", "1");
        console::set_colors_enabled(false);

        let ledger = fixed_ledger();
        let out = render_dashboard(&ledger, ledger.rate, &[]);

        assert!(
            !out.as_bytes().contains(&0x1b),
            "NO_COLOR output must contain no ANSI escape (0x1b) bytes"
        );

        std::env::remove_var("NO_COLOR");
    }

    #[test]
    fn is_truthy_accepts_common_spellings() {
        for v in ["1", "true", "TRUE", "yes", "on", " on "] {
            assert!(is_truthy(v), "expected {v:?} to be truthy");
        }
        for v in ["0", "false", "no", "off", "", "maybe"] {
            assert!(!is_truthy(v), "expected {v:?} to be falsey");
        }
    }
}
