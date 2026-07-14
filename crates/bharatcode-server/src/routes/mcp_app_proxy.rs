use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use uuid::Uuid;

const GUEST_HTML_TTL_SECS: u64 = 300; // 5 minutes
const GUEST_HTML_MAX_ENTRIES: usize = 64;

/// In-memory store for guest HTML content.
/// Maps nonce -> (html_content, csp_string, created_at)
/// Entries are consumed on first read and evicted after TTL.
type GuestHtmlStore = Arc<RwLock<HashMap<String, (String, String, Instant)>>>;

#[derive(Deserialize)]
struct ProxyQuery {
    secret: String,
    /// Comma-separated list of domains for connect-src (fetch, XHR, WebSocket)
    connect_domains: Option<String>,
    /// Comma-separated list of domains for resource loading (scripts, styles, images, fonts, media)
    resource_domains: Option<String>,
    /// Comma-separated list of origins for nested iframes (frame-src)
    frame_domains: Option<String>,
    /// Comma-separated list of allowed base URIs (base-uri)
    base_uri_domains: Option<String>,
    /// Comma-separated list of domains for script-src (external scripts like SDKs)
    script_domains: Option<String>,
}

/// The guest iframe is untrusted MCP app code, so its URL must not carry the server
/// secret: guest scripts can read their own `window.location`. The nonce is the only
/// credential here - an unguessable, single-use, TTL-bounded capability that is handed
/// out solely in response to a secret-authenticated `store_guest_html` call.
#[derive(Deserialize)]
struct GuestQuery {
    nonce: String,
}

#[derive(Deserialize)]
struct StoreGuestBody {
    secret: String,
    html: String,
    /// CSP string to apply to the guest page
    csp: Option<String>,
}

#[derive(Serialize)]
struct StoreGuestResponse {
    nonce: String,
}

const MCP_APP_PROXY_HTML: &str = include_str!("templates/mcp_app_proxy.html");

/// Build the outer sandbox CSP based on declared domains.
///
/// This CSP acts as a ceiling - the inner guest UI iframe cannot exceed these
/// permissions, even if it tried. This is the single source of truth for
/// security policy enforcement.
///
/// Every interpolated domain must already have passed [`normalize_csp_source`].
///
/// Based on the MCP Apps specification (ext-apps SEP):
/// <https://github.com/modelcontextprotocol/ext-apps/blob/main/specification/draft/apps.mdx>
fn build_outer_csp(
    connect_domains: &[String],
    resource_domains: &[String],
    frame_domains: &[String],
    base_uri_domains: &[String],
    script_domains: &[String],
) -> String {
    let resources = if resource_domains.is_empty() {
        String::new()
    } else {
        format!(" {}", resource_domains.join(" "))
    };

    let scripts = if script_domains.is_empty() {
        String::new()
    } else {
        format!(" {}", script_domains.join(" "))
    };

    let connections = if connect_domains.is_empty() {
        String::new()
    } else {
        format!(" {}", connect_domains.join(" "))
    };

    // frame-src needs 'self' so the proxy can load the guest iframe from /mcp-app-guest
    let frame_src = if frame_domains.is_empty() {
        "frame-src 'self'".to_string()
    } else {
        format!("frame-src 'self' {}", frame_domains.join(" "))
    };

    let base_uris = if base_uri_domains.is_empty() {
        String::new()
    } else {
        format!(" {}", base_uri_domains.join(" "))
    };

    format!(
        "default-src 'none'; \
         script-src 'self' 'unsafe-inline'{resources}{scripts}; \
         script-src-elem 'self' 'unsafe-inline'{resources}{scripts}; \
         style-src 'self' 'unsafe-inline'{resources}; \
         style-src-elem 'self' 'unsafe-inline'{resources}; \
         connect-src 'self'{connections}; \
         img-src 'self' data: blob:{resources}; \
         font-src 'self'{resources}; \
         media-src 'self' data: blob:{resources}; \
         {frame_src}; \
         object-src 'none'; \
         base-uri 'self'{base_uris}"
    )
}

