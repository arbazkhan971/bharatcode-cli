//! Opt-in editor/IDE bridge breadcrumb — BharatCode v80.
//!
//! While a reply is streaming, this module can drop a tiny, machine-readable
//! breadcrumb at `<working_dir>/.bharatcode/ide-bridge.json` that an editor or
//! IDE extension can poll to surface a "BharatCode is working here" indicator.
//! The breadcrumb carries just enough context to be useful — the session id,
//! the active model, a coarse status, a last-updated timestamp (both IST and
//! UTC) and the process id — and nothing more.
//!
//! The whole feature is **opt-in and defaults to off**, gated on the raw
//! `BHARATCODE_IDE_BRIDGE` environment variable. When the switch is off,
//! [`is_enabled`] returns `false`, [`write_breadcrumb`] is never called from the
//! streaming path, and no file is ever written, so default behaviour is
//! completely unchanged.
//!
//! Writes are **best-effort**: the breadcrumb is written atomically (a temp file
//! in the same directory, then a rename) so a poller never observes a partial
//! file, and any I/O error along the way is swallowed rather than surfaced — a
//! missing or stale breadcrumb must never disrupt an actual reply.

use std::path::Path;

use chrono::{FixedOffset, Utc};

/// Environment key for the IDE bridge switch. Defaults to off.
pub const IDE_BRIDGE_KEY: &str = "BHARATCODE_IDE_BRIDGE";

/// Directory (under the working dir) that holds the breadcrumb.
const BREADCRUMB_DIR: &str = ".bharatcode";

/// File name of the breadcrumb within [`BREADCRUMB_DIR`].
const BREADCRUMB_FILE: &str = "ide-bridge.json";

/// India Standard Time (UTC+05:30). BharatCode targets India, so the
/// human-facing `updated_at_ist` field is rendered against the IST wall clock
/// while `updated_at_utc` stays unambiguous for machine consumers.
fn ist_offset() -> FixedOffset {
    FixedOffset::east_opt(5 * 3600 + 30 * 60).expect("IST (+05:30) is a valid fixed offset")
}

/// Interpret a raw flag value as truthy. Mirrors the other BharatCode switches:
/// only a clearly affirmative value enables the feature; everything else
/// (including unset / unrecognised) leaves it off so default behaviour is never
/// flipped by accident.
fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on" | "enable" | "enabled"
    )
}

/// Returns `true` when the IDE bridge breadcrumb is enabled. Defaults to
/// `false`.
///
/// Reads the raw `BHARATCODE_IDE_BRIDGE` environment variable directly
/// (raw-env-first); any truthy value turns the feature on. Unset or
/// unrecognised resolves to "off".
pub fn is_enabled() -> bool {
    std::env::var(IDE_BRIDGE_KEY)
        .ok()
        .map(|raw| is_truthy(&raw))
        .unwrap_or(false)
}

/// Render the breadcrumb payload as a pretty-printed JSON string.
///
/// Kept separate from the file write so it can be unit-tested without touching
/// the filesystem. Field keys are deliberately neutral and stable so an
/// external poller can rely on them.
fn render_payload(session_id: &str, model: &str, status: &str) -> String {
    let now_utc = Utc::now();
    let now_ist = now_utc.with_timezone(&ist_offset());

    let payload = serde_json::json!({
        "session_id": session_id,
        "model": model,
        "status": status,
        "updated_at_utc": now_utc.to_rfc3339(),
        "updated_at_ist": now_ist.to_rfc3339(),
        "pid": std::process::id(),
    });

    // Pretty-print is infallible for a plain object; fall back to a compact
    // form on the impossible error path rather than panicking.
    serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string())
}

/// Atomically write the breadcrumb to `<working_dir>/.bharatcode/ide-bridge.json`.
///
/// The directory is created if needed, the payload is first written to a
/// uniquely-named temp file in the same directory, and that temp file is then
/// renamed over the destination. Because the rename is atomic on a single
/// filesystem, a concurrent poller observes either the previous breadcrumb or
/// the new one in full — never a truncated write.
///
/// This is **best-effort**: any I/O failure (unwritable directory, rename
/// across filesystems, etc.) is swallowed. A breadcrumb is a convenience, never
/// a correctness requirement, so it must never interfere with the reply.
pub fn write_breadcrumb(session_id: &str, model: &str, status: &str, working_dir: &Path) {
    let _ = write_breadcrumb_inner(session_id, model, status, working_dir);
}

