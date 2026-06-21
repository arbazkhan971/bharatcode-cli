//! `bharatcode cost`: summarize LLM spend in Indian rupees.
//!
//! Surfaces the INR cost ledger (see [`crate::commands::cost_ledger`]) as a
//! human-readable summary: a headline total, today / this-month roll-ups, a
//! recent per-session breakdown, and a monthly history. All amounts are shown
//! in ₹ using the configured USD->INR rate (`BHARATCODE_USD_INR`).
//!
//! Original BharatCode work; not ported from any third party.

use std::collections::BTreeSet;

use console::style;
use bharatcode_core::config::Config;
use bharatcode_core::model_registry::{self, ModelInfo};
use bharatcode_core::session::SessionManager;
use bharatcode_core::utils::safe_truncate;

use crate::commands::cost_ledger::{self, format_inr, format_inr_compact, SessionCost};

// Apply-patch hunk stats, declared inline so the helper lives next to its only
// caller without widening the `commands` module surface. Used below to render a
// "Recent patch activity" footer when a session patch sidecar is present.
#[path = "patch_stats.rs"]
mod patch_stats;

// Session-store storage health, declared inline next to its only caller (the
// `cost` footer) so the helper does not widen the `commands` module surface.
// Renders a read-only DB size / session / message / WAL footer below.
#[path = "db_health.rs"]
mod db_health;

// Opt-in "Extensions in use" attribution footer (BHARATCODE_COST_EXTENSIONS).
// Declared inline next to its only call site so wiring it does not widen the
// `commands` module surface. Default OFF => cost output unchanged.
#[path = "cost_extensions.rs"]
mod cost_extensions;

// GA release-profile footer (BharatCode v97). Renders one read-only themed line
// carrying the crate version, build profile (debug/release), and the GA
// release-channel marker. Opt-in via `BHARATCODE_COST_RELEASE`; default OFF =>
// the cost output is byte-identical. Declared inline next to its only call site.
#[path = "release_profile.rs"]
mod release_profile;

// CI-only machine-readable footer + GitHub Actions annotation, declared inline
// next to its only call site (the `cost` footer). Emitted only under CI; with
// no CI signal the cost output is byte-identical to before.
#[path = "ci_report.rs"]
mod ci_report;

// Strictly-local, opt-in, aggregated usage counters (BharatCode v92). Declared
// inline next to its only call site (the `cost` footer). Renders ONE muted line
// of monotonic counts (turns, tool calls, sessions, tokens, days active) read
// from a single rolling JSON aggregate under the config dir — no network, no
// per-event detail. Opt-in via `BHARATCODE_ANALYTICS_LOCAL`; default OFF => the
// cost output is byte-identical and there is zero I/O.
#[path = "analytics_local.rs"]
mod analytics_local;

// Privacy-preserving, strictly-local usage analytics, declared inline next to
// its only call site (the `cost` footer). Renders a compact "Usage (local,
// aggregated)" block of counts only. Opt-in via the env gate
// `BHARATCODE_ANALYTICS`; default OFF => the cost output is byte-identical.
#[path = "usage_analytics.rs"]
mod usage_analytics;

// Compact ₹ cost dashboard, declared inline next to its only call site (the
// early dashboard branch in `handle_cost`). Mirrors the `patch_stats` pattern so
// wiring it does not widen the `commands` module surface. Opt-in via the env
// gate `BHARATCODE_COST_DASHBOARD`; default OFF => existing cost output.
#[path = "cost_dashboard.rs"]
mod cost_dashboard;

// Locale-aware (Indian lakh / crore) number + currency grouping, declared
// inline next to its only call site (the headline ₹ totals below). Mirrors the
// `cost_dashboard` pattern so wiring it does not widen the `commands` module
// surface. Opt-in via the env gate `BHARATCODE_NUMFMT=indian`; default OFF =>
// existing ₹ formatting, byte-identical to before.
#[path = "indic_format.rs"]
mod indic_format;

/// Render an INR amount for the headline totals, honouring the
/// `BHARATCODE_NUMFMT=indian` grouping switch.
///
/// When Indian grouping is enabled the amount is routed through
/// [`indic_format::format_inr_indian`]; otherwise it uses the ledger's existing
/// [`format_inr`], so default output is byte-identical to before.
fn fmt_inr(inr: f64) -> String {
    if indic_format::indian_grouping_enabled() {
        indic_format::format_inr_indian(inr)
    } else {
        format_inr(inr)
    }
}

