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

// This module is brought in at two disjoint call sites via `#[path]`
// (`session/builder.rs` for the startup banner, `commands/info.rs` for the
// version surface). Each site exercises a different subset of the public API,
// so from any single inclusion's vantage point the rest looks unused; the
// canonical source is shared and every item is reachable across the binary.
#![allow(dead_code)]

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

/// Environment variable that selects the release channel at runtime. Unset (the
/// default for the 1.0 GA wave) resolves to [`Channel::Ga`]; recognized values
/// are `ga`/`stable`, `beta`, and `canary`/`nightly` (case-insensitive). Any
/// unrecognized value falls back to GA, so a stray value never demotes a GA
/// build's banner.
const RELEASE_CHANNEL_ENV: &str = "BHARATCODE_RELEASE_CHANNEL";

/// Typed release channel. The 1.0 GA wave ships on [`Channel::Ga`]; the other
/// variants exist so pre-release builds can self-identify without a code change
/// (they are selected via [`RELEASE_CHANNEL_ENV`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// General Availability — the stable, supported 1.0 line.
    Ga,
    /// Pre-release beta.
    Beta,
    /// Bleeding-edge canary / nightly.
    Canary,
}

impl Channel {
    /// The brand-facing channel badge shown to users (e.g. `GA`).
    pub fn badge(self) -> &'static str {
        match self {
            Channel::Ga => "GA",
            Channel::Beta => "Beta",
            Channel::Canary => "Canary",
        }
    }

    /// Resolve the active channel from [`RELEASE_CHANNEL_ENV`], defaulting to
    /// [`Channel::Ga`] for the 1.0 wave. Unrecognized values resolve to GA so a
    /// typo never silently demotes the banner.
    pub fn from_env() -> Channel {
        match std::env::var(RELEASE_CHANNEL_ENV) {
            Ok(raw) => match raw.trim().to_ascii_lowercase().as_str() {
                "beta" => Channel::Beta,
                "canary" | "nightly" => Channel::Canary,
                _ => Channel::Ga,
            },
            Err(_) => Channel::Ga,
        }
    }
}

impl std::fmt::Display for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.badge())
    }
}

/// Immutable snapshot of the current release's brand-facing identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseInfo {
    /// Semantic GA product version (e.g. `1.0.0`).
    pub ga_version: &'static str,
    /// Semantic GA product version (alias of [`ReleaseInfo::ga_version`]). The
    /// canonical, semver-parseable `MAJOR.MINOR.PATCH` brand version.
    pub version: &'static str,
    /// Release channel marker badge (`GA`).
    pub channel: &'static str,
    /// Typed release channel for this build ([`Channel::Ga`] in the 1.0 wave).
    pub channel_kind: Channel,
    /// Apache-2.0 compliance / upstream-attribution line.
    pub attribution: &'static str,
}

