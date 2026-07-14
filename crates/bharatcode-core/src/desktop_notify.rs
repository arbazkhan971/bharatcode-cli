//! Opt-in desktop notification on long-running turn completion.
//!
//! When the agent finishes a turn that ran longer than a configurable
//! threshold, this module can emit a single best-effort operating-system
//! notification so a user who stepped away from the terminal is told the work
//! is done. It is a pure side-effect: it never returns a value the agent acts
//! on and never blocks the finalization path.
//!
//! The whole feature is gated behind the `BHARATCODE_NOTIFY` boolean (env var
//! or config parameter) and is **off by default**, so the default finalization
//! path is byte-for-byte unchanged unless the user opts in.
//!
//! Delivery is layered and entirely best-effort — every backend probe and
//! spawn is fire-and-forget and all errors are swallowed:
//!   * Linux: `notify-send` (when present on `PATH`);
//!   * macOS: `osascript` (when present on `PATH`);
//!   * fallback on any platform: a terminal bell (`\x07`) written to stderr.
//!
//! Title/body strings are localized via a small self-contained locale resolver
//! that mirrors the project's existing scaffold (`BHARATCODE_LANG` →
//! `bharatcode_lang` config → `LANG` → English) and are deliberately
//! brand-neutral. This module is original work; nothing here is ported from
//! third-party sources.

use std::process::{Command, Stdio};

/// Environment variable / config parameter that turns the notification on.
/// Default: off.
const ENABLE_KEY: &str = "BHARATCODE_NOTIFY";

/// Threshold (in seconds) a turn must exceed before a notification is emitted.
/// Tunable via `BHARATCODE_NOTIFY_AFTER_SECS`.
const THRESHOLD_KEY: &str = "BHARATCODE_NOTIFY_AFTER_SECS";

/// Default threshold when unset or unparseable.
const DEFAULT_THRESHOLD_SECS: u64 = 20;
/// Lower clamp — a 0 (or below) threshold is meaningless, floor it to 1s.
const MIN_THRESHOLD_SECS: u64 = 1;
/// Upper clamp — an absurd threshold is capped so a typo cannot silently
/// disable the feature for the rest of a long session.
const MAX_THRESHOLD_SECS: u64 = 24 * 60 * 60;

/// Whether desktop notifications are enabled. Off by default.
///
/// Reads the raw `BHARATCODE_NOTIFY` environment variable first (truthy values
/// only), then falls back to the global config parameter of the same name,
/// then defaults to `false`.
pub fn is_enabled() -> bool {
    if let Ok(raw) = std::env::var(ENABLE_KEY) {
        return is_truthy(&raw);
    }
    if let Ok(raw) = crate::config::Config::global().get_param::<String>(ENABLE_KEY) {
        return is_truthy(&raw);
    }
    false
}

fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// The minimum turn duration (in seconds) that triggers a notification.
///
/// Reads `BHARATCODE_NOTIFY_AFTER_SECS` (env first, then config), defaults to
/// [`DEFAULT_THRESHOLD_SECS`], and clamps the result into
/// `[MIN_THRESHOLD_SECS, MAX_THRESHOLD_SECS]` so a 0 floors to 1 and an absurd
/// value caps out.
pub fn threshold_secs() -> u64 {
    let raw = std::env::var(THRESHOLD_KEY)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .or_else(|| {
            crate::config::Config::global()
                .get_param::<u64>(THRESHOLD_KEY)
                .ok()
        })
        .unwrap_or(DEFAULT_THRESHOLD_SECS);
    raw.clamp(MIN_THRESHOLD_SECS, MAX_THRESHOLD_SECS)
}

/// Build the default brand-neutral title/body pair for a completed turn,
/// localized off the active locale. The `elapsed_secs` is folded into the body
/// so the user knows roughly how long they were away.
pub fn completion_message(elapsed_secs: u64) -> (String, String) {
    let locale = active_locale();
    let title = match locale {
        Locale::En => "Task complete".to_string(),
        Locale::Hi => "कार्य पूर्ण".to_string(),
    };
    let body = match locale {
        Locale::En => format!("Your turn finished after {elapsed_secs}s."),
        Locale::Hi => format!("आपका टर्न {elapsed_secs}s में पूरा हुआ।"),
    };
    (title, body)
}

/// Emit a single best-effort desktop notification.
///
/// Tries the platform-native backend (`notify-send` on Linux, `osascript` on
/// macOS) when its binary is found on `PATH`; otherwise — and on any spawn
/// failure — falls back to a terminal bell on stderr. Every step is
/// fire-and-forget and never panics, so a host with no usable backend simply
/// rings the bell (or does nothing if stderr is closed) and returns.
pub fn notify(title: &str, body: &str) {
    if native_notify(title, body) {
        return;
    }
    bell_fallback();
}

/// Attempt a native notification. Returns `true` when a backend was found and
/// the spawn was issued (the child runs detached; we do not wait on it).
fn native_notify(title: &str, body: &str) -> bool {
    #[cfg(target_os = "linux")]
    {
        if has_binary("notify-send") {
            return spawn_detached("notify-send", &[title, body]);
        }
    }
    #[cfg(target_os = "macos")]
    {
        if has_binary("osascript") {
            let script = format!(
                "display notification {} with title {}",
                applescript_quote(body),
                applescript_quote(title)
            );
            return spawn_detached("osascript", &["-e", &script]);
        }
    }
    // Reference the args on platforms with no native backend so the signature
    // stays uniform without dead-code warnings.
    let _ = (title, body);
    false
}

