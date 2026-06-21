//! Security posture deep check for `bharatcode doctor` (BharatCode v96).
//!
//! A read-only, always-visible section that audits the *effective* runtime
//! security settings and prints one status row per pillar plus an aggregate:
//!
//! - **Sandbox** — the opt-in exec sandbox level (`BHARATCODE_SANDBOX`).
//! - **Egress guard** — the data-residency mode plus the offline / no-egress
//!   switch (`BHARATCODE_RESIDENCY` + `BHARATCODE_OFFLINE`).
//! - **Secret redaction** — egress secret redaction of shell output
//!   (`BHARATCODE_REDACT`).
//! - **Exec policy** — the allow/deny command-prefix screen
//!   (`BHARATCODE_EXEC_POLICY`).
//! - **Telemetry** — whether telemetry is off (the privacy-preserving default).
//! - **Config permissions** — whether the config directory is world-writable.
//!
//! The audit is split into two halves so the decision logic is unit-testable
//! without touching the environment:
//!
//! - [`gather`] is a thin reader that captures the live settings through the
//!   *same* accessors / env vars the features themselves consult, returning a
//!   plain [`SecurityPosture`] value.
//! - [`security_rows`] is pure over that captured input: given a posture it
//!   produces the display rows, so a fully-locked-down posture can be asserted
//!   all-`Ok` and a relaxed one asserted to warn on exactly the relaxed pillars.
//!
//! Like the other deep checks, every probe is best-effort and never blocks,
//! never mutates config / files / the network, and never gates the doctor run.

use crate::commands::doctor_checks::Status;

/// Look up a user-facing string through the i18n layer, falling back to the
/// English `default` when the active locale table has no entry for `key`.
///
/// `tr!` echoes the key back when it is missing, so an unchanged key is treated
/// as "untranslated". This mirrors the helper in `doctor.rs`, keeping these rows
/// renderable in English without depending on the i18n table (owned elsewhere).
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// A snapshot of the effective security settings, captured by [`gather`] from
/// the live environment / config. Kept as plain data so [`security_rows`] can be
/// exercised against hand-built postures with no I/O.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecurityPosture {
    /// Exec sandbox level read from `BHARATCODE_SANDBOX`.
    pub sandbox: SandboxLevel,
    /// Data-residency mode read from `BHARATCODE_RESIDENCY`.
    pub residency: ResidencyLevel,
    /// Offline / no-egress switch (`BHARATCODE_OFFLINE`).
    pub offline: bool,
    /// Egress secret redaction enabled (`BHARATCODE_REDACT`).
    pub redaction: bool,
    /// Exec-policy command screen enabled (`BHARATCODE_EXEC_POLICY` points at a
    /// usable policy path rather than being unset / `off`).
    pub exec_policy: bool,
    /// Telemetry is off (the privacy-preserving default).
    pub telemetry_off: bool,
    /// Whether the config directory is world-writable; `None` when the mode
    /// could not be determined (e.g. dir missing, or a non-unix platform).
    pub config_dir_world_writable: Option<bool>,
}

/// Exec sandbox level, mirroring the shell tool's `BHARATCODE_SANDBOX` parsing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxLevel {
    Off,
    ReadOnly,
    WorkspaceWrite,
}

/// Data-residency level, mirroring `bharatcode_core::residency::ResidencyMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResidencyLevel {
    Off,
    Warn,
    Strict,
}

/// One line in the Security section: a status glyph, a human label, and a short
/// detail string rendered next to it by the doctor command.
pub struct SecurityRow {
    pub status: Status,
    pub label: String,
    pub detail: String,
}

/// Read the live security settings into a [`SecurityPosture`].
///
/// Every field is read through the same env var / accessor the corresponding
/// feature consults, so the audit reflects what is actually enforced rather than
/// a parallel notion of the settings. Pure-data out; no mutation.
pub fn gather() -> SecurityPosture {
    SecurityPosture {
        sandbox: sandbox_level(),
        residency: residency_level(),
        offline: bharatcode_core::offline::is_offline(),
        redaction: env_truthy("BHARATCODE_REDACT"),
        exec_policy: exec_policy_enabled(),
        telemetry_off: !telemetry_enabled(),
        config_dir_world_writable: config_dir_mode_warn(),
    }
}

