//! Opt-in startup integrity quick-check for the session database.
//!
//! At session build time this runs a fast, best-effort `PRAGMA quick_check`
//! against the same SQLite file that `SessionStorage` opens
//! (`<data_dir>/sessions/sessions.db`). If SQLite reports physical corruption,
//! we surface a clear `tracing::warn!` plus a single user-visible heal pointer
//! at the existing `bharatcode db --vacuum` repair path, so the user gets an
//! early, actionable warning instead of an opaque sqlx error deeper in the run.
//!
//! This is deliberately cheap and non-blocking: it opens its *own* short-lived
//! read-only connection (never the shared `SessionStorage` pool), and any error
//! — missing file, lock contention, open failure — is swallowed so startup is
//! never delayed or aborted. A healthy database, or a check that fails to run,
//! is completely silent.
//!
//! The user-visible heal pointer is gated behind the `BHARATCODE_DB_PREFLIGHT`
//! environment variable (truthy => on). With the gate off (the default), the
//! warning is emitted only to the `tracing` log, keeping default startup output
//! byte-for-byte unchanged. The `quick_check` itself always runs; only the
//! extra user-facing line is gated.
//!
//! Distinct from the schema-migration advisory: that guards schema correctness;
//! this is a physical-integrity early warning.
//!
//! Original BharatCode work; not ported from any third party.

use std::path::{Path, PathBuf};

use bharatcode_core::config::paths::Paths;
use bharatcode_core::session::session_manager::{DB_NAME, SESSIONS_FOLDER};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::Row;

/// Environment switch that turns the user-visible heal pointer on.
///
/// Off (unset / non-truthy) by default => warning goes to `tracing` only and
/// default startup output is unchanged.
const PREFLIGHT_ENV: &str = "BHARATCODE_DB_PREFLIGHT";

/// Resolve the same `<data_dir>/sessions/sessions.db` path that
/// `SessionStorage::new` derives.
fn session_db_path() -> PathBuf {
    Paths::data_dir().join(SESSIONS_FOLDER).join(DB_NAME)
}

/// Parse a boolean-ish flag the way the rest of the CLI does: trimmed,
/// case-insensitive, with a small set of accepted truthy spellings. Anything
/// unrecognised (including a typo) is OFF so the gate never flips on by accident.
fn is_truthy(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on" | "enable" | "enabled"
    )
}

/// Whether the user-visible heal pointer is enabled. Environment-first; absent
/// or non-truthy => disabled.
fn hint_enabled() -> bool {
    std::env::var(PREFLIGHT_ENV)
        .map(|raw| is_truthy(&raw))
        .unwrap_or(false)
}

/// Build the heal advice for a `PRAGMA quick_check` result.
///
/// Returns `None` when SQLite reports `ok` (the healthy case). Otherwise returns
/// `Some` with concrete repair steps that point at the existing
/// `bharatcode db --vacuum` path plus a backup-and-recreate fallback. The exact
/// `quick_check` output is intentionally *not* interpolated into the advice:
/// the steps are stable and self-contained.
pub fn heal_advice(quick_check_result: &str) -> Option<String> {
    if quick_check_result.trim().eq_ignore_ascii_case("ok") {
        return None;
    }
    Some(
        "session database integrity check reported a problem. \
         To repair: run `bharatcode db` to inspect it, then `bharatcode db --vacuum` to \
         compact and rebuild it. If that does not clear the warning, back up the \
         sessions.db file and remove it so a fresh database is recreated on next start."
            .to_string(),
    )
}

/// Open a short-lived, read-only pool against `path` (never the shared
/// `SessionStorage` pool). `create_if_missing(false)` keeps the check honest: a
/// missing file is an error here, not a silently-materialised empty database.
async fn open_read_only(path: &Path) -> sqlx::Result<sqlx::Pool<sqlx::Sqlite>> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false)
        .read_only(true)
        .busy_timeout(std::time::Duration::from_secs(2));

    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
}

