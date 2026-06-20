use tokio::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW_FLAG: u32 = 0x08000000;

#[cfg(target_os = "linux")]
fn configure_parent_death_signal(command: &mut Command) {
    let parent_pid = unsafe { libc::getpid() };

    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                return Err(std::io::Error::last_os_error());
            }

            if libc::getppid() != parent_pid {
                return Err(std::io::Error::from_raw_os_error(libc::ESRCH));
            }

            Ok(())
        });
    }
}

pub trait SubprocessExt {
    fn set_no_window(&mut self) -> &mut Self;
}

/// Attaches an in-process Linux exec sandbox (landlock + seccomp) to a command
/// via a `pre_exec` hook, so only the spawned child inherits the restrictions.
///
/// On non-Linux targets (and when the policy is satisfiable) this is a no-op
/// that returns `self`, mirroring [`SubprocessExt::set_no_window`].
pub trait SandboxExt {
    fn apply_sandbox(&mut self, policy: &bharatcode_linux_sandbox::SandboxPolicy) -> &mut Self;
}

impl SandboxExt for Command {
    #[allow(unused_variables)]
    fn apply_sandbox(&mut self, policy: &bharatcode_linux_sandbox::SandboxPolicy) -> &mut Self {
        #[cfg(target_os = "linux")]
        {
            let policy = policy.clone();
            unsafe {
                self.pre_exec(move || {
                    bharatcode_linux_sandbox::apply_to_current_thread(&policy)
                        .map_err(std::io::Error::other)
                });
            }
        }
        self
    }
}

impl SandboxExt for std::process::Command {
    #[allow(unused_variables)]
    fn apply_sandbox(&mut self, policy: &bharatcode_linux_sandbox::SandboxPolicy) -> &mut Self {
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::process::CommandExt;
            let policy = policy.clone();
            unsafe {
                self.pre_exec(move || {
                    bharatcode_linux_sandbox::apply_to_current_thread(&policy)
                        .map_err(std::io::Error::other)
                });
            }
        }
        self
    }
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

#[allow(unused_variables)]
pub fn configure_subprocess(command: &mut Command) {
    // Isolate subprocess into its own process group so it does not receive
    // SIGINT when the user presses Ctrl+C in the terminal.
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(target_os = "linux")]
    configure_parent_death_signal(command);
    command.set_no_window();
}
