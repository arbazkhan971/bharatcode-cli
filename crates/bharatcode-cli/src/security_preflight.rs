//! Read-only security-hardening startup posture preflight (BharatCode v96).
//!
//! At session build time this scores the *effective* runtime configuration
//! against a small hardening checklist and logs a single one-line posture
//! summary. It is deliberately cheap, side-effect-free and non-blocking: it
//! never touches the network, never mutates config or files, and never aborts
//! startup. The checklist mirrors the controls the privacy / residency /
//! redaction / telemetry features already enforce, read through the *same*
//! environment variables / accessors those features consult, so the score
//! reflects what is actually in effect rather than a parallel notion of it.
//!
//! Controls scored (one point each):
//!
//! - **telemetry off** — usage telemetry is not collected.
//! - **residency/offline coherence** — egress is locked down, i.e. offline mode
//!   is on *or* residency is `strict` (a `warn`/`off` residency that is not
//!   composed to strict by offline mode is a relaxed control).
//! - **sandbox available** — the opt-in exec sandbox is set to a real level
//!   (`read-only` / `workspace-write`) rather than `off`.
//! - **secret redaction on** — egress secret redaction of shell output is on.
//! - **config dir not world-writable** — the config directory is not writable
//!   by other users (`unix mode & 0o002`). Indeterminate (missing dir, stat
//!   error, non-unix) scores as a pass — absence of evidence is not a finding.
//! - **no plaintext key file** — the on-disk secrets file, if present, is not
//!   group/world-readable (`unix mode & 0o077`). Absent file => pass.
//!
//! ## Modes
//!
//! Default (env unset) is silent apart from one `tracing::info!` posture line:
//! behaviour is unchanged and startup is never blocked. With
//! `BHARATCODE_HARDENED=strict` (or `on`) the build additionally emits one
//! visible warning line per *failing* control to stderr. The summary line is
//! always one line and always carries a `N/M` score.
//!
//! The split mirrors the rest of the security tooling: [`capture`] is the thin
//! reader that snapshots the live settings, and [`Posture::score`] /
//! [`Posture::warnings`] are pure over a captured snapshot so a hand-built
//! posture can be asserted directly with no I/O.
//!
//! Original BharatCode work; not ported from any third party.

use std::path::Path;

/// Environment switch that selects the visible strict posture mode.
///
/// Unset / non-strict => default: one `tracing::info!` posture line, no visible
/// warnings, default startup output unchanged. `strict` / `on` => additionally
/// print one warning line per failing control to stderr.
const HARDENED_ENV: &str = "BHARATCODE_HARDENED";

/// One hardening control: a stable, brand-free name plus whether it passed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Control {
    /// Stable identifier used in the per-control warning line. Brand-free.
    name: &'static str,
    /// Whether the running configuration satisfies this control.
    passed: bool,
}

/// A captured snapshot of the effective security settings, plus the derived
/// per-control pass/fail results. Kept as plain data so the scoring accessors
/// are pure and unit-testable against hand-built inputs with no I/O.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Posture {
    controls: Vec<Control>,
}

impl Posture {
    /// Build a posture from raw captured inputs. Pure: the same inputs always
    /// yield the same controls, so tests can assert scores deterministically.
    fn from_inputs(inputs: Inputs) -> Self {
        // Egress is coherent when it is actually locked down: offline composes
        // residency = strict at enforcement time, so either being true passes.
        let egress_locked = inputs.offline || inputs.residency_strict;

        let controls = vec![
            Control {
                name: "telemetry-off",
                passed: inputs.telemetry_off,
            },
            Control {
                name: "residency-offline-coherence",
                passed: egress_locked,
            },
            Control {
                name: "sandbox-available",
                passed: inputs.sandbox_available,
            },
            Control {
                name: "secret-redaction-on",
                passed: inputs.redaction_on,
            },
            Control {
                // Indeterminate (None) is not a finding: scores as a pass.
                name: "config-dir-not-world-writable",
                passed: !matches!(inputs.config_dir_world_writable, Some(true)),
            },
            Control {
                // Indeterminate (None) is not a finding: scores as a pass.
                name: "no-plaintext-key-file",
                passed: !matches!(inputs.secrets_file_loose_perms, Some(true)),
            },
        ];
        Posture { controls }
    }

    /// Number of controls that passed.
    pub fn score(&self) -> usize {
        self.controls.iter().filter(|c| c.passed).count()
    }

    /// Total number of controls scored.
    pub fn max_score(&self) -> usize {
        self.controls.len()
    }

    /// Whether every control passed (a fully hardened posture).
    pub fn is_fully_hardened(&self) -> bool {
        self.score() == self.max_score()
    }

