//! Crash / session resume pointer (last-good-turn recovery) for BharatCode.
//!
//! An interactive CLI session can die in ways the normal "session closed" path
//! never sees: the terminal is killed, the machine sleeps and the SSH pipe
//! drops, the process is `kill -9`'d, or the user hits Ctrl-C one too many
//! times. When that happens the in-memory pointer to "the session I was just
//! working in" is lost, and re-entering it means digging the id out of the
//! sessions list by hand.
//!
//! This module keeps a single tiny **recovery sidecar** — not the conversation,
//! just a pointer: which session was live, in which working directory, what the
//! last user prompt was, how many turns had completed, and when (IST). On every
//! successful turn the sidecar is rewritten; on a *clean* exit it is removed. So
//! the sidecar exists only while a session is mid-flight, and [`load`] hands the
//! caller back the most-recent interrupted session to re-enter after a crash.
//!
//! Design:
//!   * **Default OFF.** Nothing is written, and no startup hint is shown, unless
//!     `BHARATCODE_RESUME` is set to a truthy value. With it unset the session
//!     flow is byte-identical to before — no sidecar, no hint, no extra IO.
//!   * **One sidecar, not a log.** A single `recovery.json` is overwritten in
//!     place (atomically, via tmp+rename) so it always reflects the *latest*
//!     good turn of the *current* live session and never grows.
//!   * **Pointer, not payload.** Only metadata is stored — session id, cwd, the
//!     last user prompt (so the user recognises which session it was), a turn
//!     count, and an IST timestamp. The conversation itself stays in the
//!     sessions DB; this module never touches that DB.
//!   * **Best-effort.** [`record`] and [`clear`] never surface an error to the
//!     turn loop; a recovery-sidecar failure must not break a user's session.
//!
//! Original BharatCode work; not ported from any third party. std + serde_json
//! only.

use std::path::PathBuf;

use chrono::{DateTime, FixedOffset, Utc};
use bharatcode_core::config::paths::Paths;
use serde::{Deserialize, Serialize};

/// Environment key that turns crash/resume recovery on. Absent / falsey =>
/// fully disabled (default OFF — the session flow is unchanged).
pub const RESUME_ENABLED_KEY: &str = "BHARATCODE_RESUME";

/// Sub-directory (under the config dir) holding the recovery sidecar. Kept
/// distinct so the pointer file is easy to find and clear by hand if needed.
const RECOVERY_SUBDIR: &str = "bharatcode";

/// File name of the single recovery sidecar.
const RECOVERY_FILE: &str = "recovery.json";

/// India Standard Time (UTC+05:30). Surfaced in the sidecar (and the startup
/// hint) so an Indian user reads the local wall clock of the last good turn.
fn ist_offset() -> FixedOffset {
    FixedOffset::east_opt(5 * 3600 + 30 * 60).expect("IST (+05:30) is a valid fixed offset")
}

/// Whether crash/resume recovery is enabled for this process.
///
/// Reads `BHARATCODE_RESUME` straight from the environment and accepts the usual
/// truthy spellings (`1`, `true`, `yes`, `on`); anything else — including
/// absence — is OFF. The raw-env read (rather than going through the typed
/// config layer) mirrors the memory-store gate so that a bare `1` survives
/// instead of being coerced to a JSON number and read back as unset.
pub fn is_enabled() -> bool {
    match std::env::var(RESUME_ENABLED_KEY) {
        Ok(raw) => is_truthy(&raw),
        Err(_) => false,
    }
}

fn is_truthy(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Absolute path of the recovery sidecar (`<config_dir>/bharatcode/recovery.json`).
pub fn recovery_path() -> PathBuf {
    Paths::config_dir()
        .join(RECOVERY_SUBDIR)
        .join(RECOVERY_FILE)
}

/// A pointer to the last good turn of a live session.
///
/// Deliberately a pointer, not a payload: it carries just enough to recognise
/// and re-enter the session, never the conversation body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryPoint {
    /// Session id to re-enter.
    pub session_id: String,
    /// Working directory the session was running in.
    pub working_dir: String,
    /// The last user prompt, so the user recognises which session this is.
    pub last_prompt: String,
    /// Number of turns completed at the time the pointer was written.
    pub turns: u64,
    /// Timestamp of the last good turn, rendered in IST (`YYYY-MM-DD HH:MM:SS`).
    pub ts_ist: String,
}

