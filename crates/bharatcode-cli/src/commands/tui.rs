use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

const TUI_NPM_SPEC_ENV: &str = "BHARATCODE_TUI_NPM_SPEC";
const TUI_SCRIPT_ENV: &str = "BHARATCODE_TUI_SCRIPT";
const TUI_REL_PATH: &str = "ui/text/dist/tui.js";
const DEFAULT_NPM_SPEC: &str = "@aaif/bharatcode@latest";
const NPM_BIN_NAME: &str = "bharatcode-tui";

enum TuiSource {
    LocalScript(PathBuf),
    Npx(String),
}

fn find_env_script() -> Option<PathBuf> {
    let script = std::env::var_os(TUI_SCRIPT_ENV)?;
    let path = PathBuf::from(script);
    path.is_file().then_some(path)
}

fn find_local_script() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent().unwrap_or_else(|| Path::new("."));

    let mut dir = Some(exe_dir.to_path_buf());
    for _ in 0..6 {
        if let Some(d) = dir.clone() {
            let candidate = d.join(TUI_REL_PATH);
            if candidate.is_file() {
                return Some(candidate);
            }
            dir = d.parent().map(Path::to_path_buf);
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        let candidate = cwd.join(TUI_REL_PATH);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

fn resolve_source() -> TuiSource {
    if let Some(script) = find_env_script() {
        return TuiSource::LocalScript(script);
    }
    if let Some(script) = find_local_script() {
        return TuiSource::LocalScript(script);
    }
    let spec = std::env::var(TUI_NPM_SPEC_ENV).unwrap_or_else(|_| DEFAULT_NPM_SPEC.to_string());
    TuiSource::Npx(spec)
}

fn build_command(source: &TuiSource, args: &[String]) -> Result<Command> {
    match source {
        TuiSource::LocalScript(script) => {
            let mut cmd = Command::new("node");
            cmd.arg(script).args(args);
            Ok(cmd)
        }
        TuiSource::Npx(spec) => {
            let mut cmd = Command::new("npx");
            cmd.arg("--yes")
                .arg("--package")
                .arg(spec)
                .arg("--")
                .arg(NPM_BIN_NAME)
                .args(args);
            Ok(cmd)
        }
    }
}

pub fn handle_tui(args: Vec<String>) -> Result<()> {
    let source = resolve_source();

    let goose_binary = std::env::current_exe().context(
        "could not determine current bharatcode executable to expose as BHARATCODE_BINARY",
    )?;

    let mut cmd = build_command(&source, &args)?;
    cmd.env("BHARATCODE_BINARY", &goose_binary);

    let descriptor = match &source {
        TuiSource::LocalScript(p) => format!("node {}", p.display()),
        TuiSource::Npx(spec) => format!("npx --package {} -- {}", spec, NPM_BIN_NAME),
    };

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        Err(anyhow!("failed to exec TUI ({descriptor}): {err}"))
    }

    #[cfg(not(unix))]
    {
        let status = cmd
            .status()
            .with_context(|| format!("failed to run `{descriptor}`"))?;
        if !status.success() {
            std::process::exit(status.code().unwrap_or(1));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_guard<'a>(script: Option<&'a str>, npm_spec: Option<&'a str>) -> env_lock::EnvGuard<'a> {
        env_lock::lock_env([(TUI_SCRIPT_ENV, script), (TUI_NPM_SPEC_ENV, npm_spec)])
    }

    #[test]
    fn resolve_source_prefers_env_script_when_it_exists() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("tui.js");
        std::fs::write(&script, "console.log('local tui');").unwrap();
        let script_str = script.to_str().unwrap();
        let _guard = env_guard(Some(script_str), Some("custom-package@1"));

        match resolve_source() {
            TuiSource::LocalScript(path) => assert_eq!(path, script),
            TuiSource::Npx(spec) => panic!("expected local script, got npx {spec}"),
        }
    }

    #[test]
    fn resolve_source_ignores_missing_env_script_and_uses_npm_spec() {
        let _guard = env_guard(Some("/definitely/missing/tui.js"), Some("custom-package@2"));

        match resolve_source() {
            TuiSource::Npx(spec) => assert_eq!(spec, "custom-package@2"),
            TuiSource::LocalScript(path) => panic!("expected npx fallback, got {}", path.display()),
        }
    }

    #[test]
    fn build_command_for_local_script_uses_node_and_forwards_args() {
        let source = TuiSource::LocalScript(PathBuf::from("/tmp/tui.js"));
        let args = vec!["--theme".to_string(), "dark".to_string()];

        let cmd = build_command(&source, &args).unwrap();
        let got_args: Vec<_> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();

        assert_eq!(cmd.get_program().to_string_lossy(), "node");
        assert_eq!(got_args, vec!["/tmp/tui.js", "--theme", "dark"]);
    }

    #[test]
    fn build_command_for_npx_uses_package_spec_and_bin_name() {
        let source = TuiSource::Npx("pkg@1".to_string());
        let args = vec!["--theme".to_string(), "dark".to_string()];

        let cmd = build_command(&source, &args).unwrap();
        let got_args: Vec<_> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();

        assert_eq!(cmd.get_program().to_string_lossy(), "npx");
        assert_eq!(
            got_args,
            vec![
                "--yes",
                "--package",
                "pkg@1",
                "--",
                NPM_BIN_NAME,
                "--theme",
                "dark"
            ]
        );
    }
}
