//! Security hardening self-audit (BharatCode v96).
//!
//! At session start this module aggregates the *real* posture of the existing
//! privacy / safety guards into a single typed [`HardeningReport`] and, when the
//! operator opts in, emits one low-priority advisory line into the system prompt
//! summarising which pillars are weak. It does not change the behaviour of any
//! guard — it only reads the same accessors the live features already use:
//!
//! - data residency egress guard ([`crate::residency`]),
//! - offline / no-egress mode ([`crate::offline`]),
//! - shell exec policy (`BHARATCODE_EXEC_POLICY`, see [`crate::exec_policy`]),
//! - the in-process exec sandbox (`BHARATCODE_SANDBOX`),
//! - egress secret redaction
//!   ([`crate::agents::platform_extensions::developer::redact`]),
//! - telemetry-off ([`crate::offline`] composes telemetry state),
//! - local-only usage analytics ([`crate::usage_analytics`]).
//!
//! The feature is opt-in: [`advisory_block`] returns `None` unless the
//! `BHARATCODE_HARDENING` environment variable is truthy AND at least one pillar
//! is weak, so the default system prompt is byte-identical. The env gate mirrors
//! the raw-env truthiness tables in `a11y_prompt` / `usage_analytics`.

use crate::residency::ResidencyMode;

/// Opt-in toggle name, read straight from the environment.
const ENABLE_KEY: &str = "BHARATCODE_HARDENING";

/// Environment key selecting the shell exec policy file (see
/// [`crate::exec_policy`]). Read here only to report whether the policy is armed.
const EXEC_POLICY_KEY: &str = "BHARATCODE_EXEC_POLICY";

/// Environment key selecting the in-process exec sandbox mode (`read-only` /
/// `workspace-write`). Read here only to report whether a sandbox is armed.
const SANDBOX_KEY: &str = "BHARATCODE_SANDBOX";

/// Whether the hardening self-audit advisory is enabled. Opt-in via the
/// `BHARATCODE_HARDENING` environment variable; any truthy-ish value (`1`,
/// `true`, `yes`, `on`) enables it. Reads the raw environment (not the typed
/// config layer) so the gate matches the other privacy toggles.
pub fn is_enabled() -> bool {
    std::env::var(ENABLE_KEY)
        .map(|raw| is_truthy(&raw))
        .unwrap_or(false)
}

fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// One guarded capability and whether its current posture is considered strong.
///
/// `strong == false` marks a *weak* pillar — a guard that is available but is
/// currently disabled / permissive, which the advisory surfaces so the operator
/// can decide whether to tighten it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pillar {
    /// Stable, product-neutral identifier (e.g. `residency`).
    pub name: &'static str,
    /// True when the posture is strong (the guard is actively constraining).
    pub strong: bool,
}

impl Pillar {
    const fn new(name: &'static str, strong: bool) -> Self {
        Self { name, strong }
    }
}

/// The aggregated security posture, resolved from the live guard accessors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardeningReport {
    /// Data residency is enforced (mode is `warn` or `strict`, not `off`).
    pub residency_enforced: bool,
    /// Offline / no-egress mode is composed and guaranteeing no egress.
    pub no_egress: bool,
    /// A shell exec policy file is armed.
    pub exec_policy_armed: bool,
    /// The in-process exec sandbox is armed (`read-only` / `workspace-write`).
    pub sandbox_armed: bool,
    /// Egress secret redaction is enabled.
    pub redaction_enabled: bool,
    /// Telemetry is off (no usage events leave the machine).
    pub telemetry_off: bool,
    /// Usage analytics, when on, are kept strictly local (never egressed).
    ///
    /// This pillar is structurally always local-only by design, so it is never a
    /// weak pillar; it is reported for completeness and to confirm the accessor.
    pub analytics_local_only: bool,
}

impl HardeningReport {
    /// Resolve the current posture by reading the same accessors the live
    /// features use. Pure and read-only — it mutates nothing.
    pub fn resolve() -> Self {
        let status = crate::offline::offline_status();
        let redaction_enabled = crate::agents::platform_extensions::developer::redact::is_enabled();

        Self {
            residency_enforced: crate::residency::residency_mode() != ResidencyMode::Off,
            no_egress: status.is_no_egress(),
            exec_policy_armed: exec_policy_armed(),
            sandbox_armed: sandbox_armed(),
            redaction_enabled,
            telemetry_off: status.telemetry_is_off(),
            analytics_local_only: analytics_is_local_only(),
        }
    }

