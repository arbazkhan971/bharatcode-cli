//! CI-integration readiness doctor check — BharatCode v77.
//!
//! A single, read-only diagnostic that reports whether the current repository is
//! wired to run BharatCode non-interactively in continuous integration. It is
//! the kind of pre-flight a team wants before relying on `bharatcode` inside a
//! pipeline:
//!
//!   1. **Provider detection** — which CI provider, if any, the repo declares:
//!      GitHub Actions (`.github/workflows/*.yml|*.yaml`), GitLab CI
//!      (`.gitlab-ci.yml`), or Jenkins (`Jenkinsfile`).
//!   2. **BharatCode step presence** — whether any of those config files already
//!      invoke a `bharatcode` command (e.g. `bharatcode run --recipe ...`), so
//!      the operator knows the agent is actually exercised in CI.
//!   3. **Headless readiness** — whether `BHARATCODE_AUTOMATION` is set in the
//!      current environment, surfaced as a hint so the operator knows the
//!      non-interactive switch is in effect.
//!
//! The probe is deliberately conservative and side-effect free: it only ever
//! *reads* a bounded set of well-known config files (a hard ceiling on the
//! number of workflow files inspected and on bytes read per file, so a
//! pathological repo can never blow up wall-clock time), and it never writes,
//! mutates config, shells out, or contacts the network. There is no env gate —
//! a read-only detector is always safe to run.

use std::path::Path;

use crate::commands::doctor_checks::Status;

/// Environment key signalling that BharatCode should run headless / batch.
/// Read-only here: this check only reports whether it is set, never flips it.
const AUTOMATION_KEY: &str = "BHARATCODE_AUTOMATION";

/// Hard ceiling on the number of GitHub Actions workflow files inspected, so a
/// repo with a pathological `.github/workflows` directory can never make the
/// scan run unbounded.
const MAX_WORKFLOW_FILES: usize = 64;

/// Hard ceiling on bytes read from any single config file when grepping for a
/// `bharatcode` invocation. A workflow that legitimately invokes the agent does
/// so well within the first few KB; anything larger is almost certainly
/// generated noise and reading it in full would only waste time.
const MAX_BYTES_PER_FILE: usize = 256 * 1024;

/// A detected CI provider and whether its config already invokes `bharatcode`.
struct Detection {
    /// Human-readable provider name (e.g. "GitHub Actions").
    provider: &'static str,
    /// Whether any inspected config file references a `bharatcode` command.
    has_bharatcode_step: bool,
}

/// Read at most [`MAX_BYTES_PER_FILE`] bytes of `path` as UTF-8 (lossy), or
/// `None` when the file cannot be read. Bounding the read keeps the probe cheap
/// and safe on arbitrarily large files.
fn read_bounded(path: &Path) -> Option<String> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    let mut handle = file.take(MAX_BYTES_PER_FILE as u64);
    handle.read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Whether `text` contains a `bharatcode` command invocation.
///
/// We look for the bare `bharatcode` token rather than the full
/// `bharatcode run` phrase so a wider range of real invocations match
/// (`bharatcode run`, `./bharatcode run`, `bharatcode --version` in a smoke
/// step, a wrapper that calls `bharatcode session ...`). A readiness signal only
/// needs to know the agent is referenced at all.
fn mentions_bharatcode(text: &str) -> bool {
    text.contains("bharatcode")
}

/// Collect GitHub Actions workflow files under `.github/workflows`, bounded to
/// [`MAX_WORKFLOW_FILES`] entries and to `*.yml` / `*.yaml` files. Returns an
/// empty vec when the directory is absent.
fn github_workflow_files(root: &Path) -> Vec<std::path::PathBuf> {
    let dir = root.join(".github").join("workflows");
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut files = Vec::new();
    for entry in entries.flatten() {
        if files.len() >= MAX_WORKFLOW_FILES {
            break;
        }
        let path = entry.path();
        let is_yaml = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("yml") || e.eq_ignore_ascii_case("yaml"))
            .unwrap_or(false);
        if is_yaml && path.is_file() {
            files.push(path);
        }
    }
    files
}

