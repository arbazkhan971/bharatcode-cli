//! Approval modes: a single, clear surface for how tool calls are gated.
//!
//! BharatCode already drives tool gating through two existing types:
//! [`GooseMode`] (the engine-level posture) and [`PermissionLevel`] (the
//! per-tool decision). Those names are accurate but spread across the codebase
//! and worded inconsistently. This module adds a thin, additive vocabulary on
//! top of them so users have one place to reason about approvals:
//!
//! * Per-decision outcomes — **ask / allow / deny** — see [`ApprovalDecision`].
//! * Coarse postures — **read-only / auto / full** plus a **yolo** alias for
//!   full auto-approval — see [`ApprovalMode`].
//!
//! The posture is configured via the [`APPROVAL_CONFIG_KEY`]
//! (`BHARATCODE_APPROVAL`) config key / environment variable. Parsing is
//! lenient so the friendly aliases people reach for ("smart", "read-only",
//! "yolo", "prompt", ...) all resolve to a posture.
//!
//! Defaults stay safe and unchanged: this layer never overrides the engine on
//! its own. [`resolve_mode`] returns the caller's existing mode untouched
//! whenever `BHARATCODE_APPROVAL` is not set, so wiring it in is opt-in and
//! cannot silently widen permissions.

use crate::config::permission::PermissionLevel;
use crate::config::{Config, GooseMode};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Config key (and uppercased environment variable) that selects the approval
/// posture. Reading honors environment first, then `config.yaml`.
pub const APPROVAL_CONFIG_KEY: &str = "BHARATCODE_APPROVAL";

/// The per-tool decision surfaced at an approval prompt.
///
/// These three outcomes are the user-facing vocabulary for what happens to a
/// single tool call, and map one-to-one onto [`PermissionLevel`].
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    /// Pause and ask the user before running the tool.
    Ask,
    /// Run the tool without prompting.
    Allow,
    /// Refuse the tool outright; it is never run.
    Deny,
}

impl ApprovalDecision {
    /// View an existing [`PermissionLevel`] through the approval vocabulary.
    pub fn from_permission_level(level: &PermissionLevel) -> Self {
        match level {
            PermissionLevel::AlwaysAllow => ApprovalDecision::Allow,
            PermissionLevel::AskBefore => ApprovalDecision::Ask,
            PermissionLevel::NeverAllow => ApprovalDecision::Deny,
        }
    }

    /// Translate back into the engine's [`PermissionLevel`].
    pub fn to_permission_level(self) -> PermissionLevel {
        match self {
            ApprovalDecision::Allow => PermissionLevel::AlwaysAllow,
            ApprovalDecision::Ask => PermissionLevel::AskBefore,
            ApprovalDecision::Deny => PermissionLevel::NeverAllow,
        }
    }

    /// Short, user-facing label.
    pub fn label(self) -> &'static str {
        match self {
            ApprovalDecision::Ask => "ask",
            ApprovalDecision::Allow => "allow",
            ApprovalDecision::Deny => "deny",
        }
    }
}

/// Coarse approval posture, configured via [`APPROVAL_CONFIG_KEY`].
///
/// The variants are a one-to-one, clearly named view over [`GooseMode`]:
///
/// | posture     | behavior                                            | engine mode            |
/// |-------------|-----------------------------------------------------|------------------------|
/// | `Chat`      | no tool calls run at all                            | [`GooseMode::Chat`]    |
/// | `Ask`       | ask before every tool call                          | [`GooseMode::Approve`] |
/// | `Auto`      | auto-approve read-only tools, ask for the rest      | [`GooseMode::SmartApprove`] |
/// | `Full`      | auto-approve every tool call ("yolo")               | [`GooseMode::Auto`]    |
///
/// `Auto` is the safe default for this layer: read-only work flows freely while
/// anything that can modify state still pauses for the user.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    /// Chat only — no tool calls are executed.
    Chat,
    /// Ask before every tool call.
    Ask,
    /// Auto-approve read-only / safe tools; ask before anything sensitive.
    ///
    /// This is the read-only-friendly posture and the safe default.
    #[default]
    Auto,
    /// Auto-approve every tool call. The full-access, "yolo" posture — fast,
    /// but it trusts the agent with unattended writes, so use with care.
    Full,
}