impl RecoveryPoint {
    /// Build a pointer for the current instant.
    ///
    /// The IST timestamp is stamped here so callers do not have to reach for a
    /// clock; everything else is supplied by the turn loop.
    pub fn now(
        session_id: impl Into<String>,
        working_dir: impl Into<String>,
        last_prompt: impl Into<String>,
        turns: u64,
    ) -> Self {
        let now: DateTime<Utc> = Utc::now();
        Self {
            session_id: session_id.into(),
            working_dir: working_dir.into(),
            last_prompt: last_prompt.into(),
            turns,
            ts_ist: now
                .with_timezone(&ist_offset())
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
        }
    }

    /// A one-line, user-facing description of this pointer for the startup hint.
    /// Carries no Goose/Block identifiers.
    pub fn summary_line(&self) -> String {
        let prompt = self.last_prompt.trim();
        let preview = truncate_prompt(prompt, 60);
        if preview.is_empty() {
            format!(
                "session {} · {} turn(s) · {} IST",
                self.session_id, self.turns, self.ts_ist
            )
        } else {
            format!(
                "session {} · {} turn(s) · {} IST · last: {}",
                self.session_id, self.turns, self.ts_ist, preview
            )
        }
    }
}

/// Truncate a single-line prompt preview to `max` chars (char-safe), appending
/// an ellipsis when it was cut. Newlines collapse to spaces so the hint stays on
/// one line.
fn truncate_prompt(prompt: &str, max: usize) -> String {
    let flat: String = prompt
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    let flat = flat.trim();
    if flat.chars().count() <= max {
        return flat.to_string();
    }
    let head: String = flat.chars().take(max).collect();
    format!("{head}…")
}

/// Write (or overwrite) the recovery sidecar (best-effort).
///
/// No-op when recovery is disabled. The write goes to a unique temp file and is
/// then renamed into place, so a concurrent reader (or an interrupted write)
/// never observes a half-written sidecar. Any IO error is swallowed (logged at
/// `warn`) so a recovery-sidecar failure never breaks the user's turn.
pub fn record(point: &RecoveryPoint) {
    if !is_enabled() {
        return;
    }
    if let Err(e) = write_point(point) {
        tracing::warn!("recovery: failed to write resume pointer: {e}");
    }
}