/// Options for the `cost` subcommand.
pub struct CostOptions {
    /// Show every session with a recorded cost instead of just the most recent.
    pub all: bool,
    /// Maximum number of sessions to list when not showing all.
    pub limit: usize,
}

impl Default for CostOptions {
    fn default() -> Self {
        Self {
            all: false,
            limit: 10,
        }
    }
}

const NAME_WIDTH: usize = 40;

/// Look up a user-facing label through the i18n layer, falling back to the
/// English `default` when the active locale table has no entry for `key`.
///
/// `tr!` echoes the key back when it is missing, so an unchanged key is treated
/// as "untranslated" and the English default is used. This keeps English output
/// stable while leaving room for Hindi (and other locales) to take effect.
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// Format a token count compactly (e.g. `131k`, `2.0M`) for the model table.
fn format_tokens(n: u32) -> String {
    let n = n as f64;
    if n >= 1.0e6 {
        format!("{:.1}M", n / 1.0e6)
    } else if n >= 1.0e3 {
        format!("{:.0}k", n / 1.0e3)
    } else {
        format!("{n:.0}")
    }
}

/// Collect candidate model names worth describing: the configured active model
/// plus every distinct model recorded across the user's sessions. The active
/// model (if any) is returned first; the rest are sorted for stable output.
async fn candidate_models() -> Vec<String> {
    let active = Config::global().get_bharatcode_model().ok();

    let mut seen: BTreeSet<String> = BTreeSet::new();
    if let Ok(sessions) = SessionManager::instance().list_sessions().await {
        for session in sessions {
            if let Some(name) = session
                .model_config
                .as_ref()
                .map(|mc| mc.model_name.clone())
            {
                if !name.trim().is_empty() {
                    seen.insert(name);
                }
            }
        }
    }

    let mut out: Vec<String> = Vec::new();
    if let Some(active) = active {
        if !active.trim().is_empty() {
            seen.remove(&active);
            out.push(active);
        }
    }
    out.extend(seen);
    out
}

/// Render the registry-backed "known models" table: for each candidate model
/// that the static [`model_registry`] recognises, show its provider, context
/// window and per-1K-token ₹ cost (input / output) at the ledger `rate`.
///
/// Build the dashboard's "top models by spend" list from the resolved model
/// `candidates`, ranked highest first.
///
/// The cost ledger does not record per-model spend, so each recognised model's
/// blended ₹/1K reference rate (mean of input/output at `rate`) stands in as a
/// spend proxy: it is the best per-model ₹ signal the data affords and gives the
/// dashboard a meaningful, stable ordering. De-duplicates registry aliases so a
/// model is not listed twice. Returns `(name, ₹ proxy)` pairs; empty when no
/// candidate is recognised, in which case the dashboard simply omits the section.
fn top_models_by_spend(rate: f64, candidates: &[String]) -> Vec<(String, f64)> {
    let mut out: Vec<(String, f64)> = Vec::new();
    let mut shown: BTreeSet<&'static str> = BTreeSet::new();
    for name in candidates {
        if let Some(info) = model_registry::lookup(name) {
            if shown.insert(info.name) {
                let blended = (info.input_per_1k_inr(rate) + info.output_per_1k_inr(rate)) / 2.0;
                out.push((name.clone(), blended));
            }
        }
    }
    out
}

/// Only recognised models are shown. When none of the candidates are known,
/// nothing is printed so default output is unchanged.
fn render_known_models(rate: f64, candidates: &[String], active: Option<&str>) {
    // Resolve, de-duplicating registry hits so two aliases of one model
    // (e.g. `gpt-4o` and `openai/gpt-4o`) don't list twice.
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
        return;
    }

    println!();
    println!(
        "  {}",
        style(label("cost.models", "Known models (cost / 1K tokens)")).bold()
    );
    println!(
        "  {}",
        crate::theme::muted(label(
            "cost.models_note",
            "reference list prices, converted at the active rate",
        ))
    );
    for (name, info, is_active) in &rows {
        let marker = if *is_active { "*" } else { " " };
        // ₹ per 1K input / output tokens at the active rate.
        let in_inr = format_inr_compact(info.input_per_1k_inr(rate));
        let out_inr = format_inr_compact(info.output_per_1k_inr(rate));
        println!(
            "  {} {:<24} {:<24} {:>8} ctx   in {} / out {}",
            style(marker).green().bold(),
            safe_truncate(name, 24),
            crate::theme::muted(info.provider),
            format_tokens(info.context_window),
            crate::theme::success(in_inr),
            crate::theme::success(out_inr),
        );
    }
    if rows.iter().any(|(_, _, active)| *active) {
        println!(
            "  {}",
            crate::theme::muted(label("cost.models_active", "* active model"))
        );
    }
}

