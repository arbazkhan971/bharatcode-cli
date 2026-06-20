//! Periodic turn-checkpoint writer (rolling intra-turn progress marker).
//!
//! A long agent run can die mid-turn — the machine sleeps, the SSH pipe drops,
//! the process is `kill -9`'d — in a way the normal "turn finished" path never
//! sees. The one-shot recovery sidecar (see `session/recovery.rs`) records the
//! *last good turn*; this module is its complement: a tiny **rolling
//! checkpoint** that is rewritten on a throttled interval *while* a turn is in
//! flight, so a hard crash leaves a precise resume marker pointing at the most
//! recent message that had actually been persisted, not just the last whole
//! turn that completed.
//!
//! Design:
//!   * **Default OFF.** Nothing is written and no I/O happens unless
//!     `BHARATCODE_CHECKPOINT` is set to a truthy value. With it unset
//!     [`record`] is a no-op (zero I/O), so behavior is byte-identical to
//!     before.
//!   * **Throttled.** A checkpoint is persisted only when at least `N`
//!     milliseconds have elapsed since the last write (default
//!     [`DEFAULT_MIN_INTERVAL_MS`], overridable via
//!     `BHARATCODE_CHECKPOINT_INTERVAL_MS`). The throttle decision is the pure
//!     [`should_write`] helper so callers in a tight per-turn loop never hammer
//!     the disk.
//!   * **One rolling file, not a log.** A single `checkpoint.json` is
//!     overwritten in place (atomically, via tmp + rename — mirroring the
//!     recovery sidecar) so it always reflects the *latest* progress and never
//!     grows.
//!   * **Best-effort.** [`record`] never surfaces an error or blocks the turn;
//!     any I/O failure is swallowed (logged at `debug`).
//!   * **IST + UTC.** The checkpoint carries both a UTC timestamp and an IST
//!     (+05:30) wall-clock string, matching the recovery/audit conventions so
//!     an Indian operator reads a familiar local clock.
//!
//! Distinct from `session/recovery.rs`: that is one overwritten last-good-turn
//! sidecar; this is periodic, throttled, intra-turn progress.
//!
//! Original BharatCode work; not ported from any third party. std + chrono +
//! serde_json only.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};

use crate::config::paths::Paths;

/// Environment key that turns the periodic checkpoint writer on. Absent /
/// falsey => fully disabled (default OFF — [`record`] is a no-op).
pub const CHECKPOINT_ENABLED_KEY: &str = "BHARATCODE_CHECKPOINT";

/// Environment key overriding the throttle interval, in milliseconds. Absent or
/// unparseable => [`DEFAULT_MIN_INTERVAL_MS`].
pub const CHECKPOINT_INTERVAL_KEY: &str = "BHARATCODE_CHECKPOINT_INTERVAL_MS";

/// Default minimum gap between two persisted checkpoints (5s). Frequent enough
/// to bound the lost work after a crash, sparse enough to stay off the hot path.
pub const DEFAULT_MIN_INTERVAL_MS: u64 = 5_000;

/// Sub-directory (under the config dir) holding the rolling checkpoint. Shared
/// with the recovery sidecar so the two progress markers live side by side.
const CHECKPOINT_SUBDIR: &str = "bharatcode";

/// File name of the single rolling checkpoint.
const CHECKPOINT_FILE: &str = "checkpoint.json";

/// Process-global instant of the last persisted checkpoint, used by the
/// throttle. `None` until the first write of this process.
static LAST_WRITE: Mutex<Option<Instant>> = Mutex::new(None);

/// India Standard Time (UTC+05:30). Surfaced in the checkpoint so an Indian
/// operator reads the local wall clock of the last persisted progress.
fn ist_offset() -> FixedOffset {
    FixedOffset::east_opt(5 * 3600 + 30 * 60).expect("IST (+05:30) is a valid fixed offset")
}