/// Build the Security section rows in display order, plus a trailing aggregate.
///
/// Pure over the captured `posture`: the same input always yields the same rows,
/// so a locked-down posture asserts all-`Ok` and a relaxed one warns on exactly
/// the relaxed pillars. The aggregate is the worst per-pillar status.
pub fn security_rows(posture: &SecurityPosture) -> Vec<SecurityRow> {
    let mut rows = vec![
        sandbox_row(posture.sandbox),
        egress_row(posture.residency, posture.offline),
        redaction_row(posture.redaction),
        exec_policy_row(posture.exec_policy),
        telemetry_row(posture.telemetry_off),
        config_perms_row(posture.config_dir_world_writable),
    ];

    let worst = rows
        .iter()
        .map(|row| row.status)
        .max_by_key(severity)
        .unwrap_or(Status::Ok);
    rows.push(aggregate_row(worst));
    rows
}

/// Severity ordering used to fold per-pillar statuses into the aggregate:
/// `Ok` < `Warn` < `Fail`.
fn severity(status: &Status) -> u8 {
    match status {
        Status::Ok => 0,
        Status::Warn => 1,
        Status::Fail => 2,
    }
}

fn sandbox_row(level: SandboxLevel) -> SecurityRow {
    let (status, detail) = match level {
        SandboxLevel::Off => (
            Status::Warn,
            label("security.sandbox_off", "off (no exec sandbox)"),
        ),
        SandboxLevel::ReadOnly => (Status::Ok, label("security.sandbox_readonly", "read-only")),
        SandboxLevel::WorkspaceWrite => (
            Status::Ok,
            label("security.sandbox_workspace", "workspace-write"),
        ),
    };
    SecurityRow {
        status,
        label: label("security.sandbox", "Exec sandbox"),
        detail,
    }
}

fn egress_row(residency: ResidencyLevel, offline: bool) -> SecurityRow {
    // Offline mode composes residency = strict at enforcement time, so an
    // offline run is fully locked down regardless of the configured mode.
    let effective_strict = offline || residency == ResidencyLevel::Strict;
    let (status, detail) = if effective_strict {
        let note = if offline {
            label("security.egress_offline", "offline (no egress)")
        } else {
            label("security.egress_strict", "residency=strict")
        };
        (Status::Ok, note)
    } else {
        match residency {
            ResidencyLevel::Warn => (
                Status::Warn,
                label("security.egress_warn", "residency=warn (log only)"),
            ),
            _ => (
                Status::Warn,
                label("security.egress_off", "open (no egress guard)"),
            ),
        }
    };
    SecurityRow {
        status,
        label: label("security.egress", "Egress guard"),
        detail,
    }
}

fn redaction_row(enabled: bool) -> SecurityRow {
    let (status, detail) = if enabled {
        (Status::Ok, label("security.on", "on"))
    } else {
        (
            Status::Warn,
            label("security.redaction_off", "off (output not scanned)"),
        )
    };
    SecurityRow {
        status,
        label: label("security.redaction", "Secret redaction"),
        detail,
    }
}

fn exec_policy_row(enabled: bool) -> SecurityRow {
    let (status, detail) = if enabled {
        (Status::Ok, label("security.on", "on"))
    } else {
        (
            Status::Warn,
            label("security.exec_policy_off", "off (no command screen)"),
        )
    };
    SecurityRow {
        status,
        label: label("security.exec_policy", "Exec policy"),
        detail,
    }
}

fn telemetry_row(off: bool) -> SecurityRow {
    let (status, detail) = if off {
        (Status::Ok, label("security.telemetry_off", "off"))
    } else {
        (
            Status::Warn,
            label("security.telemetry_on", "on (usage data collected)"),
        )
    };
    SecurityRow {
        status,
        label: label("security.telemetry", "Telemetry"),
        detail,
    }
}

fn config_perms_row(world_writable: Option<bool>) -> SecurityRow {
    let (status, detail) = match world_writable {
        Some(true) => (
            Status::Warn,
            label("security.perms_world", "world-writable config directory"),
        ),
        Some(false) => (Status::Ok, label("security.perms_ok", "not world-writable")),
        None => (
            Status::Ok,
            label("security.perms_unknown", "not applicable"),
        ),
    };
    SecurityRow {
        status,
        label: label("security.perms", "Config permissions"),
        detail,
    }
}