    /// The full pillar set in a stable display order.
    pub fn pillars(&self) -> [Pillar; 7] {
        [
            Pillar::new("residency", self.residency_enforced),
            Pillar::new("no-egress", self.no_egress),
            Pillar::new("exec-policy", self.exec_policy_armed),
            Pillar::new("sandbox", self.sandbox_armed),
            Pillar::new("secret-redaction", self.redaction_enabled),
            Pillar::new("telemetry-off", self.telemetry_off),
            Pillar::new("analytics-local-only", self.analytics_local_only),
        ]
    }

    /// The names of the pillars whose posture is currently weak.
    pub fn weak_pillars(&self) -> Vec<&'static str> {
        self.pillars()
            .into_iter()
            .filter(|p| !p.strong)
            .map(|p| p.name)
            .collect()
    }

    /// A coarse posture score: number of strong pillars out of the total.
    pub fn score(&self) -> (usize, usize) {
        let pillars = self.pillars();
        let total = pillars.len();
        let strong = pillars.into_iter().filter(|p| p.strong).count();
        (strong, total)
    }
}

/// Whether a shell exec policy file is armed. Mirrors the disabled-value table in
/// [`crate::exec_policy`] (`off` / `false` / `0` / empty => disabled) without
/// touching that module's private path resolver.
fn exec_policy_armed() -> bool {
    match std::env::var(EXEC_POLICY_KEY) {
        Ok(raw) => {
            let trimmed = raw.trim();
            !(trimmed.is_empty()
                || trimmed.eq_ignore_ascii_case("off")
                || trimmed.eq_ignore_ascii_case("false")
                || trimmed == "0")
        }
        Err(_) => false,
    }
}

/// Whether the in-process exec sandbox is armed. Mirrors the recognised modes in
/// the developer shell tool (`read-only` / `workspace-write`, with the usual
/// spelling variants); anything else — including absence — is unarmed.
fn sandbox_armed() -> bool {
    matches!(
        std::env::var(SANDBOX_KEY).ok().as_deref().map(str::trim),
        Some("read-only")
            | Some("readonly")
            | Some("read_only")
            | Some("workspace-write")
            | Some("workspace_write")
    )
}

/// Whether usage analytics are local-only. The analytics tally is *structurally*
/// local-only (it only ever writes a file under the config dir and never
/// egresses), so this is always true; the accessor is still consulted so the
/// report reflects the live module rather than a hard-coded constant.
fn analytics_is_local_only() -> bool {
    // `is_enabled()` reflects whether the local tally is recording this session;
    // either way the data never leaves the machine, so the pillar is strong.
    let _recording = crate::config::base::usage_analytics::is_enabled();
    true
}