    /// One visible warning line per *failing* control, in checklist order.
    ///
    /// Empty when the posture is fully hardened. Each line names the failing
    /// control so the strict-mode output is actionable; routed through the
    /// i18n layer where a translation exists, English otherwise.
    pub fn warnings(&self) -> Vec<String> {
        self.controls
            .iter()
            .filter(|c| !c.passed)
            .map(|c| {
                format!(
                    "{} {}",
                    label("hardened.warn_prefix", "hardening: control not met:"),
                    c.name
                )
            })
            .collect()
    }

    /// A single-line posture summary carrying an `N/M` score. Never multi-line,
    /// never contains an upstream brand.
    pub fn summary_line(&self) -> String {
        let verdict = if self.is_fully_hardened() {
            label("hardened.summary_ok", "all controls met")
        } else {
            label("hardened.summary_relaxed", "one or more controls relaxed")
        };
        format!(
            "{} {}/{} ({})",
            label("hardened.summary_prefix", "startup hardening posture:"),
            self.score(),
            self.max_score(),
            verdict,
        )
    }
}

/// Raw inputs to [`Posture::from_inputs`], snapshotted by [`capture`]. Kept
/// separate from [`Posture`] so the scoring logic is pure over plain bools.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Inputs {
    /// Telemetry is off (the privacy-preserving default).
    telemetry_off: bool,
    /// Offline / no-egress mode is on.
    offline: bool,
    /// Configured residency mode is `strict`.
    residency_strict: bool,
    /// Exec sandbox is set to a real level (not `off`).
    sandbox_available: bool,
    /// Egress secret redaction is on.
    redaction_on: bool,
    /// Config directory is world-writable; `None` when indeterminate.
    config_dir_world_writable: Option<bool>,
    /// On-disk secrets file exists and is group/world-readable; `None` when the
    /// file is absent or the mode cannot be determined.
    secrets_file_loose_perms: Option<bool>,
}

/// Snapshot the live security settings into [`Inputs`].
///
/// Every field is read through the same env var / accessor the corresponding
/// feature consults, so the posture reflects what is actually enforced. Pure
/// reads; no mutation, no network, never panics.
fn capture() -> Inputs {
    Inputs {
        telemetry_off: !telemetry_enabled(),
        offline: bharatcode_core::offline::is_offline(),
        residency_strict: matches!(
            bharatcode_core::residency::residency_mode(),
            bharatcode_core::residency::ResidencyMode::Strict
        ),
        sandbox_available: sandbox_available(),
        redaction_on: env_truthy("BHARATCODE_REDACT"),
        config_dir_world_writable: config_dir_world_writable(),
        secrets_file_loose_perms: secrets_file_loose_perms(),
    }
}

/// Run the read-only posture assessment over the live configuration.
///
/// Never panics — a missing config dir, stat error, or non-unix platform all
/// resolve to indeterminate (scored as a pass), so the function is safe to call
/// unconditionally early in session build.
pub fn assess() -> Posture {
    Posture::from_inputs(capture())
}

/// Whether the visible strict posture mode is enabled.
///
/// `BHARATCODE_HARDENED=strict` (or `on`) => `true`; unset / anything else =>
/// `false` (default: tracing-only, no visible warnings).
pub fn strict_enabled() -> bool {
    std::env::var(HARDENED_ENV)
        .map(|raw| {
            matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "strict" | "on" | "1" | "true" | "yes" | "enable" | "enabled"
            )
        })
        .unwrap_or(false)
}

/// Look up a user-facing string through the i18n layer, falling back to the
/// English `default` when the active locale table has no entry for `key`.
///
/// `tr!` echoes the key back when it is missing, so an unchanged key is treated
/// as "untranslated" and the English default is used. Mirrors the helper in the
/// other security tooling, keeping these lines renderable without depending on
/// an i18n table owned elsewhere in this wave.
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// Telemetry is opt-in. It counts as enabled only when the config flag is `true`
/// and the `BHARATCODE_TELEMETRY_OFF` kill-switch is not set — same rule as the
/// doctor settings summary and the security deep-check.
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