fn aggregate_row(worst: Status) -> SecurityRow {
    let detail = match worst {
        Status::Ok => label("security.aggregate_ok", "all pillars hardened"),
        Status::Warn => label("security.aggregate_warn", "one or more pillars relaxed"),
        Status::Fail => label("security.aggregate_fail", "action needed"),
    };
    SecurityRow {
        status: worst,
        label: label("security.aggregate", "Overall posture"),
        detail,
    }
}

/// Resolve the sandbox level from `BHARATCODE_SANDBOX`, mirroring the shell
/// tool's parsing (`off` | `read-only` | `workspace-write`, default `off`).
fn sandbox_level() -> SandboxLevel {
    match std::env::var("BHARATCODE_SANDBOX").ok().as_deref() {
        Some("read-only") | Some("readonly") | Some("read_only") => SandboxLevel::ReadOnly,
        Some("workspace-write") | Some("workspace_write") => SandboxLevel::WorkspaceWrite,
        _ => SandboxLevel::Off,
    }
}

/// Resolve the configured residency mode through the shared accessor, then map
/// it onto the local enum so [`security_rows`] stays free of the core type.
fn residency_level() -> ResidencyLevel {
    match bharatcode_core::residency::residency_mode() {
        bharatcode_core::residency::ResidencyMode::Strict => ResidencyLevel::Strict,
        bharatcode_core::residency::ResidencyMode::Warn => ResidencyLevel::Warn,
        bharatcode_core::residency::ResidencyMode::Off => ResidencyLevel::Off,
    }
}

/// Mirror the exec-policy module's private `policy_path()` gate: the screen is
/// enabled when `BHARATCODE_EXEC_POLICY` is set to a non-empty value other than
/// `off` / `false` / `0`.
fn exec_policy_enabled() -> bool {
    match std::env::var("BHARATCODE_EXEC_POLICY") {
        Ok(value) => {
            let trimmed = value.trim();
            !(trimmed.is_empty()
                || trimmed.eq_ignore_ascii_case("off")
                || trimmed.eq_ignore_ascii_case("false")
                || trimmed == "0")
        }
        Err(_) => false,
    }
}

/// Telemetry is opt-in. It counts as enabled only when the config flag is `true`
/// and the `BHARATCODE_TELEMETRY_OFF` kill-switch is not set — same rule as the
/// doctor settings summary.
fn telemetry_enabled() -> bool {
    let off_via_env = std::env::var("BHARATCODE_TELEMETRY_OFF")
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            !v.is_empty() && v != "0" && v != "false" && v != "no"
        })
        .unwrap_or(false);
    if off_via_env {
        return false;
    }
    bharatcode_core::config::Config::global()
        .get_param::<bool>("BHARATCODE_TELEMETRY_ENABLED")
        .unwrap_or(false)
}

/// Inspect the config directory's permissions and report whether it is
/// world-writable (`unix mode & 0o002`).
///
/// Returns `Some(true)` when the directory is world-writable, `Some(false)` when
/// it exists and is not, and `None` when the mode cannot be determined — a
/// missing directory, a stat error, or a non-unix platform (where the unix mode
/// bits do not apply). Read-only: stats the directory, never modifies it.
pub fn config_dir_mode_warn() -> Option<bool> {
    let dir = bharatcode_core::config::paths::Paths::config_dir();
    dir_mode_world_writable(&dir)
}

/// World-writable check for an arbitrary path, factored out so tests can point
/// it at a fixture directory with known mode bits.
#[cfg(unix)]
fn dir_mode_world_writable(dir: &std::path::Path) -> Option<bool> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(dir).ok()?;
    Some(meta.permissions().mode() & 0o002 != 0)
}

#[cfg(not(unix))]
fn dir_mode_world_writable(_dir: &std::path::Path) -> Option<bool> {
    // The world-writable bit is a unix concept; report "not applicable".
    None
}

