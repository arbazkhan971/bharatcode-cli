//! Opt-in, allowlisted automation-script runner for the developer extension.
//!
//! When the `BHARATCODE_SCRIPTS` environment variable is set to a truthy value,
//! the developer extension exposes a `run_script` tool that runs a named,
//! pre-registered automation script from the project's `.bharatcode/scripts/`
//! directory. This gives recipes and agents a small, predictable scripting and
//! automation surface without handing over an open shell: the model may only
//! invoke scripts that a human has already placed (and marked executable) in
//! that directory, and only by basename.
//!
//! The runner is intentionally conservative:
//!
//! * It is opt-in. When `BHARATCODE_SCRIPTS` is unset (the default) the tool is
//!   absent from the tool list and never registered, so default behaviour is
//!   unchanged.
//! * The script `name` is an allowlist key, not a path: any path separator or
//!   `..` component is rejected so the model cannot escape the scripts
//!   directory.
//! * The target must exist *and* be marked executable; otherwise an error is
//!   returned rather than attempting to interpret an arbitrary file.
//! * It reuses the same in-process exec sandbox as the shell tool, so when
//!   `BHARATCODE_SANDBOX` is set the script runs under landlock/seccomp
//!   restrictions (the v4 sandbox) just like any other command.
//! * Captured stdout/stderr are byte-capped before being returned to the model.
//!
//! Original BharatCode work; not ported from any third party.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use rmcp::model::{CallToolResult, Content, JsonObject, Tool, ToolAnnotations};
use schemars::{schema_for, JsonSchema};
use serde::Deserialize;

#[cfg(not(windows))]
use crate::subprocess::SandboxExt;
use crate::subprocess::SubprocessExt;

/// Name of the environment variable that opts in to the `run_script` tool.
pub const SCRIPTS_ENV: &str = "BHARATCODE_SCRIPTS";

/// Process-wide lock serializing every test that mutates `BHARATCODE_SCRIPTS`.
/// The env var is process-global, so the `run_script` tests here and the
/// `developer::tests` tool-list tests in the parent module must share one gate
/// or they race (one removes the var while another expects it set).
#[cfg(test)]
pub(crate) static SCRIPTS_ENV_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Project-relative directory that holds allowlisted automation scripts.
const SCRIPTS_DIR: &str = ".bharatcode/scripts";

/// Maximum number of bytes of each captured stream returned to the model.
const OUTPUT_LIMIT_BYTES: usize = 50_000;

/// Returns true when the `run_script` tool is enabled via `BHARATCODE_SCRIPTS`.
///
/// This reads the process environment directly (raw-env-first) rather than the
/// layered config so the gate honours a freshly-exported variable without a
/// config reload. Accepted truthy values (case-insensitive): `1`, `true`,
/// `yes`, `on`. Anything else (including unset) leaves the tool disabled and
/// absent from the developer tool list.
pub fn is_enabled() -> bool {
    matches!(
        std::env::var(SCRIPTS_ENV)
            .ok()
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("1") | Some("true") | Some("yes") | Some("on")
    )
}

/// Arguments accepted by the `run_script` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RunScriptParams {
    /// Basename of the allowlisted script to run, resolved under
    /// `.bharatcode/scripts/` in the working directory. Must not contain a path
    /// separator or `..`.
    pub name: String,
    /// Arguments forwarded to the script, in order. Defaults to none.
    #[serde(default)]
    pub args: Vec<String>,
}

/// Build the `run_script` [`Tool`] definition.
///
/// The description deliberately avoids naming any upstream project; it only
/// describes the local `.bharatcode/scripts/` allowlist contract.
pub fn script_tool() -> Tool {
    let schema: JsonObject = serde_json::to_value(schema_for!(RunScriptParams))
        .expect("schema serialization should succeed")
        .as_object()
        .expect("schema should serialize to an object")
        .clone();

    Tool::new(
        "run_script".to_string(),
        "Run a named, pre-registered automation script from the project's \
         `.bharatcode/scripts/` directory and return its stdout and stderr. \
         `name` is an allowlist key (a plain basename, no path separators or \
         `..`); the target must already exist and be marked executable. Any \
         `args` are forwarded to the script in order. Honors the exec sandbox \
         when it is enabled."
            .to_string(),
        schema,
    )
    .annotate(ToolAnnotations::from_raw(
        Some("Run Script".to_string()),
        Some(false),
        Some(true),
        Some(false),
        Some(true),
    ))
}

