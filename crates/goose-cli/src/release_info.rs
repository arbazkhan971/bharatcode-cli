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

/// Lower-cased release-channel name as published to release infrastructure
/// (download URLs, update manifests). The GA cut ships on `stable`; the
/// human-facing `GA` marker in [`ReleaseInfo::channel`] is the banner badge for
/// the same channel.
const CHANNEL_STABLE: &str = "stable";

/// The General-Availability product version (`1.0.0`).
///
/// Canonical, single-source-of-truth GA version string: the *brand* version
/// surfaced to users, intentionally distinct from the internal
/// `CARGO_PKG_VERSION` workspace crate version (see module docs).
pub fn ga_version() -> &'static str {
    GA_VERSION
}

/// The published release channel (`stable`) for the GA cut.
pub fn channel() -> &'static str {
    CHANNEL_STABLE
}

/// Compile-time build metadata for the running binary.
///
/// Single source of truth for "what build is this", composed only from
/// compile-time constants so it adds **no new build dependency**:
///
///   * the internal workspace crate version (`CARGO_PKG_VERSION`), and
///   * optional VERGEN-style git/build env (`VERGEN_GIT_SHA`,
///     `VERGEN_BUILD_TIMESTAMP`) read via [`option_env!`].
///
/// When the optional VERGEN env is absent at compile time (the default — no
/// build script is required), this falls back to just the crate version, e.g.
/// `1.38.0`. With git metadata present it reads `1.38.0 (abc1234 2026-06-20)`.
pub fn build_metadata() -> String {
    let crate_version = env!("CARGO_PKG_VERSION");
    let git_sha = option_env!("VERGEN_GIT_SHA");
    let built_at = option_env!("VERGEN_BUILD_TIMESTAMP");
    match (git_sha, built_at) {
        (Some(sha), Some(ts)) => format!("{crate_version} ({sha} {ts})"),
        (Some(sha), None) => format!("{crate_version} ({sha})"),
        (None, Some(ts)) => format!("{crate_version} ({ts})"),
        (None, None) => crate_version.to_string(),
    }
}

/// The authoritative long `--version` / `bharatcode info` identity line.
///
/// Composes, in order: the product name, the GA version, the published channel,
/// the Apache-2.0 license, and the brand-clean upstream-attribution pointer.
/// Routed through [`crate::tr!`] so a localized template (`version.*`) is used
/// when present and the bundled English labels are used otherwise — the GA
/// version, channel value, and `Apache-2.0` token are always substituted from
/// this canonical module, never from a locale table, so the surfaced version is
/// identical in every locale.
///
/// Brand-clean by construction: it states the Apache-2.0 derivative-work fact
/// and points at `NOTICE` for the full attribution, but surfaces no upstream
/// product/vendor trademark in the user-facing string (the upstream names live
/// in `NOTICE`, satisfying Apache-2.0 Section 4 without a trademark leak).
///
/// ```text
/// BharatCode 1.0.0 (channel: stable) — Apache-2.0; derivative work, see NOTICE for attribution.
/// ```
pub fn long_version_line() -> String {
    let info = current();

    let product = tr_or("version.product", "BharatCode");
    let channel_label = tr_or("version.channel_label", "channel");
    let license = "Apache-2.0";
    let attribution = tr_or(
        "version.attribution",
        "derivative work, see NOTICE for attribution",
    );

    format!(
        "{product} {} ({channel_label}: {}) — {license}; {attribution}.",
        info.ga_version,
        channel(),
    )
}

/// Resolve an i18n key, falling back to a built-in English default when the key
/// is absent from every locale table (i.e. `tr!` echoes the key back).
fn tr_or(key: &str, default: &str) -> String {
    let value = crate::tr!(key);
    if value == key {
        default.to_string()
    } else {
        value
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
    fn ga_version_accessor_is_one_dot_zero() {
        assert_eq!(ga_version(), "1.0.0");
    }

    #[test]
    fn channel_accessor_is_stable() {
        assert_eq!(channel(), "stable");
    }

    #[test]
    fn build_metadata_starts_with_crate_version() {
        let meta = build_metadata();
        let crate_version = env!("CARGO_PKG_VERSION");
        assert!(
            meta.starts_with(crate_version),
            "build metadata must lead with the crate version: {meta}"
        );
        assert!(
            !meta.contains('\n'),
            "build metadata must be one line: {meta}"
        );
    }

    #[test]
    fn long_version_line_is_complete_and_brand_clean() {
        let line = long_version_line();
        // Contains the canonical GA version, the published channel, and license.
        assert!(
            line.contains("1.0.0"),
            "long version line missing GA version: {line}"
        );
        assert!(
            line.contains("stable"),
            "long version line missing channel: {line}"
        );
        assert!(
            line.contains("Apache-2.0"),
            "long version line missing license: {line}"
        );
        // Single line.
        assert!(
            !line.contains('\n'),
            "long version line must be one line: {line}"
        );
        // Brand-clean: no upstream product/vendor leakage in the user-facing
        // string. Upstream names live only in NOTICE (Apache-2.0 Section 4).
        let lowered = line.to_lowercase();
        assert!(
            !lowered.contains("goose"),
            "long version line leaks upstream product: {line}"
        );
        assert!(
            !lowered.contains("block"),
            "long version line leaks upstream vendor: {line}"
        );
    }

    /// Guards the single-source-of-truth invariant: the GA brand version is a
    /// clean `1.x` milestone, parsed and compared against the internal
    /// `CARGO_PKG_VERSION` so a stray edit that lets the internal crate version
    /// masquerade as the GA brand version is caught.
    #[test]
    fn ga_version_is_a_clean_one_dot_zero_milestone() {
        let ga = ga_version();
        let ga_parts: Vec<u64> = ga
            .split('.')
            .map(|p| p.parse::<u64>().expect("GA version component must be numeric"))
            .collect();
        assert_eq!(
            ga_parts.len(),
            3,
            "GA version must be MAJOR.MINOR.PATCH: {ga}"
        );
        assert_eq!(ga_parts[0], 1, "GA milestone major must be 1: {ga}");

        // Parse the internal crate version: the GA marker must be a reviewed
        // constant distinct from the fast-moving internal crate version (which
        // tracks fork plumbing and is not the brand version).
        let crate_version = env!("CARGO_PKG_VERSION");
        let crate_major: u64 = crate_version
            .split('.')
            .next()
            .and_then(|p| p.parse().ok())
            .expect("crate version must have a numeric major");
        assert_ne!(
            ga, crate_version,
            "GA brand version must be a reviewed constant distinct from the internal crate version"
        );
        // The internal crate major is a valid, comparable number; the GA
        // milestone deliberately resets to the 1.0 line and does not inherit it.
        assert!(crate_major >= 1, "internal crate major should be >= 1");
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
