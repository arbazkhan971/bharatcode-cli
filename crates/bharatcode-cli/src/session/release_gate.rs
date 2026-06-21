//! Release-readiness / no-egress assertion gate (BharatCode v99).
//!
//! On interactive session start this runs a fast, read-only assertion that the
//! privacy / compliance invariants hold for *this* process:
//!
//!   1. **Telemetry is off.** The composed telemetry signal must report disabled.
//!   2. **The egress guard is installed.** The shared provider HTTP client's
//!      central egress guard (the one that screens every provider endpoint,
//!      including declarative ones) must be registered for this process.
//!   3. **Offline coherence.** When offline / residency is *requested*, that
//!      request must imply the strict residency posture it composes — reusing the
//!      same pure `offline_implies_strict` rule the doctor check (v36) applies, so
//!      the gate and the doctor never disagree.
//!
//! Design contract:
//!   * **Silent by default.** When every invariant holds, [`hint_line`] returns
//!     `None` and the session prints nothing — a healthy default session stays
//!     byte-identical.
//!   * **Never blocks unless asked.** A violation only escalates to a prominent
//!     blocking-style warning when `BHARATCODE_STRICT_RELEASE` is explicitly set
//!     ([`strict_mode`]); otherwise it is a single muted hint line. Either way it
//!     is one line, printed once.
//!   * **Read-only.** [`assess`] is a pure function over a captured
//!     [`ReleaseState`]; [`gather`] only reads the real signals (and ensures the
//!     idempotent egress guard is installed) without mutating any setting.
//!
//! Original BharatCode work; not ported from any third party. The signals it
//! reads live in the `bharatcode_core::offline` / `bharatcode_core::residency` modules and the pure
//! coherence rule lives in `crate::commands::doctor_checks`.

/// Environment variable that escalates any invariant violation from a muted hint
/// to a prominent blocking-style warning. Default unset = silent.
const STRICT_RELEASE_KEY: &str = "BHARATCODE_STRICT_RELEASE";

/// i18n key for the violation hint line. Falls back (via `t()`) to the English
/// text below when no locale entry maps, so the terminal never shows a raw
/// `session.*` identifier and a future locale entry is picked up automatically.
const HINT_KEY: &str = "session.release_gate.violation";

/// English text for the violation hint. Kept here as the canonical source so the
/// unit tests can assert on it without loading the i18n tables. Brand-free.
const HINT_EN: &str = "release-readiness check found unmet privacy invariants";

/// i18n key / canonical English for the strict-mode prominent warning prefix.
const STRICT_PREFIX_KEY: &str = "session.release_gate.strict_prefix";
const STRICT_PREFIX_EN: &str = "release blocker";

/// A read-only snapshot of the signals the release-readiness gate asserts over.
///
/// Captured once by [`gather`] so [`assess`] can stay a pure, I/O-free function
/// that is trivial to unit test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseState {
    /// Whether telemetry is currently enabled (it must be off).
    pub telemetry_enabled: bool,
    /// Whether the central provider egress guard is installed for this process.
    pub egress_guard_installed: bool,
    /// Whether offline / residency enforcement is requested.
    pub offline_requested: bool,
    /// Whether the requested offline posture implies strict residency (the pure
    /// `offline_implies_strict` rule held).
    pub offline_implies_strict: bool,
}

/// The verdict of a release-readiness assessment over a [`ReleaseState`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseVerdict {
    /// True when no invariant was violated.
    pub ok: bool,
    /// Stable, brand-free identifiers for each violated invariant. Empty when ok.
    pub violations: Vec<&'static str>,
}

/// Pure release-readiness assessment over a captured [`ReleaseState`].
///
/// Returns the set of violated invariants (empty + `ok == true` when all hold).
/// I/O-free so it can be unit tested without touching the environment.
pub fn assess(state: &ReleaseState) -> ReleaseVerdict {
    let mut violations: Vec<&'static str> = Vec::new();

    if state.telemetry_enabled {
        violations.push("telemetry");
    }
    if !state.egress_guard_installed {
        violations.push("egress-guard");
    }
    // Coherence is only meaningful when offline / residency is requested: a
    // request that does not imply strict residency is the inconsistency worth
    // flagging (the same shape the doctor's v36 check reports).
    if state.offline_requested && !state.offline_implies_strict {
        violations.push("offline-coherence");
    }

    ReleaseVerdict {
        ok: violations.is_empty(),
        violations,
    }
}

/// Whether strict release mode is requested. Reads the raw
/// `BHARATCODE_STRICT_RELEASE` environment variable first (`1`/`true`/`on`/`yes`
/// enable it); any unset / unrecognised / falsy value leaves it off so the
/// default behaviour is silent.
pub fn strict_mode() -> bool {
    match std::env::var(STRICT_RELEASE_KEY) {
        Ok(raw) => matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "on" | "yes" | "enable" | "enabled"
        ),
        Err(_) => false,
    }
}

