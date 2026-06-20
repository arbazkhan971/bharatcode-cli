//! `bharatcode cost`: summarize LLM spend in Indian rupees.
//!
//! Surfaces the INR cost ledger (see [`crate::commands::cost_ledger`]) as a
//! human-readable summary: a headline total, today / this-month roll-ups, a
//! recent per-session breakdown, and a monthly history. All amounts are shown
//! in ₹ using the configured USD->INR rate (`BHARATCODE_USD_INR`).
//!
//! Original BharatCode work; not ported from any third party.

use console::style;
use goose::utils::safe_truncate;

use crate::commands::cost_ledger::{self, format_inr, format_inr_compact, SessionCost};

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