/// Detect the CI provider declared in `root` (if any) and whether its config
/// already invokes `bharatcode`.
///
/// Detection is read-only and stops at the first provider found, in a fixed
/// priority order (GitHub Actions, then GitLab CI, then Jenkins) so the single
/// reported row is deterministic. When a provider is found, all of its config
/// files are grepped for a `bharatcode` invocation.
fn detect(root: &Path) -> Option<Detection> {
    // GitHub Actions: any *.yml / *.yaml under .github/workflows.
    let gh_files = github_workflow_files(root);
    if !gh_files.is_empty() {
        let has_step = gh_files
            .iter()
            .filter_map(|p| read_bounded(p))
            .any(|text| mentions_bharatcode(&text));
        return Some(Detection {
            provider: "GitHub Actions",
            has_bharatcode_step: has_step,
        });
    }

    // GitLab CI: a top-level .gitlab-ci.yml.
    let gitlab = root.join(".gitlab-ci.yml");
    if gitlab.is_file() {
        let has_step = read_bounded(&gitlab)
            .map(|text| mentions_bharatcode(&text))
            .unwrap_or(false);
        return Some(Detection {
            provider: "GitLab CI",
            has_bharatcode_step: has_step,
        });
    }

    // Jenkins: a top-level Jenkinsfile.
    let jenkins = root.join("Jenkinsfile");
    if jenkins.is_file() {
        let has_step = read_bounded(&jenkins)
            .map(|text| mentions_bharatcode(&text))
            .unwrap_or(false);
        return Some(Detection {
            provider: "Jenkins",
            has_bharatcode_step: has_step,
        });
    }

    None
}

/// Whether the headless / automation switch is set in the environment. A bare
/// presence (even empty) counts: CI commonly exports `BHARATCODE_AUTOMATION=1`,
/// but any explicit set signals intent to run non-interactively.
fn automation_enabled() -> bool {
    std::env::var_os(AUTOMATION_KEY).is_some()
}

/// Look up a user-facing string through the i18n layer, falling back to the
/// English `default` when the active locale table has no entry for `key`.
///
/// `t()` echoes the key back when it is missing, so an unchanged key is treated
/// as "untranslated". Mirrors the helper in `doctor.rs`/`index_check.rs` so the
/// row renders in English without depending on the i18n table.
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// Report whether `cwd` is ready to run BharatCode in CI.
///
/// Returns a [`Status`] plus a human-readable message naming the detected CI
/// provider (if any), whether a `bharatcode` step is already present, and the
/// headless-automation state. The result is always non-fatal:
///
/// * [`Status::Ok`] — a known CI provider is configured **and** already invokes
///   `bharatcode`.
/// * [`Status::Warn`] — a known CI provider is configured but no `bharatcode`
///   step was found (the agent is not yet exercised in the pipeline).
/// * [`Status::Ok`] (neutral) — no CI config detected; there is nothing to wire
///   yet, so this is reported as a benign, non-error state.
pub fn ci_readiness(cwd: &Path) -> (Status, String) {
    let lbl = label("doctor.check.ci_readiness", "CI");

    let automation = if automation_enabled() {
        label("doctor.on", "on")
    } else {
        label("doctor.off", "off")
    };
    let automation_word = label("doctor.check.ci_automation", "BHARATCODE_AUTOMATION");

    match detect(cwd) {
        None => {
            let none_msg = label(
                "doctor.check.ci_none",
                "no CI provider detected; add a workflow to run bharatcode in CI",
            );
            (
                Status::Ok,
                format!("{}: {} ({} {})", lbl, none_msg, automation_word, automation),
            )
        }
        Some(d) if d.has_bharatcode_step => {
            let with_step = label("doctor.check.ci_has_step", "bharatcode step present");
            (
                Status::Ok,
                format!(
                    "{}: {} detected, {} ({} {})",
                    lbl, d.provider, with_step, automation_word, automation
                ),
            )
        }
        Some(d) => {
            let no_step = label("doctor.check.ci_no_step", "no bharatcode step");
            (
                Status::Warn,
                format!(
                    "{}: {} detected, {} ({} {})",
                    lbl, d.provider, no_step, automation_word, automation
                ),
            )
        }
    }
}

