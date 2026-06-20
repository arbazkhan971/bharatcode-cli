//! Security hardening self-audit (BharatCode v94).
//!
//! A reachable, inert library API that snapshots the *effective* security
//! posture of the running binary and renders a per-pillar report plus a single
//! 0..=100 hardening score. Read-only by design: it inspects each feature
//! through that feature's own canonical accessor (so the audit can never
//! disagree with what is actually enforced) and mutates nothing — no env, no
//! config, no global state — so default behaviour is byte-for-byte unchanged.
//!
//! The public entry point is [`audit`], equivalent to
//! `evaluate(&Posture::capture())`. [`Posture::capture`] reads the live posture;
//! [`evaluate`] is a pure, deterministic function over its input so the scoring
//! and findings are trivially unit-testable without touching any global.

use crate::residency::ResidencyMode;

/// Local English-fallback label helper. The `tr!` macro lives in the CLI crate,
/// not here, so user-facing labels are inlined; this mirrors the established
/// sibling pattern (`refactor.rs` / `web_search.rs`) so localized labels can
/// layer in later without touching call sites.
macro_rules! label {
    ($_key:expr, $default:expr) => {
        $default
    };
}

/// Severity of a single audit finding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// The pillar is engaged / hardened.
    Info,
    /// The pillar is relaxed and could be tightened.
    Warn,
}

/// One security pillar's finding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// Stable identifier for the pillar (e.g. `"sandbox"`).
    pub pillar: &'static str,
    /// Severity: `Info` = engaged, `Warn` = relaxed.
    pub severity: Severity,
    /// One-line, brand-free summary of the pillar's effective state.
    pub message: String,
}

/// Sandbox enforcement level mirrored from the shell tool's `BHARATCODE_SANDBOX`
/// parsing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxLevel {
    /// No sandbox — the exec tool runs unconstrained.
    Off,
    /// Read-only filesystem.
    ReadOnly,
    /// Writes confined to the workspace.
    WorkspaceWrite,
}

/// Snapshot of the effective security posture, captured through each feature's
/// own canonical accessor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Posture {
    /// Anonymous telemetry: `true` only when the user has explicitly opted in.
    pub telemetry: bool,
    /// Offline mode (network egress blocked).
    pub offline: bool,
    /// Data-residency enforcement mode.
    pub residency: ResidencyMode,
    /// Secret redaction in tool output.
    pub redaction: bool,
    /// Exec sandbox level.
    pub sandbox: SandboxLevel,
    /// Tool-approval posture, when explicitly configured. `None` = unset
    /// (engine default, not the full-access "yolo" posture).
    pub approval_full: bool,
}

impl Posture {
    /// Snapshot the live, effective posture through each feature's own accessor.
    pub fn capture() -> Self {
        // Telemetry: "not chosen" is treated as off.
        let telemetry = crate::posthog::get_telemetry_choice().unwrap_or(false);
        let offline = crate::offline::is_offline();
        let residency = crate::residency::residency_mode();
        let redaction = crate::agents::platform_extensions::developer::redact::is_enabled();
        let sandbox = Self::sandbox_from_env();
        // Full-access ("yolo") only when explicitly configured as such.
        let approval_full = crate::permission::approval_mode::ApprovalMode::from_config()
            .map(|m| m.is_yolo())
            .unwrap_or(false);

        Posture {
            telemetry,
            offline,
            residency,
            redaction,
            sandbox,
            approval_full,
        }
    }

    /// Parse the exec sandbox level from `BHARATCODE_SANDBOX`, mirroring the
    /// shell tool's own parsing (off / read-only / workspace-write).
    fn sandbox_from_env() -> SandboxLevel {
        match std::env::var("BHARATCODE_SANDBOX") {
            Ok(raw) => {
                let v = raw.trim().to_ascii_lowercase().replace('_', "-");
                match v.as_str() {
                    "read-only" | "readonly" | "ro" => SandboxLevel::ReadOnly,
                    "workspace-write" | "workspace" | "write" => SandboxLevel::WorkspaceWrite,
                    _ => SandboxLevel::Off,
                }
            }
            Err(_) => SandboxLevel::Off,
        }
    }
}

/// The aggregate audit result: one finding per pillar plus a 0..=100 hardening
/// score (higher is more hardened).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditReport {
    /// One finding per security pillar.
    pub findings: Vec<Finding>,
    /// Overall hardening score in `0..=100`.
    pub score: u8,
}

/// The reachable entry point: snapshot the live posture and evaluate it.
pub fn audit() -> AuditReport {
    evaluate(&Posture::capture())
}

/// Pure scoring weights. Sandbox and residency are weighted highest; the total
/// is normalized to 0..=100 below.
const W_SANDBOX: u32 = 30;
const W_RESIDENCY: u32 = 25;
const W_REDACTION: u32 = 15;
const W_OFFLINE: u32 = 10;
const W_APPROVAL: u32 = 10;
const W_TELEMETRY: u32 = 10;
const W_TOTAL: u32 = W_SANDBOX + W_RESIDENCY + W_REDACTION + W_OFFLINE + W_APPROVAL + W_TELEMETRY;

