//! Post-finalization CI / quality hook.
//!
//! After the agent finishes a turn that changed files, this module can
//! optionally run a project-detected CI / quality command (for example
//! `cargo fmt --check`, `npm run lint`, or a user-specified command) and emit a
//! single one-line `CI: passed/failed (<cmd>)` status so the local edit loop is
//! wired into the project's own quality gate.
//!
//! The whole feature is gated behind the `BHARATCODE_CI` environment variable
//! (or the config parameter of the same name) and is a no-op (returns `None`)
//! when unset, so the default finalization path is completely unchanged unless
//! the user opts in.
//!
//! The `BHARATCODE_CI` value does double duty:
//!   * a bare truthy boolean (`1` / `true` / `yes` / `on`) enables the feature
//!     and lets it auto-detect a command from a marker file in the working
//!     directory (`Cargo.toml` => `cargo fmt --check`, `package.json` =>
//!     `npm run lint --if-present`, `pyproject.toml` => `ruff check`);
//!   * any other non-empty value is treated as an explicit command string and is
//!     used verbatim (it also implies "enabled").
//!
//! Status labels are localized via a small self-contained locale resolver that
//! mirrors the project's existing scaffold (`BHARATCODE_LANG` → `bharatcode_lang`
//! config → `LANG` → English). This module is original work; nothing here is
//! ported from third-party sources.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

/// Environment variable / config parameter that turns the CI hook on. It also
/// carries the explicit command string when the user supplies one. Default: off.
const ENABLE_KEY: &str = "BHARATCODE_CI";

/// Wall-clock budget for the CI command. Tunable via `BHARATCODE_CI_TIMEOUT_SECS`.
const TIMEOUT_KEY: &str = "BHARATCODE_CI_TIMEOUT_SECS";
const DEFAULT_TIMEOUT_SECS: u64 = 300;

/// Whether the CI hook is enabled. Off by default.
///
/// Reads the raw `BHARATCODE_CI` environment variable first (any non-empty
/// value — truthy boolean or command string — enables it), then falls back to
/// the global config parameter of the same name, then defaults to `false`.
pub fn is_enabled() -> bool {
    if let Some(raw) = raw_value() {
        return !raw.trim().is_empty();
    }
    false
}

/// The configured `BHARATCODE_CI` value: raw environment variable first, then
/// the global config parameter. `None` when neither is set.
fn raw_value() -> Option<String> {
    if let Ok(raw) = std::env::var(ENABLE_KEY) {
        return Some(raw);
    }
    crate::config::Config::global()
        .get_param::<String>(ENABLE_KEY)
        .ok()
}

fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Resolve the CI command to run for `working_dir`, or `None` when the feature
/// is disabled or no command can be determined.
///
/// When `BHARATCODE_CI` holds an explicit command string (anything other than a
/// bare truthy boolean) it is split into tokens and used verbatim. Otherwise the
/// command is auto-detected from a marker file in `working_dir`:
///   * `Cargo.toml`     => `cargo fmt --check`
///   * `package.json`   => `npm run lint --if-present`
///   * `pyproject.toml` => `ruff check`
pub fn ci_command(working_dir: &Path) -> Option<Vec<String>> {
    let raw = raw_value()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    if !is_truthy(trimmed) {
        let parts: Vec<String> = trimmed.split_whitespace().map(|s| s.to_string()).collect();
        if parts.is_empty() {
            return None;
        }
        return Some(parts);
    }

    auto_detect(working_dir)
}

/// Auto-detect a CI / quality command from a marker file. First marker wins.
fn auto_detect(working_dir: &Path) -> Option<Vec<String>> {
    let words = |s: &str| s.split_whitespace().map(|w| w.to_string()).collect();
    if working_dir.join("Cargo.toml").is_file() {
        return Some(words("cargo fmt --check"));
    }
    if working_dir.join("package.json").is_file() {
        return Some(words("npm run lint --if-present"));
    }
    if working_dir.join("pyproject.toml").is_file() {
        return Some(words("ruff check"));
    }
    None
}

