//! Text embeddings client (opt-in, default OFF).
//!
//! Turns text into vectors via the active local / India provider's embeddings
//! endpoint — either Ollama's `/api/embeddings` or an OpenAI-compatible
//! `/v1/embeddings`. The feature is gated entirely behind the
//! [`EMBED_MODEL_KEY`] (`BHARATCODE_EMBED_MODEL`) config key: when that key is
//! unset, [`EmbeddingClient::from_env`] returns `None` and nothing in this
//! module ever runs, so default behaviour is unchanged.
//!
//! Every request is issued through the shared [`ApiClient`] in
//! `goose-providers`, so the central data-residency / offline egress guard
//! (`screen_endpoint`, installed at registry init via
//! `crate::offline::install_egress_guard`) is applied automatically — this
//! module never opens a raw socket of its own.
//!
//! This is the reusable building block that later retrieval / codebase-context
//! consumers call to vectorise text. Embedding models are intentionally **not**
//! priced against the USD/INR cost registry: embeddings are cheap and out of
//! scope for per-token cost capture, so no entry is added there.

use super::api_client::{ApiClient, AuthMethod};
use crate::config::{Config, ConfigError};
use goose_providers::errors::ProviderError;
use serde_json::{json, Value};
use url::Url;

/// Config key that gates the whole feature. Unset => embeddings are OFF.
pub const EMBED_MODEL_KEY: &str = "BHARATCODE_EMBED_MODEL";

/// Host of the active local / India provider, resolved the same way
/// `ollama.rs` resolves it. Defaults to Ollama's loopback host so the egress
/// guard treats it as local.
const DEFAULT_EMBED_HOST: &str = "localhost";
const OLLAMA_DEFAULT_PORT: u16 = 11434;

/// Suffix that identifies an Ollama-style embeddings endpoint. When the base
/// URL already ends in this we POST to it directly; otherwise we treat the base
/// as an OpenAI-compatible root and POST to `{base}/v1/embeddings`.
const OLLAMA_EMBED_SUFFIX: &str = "/api/embeddings";
const OPENAI_EMBED_PATH: &str = "v1/embeddings";

/// A small, reusable client that turns text into embedding vectors.
///
/// Construct it with [`EmbeddingClient::from_env`]; it stays disabled (returns
/// `None`) unless [`EMBED_MODEL_KEY`] is configured.
pub struct EmbeddingClient {
    base_url: String,
    model: String,
    api_key: Option<String>,
}

impl EmbeddingClient {
    /// Build a client from configuration, or `None` when embeddings are OFF.
    ///
    /// Returns `None` whenever [`EMBED_MODEL_KEY`] is unset (the default), so
    /// callers can cheaply probe whether embeddings are available without
    /// changing any behaviour when they are not.
    pub fn from_env() -> Option<EmbeddingClient> {
        let config = Config::global();
        let model: String = match config.get_param::<String>(EMBED_MODEL_KEY) {
            Ok(model) if !model.trim().is_empty() => model,
            Ok(_) => return None,
            Err(ConfigError::NotFound(_)) => return None,
            Err(e) => {
                tracing::warn!("Invalid {} value: {}", EMBED_MODEL_KEY, e);
                return None;
            }
        };

        let base_url = resolve_base_url(config);
        let api_key = config
            .get_secret::<String>("OPENAI_API_KEY")
            .ok()
            .filter(|k| !k.is_empty());

        Some(EmbeddingClient {
            base_url,
            model,
            api_key,
        })
    }

    /// Construct directly from explicit parts (used by tests / callers that
    /// already know the endpoint).
    pub fn new(base_url: String, model: String, api_key: Option<String>) -> Self {
        Self {
            base_url,
            model,
            api_key,
        }
    }

    /// Embed one or more texts, returning one `f32` vector per input in order.
    ///
    /// Detects the endpoint shape from the base URL: a trailing
    /// `/api/embeddings` is treated as Ollama; anything else is treated as an
    /// OpenAI-compatible root and the request goes to `{base}/v1/embeddings`.
    /// Both response shapes are handled: OpenAI's `data[].embedding`, and
    /// Ollama's singular `embedding` / batched `embeddings`.
    pub async fn embed_texts(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, ProviderError> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }

        let payload = json!({
            "model": self.model,
            "input": inputs,
        });

        let trimmed = self.base_url.trim_end_matches('/');
        let (host, path) = if trimmed.ends_with(OLLAMA_EMBED_SUFFIX) {
            let host = trimmed.strip_suffix(OLLAMA_EMBED_SUFFIX).unwrap_or(trimmed);
            (
                host.to_string(),
                OLLAMA_EMBED_SUFFIX.trim_start_matches('/').to_string(),
            )
        } else {
            (trimmed.to_string(), OPENAI_EMBED_PATH.to_string())
        };

        let auth = match &self.api_key {
            Some(key) => AuthMethod::BearerToken(key.clone()),
            None => AuthMethod::NoAuth,
        };

        let client = ApiClient::new_with_tls(host, auth, None)
            .map_err(|e| ProviderError::RequestFailed(format!("invalid embeddings host: {e}")))?;

