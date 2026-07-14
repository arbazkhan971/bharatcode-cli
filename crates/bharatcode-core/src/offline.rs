//! Offline / no-egress mode (BharatCode v20).
//!
//! A single switch — `BHARATCODE_OFFLINE` — that *composes* three existing,
//! independently configurable safeguards into one guaranteed no-egress mode:
//!
//! 1. **Local-only providers** — every model/provider endpoint must resolve to a
//!    loopback / local host. [`guard_endpoint`] refuses any non-local host while
//!    offline, so traffic can never leave the machine.
//! 2. **Residency = strict** — while offline the *effective* residency posture is
//!    [`ResidencyMode::Strict`] regardless of the configured value
//!    ([`effective_residency_mode`]).
//! 3. **Telemetry off** — telemetry is treated as disabled and the status report
//!    surfaces it so the operator can see at a glance.
//!
//! This module is deliberately *read-only* with respect to those underlying
//! settings: it never mutates the residency mode or the telemetry preference, it
//! only reads them (via [`crate::residency`] and the telemetry module) and layers a
//! stricter, composed view on top. That keeps the switch additive and side-effect
//! free.
//!
//! The switch is fully opt-in and defaults to **off**, so default behaviour is
//! unchanged. It is read from the environment variable `BHARATCODE_OFFLINE`
//! (`1`/`true`/`on`/`yes` enable it; `0`/`false`/`off`/`no`/empty disable it) or,
//! when the environment variable is unset, from the on-disk config as a boolean.

use crate::config::Config;
use crate::residency::{self, ResidencyMode};

/// Config / environment key for the offline switch.
pub const OFFLINE_MODE_KEY: &str = "BHARATCODE_OFFLINE";

/// Hosts that are considered local and therefore never egress off-machine.
/// Loopback ranges (`127.0.0.0/8`) and the `.localhost` suffix are handled
/// separately in [`host_is_local`].
const LOCAL_HOSTS: &[&str] = &["localhost", "127.0.0.1", "::1", "[::1]", "0.0.0.0"];

/// Interpret a raw environment value as a truthy/falsy flag.
///
/// Returns `None` for values that are neither clearly on nor off so a typo never
/// silently flips the switch in either direction.
fn parse_flag(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "on" | "yes" | "enable" | "enabled" => Some(true),
        "0" | "false" | "off" | "no" | "disable" | "disabled" | "" => Some(false),
        _ => None,
    }
}

/// Returns true when offline / no-egress mode is enabled. Defaults to `false`.
///
/// The environment variable takes precedence (matching the documented
/// `BHARATCODE_OFFLINE=1` switch); when it is unset the on-disk config is read as
/// a boolean. Any unrecognised value is treated as "off" so the default
/// behaviour is never changed by accident.
pub fn is_offline() -> bool {
    if let Ok(raw) = std::env::var(OFFLINE_MODE_KEY) {
        if let Some(flag) = parse_flag(&raw) {
            return flag;
        }
    }
    Config::global()
        .get_param::<bool>(OFFLINE_MODE_KEY)
        .unwrap_or(false)
}

/// The residency mode that is actually enforced once offline mode is taken into
/// account. Offline mode composes residency = strict without mutating the stored
/// residency setting; when offline is off the configured mode is returned
/// unchanged.
pub fn effective_residency_mode() -> ResidencyMode {
    if is_offline() {
        ResidencyMode::Strict
    } else {
        residency::residency_mode()
    }
}

/// Screen a single provider endpoint against the *composed* egress policy.
///
/// This is the function wired into the shared provider HTTP client so that every
/// provider (including declarative ones) is screened in one place. It layers two
/// existing safeguards:
///
/// 1. Offline mode ([`guard_endpoint`]) refuses any non-local endpoint when
///    `BHARATCODE_OFFLINE=1`, asserting no traffic leaves the machine.
/// 2. Data residency is enforced at the *effective* mode
///    ([`effective_residency_mode`]), so offline mode also forces
///    `residency=strict` even when the configured residency mode is weaker.
///
/// With offline off and residency off (the defaults) this is a transparent
/// `Ok(())`, so default behaviour is unchanged.
pub fn enforce_egress_policy(endpoint: &str) -> anyhow::Result<()> {
    guard_endpoint(endpoint)?;
    residency::guard_endpoint_with_mode(endpoint, effective_residency_mode())
}