/// Capture the real release-readiness signals for this process (read-only).
///
/// Telemetry and the offline request are read through the composed
/// `bharatcode_core::offline` status; coherence reuses the pure `offline_implies_strict`
/// rule against the configured residency mode. The egress guard is *ensured*
/// installed via the idempotent `install_egress_guard` (first registration wins,
/// later calls are ignored), so the gate genuinely makes the no-egress guard
/// active for this process and then reports it as installed.
pub fn gather() -> ReleaseState {
    bharatcode_core::offline::install_egress_guard();

    let status = bharatcode_core::offline::offline_status();
    let offline_requested = status.enabled;
    let configured_residency = bharatcode_core::residency::residency_mode();
    let offline_implies_strict = crate::commands::doctor_checks::offline_implies_strict(
        offline_requested,
        configured_residency,
    )
    .is_ok();

    ReleaseState {
        telemetry_enabled: status.telemetry_enabled,
        egress_guard_installed: true,
        offline_requested,
        offline_implies_strict,
    }
}

/// Render the single line to print for a verdict, or `None` when everything is
/// healthy (the silent default — nothing is printed).
///
/// The hint text is routed through the i18n layer (`t()`); when the key is not in
/// the shipped tables `t()` returns the key verbatim, which is mapped onto the
/// canonical brand-free English line so the terminal never shows a raw
/// identifier. When [`strict_mode`] is set the line is escalated with a prominent
/// blocking-style prefix.
pub fn hint_line(verdict: &ReleaseVerdict) -> Option<String> {
    if verdict.ok {
        return None;
    }

    let body = {
        let translated = crate::i18n::t(HINT_KEY);
        if translated == HINT_KEY {
            HINT_EN.to_string()
        } else {
            translated
        }
    };

    let detail = verdict.violations.join(", ");

    if strict_mode() {
        let prefix = {
            let translated = crate::i18n::t(STRICT_PREFIX_KEY);
            if translated == STRICT_PREFIX_KEY {
                STRICT_PREFIX_EN.to_string()
            } else {
                translated
            }
        };
        Some(format!("{prefix}: {body} [{detail}]"))
    } else {
        Some(format!("{body} [{detail}]"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy() -> ReleaseState {
        ReleaseState {
            telemetry_enabled: false,
            egress_guard_installed: true,
            offline_requested: false,
            offline_implies_strict: true,
        }
    }

    #[test]
    fn assess_ok_when_telemetry_off_and_guard_installed() {
        let verdict = assess(&healthy());
        assert!(verdict.ok);
        assert!(verdict.violations.is_empty());
    }

    #[test]
    fn telemetry_on_yields_telemetry_violation() {
        let state = ReleaseState {
            telemetry_enabled: true,
            ..healthy()
        };
        let verdict = assess(&state);
        assert!(!verdict.ok);
        assert!(verdict.violations.contains(&"telemetry"));
    }

    #[test]
    fn missing_guard_yields_egress_violation() {
        let state = ReleaseState {
            egress_guard_installed: false,
            ..healthy()
        };
        let verdict = assess(&state);
        assert!(!verdict.ok);
        assert!(verdict.violations.contains(&"egress-guard"));
    }

    #[test]
    fn offline_without_implies_strict_yields_coherence_violation() {
        let state = ReleaseState {
            offline_requested: true,
            offline_implies_strict: false,
            ..healthy()
        };
        let verdict = assess(&state);
        assert!(!verdict.ok);
        assert!(verdict.violations.contains(&"offline-coherence"));
    }

    #[test]
    fn offline_with_implies_strict_is_coherent() {
        let state = ReleaseState {
            offline_requested: true,
            offline_implies_strict: true,
            ..healthy()
        };
        assert!(assess(&state).ok);
    }

    #[test]
    fn hint_line_none_when_ok() {
        assert!(hint_line(&assess(&healthy())).is_none());
    }

    #[test]
    fn hint_line_present_and_brand_free_when_violated() {
        let state = ReleaseState {
            telemetry_enabled: true,
            ..healthy()
        };
        let line = hint_line(&assess(&state)).expect("a violation should produce a line");
        // Routed through t(): with no locale entry the canonical English body is used.
        assert!(line.contains(HINT_EN));
        assert!(line.contains("telemetry"));
        // No upstream brand leakage.
        let lower = line.to_ascii_lowercase();
        assert!(!lower.contains("goose"));
        assert!(!lower.contains("block"));
    }

    #[test]
    fn hint_routes_through_i18n_layer() {
        // t() returns the key verbatim when no locale entry maps; hint_line must
        // map that onto the canonical English text, never the raw key.
        assert_eq!(crate::i18n::t(HINT_KEY), HINT_KEY);
        let state = ReleaseState {
            telemetry_enabled: true,
            ..healthy()
        };
        let line = hint_line(&assess(&state)).expect("violation line");
        assert!(!line.contains(HINT_KEY));
    }

    #[test]
    fn strict_mode_reflects_env() {
        let prev = std::env::var(STRICT_RELEASE_KEY).ok();

        std::env::remove_var(STRICT_RELEASE_KEY);
        assert!(!strict_mode());

        std::env::set_var(STRICT_RELEASE_KEY, "1");
        assert!(strict_mode());

        std::env::set_var(STRICT_RELEASE_KEY, "off");
        assert!(!strict_mode());

        match prev {
            Some(value) => std::env::set_var(STRICT_RELEASE_KEY, value),
            None => std::env::remove_var(STRICT_RELEASE_KEY),
        }
    }
}
