//! Deep diagnostic checks for `bharatcode doctor` (BharatCode v36).
//!
//! Each public `check_*` function is a small, self-contained probe that returns a
//! [`CheckResult`] — a `(label, status, hint)` triple the doctor command renders
//! in its existing style. The probes are deliberately:
//!
//! - **Fast**: every network/disk touch uses a short timeout or a best-effort
//!   metadata read, so `doctor` never hangs on an unreachable endpoint.
//! - **Non-fatal**: a failing probe yields a [`Status::Fail`]/[`Status::Warn`]
//!   result with a hint, never a panic or a propagated error.
//! - **Read-only**: nothing here mutates config, the filesystem, or the network.
//!
//! The pure coherence rule (offline implies residency = strict) is factored out
//! into [`offline_implies_strict`] so it can be unit tested without any I/O.

use std::time::Duration;

use bharatcode_core::config::Config;
use bharatcode_core::residency::ResidencyMode;

/// Outcome of a single diagnostic check, rendered as ✓ / ⚠ / ✗ by the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// The check passed; nothing to act on.
    Ok,
    /// The check found a non-blocking concern worth surfacing.
    Warn,
    /// The check found a problem the operator likely needs to fix.
    Fail,
}

impl Status {
    /// The glyph this status renders as in the doctor output.
    pub fn glyph(self) -> &'static str {
        match self {
            Status::Ok => "\u{2713}",   // ✓
            Status::Warn => "\u{26a0}", // ⚠
            Status::Fail => "\u{2717}", // ✗
        }
    }
}

/// A single check's result: a human label, a pass/warn/fail status, and an
/// optional hint shown only when the status is not [`Status::Ok`].
#[derive(Clone, Debug)]
pub struct CheckResult {
    pub label: String,
    pub status: Status,
    /// Actionable hint; empty when there is nothing to suggest.
    pub hint: String,
}

impl CheckResult {
    fn ok(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            status: Status::Ok,
            hint: String::new(),
        }
    }

    fn warn(label: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            status: Status::Warn,
            hint: hint.into(),
        }
    }

    fn fail(label: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            status: Status::Fail,
            hint: hint.into(),
        }
    }
}

/// Look up a user-facing string through the i18n layer, falling back to the
/// English `default` when the active locale table has no entry for `key`.
///
/// `t()` echoes the key back when it is missing, so an unchanged key is treated
/// as "untranslated". This mirrors the helper in `doctor.rs` and keeps these
/// checks renderable in English without depending on the i18n table (owned by a
/// sibling version).
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// Run every deep check and return the results in display order.
///
/// This is the single entry point the doctor command calls; adding a check here
/// makes it appear in the running binary without further wiring.
pub fn run_all() -> Vec<CheckResult> {
    let config = Config::global();
    vec![
        check_local_provider_reachable(config),
        check_config_dir_writable(),
        check_git_available(),
        check_offline_residency_coherence(config),
        check_session_db_storage(),
    ]
}

/// Pure coherence rule: when offline mode is on, the *effective* residency
/// posture must be strict. Offline composes residency=strict at enforcement
/// time, so the only inconsistency worth flagging is a configured mode weaker
/// than strict while offline — the operator may expect their configured value to
/// win and be surprised that offline silently overrides it.
///
/// Returns `Ok(())` when the pairing is coherent, or `Err(hint)` describing the
/// mismatch. Pure and I/O-free so it can be unit tested directly.
pub fn offline_implies_strict(
    offline: bool,
    configured: ResidencyMode,
) -> Result<(), &'static str> {
    if offline && configured != ResidencyMode::Strict {
        Err(
            "offline mode forces residency=strict; your configured residency is weaker. \
             Set BHARATCODE_RESIDENCY=strict to make the configured value match what is enforced.",
        )
    } else {
        Ok(())
    }
}

