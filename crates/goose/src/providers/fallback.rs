//! Opt-in model fallback chain.
//!
//! When the primary provider's stream fails with a *retryable* error
//! (rate limit / server error / "overloaded"), the same request can be
//! transparently retried against the next model listed in the
//! `BHARATCODE_FALLBACK_MODELS` environment variable, in order, before the
//! error is surfaced to the caller.
//!
//! The chain is a comma-separated list of entries. Each entry is either a
//! fully-qualified `provider/model` pair or a bare `model` name (in which case
//! the current provider is reused). Empty/unset leaves behaviour unchanged:
//! exactly one attempt against the primary provider.
//!
//! This module is intentionally a thin helper around the real streaming path
//! (see [`crate::agents::reply_parts::Agent::stream_response_from_provider`]),
//! so that it is genuinely exercised by the running agent loop rather than
//! being dead, default-off code.

use goose_providers::errors::ProviderError;

/// Environment variable holding the comma-separated fallback chain.
const ENV_VAR: &str = "BHARATCODE_FALLBACK_MODELS";

/// A single parsed fallback target.
///
/// `provider` is `None` when the entry was a bare model name, meaning the
/// primary provider should be reused with the given model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackTarget {
    pub provider: Option<String>,
    pub model: String,
}

/// Parse a fallback chain from the `BHARATCODE_FALLBACK_MODELS` environment
/// variable. Returns an empty vector when unset or empty (fallback disabled).
pub fn fallback_chain_from_env() -> Vec<FallbackTarget> {
    match std::env::var(ENV_VAR) {
        Ok(raw) => parse_fallback_chain(&raw),
        Err(_) => Vec::new(),
    }
}

/// Parse a comma-separated fallback chain.
///
/// Each non-empty entry is split on the first `/` into a `provider/model`
/// pair; an entry with no `/` is treated as a bare model name (current
/// provider reused). Surrounding whitespace is trimmed and blank entries are
/// skipped, so values like `", anthropic/claude , gpt-4o ,"` parse cleanly.
pub fn parse_fallback_chain(raw: &str) -> Vec<FallbackTarget> {
    raw.split(',')
        .filter_map(|entry| {
            let entry = entry.trim();
            if entry.is_empty() {
                return None;
            }
            match entry.split_once('/') {
                Some((provider, model)) => {
                    let provider = provider.trim();
                    let model = model.trim();
                    if model.is_empty() {
                        return None;
                    }
                    Some(FallbackTarget {
                        provider: (!provider.is_empty()).then(|| provider.to_string()),
                        model: model.to_string(),
                    })
                }
                None => Some(FallbackTarget {
                    provider: None,
                    model: entry.to_string(),
                }),
            }
        })
        .collect()
}

/// Classify whether an error is worth retrying against a fallback model.
///
/// Worthy: rate limits, server errors, and "overloaded"-style transient
/// failures. NOT worthy: authentication and context-length errors — those
/// will not be fixed by retrying the same request elsewhere, so we surface
/// them immediately.
pub fn is_fallback_worthy(error: &ProviderError) -> bool {
    match error {
        ProviderError::RateLimitExceeded { .. } => true,
        ProviderError::ServerError(_) => true,
        // Some providers surface overload/transient conditions as a generic
        // request failure; treat the well-known "overloaded" signal as worthy.
        ProviderError::RequestFailed(msg) => is_overloaded_message(msg),
        ProviderError::Authentication(_)
        | ProviderError::ContextLengthExceeded(_)
        | ProviderError::NetworkError(_)
        | ProviderError::ExecutionError(_)
        | ProviderError::UsageError(_)
        | ProviderError::NotImplemented(_)
        | ProviderError::EndpointNotFound(_)
        | ProviderError::CreditsExhausted { .. }
        | ProviderError::Refusal { .. } => false,
    }
}

/// True when a free-form error message indicates a transient "overloaded"
/// condition (case-insensitive).
fn is_overloaded_message(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("overloaded") || lower.contains("overload")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn parse_empty_is_disabled() {
        assert!(parse_fallback_chain("").is_empty());
        assert!(parse_fallback_chain("   ").is_empty());
        assert!(parse_fallback_chain(",, ,").is_empty());
    }

    #[test]
    fn parse_bare_model_reuses_current_provider() {
        let chain = parse_fallback_chain("gpt-4o");
        assert_eq!(
            chain,
            vec![FallbackTarget {
                provider: None,
                model: "gpt-4o".to_string(),
            }]
        );
    }

    #[test]
    fn parse_qualified_provider_model() {
        let chain = parse_fallback_chain("anthropic/claude-3-5-sonnet");
        assert_eq!(
            chain,
            vec![FallbackTarget {
                provider: Some("anthropic".to_string()),
                model: "claude-3-5-sonnet".to_string(),
            }]
        );
    }

    #[test]
    fn parse_mixed_chain_in_order_with_whitespace() {
        let chain = parse_fallback_chain(" anthropic/claude , gpt-4o ,openai/gpt-4o-mini, ");
        assert_eq!(
            chain,
            vec![
                FallbackTarget {
                    provider: Some("anthropic".to_string()),
                    model: "claude".to_string(),
                },
                FallbackTarget {
                    provider: None,
                    model: "gpt-4o".to_string(),
                },
                FallbackTarget {
                    provider: Some("openai".to_string()),
                    model: "gpt-4o-mini".to_string(),
                },
            ]
        );
    }

    #[test]
    fn parse_skips_entries_with_empty_model() {
        // "provider/" has no model and must be skipped.
        let chain = parse_fallback_chain("anthropic/, valid-model");
        assert_eq!(
            chain,
            vec![FallbackTarget {
                provider: None,
                model: "valid-model".to_string(),
            }]
        );
    }

    #[test]
    fn classifier_worthy_errors() {
        assert!(is_fallback_worthy(&ProviderError::RateLimitExceeded {
            details: "429".to_string(),
            retry_delay: Some(Duration::from_secs(1)),
        }));
        assert!(is_fallback_worthy(&ProviderError::ServerError(
            "500".to_string()
        )));
        assert!(is_fallback_worthy(&ProviderError::RequestFailed(
            "Provider Overloaded, try again".to_string()
        )));
    }

    #[test]
    fn classifier_unworthy_errors() {
        assert!(!is_fallback_worthy(&ProviderError::Authentication(
            "bad key".to_string()
        )));
        assert!(!is_fallback_worthy(&ProviderError::ContextLengthExceeded(
            "too long".to_string()
        )));
        // A plain request failure that is not an overload is not worthy.
        assert!(!is_fallback_worthy(&ProviderError::RequestFailed(
            "bad request".to_string()
        )));
    }
}