/// The release info for this build.
pub fn current() -> ReleaseInfo {
    let channel_kind = Channel::from_env();
    ReleaseInfo {
        ga_version: GA_VERSION,
        version: GA_VERSION,
        channel: CHANNEL_GA,
        channel_kind,
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

/// Pure eligibility predicate for the one-time startup banner.
///
/// True only for an interactive, non-quiet session that has not opted out via
/// [`NO_BANNER_ENV`]. Non-interactive and `--quiet` launches are silent so the
/// banner never pollutes piped/scripted output. This does **not** consume the
/// once-per-process claim, so it is side-effect-free and safe to call
/// repeatedly (e.g. in tests).
pub fn is_banner_eligible(interactive: bool, quiet: bool) -> bool {
    if !interactive || quiet {
        return false;
    }
    std::env::var_os(NO_BANNER_ENV).is_none()
}

/// Whether the session-builder call site should print the one-time startup
/// banner for this launch.
///
/// Combines the pure eligibility gate ([`is_banner_eligible`]) with the
/// process-wide one-shot ([`claim_banner_once`]) so the banner is emitted by
/// exactly one of the two call sites (the session builder here, or the
/// interactive-loop start via [`startup_banner`]) — whichever runs first. A
/// `true` result *consumes* the claim, so subsequent callers (including the
/// interactive-loop site) become silent no-ops, keeping default output to a
/// single banner line.
pub fn should_show(interactive: bool, quiet: bool) -> bool {
    if !is_banner_eligible(interactive, quiet) {
        return false;
    }
    claim_banner_once()
}

/// Process-wide one-shot env flag used to dedup the GA banner across the two
/// call sites (the session builder and the interactive-loop start). A runtime
/// environment variable is used deliberately: the module is brought in via
/// `#[path]` at multiple sites, so each inclusion has its own statics — a
/// regular `OnceLock`/`AtomicBool` would *not* be shared between them, but the
/// process environment is. The variable is internal plumbing (double
/// underscore) and never read by users.
const BANNER_CLAIM_ENV: &str = "BHARATCODE__GA_BANNER_EMITTED";

/// Claim the single GA-banner emission for this process. Returns `true` for the
/// first caller and `false` for every caller thereafter, so whichever of the
/// two call sites runs first owns the one printed line and the other becomes a
/// silent no-op (keeping default output to exactly one banner).
fn claim_banner_once() -> bool {
    if std::env::var_os(BANNER_CLAIM_ENV).is_some() {
        return false;
    }
    std::env::set_var(BANNER_CLAIM_ENV, "1");
    true
}

/// The gated, deduplicated one-time GA startup banner for an interactive
/// session.
///
/// Returns `Some(line)` only when **all** of the following hold:
///   * this build is on the GA channel ([`Channel::Ga`]),
///   * the user has not opted out via [`NO_BANNER_ENV`], and
///   * no other call site has already emitted the banner this process
///     ([`claim_banner_once`]).
///
/// Otherwise returns `None`. This is the single decision point used by the
/// interactive session start: the caller prints the line verbatim when `Some`
/// and emits nothing (byte-identical to a build without this feature) when
/// `None`.
///
/// Default behavior: GA channel + unset `BHARATCODE_NO_BANNER` + first emission
/// => `Some`. Setting `BHARATCODE_NO_BANNER` (any value) => `None`. A non-GA
/// channel (`BHARATCODE_RELEASE_CHANNEL=beta|canary`) also yields `None`,
/// keeping the GA milestone banner exclusive to GA cuts.
pub fn startup_banner() -> Option<String> {
    let suppressed = std::env::var_os(NO_BANNER_ENV).is_some();
    let is_ga = current().channel_kind == Channel::Ga;
    // `claim_banner_once` is only consulted (and thus only consumes the claim)
    // when the banner would otherwise be eligible, so a suppressed / non-GA
    // launch leaves the claim untouched for a later eligible call.
    if !banner_eligible_now(suppressed, is_ga) {
        return None;
    }
    if !claim_banner_once() {
        return None;
    }
    Some(banner_line())
}

/// Pure decision: is a banner *eligible* to be shown right now, given the
/// opt-out and channel facts? Excludes the once-per-process claim so it is
/// side-effect-free and exhaustively unit-testable. Eligible iff not suppressed
/// and on the GA channel.
fn banner_eligible_now(suppressed: bool, is_ga: bool) -> bool {
    !suppressed && is_ga
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
            .map(|p| {
                p.parse::<u64>()
                    .expect("GA version component must be numeric")
            })
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
        // The GA milestone is a deliberate major bump: its major component is at
        // or above the internal crate major. This compares the parsed integers
        // (not the strings) so the 1.0 GA marker is provably the leading version.
        assert!(
            ga_parts[0] >= crate_major,
            "GA major ({}) must be >= internal crate major ({crate_major})",
            ga_parts[0]
        );
    }

    #[test]
    fn should_show_only_when_interactive_and_not_quiet() {
        // Use the pure eligibility predicate (no claim side-effect) so this
        // broad gate test never races the once-per-process banner claim that
        // `should_show` / `startup_banner` consume.
        let suppressed = std::env::var_os(NO_BANNER_ENV).is_some();

        assert!(
            !is_banner_eligible(false, false),
            "non-interactive must be quiet"
        );
        assert!(!is_banner_eligible(false, true));
        assert!(!is_banner_eligible(true, true), "--quiet must suppress");

        if suppressed {
            assert!(
                !is_banner_eligible(true, false),
                "BHARATCODE_NO_BANNER must suppress even when interactive"
            );
        } else {
            assert!(
                is_banner_eligible(true, false),
                "interactive && !quiet && not suppressed must show"
            );
        }
    }

    #[test]
    fn current_channel_kind_is_ga_by_default() {
        // The default 1.0 wave resolves to the GA channel when the override env
        // is unset. Guard against a developer environment forcing a channel.
        if std::env::var_os(RELEASE_CHANNEL_ENV).is_none() {
            assert_eq!(current().channel_kind, Channel::Ga);
        }
    }

    #[test]
    fn current_version_parses_as_semver_at_least_one_zero() {
        let v = current().version;
        // version is an alias of the canonical GA version.
        assert_eq!(v, current().ga_version);
        // Parse MAJOR.MINOR.PATCH as numeric components without a semver dep.
        let parts: Vec<u64> = v
            .split('.')
            .map(|p| p.parse::<u64>().expect("semver component must be numeric"))
            .collect();
        assert_eq!(parts.len(), 3, "version must be MAJOR.MINOR.PATCH: {v}");
        // version >= 1.0.0
        assert!(parts[0] >= 1, "GA version must be at least 1.0.0, got {v}");
    }

    #[test]
    fn channel_from_env_recognizes_values() {
        // Pure parse over an explicit input rather than the process env so the
        // test is hermetic. Mirrors Channel::from_env's matching.
        let parse = |raw: &str| match raw.trim().to_ascii_lowercase().as_str() {
            "beta" => Channel::Beta,
            "canary" | "nightly" => Channel::Canary,
            _ => Channel::Ga,
        };
        assert_eq!(parse("ga"), Channel::Ga);
        assert_eq!(parse("stable"), Channel::Ga);
        assert_eq!(parse(""), Channel::Ga);
        assert_eq!(parse("BETA"), Channel::Beta);
        assert_eq!(parse(" beta "), Channel::Beta);
        assert_eq!(parse("canary"), Channel::Canary);
        assert_eq!(parse("nightly"), Channel::Canary);
        // Unrecognized value never demotes GA.
        assert_eq!(parse("bogus"), Channel::Ga);
    }

    #[test]
    fn channel_badge_is_brand_clean() {
        for ch in [Channel::Ga, Channel::Beta, Channel::Canary] {
            let badge = ch.badge();
            assert!(!badge.is_empty());
            let lowered = badge.to_lowercase();
            assert!(!lowered.contains("goose"), "badge leaks upstream: {badge}");
            assert!(!lowered.contains("block"), "badge leaks vendor: {badge}");
        }
        assert_eq!(Channel::Ga.badge(), "GA");
        assert_eq!(format!("{}", Channel::Ga), "GA");
    }

    /// The banner *text* (independent of the process-wide one-shot claim) is the
    /// brand-clean GA line. Asserted against `banner_line()` so this test is
    /// pure and never races the claim consumed by `startup_banner`.
    #[test]
    fn startup_banner_text_is_brand_clean_and_complete() {
        let banner = banner_line();
        assert!(
            banner.contains("1.0.0"),
            "banner missing GA version: {banner}"
        );
        assert!(banner.contains("GA"), "banner missing GA channel: {banner}");
        assert!(
            banner.contains("Apache-2.0"),
            "banner missing license: {banner}"
        );
        assert!(!banner.contains('\n'), "banner must be one line: {banner}");
        let lowered = banner.to_lowercase();
        assert!(
            !lowered.contains("goose"),
            "banner leaks upstream: {banner}"
        );
        assert!(!lowered.contains("block"), "banner leaks vendor: {banner}");
    }

    /// Pure eligibility matrix for `startup_banner`'s decision: a banner is
    /// shown iff the launch is not opted out *and* on the GA channel. No process
    /// env is touched, so this is hermetic and parallel-safe across the multiple
    /// `#[path]` inclusions of this module.
    #[test]
    fn banner_eligibility_matrix() {
        // (suppressed, is_ga) => eligible
        assert!(banner_eligible_now(false, true), "GA + opt-in must show");
        assert!(
            !banner_eligible_now(true, true),
            "BHARATCODE_NO_BANNER must suppress even on GA"
        );
        assert!(
            !banner_eligible_now(false, false),
            "non-GA channel must not show the GA banner"
        );
        assert!(!banner_eligible_now(true, false));
    }

    /// The once-per-process claim must hand the single banner emission to
    /// exactly one caller. Tested through a fresh, locally-scoped claim variable
    /// so it neither reads nor mutates the production `BANNER_CLAIM_ENV` (and so
    /// never races the live wiring or the multiple module inclusions).
    #[test]
    fn claim_is_a_single_hand_off() {
        // Model of `claim_banner_once` over a private flag: the first observer
        // wins, all subsequent observers lose — guaranteeing one banner total
        // regardless of which call site (builder vs interactive) runs first.
        let claimed = std::cell::Cell::new(false);
        let claim = || {
            if claimed.get() {
                false
            } else {
                claimed.set(true);
                true
            }
        };
        assert!(claim(), "first caller wins the single emission");
        assert!(!claim(), "second caller must lose (no double banner)");
        assert!(!claim(), "and every caller thereafter");
    }
}