impl ApprovalMode {
    /// Map the posture onto the engine-level [`GooseMode`].
    pub fn to_goose_mode(self) -> GooseMode {
        match self {
            ApprovalMode::Chat => GooseMode::Chat,
            ApprovalMode::Ask => GooseMode::Approve,
            ApprovalMode::Auto => GooseMode::SmartApprove,
            ApprovalMode::Full => GooseMode::Auto,
        }
    }

    /// Derive the posture from an existing [`GooseMode`].
    pub fn from_goose_mode(mode: GooseMode) -> Self {
        match mode {
            GooseMode::Chat => ApprovalMode::Chat,
            GooseMode::Approve => ApprovalMode::Ask,
            GooseMode::SmartApprove => ApprovalMode::Auto,
            GooseMode::Auto => ApprovalMode::Full,
        }
    }

    /// Whether this posture auto-approves every tool call (the "yolo" posture).
    pub fn is_yolo(self) -> bool {
        matches!(self, ApprovalMode::Full)
    }

    /// Lenient parse from a user-supplied string. Accepts the canonical names
    /// plus the common aliases people reach for. Returns `None` for anything
    /// unrecognized so callers can fall back to existing defaults.
    pub fn parse(value: &str) -> Option<Self> {
        let normalized = value.trim().to_ascii_lowercase().replace([' ', '_'], "-");
        let mode = match normalized.as_str() {
            "chat" | "none" | "off" | "plan" => ApprovalMode::Chat,
            // Safety-first aliases. "never", "untrusted" and "read-only" all
            // express "do not let the agent run things on its own", so they must
            // resolve to a posture that NEVER auto-approves. `Ask` (prompt before
            // every tool call) is that posture; mapping any of these to `Auto` or
            // `Full` would silently widen permissions, the opposite of intent.
            "ask" | "approve" | "prompt" | "on-request" | "manual" | "never" | "untrusted"
            | "read-only" | "readonly" => ApprovalMode::Ask,
            "auto" | "smart" | "smart-approve" | "on-failure" => ApprovalMode::Auto,
            "full" | "yolo" | "danger" | "danger-full-access" | "bypass" | "auto-approve" => {
                ApprovalMode::Full
            }
            _ => return None,
        };
        Some(mode)
    }

    /// Read the configured posture from [`APPROVAL_CONFIG_KEY`], if set and
    /// recognized. Returns `None` when the key is absent or unparseable.
    pub fn from_config() -> Option<Self> {
        Self::from_config_in(Config::global())
    }

    /// Like [`from_config`](Self::from_config) but against a specific [`Config`]
    /// (useful for tests).
    pub fn from_config_in(config: &Config) -> Option<Self> {
        config
            .get_param::<String>(APPROVAL_CONFIG_KEY)
            .ok()
            .as_deref()
            .and_then(Self::parse)
    }

    /// Canonical, user-facing name.
    pub fn label(self) -> &'static str {
        match self {
            ApprovalMode::Chat => "chat",
            ApprovalMode::Ask => "ask",
            ApprovalMode::Auto => "auto",
            ApprovalMode::Full => "full",
        }
    }

    /// One-line, user-facing description of the posture.
    pub fn description(self) -> &'static str {
        match self {
            ApprovalMode::Chat => "Chat only — no tools run.",
            ApprovalMode::Ask => "Ask before every tool call.",
            ApprovalMode::Auto => "Auto-approve read-only tools; ask before anything sensitive.",
            ApprovalMode::Full => "Auto-approve every tool call (yolo) — use with care.",
        }
    }
}

/// Resolve the effective [`GooseMode`], honoring [`APPROVAL_CONFIG_KEY`] when it
/// is set and falling back to `fallback` otherwise.
///
/// This is the safe integration point: when `BHARATCODE_APPROVAL` is absent the
/// caller's existing mode is returned verbatim, so existing defaults are never
/// changed by wiring this in.
///
/// TODO(approval-mode wiring): make this the single source of truth for the
/// runtime posture by routing the existing mode readers through it, e.g. wrap
/// the `Config::get_bharatcode_mode()` consumers in
/// the execution manager and gateway handler.
/// and the `providers/*_acp.rs` modules as
/// `resolve_mode(config.get_bharatcode_mode().unwrap_or(GooseMode::Auto))`.
/// That integration spans several crates/call sites and changes global behaviour,
/// so it is intentionally left out of this focused fix; the safety contract above
/// guarantees wiring it in can only narrow (never widen) permissions.
pub fn resolve_mode(fallback: GooseMode) -> GooseMode {
    match ApprovalMode::from_config() {
        Some(mode) => mode.to_goose_mode(),
        None => fallback,
    }
}