/// Public entry point used by the agent finalization path.
///
/// Returns `None` when the feature is disabled (the default), when no files were
/// changed in the turn, or when no CI command can be determined — so the caller
/// can wire it in with a single `if let` and never pay any cost unless opted in.
/// When enabled with changed files and a resolvable command, runs the command
/// and returns a one-line `CI: passed/failed (<cmd>)` status.
pub async fn run_ci(changed_files: &[PathBuf], working_dir: &Path) -> Option<String> {
    if !is_enabled() {
        return None;
    }
    if changed_files.is_empty() {
        return None;
    }
    let command = ci_command(working_dir)?;
    let (program, args) = command.split_first()?;
    let pretty = command.join(" ");

    let passed = match spawn_and_wait(working_dir, program, args, configured_timeout()).await {
        CmdResult::Success => true,
        CmdResult::Failure | CmdResult::TimedOut => false,
        // A missing tool or spawn error is not a CI failure to report; stay quiet.
        CmdResult::ToolMissing | CmdResult::SpawnError => return None,
    };

    let verdict = if passed {
        label(Label::Passed)
    } else {
        label(Label::Failed)
    };
    Some(format!("{} {} ({})", label(Label::Prefix), verdict, pretty))
}

fn configured_timeout() -> Duration {
    let secs = std::env::var(TIMEOUT_KEY)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .or_else(|| {
            crate::config::Config::global()
                .get_param::<u64>(TIMEOUT_KEY)
                .ok()
        })
        .filter(|s| *s > 0)
        .unwrap_or(DEFAULT_TIMEOUT_SECS);
    Duration::from_secs(secs)
}

enum CmdResult {
    Success,
    Failure,
    TimedOut,
    ToolMissing,
    SpawnError,
}

async fn spawn_and_wait(
    dir: &Path,
    program: &str,
    args: &[String],
    timeout: Duration,
) -> CmdResult {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) if e.kind() == ErrorKind::NotFound => return CmdResult::ToolMissing,
        Err(_) => return CmdResult::SpawnError,
    };

    match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => {
            if status.success() {
                CmdResult::Success
            } else {
                CmdResult::Failure
            }
        }
        Ok(Err(_)) => CmdResult::SpawnError,
        Err(_) => {
            let _ = child.start_kill();
            CmdResult::TimedOut
        }
    }
}

// ----------------------------------------------------------------------------
// Localization for the user-facing status labels.
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
enum Locale {
    En,
    Hi,
}

#[derive(Debug, Clone, Copy)]
enum Label {
    Prefix,
    Passed,
    Failed,
}

