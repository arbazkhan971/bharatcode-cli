//! Compact single-screen ₹ cost dashboard for `bharatcode cost`.
//!
//! This is an opt-in alternate rendering of the same INR ledger that the `cost`
//! command already builds (see [`crate::commands::cost_ledger`]). Instead of the
//! default line-by-line summary it paints one aligned ASCII panel that fits on a
//! single screen: the headline total alongside today / this-month roll-ups, a
//! short "top sessions" list, and a per-known-model ₹/1K reference table.
//!
//! It is purely presentational and read-only: it consumes a borrowed
//! [`CostLedger`] plus the already-resolved model candidates and returns a
//! [`String`]. All amounts go through [`format_inr`] / [`format_inr_compact`] and
//! all colour goes through the [`crate::theme`] role helpers, so the panel
//! honours `NO_COLOR` and the active locale exactly like the rest of the CLI.
//!
//! The panel is gated behind `BHARATCODE_COST_DASHBOARD` at the call site; with
//! the variable unset the default cost output is byte-identical.
//!
//! Original BharatCode work; not ported from any third party.

use std::collections::BTreeSet;

use goose::model_registry::{self, ModelInfo};
use goose::utils::safe_truncate;

use crate::commands::cost_ledger::{format_inr, format_inr_compact, CostLedger, SessionCost};

/// Environment key that opts into the dashboard view of `bharatcode cost`.
/// Truthy (`1` / `true` / `yes` / `on`) switches the command to the panel;
/// anything else — including absence — leaves the existing output unchanged.
pub const COST_DASHBOARD_ENABLED_KEY: &str = "BHARATCODE_COST_DASHBOARD";

/// Inner content width of the panel (the dashed rules and label columns are
/// sized to this). Chosen to fit comfortably inside an 80-column terminal once
/// the two-space outer indent and the `| ` / ` |` borders are added.
const PANEL_WIDTH: usize = 72;

/// How many recent sessions the "top sessions" block lists.
const TOP_SESSIONS: usize = 5;

/// Width of the model-name column in the known-models table.
const MODEL_COL: usize = 22;

/// Width of the provider column in the known-models table.
const PROVIDER_COL: usize = 20;

/// Visible column at which the scope ₹ figure begins, so every scope line aligns.
const SCOPE_AMOUNT_AT: usize = 26;

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

/// A horizontal dashed rule spanning the inner panel width.
fn rule() -> String {
    "-".repeat(PANEL_WIDTH)
}

/// Render one labelled scope figure (e.g. `Total spend ........ ₹1,234.00`) with
/// the dotted leader sized so the ₹ amount lands in a fixed column.
fn scope_line(label_text: &str, amount: &str) -> String {
    let lead = visible_width(label_text);
    let dots = SCOPE_AMOUNT_AT.saturating_sub(lead + 1);
    format!(
        "{} {} {}",
        label_text,
        crate::theme::muted(".".repeat(dots.max(1))),
        crate::theme::success(amount),
    )
}

