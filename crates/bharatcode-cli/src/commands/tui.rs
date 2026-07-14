use anyhow::{anyhow, bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

const TUI_SCRIPT_ENV: &str = "BHARATCODE_TUI_SCRIPT";

#[derive(Debug, PartialEq, Eq)]
enum TuiSource {
    CurrentBinary(PathBuf),
    Script(PathBuf),
}

fn select_source(script_override: Option<&Path>, current_exe: PathBuf) -> Result<TuiSource> {
    if let Some(script) = script_override {
        if !script.is_file() {
            bail!(
                "{TUI_SCRIPT_ENV} points at `{}`, which is not a file",
                script.display()
            );
        }
        return Ok(TuiSource::Script(script.to_path_buf()));
    }

    Ok(TuiSource::CurrentBinary(current_exe))
}

fn resolve_source() -> Result<TuiSource> {
    let current_exe =
        std::env::current_exe().context("could not determine the current bharatcode executable")?;
    let script = std::env::var_os(TUI_SCRIPT_ENV).map(PathBuf::from);
    select_source(script.as_deref(), current_exe)
}

fn program_on_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    let extensions: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
            .split(';')
            .filter(|ext| !ext.is_empty())
            .map(str::to_string)
            .collect()
    } else {
        vec![String::new()]
    };

    std::env::split_paths(&path).any(|dir| {
        extensions
            .iter()
            .any(|ext| dir.join(format!("{program}{ext}")).is_file())
    })
}

fn build_command(source: &TuiSource, args: &[String]) -> Result<Command> {
    match source {
        TuiSource::CurrentBinary(binary) => {
            let mut command = Command::new(binary);
            command.args(args);
            Ok(command)
        }
        TuiSource::Script(script) => {
            if !program_on_path("node") {
                bail!(
                    "cannot run TUI launcher `{}`: `node` is not on PATH",
                    script.display()
                );
            }
            let mut command = Command::new("node");
            command.arg(script).args(args);
            Ok(command)
        }
    }
}

pub fn handle_tui(args: Vec<String>) -> Result<()> {
    let source = resolve_source()?;
    let current_exe =
        std::env::current_exe().context("could not determine current bharatcode executable")?;
    let mut command = build_command(&source, &args)?;
    command
        .env("BHARATCODE_BINARY", current_exe)
        .env("BHARATCODE_TUI_ACTIVE", "1");

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let error = command.exec();
        Err(anyhow!("failed to start terminal UI: {error}"))
    }

    #[cfg(not(unix))]
    {
        let status = command.status().context("failed to start terminal UI")?;
        if !status.success() {
            std::process::exit(status.code().unwrap_or(1));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command_parts(command: &Command) -> (String, Vec<String>) {
        (
            command.get_program().to_string_lossy().into_owned(),
            command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
        )
    }

    #[test]
    fn defaults_to_the_current_binary_without_network_resolution() {
        let source = select_source(None, PathBuf::from("/opt/bin/bharatcode")).unwrap();
        assert_eq!(
            source,
            TuiSource::CurrentBinary(PathBuf::from("/opt/bin/bharatcode"))
        );
    }

    #[test]
    fn explicit_script_override_must_exist() {
        let error = select_source(
            Some(Path::new("/definitely/missing/tui.js")),
            PathBuf::from("bharatcode"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains(TUI_SCRIPT_ENV));
    }

    #[test]
    fn current_binary_receives_all_forwarded_arguments() {
        let source = TuiSource::CurrentBinary(PathBuf::from("/opt/bin/bharatcode"));
        let command = build_command(&source, &["--help".to_string()]).unwrap();
        let (program, args) = command_parts(&command);
        assert_eq!(program, "/opt/bin/bharatcode");
        assert_eq!(args, ["--help"]);
    }

    #[test]
    fn existing_script_override_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("tui.js");
        std::fs::write(&script, "// launcher").unwrap();
        assert_eq!(
            select_source(Some(&script), PathBuf::from("bharatcode")).unwrap(),
            TuiSource::Script(script)
        );
    }
}