/// Evaluate a posture into a report. Deterministic over its input.
pub fn evaluate(posture: &Posture) -> AuditReport {
    let mut findings = Vec::with_capacity(6);
    let mut earned: u32 = 0;

    // Sandbox.
    let (sev, msg, pts) = match posture.sandbox {
        SandboxLevel::Off => (
            Severity::Warn,
            label!("audit.sandbox.off", "sandbox OFF").to_string(),
            0,
        ),
        SandboxLevel::ReadOnly => (
            Severity::Info,
            label!("audit.sandbox.ro", "sandbox read-only").to_string(),
            W_SANDBOX,
        ),
        SandboxLevel::WorkspaceWrite => (
            Severity::Info,
            label!("audit.sandbox.ws", "sandbox workspace-write").to_string(),
            W_SANDBOX,
        ),
    };
    findings.push(Finding {
        pillar: "sandbox",
        severity: sev,
        message: msg,
    });
    earned += pts;

    // Residency. Warn earns half credit.
    let (sev, msg, pts) = match posture.residency {
        ResidencyMode::Off => (
            Severity::Warn,
            label!("audit.residency.off", "residency off").to_string(),
            0,
        ),
        ResidencyMode::Warn => (
            Severity::Info,
            label!("audit.residency.warn", "residency warn").to_string(),
            W_RESIDENCY / 2,
        ),
        ResidencyMode::Strict => (
            Severity::Info,
            label!("audit.residency.strict", "residency strict").to_string(),
            W_RESIDENCY,
        ),
    };
    findings.push(Finding {
        pillar: "residency",
        severity: sev,
        message: msg,
    });
    earned += pts;

    // Redaction.
    let (sev, msg, pts) = if posture.redaction {
        (
            Severity::Info,
            label!("audit.redaction.on", "secret redaction ON").to_string(),
            W_REDACTION,
        )
    } else {
        (
            Severity::Warn,
            label!("audit.redaction.off", "secret redaction OFF").to_string(),
            0,
        )
    };
    findings.push(Finding {
        pillar: "redaction",
        severity: sev,
        message: msg,
    });
    earned += pts;

    // Offline.
    let (sev, msg, pts) = if posture.offline {
        (
            Severity::Info,
            label!("audit.offline.on", "offline mode ON").to_string(),
            W_OFFLINE,
        )
    } else {
        (
            Severity::Warn,
            label!("audit.offline.off", "offline mode off").to_string(),
            0,
        )
    };
    findings.push(Finding {
        pillar: "offline",
        severity: sev,
        message: msg,
    });
    earned += pts;

    // Approval. Full-access ("yolo") earns no credit.
    let (sev, msg, pts) = if posture.approval_full {
        (
            Severity::Warn,
            label!("audit.approval.full", "approval full-access").to_string(),
            0,
        )
    } else {
        (
            Severity::Info,
            label!("audit.approval.gated", "approval gated").to_string(),
            W_APPROVAL,
        )
    };
    findings.push(Finding {
        pillar: "approval",
        severity: sev,
        message: msg,
    });
    earned += pts;

    // Telemetry. Off is the hardened state.
    let (sev, msg, pts) = if posture.telemetry {
        (
            Severity::Warn,
            label!("audit.telemetry.on", "telemetry ON").to_string(),
            0,
        )
    } else {
        (
            Severity::Info,
            label!("audit.telemetry.off", "telemetry OFF").to_string(),
            W_TELEMETRY,
        )
    };
    findings.push(Finding {
        pillar: "telemetry",
        severity: sev,
        message: msg,
    });
    earned += pts;

    let score = ((earned * 100) / W_TOTAL) as u8;
    AuditReport { findings, score }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relaxed() -> Posture {
        Posture {
            telemetry: true,
            offline: false,
            residency: ResidencyMode::Off,
            redaction: false,
            sandbox: SandboxLevel::Off,
            approval_full: true,
        }
    }

    fn hardened() -> Posture {
        Posture {
            telemetry: false,
            offline: true,
            residency: ResidencyMode::Strict,
            redaction: true,
            sandbox: SandboxLevel::WorkspaceWrite,
            approval_full: false,
        }
    }

    #[test]
    fn relaxed_scores_zero_hardened_scores_full() {
        assert_eq!(evaluate(&relaxed()).score, 0);
        assert_eq!(evaluate(&hardened()).score, 100);
    }

    #[test]
    fn exactly_one_finding_per_pillar() {
        let report = evaluate(&hardened());
        assert_eq!(report.findings.len(), 6);
        let mut pillars: Vec<&str> = report.findings.iter().map(|f| f.pillar).collect();
        pillars.sort_unstable();
        pillars.dedup();
        assert_eq!(pillars.len(), 6);
    }

    #[test]
    fn residency_warn_earns_partial_credit() {
        let mut p = relaxed();
        p.residency = ResidencyMode::Warn;
        let warn_score = evaluate(&p).score;
        let mut off = relaxed();
        off.residency = ResidencyMode::Off;
        let off_score = evaluate(&off).score;
        let mut strict = relaxed();
        strict.residency = ResidencyMode::Strict;
        let strict_score = evaluate(&strict).score;
        assert!(off_score < warn_score, "warn should beat off");
        assert!(warn_score < strict_score, "strict should beat warn");
    }

    #[test]
    fn full_approval_earns_no_credit() {
        let mut yolo = hardened();
        yolo.approval_full = true;
        let mut gated = hardened();
        gated.approval_full = false;
        assert!(evaluate(&yolo).score < evaluate(&gated).score);
    }

    #[test]
    fn hardening_is_monotonic_in_redaction() {
        let mut off = relaxed();
        off.redaction = false;
        let mut on = relaxed();
        on.redaction = true;
        assert!(evaluate(&off).score < evaluate(&on).score);
    }

    #[test]
    fn score_is_bounded_and_pure() {
        let r1 = evaluate(&hardened());
        let r2 = evaluate(&hardened());
        assert_eq!(r1, r2);
        assert!(r1.score <= 100);
    }

    #[test]
    fn messages_are_brand_free() {
        for p in [relaxed(), hardened()] {
            for f in evaluate(&p).findings {
                let lower = f.message.to_lowercase();
                assert!(!lower.contains("goose"), "brand leak: {}", f.message);
                assert!(!lower.contains("block"), "brand leak: {}", f.message);
            }
        }
    }
}
