//! INR budget gate for BharatCode.
//!
//! BharatCode already accounts for per-session spend in US dollars (the agent
//! runtime records an `accumulated_cost` per session) and exposes a rupee view
//! of that via [`crate::commands::cost_ledger`]. This module adds an *optional*
//! spend cap on top of that data so users can bound how much a session — or a
//! whole day — is allowed to cost before BharatCode warns or refuses to start
//! further model turns.
//!
//! Configuration (all read through `bharatcode_core::config::Config`, so each key may be
//! set in the config file or via the matching environment variable):
//!
//!   * `BHARATCODE_BUDGET_INR`  — the ₹ cap. **Absent / non-positive => the gate
//!     is fully disabled and behaviour is unchanged (default OFF).**
//!   * `BHARATCODE_BUDGET_MODE` — `warn` (default, never blocks) or `deny`
//!     (blocks the next model turn once the cap is exceeded).
//!   * `BHARATCODE_BUDGET_SCOPE` — `session` (default, this session's spend) or
//!     `day` (today's spend across all sessions, via the cost ledger).
//!
//! The gate only ever *reads* recorded cost; it never mutates session state.
//! The decision logic ([`BudgetConfig::decide`]) is pure and unit-tested; the
//! IO (config read + ledger roll-up) lives in [`gate_turn`], which the CLI
//! session loop calls once before each model turn.
//!
//! Original BharatCode work; not ported from any third party.

use bharatcode_core::config::Config;

use crate::commands::cost_ledger::{self, format_inr, usd_to_inr};

/// Config / environment key holding the ₹ spend cap. Absent => gate disabled.
pub const BUDGET_INR_KEY: &str = "BHARATCODE_BUDGET_INR";
/// Config / environment key selecting `warn` (default) or `deny` behaviour.
pub const BUDGET_MODE_KEY: &str = "BHARATCODE_BUDGET_MODE";
/// Config / environment key selecting `session` (default) or `day` scope.
pub const BUDGET_SCOPE_KEY: &str = "BHARATCODE_BUDGET_SCOPE";

/// Fraction of the cap at or above which warn nudging begins (below the cap).
pub const WARN_FRACTION: f64 = 0.8;

/// What to do when spend reaches the cap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetMode {
    /// Never block; only surface a warning.
    Warn,
    /// Block the next model turn once the cap is exceeded.
    Deny,
}

/// Which spend total the cap is measured against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetScope {
    /// Just this session's accumulated cost.
    Session,
    /// Today's spend across all sessions (UTC), via the cost ledger.
    Day,
}

impl BudgetScope {
    /// i18n key for this scope's short human label (used in messages).
    fn label_key(self) -> &'static str {
        match self {
            BudgetScope::Session => "budget.scope_session",
            BudgetScope::Day => "budget.scope_day",
        }
    }
}

/// A resolved, active budget configuration. Only constructed when a positive
/// cap is configured (see [`BudgetConfig::resolve`]).
pub struct BudgetConfig {
    /// The ₹ spend cap.
    pub cap_inr: f64,
    /// Warn vs. deny behaviour at/over the cap.
    pub mode: BudgetMode,
    /// Session vs. day measurement scope.
    pub scope: BudgetScope,
}

/// The action the gate decided on for a single model turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetAction {
    /// Comfortably under the cap; proceed silently.
    Allow,
    /// Near or over the cap (but not blocking); proceed after warning.
    Warn,
    /// Over the cap in `deny` mode; the turn must not start.
    Block,
}

/// The outcome of a budget check: an [`BudgetAction`] plus an optional
/// already-formatted, localized ₹ message for the user.
pub struct BudgetDecision {
    /// Whether to allow, warn, or block.
    pub action: BudgetAction,
    /// User-facing message (present for `Warn` and `Block`).
    pub message: Option<String>,
}

impl BudgetDecision {
    /// A silent "proceed" decision.
    fn allow() -> Self {
        Self {
            action: BudgetAction::Allow,
            message: None,
        }
    }

    /// Whether the model turn must be blocked.
    pub fn is_blocked(&self) -> bool {
        self.action == BudgetAction::Block
    }

    /// The user-facing message, if any.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }
}

impl BudgetConfig {
    /// Resolve the active budget configuration from config / environment.
    ///
    /// Returns `None` when no positive `BHARATCODE_BUDGET_INR` is configured, in
    /// which case the gate is fully disabled (default OFF — behaviour unchanged).
    pub fn resolve() -> Option<Self> {
        let config = Config::global();

        let cap_inr = match config.get_param::<f64>(BUDGET_INR_KEY) {
            Ok(v) if v.is_finite() && v > 0.0 => v,
            _ => return None,
        };

        let mode = match config.get_param::<String>(BUDGET_MODE_KEY) {
            Ok(s) if s.trim().eq_ignore_ascii_case("deny") => BudgetMode::Deny,
            _ => BudgetMode::Warn,
        };

        let scope = match config.get_param::<String>(BUDGET_SCOPE_KEY) {
            Ok(s) if s.trim().eq_ignore_ascii_case("day") => BudgetScope::Day,
            _ => BudgetScope::Session,
        };

        Some(Self {
            cap_inr,
            mode,
            scope,
        })
    }

