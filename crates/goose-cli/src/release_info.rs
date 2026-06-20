//! Canonical GA release-info source for BharatCode.
//!
//! A single, self-contained place that answers "what release is this, and how
//! should it be announced once at startup?". It deliberately keeps the
//! *marketing* GA version (the `1.0` General-Availability milestone) separate
//! from the *internal* crate/workspace version: the workspace `Cargo.toml`
//! tracks fast-moving fork plumbing (currently `1.38.x`), while the
//! user-facing product reaches GA at `1.0.0`. Conflating the two would leak an
//! incidental internal number into a brand-facing banner, so the GA marker is
//! pinned here as a deliberate, reviewed constant.
//!
//! The surfaced artifact is a brand-clean, single-line banner:
//!
//! ```text
//! BharatCode 1.0.0 (GA) — Apache-2.0
//! ```
//!
//! It is shown exactly once, at the start of an interactive session, and is
//! quiet by default under `--quiet` / non-interactive launches. The banner is
//! routed through the i18n `t()` fallback ([`crate::tr!`]) so a localized
//! template can be added later without touching this module; until then the
//! built-in English line is used verbatim.
//!
//! Everything here is pure (no I/O beyond an env-var read in [`should_show`]),
//! so the release struct and the banner string are trivially unit-testable.

/// The General-Availability product version. This is the *brand* version, not
/// the internal crate version; it is intentionally pinned to the `1.0.0` GA
/// milestone and reviewed by hand on each GA cut.
const GA_VERSION: &str = "1.0.0";

/// Release channel marker surfaced in the banner.
const CHANNEL_GA: &str = "GA";

/// Apache-2.0 compliance / upstream-attribution line. Mirrors the project
/// `NOTICE`: BharatCode is a derivative work of Goose, distributed under the
/// Apache License 2.0. Kept terse and brand-clean (it names the upstream
/// project as a licensing fact, which Apache-2.0 Section 4 requires, but does
/// not surface any upstream trademark in the banner shown to users).
const ATTRIBUTION: &str =
    "Derivative work under the Apache License 2.0 — see NOTICE for attribution.";

/// i18n key for the localizable banner template. Absent from the bundled
/// locale tables today, so [`crate::tr!`] resolves it to the key itself; that
/// is the signal to fall back to the built-in English banner. Adding this key
/// to the locale JSON later (with `{version}` / `{channel}` placeholders)
/// localizes the banner with no code change here.
const BANNER_KEY: &str = "release.banner";

/// Environment variable that suppresses the one-time startup banner even on an
/// interactive launch. Unset (the default) leaves the banner enabled for
/// interactive sessions.
const NO_BANNER_ENV: &str = "BHARATCODE_NO_BANNER";

/// Immutable snapshot of the current release's brand-facing identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseInfo {
    /// Semantic GA product version (e.g. `1.0.0`).
    pub ga_version: &'static str,
    /// Release channel marker (`GA`).
    pub channel: &'static str,
    /// Apache-2.0 compliance / upstream-attribution line.
    pub attribution: &'static str,
}

/// The release info for this build.
pub fn current() -> ReleaseInfo {
    ReleaseInfo {
        ga_version: GA_VERSION,
        channel: CHANNEL_GA,
        attribution: ATTRIBUTION,
    }
}

/// The brand-clean, single-line startup banner.
///
/// Routed through the i18n `t()` fallback: when a localized `release.banner`
/// template is registered it is used (with `{version}`/`{channel}`
/// substitution); until then this returns the built-in English line:
///
/// ```text
/// BharatCode 1.0.0 (GA) — Apache-2.0
/// ```
pub fn banner_line() -> String {
    let info = current();
    let template = crate::tr!(BANNER_KEY);
    if template != BANNER_KEY {
        // A localized template exists: fill in the brand placeholders.
        return template
            .replace("{version}", info.ga_version)
            .replace("{channel}", info.channel);
    }
    // No localization yet: emit the canonical brand-clean English banner.
    format!(
        "BharatCode {} ({}) — Apache-2.0",
        info.ga_version, info.channel
    )
}

/// Whether the one-time startup banner should be shown for this launch.
///
/// True only for an interactive, non-quiet session that has not opted out via
/// [`NO_BANNER_ENV`]. Non-interactive and `--quiet` launches are silent so the
/// banner never pollutes piped/scripted output.
pub fn should_show(interactive: bool, quiet: bool) -> bool {
    if !interactive || quiet {
        return false;
    }
    std::env::var_os(NO_BANNER_ENV).is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ga_version_is_one_dot_zero_semver() {
        let v = current().ga_version;
        assert_eq!(v, "1.0.0");
        // Parse as a semantic version without pulling a semver dependency:
        // exactly three dot-separated numeric components, equal to 1.0.0.
        let parts: Vec<u64> = v
            .split('.')
            .map(|p| p.parse::<u64>().expect("semver component must be numeric"))
            .collect();
        assert_eq!(parts, vec![1, 0, 0]);
    }

    #[test]
    fn current_is_ga_channel() {
        assert_eq!(current().channel, "GA");
    }

    #[test]
    fn banner_is_brand_clean_and_complete() {
        let line = banner_line();
        // Contains the GA version, the GA channel marker, and the license.
        assert!(line.contains("1.0.0"), "banner missing GA version: {line}");
        assert!(line.contains("GA"), "banner missing GA channel: {line}");
        assert!(
            line.contains("Apache-2.0"),
            "banner missing license: {line}"
        );
        // Single line.
        assert!(!line.contains('\n'), "banner must be one line: {line}");
        // Brand-clean: no upstream product/vendor leakage.
        let lowered = line.to_lowercase();
        assert!(!lowered.contains("goose"), "banner leaks upstream: {line}");
        assert!(!lowered.contains("block"), "banner leaks vendor: {line}");
    }

    #[test]
    fn attribution_is_apache_and_clean() {
        let a = current().attribution;
        assert!(a.contains("Apache License 2.0"));
        let lowered = a.to_lowercase();
        assert!(!lowered.contains("goose"));
        assert!(!lowered.contains("block"));
    }

    #[test]
    fn should_show_only_when_interactive_and_not_quiet() {
        // Guard against a developer's environment having the opt-out set.
        let suppressed = std::env::var_os(NO_BANNER_ENV).is_some();

        assert!(!should_show(false, false), "non-interactive must be quiet");
        assert!(!should_show(false, true));
        assert!(!should_show(true, true), "--quiet must suppress");

        if suppressed {
            assert!(
                !should_show(true, false),
                "BHARATCODE_NO_BANNER must suppress even when interactive"
            );
        } else {
            assert!(
                should_show(true, false),
                "interactive && !quiet && not suppressed must show"
            );
        }
    }
}
