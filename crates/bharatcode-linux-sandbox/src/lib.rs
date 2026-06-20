// Derived from OpenAI Codex `codex-linux-sandbox` (https://github.com/openai/codex),
// Apache-2.0, Copyright 2025 OpenAI. See LICENSES/LICENSE-codex and NOTICE.
//
// This crate vendors the PURE, in-process Linux sandbox primitives from Codex's
// `linux-sandbox/src/landlock.rs` (set_no_new_privs, the Landlock filesystem
// ruleset, and the Restricted-mode network seccomp filter). The Codex
// dependencies on `codex-protocol` (PermissionProfile / NetworkSandboxPolicy /
// CodexErr / SandboxErr) and `codex-utils-absolute-path::AbsolutePathBuf` are
// replaced with plain local types (`SandboxPolicy` over `std::path::PathBuf`
// plus a `thiserror` enum). The bubblewrap launcher, proxy-routed network mode,
// and seatbelt/windows backends are intentionally dropped to stay build-safe and
// LGPL-free. The unsupported-architecture `unimplemented!()` panic is replaced
// with a returned `SandboxError::UnsupportedArch`, and a no-op stub is provided
// on non-Linux targets so callers can compile and link unconditionally.

//! In-process Linux exec sandbox primitives behind a simple policy struct.
//!
//! [`apply_to_current_thread`] applies the policy to the *current thread* only
//! (so a forked child inherits it without affecting the parent process). It is
//! meant to be invoked from a `pre_exec` closure after `fork`/before `exec`.

use std::path::PathBuf;

/// A minimal, backend-agnostic description of what an exec sandbox should allow.
#[derive(Debug, Clone, Default)]
pub struct SandboxPolicy {
    /// Filesystem roots the sandboxed process may write to. Read access to the
    /// whole filesystem is always granted; writes are restricted to these roots
    /// (plus `/dev/null`).
    pub writable_roots: Vec<PathBuf>,
    /// When `false`, a network seccomp filter is installed denying outbound
    /// network syscalls (AF_UNIX sockets remain available for local IPC).
    pub allow_network: bool,
}

/// Errors that can occur while installing sandbox restrictions.
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    /// Landlock accepted the ruleset but the kernel could not enforce it
    /// (Landlock unavailable or disabled). Treated as fail-closed.
    #[error("Landlock was not able to fully enforce all sandbox rules")]
    LandlockNotEnforced,

    /// A Landlock ruleset operation failed.
    #[error("landlock error: {0}")]
    Landlock(String),

    /// Building or applying the seccomp filter failed.
    #[error("seccomp filter error: {0}")]
    Seccomp(String),

    /// The seccomp network filter only supports x86_64 / aarch64.
    #[error("seccomp network sandbox is not supported on this CPU architecture")]
    UnsupportedArch,

    /// An underlying OS error (e.g. `prctl`).
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[cfg(target_os = "linux")]
impl From<landlock::RulesetError> for SandboxError {
    fn from(err: landlock::RulesetError) -> Self {
        SandboxError::Landlock(err.to_string())
    }
}

#[cfg(target_os = "linux")]
impl From<seccompiler::Error> for SandboxError {
    fn from(err: seccompiler::Error) -> Self {
        SandboxError::Seccomp(err.to_string())
    }
}

#[cfg(target_os = "linux")]
impl From<seccompiler::BackendError> for SandboxError {
    fn from(err: seccompiler::BackendError) -> Self {
        SandboxError::Seccomp(err.to_string())
    }
}

/// Whether the network seccomp filter should be installed for this policy.
///
/// Kept as a pure, platform-independent helper so it can be unit-tested without
/// touching landlock/seccomp.
pub fn should_restrict_network(policy: &SandboxPolicy) -> bool {
    !policy.allow_network
}

/// Apply the sandbox policy to the current thread.
///
/// On Linux this:
/// - enables `PR_SET_NO_NEW_PRIVS` when a seccomp filter will be installed,
/// - installs a Restricted-mode network seccomp filter when network access is
///   denied, and
/// - installs Landlock filesystem rules granting read access to `/` and write
///   access only to `policy.writable_roots` (plus `/dev/null`).
#[cfg(target_os = "linux")]
pub fn apply_to_current_thread(policy: &SandboxPolicy) -> Result<(), SandboxError> {
    linux::apply_to_current_thread(policy)
}

/// No-op on non-Linux targets so callers can link unconditionally.
#[cfg(not(target_os = "linux"))]
pub fn apply_to_current_thread(_policy: &SandboxPolicy) -> Result<(), SandboxError> {
    Ok(())
}

#[cfg(target_os = "linux")]
mod linux {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use landlock::Access;
    use landlock::AccessFs;
    use landlock::CompatLevel;
    use landlock::Compatible;
    use landlock::Ruleset;
    use landlock::RulesetAttr;
    use landlock::RulesetCreatedAttr;
    use landlock::ABI;
    use seccompiler::apply_filter;
    use seccompiler::BpfProgram;
    use seccompiler::SeccompAction;
    use seccompiler::SeccompCmpArgLen;
    use seccompiler::SeccompCmpOp;
    use seccompiler::SeccompCondition;
    use seccompiler::SeccompFilter;
    use seccompiler::SeccompRule;
    use seccompiler::TargetArch;

    use super::should_restrict_network;
    use super::SandboxError;
    use super::SandboxPolicy;

    pub(super) fn apply_to_current_thread(policy: &SandboxPolicy) -> Result<(), SandboxError> {
        let restrict_network = should_restrict_network(policy);

        // `PR_SET_NO_NEW_PRIVS` is required before installing a seccomp filter.
        // Landlock sets its own `no_new_privs`, so we only need to set it here
        // for the seccomp path.
        if restrict_network {
            set_no_new_privs()?;
            install_network_seccomp_filter_on_current_thread()?;
        }

        install_filesystem_landlock_rules_on_current_thread(&policy.writable_roots)?;

        Ok(())
    }