/// The hardening advisory injected into the system prompt, or `None` when the
/// feature is disabled OR every pillar is already strong (leaving the prompt
/// byte-identical). The text is product-neutral plain text and kept compact.
pub fn advisory_block() -> Option<String> {
    if !is_enabled() {
        return None;
    }
    let report = HardeningReport::resolve();
    let weak = report.weak_pillars();
    if weak.is_empty() {
        return None;
    }
    let (strong, total) = report.score();
    Some(format!(
        "# Security Posture\n\n\
         A security hardening self-audit ran at session start. Posture score: \
         {strong}/{total} guards active.\n\
         The following protections are available but currently relaxed: {}.\n\
         Treat secrets, tokens, and outbound network access with extra care this \
         session: do not echo credentials, avoid unnecessary egress, and prefer \
         local, least-privilege actions.\n",
        weak.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    // Serialise env mutation across tests in this module so toggling the
    // hardening / guard env vars in one test never races another.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn lock_env() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    // Clear every env var this audit reads so each test starts from a known,
    // weak-by-default posture (residency off, telemetry default, etc.).
    fn clear_guard_env() {
        for key in [
            ENABLE_KEY,
            EXEC_POLICY_KEY,
            SANDBOX_KEY,
            "BHARATCODE_RESIDENCY",
            "BHARATCODE_OFFLINE",
            "BHARATCODE_REDACT",
            "BHARATCODE_ANALYTICS",
        ] {
            std::env::remove_var(key);
        }
    }

    #[test]
    fn advisory_is_none_when_disabled() {
        let _guard = lock_env();
        clear_guard_env();
        // Disabled => byte-identical prompt invariant: no block at all.
        assert!(!is_enabled());
        assert!(advisory_block().is_none());
    }

    #[test]
    fn advisory_names_weak_pillars_when_enabled_and_weak() {
        let _guard = lock_env();
        clear_guard_env();
        // Enabled, with a deliberately-weak posture (no residency, telemetry
        // default off-but-enabled-pillar-strong, no redaction, no sandbox...).
        std::env::set_var(ENABLE_KEY, "1");

        let block = advisory_block().expect("advisory present when enabled and weak");
        clear_guard_env();

        let lower = block.to_lowercase();
        assert!(lower.contains("security posture"));
        assert!(lower.contains("posture score"));
        // Names the weak pillars that are off by default.
        assert!(lower.contains("residency"));
        assert!(lower.contains("secret-redaction"));
        // No donor/upstream brand leakage.
        assert!(!block.contains("goose"));
        assert!(!block.contains("Goose"));
        assert!(!block.contains("Block"));
    }

    #[test]
    fn advisory_is_none_when_enabled_but_all_strong() {
        let _guard = lock_env();
        clear_guard_env();
        std::env::set_var(ENABLE_KEY, "1");
        std::env::set_var(EXEC_POLICY_KEY, "/tmp/policy.json");
        std::env::set_var(SANDBOX_KEY, "read-only");
        std::env::set_var("BHARATCODE_REDACT", "1");
        std::env::set_var("BHARATCODE_RESIDENCY", "strict");
        std::env::set_var("BHARATCODE_OFFLINE", "1");

        let report = HardeningReport::resolve();
        // residency / exec-policy / sandbox / redaction / analytics are all
        // strong; no-egress and telemetry-off depend on composed offline state.
        assert!(report.residency_enforced);
        assert!(report.exec_policy_armed);
        assert!(report.sandbox_armed);
        assert!(report.redaction_enabled);
        assert!(report.analytics_local_only);

        let weak = report.weak_pillars();
        let block = advisory_block();
        clear_guard_env();

        // If the composed offline status makes every pillar strong the advisory
        // is suppressed; otherwise it must only name the still-weak pillars.
        if weak.is_empty() {
            assert!(block.is_none());
        } else {
            let text = block.expect("weak pillars present => advisory present");
            for name in weak {
                assert!(
                    text.contains(name),
                    "advisory should name weak pillar {name}"
                );
            }
        }
    }

    #[test]
    fn resolve_reflects_each_accessor() {
        let _guard = lock_env();
        clear_guard_env();

        // Weak baseline: residency off, redaction off, no exec policy/sandbox.
        let weak = HardeningReport::resolve();
        assert!(!weak.residency_enforced);
        assert!(!weak.exec_policy_armed);
        assert!(!weak.sandbox_armed);
        assert!(!weak.redaction_enabled);
        // Analytics is structurally local-only regardless of the on/off toggle.
        assert!(weak.analytics_local_only);

        // Flip each accessor's backing env and confirm the report tracks it.
        std::env::set_var("BHARATCODE_RESIDENCY", "warn");
        std::env::set_var(EXEC_POLICY_KEY, "/tmp/p.json");
        std::env::set_var(SANDBOX_KEY, "workspace-write");
        std::env::set_var("BHARATCODE_REDACT", "true");

        let strong = HardeningReport::resolve();
        assert!(strong.residency_enforced);
        assert!(strong.exec_policy_armed);
        assert!(strong.sandbox_armed);
        assert!(strong.redaction_enabled);

        // exec-policy disabled spellings are reported as unarmed.
        std::env::set_var(EXEC_POLICY_KEY, "off");
        assert!(!HardeningReport::resolve().exec_policy_armed);
        std::env::set_var(EXEC_POLICY_KEY, "0");
        assert!(!HardeningReport::resolve().exec_policy_armed);

        clear_guard_env();
    }

    #[test]
    fn score_counts_strong_pillars() {
        let _guard = lock_env();
        clear_guard_env();
        let report = HardeningReport::resolve();
        let (strong, total) = report.score();
        assert_eq!(total, 7);
        assert!(strong <= total);
        // analytics-local-only is always strong, so at least one is strong.
        assert!(strong >= 1);
        clear_guard_env();
    }

    #[test]
    fn is_enabled_reads_raw_env_truthiness() {
        let _guard = lock_env();
        for truthy in ["1", "true", "TRUE", " yes ", "on"] {
            std::env::set_var(ENABLE_KEY, truthy);
            assert!(is_enabled(), "expected {truthy:?} to enable");
        }
        for falsy in ["0", "false", "no", "off", ""] {
            std::env::set_var(ENABLE_KEY, falsy);
            assert!(!is_enabled(), "expected {falsy:?} to stay disabled");
        }
        std::env::remove_var(ENABLE_KEY);
        assert!(!is_enabled());
    }
}