    /// Compute the ₹ spend this gate measures against, given the current
    /// session's recorded USD cost.
    ///
    /// For [`BudgetScope::Session`] this just converts `session_usd`. For
    /// [`BudgetScope::Day`] it rolls up today's spend across all sessions via
    /// the cost ledger, falling back to the session figure if the ledger can
    /// not be built.
    pub async fn spent_inr(&self, session_usd: f64) -> f64 {
        let session_usd = if session_usd.is_finite() && session_usd > 0.0 {
            session_usd
        } else {
            0.0
        };
        match self.scope {
            BudgetScope::Session => usd_to_inr(session_usd),
            BudgetScope::Day => match cost_ledger::build_ledger().await {
                Ok(ledger) => ledger.today_inr(),
                Err(_) => usd_to_inr(session_usd),
            },
        }
    }

    /// Pure decision: given the ₹ already spent, decide whether to allow, warn,
    /// or block, and build the matching localized message.
    pub fn decide(&self, spent_inr: f64) -> BudgetDecision {
        if !spent_inr.is_finite() || self.cap_inr <= 0.0 {
            return BudgetDecision::allow();
        }

        let scope = crate::tr!(self.scope.label_key());
        let spent_s = format_inr(spent_inr);
        let cap_s = format_inr(self.cap_inr);

        if spent_inr >= self.cap_inr {
            let (action, key) = match self.mode {
                BudgetMode::Deny => (BudgetAction::Block, "budget.deny"),
                BudgetMode::Warn => (BudgetAction::Warn, "budget.over_warn"),
            };
            let message = crate::tr!(key)
                .replace("{scope}", &scope)
                .replace("{spent}", &spent_s)
                .replace("{cap}", &cap_s);
            BudgetDecision {
                action,
                message: Some(message),
            }
        } else if spent_inr >= self.cap_inr * WARN_FRACTION {
            let pct = format!("{:.0}%", (spent_inr / self.cap_inr) * 100.0);
            let message = crate::tr!("budget.warn")
                .replace("{scope}", &scope)
                .replace("{spent}", &spent_s)
                .replace("{cap}", &cap_s)
                .replace("{pct}", &pct);
            BudgetDecision {
                action: BudgetAction::Warn,
                message: Some(message),
            }
        } else {
            BudgetDecision::allow()
        }
    }
}

/// Gate a single model turn against the configured INR budget.
///
/// `session_usd` is the current session's recorded `accumulated_cost` (USD).
/// Returns [`BudgetDecision::allow`] (silent, never blocking) when no budget is
/// configured, so callers can wire this in unconditionally and keep the feature
/// off by default.
pub async fn gate_turn(session_usd: f64) -> BudgetDecision {
    match BudgetConfig::resolve() {
        None => BudgetDecision::allow(),
        Some(cfg) => {
            let spent = cfg.spent_inr(session_usd).await;
            cfg.decide(spent)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(cap: f64, mode: BudgetMode) -> BudgetConfig {
        BudgetConfig {
            cap_inr: cap,
            mode,
            scope: BudgetScope::Session,
        }
    }

    #[test]
    fn under_warn_threshold_allows_silently() {
        let d = cfg(100.0, BudgetMode::Deny).decide(50.0);
        assert_eq!(d.action, BudgetAction::Allow);
        assert!(d.message.is_none());
        assert!(!d.is_blocked());
    }

    #[test]
    fn near_cap_warns_in_either_mode() {
        for mode in [BudgetMode::Warn, BudgetMode::Deny] {
            let d = cfg(100.0, mode).decide(85.0);
            assert_eq!(d.action, BudgetAction::Warn);
            assert!(!d.is_blocked());
            assert!(d.message.unwrap().contains('₹'));
        }
    }

    #[test]
    fn over_cap_blocks_only_in_deny_mode() {
        let denied = cfg(100.0, BudgetMode::Deny).decide(120.0);
        assert_eq!(denied.action, BudgetAction::Block);
        assert!(denied.is_blocked());
        assert!(denied.message.unwrap().contains('₹'));

        let warned = cfg(100.0, BudgetMode::Warn).decide(120.0);
        assert_eq!(warned.action, BudgetAction::Warn);
        assert!(!warned.is_blocked());
    }

    #[test]
    fn non_finite_spend_is_allowed() {
        let d = cfg(100.0, BudgetMode::Deny).decide(f64::NAN);
        assert_eq!(d.action, BudgetAction::Allow);
    }

    #[test]
    fn exactly_at_cap_counts_as_over() {
        let d = cfg(100.0, BudgetMode::Deny).decide(100.0);
        assert_eq!(d.action, BudgetAction::Block);
    }
}