/// A copy-paste GitHub Actions snippet that runs BharatCode non-interactively
/// from a recipe, with the headless `BHARATCODE_AUTOMATION` switch set.
///
/// This is reference text only — `ci_readiness` never writes it anywhere. It is
/// surfaced for operators who want a known-good starting point for a
/// `.github/workflows/bharatcode.yml`.
pub fn sample_workflow() -> &'static str {
    r#"# .github/workflows/bharatcode.yml
name: BharatCode
on: [push, pull_request]
jobs:
  bharatcode:
    runs-on: ubuntu-latest
    env:
      BHARATCODE_AUTOMATION: "1"
    steps:
      - uses: actions/checkout@v4
      - name: Run BharatCode recipe
        run: bharatcode run --recipe .bharatcode/ci.yml
"#
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(root: &Path, rel: &str, contents: &str) {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn github_workflow_with_bharatcode_step_is_ok() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write(
            root,
            ".github/workflows/x.yml",
            "jobs:\n  ci:\n    steps:\n      - run: bharatcode run --recipe ci.yml\n",
        );

        let (status, msg) = ci_readiness(root);
        assert_eq!(status, Status::Ok, "msg: {msg}");
        assert!(msg.contains("GitHub Actions"), "msg: {msg}");
    }

    #[test]
    fn github_workflow_without_bharatcode_step_warns() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write(
            root,
            ".github/workflows/x.yml",
            "jobs:\n  ci:\n    steps:\n      - run: cargo test\n",
        );

        let (status, msg) = ci_readiness(root);
        assert_eq!(status, Status::Warn, "msg: {msg}");
        assert!(msg.contains("GitHub Actions"), "msg: {msg}");
    }

    #[test]
    fn no_ci_files_is_neutral_non_error() {
        let dir = TempDir::new().unwrap();
        let (status, msg) = ci_readiness(dir.path());
        // No CI config: a benign, non-error state.
        assert_eq!(status, Status::Ok, "msg: {msg}");
        assert_ne!(status, Status::Fail, "msg: {msg}");
    }

    #[test]
    fn gitlab_ci_without_step_warns() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write(
            root,
            ".gitlab-ci.yml",
            "test:\n  script:\n    - cargo build\n",
        );

        let (status, msg) = ci_readiness(root);
        assert_eq!(status, Status::Warn, "msg: {msg}");
        assert!(msg.contains("GitLab CI"), "msg: {msg}");
    }

    #[test]
    fn jenkinsfile_with_step_is_ok() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write(
            root,
            "Jenkinsfile",
            "pipeline { stages { stage('x') { steps { sh 'bharatcode run --recipe ci.yml' } } } }\n",
        );

        let (status, msg) = ci_readiness(root);
        assert_eq!(status, Status::Ok, "msg: {msg}");
        assert!(msg.contains("Jenkins"), "msg: {msg}");
    }

    #[test]
    fn github_actions_takes_priority_over_jenkins() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write(
            root,
            ".github/workflows/ci.yaml",
            "jobs:\n  ci:\n    steps:\n      - run: cargo test\n",
        );
        write(root, "Jenkinsfile", "sh 'bharatcode run'\n");

        let (_status, msg) = ci_readiness(root);
        // GitHub Actions is detected first and reported, not Jenkins.
        assert!(msg.contains("GitHub Actions"), "msg: {msg}");
        assert!(!msg.contains("Jenkins"), "msg: {msg}");
    }

    #[test]
    fn sample_workflow_mentions_automation_and_run_recipe() {
        let snippet = sample_workflow();
        assert!(
            snippet.contains("BHARATCODE_AUTOMATION"),
            "sample workflow must set the automation switch"
        );
        assert!(
            snippet.contains("bharatcode run --recipe"),
            "sample workflow must invoke bharatcode run --recipe"
        );
    }
}