/// Quote a string for safe embedding in an AppleScript string literal.
#[cfg(target_os = "macos")]
fn applescript_quote(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// `which`-style probe: is `name` an executable on `PATH`? Pure filesystem
/// inspection, no process spawned.
#[allow(dead_code)]
fn has_binary(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        let candidate = dir.join(name);
        is_executable_file(&candidate)
    })
}

#[allow(dead_code)]
fn is_executable_file(path: &std::path::Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Spawn a detached, silent child. Returns `true` when the spawn itself
/// succeeded; the child is intentionally not waited on. All output is
/// discarded.
#[allow(dead_code)]
fn spawn_detached(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

/// Last-resort fallback: ring the terminal bell on stderr. Errors are ignored.
fn bell_fallback() {
    use std::io::Write;
    let mut err = std::io::stderr();
    let _ = err.write_all(b"\x07");
    let _ = err.flush();
}

// ----------------------------------------------------------------------------
// Localization for the user-facing title/body.
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Locale {
    En,
    Hi,
}

fn normalize_locale(raw: &str) -> Locale {
    let lowered = raw.trim().to_ascii_lowercase();
    let primary = lowered.split(['_', '-', '.']).next().unwrap_or("");
    match primary {
        "hi" => Locale::Hi,
        _ => Locale::En,
    }
}

fn active_locale() -> Locale {
    if let Some(loc) = std::env::var("BHARATCODE_LANG")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        return normalize_locale(&loc);
    }
    if let Some(loc) = crate::config::Config::global()
        .get_param::<String>("bharatcode_lang")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        return normalize_locale(&loc);
    }
    if let Some(loc) = std::env::var("LANG").ok().filter(|s| !s.trim().is_empty()) {
        return normalize_locale(&loc);
    }
    Locale::En
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialize every env-touching test so parallel tests never observe each
    /// other's `BHARATCODE_*` values (the process env is global).
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct EnvGuard {
        key: &'static str,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> EnvGuard {
            let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            std::env::set_var(key, value);
            EnvGuard { key, _lock: lock }
        }

        fn unset(key: &'static str) -> EnvGuard {
            let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            std::env::remove_var(key);
            EnvGuard { key, _lock: lock }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            std::env::remove_var(self.key);
        }
    }

    #[test]
    fn truthy_only_for_known_affirmatives() {
        assert!(is_truthy("1"));
        assert!(is_truthy("true"));
        assert!(is_truthy(" YES "));
        assert!(is_truthy("On"));
        assert!(!is_truthy("0"));
        assert!(!is_truthy("false"));
        assert!(!is_truthy(""));
        assert!(!is_truthy("maybe"));
    }

    #[test]
    fn disabled_by_default_when_unset() {
        let _guard = EnvGuard::unset(ENABLE_KEY);
        assert!(!is_enabled());
    }

    #[test]
    fn enabled_when_env_is_one() {
        let _guard = EnvGuard::set(ENABLE_KEY, "1");
        assert!(is_enabled());
    }

    #[test]
    fn threshold_defaults_to_twenty_when_unset() {
        let _guard = EnvGuard::unset(THRESHOLD_KEY);
        assert_eq!(threshold_secs(), DEFAULT_THRESHOLD_SECS);
    }

    #[test]
    fn threshold_clamps_zero_up_to_floor() {
        let _guard = EnvGuard::set(THRESHOLD_KEY, "0");
        assert_eq!(threshold_secs(), MIN_THRESHOLD_SECS);
    }

    #[test]
    fn threshold_clamps_absurd_down_to_cap() {
        let _guard = EnvGuard::set(THRESHOLD_KEY, "999999999999");
        assert_eq!(threshold_secs(), MAX_THRESHOLD_SECS);
    }

    #[test]
    fn threshold_passes_through_reasonable_value() {
        let _guard = EnvGuard::set(THRESHOLD_KEY, "45");
        assert_eq!(threshold_secs(), 45);
    }

    #[test]
    fn notify_does_not_panic_without_backend() {
        // We cannot guarantee the absence of a backend on every host, but the
        // call must never panic regardless of what is (or is not) installed.
        // The bell fallback writes to stderr, which is always safe.
        notify("title", "body");
    }

    #[test]
    fn completion_message_is_non_empty_and_brand_neutral() {
        let (title, body) = completion_message(42);
        assert!(!title.trim().is_empty(), "title must be non-empty");
        assert!(!body.trim().is_empty(), "body must be non-empty");
        for s in [&title, &body] {
            let lower = s.to_ascii_lowercase();
            assert!(!lower.contains("goose"), "must not leak brand: {s}");
            assert!(!lower.contains("block"), "must not leak brand: {s}");
        }
    }

    #[test]
    fn completion_message_english_includes_elapsed() {
        let _guard = EnvGuard::set("BHARATCODE_LANG", "en_US.UTF-8");
        let (title, body) = completion_message(7);
        assert_eq!(title, "Task complete");
        assert!(body.contains("7s"), "body should mention elapsed: {body}");
    }

    #[test]
    fn normalize_locale_maps_hindi_variants() {
        assert_eq!(normalize_locale("hi"), Locale::Hi);
        assert_eq!(normalize_locale("hi_IN.UTF-8"), Locale::Hi);
        assert_eq!(normalize_locale("en_US"), Locale::En);
        assert_eq!(normalize_locale("fr"), Locale::En);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn applescript_quote_escapes_quotes_and_backslashes() {
        assert_eq!(applescript_quote("a\"b"), "\"a\\\"b\"");
        assert_eq!(applescript_quote("a\\b"), "\"a\\\\b\"");
    }
}