        let response = client
            .api_post(None, &path, &payload)
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("embeddings request failed: {e}")))?;

        if !response.status.is_success() {
            return Err(ProviderError::RequestFailed(format!(
                "embeddings endpoint returned HTTP {}",
                response.status
            )));
        }

        let body = response.payload.ok_or_else(|| {
            ProviderError::RequestFailed("embeddings response had no body".to_string())
        })?;

        parse_embeddings(&body)
    }
}

/// Resolve the embeddings endpoint base URL from the active local / India
/// provider host, mirroring how `ollama.rs` / `openai_compatible.rs` resolve a
/// host into a fully-qualified base URL (scheme + default loopback port).
fn resolve_base_url(config: &Config) -> String {
    let host: String = config
        .get_param("OLLAMA_HOST")
        .unwrap_or_else(|_| DEFAULT_EMBED_HOST.to_string());

    let base = if host.starts_with("http://") || host.starts_with("https://") {
        host.clone()
    } else {
        format!("http://{host}")
    };

    let mut base_url = match Url::parse(&base) {
        Ok(url) => url,
        Err(_) => return base,
    };

    let explicit_port = host.contains(':');
    let is_localhost = host == "localhost" || host == "127.0.0.1" || host == "::1";
    if base_url.port().is_none() && !explicit_port && !host.starts_with("http") && is_localhost {
        let _ = base_url.set_port(Some(OLLAMA_DEFAULT_PORT));
    }

    base_url.to_string()
}

/// Parse embedding vectors out of either an OpenAI-compatible
/// (`data[].embedding`) or Ollama (`embedding` / `embeddings`) JSON response.
fn parse_embeddings(body: &Value) -> Result<Vec<Vec<f32>>, ProviderError> {
    if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
        let mut out = Vec::with_capacity(data.len());
        for item in data {
            let vec = item
                .get("embedding")
                .and_then(|e| e.as_array())
                .ok_or_else(|| {
                    ProviderError::RequestFailed("missing data[].embedding array".to_string())
                })?;
            out.push(to_f32_vec(vec)?);
        }
        return Ok(out);
    }

    if let Some(embeddings) = body.get("embeddings").and_then(|e| e.as_array()) {
        let mut out = Vec::with_capacity(embeddings.len());
        for item in embeddings {
            let vec = item.as_array().ok_or_else(|| {
                ProviderError::RequestFailed("malformed embeddings[] entry".to_string())
            })?;
            out.push(to_f32_vec(vec)?);
        }
        return Ok(out);
    }

    if let Some(embedding) = body.get("embedding").and_then(|e| e.as_array()) {
        return Ok(vec![to_f32_vec(embedding)?]);
    }

    Err(ProviderError::RequestFailed(
        "embeddings response had no recognisable vector field".to_string(),
    ))
}

fn to_f32_vec(values: &[Value]) -> Result<Vec<f32>, ProviderError> {
    values
        .iter()
        .map(|v| {
            v.as_f64().map(|f| f as f32).ok_or_else(|| {
                ProviderError::RequestFailed("non-numeric embedding value".to_string())
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn from_env_is_none_when_key_absent() {
        let _guard = env_lock::lock_env([(EMBED_MODEL_KEY, None::<&str>)]);
        let config_file = tempfile::NamedTempFile::new().unwrap();
        let secrets_file = tempfile::NamedTempFile::new().unwrap();
        let _config = crate::config::Config::new_with_config_paths(
            vec![config_file.path().to_path_buf()],
            secrets_file.path(),
        )
        .unwrap();

        assert!(
            EmbeddingClient::from_env().is_none(),
            "embeddings must stay OFF when {EMBED_MODEL_KEY} is unset"
        );
    }

    #[tokio::test]
    async fn embed_texts_openai_compatible_returns_vectors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{ "embedding": [0.1, 0.2, 0.3] }]
            })))
            .mount(&server)
            .await;

        let client = EmbeddingClient::new(server.uri(), "test-embed".to_string(), None);
        let vectors = client.embed_texts(&["x".to_string()]).await.unwrap();

        assert_eq!(vectors, vec![vec![0.1_f32, 0.2, 0.3]]);
        assert_eq!(vectors[0].len(), 3);
    }

    #[tokio::test]
    async fn embed_texts_parses_ollama_endpoint() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "embedding": [0.4, 0.5, 0.6]
            })))
            .mount(&server)
            .await;

        let base = format!("{}/api/embeddings", server.uri());
        let client = EmbeddingClient::new(base, "test-embed".to_string(), None);
        let vectors = client.embed_texts(&["x".to_string()]).await.unwrap();

        assert_eq!(vectors, vec![vec![0.4_f32, 0.5, 0.6]]);
    }

    #[tokio::test]
    async fn embed_texts_empty_input_skips_request() {
        let client = EmbeddingClient::new("http://127.0.0.1:1".to_string(), "m".to_string(), None);
        let vectors = client.embed_texts(&[]).await.unwrap();
        assert!(vectors.is_empty());
    }
}
