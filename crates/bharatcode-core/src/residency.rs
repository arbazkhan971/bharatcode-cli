//! Data-residency egress guard (BharatCode v13).
//!
//! Optional guard that inspects the endpoint host a provider is about to talk
//! to and, depending on the configured mode, either allows it silently, logs a
//! warning, or blocks the request entirely. This supports DPDP-style data
//! sovereignty requirements where operators want to keep model traffic inside
//! an approved set of hosts (for example an in-country gateway).
//!
//! The guard is fully opt-in and defaults to [`ResidencyMode::Off`], so the
//! default behaviour is unchanged. It is controlled by two configuration
//! values (each readable from an environment variable of the same name in
//! upper case, or from the on-disk config):
//!
//! - `BHARATCODE_RESIDENCY` — `off` (default), `warn`, or `strict`.
//! - `BHARATCODE_RESIDENCY_ALLOWLIST` — comma/whitespace separated list of
//!   permitted hostnames. Loopback hosts (`localhost`, `127.0.0.1`, `::1`) are
//!   always allowed so local inference is never blocked.
//!
//! Allowlist entries are matched case-insensitively against the endpoint host.
//! An entry also matches subdomains, so `example.com` permits
//! `api.example.com`. A leading `*.` wildcard (`*.example.com`) is treated the
//! same as the bare suffix form.

use crate::config::Config;

/// Config / environment key selecting the residency mode.
pub const RESIDENCY_MODE_KEY: &str = "BHARATCODE_RESIDENCY";
/// Config / environment key holding the host allowlist.
pub const RESIDENCY_ALLOWLIST_KEY: &str = "BHARATCODE_RESIDENCY_ALLOWLIST";

/// Hosts that are always permitted regardless of the configured allowlist so
/// that local inference and loopback endpoints are never blocked.
const ALWAYS_ALLOWED: &[&str] = &["localhost", "127.0.0.1", "::1", "[::1]"];

/// How the data-residency guard reacts to a non-allowlisted endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResidencyMode {
    /// Guard disabled — every endpoint is permitted (default).
    #[default]
    Off,
    /// Permit every endpoint but log a warning for non-allowlisted hosts.
    Warn,
    /// Block (error) on any non-allowlisted host.
    Strict,
}

impl ResidencyMode {
    /// Parse a mode from a human-supplied string. Unknown / empty values fall
    /// back to [`ResidencyMode::Off`] so a typo never silently enables blocking.
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "warn" | "warning" => ResidencyMode::Warn,
            "strict" | "block" | "on" | "enforce" => ResidencyMode::Strict,
            _ => ResidencyMode::Off,
        }
    }
}

/// Resolve the configured residency mode (defaults to [`ResidencyMode::Off`]).
pub fn residency_mode() -> ResidencyMode {
    Config::global()
        .get_param::<String>(RESIDENCY_MODE_KEY)
        .ok()
        .map(|raw| ResidencyMode::parse(&raw))
        .unwrap_or_default()
}

/// Resolve the configured host allowlist (excluding the always-allowed loopback
/// hosts, which are handled separately in [`host_is_allowed`]).
fn configured_allowlist() -> Vec<String> {
    Config::global()
        .get_param::<String>(RESIDENCY_ALLOWLIST_KEY)
        .ok()
        .map(|raw| parse_allowlist(&raw))
        .unwrap_or_default()
}

/// Split a raw allowlist string on commas / whitespace into normalized hosts.
fn parse_allowlist(raw: &str) -> Vec<String> {
    raw.split(|c: char| c == ',' || c.is_whitespace())
        .map(normalize_host_entry)
        .filter(|s| !s.is_empty())
        .collect()
}

/// Normalize an allowlist entry to a bare lowercase host, dropping any scheme,
/// path, port, or leading `*.` wildcard so matching is forgiving of how the
/// operator wrote it.
fn normalize_host_entry(entry: &str) -> String {
    let mut host = entry.trim();
    if let Some((_, after_scheme)) = host.split_once("://") {
        host = after_scheme;
    }
    // Drop any path / query suffix.
    if let Some((before_path, _)) = host.split_once('/') {
        host = before_path;
    }
    // Drop a trailing :port (but keep IPv6 brackets intact).
    if !host.starts_with('[') {
        if let Some((before_port, _)) = host.rsplit_once(':') {
            host = before_port;
        }
    }
    host.trim_start_matches("*.")
        .trim_matches('.')
        .to_ascii_lowercase()
}

/// Extract the bare lowercase host from an endpoint string that may be a full
/// URL (`https://api.example.com/v1`) or a bare host (`api.example.com:443`).
fn endpoint_host(endpoint: &str) -> Option<String> {
    let trimmed = endpoint.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Prefer a real URL parse when a scheme is present.
    if let Ok(url) = url::Url::parse(trimmed) {
        if let Some(host) = url.host_str() {
            return Some(host.to_ascii_lowercase());
        }
    }
    let host = normalize_host_entry(trimmed);
    (!host.is_empty()).then_some(host)
}