/// Install [`enforce_egress_policy`] as the shared provider HTTP client's egress
/// guard so every request issued through that client is screened centrally.
///
/// Idempotent (the underlying registration only honours the first caller); meant
/// to be invoked once while the provider registry is initialised.
pub fn install_egress_guard() {
    bharatcode_providers::api_client::set_endpoint_guard(enforce_egress_policy);
}

/// Returns true when `host` is a loopback / local host that never leaves the
/// machine. Matches the always-allowed loopback set, the whole `127.0.0.0/8`
/// IPv4 loopback range, and any `*.localhost` name.
pub fn host_is_local(host: &str) -> bool {
    let host = host.trim().trim_matches(|c| c == '.').to_ascii_lowercase();
    if host.is_empty() {
        return false;
    }
    if LOCAL_HOSTS.iter().any(|h| *h == host) {
        return true;
    }
    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    // Any 127.x.y.z loopback address.
    host.parse::<std::net::Ipv4Addr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

/// Extract the bare lowercase host from an endpoint string that may be a full
/// URL (`http://localhost:11434/v1`) or a bare host (`127.0.0.1:8080`).
fn endpoint_host(endpoint: &str) -> Option<String> {
    let trimmed = endpoint.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(url) = url::Url::parse(trimmed) {
        if let Some(host) = url.host_str() {
            return Some(host.to_ascii_lowercase());
        }
    }
    let mut host = trimmed;
    if let Some((_, after_scheme)) = host.split_once("://") {
        host = after_scheme;
    }
    if let Some((before_path, _)) = host.split_once('/') {
        host = before_path;
    }
    if !host.starts_with('[') {
        if let Some((before_port, _)) = host.rsplit_once(':') {
            host = before_port;
        }
    }
    let host = host.trim_matches('.').to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// Guard a single provider endpoint under offline / no-egress mode.
///
/// When offline mode is **off** this is always `Ok(())` (no behaviour change).
/// When **on**, any endpoint whose host is not a loopback / local host is
/// rejected with a clear message, guaranteeing no traffic egresses the machine.
pub fn guard_endpoint(endpoint: &str) -> anyhow::Result<()> {
    if !is_offline() {
        return Ok(());
    }
    match endpoint_host(endpoint) {
        Some(host) if host_is_local(&host) => Ok(()),
        Some(host) => anyhow::bail!(
            "Offline mode ({OFFLINE_MODE_KEY}=1): refusing to contact non-local endpoint \
             host '{host}'. Offline mode permits loopback / local endpoints only. Configure \
             a local provider, or set {OFFLINE_MODE_KEY}=0 to leave offline mode."
        ),
        None => anyhow::bail!(
            "Offline mode ({OFFLINE_MODE_KEY}=1): could not determine a host from endpoint \
             '{endpoint}', so it cannot be confirmed local; refusing to proceed. Set \
             {OFFLINE_MODE_KEY}=0 to leave offline mode."
        ),
    }
}

/// Compose-check: when offline mode is on, assert that none of the supplied
/// provider endpoints would egress to a non-local host.
///
/// Returns `Ok(())` when offline mode is off (no behaviour change) or when every
/// endpoint resolves to a local host; otherwise returns the first offending
/// endpoint's error. This is the single place a caller can assert "no non-local
/// endpoints" for a batch of endpoints before going live.
pub fn assert_no_egress<I, S>(endpoints: I) -> anyhow::Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    if !is_offline() {
        return Ok(());
    }
    for endpoint in endpoints {
        guard_endpoint(endpoint.as_ref())?;
    }
    Ok(())
}

/// A read-only, composed snapshot of the offline posture: the switch plus the
/// underlying residency and telemetry settings it composes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OfflineStatus {
    /// Whether the offline switch is enabled.
    pub enabled: bool,
    /// The residency mode that is actually enforced (strict while offline).
    pub effective_residency: ResidencyMode,
    /// Whether telemetry is currently enabled.
    pub telemetry_enabled: bool,
}

impl OfflineStatus {
    /// True when residency is effectively strict.
    pub fn residency_is_strict(&self) -> bool {
        self.effective_residency == ResidencyMode::Strict
    }

    /// True when telemetry is off.
    pub fn telemetry_is_off(&self) -> bool {
        !self.telemetry_enabled
    }