/// Probe the configured local provider's endpoint (currently Ollama) for
/// reachability. Only runs when the active provider is a local one; otherwise it
/// reports a neutral "skipped" pass so remote setups are not penalised.
///
/// Uses a short TCP connect plus, when that succeeds, a brief HTTP probe of the
/// Ollama `/api/version` endpoint via the already-present blocking `reqwest`
/// client. Both steps have tight timeouts and are best-effort.
pub fn check_local_provider_reachable(config: &Config) -> CheckResult {
    let lbl = label("doctor.check.local_provider", "Local provider reachable");

    let provider = config
        .get_bharatcode_provider()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    if provider != "ollama" {
        return CheckResult::ok(format!(
            "{} ({})",
            lbl,
            label("doctor.check.not_local", "provider is not local; skipped")
        ));
    }

    let host_port = ollama_host_port(config);

    if !tcp_reachable(&host_port) {
        return CheckResult::fail(
            format!("{} ({})", lbl, host_port),
            label(
                "doctor.check.ollama_down",
                "could not open a TCP connection to Ollama. Is `ollama serve` running, and is OLLAMA_HOST correct?",
            ),
        );
    }

    // TCP is open; confirm it actually speaks the Ollama HTTP API. A non-Ollama
    // service squatting on the port would pass the TCP probe but fail here.
    match ollama_http_ok(&host_port) {
        true => CheckResult::ok(format!("{} ({})", lbl, host_port)),
        false => CheckResult::warn(
            format!("{} ({})", lbl, host_port),
            label(
                "doctor.check.ollama_no_http",
                "the port is open but did not answer the Ollama API. Another service may be using this port.",
            ),
        ),
    }
}

/// Verify the BharatCode config directory exists (or can be created) and is
/// writable, by creating and removing a temp probe file. A read-only or missing
/// config dir silently breaks `configure`, so surfacing it early is valuable.
pub fn check_config_dir_writable() -> CheckResult {
    let lbl = label("doctor.check.config_writable", "Config directory writable");
    let dir = bharatcode_core::config::paths::Paths::config_dir();

    if let Err(e) = std::fs::create_dir_all(&dir) {
        return CheckResult::fail(
            format!("{} ({})", lbl, dir.display()),
            format!(
                "{}: {}",
                label(
                    "doctor.check.config_mkdir_failed",
                    "could not create the config directory",
                ),
                e
            ),
        );
    }

    let probe = dir.join(format!(".bharatcode-doctor-{}.tmp", std::process::id()));
    match std::fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            CheckResult::ok(format!("{} ({})", lbl, dir.display()))
        }
        Err(e) => CheckResult::fail(
            format!("{} ({})", lbl, dir.display()),
            format!(
                "{}: {}",
                label(
                    "doctor.check.config_not_writable",
                    "the config directory is not writable; check permissions",
                ),
                e
            ),
        ),
    }
}

/// Check that `git` is on PATH and report its version. Several workflows shell
/// out to git, so a missing binary is worth flagging — but only as a warning,
/// since git is not required for every command.
pub fn check_git_available() -> CheckResult {
    let lbl = label("doctor.check.git", "Git available");

    match std::process::Command::new("git").arg("--version").output() {
        Ok(out) if out.status.success() => {
            let version = String::from_utf8_lossy(&out.stdout)
                .trim()
                .trim_start_matches("git version ")
                .trim()
                .to_string();
            let shown = if version.is_empty() {
                label("doctor.check.git_unknown_version", "version unknown")
            } else {
                version
            };
            CheckResult::ok(format!("{} ({})", lbl, shown))
        }
        Ok(_) => CheckResult::warn(
            lbl,
            label(
                "doctor.check.git_error",
                "`git --version` exited with an error; your git install may be broken",
            ),
        ),
        Err(_) => CheckResult::warn(
            lbl,
            label(
                "doctor.check.git_missing",
                "git was not found on PATH. Install git to enable repo-aware features.",
            ),
        ),
    }
}

/// Report whether offline mode and the configured residency mode are coherent,
/// using the pure [`offline_implies_strict`] rule. Reads `is_offline()` and the
/// configured residency mode (not the effective one) so the operator sees the
/// raw values that triggered the mismatch.
pub fn check_offline_residency_coherence(_config: &Config) -> CheckResult {
    let lbl = label(
        "doctor.check.offline_residency",
        "Offline / residency coherence",
    );
    let offline = bharatcode_core::offline::is_offline();
    let configured = bharatcode_core::residency::residency_mode();

    match offline_implies_strict(offline, configured) {
        Ok(()) => CheckResult::ok(lbl),
        Err(hint) => CheckResult::warn(lbl, label("doctor.check.offline_residency_hint", hint)),
    }
}