/// Reject a script `name` that is anything other than a single safe basename.
///
/// A valid name is non-empty, contains no path separator, is not a `.`/`..`
/// component, and does not embed a `..` traversal. This keeps `name` an
/// allowlist key that can only resolve to a direct child of the scripts
/// directory.
fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Script name must not be empty".to_string());
    }
    if name.contains('/') || name.contains('\\') {
        return Err(format!(
            "Invalid script name '{name}': path separators are not allowed"
        ));
    }
    if name == "." || name == ".." || name.contains("..") {
        return Err(format!(
            "Invalid script name '{name}': '..' traversal is not allowed"
        ));
    }
    Ok(())
}

/// True when `path` exists and has at least one executable permission bit.
///
/// On non-Unix platforms, existence as a file is treated as sufficient since
/// there is no executable mode bit to consult.
fn is_executable(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => {
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
        _ => false,
    }
}

/// Truncate `bytes` of captured output to the byte cap, appending a marker when
/// truncation occurred, and return it as lossy UTF-8.
fn cap_output(bytes: &[u8]) -> String {
    if bytes.len() <= OUTPUT_LIMIT_BYTES {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let mut text = String::from_utf8_lossy(&bytes[..OUTPUT_LIMIT_BYTES]).into_owned();
    text.push_str("\n[output truncated]");
    text
}

fn error_result(message: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![
        Content::text(format!("Error: {}", message.into())).with_priority(0.0)
    ])
}

/// Resolve, validate, and run an allowlisted automation script.
///
/// Resolves `<working_dir>/.bharatcode/scripts/<name>`, rejecting any `name`
/// that is not a safe basename. The target must exist and be executable. The
/// script is executed directly (not via a shell), with `args` forwarded in
/// order; when `BHARATCODE_SANDBOX` is set the same in-process exec sandbox as
/// the shell tool is applied. Captured stdout/stderr are byte-capped and
/// returned to the model.
pub async fn run(params: RunScriptParams, working_dir: Option<&Path>) -> CallToolResult {
    if let Err(message) = validate_name(&params.name) {
        return error_result(message);
    }

    let base: PathBuf = match working_dir {
        Some(dir) => dir.to_path_buf(),
        None => match std::env::current_dir() {
            Ok(dir) => dir,
            Err(error) => {
                return error_result(format!("Could not resolve working directory: {error}"))
            }
        },
    };

    let script_path = base.join(SCRIPTS_DIR).join(&params.name);

    if !script_path.exists() {
        return error_result(format!(
            "Script '{}' not found under {SCRIPTS_DIR}",
            params.name
        ));
    }
    if !is_executable(&script_path) {
        return error_result(format!(
            "Script '{}' is not executable; mark it executable to allow it",
            params.name
        ));
    }

    let mut command = tokio::process::Command::new(&script_path);
    command.args(&params.args);
    command.current_dir(&base);
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    #[cfg(not(windows))]
    apply_sandbox_if_enabled(&mut command, &base);

    command.set_no_window();

    let output = match command.output().await {
        Ok(output) => output,
        Err(error) => {
            return error_result(format!("Failed to run script '{}': {error}", params.name))
        }
    };

    let stdout = cap_output(&output.stdout);
    let stderr = cap_output(&output.stderr);
    let exit_code = output.status.code();

    let mut rendered = String::new();
    if !stdout.is_empty() {
        rendered.push_str(&stdout);
    }
    if !stderr.is_empty() {
        if !rendered.is_empty() {
            rendered.push('\n');
        }
        rendered.push_str("[stderr]\n");
        rendered.push_str(&stderr);
    }
    if rendered.is_empty() {
        rendered = format!("[script '{}' produced no output]", params.name);
    }

    let is_error = exit_code != Some(0);
    let content = vec![Content::text(rendered).with_priority(0.0)];
    if is_error {
        CallToolResult::error(content)
    } else {
        CallToolResult::success(content)
    }
}