    /// Enable `PR_SET_NO_NEW_PRIVS` so seccomp can be applied safely.
    fn set_no_new_privs() -> Result<(), SandboxError> {
        let result = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
        if result != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }

    /// Installs Landlock file-system rules on the current thread allowing read
    /// access to the entire file-system while restricting write access to
    /// `/dev/null` and the provided list of `writable_roots`.
    #[allow(deprecated)] // `set_no_new_privs(bool)` retained for parity across landlock 0.4.x
    fn install_filesystem_landlock_rules_on_current_thread(
        writable_roots: &[PathBuf],
    ) -> Result<(), SandboxError> {
        let abi = ABI::V5;
        let access_rw = AccessFs::from_all(abi);
        let access_ro = AccessFs::from_read(abi);

        let mut ruleset = Ruleset::default()
            .set_compatibility(CompatLevel::BestEffort)
            .handle_access(access_rw)?
            .create()?
            .add_rules(landlock::path_beneath_rules(["/"], access_ro))?
            .add_rules(landlock::path_beneath_rules(["/dev/null"], access_rw))?
            .set_no_new_privs(true);

        if !writable_roots.is_empty() {
            ruleset = ruleset.add_rules(landlock::path_beneath_rules(writable_roots, access_rw))?;
        }

        let status = ruleset.restrict_self()?;

        if status.ruleset == landlock::RulesetStatus::NotEnforced {
            return Err(SandboxError::LandlockNotEnforced);
        }

        Ok(())
    }

    /// Installs a Restricted-mode seccomp filter denying outbound network
    /// syscalls (plus ptrace / process_vm_* / io_uring), applied to the current
    /// thread only. AF_UNIX sockets remain allowed for local IPC.
    fn install_network_seccomp_filter_on_current_thread() -> Result<(), SandboxError> {
        fn deny_syscall(rules: &mut BTreeMap<i64, Vec<SeccompRule>>, nr: i64) {
            rules.insert(nr, vec![]); // empty rule vec = unconditional match
        }

        let mut rules: BTreeMap<i64, Vec<SeccompRule>> = BTreeMap::new();

        deny_syscall(&mut rules, libc::SYS_ptrace);
        deny_syscall(&mut rules, libc::SYS_process_vm_readv);
        deny_syscall(&mut rules, libc::SYS_process_vm_writev);
        deny_syscall(&mut rules, libc::SYS_io_uring_setup);
        deny_syscall(&mut rules, libc::SYS_io_uring_enter);
        deny_syscall(&mut rules, libc::SYS_io_uring_register);

        deny_syscall(&mut rules, libc::SYS_connect);
        deny_syscall(&mut rules, libc::SYS_accept);
        deny_syscall(&mut rules, libc::SYS_accept4);
        deny_syscall(&mut rules, libc::SYS_bind);
        deny_syscall(&mut rules, libc::SYS_listen);
        deny_syscall(&mut rules, libc::SYS_getpeername);
        deny_syscall(&mut rules, libc::SYS_getsockname);
        deny_syscall(&mut rules, libc::SYS_shutdown);
        deny_syscall(&mut rules, libc::SYS_sendto);
        deny_syscall(&mut rules, libc::SYS_sendmmsg);
        // NOTE: allowing recvfrom lets tools like `cargo clippy` run with their
        // socketpair + child processes for sub-process management.
        deny_syscall(&mut rules, libc::SYS_recvmmsg);
        deny_syscall(&mut rules, libc::SYS_getsockopt);
        deny_syscall(&mut rules, libc::SYS_setsockopt);

        // For `socket`/`socketpair` we allow AF_UNIX (arg0 == AF_UNIX) and deny
        // every other address family.
        let unix_only_rule = SeccompRule::new(vec![SeccompCondition::new(
            0, // first argument (domain)
            SeccompCmpArgLen::Dword,
            SeccompCmpOp::Ne,
            libc::AF_UNIX as u64,
        )?])?;

        rules.insert(libc::SYS_socket, vec![unix_only_rule.clone()]);
        rules.insert(libc::SYS_socketpair, vec![unix_only_rule]);

        let target_arch = if cfg!(target_arch = "x86_64") {
            TargetArch::x86_64
        } else if cfg!(target_arch = "aarch64") {
            TargetArch::aarch64
        } else {
            return Err(SandboxError::UnsupportedArch);
        };

        let filter = SeccompFilter::new(
            rules,
            SeccompAction::Allow,                     // default – allow
            SeccompAction::Errno(libc::EPERM as u32), // when rule matches – EPERM
            target_arch,
        )?;

        let prog: BpfProgram = filter.try_into()?;

        apply_filter(&prog)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_allowed_policy_does_not_restrict_network() {
        let policy = SandboxPolicy {
            writable_roots: vec![PathBuf::from("/tmp/work")],
            allow_network: true,
        };
        assert!(!should_restrict_network(&policy));
    }

    #[test]
    fn network_denied_policy_restricts_network() {
        let policy = SandboxPolicy {
            writable_roots: vec![PathBuf::from("/tmp/work")],
            allow_network: false,
        };
        assert!(should_restrict_network(&policy));
    }

    #[test]
    fn default_policy_has_no_writable_roots_and_restricts_network() {
        let policy = SandboxPolicy::default();
        assert!(policy.writable_roots.is_empty());
        assert!(should_restrict_network(&policy));
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn apply_is_noop_on_non_linux() {
        let policy = SandboxPolicy::default();
        assert!(apply_to_current_thread(&policy).is_ok());
    }
}
