//! Best-effort, dependency-free desktop notifications for long interactive turns.
//!
//! Opt-in via `BHARATCODE_NOTIFY` (truthy). When an interactive agent turn runs
//! longer than `BHARATCODE_NOTIFY_MIN_SECS` (default 20s), the CLI fires a single
//! OS notification so the user can wander off and be pinged when work lands.
//!
//! The backend is intentionally zero-dependency: it shells out to `notify-send`
//! on Linux or `osascript` on macOS, and falls back to a terminal BEL (`\x07`)
//! when neither is available. Every entry point here is best-effort and never
//! errors out — notifications are a convenience, never a correctness concern, so
//! a missing binary or a sandboxed environment simply produces no ping.
//!
//! Defaults are OFF: with `BHARATCODE_NOTIFY` unset the interactive loop behaves
//! exactly as before.

/// Env var gating the whole feature. Unset / non-truthy = OFF.
const NOTIFY_ENABLED_KEY: &str = "BHARATCODE_NOTIFY";

/// Env var overriding the minimum elapsed-seconds threshold before a turn is
/// considered "long enough" to be worth a notification.
const NOTIFY_MIN_SECS_KEY: &str = "BHARATCODE_NOTIFY_MIN_SECS";

/// Test/override seam: force a specific backend. `none` disables the real
/// platform commands and the BEL fallback (used by the unit test so it can
/// exercise [`notify`] without touching the terminal or spawning processes).
const NOTIFY_BACKEND_KEY: &str = "BHARATCODE_NOTIFY_BACKEND";

/// User-facing notification title fired on long-turn completion.
pub const TURN_COMPLETE_TITLE: &str = "BharatCode: task complete";

/// User-facing notification body fired on long-turn completion.
pub const TURN_COMPLETE_BODY: &str = "Your interactive turn has finished.";

/// Default threshold: only turns of at least this many seconds are notified.
const DEFAULT_MIN_SECS: u64 = 20;

/// Upper clamp so a wildly large `BHARATCODE_NOTIFY_MIN_SECS` (e.g. a fat-finger
/// or an overflow attempt) can never silently disable notifications forever.
/// One hour is comfortably longer than any realistic interactive turn.
const MAX_MIN_SECS: u64 = 3600;

/// Whether desktop notifications are enabled for this process.
///
/// Reads `BHARATCODE_NOTIFY` straight from the environment and accepts the usual
/// truthy spellings (`1`, `true`, `yes`, `on`); anything else — including
/// absence — is OFF. The raw-env read mirrors the other `BHARATCODE_*` gates so
/// a bare `1` survives instead of being coerced through the typed config layer.
pub fn is_enabled() -> bool {
    match std::env::var(NOTIFY_ENABLED_KEY) {
        Ok(raw) => is_truthy(&raw),
        Err(_) => false,
    }
}

/// Minimum elapsed seconds before a finished turn is worth a notification.
///
/// Defaults to [`DEFAULT_MIN_SECS`]. A `BHARATCODE_NOTIFY_MIN_SECS` that fails to
/// parse falls back to the default; a parsed value is clamped to
/// `[0, MAX_MIN_SECS]` so an absurd value can neither overflow nor mute the
/// feature indefinitely.
pub fn min_secs() -> u64 {
    match std::env::var(NOTIFY_MIN_SECS_KEY) {
        Ok(raw) => match raw.trim().parse::<u64>() {
            Ok(v) => v.min(MAX_MIN_SECS),
            Err(_) => DEFAULT_MIN_SECS,
        },
        Err(_) => DEFAULT_MIN_SECS,
    }
}

fn is_truthy(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Fire a single best-effort desktop notification.
///
/// Picks a backend in this order:
/// * `BHARATCODE_NOTIFY_BACKEND=none` — do nothing (test/override seam).
/// * Linux — spawn `notify-send <title> <body>`.
/// * macOS — spawn `osascript -e 'display notification ...'`.
/// * anything else, or if the spawn fails — emit a terminal BEL (`\x07`).
///
/// Always returns `Ok(())`: a missing binary, a failed spawn, or an exotic
/// platform are all swallowed. The returned `Result` exists only so callers may
/// `let _ =` it uniformly; it never carries a hard error.
pub fn notify(title: &str, body: &str) -> std::io::Result<()> {
    if let Ok(backend) = std::env::var(NOTIFY_BACKEND_KEY) {
        if backend.trim().eq_ignore_ascii_case("none") {
            return Ok(());
        }
    }

    let spawned = spawn_platform_notification(title, body);
    if !spawned {
        // No platform backend available (or it failed): fall back to a BEL so
        // there is at least an audible/visual cue in the terminal.
        print!("\x07");
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }
    Ok(())
}

/// Try to spawn the platform-native notification command. Returns `true` only if
/// a command was successfully launched; any failure (missing binary, spawn
/// error, unsupported platform) returns `false` so the caller can fall back.
#[cfg(target_os = "linux")]
fn spawn_platform_notification(title: &str, body: &str) -> bool {
    use std::process::Command;
    Command::new("notify-send")
        .arg(title)
        .arg(body)
        .spawn()
        .is_ok()
}

#[cfg(target_os = "macos")]
fn spawn_platform_notification(title: &str, body: &str) -> bool {
    use std::process::Command;
    // AppleScript string-escape: backslashes and quotes only.
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!(
        "display notification \"{}\" with title \"{}\"",
        esc(body),
        esc(title)
    );
    Command::new("osascript")
        .arg("-e")
        .arg(script)
        .spawn()
        .is_ok()
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn spawn_platform_notification(_title: &str, _body: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Env vars are process-global; serialize the env-touching tests so they do
    // not race each other under the test harness's thread pool.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn is_enabled_false_when_unset() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(NOTIFY_ENABLED_KEY);
        assert!(!is_enabled());
    }

    #[test]
    fn is_enabled_true_on_one() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(NOTIFY_ENABLED_KEY, "1");
        assert!(is_enabled());
        std::env::remove_var(NOTIFY_ENABLED_KEY);
    }

    #[test]
    fn min_secs_defaults_to_twenty() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(NOTIFY_MIN_SECS_KEY);
        assert_eq!(min_secs(), DEFAULT_MIN_SECS);
        assert_eq!(min_secs(), 20);
    }

    #[test]
    fn min_secs_clamps_huge_value() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(NOTIFY_MIN_SECS_KEY, "99999999999");
        assert_eq!(min_secs(), MAX_MIN_SECS);
        std::env::remove_var(NOTIFY_MIN_SECS_KEY);
    }

    #[test]
    fn min_secs_falls_back_on_garbage() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(NOTIFY_MIN_SECS_KEY, "not-a-number");
        assert_eq!(min_secs(), DEFAULT_MIN_SECS);
        std::env::remove_var(NOTIFY_MIN_SECS_KEY);
    }

    #[test]
    fn notify_with_none_backend_is_ok_and_silent() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(NOTIFY_BACKEND_KEY, "none");
        // Must not panic, must not spawn anything, must not touch the terminal.
        let result = notify("BharatCode", "task complete");
        assert!(result.is_ok());
        std::env::remove_var(NOTIFY_BACKEND_KEY);
    }

    #[test]
    fn is_truthy_recognizes_common_values() {
        assert!(is_truthy("1"));
        assert!(is_truthy("TRUE"));
        assert!(is_truthy(" on "));
        assert!(is_truthy("yes"));
        assert!(!is_truthy("0"));
        assert!(!is_truthy(""));
        assert!(!is_truthy("off"));
    }
}