/// Inner worker returning `io::Result` so the happy path stays terse; the public
/// entry point discards the result to keep the feature best-effort.
fn write_breadcrumb_inner(
    session_id: &str,
    model: &str,
    status: &str,
    working_dir: &Path,
) -> std::io::Result<()> {
    let dir = working_dir.join(BREADCRUMB_DIR);
    std::fs::create_dir_all(&dir)?;

    let dest = dir.join(BREADCRUMB_FILE);
    let body = render_payload(session_id, model, status);

    // Temp file in the *same* directory so the final rename stays on one
    // filesystem (and is therefore atomic). The pid + a nanosecond stamp keep
    // concurrent writers from colliding on the temp name.
    let stamp = Utc::now().timestamp_nanos_opt().unwrap_or_default();
    let tmp = dir.join(format!(".ide-bridge.{}.{}.tmp", std::process::id(), stamp));

    std::fs::write(&tmp, body.as_bytes())?;
    match std::fs::rename(&tmp, &dest) {
        Ok(()) => Ok(()),
        Err(e) => {
            // Clean up the orphaned temp file on a failed rename; ignore any
            // secondary error.
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn is_enabled_is_false_when_unset() {
        // Snapshot and clear the var so the test is deterministic regardless of
        // the ambient environment, then restore it afterwards.
        let prior = std::env::var(IDE_BRIDGE_KEY).ok();
        std::env::remove_var(IDE_BRIDGE_KEY);
        assert!(
            !is_enabled(),
            "feature must default OFF when env var is unset"
        );
        if let Some(v) = prior {
            std::env::set_var(IDE_BRIDGE_KEY, v);
        }
    }

    #[test]
    fn write_breadcrumb_creates_parseable_json_with_expected_fields() {
        let tmp = tempfile::tempdir().expect("temp dir");
        write_breadcrumb("sess-abc", "model-x", "streaming", tmp.path());

        let path = tmp.path().join(BREADCRUMB_DIR).join(BREADCRUMB_FILE);
        assert!(path.exists(), "breadcrumb file must be created");

        let raw = std::fs::read_to_string(&path).expect("read breadcrumb");
        let v: Value = serde_json::from_str(&raw).expect("breadcrumb must be valid JSON");

        assert_eq!(v["session_id"], "sess-abc");
        assert_eq!(v["model"], "model-x");
        assert_eq!(v["status"], "streaming");
        assert!(v["updated_at_utc"].is_string());
        assert!(v["updated_at_ist"].is_string());
        assert!(v["pid"].is_number());
    }

    #[test]
    fn second_write_updates_timestamp_and_leaves_no_partial_file() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let dir = tmp.path().join(BREADCRUMB_DIR);
        let path = dir.join(BREADCRUMB_FILE);

        write_breadcrumb("sess-1", "model-1", "streaming", tmp.path());
        let first: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let first_ts = first["updated_at_utc"].as_str().unwrap().to_string();

        // Ensure the RFC3339 timestamp (nanosecond precision) actually advances.
        std::thread::sleep(std::time::Duration::from_millis(5));

        write_breadcrumb("sess-2", "model-2", "thinking", tmp.path());
        let second: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let second_ts = second["updated_at_utc"].as_str().unwrap().to_string();

        assert_eq!(second["session_id"], "sess-2");
        assert_eq!(second["status"], "thinking");
        assert_ne!(first_ts, second_ts, "updated_at must advance across writes");

        // Atomicity: the rename leaves exactly the destination file behind, with
        // no `.tmp` siblings lingering in the breadcrumb directory.
        let leftover_tmp = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().ends_with(".tmp"));
        assert!(
            !leftover_tmp,
            "no partial/temp file may remain after a write"
        );
    }

    #[test]
    fn disabled_path_writes_nothing() {
        // The streaming call site is guarded by `is_enabled()`; emulate the
        // disabled environment and confirm no breadcrumb is produced when the
        // guard would be false.
        let prior = std::env::var(IDE_BRIDGE_KEY).ok();
        std::env::remove_var(IDE_BRIDGE_KEY);

        let tmp = tempfile::tempdir().expect("temp dir");
        if is_enabled() {
            write_breadcrumb("sess", "model", "streaming", tmp.path());
        }
        let path = tmp.path().join(BREADCRUMB_DIR).join(BREADCRUMB_FILE);
        assert!(!path.exists(), "disabled feature must not write any file");

        if let Some(v) = prior {
            std::env::set_var(IDE_BRIDGE_KEY, v);
        }
    }

    #[test]
    fn payload_keys_carry_no_upstream_branding() {
        let body = render_payload("s", "m", "streaming");
        let v: Value = serde_json::from_str(&body).unwrap();
        let obj = v.as_object().unwrap();
        for key in obj.keys() {
            let lower = key.to_ascii_lowercase();
            assert!(
                !lower.contains("goose") && !lower.contains("block"),
                "payload key must not leak upstream branding: {key}"
            );
        }
    }
}