/// Truthiness used by the redaction gate (case-insensitive `1`/`true`/`yes`/`on`).
fn env_truthy(key: &str) -> bool {
    matches!(
        std::env::var(key)
            .ok()
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("1") | Some("true") | Some("yes") | Some("on")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fully hardened posture: every pillar at its tightest setting.
    fn locked_down() -> SecurityPosture {
        SecurityPosture {
            sandbox: SandboxLevel::WorkspaceWrite,
            residency: ResidencyLevel::Strict,
            offline: true,
            redaction: true,
            exec_policy: true,
            telemetry_off: true,
            config_dir_world_writable: Some(false),
        }
    }

    /// Render a row the way the doctor command would, exercising the
    /// `Status::glyph()` mapping so it must compile and produce a glyph.
    fn render(row: &SecurityRow) -> String {
        format!("{} {}: {}", row.status.glyph(), row.label, row.detail)
    }

    #[test]
    fn locked_down_posture_is_all_ok() {
        let rows = security_rows(&locked_down());
        // Six pillars plus the aggregate.
        assert_eq!(rows.len(), 7, "expected six pillars and one aggregate");
        for row in &rows {
            assert_eq!(
                row.status,
                Status::Ok,
                "row `{}` should be Ok in a locked-down posture",
                row.label
            );
        }
    }

    #[test]
    fn sandbox_off_and_telemetry_on_warn_others_stay_ok() {
        let mut posture = locked_down();
        posture.sandbox = SandboxLevel::Off;
        posture.telemetry_off = false; // telemetry on

        let rows = security_rows(&posture);
        let sandbox = label("security.sandbox", "Exec sandbox");
        let telemetry = label("security.telemetry", "Telemetry");
        let aggregate = label("security.aggregate", "Overall posture");

        for row in &rows {
            if row.label == sandbox || row.label == telemetry {
                assert_eq!(
                    row.status,
                    Status::Warn,
                    "relaxed pillar `{}` should warn",
                    row.label
                );
            } else if row.label == aggregate {
                // The aggregate folds in the worst pillar status.
                assert_eq!(row.status, Status::Warn, "aggregate should warn");
            } else {
                assert_eq!(
                    row.status,
                    Status::Ok,
                    "untouched pillar `{}` should stay Ok",
                    row.label
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn world_writable_dir_warns_secure_dir_ok() {
        use std::os::unix::fs::PermissionsExt;

        let open = tempfile::tempdir().unwrap();
        std::fs::set_permissions(open.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        assert_eq!(
            dir_mode_world_writable(open.path()),
            Some(true),
            "0o777 dir must be flagged world-writable"
        );

        let secure = tempfile::tempdir().unwrap();
        std::fs::set_permissions(secure.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            dir_mode_world_writable(secure.path()),
            Some(false),
            "0o700 dir must not be flagged world-writable"
        );

        // And the rows map a world-writable dir to Warn while a secure dir is Ok.
        let mut posture = locked_down();
        posture.config_dir_world_writable = Some(true);
        let perms = label("security.perms", "Config permissions");
        let perm_row = security_rows(&posture)
            .into_iter()
            .find(|row| row.label == perms)
            .expect("config permissions row");
        assert_eq!(perm_row.status, Status::Warn);
    }

    #[test]
    fn missing_mode_is_treated_as_ok() {
        let mut posture = locked_down();
        posture.config_dir_world_writable = None;
        let perms = label("security.perms", "Config permissions");
        let perm_row = security_rows(&posture)
            .into_iter()
            .find(|row| row.label == perms)
            .expect("config permissions row");
        assert_eq!(perm_row.status, Status::Ok);
    }

    #[test]
    fn labels_are_brand_free() {
        // Exercise every distinct posture path so all row labels/details render.
        let mut relaxed = locked_down();
        relaxed.sandbox = SandboxLevel::Off;
        relaxed.residency = ResidencyLevel::Off;
        relaxed.offline = false;
        relaxed.redaction = false;
        relaxed.exec_policy = false;
        relaxed.telemetry_off = false;
        relaxed.config_dir_world_writable = Some(true);

        for posture in [&locked_down(), &relaxed] {
            for row in security_rows(posture) {
                let line = render(&row).to_ascii_lowercase();
                assert!(!row.status.glyph().is_empty());
                assert!(
                    !line.contains("goose") && !line.contains("block"),
                    "security row leaked an upstream brand: {line}"
                );
            }
        }
    }
}
