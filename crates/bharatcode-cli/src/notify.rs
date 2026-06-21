//! Opt-in, best-effort desktop notification on long interactive turn completion.
//!
//! When `BHARATCODE_NOTIFY` is truthy and an *interactive* agent turn runs longer
//! than [`threshold`] (default 30s, override via `BHARATCODE_NOTIFY_THRESHOLD_SECS`),
//! the CLI fires a single OS notification so a user who tabbed away gets pinged
//! when the work lands.
//!
//! The backend is intentionally zero-dependency and platform-native: it shells
//! out to `notify-send` on Linux, `osascript` on macOS, or a PowerShell toast on
//! Windows, each spawned detached. Everything here is strictly best-effort — a
//! missing binary, a sandboxed environment, or an unsupported platform simply
//! produces no ping. All errors are swallowed; [`notify`] never panics and never
//! propagates a failure.
//!
//! Defaults are OFF: with `BHARATCODE_NOTIFY` unset the interactive loop behaves
//! exactly as before, so this module is zero behavior change by default.

use std::time::Duration;

/// Env var gating the whole feature. Unset / non-truthy = OFF.
const NOTIFY_ENABLED_KEY: &str = "BHARATCODE_NOTIFY";

/// Env var overriding the elapsed-seconds threshold before a finished turn is
/// considered "long enough" to be worth a notification.
const NOTIFY_THRESHOLD_KEY: &str = "BHARATCODE_NOTIFY_THRESHOLD_SECS";

/// Test/override seam: force a specific backend. `none` disables the real
/// platform commands entirely so the unit test can exercise [`notify`] without
/// spawning any process.
const NOTIFY_BACKEND_KEY: &str = "BHARATCODE_NOTIFY_BACKEND";

/// Default threshold in seconds: only turns of at least this long are notified.
const DEFAULT_THRESHOLD_SECS: u64 = 30;

/// Lower clamp. A threshold of zero would notify on every interactive turn, which
/// defeats the "long-running" intent; one second is the smallest meaningful gate.
const MIN_THRESHOLD_SECS: u64 = 1;

/// Upper clamp so a fat-fingered or overflow-y `BHARATCODE_NOTIFY_THRESHOLD_SECS`
/// can never silently mute notifications forever. One hour is comfortably longer
/// than any realistic interactive turn.
const MAX_THRESHOLD_SECS: u64 = 3600;

/// Whether desktop notifications are enabled for this process.
///
/// Reads `BHARATCODE_NOTIFY` straight from the environment ("raw-env-first") and
/// accepts the usual truthy spellings (`1`, `true`, `yes`, `on`); anything else —
/// including absence — is OFF. The raw-env read mirrors the other `BHARATCODE_*`
/// gates so a bare `1` survives instead of being coerced through a typed config
/// layer.
pub fn is_enabled() -> bool {
    match std::env::var(NOTIFY_ENABLED_KEY) {
        Ok(raw) => is_truthy(&raw),
        Err(_) => false,
    }
}

/// Minimum elapsed time before a finished turn is worth a notification.
///
/// Defaults to [`DEFAULT_THRESHOLD_SECS`]. A `BHARATCODE_NOTIFY_THRESHOLD_SECS`
/// that fails to parse falls back to the default; a parsed value is clamped to
/// `[MIN_THRESHOLD_SECS, MAX_THRESHOLD_SECS]` so neither `0` nor an absurd value
/// can overflow, notify-spam, or mute the feature indefinitely.
pub fn threshold() -> Duration {
    let secs = match std::env::var(NOTIFY_THRESHOLD_KEY) {
        Ok(raw) => match raw.trim().parse::<u64>() {
            Ok(v) => v.clamp(MIN_THRESHOLD_SECS, MAX_THRESHOLD_SECS),
            Err(_) => DEFAULT_THRESHOLD_SECS,
        },
        Err(_) => DEFAULT_THRESHOLD_SECS,
    };
    Duration::from_secs(secs)
}