/// Apply the in-process exec sandbox when `BHARATCODE_SANDBOX` requests it,
/// mirroring the developer shell tool's policy mapping.
#[cfg(not(windows))]
fn apply_sandbox_if_enabled(command: &mut tokio::process::Command, working_dir: &Path) {
    let (writable_roots, allow_network) = match std::env::var("BHARATCODE_SANDBOX")
        .ok()
        .as_deref()
        .map(str::trim)
    {
        Some("read-only") | Some("readonly") | Some("read_only") => (Vec::new(), false),
        Some("workspace-write") | Some("workspace_write") => {
            (vec![working_dir.to_path_buf()], true)
        }
        _ => return,
    };

    let policy = bharatcode_linux_sandbox::SandboxPolicy {
        writable_roots,
        allow_network,
    };
    command.apply_sandbox(&policy);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first_text(result: &CallToolResult) -> String {
        match &result.content[0].raw {
            rmcp::model::RawContent::Text(text) => text.text.clone(),
            _ => panic!("expected text content"),
        }
    }

    #[test]
    fn is_enabled_false_when_unset() {
        let _lock = SCRIPTS_ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::remove_var(SCRIPTS_ENV);
        assert!(!is_enabled());
    }

    #[test]
    fn is_enabled_reflects_env() {
        let _lock = SCRIPTS_ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::set_var(SCRIPTS_ENV, "1");
        assert!(is_enabled());
        std::env::set_var(SCRIPTS_ENV, "false");
        assert!(!is_enabled());
        std::env::remove_var(SCRIPTS_ENV);
    }

    #[test]
    fn name_validation_rejects_traversal() {
        assert!(validate_name("../etc/passwd").is_err());
        assert!(validate_name("a/b").is_err());
        assert!(validate_name("..").is_err());
        assert!(validate_name("").is_err());
    }

    #[test]
    fn name_validation_accepts_plain_basename() {
        assert!(validate_name("hello").is_ok());
        assert!(validate_name("build-site.sh").is_ok());
    }

    #[test]
    fn tool_description_has_no_upstream_branding() {
        let tool = script_tool();
        let description = tool.description.unwrap_or_default().to_lowercase();
        assert!(!description.contains("goose"));
        assert!(!description.contains("block"));
        assert_eq!(tool.name, "run_script");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_executes_allowlisted_script_and_returns_stdout() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let scripts_dir = temp.path().join(SCRIPTS_DIR);
        std::fs::create_dir_all(&scripts_dir).unwrap();
        let script = scripts_dir.join("hello");
        std::fs::write(&script, "#!/bin/sh\necho hello-from-script\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        // See run_forwards_args: retry to absorb a transient ETXTBSY from the
        // write+chmod+exec sequence under parallel load.
        let mut result = run(
            RunScriptParams {
                name: "hello".to_string(),
                args: vec![],
            },
            Some(temp.path()),
        )
        .await;
        for _ in 0..5 {
            if result.is_error != Some(true) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            result = run(
                RunScriptParams {
                    name: "hello".to_string(),
                    args: vec![],
                },
                Some(temp.path()),
            )
            .await;
        }

        assert_eq!(result.is_error, Some(false));
        assert!(first_text(&result).contains("hello-from-script"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_forwards_args() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let scripts_dir = temp.path().join(SCRIPTS_DIR);
        std::fs::create_dir_all(&scripts_dir).unwrap();
        let script = scripts_dir.join("echo-args");
        std::fs::write(&script, "#!/bin/sh\necho \"$1 $2\"\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        // Writing + chmod + exec in quick succession can briefly yield ETXTBSY
        // ("Text file busy") under heavy parallel test load; retry to keep the
        // assertion deterministic.
        let mut result = run(
            RunScriptParams {
                name: "echo-args".to_string(),
                args: vec!["alpha".to_string(), "beta".to_string()],
            },
            Some(temp.path()),
        )
        .await;
        for _ in 0..5 {
            if result.is_error != Some(true) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            result = run(
                RunScriptParams {
                    name: "echo-args".to_string(),
                    args: vec!["alpha".to_string(), "beta".to_string()],
                },
                Some(temp.path()),
            )
            .await;
        }

        assert_eq!(result.is_error, Some(false));
        assert!(first_text(&result).contains("alpha beta"));
    }

    #[tokio::test]
    async fn run_missing_script_is_error() {
        let temp = tempfile::tempdir().unwrap();
        let result = run(
            RunScriptParams {
                name: "does-not-exist".to_string(),
                args: vec![],
            },
            Some(temp.path()),
        )
        .await;

        assert_eq!(result.is_error, Some(true));
        assert!(first_text(&result).to_lowercase().contains("not found"));
    }

    #[tokio::test]
    async fn run_rejects_traversal_name() {
        let temp = tempfile::tempdir().unwrap();
        let result = run(
            RunScriptParams {
                name: "../etc/passwd".to_string(),
                args: vec![],
            },
            Some(temp.path()),
        )
        .await;

        assert_eq!(result.is_error, Some(true));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_non_executable_script_is_error() {
        let temp = tempfile::tempdir().unwrap();
        let scripts_dir = temp.path().join(SCRIPTS_DIR);
        std::fs::create_dir_all(&scripts_dir).unwrap();
        let script = scripts_dir.join("plain");
        std::fs::write(&script, "#!/bin/sh\necho nope\n").unwrap();

        let result = run(
            RunScriptParams {
                name: "plain".to_string(),
                args: vec![],
            },
            Some(temp.path()),
        )
        .await;

        assert_eq!(result.is_error, Some(true));
        assert!(first_text(&result)
            .to_lowercase()
            .contains("not executable"));
    }
}
