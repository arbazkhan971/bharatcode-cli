//! `bharatcode privacy`: one-screen data-governance posture report.
//!
//! Surfaces, on a single screen, the *effective* data-governance posture of the
//! running binary so an operator can confirm at a glance what BharatCode will
//! and will not do with their data:
//!
//!   * **Data residency** — the egress guard mode (`off`/`warn`/`strict`) and
//!     the mode actually enforced once offline mode composes in.
//!   * **Offline / no-egress** — whether the single offline switch is on.
//!   * **Secret redaction** — whether secrets are scrubbed before logging.
//!   * **DPDP audit log** — whether the append-only audit log is on, and where.
//!   * **Telemetry** — whether any usage telemetry is enabled (always off here).
//!   * **Local-first provider** — whether the active provider is local-only.
//!   * **Egress guard** — whether the central provider egress guard is active.
//!
//! Each row is read through the *same* accessors the features themselves use —
//! [`goose::residency`], [`goose::offline`], and [`crate::commands::audit`] —
//! so the report can never drift from real behaviour. Nothing here mutates
//! state; it is a pure read of the resolved configuration.
//!
//! Original BharatCode work; not ported from any third party.

use goose::config::Config;
use goose::offline::{self, OfflineStatus};
use goose::residency::{self, ResidencyMode, RESIDENCY_MODE_KEY};

use crate::commands::audit::AUDIT_ENABLED_KEY;

/// Config / environment key toggling secret redaction before logging.
pub const REDACT_KEY: &str = "BHARATCODE_REDACT";

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

/// Read a boolean-ish config/env flag the way the privacy features do:
/// the environment variable wins (so a documented `KEY=1` switch always works),
/// then the on-disk config is consulted. Unrecognised / absent values are OFF
/// so a typo never silently flips a posture on.
fn flag_enabled(key: &str) -> bool {
    if let Ok(raw) = std::env::var(key) {
        return is_truthy(&raw);
    }
    match Config::global().get_param::<String>(key) {
        Ok(v) => is_truthy(&v),
        Err(_) => Config::global().get_param::<bool>(key).unwrap_or(false),
    }
}

fn is_truthy(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on" | "enable" | "enabled"
    )
}

/// One row of the posture report: an on/off pillar with its effective value and
/// where that value came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostureRow {
    /// Short pillar name (already localized for display).
    pub name: String,
    /// Whether this pillar is in its privacy-preserving ("good") state.
    pub good: bool,
    /// The effective value, rendered for display (e.g. `strict`, `on`, `off`).
    pub value: String,
    /// Where the value resolved from (env key, config key, or a derived note).
    pub source: String,
}

/// The fully resolved data-governance posture, read once from the live config.
///
/// Every field is derived from the same accessor the corresponding feature uses
/// at runtime, so the report reflects real behaviour rather than a parallel copy
/// of the rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivacyPosture {
    /// Configured residency mode (before offline composes in).
    pub residency: ResidencyMode,
    /// Residency mode actually enforced once offline mode is considered.
    pub effective_residency: ResidencyMode,
    /// Whether offline / no-egress mode is on.
    pub offline: bool,
    /// Whether secret redaction is on.
    pub redact: bool,
    /// Whether the DPDP audit log is on.
    pub audit: bool,
    /// Absolute path of the DPDP audit log.
    pub audit_path: String,
    /// Whether telemetry is enabled (always false in BharatCode).
    pub telemetry: bool,
    /// Active provider name.
    pub provider: String,
    /// Whether the active provider is local-only (no off-machine egress).
    pub provider_is_local: bool,
}

impl PrivacyPosture {
    /// Resolve the posture from the live configuration and feature accessors.
    pub fn resolve() -> Self {
        let config = Config::global();
        let status: OfflineStatus = offline::offline_status();
        let provider = goose::config::providers::get_active_provider(config)
            .unwrap_or_else(|| "ollama".to_string());
        let provider_is_local = offline::host_is_local(&provider)
            || provider.eq_ignore_ascii_case("ollama")
            || provider.eq_ignore_ascii_case("local");

        Self {
            residency: residency::residency_mode(),
            effective_residency: status.effective_residency,
            offline: status.enabled,
            redact: flag_enabled(REDACT_KEY),
            audit: flag_enabled(AUDIT_ENABLED_KEY),
            audit_path: crate::commands::audit::audit_log_path()
                .display()
                .to_string(),
            telemetry: status.telemetry_enabled,
            provider,
            provider_is_local,
        }
    }