/// Validate a single CSP source expression and normalize it to an origin.
///
/// Domains originate in MCP app metadata, which is attacker-controlled. A raw value
/// reaching the policy could end the directive (`;`), append itself to a directive it
/// was never meant for (whitespace), smuggle in a keyword such as `'unsafe-eval'`, or
/// close the `content="..."` attribute of the meta tag the policy is serialized into.
/// Only scheme-qualified origins and bare host sources survive; everything else is
/// dropped, which can only ever tighten the resulting policy.
///
/// Mirrors the validation in `bharatcode_core::acp::mcp_app_proxy`.
fn normalize_csp_source(source: &str) -> Option<String> {
    let source = source.trim();
    if source.is_empty()
        || source
            .chars()
            .any(|c| c.is_ascii_whitespace() || matches!(c, ';' | ',' | '"' | '\'' | '<' | '>'))
    {
        return None;
    }

    if let Some((scheme, rest)) = source.split_once("://") {
        let scheme = scheme.to_ascii_lowercase();
        if !matches!(scheme.as_str(), "http" | "https" | "ws" | "wss") {
            return None;
        }

        let authority = rest.split(['/', '?', '#']).next()?;
        if !is_valid_csp_host_source(authority) {
            return None;
        }

        return Some(format!("{scheme}://{}", authority.to_ascii_lowercase()));
    }

    if is_valid_csp_host_source(source) {
        return Some(source.to_ascii_lowercase());
    }

    None
}

/// A bare `*` is rejected: it would let an app opt out of the ceiling entirely.
fn is_valid_csp_host_source(source: &str) -> bool {
    if source.is_empty() || source == "*" || source.contains('@') {
        return false;
    }

    let (host, port) = split_host_and_port(source);
    if host.is_empty() {
        return false;
    }
    if port.is_some_and(|port| port.is_empty() || port.parse::<u16>().is_err()) {
        return false;
    }

    let host = host.strip_prefix("*.").unwrap_or(host);
    if host.eq_ignore_ascii_case("localhost")
        || host.parse::<std::net::Ipv4Addr>().is_ok()
        || host.parse::<std::net::Ipv6Addr>().is_ok()
    {
        return true;
    }

    host.contains('.')
        && host
            .split('.')
            .all(|label| is_valid_dns_label(label) && label != "*")
}

fn split_host_and_port(source: &str) -> (&str, Option<&str>) {
    if let Some(remainder) = source.strip_prefix('[') {
        if let Some((host, tail)) = remainder.split_once(']') {
            let port = tail.strip_prefix(':');
            return (host, port);
        }
    }

    match source.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => (host, Some(port)),
        _ => (source, None),
    }
}

fn is_valid_dns_label(label: &str) -> bool {
    !label.is_empty()
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Escape a value for interpolation into a double-quoted HTML attribute.
///
/// The policy is already source-validated, so this is a second, structural barrier
/// against attribute breakout rather than the primary defense. Single quotes are left
/// alone: they delimit every CSP keyword and cannot terminate a double-quoted attribute.
fn escape_html_attribute(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            _ => escaped.push(c),
        }
    }
    escaped
}

/// Parse comma-separated domains, dropping any that are not valid CSP sources
fn parse_domains(domains: Option<&String>) -> Vec<String> {
    domains
        .map(|d| d.split(',').filter_map(normalize_csp_source).collect())
        .unwrap_or_default()
}

#[derive(Clone)]
struct AppState {
    secret_key: String,
    guest_store: GuestHtmlStore,
}