fn write_point(point: &RecoveryPoint) -> std::io::Result<()> {
    let path = recovery_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_vec_pretty(point)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let tmp = path.with_extension(format!(
        "tmp.{}.{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(&tmp, &content)?;
    std::fs::rename(&tmp, &path)
}

/// Load the most-recent recovery pointer, if one exists and parses.
///
/// Returns `None` when recovery is disabled, when no sidecar is present (the
/// common case — a clean exit removes it), or when the sidecar cannot be read or
/// parsed. Never errors into the caller.
pub fn load() -> Option<RecoveryPoint> {
    if !is_enabled() {
        return None;
    }
    let path = recovery_path();
    let bytes = std::fs::read(&path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Remove the recovery sidecar (best-effort).
///
/// Called on a clean exit: once the session has closed normally there is nothing
/// to recover, so the pointer is deleted. A missing file is success; any other
/// IO error is swallowed (logged at `warn`).
pub fn clear() {
    let path = recovery_path();
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => tracing::warn!("recovery: failed to clear resume pointer: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_enabled_honors_env() {
        // Serialize the process-global env mutation through the shared env lock so
        // these env-mutating tests do not race sibling tests under parallelism.
        let _guard = env_lock::lock_env([(RESUME_ENABLED_KEY, None::<&str>)]);

        std::env::remove_var(RESUME_ENABLED_KEY);
        assert!(!is_enabled(), "unset must be OFF");

        for v in ["1", "true", "TRUE", "yes", "on", " On "] {
            std::env::set_var(RESUME_ENABLED_KEY, v);
            assert!(is_enabled(), "expected {v:?} to be truthy");
        }
        for v in ["0", "false", "no", "off", ""] {
            std::env::set_var(RESUME_ENABLED_KEY, v);
            assert!(!is_enabled(), "expected {v:?} to be falsey");
        }
        std::env::remove_var(RESUME_ENABLED_KEY);
    }

    #[test]
    fn record_load_round_trip() {
        let dir = std::env::temp_dir().join(format!("bc-recovery-rt-{}", std::process::id()));
        let dir_str = dir.display().to_string();
        // Pin both the config-dir root and the enable flag for this test.
        let _guard = env_lock::lock_env([
            ("BHARATCODE_PATH_ROOT", Some(dir_str.as_str())),
            (RESUME_ENABLED_KEY, Some("1")),
        ]);

        let _ = std::fs::remove_file(recovery_path());

        let point = RecoveryPoint::now(
            "sess-recover-1",
            "/home/user/project",
            "fix the failing test in foo.rs",
            7,
        );
        record(&point);

        let loaded = load().expect("a recorded pointer must load back");
        assert_eq!(loaded, point);
        assert_eq!(loaded.session_id, "sess-recover-1");
        assert_eq!(loaded.working_dir, "/home/user/project");
        assert_eq!(loaded.turns, 7);
        assert!(!loaded.ts_ist.is_empty());

        let _ = std::fs::remove_file(recovery_path());
    }

    #[test]
    fn load_none_when_absent() {
        let dir = std::env::temp_dir().join(format!("bc-recovery-absent-{}", std::process::id()));
        let dir_str = dir.display().to_string();
        let _guard = env_lock::lock_env([
            ("BHARATCODE_PATH_ROOT", Some(dir_str.as_str())),
            (RESUME_ENABLED_KEY, Some("1")),
        ]);

        let _ = std::fs::remove_file(recovery_path());
        assert!(load().is_none(), "absent sidecar must load as None");
    }

    #[test]
    fn clear_removes_sidecar() {
        let dir = std::env::temp_dir().join(format!("bc-recovery-clear-{}", std::process::id()));
        let dir_str = dir.display().to_string();
        let _guard = env_lock::lock_env([
            ("BHARATCODE_PATH_ROOT", Some(dir_str.as_str())),
            (RESUME_ENABLED_KEY, Some("1")),
        ]);

        let point = RecoveryPoint::now("sess-clear", "/tmp/work", "do the thing", 1);
        record(&point);
        assert!(load().is_some(), "sidecar should exist after record");

        clear();
        assert!(load().is_none(), "sidecar should be gone after clear");
        // Clearing an already-absent sidecar is a no-op, not an error.
        clear();
        assert!(load().is_none());
    }

    #[test]
    fn disabled_records_nothing() {
        let dir = std::env::temp_dir().join(format!("bc-recovery-off-{}", std::process::id()));
        let dir_str = dir.display().to_string();
        let _guard = env_lock::lock_env([
            ("BHARATCODE_PATH_ROOT", Some(dir_str.as_str())),
            (RESUME_ENABLED_KEY, None::<&str>),
        ]);

        let _ = std::fs::remove_file(recovery_path());
        let point = RecoveryPoint::now("sess-off", "/tmp/off", "nothing", 1);
        record(&point);
        // With the flag unset, record is a no-op and load returns None.
        assert!(!recovery_path().exists(), "no sidecar when disabled");
        assert!(load().is_none());
    }

    #[test]
    fn summary_line_truncates_long_prompt_and_hides_newlines() {
        let long = "a".repeat(200);
        let point = RecoveryPoint::now("s", "/d", format!("line1\n{long}"), 3);
        let line = point.summary_line();
        assert!(line.contains("session s"));
        assert!(line.contains("3 turn(s)"));
        assert!(!line.contains('\n'), "summary must stay single-line");
        assert!(line.contains('…'), "long prompt should be truncated");
    }
}