fn is_truthy(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Fire a single best-effort desktop notification.
///
/// Picks a platform-native backend via `cfg(target_os)` and spawns it detached:
/// * Linux — `notify-send <title> <body>`.
/// * macOS — `osascript -e 'display notification ...'`.
/// * Windows — a PowerShell toast.
/// * any other platform — no-op.
///
/// The `BHARATCODE_NOTIFY_BACKEND=none` override short-circuits to a no-op before
/// any spawn (test/override seam). Every error — a missing binary, a failed
/// spawn, an exotic platform — is swallowed; this function never panics and never
/// returns an error.
pub fn notify(title: &str, body: &str) {
    if let Ok(backend) = std::env::var(NOTIFY_BACKEND_KEY) {
        if backend.trim().eq_ignore_ascii_case("none") {
            return;
        }
    }
    spawn_platform_notification(title, body);
}

/// Spawn the platform-native notification command, detached. Any failure (missing
/// binary, spawn error, unsupported platform) is swallowed.
#[cfg(target_os = "linux")]
fn spawn_platform_notification(title: &str, body: &str) {
    use std::process::Command;
    let _ = Command::new("notify-send").arg(title).arg(body).spawn();
}

#[cfg(target_os = "macos")]
fn spawn_platform_notification(title: &str, body: &str) {
    use std::process::Command;
    // AppleScript string-escape: backslashes and quotes only.
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!(
        "display notification \"{}\" with title \"{}\"",
        esc(body),
        esc(title)
    );
    let _ = Command::new("osascript").arg("-e").arg(script).spawn();
}

#[cfg(target_os = "windows")]
fn spawn_platform_notification(title: &str, body: &str) {
    use std::process::Command;
    // Single-quote escape for the PowerShell string literals.
    let esc = |s: &str| s.replace('\'', "''");
    let script = format!(
        "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType=WindowsRuntime] | Out-Null; \
         $t=[Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
         $x=$t.GetElementsByTagName('text'); $x.Item(0).AppendChild($t.CreateTextNode('{}')) | Out-Null; \
         $x.Item(1).AppendChild($t.CreateTextNode('{}')) | Out-Null; \
         [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('BharatCode').Show([Windows.UI.Notifications.ToastNotification]::new($t))",
        esc(title),
        esc(body)
    );
    let _ = Command::new("powershell")
        .arg("-NoProfile")
        .arg("-Command")
        .arg(script)
        .spawn();
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn spawn_platform_notification(_title: &str, _body: &str) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Env vars are process-global; serialize the env-touching tests so they do
    // not race each other under the test harness's thread pool.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn threshold_defaults_to_thirty_seconds() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(NOTIFY_THRESHOLD_KEY);
        assert_eq!(threshold(), Duration::from_secs(30));
        assert_eq!(threshold(), Duration::from_secs(DEFAULT_THRESHOLD_SECS));
    }

    #[test]
    fn threshold_clamps_zero_to_floor() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(NOTIFY_THRESHOLD_KEY, "0");
        assert_eq!(threshold(), Duration::from_secs(MIN_THRESHOLD_SECS));
        std::env::remove_var(NOTIFY_THRESHOLD_KEY);
    }

    #[test]
    fn threshold_clamps_absurd_value_to_ceiling() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(NOTIFY_THRESHOLD_KEY, "99999999999");
        assert_eq!(threshold(), Duration::from_secs(MAX_THRESHOLD_SECS));
        std::env::remove_var(NOTIFY_THRESHOLD_KEY);
    }

    #[test]
    fn threshold_falls_back_on_garbage() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(NOTIFY_THRESHOLD_KEY, "not-a-number");
        assert_eq!(threshold(), Duration::from_secs(DEFAULT_THRESHOLD_SECS));
        std::env::remove_var(NOTIFY_THRESHOLD_KEY);
    }

    #[test]
    fn is_enabled_false_when_unset() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(NOTIFY_ENABLED_KEY);
        assert!(!is_enabled());
    }

    #[test]
    fn is_enabled_true_on_truthy_values() {
        let _guard = ENV_LOCK.lock().unwrap();
        for v in ["1", "true", "YES", " on "] {
            std::env::set_var(NOTIFY_ENABLED_KEY, v);
            assert!(is_enabled(), "expected {v:?} to enable notifications");
        }
        std::env::remove_var(NOTIFY_ENABLED_KEY);
    }

    #[test]
    fn notify_with_none_backend_is_a_silent_no_op() {
        let _guard = ENV_LOCK.lock().unwrap();
        // The `none` backend short-circuits before any spawn: this must return
        // without panicking and without launching a process.
        std::env::set_var(NOTIFY_BACKEND_KEY, "none");
        notify("BharatCode", "task complete");
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