/// Entry point for `bharatcode cost`.
pub async fn handle_cost(opts: CostOptions) -> anyhow::Result<()> {
    let ledger = cost_ledger::build_ledger().await?;

    // Compact dashboard view: opt-in via the `BHARATCODE_COST_DASHBOARD` env
    // gate only (the `--dashboard` clap flag lives on the `cost` CLI surface,
    // which is owned by another version; this gate is the in-binary call site
    // until that flag lands and can simply OR into the condition here). Builds on
    // the same ledger, derives a "top models by spend" list from the resolved
    // candidates, prints the themed panel via `theme::neutral`, and returns early
    // so plain `bharatcode cost` is byte-identical when the gate is unset.
    if cost_dashboard::is_enabled() {
        let candidates = candidate_models().await;
        let models = top_models_by_spend(ledger.rate, &candidates);
        print!(
            "{}",
            crate::theme::neutral(cost_dashboard::render_dashboard(
                &ledger,
                ledger.rate,
                &models,
            ))
        );
        return Ok(());
    }

    println!();
    println!(
        "  {}",
        crate::theme::heading(label("cost.title", "BharatCode cost ledger (INR)"))
    );
    println!(
        "  {}",
        crate::theme::muted(
            label(
                "cost.rate",
                "USD -> INR rate: {rate}  (set BHARATCODE_USD_INR to override)",
            )
            .replace("{rate}", &format!("{:.2}", ledger.rate))
        )
    );
    println!();

    // Headline totals.
    println!(
        "  {:<12} {}",
        style(label("cost.total", "Total spend")).bold(),
        style(fmt_inr(ledger.total_inr)).green().bold(),
    );
    println!(
        "  {:<12} {}",
        style(label("cost.today", "Today")).bold(),
        crate::theme::success(fmt_inr(ledger.today_inr())),
    );
    println!(
        "  {:<12} {}",
        style(label("cost.this_month", "This month")).bold(),
        crate::theme::success(fmt_inr(ledger.this_month_inr())),
    );
    println!(
        "  {:<12} {}",
        style(label("cost.in_usd", "In USD")).dim(),
        style(format!("${:.4}", ledger.total_usd)).dim(),
    );

    // Registry-backed model metadata: context window + ₹/1K cost for the
    // active and previously-used models that the static registry recognises.
    let active_model = Config::global().get_bharatcode_model().ok();
    let candidates = candidate_models().await;
    render_known_models(ledger.rate, &candidates, active_model.as_deref());

    // Optional "Recent patch activity" footer. Rendered only when a session
    // patch sidecar exists and holds an apply-patch envelope; absent that data
    // nothing is printed, keeping the default cost output byte-identical.
    if let Some(envelope) = patch_stats::recent_patch_envelope() {
        let stats = patch_stats::parse_patch_stats(&envelope);
        if !stats.is_empty() {
            println!(
                "  {} {}",
                style(label("cost.patch_activity", "Recent patch activity:")).bold(),
                crate::theme::muted(patch_stats::render_diffstat(&stats)),
            );
        }
    }

    // Opt-in "Extensions in use" attribution footer (BHARATCODE_COST_EXTENSIONS).
    // Lists installed plugins + configured MCP extensions by name only so a user
    // can see what third-party surface their spend ran through. Disabled by
    // default and silent when nothing is installed => cost output unchanged.
    if let Some(f) = cost_extensions::extensions_footer() {
        println!("{f}");
    }

    // Opt-in, strictly-local usage analytics footer (BHARATCODE_ANALYTICS).
    // Summarizes the locally-aggregated usage counters (turns, tool-call
    // counts, tokens, ₹ spend by day) as one muted block. Default OFF => None
    // => nothing printed, so the cost output is byte-identical. Rendered here,
    // before the empty-sessions early return, so it shows with or without spend.
    if let Some(f) = usage_analytics::analytics_footer() {
        println!("{f}");
    }

    // Opt-in, strictly-local aggregated usage footer (BHARATCODE_ANALYTICS_LOCAL).
    // Renders ONE muted line of monotonic counters (turns, tool calls, sessions,
    // tokens, days active) read from a single rolling JSON aggregate under the
    // config dir. No network, no per-event detail. Default OFF => `usage_footer`
    // returns None => nothing printed and no file is touched, so the cost output
    // is byte-identical. Rendered here, before the empty-sessions early return,
    // so it shows with or without recorded spend.
    if let Some(line) = analytics_local::usage_footer() {
        println!("{}", crate::theme::muted(&line));
    }

    // Optional storage-health footer. Rendered only when the session store
    // (`sessions.db`) exists and is non-empty; absent or empty, nothing is
    // printed, keeping the default cost output byte-identical. Read-only: it
    // stats the DB + its WAL sidecar and queries the session read API.
    if let Some(line) = db_health::storage_footer().await {
        println!("  {}", crate::theme::muted(line));
    }

    // Optional GA release-profile footer. Rendered only when
    // `BHARATCODE_COST_RELEASE` is truthy; unset/falsey => nothing printed, so
    // the default cost output stays byte-identical. Read-only: it reports only
    // compile-time build identity (version + profile + GA channel).
    if let Some(line) = release_profile::release_footer() {
        println!("  {}", crate::theme::muted(line));
    }

    // CI-only machine-readable footer + GitHub Actions annotation. Emitted only
    // under CI (env CI / GITHUB_ACTIONS / BHARATCODE_CI); with no CI signal this
    // whole block is skipped and the human output above is byte-identical.
    if ci_report::is_ci() {
        // The ₹ budget cap, read the same way the budget gate does (config key
        // or matching env var); absent / non-positive => no cap.
        let budget_inr =
            match Config::global().get_param::<f64>(crate::commands::budget::BUDGET_INR_KEY) {
                Ok(v) if v.is_finite() && v > 0.0 => Some(v),
                _ => None,
            };
        let day_inr = ledger.today_inr();
        let month_inr = ledger.this_month_inr();
        let over_budget = budget_inr.map(|cap| day_inr > cap).unwrap_or(false);

        println!(
            "{}",
            ci_report::ci_footer_json(day_inr, month_inr, budget_inr)
        );
        let msg = label(
            "cost.ci_summary",
            "BharatCode spend: today {day}, month {month}",
        )
        .replace("{day}", &fmt_inr(day_inr))
        .replace("{month}", &fmt_inr(month_inr));
        if let Some(annotation) = ci_report::github_annotation(over_budget, &msg) {
            println!("{annotation}");
        }
    }

    if ledger.sessions.is_empty() {
        println!();
        println!(
            "  {}",
            crate::theme::muted(label(
                "cost.empty",
                "No recorded LLM spend yet. Run a session to start the ledger.",
            ))
        );
        return Ok(());
    }

    // Per-session breakdown (most recent first).
    println!();
    let shown: Vec<&SessionCost> = if opts.all {
        ledger.sessions.iter().collect()
    } else {
        ledger.sessions.iter().take(opts.limit.max(1)).collect()
    };
    println!(
        "  {}",
        style(
            label("cost.sessions", "Sessions ({shown} of {total})")
                .replace("{shown}", &shown.len().to_string())
                .replace("{total}", &ledger.sessions.len().to_string())
        )
        .bold()
    );
    for s in &shown {
        let when = s.updated_at.format("%Y-%m-%d").to_string();
        let name = if s.name.is_empty() {
            s.id.as_str()
        } else {
            s.name.as_str()
        };
        println!(
            "    {}  {:<width$}  {}",
            style(when).dim(),
            safe_truncate(name, NAME_WIDTH),
            crate::theme::success(format_inr_compact(s.inr)),
            width = NAME_WIDTH,
        );
    }
    if !opts.all && ledger.sessions.len() > shown.len() {
        println!(
            "    {}",
            crate::theme::muted(
                label(
                    "cost.more",
                    "... {count} more (use --all to show every session)",
                )
                .replace(
                    "{count}",
                    &(ledger.sessions.len() - shown.len()).to_string()
                )
            )
        );
    }

    // Monthly history (most recent month first).
    println!();
    println!("  {}", style(label("cost.by_month", "By month")).bold());
    for (month, inr) in ledger.by_month.iter().rev() {
        println!(
            "    {}  {}",
            style(month).dim(),
            crate::theme::success(fmt_inr(*inr)),
        );
    }

    Ok(())
}
