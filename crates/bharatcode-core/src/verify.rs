//! Verify-before-done.
//!
//! After the agent finishes a turn that may have changed code, this module can
//! optionally detect the project's build system, run a lightweight verification
//! command (build / test / check), and classify the result into a single
//! `Verified` / `Failed` / `Skipped(<reason>)` status line.
//!
//! The whole feature is gated behind configuration and is **off by default** so
//! it never runs a build without the user opting in. Enable it with the
//! `BHARATCODE_VERIFY` boolean (env var or config parameter).
//!
//! Tuning parameters (all optional):
//!   * `BHARATCODE_VERIFY`            — enable the feature (bool, default `false`)
//!   * `BHARATCODE_VERIFY_TASK`       — `build` (default) | `test` | `check`
//!   * `BHARATCODE_VERIFY_TIMEOUT_SECS` — wall-clock budget, default `300`
//!
//! Status labels are localized via a small self-contained locale resolver that
//! mirrors the project's existing scaffold (`BHARATCODE_LANG` → `bharatcode_lang`
//! config → `LANG` → English). This module is original work; nothing here is
//! ported from third-party sources.

use std::io::ErrorKind;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

const ENABLE_KEY: &str = "BHARATCODE_VERIFY";
const TASK_KEY: &str = "BHARATCODE_VERIFY_TASK";
const TIMEOUT_KEY: &str = "BHARATCODE_VERIFY_TIMEOUT_SECS";
const DEFAULT_TIMEOUT_SECS: u64 = 300;

/// Build system detected from marker files in the working directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectKind {
    Cargo,
    Node,
    Go,
    Python,
    Make,
}

/// Which flavor of verification command to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Task {
    Build,
    Test,
    Check,
}

impl Task {
    fn parse(raw: &str) -> Task {
        match raw.trim().to_ascii_lowercase().as_str() {
            "test" => Task::Test,
            "check" => Task::Check,
            _ => Task::Build,
        }
    }
}

/// Outcome of a verification attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    Verified { command: String },
    Failed { command: String, detail: String },
    Skipped { reason: String },
}

impl Outcome {
    /// Render the single-line, localized status string.
    fn status_line(&self) -> String {
        match self {
            Outcome::Verified { command } => format!("{} ({})", label(Label::Verified), command),
            Outcome::Failed { command, detail } => {
                format!("{} ({}: {})", label(Label::Failed), command, detail)
            }
            Outcome::Skipped { reason } => format!("{}({})", label(Label::Skipped), reason),
        }
    }
}

/// Public entry point used by the agent finalization path.
///
/// Returns `None` when verification is disabled (the default), so the caller can
/// wire it in with a single `if let` and never pay any cost unless opted in.
/// When enabled, runs the verification and returns a localized status line.
pub async fn verify_and_format(working_dir: &Path) -> Option<String> {
    if !enabled() {
        return None;
    }
    Some(run(working_dir).await.status_line())
}

fn enabled() -> bool {
    if let Ok(raw) = std::env::var(ENABLE_KEY) {
        return is_truthy(&raw);
    }
    crate::config::Config::global()
        .get_param::<bool>(ENABLE_KEY)
        .unwrap_or(false)
}

fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn configured_task() -> Task {
    if let Ok(raw) = std::env::var(TASK_KEY) {
        if !raw.trim().is_empty() {
            return Task::parse(&raw);
        }
    }
    crate::config::Config::global()
        .get_param::<String>(TASK_KEY)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(|s| Task::parse(&s))
        .unwrap_or(Task::Build)
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

async fn run(working_dir: &Path) -> Outcome {
    let Some(kind) = detect(working_dir) else {
        return Outcome::Skipped {
            reason: label(Label::ReasonNoProject),
        };
    };
    let task = configured_task();
    let (program, args) = command_for(kind, task);
    let pretty = if args.is_empty() {
        program.to_string()
    } else {
        format!("{} {}", program, args.join(" "))
    };

    match spawn_and_wait(working_dir, program, &args, configured_timeout()).await {
        CmdResult::Success => Outcome::Verified { command: pretty },
        CmdResult::Failure { code } => Outcome::Failed {
            command: pretty,
            detail: match code {
                Some(c) => format!("{} {}", label(Label::DetailExit), c),
                None => label(Label::DetailSignal),
            },
        },
        CmdResult::TimedOut => Outcome::Failed {
            command: pretty,
            detail: label(Label::DetailTimedOut),
        },
        CmdResult::ToolMissing => Outcome::Skipped {
            reason: format!("{}: {}", label(Label::ReasonToolMissing), program),
        },
        CmdResult::SpawnError => Outcome::Skipped {
            reason: label(Label::ReasonSpawnError),
        },
    }
}

/// First marker file wins; ordering favors the most specific ecosystem.
fn detect(dir: &Path) -> Option<ProjectKind> {
    if dir.join("Cargo.toml").is_file() {
        return Some(ProjectKind::Cargo);
    }
    if dir.join("package.json").is_file() {
        return Some(ProjectKind::Node);
    }
    if dir.join("go.mod").is_file() {
        return Some(ProjectKind::Go);
    }
    if dir.join("pyproject.toml").is_file()
        || dir.join("setup.py").is_file()
        || dir.join("setup.cfg").is_file()
    {
        return Some(ProjectKind::Python);
    }
    if dir.join("Makefile").is_file() || dir.join("makefile").is_file() {
        return Some(ProjectKind::Make);
    }
    None
}

fn command_for(kind: ProjectKind, task: Task) -> (&'static str, Vec<&'static str>) {
    match kind {
        ProjectKind::Cargo => match task {
            Task::Build => ("cargo", vec!["build"]),
            Task::Test => ("cargo", vec!["test"]),
            Task::Check => ("cargo", vec!["check"]),
        },
        ProjectKind::Node => match task {
            Task::Build => ("npm", vec!["run", "build"]),
            Task::Test => ("npm", vec!["test"]),
            Task::Check => ("npm", vec!["run", "build"]),
        },
        ProjectKind::Go => match task {
            Task::Build => ("go", vec!["build", "./..."]),
            Task::Test => ("go", vec!["test", "./..."]),
            Task::Check => ("go", vec!["vet", "./..."]),
        },
        ProjectKind::Python => match task {
            Task::Build => ("python3", vec!["-m", "compileall", "-q", "."]),
            Task::Test => ("python3", vec!["-m", "pytest", "-q"]),
            Task::Check => ("python3", vec!["-m", "compileall", "-q", "."]),
        },
        ProjectKind::Make => match task {
            Task::Build => ("make", vec![]),
            Task::Test => ("make", vec!["test"]),
            Task::Check => ("make", vec![]),
        },
    }
}

enum CmdResult {
    Success,
    Failure { code: Option<i32> },
    TimedOut,
    ToolMissing,
    SpawnError,
}

async fn spawn_and_wait(dir: &Path, program: &str, args: &[&str], timeout: Duration) -> CmdResult {
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
                CmdResult::Failure {
                    code: status.code(),
                }
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
    Verified,
    Failed,
    Skipped,
    ReasonNoProject,
    ReasonToolMissing,
    ReasonSpawnError,
    DetailExit,
    DetailSignal,
    DetailTimedOut,
}

fn label(which: Label) -> String {
    let s = match (active_locale(), which) {
        (Locale::En, Label::Verified) => "Verified",
        (Locale::En, Label::Failed) => "Failed",
        (Locale::En, Label::Skipped) => "Skipped",
        (Locale::En, Label::ReasonNoProject) => "no recognized project",
        (Locale::En, Label::ReasonToolMissing) => "build tool not found",
        (Locale::En, Label::ReasonSpawnError) => "could not start verification",
        (Locale::En, Label::DetailExit) => "exit",
        (Locale::En, Label::DetailSignal) => "terminated by signal",
        (Locale::En, Label::DetailTimedOut) => "timed out",
        (Locale::Hi, Label::Verified) => "सत्यापित",
        (Locale::Hi, Label::Failed) => "विफल",
        (Locale::Hi, Label::Skipped) => "छोड़ा गया",
        (Locale::Hi, Label::ReasonNoProject) => "कोई पहचाना गया प्रोजेक्ट नहीं",
        (Locale::Hi, Label::ReasonToolMissing) => "बिल्ड टूल नहीं मिला",
        (Locale::Hi, Label::ReasonSpawnError) => "सत्यापन शुरू नहीं हो सका",
        (Locale::Hi, Label::DetailExit) => "निकास",
        (Locale::Hi, Label::DetailSignal) => "सिग्नल द्वारा समाप्त",
        (Locale::Hi, Label::DetailTimedOut) => "समय समाप्त",
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
    fn task_parsing_defaults_to_build() {
        assert_eq!(Task::parse("test"), Task::Test);
        assert_eq!(Task::parse("CHECK"), Task::Check);
        assert_eq!(Task::parse("build"), Task::Build);
        assert_eq!(Task::parse("whatever"), Task::Build);
        assert_eq!(Task::parse(""), Task::Build);
    }

    #[test]
    fn detect_picks_marker_files() {
        let dir = std::env::temp_dir().join(format!("bc_verify_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(detect(&dir), None);
        std::fs::write(dir.join("Cargo.toml"), "[package]").unwrap();
        assert_eq!(detect(&dir), Some(ProjectKind::Cargo));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn command_mapping_is_stable() {
        assert_eq!(
            command_for(ProjectKind::Cargo, Task::Build),
            ("cargo", vec!["build"])
        );
        assert_eq!(
            command_for(ProjectKind::Go, Task::Test),
            ("go", vec!["test", "./..."])
        );
        assert_eq!(
            command_for(ProjectKind::Node, Task::Test),
            ("npm", vec!["test"])
        );
    }

    #[test]
    fn status_lines_are_formatted() {
        let verified = Outcome::Verified {
            command: "cargo build".into(),
        };
        assert!(verified.status_line().contains("cargo build"));

        let skipped = Outcome::Skipped {
            reason: "no recognized project".into(),
        };
        let line = skipped.status_line();
        assert!(line.contains('('));
        assert!(line.ends_with(')'));
    }

    #[test]
    fn normalize_locale_maps_hindi_variants() {
        assert!(matches!(normalize_locale("hi"), Locale::Hi));
        assert!(matches!(normalize_locale("hi_IN.UTF-8"), Locale::Hi));
        assert!(matches!(normalize_locale("en_US"), Locale::En));
        assert!(matches!(normalize_locale("fr"), Locale::En));
    }
}