    /// True when every pillar is in its strongest privacy-preserving state:
    /// residency strict, offline on, redaction on, telemetry off, local provider.
    pub fn is_fully_locked_down(&self) -> bool {
        self.effective_residency == ResidencyMode::Strict
            && self.offline
            && self.redact
            && !self.telemetry
            && self.provider_is_local
    }

    /// Render the posture as an ordered list of display rows.
    pub fn rows(&self) -> Vec<PostureRow> {
        let residency_value = match self.effective_residency {
            ResidencyMode::Off => "off",
            ResidencyMode::Warn => "warn",
            ResidencyMode::Strict => "strict",
        };
        let residency_source = if self.offline && self.residency != ResidencyMode::Strict {
            label(
                "privacy.src_offline_forced",
                "forced by offline mode (BHARATCODE_OFFLINE)",
            )
        } else {
            RESIDENCY_MODE_KEY.to_string()
        };

        vec![
            PostureRow {
                name: label("privacy.row_residency", "Data residency"),
                // Any active guard (warn or strict) counts as privacy-preserving.
                good: self.effective_residency != ResidencyMode::Off,
                value: residency_value.to_string(),
                source: residency_source,
            },
            PostureRow {
                name: label("privacy.row_offline", "Offline / no-egress"),
                good: self.offline,
                value: on_off(self.offline),
                source: offline::OFFLINE_MODE_KEY.to_string(),
            },
            PostureRow {
                name: label("privacy.row_redact", "Secret redaction"),
                good: self.redact,
                value: on_off(self.redact),
                source: REDACT_KEY.to_string(),
            },
            PostureRow {
                name: label("privacy.row_audit", "DPDP audit log"),
                good: self.audit,
                value: if self.audit {
                    format!("{} → {}", on_off(true), self.audit_path)
                } else {
                    on_off(false)
                },
                source: AUDIT_ENABLED_KEY.to_string(),
            },
            PostureRow {
                name: label("privacy.row_telemetry", "Telemetry"),
                // Telemetry OFF is the privacy-preserving state.
                good: !self.telemetry,
                value: on_off(self.telemetry),
                source: label("privacy.src_builtin", "built-in (telemetry disabled)"),
            },
            PostureRow {
                name: label("privacy.row_provider", "Local-first provider"),
                good: self.provider_is_local,
                value: self.provider.clone(),
                source: "BHARATCODE_PROVIDER".to_string(),
            },
            PostureRow {
                name: label("privacy.row_egress", "Egress guard"),
                // The central guard is always installed; it is active (enforcing)
                // whenever offline mode is on or residency is not off.
                good: self.offline || self.effective_residency != ResidencyMode::Off,
                value: if self.offline || self.effective_residency != ResidencyMode::Off {
                    label("privacy.egress_enforcing", "installed (enforcing)")
                } else {
                    label("privacy.egress_passthrough", "installed (pass-through)")
                },
                source: label("privacy.src_builtin_guard", "built-in egress guard"),
            },
        ]
    }
}

fn on_off(v: bool) -> String {
    if v {
        "on".to_string()
    } else {
        "off".to_string()
    }
}

