//! Final release-gate orchestrator (BharatCode v100).
//!
//! A single read-only aggregator that *composes* the existing release-posture
//! signals — already implemented and independently tested elsewhere — into one
//! [`GateReport`] verdict. It re-reads the REAL signals via their existing
//! accessors; it never duplicates their logic, mutates state, or touches the
//! network:
//!
//! 1. **Telemetry off** — telemetry must be disabled
//!    ([`crate::posthog::is_telemetry_enabled`] `== false`).
//! 2. **Offline / residency strict** — the effective residency posture is
//!    [`crate::residency::ResidencyMode::Strict`]
//!    ([`crate::offline::effective_residency_mode`]). This pillar is only
//!    *required* when the offline switch is engaged; otherwise it is reported as
//!    informational and never blocks the gate, so default behaviour is unchanged.
//! 3. **Compliance files present** — the Apache-2.0 derivative-work files exist
//!    and are non-empty at the repository root (`LICENSE`, `NOTICE`,
//!    `MODIFICATIONS.md`).
//! 4. **Leak-free** — the product identity string carries no residual upstream
//!    trademark, checked through the v99 compliance scanner
//!    ([`crate::agents::platform_extensions::developer::compliance::scan_for_marks`]).
//!
//! [`evaluate`] returns the composed verdict; `ready` is the logical AND of the
//! *required* pillars. The feature is fully opt-in: [`is_enabled`] reads the
//! `BHARATCODE_RELEASE_GATE` environment variable (default **off**), so with the
//! flag unset nothing is surfaced and default behaviour is unchanged.

use std::path::{Path, PathBuf};

use crate::agents::platform_extensions::developer::compliance::{scan_for_marks, DEFAULT_ALLOW};
use crate::residency::ResidencyMode;

/// Environment / config key for the opt-in release-gate notification.
pub const RELEASE_GATE_KEY: &str = "BHARATCODE_RELEASE_GATE";

/// Compliance files required at the repository root for the "compliance present"
/// pillar. These mirror the Apache-2.0 derivative-work obligations.
const COMPLIANCE_FILES: &[&str] = &["LICENSE", "NOTICE", "MODIFICATIONS.md"];

/// The product identity string scanned for residual upstream trademark leakage.
/// It is brand-clean by construction; the leak pillar re-checks it through the
/// shared scanner so a future regression would flip the gate.
const IDENTITY: &str = "BharatCode";

/// The composed release-gate verdict.
///
/// Each entry in `pillars` is a `(label, ok)` pair, in evaluation order, so a
/// failing pillar can be named precisely. `ready` is the logical AND of the
/// *required* pillars (see [`evaluate`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateReport {
    /// Ordered `(label, ok)` pairs for every evaluated pillar.
    pub pillars: Vec<(String, bool)>,
    /// True when telemetry is off.
    pub telemetry_off: bool,
    /// True when all required compliance files are present and non-empty.
    pub compliance_present: bool,
    /// True when the product identity carries no residual trademark leak.
    pub leak_free: bool,
    /// Overall verdict: the AND of all required pillars.
    pub ready: bool,
}

impl GateReport {
    /// Names of the pillars that are currently failing (in evaluation order).
    pub fn failing(&self) -> Vec<&str> {
        self.pillars
            .iter()
            .filter(|(_, ok)| !*ok)
            .map(|(name, _)| name.as_str())
            .collect()
    }

    /// A single-line, human-readable summary for an end-of-turn notification.
    /// Never references any upstream brand name.
    pub fn summary_line(&self) -> String {
        if self.ready {
            return "Release gate: READY — all posture checks passed.".to_string();
        }
        let failing = self.failing();
        if failing.is_empty() {
            "Release gate: NOT READY.".to_string()
        } else {
            format!("Release gate: NOT READY — failing: {}.", failing.join(", "))
        }
    }
}

/// Whether the release-gate notification is enabled. Off by default.
///
/// Reads the raw `BHARATCODE_RELEASE_GATE` environment variable directly (only
/// explicit truthy strings enable it) before falling back to the global config
/// parameter of the same name. The raw-string read deliberately sidesteps the
/// numeric-`1` coercion quirk in the typed config getter.
pub fn is_enabled() -> bool {
    if let Ok(raw) = std::env::var(RELEASE_GATE_KEY) {
        return is_truthy(&raw);
    }
    crate::config::Config::global()
        .get_param::<bool>(RELEASE_GATE_KEY)
        .unwrap_or(false)
}

fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Best-effort repository root: walk up from the current directory until a
/// `LICENSE` file is found, falling back to the current directory. Read-only.
fn repo_root() -> PathBuf {
    let start = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut dir: &Path = start.as_path();
    loop {
        if dir.join("LICENSE").is_file() {
            return dir.to_path_buf();
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => break,
        }
    }
    start
}

