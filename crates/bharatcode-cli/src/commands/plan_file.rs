//! Explicit plan-mode plan-file persistence for BharatCode.
//!
//! When a user drops into plan mode (`/plan`) and the reasoner produces a plan,
//! that plan only ever lives in the in-memory conversation. Once the turn is over
//! — or the session ends — the plan is gone unless the user happened to copy it
//! out. This module gives plans a durable home: a small markdown file under the
//! config dir, one per session, so a plan survives across turns and sessions and
//! can be re-read or shared later.
//!
//! Design:
//!   * **Default OFF.** Nothing is written unless `BHARATCODE_PLAN_FILE` is set to
//!     a truthy value. With it unset, the plan flow is byte-identical to before.
//!   * **One file per session** (`plan-<session_id>.md`) under the config dir, so
//!     re-planning within a session overwrites that session's plan-file with the
//!     latest plan rather than scattering files.
//!   * **Human-readable header.** Each file opens with a timestamp line in both
//!     IST (the relevant wall clock for Indian users) and UTC, mirroring the
//!     audit log's dual-timezone convention.
//!   * **Best-effort surface.** The writer is only ever reached from plan mode,
//!     so persistence is opt-in and never on the default path.
//!
//! Original BharatCode work; not ported from any third party.

use std::io::Write;
use std::path::PathBuf;

use bharatcode_core::config::paths::Paths;
use chrono::{DateTime, FixedOffset, Utc};

/// Environment key that turns plan-file persistence on. Absent / falsey => fully
/// disabled (default OFF — the plan flow is unchanged).
pub const PLAN_FILE_ENABLED_KEY: &str = "BHARATCODE_PLAN_FILE";

/// India Standard Time (UTC+05:30). Surfaced alongside UTC in the header so an
/// Indian user reads the local wall clock without losing the unambiguous UTC.
fn ist_offset() -> FixedOffset {
    FixedOffset::east_opt(5 * 3600 + 30 * 60).expect("IST (+05:30) is a valid fixed offset")
}

/// Whether plan-file persistence is enabled for this process.
///
/// Reads `BHARATCODE_PLAN_FILE` straight from the environment and accepts the
/// usual truthy spellings (`1`, `true`, `yes`, `on`); anything else — including
/// absence — is OFF.
pub fn is_enabled() -> bool {
    match std::env::var(PLAN_FILE_ENABLED_KEY) {
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

/// Absolute path of the plan-file for a given session.
///
/// One file per session (`plan-<session_id>.md`) under the config dir, mirroring
/// the pathing of the DPDP audit log.
pub fn plan_path(session_id: &str) -> PathBuf {
    Paths::in_config_dir(&format!("plan-{session_id}.md"))
}

/// Persist `plan` for `session_id` to its markdown plan-file (overwriting any
/// earlier plan for the same session).
///
/// Writes a dual-timezone header line (IST + UTC) followed by the plan body. The
/// caller is responsible for gating on [`is_enabled`]; this function always
/// writes when called so it can be unit-tested directly.
pub fn save_plan(session_id: &str, plan: &str) -> std::io::Result<()> {
    let path = plan_path(session_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::File::create(&path)?;
    write!(file, "{}", render(session_id, plan))?;
    Ok(())
}

/// Render the full plan-file contents (header + body) as a string.
fn render(session_id: &str, plan: &str) -> String {
    let now: DateTime<Utc> = Utc::now();
    let ist = now.with_timezone(&ist_offset()).format("%Y-%m-%d %H:%M:%S");
    let utc = now.format("%Y-%m-%d %H:%M:%S");
    let body = plan.trim_end();
    format!("<!-- BharatCode plan · session {session_id} · {ist} IST ({utc} UTC) -->\n\n{body}\n")
}

/// A human-readable one-liner pointing at the just-saved plan-file, or `None`
/// when no plan-file exists for the session yet.
///
/// Routed through the i18n layer so the pointer can be localised; the English
/// default is used whenever the active locale has no entry for the key.
pub fn latest_pointer(session_id: &str) -> Option<String> {
    let path = plan_path(session_id);
    if !path.exists() {
        return None;
    }
    Some(
        label("plan_file.saved", "Plan saved to {path}")
            .replace("{path}", &path.display().to_string()),
    )
}

/// Look up a user-facing label through the i18n layer, falling back to the
/// English `default` when the active locale table has no entry for `key`.
///
/// Mirrors the pattern in [`crate::commands::audit`]: `tr!` echoes the key back
/// when it is missing, so an unchanged key is treated as "untranslated".
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_then_read_back_has_header_and_body() {
        let dir = std::env::temp_dir().join(format!("bc-plan-test-{}", std::process::id()));
        let dir_str = dir.display().to_string();
        // Serialize the process-global BHARATCODE_PATH_ROOT mutation so this test
        // does not race with the sibling env-mutating test under parallelism.
        let _guard = env_lock::lock_env([("BHARATCODE_PATH_ROOT", Some(dir_str.as_str()))]);

        let session_id = "sess-plan-1";
        let path = plan_path(session_id);
        let _ = std::fs::remove_file(&path);

        let plan = "1. Read the file\n2. Make the change\n3. Run the tests";
        save_plan(session_id, plan).unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        // Header present (dual-timezone marker line).
        assert!(contents.contains("BharatCode plan"));
        assert!(contents.contains("IST"));
        assert!(contents.contains("UTC"));
        assert!(contents.contains(session_id));
        // Full plan body present.
        assert!(contents.contains("1. Read the file"));
        assert!(contents.contains("3. Run the tests"));

        // The pointer points at the file that now exists.
        let pointer = latest_pointer(session_id).expect("plan-file exists");
        assert!(pointer.contains(&path.display().to_string()));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn is_enabled_false_when_env_unset() {
        let _guard = env_lock::lock_env([(PLAN_FILE_ENABLED_KEY, None::<&str>)]);
        assert!(!is_enabled());
    }

    #[test]
    fn is_enabled_truthy_spellings() {
        let _guard = env_lock::lock_env([(PLAN_FILE_ENABLED_KEY, None::<&str>)]);
        for v in ["1", "true", "yes", "on", "ON", " Yes "] {
            std::env::set_var(PLAN_FILE_ENABLED_KEY, v);
            assert!(is_enabled(), "expected {v:?} to be truthy");
        }
        for v in ["0", "false", "no", "off", ""] {
            std::env::set_var(PLAN_FILE_ENABLED_KEY, v);
            assert!(!is_enabled(), "expected {v:?} to be falsey");
        }
        std::env::remove_var(PLAN_FILE_ENABLED_KEY);
    }

    #[test]
    fn latest_pointer_none_when_no_file() {
        let dir = std::env::temp_dir().join(format!("bc-plan-none-{}", std::process::id()));
        let dir_str = dir.display().to_string();
        let _guard = env_lock::lock_env([("BHARATCODE_PATH_ROOT", Some(dir_str.as_str()))]);
        let path = plan_path("sess-absent");
        let _ = std::fs::remove_file(&path);
        assert!(latest_pointer("sess-absent").is_none());
    }
}
