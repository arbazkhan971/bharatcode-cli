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
use goose::config::Config;
use goose::model_registry::{self, ModelInfo};
use goose::session::SessionManager;
use goose::utils::safe_truncate;

use crate::commands::cost_ledger::{self, format_inr, format_inr_compact, SessionCost};

// Apply-patch hunk stats, declared inline so the helper lives next to its only
// caller without widening the `commands` module surface. Used below to render a
// "Recent patch activity" footer when a session patch sidecar is present.
#[path = "patch_stats.rs"]
mod patch_stats;

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
        style(format_inr(ledger.total_inr)).green().bold(),
    );
    println!(
        "  {:<12} {}",
        style(label("cost.today", "Today")).bold(),
        crate::theme::success(format_inr(ledger.today_inr())),
    );
    println!(
        "  {:<12} {}",
        style(label("cost.this_month", "This month")).bold(),
        crate::theme::success(format_inr(ledger.this_month_inr())),
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
            crate::theme::success(format_inr(*inr)),
        );
    }

    Ok(())
}