/// Run `PRAGMA quick_check` against `path` and return its first-row text.
///
/// A healthy database yields `"ok"`. Any failure to open or query is propagated
/// as `Err` so the caller can swallow it (best-effort: never block startup).
async fn run_quick_check(path: &Path) -> sqlx::Result<String> {
    let pool = open_read_only(path).await?;
    let result: String = sqlx::query("PRAGMA quick_check")
        .fetch_one(&pool)
        .await?
        .try_get(0)?;
    pool.close().await;
    Ok(result)
}

/// Best-effort startup integrity quick-check.
///
/// Resolves the session database path, and — only if the file exists — runs
/// `PRAGMA quick_check` on a short read-only connection. On a non-`ok` result it
/// logs a `tracing::warn!` and, when `BHARATCODE_DB_PREFLIGHT` is truthy, prints
/// a single user-visible heal pointer. A missing file, an open/query failure, or
/// a healthy database all resolve to `Ok(None)` without panicking. Returns the
/// raw `quick_check` string (`Some`) when the check actually ran.
pub async fn preflight() -> sqlx::Result<Option<String>> {
    preflight_path(&session_db_path()).await
}

/// Inner worker for [`preflight`], parameterised on the database path so it is
/// testable without touching the real data directory.
async fn preflight_path(path: &Path) -> sqlx::Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }

    let result = match run_quick_check(path).await {
        Ok(result) => result,
        // Swallow: a failed check must never block or slow down startup.
        Err(e) => {
            tracing::debug!(error = %e, "session-db quick_check could not run; skipping");
            return Ok(None);
        }
    };

    if let Some(advice) = heal_advice(&result) {
        tracing::warn!(
            quick_check = %result.trim(),
            path = %path.display(),
            "session database failed integrity quick_check"
        );
        if hint_enabled() {
            eprintln!("{}", crate::tr!("db.preflight.hint"));
            eprintln!("{advice}");
        }
    }

    Ok(Some(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heal_advice_none_when_ok() {
        assert!(heal_advice("ok").is_none());
        assert!(heal_advice("OK").is_none());
        assert!(heal_advice("  ok  ").is_none());
    }

    #[test]
    fn heal_advice_some_points_at_vacuum_for_corruption() {
        let advice = heal_advice("*** in database main ***\nrow 3 missing from index idx")
            .expect("non-ok quick_check should yield repair advice");
        assert!(
            advice.contains("db --vacuum"),
            "advice must point at the `bharatcode db --vacuum` repair path: {advice}"
        );
    }

    #[test]
    fn heal_advice_some_for_arbitrary_non_ok() {
        assert!(heal_advice("not ok").is_some());
        assert!(heal_advice("").is_some());
    }

    #[test]
    fn is_truthy_matches_documented_spellings() {
        for v in ["1", "true", "TRUE", " yes ", "on", "enable", "enabled"] {
            assert!(is_truthy(v), "{v:?} should be truthy");
        }
        for v in ["0", "false", "", "maybe", "off"] {
            assert!(!is_truthy(v), "{v:?} should not be truthy");
        }
    }

    #[tokio::test]
    async fn run_quick_check_missing_path_is_err() {
        // A non-existent file cannot be opened read-only with
        // create_if_missing(false): the check surfaces an error the caller swallows.
        let missing = std::path::Path::new("/nonexistent/bharatcode-preflight/sessions.db");
        assert!(run_quick_check(missing).await.is_err());
    }

    #[tokio::test]
    async fn preflight_missing_db_is_ok_none() {
        // A missing database path must resolve to Ok(None) without panicking:
        // a fresh install (no sessions.db yet) is the common case and must be silent.
        let missing = std::path::Path::new("/nonexistent/bharatcode-preflight/sessions.db");
        let result = preflight_path(missing).await;
        assert!(matches!(result, Ok(None)));
    }
}