/// Entry point for `bharatcode privacy`: render the resolved posture.
pub async fn handle_privacy() -> anyhow::Result<()> {
    let posture = PrivacyPosture::resolve();

    println!();
    println!(
        "  {}",
        crate::theme::heading(label("privacy.title", "BharatCode privacy posture"))
    );
    println!(
        "  {}",
        crate::theme::muted(label(
            "privacy.subtitle",
            "Resolved data-governance posture for this session",
        ))
    );
    println!();

    let name_width = posture
        .rows()
        .iter()
        .map(|r| r.name.chars().count())
        .max()
        .unwrap_or(20)
        .max(20);

    for row in posture.rows() {
        let mark = if row.good {
            crate::theme::success("✓".to_string())
        } else {
            crate::theme::warning("✗".to_string())
        };
        let value = if row.good {
            crate::theme::success(row.value.clone())
        } else {
            crate::theme::warning(row.value.clone())
        };
        println!(
            "  {}  {:<width$}  {}  {}",
            mark,
            row.name,
            value,
            crate::theme::muted(format!("[{}]", row.source)),
            width = name_width,
        );
    }

    println!();
    if posture.is_fully_locked_down() {
        println!(
            "  {}",
            crate::theme::success(label(
                "privacy.locked_down",
                "All pillars locked down: strict residency, offline, redaction on, telemetry off, local provider.",
            ))
        );
    } else {
        println!(
            "  {}",
            crate::theme::muted(label(
                "privacy.hint",
                "Tighten posture: BHARATCODE_OFFLINE=1, BHARATCODE_RESIDENCY=strict, BHARATCODE_REDACT=1, BHARATCODE_AUDIT=1.",
            ))
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_posture() -> PrivacyPosture {
        PrivacyPosture {
            residency: ResidencyMode::Off,
            effective_residency: ResidencyMode::Off,
            offline: false,
            redact: false,
            audit: false,
            audit_path: "/tmp/audit.jsonl".to_string(),
            telemetry: false,
            provider: "ollama".to_string(),
            provider_is_local: true,
        }
    }

    #[test]
    fn truthy_parsing_is_forgiving_but_strict_on_typos() {
        assert!(is_truthy("1"));
        assert!(is_truthy("TRUE"));
        assert!(is_truthy(" on "));
        assert!(is_truthy("yes"));
        assert!(is_truthy("enabled"));
        assert!(!is_truthy("0"));
        assert!(!is_truthy("false"));
        assert!(!is_truthy(""));
        assert!(!is_truthy("maybe"));
    }

    #[test]
    fn default_posture_is_not_fully_locked_down() {
        let p = base_posture();
        assert!(!p.is_fully_locked_down());
    }

    #[test]
    fn full_lockdown_requires_every_pillar() {
        let p = PrivacyPosture {
            effective_residency: ResidencyMode::Strict,
            offline: true,
            redact: true,
            telemetry: false,
            provider_is_local: true,
            ..base_posture()
        };
        assert!(p.is_fully_locked_down());

        // Flipping any single pillar breaks lockdown.
        assert!(!PrivacyPosture {
            telemetry: true,
            ..p.clone()
        }
        .is_fully_locked_down());
        assert!(!PrivacyPosture {
            redact: false,
            ..p.clone()
        }
        .is_fully_locked_down());
        assert!(!PrivacyPosture {
            offline: false,
            ..p.clone()
        }
        .is_fully_locked_down());
        assert!(!PrivacyPosture {
            provider_is_local: false,
            ..p.clone()
        }
        .is_fully_locked_down());
        assert!(!PrivacyPosture {
            effective_residency: ResidencyMode::Off,
            ..p
        }
        .is_fully_locked_down());
    }

    #[test]
    fn rows_cover_all_seven_pillars() {
        let rows = base_posture().rows();
        assert_eq!(rows.len(), 7);
        // Residency off, offline off, redact off, audit off, telemetry off (good),
        // provider local (good), egress pass-through.
        let by_good: Vec<bool> = rows.iter().map(|r| r.good).collect();
        assert_eq!(by_good, vec![false, false, false, false, true, true, false]);
    }

    #[test]
    fn audit_row_shows_path_when_enabled() {
        let p = PrivacyPosture {
            audit: true,
            ..base_posture()
        };
        let audit_row = p
            .rows()
            .into_iter()
            .find(|r| r.source == AUDIT_ENABLED_KEY)
            .expect("audit row present");
        assert!(audit_row.good);
        assert!(audit_row.value.contains("/tmp/audit.jsonl"));
    }

    #[test]
    fn offline_forces_strict_residency_source_note() {
        let p = PrivacyPosture {
            residency: ResidencyMode::Off,
            effective_residency: ResidencyMode::Strict,
            offline: true,
            ..base_posture()
        };
        let residency_row = &p.rows()[0];
        assert_eq!(residency_row.value, "strict");
        assert!(residency_row.good);
        // Source notes that offline forced it, not the residency key.
        assert_ne!(residency_row.source, RESIDENCY_MODE_KEY);
    }
}