/// True when telemetry is currently disabled. When the `telemetry` feature is
/// compiled out telemetry can never run, so this is always `true`.
fn telemetry_off() -> bool {
    #[cfg(feature = "telemetry")]
    {
        !crate::posthog::is_telemetry_enabled()
    }
    #[cfg(not(feature = "telemetry"))]
    {
        true
    }
}

/// True when every required compliance file under `root` exists and is non-empty.
fn compliance_files_present(root: &Path) -> bool {
    COMPLIANCE_FILES.iter().all(|name| {
        std::fs::metadata(root.join(name))
            .map(|m| m.is_file() && m.len() > 0)
            .unwrap_or(false)
    })
}

/// True when the product identity carries no residual upstream trademark, per
/// the shared v99 scanner.
fn leak_free() -> bool {
    scan_for_marks(IDENTITY, DEFAULT_ALLOW).is_empty()
}

/// Compose the live release-posture signals into a [`GateReport`] (read-only).
///
/// `ready` is the AND of the required pillars. The residency/offline pillar is
/// only *required* when the offline switch is engaged; off the offline path it
/// is reported informationally and never blocks the gate, keeping default
/// behaviour unchanged.
pub fn evaluate() -> GateReport {
    evaluate_at(&repo_root())
}

/// [`evaluate`] against an explicit repository root, used by tests so the
/// compliance-file pillar can be exercised against a temporary tree.
pub fn evaluate_at(root: &Path) -> GateReport {
    let telemetry_off = telemetry_off();
    let compliance_present = compliance_files_present(root);
    let leak_free = leak_free();

    let offline_on = crate::offline::is_offline();
    let residency_strict = crate::offline::effective_residency_mode() == ResidencyMode::Strict;

    let mut pillars = vec![
        ("telemetry-off".to_string(), telemetry_off),
        ("compliance-present".to_string(), compliance_present),
        ("leak-free".to_string(), leak_free),
    ];

    let mut ready = telemetry_off && compliance_present && leak_free;

    if offline_on {
        pillars.push(("residency-strict".to_string(), residency_strict));
        ready = ready && residency_strict;
    }

    GateReport {
        pillars,
        telemetry_off,
        compliance_present,
        leak_free,
        ready,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_file(dir: &Path, name: &str, contents: &str) {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    fn seed_compliance(dir: &Path) {
        for name in COMPLIANCE_FILES {
            write_file(dir, name, "non-empty\n");
        }
    }

    #[test]
    fn is_truthy_only_for_explicit_on_values() {
        for v in ["1", "true", "TRUE", " yes ", "on"] {
            assert!(is_truthy(v), "{v:?} should be truthy");
        }
        for v in ["", "0", "false", "no", "off", "2", "nope"] {
            assert!(!is_truthy(v), "{v:?} should not be truthy");
        }
    }

    #[test]
    fn ready_when_telemetry_off_and_compliance_present() {
        // Telemetry is compiled off / disabled by default in this build, so the
        // telemetry pillar is satisfied without touching global state.
        assert!(telemetry_off());

        let tmp =
            std::env::temp_dir().join(format!("bc_release_gate_ready_{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        seed_compliance(&tmp);

        let report = evaluate_at(&tmp);
        assert!(report.telemetry_off);
        assert!(report.compliance_present);
        assert!(report.leak_free);
        assert!(report.ready, "expected ready: {:?}", report.pillars);
        assert!(report.failing().is_empty());

        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn not_ready_and_pillar_named_when_notice_missing() {
        let tmp =
            std::env::temp_dir().join(format!("bc_release_gate_missing_{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        seed_compliance(&tmp);
        fs::remove_file(tmp.join("NOTICE")).unwrap();

        let report = evaluate_at(&tmp);
        assert!(!report.compliance_present);
        assert!(!report.ready);
        assert!(
            report.failing().contains(&"compliance-present"),
            "missing pillar should be reported: {:?}",
            report.pillars
        );
        assert!(report.summary_line().contains("NOT READY"));

        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn leak_pillar_passes_for_clean_identity() {
        assert!(leak_free(), "{IDENTITY:?} must carry no residual mark");
    }

    #[test]
    fn summary_line_ready_when_all_pass() {
        let tmp =
            std::env::temp_dir().join(format!("bc_release_gate_summary_{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        seed_compliance(&tmp);

        let report = evaluate_at(&tmp);
        assert_eq!(
            report.summary_line(),
            "Release gate: READY — all posture checks passed."
        );

        fs::remove_dir_all(&tmp).ok();
    }
}
