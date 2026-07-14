use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
pub use bharatcode_core::acp::transport::auth::check_acp_token;
use bharatcode_core::acp::transport::auth::token_matches;

/// A blank secret would otherwise authenticate any request carrying an empty
/// `X-Secret-Key` header, so an unusable secret rejects everything instead.
fn secret_matches(candidate: Option<&str>, expected: &str) -> bool {
    !expected.trim().is_empty() && token_matches(candidate, expected)
}

pub async fn check_token(
    State(state): State<String>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if request.uri().path() == "/status"
        || request.uri().path() == "/features"
        || request.uri().path() == "/mcp-ui-proxy"
        || request.uri().path() == "/mcp-app-proxy"
        || request.uri().path() == "/mcp-app-guest"
    {
        return Ok(next.run(request).await);
    }
    let secret_key = request
        .headers()
        .get("X-Secret-Key")
        .and_then(|value| value.to_str().ok());

    if secret_matches(secret_key, &state) {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

#[cfg(test)]
mod tests {
    use super::secret_matches;

    #[test]
    fn accepts_only_the_exact_secret() {
        assert!(secret_matches(Some("s3cret"), "s3cret"));
        assert!(!secret_matches(Some("s3cre"), "s3cret"));
        assert!(!secret_matches(Some("s3cret1"), "s3cret"));
        assert!(!secret_matches(Some(""), "s3cret"));
        assert!(!secret_matches(None, "s3cret"));
    }

    #[test]
    fn blank_configured_secret_never_authenticates() {
        for expected in ["", " ", "\t\n"] {
            assert!(!secret_matches(Some(""), expected));
            assert!(!secret_matches(Some(expected), expected));
            assert!(!secret_matches(Some("anything"), expected));
            assert!(!secret_matches(None, expected));
        }
    }
}