/// Whether the periodic checkpoint writer is enabled for this process.
///
/// Reads `BHARATCODE_CHECKPOINT` straight from the environment and accepts the
/// usual truthy spellings (`1`, `true`, `yes`, `on`); anything else — including
/// absence — is OFF. The raw-env read (rather than the typed config layer)
/// mirrors the recovery sidecar gate.
pub fn is_enabled() -> bool {
    match std::env::var(CHECKPOINT_ENABLED_KEY) {
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

/// The configured throttle interval, read from `BHARATCODE_CHECKPOINT_INTERVAL_MS`
/// or [`DEFAULT_MIN_INTERVAL_MS`] when unset / unparseable.
fn min_interval() -> Duration {
    let ms = std::env::var(CHECKPOINT_INTERVAL_KEY)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_MIN_INTERVAL_MS);
    Duration::from_millis(ms)
}

/// Throttle decision: should a checkpoint be persisted *now*?
///
/// Pure and clock-injected so it is trivially testable. A write is allowed when
/// there has been no prior write (`last_write` is `None`) or when at least
/// `min_interval` has elapsed since it. Inside the interval it returns `false`.
pub fn should_write(last_write: Option<Instant>, now: Instant, min_interval: Duration) -> bool {
    match last_write {
        None => true,
        Some(prev) => now.duration_since(prev) >= min_interval,
    }
}

/// Absolute path of the rolling checkpoint
/// (`<config_dir>/bharatcode/checkpoint.json`).
pub fn checkpoint_path() -> PathBuf {
    Paths::config_dir()
        .join(CHECKPOINT_SUBDIR)
        .join(CHECKPOINT_FILE)
}

/// A rolling intra-turn progress marker.
///
/// Deliberately a pointer, not a payload: it carries just enough to resume —
/// which session, how far in (turn index), the last message that had been
/// persisted, and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Session id the run belongs to.
    pub session_id: String,
    /// Index of the turn currently in flight.
    pub turn_index: u64,
    /// Id of the last message known to have been saved, if any. Empty when the
    /// in-flight turn has not yet produced a persisted message.
    pub last_message_id: String,
    /// Timestamp of this checkpoint, in UTC (RFC 3339).
    pub ts_utc: String,
    /// Same instant rendered in IST (`YYYY-MM-DD HH:MM:SS`).
    pub ts_ist: String,
}

impl Checkpoint {
    /// Build a checkpoint stamped at the current instant.
    ///
    /// Both timestamps are stamped here so callers in the turn loop do not have
    /// to reach for a clock; everything else is supplied by the caller.
    pub fn now(
        session_id: impl Into<String>,
        turn_index: u64,
        last_message_id: impl Into<String>,
    ) -> Self {
        let now: DateTime<Utc> = Utc::now();
        Self {
            session_id: session_id.into(),
            turn_index,
            last_message_id: last_message_id.into(),
            ts_utc: now.to_rfc3339(),
            ts_ist: now
                .with_timezone(&ist_offset())
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
        }
    }

    /// A one-line, user-facing resumable-pointer rendering. Carries no
    /// Goose/Block identifiers.
    pub fn summary_line(&self) -> String {
        let msg = self.last_message_id.trim();
        if msg.is_empty() {
            format!(
                "resume session {} · turn {} · {} IST",
                self.session_id, self.turn_index, self.ts_ist
            )
        } else {
            format!(
                "resume session {} · turn {} · after msg {} · {} IST",
                self.session_id, self.turn_index, msg, self.ts_ist
            )
        }
    }
}

/// Record a checkpoint for the current point in a turn (best-effort, throttled).
///
/// No-op (zero I/O) when the writer is disabled. When enabled, it persists only
/// if the throttle ([`should_write`]) allows — otherwise it returns immediately
/// without touching the disk. The write goes to a unique temp file and is then
/// renamed into place, so a concurrent reader (or an interrupted write) never
/// observes a half-written checkpoint. Any I/O error is swallowed (logged at
/// `debug`) so a checkpoint failure never breaks or blocks the user's turn.
pub fn record(session_id: &str, turn_index: u64, last_message_id: &str) {
    if !is_enabled() {
        return;
    }

    let now = Instant::now();
    {
        let mut guard = match LAST_WRITE.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if !should_write(*guard, now, min_interval()) {
            return;
        }
        // Reserve the slot before doing I/O so concurrent callers in the same
        // window do not all pile into a write.
        *guard = Some(now);
    }

    let checkpoint = Checkpoint::now(session_id, turn_index, last_message_id);
    if let Err(e) = write_checkpoint(&checkpoint) {
        tracing::debug!(target: "turn_checkpoint", error = %e, "failed to write turn checkpoint");
    }
}