/// The exec sandbox is "available" when `BHARATCODE_SANDBOX` selects a real
/// level (`read-only` / `workspace-write`), mirroring the shell tool's parsing.
/// Unset / `off` / unrecognised => not available.
fn sandbox_available() -> bool {
    matches!(
        std::env::var("BHARATCODE_SANDBOX").ok().as_deref(),
        Some("read-only")
            | Some("readonly")
            | Some("read_only")
            | Some("workspace-write")
            | Some("workspace_write")
    )
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

/// Inspect the config directory's permissions and report whether it is
/// world-writable (`unix mode & 0o002`).
///
/// `Some(true)` => world-writable; `Some(false)` => exists and is not;
/// `None` => indeterminate (missing dir, stat error, or non-unix platform).
/// Read-only: stats the directory, never modifies it. Never panics.
fn config_dir_world_writable() -> Option<bool> {
    let dir = bharatcode_core::config::paths::Paths::config_dir();
    dir_world_writable(&dir)
}

/// World-writable check for an arbitrary path, factored out so tests can point
/// it at a fixture directory with known mode bits.
#[cfg(unix)]
fn dir_world_writable(dir: &Path) -> Option<bool> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(dir).ok()?;
    Some(meta.permissions().mode() & 0o002 != 0)
}

#[cfg(not(unix))]
fn dir_world_writable(_dir: &Path) -> Option<bool> {
    // The world-writable bit is a unix concept; report indeterminate.
    None
}

/// Whether the on-disk plaintext secrets file (if present) is group/world
/// readable (`unix mode & 0o077`).
///
/// `Some(true)` => present with loose permissions (a plaintext key file readable
/// by other users); `Some(false)` => present and tightly owner-only; `None` =>
/// absent, unreadable mode, or non-unix platform. Read-only; never panics.
fn secrets_file_loose_perms() -> Option<bool> {
    let path = bharatcode_core::config::paths::Paths::config_dir().join("secrets.yaml");
    file_loose_perms(&path)
}

/// Loose-permission check for an arbitrary file path, factored out for testing.
#[cfg(unix)]
fn file_loose_perms(path: &Path) -> Option<bool> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    Some(meta.permissions().mode() & 0o077 != 0)
}

