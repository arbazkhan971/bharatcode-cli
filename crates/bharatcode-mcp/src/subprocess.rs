use std::sync::OnceLock;
use tokio::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW_FLAG: u32 = 0x08000000;

pub trait SubprocessExt {
    fn set_no_window(&mut self) -> &mut Self;
}

impl SubprocessExt for Command {
    fn set_no_window(&mut self) -> &mut Self {
        #[cfg(windows)]
        {
            self.creation_flags(CREATE_NO_WINDOW_FLAG);
        }
        self
    }
}

impl SubprocessExt for std::process::Command {
    fn set_no_window(&mut self) -> &mut Self {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            self.creation_flags(CREATE_NO_WINDOW_FLAG);
        }
        self
    }
}

/// Resolve the user's full PATH by running a login shell.
///
/// When goosed is launched from a desktop app (e.g. Electron), it may inherit
/// a minimal PATH like `/usr/bin:/bin`. This function spawns a login shell to
/// source the user's profile and recover the full PATH.
///
/// Shared with `crates/bharatcode-core/src/agents/platform_extensions/developer/shell.rs`.
/// where it was introduced in #5774 for the developer extension. This makes the
/// same fix available to all MCP extensions in goose-mcp.
#[cfg(not(windows))]
const LOGIN_SHELL_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

#[cfg(not(windows))]
fn resolve_login_shell_path() -> Option<String> {
    use std::path::PathBuf;

    // Prefer the user's configured shell so we source the right profile files.
    // Fall back to /bin/bash (common default) then sh as last resort.
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| PathBuf::from(s).is_file())
        .unwrap_or_else(|| {
            if PathBuf::from("/bin/bash").is_file() {
                "/bin/bash".to_string()
            } else {
                "sh".to_string()
            }
        });

    probe_login_shell_path(&shell, LOGIN_SHELL_PROBE_TIMEOUT)
}

/// Run `<shell> -l -i -c 'echo $PATH'` and return the resolved PATH.
///
/// A user profile can hang (a prompt hook waiting on a slow network mount, say).
/// This probe runs behind the `OnceLock` below, so a hang would wedge every
/// caller forever — hence the timeout, after which the shell is killed and
/// reaped and we fall back to the inherited PATH.
#[cfg(not(windows))]
fn probe_login_shell_path(shell: &str, timeout: std::time::Duration) -> Option<String> {
    use process_wrap::std::{CommandWrap, ProcessSession};
    use std::io::Read;
    use std::process::Stdio;

    let mut cmd = CommandWrap::from(std::process::Command::new(shell));
    cmd.command_mut()
        .args(["-l", "-i", "-c", "echo $PATH"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    // Spawn in a new session so that interactive shell job-control setup
    // cannot steal the terminal foreground from the parent goose process.
    cmd.wrap(ProcessSession);

    let mut child = cmd.spawn().ok()?;
    let mut stdout = child.stdout().take()?;

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if stdout.read_to_end(&mut buf).is_ok() {
            let _ = tx.send(buf);
        }
    });

    match rx.recv_timeout(timeout) {
        Ok(buf) if child.wait().is_ok_and(|status| status.success()) => {
            // Take the last non-empty line — interactive shells may emit
            // extra output from profile scripts before our echo.
            String::from_utf8_lossy(&buf)
                .lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .map(|line| line.trim().to_string())
                .filter(|path| !path.is_empty())
        }
        _ => {
            let _ = child.kill();
            let _ = child.wait();
            None
        }
    }
}

/// Returns the user's full login shell PATH, resolved once and cached.
///
/// Call this before spawning subprocesses to ensure they inherit the user's
/// full PATH rather than the restricted one from the desktop app launcher.
#[cfg(not(windows))]
pub fn user_login_path() -> Option<&'static str> {
    static CACHED: OnceLock<Option<String>> = OnceLock::new();
    CACHED.get_or_init(resolve_login_shell_path).as_deref()
}

/// Merge the login shell PATH with the current process PATH.
///
/// Prepends login shell entries so user tools are found first, while
/// preserving any runtime PATH additions (e.g. from direnv, nix, or
/// auto-install helpers like ensure_peekaboo).
#[cfg(not(windows))]
pub fn merged_path() -> Option<String> {
    let login = user_login_path()?;
    let current = std::env::var("PATH").unwrap_or_default();
    if current.is_empty() {
        return Some(login.to_string());
    }
    // Deduplicate: login shell entries first, then any current entries not already present.
    let login_entries: Vec<&str> = login.split(':').collect();
    let mut seen: std::collections::HashSet<&str> = login_entries.iter().copied().collect();
    let mut merged = login_entries;
    for entry in current.split(':') {
        if seen.insert(entry) {
            merged.push(entry);
        }
    }
    Some(merged.join(":"))
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    fn shell_script(dir: &std::path::Path, name: &str, body: &str) -> String {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.to_str().unwrap().to_string()
    }

    #[test]
    fn probe_takes_the_last_non_empty_line_of_shell_output() {
        let dir = tempfile::tempdir().unwrap();
        let shell = shell_script(
            dir.path(),
            "noisy-shell",
            "echo 'profile banner'\necho\necho /opt/bin:/usr/bin",
        );

        let resolved = probe_login_shell_path(&shell, Duration::from_secs(5));

        assert_eq!(resolved.as_deref(), Some("/opt/bin:/usr/bin"));
    }

    #[test]
    fn probe_gives_up_instead_of_blocking_on_a_hanging_shell() {
        let dir = tempfile::tempdir().unwrap();
        let shell = shell_script(dir.path(), "hanging-shell", "exec sleep 30");

        let start = Instant::now();
        let resolved = probe_login_shell_path(&shell, Duration::from_millis(200));

        assert!(resolved.is_none());
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "a hanging profile must not wedge the PATH probe"
        );
    }

    #[test]
    fn probe_returns_none_when_the_shell_fails() {
        let dir = tempfile::tempdir().unwrap();
        let shell = shell_script(dir.path(), "failing-shell", "exit 1");

        assert!(probe_login_shell_path(&shell, Duration::from_secs(5)).is_none());
    }
}