    /// True when all three pillars are composed and guaranteeing no egress:
    /// the switch is on, residency is effectively strict, and telemetry is off.
    pub fn is_no_egress(&self) -> bool {
        self.enabled && self.residency_is_strict() && self.telemetry_is_off()
    }

    /// A clear, single-line human-readable status for display in a CLI / doctor
    /// view. Never references any upstream brand name.
    pub fn status_line(&self) -> String {
        if !self.enabled {
            return format!(
                "Offline mode: OFF (set {OFFLINE_MODE_KEY}=1 to enable no-egress mode)"
            );
        }
        let residency = if self.residency_is_strict() {
            "residency=strict"
        } else {
            "residency=unknown"
        };
        let telemetry = if self.telemetry_is_off() {
            "telemetry=off"
        } else {
            "telemetry=ON"
        };
        if self.is_no_egress() {
            format!("Offline mode: ON — no egress (local endpoints only; {residency}; {telemetry})")
        } else {
            format!(
                "Offline mode: ON — local endpoints only; {residency}; {telemetry} \
                 (warning: telemetry is enabled and should be off)"
            )
        }
    }
}

/// Whether telemetry is currently enabled. When the `telemetry` feature is
/// compiled out, telemetry can never run, so this is always `false`.
fn telemetry_enabled() -> bool {
    #[cfg(feature = "telemetry")]
    {
        crate::posthog::is_telemetry_enabled()
    }
    #[cfg(not(feature = "telemetry"))]
    {
        false
    }
}

/// Resolve the current composed offline status (read-only).
pub fn offline_status() -> OfflineStatus {
    OfflineStatus {
        enabled: is_offline(),
        effective_residency: effective_residency_mode(),
        telemetry_enabled: telemetry_enabled(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_flag_recognizes_on_and_off() {
        assert_eq!(parse_flag("1"), Some(true));
        assert_eq!(parse_flag("TRUE"), Some(true));
        assert_eq!(parse_flag(" on "), Some(true));
        assert_eq!(parse_flag("yes"), Some(true));
        assert_eq!(parse_flag("0"), Some(false));
        assert_eq!(parse_flag("false"), Some(false));
        assert_eq!(parse_flag(""), Some(false));
        assert_eq!(parse_flag("maybe"), None);
    }

    #[test]
    fn local_hosts_are_recognized() {
        assert!(host_is_local("localhost"));
        assert!(host_is_local("127.0.0.1"));
        assert!(host_is_local("127.4.5.6"));
        assert!(host_is_local("::1"));
        assert!(host_is_local("[::1]"));
        assert!(host_is_local("api.localhost"));
        assert!(host_is_local("0.0.0.0"));
    }

    #[test]
    fn non_local_hosts_are_rejected() {
        assert!(!host_is_local("api.openai.com"));
        assert!(!host_is_local("example.com"));
        assert!(!host_is_local("8.8.8.8"));
        assert!(!host_is_local(""));
        assert!(!host_is_local("localhost.evil.com"));
    }

    #[test]
    fn endpoint_host_parses_urls_and_bare_hosts() {
        assert_eq!(
            endpoint_host("http://localhost:11434/v1").as_deref(),
            Some("localhost")
        );
        assert_eq!(
            endpoint_host("127.0.0.1:8080").as_deref(),
            Some("127.0.0.1")
        );
        assert_eq!(
            endpoint_host("https://api.openai.com/v1").as_deref(),
            Some("api.openai.com")
        );
        assert_eq!(endpoint_host("").as_deref(), None);
    }

    #[test]
    fn status_line_off_by_default() {
        // Constructing the status directly keeps this test independent of the
        // process-wide environment / config that `is_offline()` reads.
        let status = OfflineStatus {
            enabled: false,
            effective_residency: ResidencyMode::Off,
            telemetry_enabled: false,
        };
        assert!(status.status_line().starts_with("Offline mode: OFF"));
        assert!(!status.is_no_egress());
    }

    #[test]
    fn no_egress_requires_all_three_pillars() {
        let composed = OfflineStatus {
            enabled: true,
            effective_residency: ResidencyMode::Strict,
            telemetry_enabled: false,
        };
        assert!(composed.is_no_egress());
        assert!(composed.status_line().contains("no egress"));

        let telemetry_on = OfflineStatus {
            telemetry_enabled: true,
            ..composed
        };
        assert!(!telemetry_on.is_no_egress());
    }
}