#[cfg(not(unix))]
fn file_loose_perms(_path: &Path) -> Option<bool> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialize `BHARATCODE_HARDENED` mutation so concurrently-running tests in
    /// this module do not clobber each other's view of the process environment.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// A fully hardened set of inputs: every control at its tightest setting.
    fn hardened_inputs() -> Inputs {
        Inputs {
            telemetry_off: true,
            offline: true,
            residency_strict: true,
            sandbox_available: true,
            redaction_on: true,
            config_dir_world_writable: Some(false),
            secrets_file_loose_perms: Some(false),
        }
    }

    /// A maximally lax set of inputs: every control failing.
    fn lax_inputs() -> Inputs {
        Inputs {
            telemetry_off: false,
            offline: false,
            residency_strict: false,
            sandbox_available: false,
            redaction_on: false,
            config_dir_world_writable: Some(true),
            secrets_file_loose_perms: Some(true),
        }
    }

    #[test]
    fn hardened_posture_scores_max_with_no_warnings() {
        let posture = Posture::from_inputs(hardened_inputs());
        assert_eq!(
            posture.score(),
            posture.max_score(),
            "fully hardened posture must score the maximum"
        );
        assert!(
            posture.is_fully_hardened(),
            "fully hardened posture must report fully hardened"
        );
        assert!(
            posture.warnings().is_empty(),
            "fully hardened posture must produce no warnings, got: {:?}",
            posture.warnings()
        );
    }

    #[test]
    fn lax_posture_lists_failing_controls_by_name() {
        let posture = Posture::from_inputs(lax_inputs());
        assert_eq!(posture.score(), 0, "fully lax posture should score zero");

        let warnings = posture.warnings();
        assert_eq!(
            warnings.len(),
            posture.max_score(),
            "every control should warn in a fully lax posture"
        );

        // Each failing control must be named in the warnings, by name.
        for name in [
            "telemetry-off",
            "residency-offline-coherence",
            "sandbox-available",
            "secret-redaction-on",
            "config-dir-not-world-writable",
            "no-plaintext-key-file",
        ] {
            assert!(
                warnings.iter().any(|w| w.contains(name)),
                "warnings must name the failing control `{name}`, got: {warnings:?}"
            );
        }
    }

    #[test]
    fn offline_alone_satisfies_egress_coherence() {
        // Offline composes residency = strict at enforcement time, so offline
        // alone (residency not strict) must still pass the coherence control.
        let mut inputs = lax_inputs();
        inputs.offline = true;
        inputs.residency_strict = false;
        let posture = Posture::from_inputs(inputs);
        assert!(
            !posture
                .warnings()
                .iter()
                .any(|w| w.contains("residency-offline-coherence")),
            "offline alone should satisfy egress coherence"
        );
    }

    #[test]
    fn indeterminate_perms_are_not_findings() {
        // Missing config dir / absent secrets file => None => scored as a pass.
        let mut inputs = hardened_inputs();
        inputs.config_dir_world_writable = None;
        inputs.secrets_file_loose_perms = None;
        let posture = Posture::from_inputs(inputs);
        assert!(
            posture.is_fully_hardened(),
            "indeterminate permission checks must not count as findings"
        );
    }

    #[test]
    fn summary_line_is_one_line_with_score_and_brand_free() {
        for inputs in [hardened_inputs(), lax_inputs()] {
            let posture = Posture::from_inputs(inputs);
            let line = posture.summary_line();
            assert!(
                !line.contains('\n'),
                "summary must be a single line, got: {line:?}"
            );
            assert!(
                line.contains(&format!("{}/{}", posture.score(), posture.max_score())),
                "summary must carry an N/M score, got: {line:?}"
            );
            let lower = line.to_ascii_lowercase();
            assert!(
                !lower.contains("goose") && !lower.contains("block"),
                "summary leaked an upstream brand: {line:?}"
            );
        }
    }

    #[test]
    fn warnings_are_brand_free() {
        for w in Posture::from_inputs(lax_inputs()).warnings() {
            let lower = w.to_ascii_lowercase();
            assert!(
                !lower.contains("goose") && !lower.contains("block"),
                "warning leaked an upstream brand: {w:?}"
            );
        }
    }

    #[test]
    fn strict_enabled_reflects_env() {
        let _g = env_lock();
        let prev = std::env::var(HARDENED_ENV).ok();

        std::env::remove_var(HARDENED_ENV);
        assert!(!strict_enabled(), "unset => not strict");

        std::env::set_var(HARDENED_ENV, "strict");
        assert!(strict_enabled(), "`strict` => strict");

        std::env::set_var(HARDENED_ENV, "on");
        assert!(strict_enabled(), "`on` => strict");

        std::env::set_var(HARDENED_ENV, "off");
        assert!(!strict_enabled(), "`off` => not strict");

        std::env::set_var(HARDENED_ENV, "");
        assert!(!strict_enabled(), "empty => not strict");

        match prev {
            Some(v) => std::env::set_var(HARDENED_ENV, v),
            None => std::env::remove_var(HARDENED_ENV),
        }
    }

    #[test]
    fn assess_never_panics_on_missing_config_dir() {
        let _g = env_lock();
        let prev = std::env::var("HOME").ok();
        let prev_xdg = std::env::var("XDG_CONFIG_HOME").ok();

        // Point config resolution at a path that does not exist. `assess` must
        // resolve the missing dir / absent secrets file to indeterminate
        // (scored as a pass) rather than panicking.
        let missing = std::env::temp_dir().join("bharatcode-no-such-dir-v96-preflight");
        let _ = std::fs::remove_dir_all(&missing);
        std::env::set_var("XDG_CONFIG_HOME", &missing);
        std::env::set_var("HOME", &missing);

        let posture = assess();
        // It produced a well-formed posture (max score is the control count).
        assert!(posture.max_score() >= 1);
        let _ = posture.summary_line();
        let _ = posture.warnings();

        match prev {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match prev_xdg {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn loose_secrets_file_is_flagged_tight_is_not() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();

        let loose = dir.path().join("loose-secrets.yaml");
        let mut f = std::fs::File::create(&loose).unwrap();
        // Build a fake secret from fragments so push-protection never sees a
        // contiguous token literal.
        let fake_suffix = "FAKEFAKEFAKE0000";
        writeln!(f, "openai_api_key: sk-{fake_suffix}").unwrap();
        std::fs::set_permissions(&loose, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            file_loose_perms(&loose),
            Some(true),
            "0o644 secrets file must be flagged loose"
        );

        let tight = dir.path().join("tight-secrets.yaml");
        std::fs::write(&tight, "k: v\n").unwrap();
        std::fs::set_permissions(&tight, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            file_loose_perms(&tight),
            Some(false),
            "0o600 secrets file must not be flagged loose"
        );

        let absent = dir.path().join("nope-secrets.yaml");
        assert_eq!(
            file_loose_perms(&absent),
            None,
            "absent secrets file must be indeterminate, not a finding"
        );
    }

    #[cfg(unix)]
    #[test]
    fn world_writable_dir_is_flagged_secure_is_not() {
        use std::os::unix::fs::PermissionsExt;

        let open = tempfile::tempdir().unwrap();
        std::fs::set_permissions(open.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        assert_eq!(dir_world_writable(open.path()), Some(true));

        let secure = tempfile::tempdir().unwrap();
        std::fs::set_permissions(secure.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(dir_world_writable(secure.path()), Some(false));
    }
}