fn label(which: Label) -> String {
    let s = match (active_locale(), which) {
        (Locale::En, Label::Prefix) => "CI:",
        (Locale::En, Label::Passed) => "passed",
        (Locale::En, Label::Failed) => "failed",
        (Locale::Hi, Label::Prefix) => "CI:",
        (Locale::Hi, Label::Passed) => "सफल",
        (Locale::Hi, Label::Failed) => "विफल",
    };
    s.to_string()
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

    /// Serialize every env-touching test in this module so parallel tests never
    /// observe each other's `BHARATCODE_CI` value (the env is process-global).
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Minimal scoped env setter so enable/disable tests don't leak state. Holds
    /// the shared `ENV_LOCK` for its lifetime, restoring the key on drop.
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

    fn unique_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "bc_ci_{}_{}_{:?}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
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
        assert!(!is_truthy("cargo fmt --check"));
    }

    #[test]
    fn disabled_by_default_when_unset() {
        let _guard = EnvGuard::unset(ENABLE_KEY);
        assert!(!is_enabled());
    }

    #[test]
    fn ci_command_auto_detects_cargo_fmt_check() {
        let dir = unique_dir("cargo");
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();

        let _guard = EnvGuard::set(ENABLE_KEY, "1");
        assert_eq!(
            ci_command(&dir),
            Some(vec![
                "cargo".to_string(),
                "fmt".to_string(),
                "--check".to_string(),
            ])
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ci_command_auto_detects_node_and_python() {
        let node = unique_dir("node");
        std::fs::write(node.join("package.json"), "{}").unwrap();
        let py = unique_dir("py");
        std::fs::write(py.join("pyproject.toml"), "[project]\n").unwrap();

        let _guard = EnvGuard::set(ENABLE_KEY, "true");
        assert_eq!(
            ci_command(&node),
            Some(
                ["npm", "run", "lint", "--if-present"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            )
        );
        assert_eq!(
            ci_command(&py),
            Some(["ruff", "check"].iter().map(|s| s.to_string()).collect())
        );

        std::fs::remove_dir_all(&node).ok();
        std::fs::remove_dir_all(&py).ok();
    }

    #[test]
    fn ci_command_respects_explicit_override_string() {
        // Even with a Cargo.toml present, an explicit override wins verbatim.
        let dir = unique_dir("override");
        std::fs::write(dir.join("Cargo.toml"), "[package]\n").unwrap();

        let _guard = EnvGuard::set(ENABLE_KEY, "just lint");
        assert_eq!(
            ci_command(&dir),
            Some(vec!["just".to_string(), "lint".to_string()])
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ci_command_none_when_no_marker_and_truthy() {
        let dir = unique_dir("nomarker");
        let _guard = EnvGuard::set(ENABLE_KEY, "yes");
        assert_eq!(ci_command(&dir), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn run_ci_none_when_changed_files_empty() {
        let dir = unique_dir("empty");
        std::fs::write(dir.join("Cargo.toml"), "[package]\n").unwrap();

        let _guard = EnvGuard::set(ENABLE_KEY, "1");
        let changed: Vec<PathBuf> = Vec::new();
        assert_eq!(run_ci(&changed, &dir).await, None);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn run_ci_none_when_disabled() {
        let _guard = EnvGuard::unset(ENABLE_KEY);
        let dir = unique_dir("off");
        let changed = vec![dir.join("lib.rs")];
        assert_eq!(run_ci(&changed, &dir).await, None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn run_ci_message_format_uses_ci_prefix_and_no_upstream_brand() {
        // An explicit override pointing at a command that exits cleanly produces
        // a deterministic "passed" line. Skip the assertion gracefully on the
        // rare host that lacks `/usr/bin/true`.
        let dir = unique_dir("msg");
        let _guard = EnvGuard::set(ENABLE_KEY, "/usr/bin/true");

        let changed = vec![dir.join("lib.rs")];
        if let Some(produced) = run_ci(&changed, &dir).await {
            assert!(produced.contains("CI:"), "expected CI prefix: {produced}");
            assert!(
                produced.contains("passed"),
                "expected a passed verdict: {produced}"
            );
            let lower = produced.to_ascii_lowercase();
            assert!(!lower.contains("goose"), "must not leak brand: {produced}");
            assert!(!lower.contains("block"), "must not leak brand: {produced}");
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn message_label_format_contains_ci_and_no_brand() {
        let prefix = label(Label::Prefix);
        let passed = label(Label::Passed);
        let line = format!("{} {} ({})", prefix, passed, "cargo fmt --check");
        assert!(line.contains("CI:"), "expected CI prefix: {line}");
        let lower = line.to_ascii_lowercase();
        assert!(!lower.contains("goose"));
        assert!(!lower.contains("block"));
    }

    #[test]
    fn normalize_locale_maps_hindi_variants() {
        assert!(matches!(normalize_locale("hi"), Locale::Hi));
        assert!(matches!(normalize_locale("hi_IN.UTF-8"), Locale::Hi));
        assert!(matches!(normalize_locale("en_US"), Locale::En));
        assert!(matches!(normalize_locale("fr"), Locale::En));
    }
}