#[utoipa::path(
    get,
    path = "/mcp-app-proxy",
    params(
        ("secret" = String, Query, description = "Secret key for authentication"),
        ("connect_domains" = Option<String>, Query, description = "Comma-separated domains for connect-src"),
        ("resource_domains" = Option<String>, Query, description = "Comma-separated domains for resource loading"),
        ("frame_domains" = Option<String>, Query, description = "Comma-separated origins for nested iframes (frame-src)"),
        ("base_uri_domains" = Option<String>, Query, description = "Comma-separated allowed base URIs (base-uri)"),
        ("script_domains" = Option<String>, Query, description = "Comma-separated domains for script-src")
    ),
    responses(
        (status = 200, description = "MCP App proxy HTML page", content_type = "text/html"),
        (status = 401, description = "Unauthorized - invalid or missing secret"),
    )
)]
async fn mcp_app_proxy(
    State(state): State<AppState>,
    Query(params): Query<ProxyQuery>,
) -> Response {
    if params.secret != state.secret_key {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let connect_domains = parse_domains(params.connect_domains.as_ref());
    let resource_domains = parse_domains(params.resource_domains.as_ref());
    let frame_domains = parse_domains(params.frame_domains.as_ref());
    let base_uri_domains = parse_domains(params.base_uri_domains.as_ref());
    let script_domains = parse_domains(params.script_domains.as_ref());

    let csp = build_outer_csp(
        &connect_domains,
        &resource_domains,
        &frame_domains,
        &base_uri_domains,
        &script_domains,
    );

    let html = MCP_APP_PROXY_HTML.replace("{{OUTER_CSP}}", &escape_html_attribute(&csp));

    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (
                header::HeaderName::from_static("referrer-policy"),
                "no-referrer",
            ),
        ],
        Html(html),
    )
        .into_response()
}