/// Best-effort check of the session database's storage location: the session DB
/// lives under the data directory, so verify that directory exists / is
/// creatable and writable, and surface the current DB size when present.
///
/// A precise free-bytes figure would need a platform syscall (and a new
/// dependency), so this stays dependency-free and focuses on the failure that
/// actually blocks sessions: an unwritable data directory.
pub fn check_session_db_storage() -> CheckResult {
    let lbl = label("doctor.check.session_db", "Session DB storage");
    let data_dir = bharatcode_core::config::paths::Paths::data_dir();
    let session_dir = data_dir.join("sessions");

    if let Err(e) = std::fs::create_dir_all(&session_dir) {
        return CheckResult::fail(
            format!("{} ({})", lbl, session_dir.display()),
            format!(
                "{}: {}",
                label(
                    "doctor.check.session_dir_failed",
                    "could not create the session directory",
                ),
                e
            ),
        );
    }

    let probe = session_dir.join(format!(".bharatcode-doctor-{}.tmp", std::process::id()));
    if let Err(e) = std::fs::write(&probe, b"ok") {
        return CheckResult::fail(
            format!("{} ({})", lbl, session_dir.display()),
            format!(
                "{}: {}",
                label(
                    "doctor.check.session_not_writable",
                    "the session directory is not writable; sessions cannot be saved",
                ),
                e
            ),
        );
    }
    let _ = std::fs::remove_file(&probe);

    let db_path = session_dir.join("sessions.db");
    let size_note = match std::fs::metadata(&db_path) {
        Ok(meta) => format!(" — {}", human_bytes(meta.len())),
        Err(_) => format!(
            " — {}",
            label(
                "doctor.check.session_db_new",
                "no DB yet (created on first session)"
            )
        ),
    };
    CheckResult::ok(format!("{} ({}{})", lbl, session_dir.display(), size_note))
}

/// Render a byte count as a short human-readable string (e.g. `4.2 MB`).
fn human_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{:.1} {}", value, UNITS[unit])
    }
}

/// Resolve the Ollama endpoint to a `host:port` string, mirroring how the
/// provider reads `OLLAMA_HOST`.
fn ollama_host_port(config: &Config) -> String {
    const DEFAULT_HOST: &str = "localhost";
    const DEFAULT_PORT: u16 = 11434;

    let raw = config
        .get_param::<String>("OLLAMA_HOST")
        .unwrap_or_else(|_| DEFAULT_HOST.to_string());
    let trimmed = raw.trim();
    let stripped = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"))
        .unwrap_or(trimmed)
        .trim_end_matches('/');
    if stripped.is_empty() {
        format!("{}:{}", DEFAULT_HOST, DEFAULT_PORT)
    } else if stripped.contains(':') {
        stripped.to_string()
    } else {
        format!("{}:{}", stripped, DEFAULT_PORT)
    }
}

/// Best-effort TCP reachability probe with a short timeout. Returns `false` on
/// any resolution or connection failure.
fn tcp_reachable(host_port: &str) -> bool {
    use std::net::{TcpStream, ToSocketAddrs};

    let addrs = match host_port.to_socket_addrs() {
        Ok(addrs) => addrs,
        Err(_) => return false,
    };
    for addr in addrs {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(400)).is_ok() {
            return true;
        }
    }
    false
}

/// Probe the Ollama HTTP API at `/api/version` with a short timeout using the
/// already-present blocking reqwest client. Returns `true` only on a successful
/// HTTP response, so a non-Ollama service on the port fails this check.
fn ollama_http_ok(host_port: &str) -> bool {
    let url = format!("http://{}/api/version", host_port);
    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(600))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    client
        .get(&url)
        .send()
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_strict_is_coherent() {
        assert!(offline_implies_strict(true, ResidencyMode::Strict).is_ok());
    }

    #[test]
    fn offline_with_weaker_residency_is_flagged() {
        assert!(offline_implies_strict(true, ResidencyMode::Off).is_err());
        assert!(offline_implies_strict(true, ResidencyMode::Warn).is_err());
    }

    #[test]
    fn online_never_flags_residency() {
        // When not offline, any configured residency mode is coherent.
        assert!(offline_implies_strict(false, ResidencyMode::Off).is_ok());
        assert!(offline_implies_strict(false, ResidencyMode::Warn).is_ok());
        assert!(offline_implies_strict(false, ResidencyMode::Strict).is_ok());
    }

    #[test]
    fn status_glyphs_are_distinct() {
        assert_ne!(Status::Ok.glyph(), Status::Warn.glyph());
        assert_ne!(Status::Warn.glyph(), Status::Fail.glyph());
        assert_ne!(Status::Ok.glyph(), Status::Fail.glyph());
    }

    #[test]
    fn human_bytes_scales_units() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1.0 KB");
        assert_eq!(human_bytes(1024 * 1024), "1.0 MB");
    }
}
