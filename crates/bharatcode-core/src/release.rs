//! Canonical product release identity for BharatCode (v98: 1.0 GA).
//!
//! This module is the single source of truth for "what release is this": the
//! General-Availability semantic version, the active release [`Channel`]
//! (`stable` / `beta` / `nightly`), the compile-time build metadata, and the
//! human-facing GA banner string used at the 1.0 milestone. It is exposed as
//! reachable public API on the crate (`bharatcode_core::release`, the product's
//! `bharatcode::release` surface) so the version surface, the `serve` flow, and
//! the update flow can all share one identity instead of re-deriving it.
//!
//! Everything here is **pure**: the only runtime input is a single env-var read
//! in [`resolve_channel`] (which is also offered as a pure overload taking the
//! raw value). The GA version, channel mapping, and banner are otherwise
//! computed from compile-time constants, so the whole surface is trivially
//! unit-testable and free of I/O.
//!
//! ## Brand cleanliness
//!
//! The GA brand version (`1.0.0`) is intentionally distinct from the internal
//! workspace crate version (`CARGO_PKG_VERSION`, which tracks fast-moving fork
//! plumbing). Conflating them would leak an incidental internal number into a
//! brand-facing banner, so the GA marker is pinned here as a reviewed constant.
//! The user-facing banner names neither the upstream product nor vendor; the
//! Apache-2.0 derivative-work attribution lives in the project `NOTICE`
//! (satisfying Apache-2.0 Section 4 without a trademark leak).
//!
//! Original BharatCode work; not ported from any third party (std only).

/// The General-Availability product version for the 1.0 milestone.
///
/// This is the **brand** semantic version surfaced to users, intentionally
/// distinct from the internal `CARGO_PKG_VERSION` crate version. It is a
/// reviewed constant, pinned to the `1.0.0` GA cut.
pub const GA_VERSION: &str = "1.0.0";

/// Environment variable that selects the release channel at runtime.
///
/// Recognized values are `stable`, `beta`, and `nightly` (case-insensitive,
/// surrounding whitespace ignored). Unset or any unrecognized value resolves to
/// [`Channel::Stable`], so a stray value never silently demotes a GA build.
pub const CHANNEL_ENV: &str = "BHARATCODE_RELEASE_CHANNEL";

/// A parsed semantic version (`MAJOR.MINOR.PATCH`), without pulling in a semver
/// dependency. Pre-release / build-metadata suffixes are not modeled because
/// the GA brand version never carries them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SemVer {
    /// Major version component.
    pub major: u64,
    /// Minor version component.
    pub minor: u64,
    /// Patch version component.
    pub patch: u64,
}

impl SemVer {
    /// Parse a `MAJOR.MINOR.PATCH` string into a [`SemVer`].
    ///
    /// Returns `None` if the input does not have exactly three dot-separated
    /// numeric components.
    pub fn parse(raw: &str) -> Option<SemVer> {
        let mut parts = raw.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(SemVer {
            major,
            minor,
            patch,
        })
    }
}

impl std::fmt::Display for SemVer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The parsed GA semantic version. Panics at first use only if [`GA_VERSION`]
/// is malformed, which the unit tests guard against.
pub fn ga_semver() -> SemVer {
    SemVer::parse(GA_VERSION).expect("GA_VERSION must be a valid MAJOR.MINOR.PATCH semver")
}

/// Whether this build is the General-Availability `1.x` line.
///
/// True when the GA semantic version has major component `1` (the 1.0 GA
/// milestone and any subsequent `1.x` maintenance release).
pub fn is_ga() -> bool {
    ga_semver().major == 1
}

/// The product release channel.
///
/// The 1.0 GA wave ships on [`Channel::Stable`]; the other variants let a
/// pre-release build self-identify (selected via [`CHANNEL_ENV`]) without a
/// code change.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Channel {
    /// The stable, supported GA line. Default.
    #[default]
    Stable,
    /// Pre-release beta.
    Beta,
    /// Bleeding-edge nightly.
    Nightly,
}

impl Channel {
    /// The lower-case channel slug as published to release infrastructure
    /// (download URLs, update manifests). Round-trips with [`Channel::parse`].
    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Beta => "beta",
            Channel::Nightly => "nightly",
        }
    }

    /// Parse a channel slug. Recognizes `stable`, `beta`, and `nightly`
    /// (case-insensitive, surrounding whitespace ignored). Any other value —
    /// including the empty string — falls back to [`Channel::Stable`] so a typo
    /// never demotes a GA build.
    pub fn parse(raw: &str) -> Channel {
        match raw.trim().to_ascii_lowercase().as_str() {
            "beta" => Channel::Beta,
            "nightly" => Channel::Nightly,
            _ => Channel::Stable,
        }
    }
}

impl std::fmt::Display for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Resolve the active release channel from the process environment.
///
/// Reads [`CHANNEL_ENV`]; unset or unrecognized resolves to the default
/// [`Channel::Stable`]. This is the single runtime input of the module.
pub fn resolve_channel() -> Channel {
    match std::env::var(CHANNEL_ENV) {
        Ok(raw) => Channel::parse(&raw),
        Err(_) => Channel::default(),
    }
}