/// Returns true when `host` is the always-allowed loopback set or matches an
/// allowlist entry exactly or as a parent domain suffix.
fn host_is_allowed(host: &str, allowlist: &[String]) -> bool {
    let host = host.trim_matches(|c| c == '.').to_ascii_lowercase();
    if ALWAYS_ALLOWED.iter().any(|h| *h == host) {
        return true;
    }
    allowlist
        .iter()
        .any(|entry| host == *entry || host.ends_with(format!(".{entry}").as_str()))
}

/// Guard a provider endpoint before a request is set up.
///
/// `endpoint` may be a full base URL or a bare host. Behaviour depends on the
/// configured [`ResidencyMode`]:
///
/// - [`ResidencyMode::Off`]: always `Ok(())` (default, no behaviour change).
/// - [`ResidencyMode::Warn`]: always `Ok(())`, but logs a warning for hosts
///   that are not on the allowlist.
/// - [`ResidencyMode::Strict`]: returns an error for any host not on the
///   allowlist, with a clear data-residency message.
pub fn guard_endpoint(endpoint: &str) -> anyhow::Result<()> {
    guard_endpoint_with_mode(endpoint, residency_mode())
}

/// Guard a provider endpoint against an explicitly supplied residency mode.
///
/// Identical to [`guard_endpoint`] but takes the mode as a parameter so a caller
/// can enforce an *effective* mode that composes other policy (for example
/// offline mode forcing [`ResidencyMode::Strict`]) without mutating the stored
/// residency setting.
pub fn guard_endpoint_with_mode(endpoint: &str, mode: ResidencyMode) -> anyhow::Result<()> {
    if mode == ResidencyMode::Off {
        return Ok(());
    }

    let allowlist = configured_allowlist();
    let host = match endpoint_host(endpoint) {
        Some(host) => host,
        // If we cannot determine a host we conservatively allow in warn mode
        // and report it in strict mode rather than guessing.
        None => {
            if mode == ResidencyMode::Strict {
                anyhow::bail!(
                    "Data-residency guard (strict): could not determine the endpoint host \
                     from '{endpoint}'. Set {RESIDENCY_ALLOWLIST_KEY} or {RESIDENCY_MODE_KEY}=off."
                );
            }
            return Ok(());
        }
    };

    if host_is_allowed(&host, &allowlist) {
        return Ok(());
    }

    match mode {
        ResidencyMode::Warn => {
            tracing::warn!(
                target: "residency",
                host = %host,
                "Data-residency guard (warn): endpoint host '{host}' is not on the \
                 allowlist ({RESIDENCY_ALLOWLIST_KEY}). Allowing because mode is 'warn'."
            );
            Ok(())
        }
        ResidencyMode::Strict => anyhow::bail!(
            "Data-residency guard (strict): endpoint host '{host}' is not on the allowlist. \
             Add it to {RESIDENCY_ALLOWLIST_KEY} (comma separated), or set \
             {RESIDENCY_MODE_KEY}=warn or {RESIDENCY_MODE_KEY}=off to permit it."
        ),
        ResidencyMode::Off => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mode_defaults_to_off() {
        assert_eq!(ResidencyMode::parse(""), ResidencyMode::Off);
        assert_eq!(ResidencyMode::parse("nonsense"), ResidencyMode::Off);
        assert_eq!(ResidencyMode::parse("off"), ResidencyMode::Off);
    }

    #[test]
    fn parse_mode_recognizes_warn_and_strict() {
        assert_eq!(ResidencyMode::parse("WARN"), ResidencyMode::Warn);
        assert_eq!(ResidencyMode::parse(" warning "), ResidencyMode::Warn);
        assert_eq!(ResidencyMode::parse("strict"), ResidencyMode::Strict);
        assert_eq!(ResidencyMode::parse("Block"), ResidencyMode::Strict);
    }

    #[test]
    fn normalize_strips_scheme_path_and_port() {
        assert_eq!(
            normalize_host_entry("https://api.example.com/v1"),
            "api.example.com"
        );
        assert_eq!(normalize_host_entry("Example.COM:443"), "example.com");
        assert_eq!(normalize_host_entry("*.example.com"), "example.com");
        assert_eq!(normalize_host_entry("  foo.io  "), "foo.io");
    }

    #[test]
    fn endpoint_host_handles_urls_and_bare_hosts() {
        assert_eq!(
            endpoint_host("https://api.openai.com/v1").as_deref(),
            Some("api.openai.com")
        );
        assert_eq!(
            endpoint_host("localhost:1234").as_deref(),
            Some("localhost")
        );
        assert_eq!(endpoint_host("").as_deref(), None);
    }

    #[test]
    fn loopback_always_allowed() {
        assert!(host_is_allowed("localhost", &[]));
        assert!(host_is_allowed("127.0.0.1", &[]));
        assert!(host_is_allowed("::1", &[]));
    }

    #[test]
    fn allowlist_matches_exact_and_subdomain() {
        let allow = vec!["example.com".to_string()];
        assert!(host_is_allowed("example.com", &allow));
        assert!(host_is_allowed("api.example.com", &allow));
        assert!(!host_is_allowed("notexample.com", &allow));
        assert!(!host_is_allowed("example.com.evil.com", &allow));
    }
}