fn write_checkpoint(checkpoint: &Checkpoint) -> std::io::Result<()> {
    let path = checkpoint_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_vec_pretty(checkpoint)
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

/// Read the latest rolling checkpoint back, for a resume hint.
///
/// Returns `None` when no checkpoint is present or when it cannot be read or
/// parsed. Unlike [`record`], reading is allowed regardless of the enable gate
/// so a resume flow can surface a marker left by a previous (enabled) run.
/// Never errors into the caller.
pub fn latest() -> Option<Checkpoint> {
    let path = checkpoint_path();
    let bytes = std::fs::read(&path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_enabled_honors_env() {
        let _guard = env_lock::lock_env([(CHECKPOINT_ENABLED_KEY, None::<&str>)]);

        std::env::remove_var(CHECKPOINT_ENABLED_KEY);
        assert!(!is_enabled(), "unset must be OFF");

        for v in ["1", "true", "TRUE", "yes", "on", " On "] {
            std::env::set_var(CHECKPOINT_ENABLED_KEY, v);
            assert!(is_enabled(), "expected {v:?} to be truthy");
        }
        for v in ["0", "false", "no", "off", ""] {
            std::env::set_var(CHECKPOINT_ENABLED_KEY, v);
            assert!(!is_enabled(), "expected {v:?} to be falsey");
        }
        std::env::remove_var(CHECKPOINT_ENABLED_KEY);
    }

    #[test]
    fn checkpoint_round_trips_through_serde() {
        let cp = Checkpoint::now("sess-rt-1", 4, "msg_abc123");
        let json = serde_json::to_string(&cp).expect("serialize");
        let back: Checkpoint = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, cp);
        assert_eq!(back.session_id, "sess-rt-1");
        assert_eq!(back.turn_index, 4);
        assert_eq!(back.last_message_id, "msg_abc123");
        assert!(!back.ts_utc.is_empty());
        assert!(!back.ts_ist.is_empty());
    }

    #[test]
    fn should_write_respects_interval() {
        let interval = Duration::from_millis(5_000);
        let base = Instant::now();

        // No prior write => always allowed.
        assert!(should_write(None, base, interval));

        // Inside the interval => throttled (false).
        let inside = base + Duration::from_millis(1_000);
        assert!(!should_write(Some(base), inside, interval));

        // Exactly at the boundary => allowed.
        let at_boundary = base + interval;
        assert!(should_write(Some(base), at_boundary, interval));

        // Past the interval => allowed.
        let past = base + Duration::from_millis(6_000);
        assert!(should_write(Some(base), past, interval));
    }

    #[test]
    fn record_is_noop_when_disabled() {
        let dir = std::env::temp_dir().join(format!("bc-checkpoint-off-{}", std::process::id()));
        let dir_str = dir.display().to_string();
        let _guard = env_lock::lock_env([
            ("BHARATCODE_PATH_ROOT", Some(dir_str.as_str())),
            (CHECKPOINT_ENABLED_KEY, None::<&str>),
        ]);

        let _ = std::fs::remove_file(checkpoint_path());
        record("sess-off", 1, "msg_off");
        // With the gate off, record performs zero I/O: no file is created.
        assert!(
            !checkpoint_path().exists(),
            "no checkpoint file when disabled"
        );
        assert!(latest().is_none());
    }

    #[test]
    fn record_writes_and_latest_reads_back_when_enabled() {
        let dir = std::env::temp_dir().join(format!("bc-checkpoint-rt-{}", std::process::id()));
        let dir_str = dir.display().to_string();
        let _guard = env_lock::lock_env([
            ("BHARATCODE_PATH_ROOT", Some(dir_str.as_str())),
            (CHECKPOINT_ENABLED_KEY, Some("1")),
        ]);

        // Reset the process-global throttle so this test always writes.
        *LAST_WRITE.lock().unwrap() = None;
        let _ = std::fs::remove_file(checkpoint_path());

        record("sess-rt", 9, "msg_xyz");
        let loaded = latest().expect("a recorded checkpoint must read back");
        assert_eq!(loaded.session_id, "sess-rt");
        assert_eq!(loaded.turn_index, 9);
        assert_eq!(loaded.last_message_id, "msg_xyz");

        let _ = std::fs::remove_file(checkpoint_path());
    }

    #[test]
    fn summary_line_renders_single_resumable_pointer() {
        let with_msg = Checkpoint::now("s1", 3, "msg_42");
        let line = with_msg.summary_line();
        assert!(line.contains("resume session s1"));
        assert!(line.contains("turn 3"));
        assert!(line.contains("msg_42"));
        assert!(!line.contains('\n'), "summary must stay single-line");

        let no_msg = Checkpoint::now("s2", 0, "");
        let line2 = no_msg.summary_line();
        assert!(line2.contains("resume session s2"));
        assert!(line2.contains("turn 0"));
        assert!(!line2.contains('\n'));
    }
}