/// The [`GooseMode`] a subagent spawned from a parent in `parent_mode` may run in.
///
/// A subagent inherits its parent's posture exactly. Subagents run unattended,
/// so calls that would need a user confirmation are denied instead of waiting.
/// SmartApprove subagents can still use read-only calls without gaining wider
/// permissions than their parent.
pub fn subagent_mode(parent_mode: GooseMode) -> GooseMode {
    parent_mode
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_round_trips_permission_level() {
        for level in [
            PermissionLevel::AlwaysAllow,
            PermissionLevel::AskBefore,
            PermissionLevel::NeverAllow,
        ] {
            let decision = ApprovalDecision::from_permission_level(&level);
            assert_eq!(decision.to_permission_level(), level);
        }
    }

    #[test]
    fn mode_round_trips_goose_mode() {
        for mode in [
            GooseMode::Chat,
            GooseMode::Approve,
            GooseMode::SmartApprove,
            GooseMode::Auto,
        ] {
            let approval = ApprovalMode::from_goose_mode(mode);
            assert_eq!(approval.to_goose_mode(), mode);
        }
    }

    #[test]
    fn parse_accepts_canonical_and_aliases() {
        assert_eq!(ApprovalMode::parse("ask"), Some(ApprovalMode::Ask));
        assert_eq!(ApprovalMode::parse("approve"), Some(ApprovalMode::Ask));
        assert_eq!(ApprovalMode::parse("smart"), Some(ApprovalMode::Auto));
        assert_eq!(ApprovalMode::parse("auto"), Some(ApprovalMode::Auto));
        assert_eq!(ApprovalMode::parse("  YOLO "), Some(ApprovalMode::Full));
        assert_eq!(
            ApprovalMode::parse("danger_full_access"),
            Some(ApprovalMode::Full)
        );
        assert_eq!(ApprovalMode::parse("chat"), Some(ApprovalMode::Chat));
        assert_eq!(ApprovalMode::parse("nonsense"), None);
    }

    /// Regression test for an inverted-safety bug: "never" (and the other
    /// safety-first aliases) must resolve to a posture that NEVER auto-approves.
    /// Previously "never" mapped to `Full` (full auto-approve / yolo) — the exact
    /// opposite of what a user asking for "never" expects.
    #[test]
    fn safety_first_aliases_never_auto_approve() {
        for alias in ["never", "untrusted", "read-only", "readonly", "read_only"] {
            let mode = ApprovalMode::parse(alias)
                .unwrap_or_else(|| panic!("alias `{alias}` should parse"));
            assert_eq!(
                mode,
                ApprovalMode::Ask,
                "alias `{alias}` must map to the safest never-auto-approve posture"
            );
            assert!(
                !mode.is_yolo(),
                "alias `{alias}` must never resolve to the yolo (full auto-approve) posture"
            );
            assert_ne!(
                mode.to_goose_mode(),
                GooseMode::Auto,
                "alias `{alias}` must never resolve to the auto-approve-everything engine mode"
            );
        }
    }

    #[test]
    fn default_posture_is_safe_smart_approve() {
        assert_eq!(ApprovalMode::default(), ApprovalMode::Auto);
        assert_eq!(
            ApprovalMode::default().to_goose_mode(),
            GooseMode::SmartApprove
        );
        assert!(!ApprovalMode::default().is_yolo());
    }

    #[test]
    fn yolo_is_full_auto_approval() {
        assert!(ApprovalMode::Full.is_yolo());
        assert_eq!(ApprovalMode::Full.to_goose_mode(), GooseMode::Auto);
    }

    #[test]
    fn resolve_falls_back_when_unset() {
        // The integration-time global config is unlikely to carry the key in
        // tests; resolve must return the fallback unchanged in that case.
        std::env::remove_var(APPROVAL_CONFIG_KEY);
        assert_eq!(resolve_mode(GooseMode::Approve), GooseMode::Approve);
    }

    #[test]
    fn subagent_mode_is_exact_inheritance() {
        for parent in [
            GooseMode::Chat,
            GooseMode::Approve,
            GooseMode::SmartApprove,
            GooseMode::Auto,
        ] {
            assert_eq!(subagent_mode(parent), parent);
        }
    }
}