/// Render the registry-backed per-model ₹/1K rows.
///
/// Each candidate that the static [`model_registry`] recognises produces one row
/// whose plain-text prefix (marker + name + provider + `in `) is a fixed visible
/// width, so the trailing ₹ figures align across rows. Returns the rendered row
/// strings plus a leading heading and a trailing `* active model` note when one
/// of the rows is the active model. Empty when no candidate is recognised.
fn model_rows(rate: f64, candidates: &[String], active: Option<&str>) -> Vec<String> {
    // De-duplicate registry hits so two aliases of one model don't list twice.
    let mut rows: Vec<(&str, &'static ModelInfo, bool)> = Vec::new();
    let mut shown: BTreeSet<&'static str> = BTreeSet::new();
    for name in candidates {
        if let Some(info) = model_registry::lookup(name) {
            if shown.insert(info.name) {
                let is_active = active == Some(name.as_str());
                rows.push((name.as_str(), info, is_active));
            }
        }
    }
    if rows.is_empty() {
        return Vec::new();
    }

    let mut out: Vec<String> = Vec::new();
    out.push(
        crate::theme::heading(label("cost.dash_models", "Per-model rate (₹ / 1K tokens)"))
            .to_string(),
    );

    let mut has_active = false;
    for (name, info, is_active) in &rows {
        if *is_active {
            has_active = true;
        }
        let marker = if *is_active { "*" } else { " " };
        let name_col = pad_to(&safe_truncate(name, MODEL_COL), MODEL_COL);
        let provider_col = pad_to(&safe_truncate(info.provider, PROVIDER_COL), PROVIDER_COL);
        // Fixed-width plain prefix: everything up to (but not including) the first
        // ₹ figure. Built without colour so its visible width is identical on
        // every row; only the trailing amounts are painted.
        let prefix = format!("{marker} {name_col}  {provider_col}  in ");
        let in_inr = format_inr_compact(info.input_per_1k_inr(rate));
        let out_inr = format_inr_compact(info.output_per_1k_inr(rate));
        out.push(format!(
            "{}{} {} {}",
            prefix,
            crate::theme::success(in_inr),
            crate::theme::muted("/ out"),
            crate::theme::success(out_inr),
        ));
    }
    if has_active {
        out.push(crate::theme::muted(label("cost.dash_active", "* active model")).to_string());
    }
    out
}

/// Render the "top sessions" block: the most-recent sessions with their ₹ spend,
/// each padded to a fixed name column so the amounts align.
fn session_rows(sessions: &[SessionCost]) -> Vec<String> {
    if sessions.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<String> = Vec::new();
    out.push(crate::theme::heading(label("cost.dash_top_sessions", "Top sessions")).to_string());
    const NAME_COL: usize = 34;
    for s in sessions.iter().take(TOP_SESSIONS) {
        let when = s.updated_at.format("%Y-%m-%d").to_string();
        let raw_name = if s.name.is_empty() {
            s.id.as_str()
        } else {
            s.name.as_str()
        };
        let name_col = pad_to(&safe_truncate(raw_name, NAME_COL), NAME_COL);
        out.push(format!(
            "{} {}  {}",
            crate::theme::muted(when),
            name_col,
            crate::theme::success(format_inr_compact(s.inr)),
        ));
    }
    out
}

/// Wrap a body line in the panel border with the standard outer indent. Content
/// is padded on the right so the closing `|` lands in the same column on every
/// row (padding uses visible width, ignoring any ANSI styling).
fn body_line(content: &str) -> String {
    let padded = pad_to(content, PANEL_WIDTH);
    format!("  | {padded} |")
}

/// A full-width horizontal border (`+----+`) for the panel's top and bottom.
fn border() -> String {
    format!("  +{}+", "-".repeat(PANEL_WIDTH + 2))
}

/// Build the aligned single-screen ₹ cost dashboard for the given ledger.
///
/// `rate` is the active USD->INR rate (normally `ledger.rate`), `candidates` is
/// the de-duplicated list of model names worth describing (active model first),
/// and `active` is the configured active model name, if any.
pub fn render_dashboard(
    ledger: &CostLedger,
    rate: f64,
    candidates: &[String],
    active: Option<&str>,
) -> String {
    let mut lines: Vec<String> = Vec::new();

    lines.push(String::new());
    lines.push(border());
    lines.push(body_line(
        &crate::theme::heading(label("cost.dash_title", "BharatCode cost dashboard (INR)"))
            .to_string(),
    ));
    lines.push(body_line(
        &crate::theme::muted(
            label("cost.dash_rate", "USD -> INR rate: {rate}")
                .replace("{rate}", &format!("{rate:.2}")),
        )
        .to_string(),
    ));
    lines.push(body_line(&crate::theme::muted(rule()).to_string()));

    if ledger.sessions.is_empty() {
        // Empty-ledger panel: a clearly-worded "no spend yet" line plus zeroed
        // scope figures, so the dashboard is still a complete, self-explanatory
        // screen carrying the ₹ symbol and the three scope labels.
        lines.push(body_line(
            &crate::theme::muted(label(
                "cost.dash_empty",
                "No spend yet. Run a session to start the ledger.",
            ))
            .to_string(),
        ));
        lines.push(body_line(""));
        lines.push(body_line(&scope_line(
            &label("cost.dash_total", "Total spend"),
            &format_inr(0.0),
        )));
        lines.push(body_line(&scope_line(
            &label("cost.dash_today", "Today"),
            &format_inr(0.0),
        )));
        lines.push(body_line(&scope_line(
            &label("cost.dash_month", "This month"),
            &format_inr(0.0),
        )));
        lines.push(border());
        lines.push(String::new());
        return lines.join("\n");
    }

    // Scope figures.
    lines.push(body_line(&scope_line(
        &label("cost.dash_total", "Total spend"),
        &format_inr(ledger.total_inr),
    )));
    lines.push(body_line(&scope_line(
        &label("cost.dash_today", "Today"),
        &format_inr(ledger.today_inr()),
    )));
    lines.push(body_line(&scope_line(
        &label("cost.dash_month", "This month"),
        &format_inr(ledger.this_month_inr()),
    )));

    // Top sessions.
    let sessions = session_rows(&ledger.sessions);
    if !sessions.is_empty() {
        lines.push(body_line(&crate::theme::muted(rule()).to_string()));
        for r in sessions {
            lines.push(body_line(&r));
        }
    }

    // Per-model ₹/1K reference.
    let models = model_rows(rate, candidates, active);
    if !models.is_empty() {
        lines.push(body_line(&crate::theme::muted(rule()).to_string()));
        for r in models {
            lines.push(body_line(&r));
        }
    }

    lines.push(border());
    lines.push(String::new());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use std::collections::BTreeMap;

    fn session(id: &str, name: &str, inr: f64, day: u32) -> SessionCost {
        SessionCost {
            id: id.to_string(),
            name: name.to_string(),
            usd: inr / 88.0,
            inr,
            updated_at: Utc.with_ymd_and_hms(2026, 6, day, 12, 0, 0).unwrap(),
        }
    }

    fn fixed_ledger() -> CostLedger {
        let mut by_day: BTreeMap<String, f64> = BTreeMap::new();
        by_day.insert("2026-06-10".to_string(), 100.0);
        by_day.insert("2026-06-12".to_string(), 230.0);
        let mut by_month: BTreeMap<String, f64> = BTreeMap::new();
        by_month.insert("2026-06".to_string(), 330.0);
        CostLedger {
            rate: 88.0,
            sessions: vec![
                session("s-aaaaaaaa", "refactor cost ledger", 230.0, 12),
                session("s-bbbbbbbb", "", 100.0, 10),
            ],
            total_usd: 330.0 / 88.0,
            total_inr: 330.0,
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

    /// Strip ANSI styling so visible-layout assertions hold regardless of whether
    /// colour happens to be enabled in the test process.
    ///
    /// A self-contained CSI/SGR remover: `console::strip_ansi_codes` is gated
    /// behind the crate's `ansi-parsing` feature, which this build does not
    /// enable, so we drop `ESC [ ... <final byte>` sequences ourselves.
    fn plain(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\u{1b}' {
                // Consume the optional `[` and the parameter/intermediate bytes
                // up to and including the final byte in the 0x40..=0x7e range.
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
    fn dashboard_has_rupee_symbol_and_all_three_scopes() {
        let ledger = fixed_ledger();
        let candidates: Vec<String> = vec!["gpt-4o".to_string(), "sarvam-m".to_string()];
        let out = render_dashboard(&ledger, ledger.rate, &candidates, Some("gpt-4o"));
        let flat = plain(&out);

        assert!(flat.contains('₹'), "dashboard must show the rupee symbol");
        assert!(flat.contains("Total spend"), "missing total scope label");
        assert!(flat.contains("Today"), "missing today scope label");
        assert!(flat.contains("This month"), "missing month scope label");
        // The headline total must actually appear in full ₹ form.
        assert!(flat.contains(&format_inr(330.0)), "missing total figure");
    }

    #[test]
    fn no_upstream_branding_leaks() {
        let ledger = fixed_ledger();
        let candidates: Vec<String> = vec!["gpt-4o".to_string()];
        let out = render_dashboard(&ledger, ledger.rate, &candidates, None);
        let lower = out.to_lowercase();
        assert!(!lower.contains("goose"), "must not leak the goose name");
        assert!(!lower.contains("block"), "must not leak the Block name");
    }

    #[test]
    fn model_rows_align_up_to_the_rupee() {
        let ledger = fixed_ledger();
        // Two known models of different name/provider lengths to exercise the
        // fixed-width prefix padding.
        let candidates: Vec<String> = vec!["gpt-4o".to_string(), "sarvam-m".to_string()];
        let out = render_dashboard(&ledger, ledger.rate, &candidates, Some("gpt-4o"));

        // The visible (un-styled) offset of the first ₹ on each model row. Model
        // rows are the ones carrying an ` in ` segment before the figure.
        let offsets: Vec<usize> = plain(&out)
            .lines()
            .filter(|l| l.contains(" in ") && l.contains('₹'))
            .map(|l| {
                let prefix = &l[..l.find('₹').unwrap()];
                visible_width(prefix)
            })
            .collect();

        assert!(
            offsets.len() >= 2,
            "expected at least two aligned model rows, got {}",
            offsets.len()
        );
        assert!(
            offsets.iter().all(|&w| w == offsets[0]),
            "model rows are not aligned up to the ₹: offsets = {offsets:?}"
        );
    }

    #[test]
    fn empty_ledger_renders_no_spend_panel() {
        let ledger = empty_ledger();
        let out = render_dashboard(&ledger, ledger.rate, &[], None);
        let flat = plain(&out).to_lowercase();
        assert!(
            flat.contains("no spend yet"),
            "empty ledger must render a 'no spend yet' panel, got:\n{out}"
        );
        // Even empty, it is still a complete ₹ panel.
        assert!(plain(&out).contains('₹'));
    }

    #[test]
    fn no_color_output_is_free_of_ansi_escapes() {
        std::env::set_var("NO_COLOR", "1");
        // The console crate's colour state is process-global; force it off so the
        // assertion is deterministic regardless of test ordering and TTY state.
        console::set_colors_enabled(false);

        let ledger = fixed_ledger();
        let candidates: Vec<String> = vec!["gpt-4o".to_string(), "sarvam-m".to_string()];
        let out = render_dashboard(&ledger, ledger.rate, &candidates, Some("gpt-4o"));

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