/// Compile-time build metadata for the running binary.
///
/// Composed only from compile-time constants so it adds **no new build
/// dependency**: the internal workspace crate version (`CARGO_PKG_VERSION`)
/// plus optional VERGEN-style git/build env (`VERGEN_GIT_SHA`,
/// `VERGEN_BUILD_TIMESTAMP`) read via [`option_env!`]. With no VERGEN env
/// present (the default — no build script required) this is just the crate
/// version, e.g. `1.38.0`; with git metadata it reads `1.38.0 (abc1234)`.
pub fn build_metadata() -> String {
    let crate_version = env!("CARGO_PKG_VERSION");
    match (
        option_env!("VERGEN_GIT_SHA"),
        option_env!("VERGEN_BUILD_TIMESTAMP"),
    ) {
        (Some(sha), Some(ts)) => format!("{crate_version} ({sha} {ts})"),
        (Some(sha), None) => format!("{crate_version} ({sha})"),
        (None, Some(ts)) => format!("{crate_version} ({ts})"),
        (None, None) => crate_version.to_string(),
    }
}

/// Immutable snapshot of this build's brand-facing release identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// The GA brand semantic version (`1.0.0`).
    pub version: SemVer,
    /// The active release channel.
    pub channel: Channel,
    /// Compile-time build metadata (crate version + optional git/build env).
    pub build: String,
}

/// The release identity for this build, resolving the channel from the
/// environment. This is the convenience entry point for the version surface,
/// the `serve` flow, and the update flow.
pub fn resolve() -> Release {
    Release {
        version: ga_semver(),
        channel: resolve_channel(),
        build: build_metadata(),
    }
}

/// The single-line, brand-clean GA banner shown at the 1.0 milestone.
///
/// Brand-clean by construction: it states the product name, the `1.0` GA
/// version, and the Apache-2.0 license, but surfaces no upstream product or
/// vendor trademark (those live in `NOTICE`, satisfying Apache-2.0 Section 4).
///
/// ```text
/// BharatCode 1.0.0 (GA) — Apache-2.0
/// ```
pub fn ga_banner() -> String {
    format!("BharatCode {GA_VERSION} (GA) — Apache-2.0")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_guard(value: Option<&str>) -> env_lock::EnvGuard<'_> {
        env_lock::lock_env([(CHANNEL_ENV, value)])
    }

    #[test]
    fn ga_version_parses_as_semver_one_dot_zero() {
        let v = SemVer::parse(GA_VERSION).expect("GA_VERSION must parse");
        assert_eq!(v.major, 1, "GA milestone major must be 1: {GA_VERSION}");
        assert_eq!(
            v,
            SemVer {
                major: 1,
                minor: 0,
                patch: 0
            }
        );
        assert_eq!(ga_semver(), v);
    }

    #[test]
    fn is_ga_true_for_one_dot_zero() {
        assert!(is_ga());
    }

    #[test]
    fn semver_parse_rejects_malformed() {
        assert!(SemVer::parse("1.0").is_none());
        assert!(SemVer::parse("1.0.0.0").is_none());
        assert!(SemVer::parse("1.0.x").is_none());
        assert!(SemVer::parse("").is_none());
    }

    #[test]
    fn semver_display_round_trips() {
        assert_eq!(ga_semver().to_string(), GA_VERSION);
    }

    #[test]
    fn resolve_channel_unset_is_stable() {
        let _guard = env_guard(None);
        assert_eq!(resolve_channel(), Channel::Stable);
    }

    #[test]
    fn resolve_channel_beta_and_nightly_map() {
        {
            let _guard = env_guard(Some("beta"));
            assert_eq!(resolve_channel(), Channel::Beta);
        }
        {
            let _guard = env_guard(Some("nightly"));
            assert_eq!(resolve_channel(), Channel::Nightly);
        }
        {
            let _guard = env_guard(Some("  NIGHTLY  "));
            assert_eq!(resolve_channel(), Channel::Nightly);
        }
    }

    #[test]
    fn resolve_channel_garbage_is_stable() {
        let _guard = env_guard(Some("garbage"));
        assert_eq!(resolve_channel(), Channel::Stable);
    }

    #[test]
    fn channel_default_is_stable() {
        assert_eq!(Channel::default(), Channel::Stable);
    }

    #[test]
    fn channel_as_str_round_trips() {
        for ch in [Channel::Stable, Channel::Beta, Channel::Nightly] {
            assert_eq!(Channel::parse(ch.as_str()), ch);
            assert_eq!(format!("{ch}"), ch.as_str());
        }
        // Case-insensitive parse with whitespace also round-trips to the slug.
        assert_eq!(Channel::parse(" Beta ").as_str(), "beta");
    }

    #[test]
    fn ga_banner_is_one_line_branded_and_clean() {
        let banner = ga_banner();
        assert!(!banner.contains('\n'), "banner must be one line: {banner}");
        assert!(banner.contains("1.0"), "banner missing 1.0: {banner}");
        assert!(
            banner.contains("BharatCode"),
            "banner missing product name: {banner}"
        );
        let lowered = banner.to_lowercase();
        assert!(
            !lowered.contains("goose"),
            "banner leaks upstream: {banner}"
        );
        assert!(!lowered.contains("block"), "banner leaks vendor: {banner}");
    }

    #[test]
    fn resolve_snapshots_identity() {
        let _guard = env_guard(Some("beta"));
        let r = resolve();
        assert_eq!(r.version, ga_semver());
        assert_eq!(r.channel, Channel::Beta);
        assert!(r.build.starts_with(env!("CARGO_PKG_VERSION")));
        assert!(!r.build.contains('\n'));
    }

    #[test]
    fn build_metadata_leads_with_crate_version_single_line() {
        let meta = build_metadata();
        assert!(meta.starts_with(env!("CARGO_PKG_VERSION")), "{meta}");
        assert!(!meta.contains('\n'), "{meta}");
    }
}