/// Store guest HTML and return a nonce for retrieval.
/// The proxy page calls this via fetch, then sets the guest iframe src to /mcp-app-guest?nonce=...
async fn store_guest_html(
    State(state): State<AppState>,
    Json(body): Json<StoreGuestBody>,
) -> Response {
    if body.secret != state.secret_key {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let nonce = Uuid::new_v4().to_string();
    let csp = body.csp.unwrap_or_default();

    {
        let mut store = state.guest_store.write().await;

        // Evict expired entries
        let cutoff = Instant::now() - std::time::Duration::from_secs(GUEST_HTML_TTL_SECS);
        store.retain(|_, (_, _, created)| *created > cutoff);

        // If still at capacity, drop the oldest entry
        if store.len() >= GUEST_HTML_MAX_ENTRIES {
            if let Some(oldest_key) = store
                .iter()
                .min_by_key(|(_, (_, _, created))| *created)
                .map(|(k, _)| k.clone())
            {
                store.remove(&oldest_key);
            }
        }

        store.insert(nonce.clone(), (body.html, csp, Instant::now()));
    }

    (StatusCode::OK, Json(StoreGuestResponse { nonce })).into_response()
}

/// Serve stored guest HTML with a real HTTPS URL.
/// This gives the guest iframe `window.location.protocol === "https:"`,
/// which is required by SDKs like Square Web Payments that check for secure context.
///
/// Authenticated by the single-use nonce alone - see [`GuestQuery`].
async fn serve_guest_html(
    State(state): State<AppState>,
    Query(params): Query<GuestQuery>,
) -> Response {
    // Consume the entry (one-time use)
    let entry = {
        let mut store = state.guest_store.write().await;
        let cutoff = Instant::now() - std::time::Duration::from_secs(GUEST_HTML_TTL_SECS);
        store.retain(|_, (_, _, created)| *created > cutoff);
        store.remove(&params.nonce)
    };

    match entry {
        Some((html, csp, _created)) => {
            let mut response = Html(html).into_response();
            let headers = response.headers_mut();
            // Use strict-origin so third-party SDKs (e.g. Square Web Payments)
            // receive the origin in their requests, which they need for auth.
            // no-referrer would cause 401s from SDK servers.
            headers.insert(
                header::HeaderName::from_static("referrer-policy"),
                header::HeaderValue::from_static("strict-origin"),
            );
            if !csp.is_empty() {
                match csp.parse::<header::HeaderValue>() {
                    Ok(csp_value) => {
                        headers.insert(header::CONTENT_SECURITY_POLICY, csp_value);
                    }
                    Err(_) => {
                        return (
                            StatusCode::BAD_REQUEST,
                            "Invalid characters in Content-Security-Policy value",
                        )
                            .into_response();
                    }
                }
            }
            response
        }
        None => (
            StatusCode::NOT_FOUND,
            "Guest content not found or already consumed",
        )
            .into_response(),
    }
}

pub fn routes(secret_key: String) -> Router {
    let state = AppState {
        secret_key,
        guest_store: Arc::new(RwLock::new(HashMap::new())),
    };

    Router::new()
        .route("/mcp-app-proxy", get(mcp_app_proxy))
        .route("/mcp-app-guest", get(serve_guest_html))
        .route("/mcp-app-guest", post(store_guest_html))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "super-secret-key";

    fn test_state() -> AppState {
        AppState {
            secret_key: SECRET.to_string(),
            guest_store: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn body_string(response: Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    async fn store(state: &AppState, html: &str, csp: Option<&str>) -> String {
        let response = store_guest_html(
            State(state.clone()),
            Json(StoreGuestBody {
                secret: SECRET.to_string(),
                html: html.to_string(),
                csp: csp.map(str::to_string),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let body = body_string(response).await;
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        json["nonce"].as_str().unwrap().to_string()
    }

    #[test]
    fn normalizes_valid_csp_sources_to_origins() {
        assert_eq!(
            normalize_csp_source("https://cdn.example.com/assets/app.js"),
            Some("https://cdn.example.com".to_string())
        );
        assert_eq!(
            normalize_csp_source("wss://api.example.com/socket"),
            Some("wss://api.example.com".to_string())
        );
        assert_eq!(
            normalize_csp_source("https://*.squarecdn.com"),
            Some("https://*.squarecdn.com".to_string())
        );
        assert_eq!(
            normalize_csp_source("localhost:3000"),
            Some("localhost:3000".to_string())
        );
    }

    #[test]
    fn rejects_csp_sources_that_could_alter_the_policy() {
        // Directive termination / injection of a new directive
        assert_eq!(normalize_csp_source("example.com; script-src *"), None);
        assert_eq!(normalize_csp_source("example.com;"), None);
        // Whitespace smuggles an extra source into the current directive
        assert_eq!(normalize_csp_source("example.com evil.com"), None);
        // CSP keywords must never come from app metadata
        assert_eq!(normalize_csp_source("'unsafe-eval'"), None);
        assert_eq!(normalize_csp_source("'none'"), None);
        // Wildcards and dangerous schemes defeat the ceiling
        assert_eq!(normalize_csp_source("*"), None);
        assert_eq!(normalize_csp_source("javascript:alert(1)"), None);
        assert_eq!(normalize_csp_source("data:"), None);
        // Attribute breakout attempts
        assert_eq!(normalize_csp_source("evil.com\" onload=\"alert(1)"), None);
        assert_eq!(normalize_csp_source("evil.com\"><script>"), None);
        // Malformed hosts
        assert_eq!(normalize_csp_source("https://user@example.com"), None);
        assert_eq!(normalize_csp_source("example.com:notaport"), None);
        assert_eq!(normalize_csp_source(""), None);
    }

    #[test]
    fn parse_domains_drops_invalid_sources() {
        let domains =
            "https://cdn.example.com/app.js, *, evil.com\" onload=\"x, api.example.com".to_string();

        assert_eq!(
            parse_domains(Some(&domains)),
            vec![
                "https://cdn.example.com".to_string(),
                "api.example.com".to_string(),
            ]
        );
    }

    #[test]
    fn escapes_html_attribute_delimiters() {
        assert_eq!(
            escape_html_attribute("a\"b<c>d&e"),
            "a&quot;b&lt;c&gt;d&amp;e"
        );
        // CSP keywords must survive verbatim
        assert_eq!(
            escape_html_attribute("script-src 'self'"),
            "script-src 'self'"
        );
    }

    #[tokio::test]
    async fn proxy_html_cannot_be_injected_through_domain_params() {
        let response = mcp_app_proxy(
            State(test_state()),
            Query(ProxyQuery {
                secret: SECRET.to_string(),
                connect_domains: Some("evil.com\" onload=\"alert(1)".to_string()),
                resource_domains: Some("x; script-src 'unsafe-eval'".to_string()),
                frame_domains: Some("*".to_string()),
                base_uri_domains: Some("javascript:alert(1)".to_string()),
                script_domains: Some("'unsafe-eval'".to_string()),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let body = body_string(response).await;
        assert!(!body.contains("onload"));
        assert!(!body.contains("unsafe-eval"));
        assert!(!body.contains("javascript:"));
        // Hostile sources are dropped, leaving the default ceiling intact
        assert!(body.contains("content=\"default-src 'none'; "));
        assert!(body.contains("frame-src 'self'; "));
    }

    #[tokio::test]
    async fn proxy_html_keeps_valid_domains() {
        let response = mcp_app_proxy(
            State(test_state()),
            Query(ProxyQuery {
                secret: SECRET.to_string(),
                connect_domains: Some("https://connect.squareup.com".to_string()),
                resource_domains: None,
                frame_domains: None,
                base_uri_domains: None,
                script_domains: Some("https://*.squarecdn.com".to_string()),
            }),
        )
        .await;

        let body = body_string(response).await;
        assert!(body.contains("connect-src 'self' https://connect.squareup.com;"));
        assert!(body.contains("script-src 'self' 'unsafe-inline' https://*.squarecdn.com;"));
    }

    #[tokio::test]
    async fn proxy_rejects_wrong_secret() {
        let response = mcp_app_proxy(
            State(test_state()),
            Query(ProxyQuery {
                secret: "wrong".to_string(),
                connect_domains: None,
                resource_domains: None,
                frame_domains: None,
                base_uri_domains: None,
                script_domains: None,
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn guest_html_is_served_by_nonce_alone_and_consumed() {
        let state = test_state();
        let nonce = store(&state, "<p>guest</p>", Some("default-src 'none'")).await;

        let response = serve_guest_html(
            State(state.clone()),
            Query(GuestQuery {
                nonce: nonce.clone(),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_SECURITY_POLICY)
                .unwrap(),
            "default-src 'none'"
        );
        assert_eq!(body_string(response).await, "<p>guest</p>");

        let response = serve_guest_html(State(state), Query(GuestQuery { nonce })).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn guest_html_rejects_unknown_nonce() {
        let response = serve_guest_html(
            State(test_state()),
            Query(GuestQuery {
                nonce: Uuid::new_v4().to_string(),
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn store_guest_html_rejects_wrong_secret() {
        let response = store_guest_html(
            State(test_state()),
            Json(StoreGuestBody {
                secret: "wrong".to_string(),
                html: "<p>guest</p>".to_string(),
                csp: None,
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn guest_csp_with_control_characters_is_rejected() {
        let state = test_state();
        let nonce = store(
            &state,
            "<p>guest</p>",
            Some("default-src 'none'\r\nX-Evil: 1"),
        )
        .await;

        let response = serve_guest_html(State(state), Query(GuestQuery { nonce })).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    /// Every sandbox token list the template applies to the guest iframe.
    fn guest_sandbox_attributes() -> Vec<&'static str> {
        MCP_APP_PROXY_HTML
            .split("setAttribute('sandbox', '")
            .skip(1)
            .map(|value| {
                value
                    .split('\'')
                    .next()
                    .expect("unterminated sandbox attribute")
            })
            .collect()
    }

    /// The guest document is untrusted app code. These template invariants keep the server
    /// secret out of its reach; both are trivial to regress with a one-line edit.
    #[test]
    fn guest_iframe_is_isolated_from_the_proxy_origin() {
        let sandboxes = guest_sandbox_attributes();
        assert_eq!(sandboxes, vec!["allow-scripts allow-forms"]);
        assert!(
            !sandboxes
                .iter()
                .any(|sandbox| sandbox.contains("allow-same-origin")),
            "guest iframe must not be same-origin with the proxy: it could read the \
             secret from the proxy URL and drive the authenticated REST API"
        );
    }

    #[test]
    fn guest_iframe_url_never_carries_the_secret() {
        assert!(MCP_APP_PROXY_HTML.contains("'/mcp-app-guest?nonce='"));
        assert!(
            !MCP_APP_PROXY_HTML.contains("mcp-app-guest?secret="),
            "the guest URL is readable by guest scripts and must stay secret-free"
        );
        assert!(
            !MCP_APP_PROXY_HTML.contains(".srcdoc"),
            "an about:srcdoc guest inherits the proxy URL (which carries the secret) as \
             its document.referrer; the guest must always load from the nonce URL"
        );
    }
}
