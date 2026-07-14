use async_trait::async_trait;
use futures::Stream;
use rmcp::model::Tool;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use utoipa::ToSchema;

use crate::{
    canonical::{map_to_canonical_model, CanonicalModelRegistry},
    conversation::{
        message::{Message, MessageContent},
        token_usage::{ProviderUsage, Usage},
    },
    errors::ProviderError,
    goose_mode::GooseMode,
    model::ModelConfig,
    permission::PermissionConfirmation,
    retry::RetryConfig,
};

/// Information about a model's capabilities
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
pub struct ModelInfo {
    /// The name of the model
    pub name: String,
    /// The underlying model resolved from provider metadata, when the configured model is an alias or endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_model: Option<String>,
    /// The maximum context length this model supports
    pub context_limit: usize,
    /// Cost per token for input in USD (optional)
    pub input_token_cost: Option<f64>,
    /// Cost per token for output in USD (optional)
    pub output_token_cost: Option<f64>,
    /// Currency for the costs (default: "$")
    pub currency: Option<String>,
    /// Whether this model supports cache control
    pub supports_cache_control: Option<bool>,
    /// Whether this model supports reasoning/thinking controls
    #[serde(default)]
    pub reasoning: bool,
}

impl ModelInfo {
    /// Create a new ModelInfo with just name and context limit
    pub fn new(name: impl Into<String>, context_limit: usize) -> Self {
        Self {
            name: name.into(),
            resolved_model: None,
            context_limit,
            input_token_cost: None,
            output_token_cost: None,
            currency: None,
            supports_cache_control: None,
            reasoning: false,
        }
    }

    /// Create a new ModelInfo with cost information (per token)
    pub fn with_cost(
        name: impl Into<String>,
        context_limit: usize,
        input_cost: f64,
        output_cost: f64,
    ) -> Self {
        Self {
            name: name.into(),
            resolved_model: None,
            context_limit,
            input_token_cost: Some(input_cost),
            output_token_cost: Some(output_cost),
            currency: Some("$".to_string()),
            supports_cache_control: None,
            reasoning: false,
        }
    }
}

/// A message stream yields partial text content but complete tool calls, all within the Message object
/// So a message with text will contain potentially just a word of a longer response, but tool calls
/// messages will only be yielded once concatenated.
pub type MessageStream = Pin<
    Box<dyn Stream<Item = Result<(Option<Message>, Option<ProviderUsage>), ProviderError>> + Send>,
>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PermissionRouting {
    ActionRequired,
    Noop,
}

pub fn model_info_for_provider_model(provider_name: &str, model_name: &str) -> ModelInfo {
    let registry = CanonicalModelRegistry::bundled().ok();
    let canonical = registry.as_ref().and_then(|registry| {
        let canonical_id = map_to_canonical_model(provider_name, model_name, registry)?;
        let (provider, model) = canonical_id.split_once('/')?;
        registry.get(provider, model)
    });

    let reasoning = canonical
        .as_ref()
        .and_then(|model| model.reasoning)
        .unwrap_or_else(|| ModelConfig::new_or_fail(model_name).is_reasoning_model());

    ModelInfo {
        name: model_name.to_string(),
        resolved_model: None,
        context_limit: ModelConfig::new_or_fail(model_name)
            .with_canonical_limits(provider_name)
            .context_limit(),
        input_token_cost: None,
        output_token_cost: None,
        currency: None,
        supports_cache_control: None,
        reasoning,
    }
}

/// Collect all chunks from a MessageStream into a single Message and ProviderUsage
pub async fn collect_stream(
    mut stream: MessageStream,
) -> Result<(Message, ProviderUsage), ProviderError> {
    use futures::StreamExt;

    let mut final_message: Option<Message> = None;
    let mut final_usage: Option<ProviderUsage> = None;

    while let Some(result) = stream.next().await {
        let (msg_opt, usage_opt) = result?;

        if let Some(msg) = msg_opt {
            final_message = Some(match final_message {
                Some(mut prev) => {
                    for new_content in msg.content {
                        match (&mut prev.content.last_mut(), &new_content) {
                            // Coalesce consecutive text blocks
                            (
                                Some(MessageContent::Text(last_text)),
                                MessageContent::Text(new_text),
                            ) => {
                                last_text.text.push_str(&new_text.text);
                            }
                            _ => {
                                prev.content.push(new_content);
                            }
                        }
                    }
                    prev
                }
                None => msg,
            });
        }

        if let Some(usage) = usage_opt {
            final_usage = Some(usage);
        }
    }

    match final_message {
        Some(msg) => {
            let usage = final_usage
                .unwrap_or_else(|| ProviderUsage::new("unknown".to_string(), Usage::default()));
            Ok((msg, usage))
        }
        None => Err(ProviderError::ExecutionError(
            "Stream yielded no message".to_string(),
        )),
    }
}

pub fn stream_from_single_message(message: Message, usage: ProviderUsage) -> MessageStream {
    let stream = futures::stream::once(async move { Ok((Some(message), Some(usage))) });
    Box::pin(stream)
}

/// Whether a fast-model failure is worth retrying on the regular model.
///
/// Only transient conditions and fast-model capability/availability limits qualify: a regular
/// model can plausibly succeed where a smaller, cheaper one did not. Everything else fails
/// closed — retrying would double the cost of a request that is already doomed, and for
/// `Refusal` or `CreditsExhausted` it would also be user-hostile.
///
/// `ProviderError` has no cancellation or invalid-request variant of its own. Cancellation
/// arrives as `ExecutionError` (that is what `From<anyhow::Error>` produces for any untyped
/// error), and an invalid request arrives as `RequestFailed`. Both are treated as permanent
/// rather than distinguished by sniffing the message string, so a cancelled fast call stays
/// cancelled instead of silently spawning a second call.
///
/// The match is deliberately exhaustive: a new `ProviderError` variant breaks this build and
/// forces an explicit fallback decision instead of defaulting into a retry.
fn is_fallback_worthy(error: &ProviderError) -> bool {
    match error {
        // Transient: the fast model's failure says nothing about the regular model's odds.
        ProviderError::RateLimitExceeded { .. }
        | ProviderError::ServerError(_)
        | ProviderError::NetworkError(_) => true,

        // Capability/availability: the fast model is too small or not served here; the regular
        // model has its own (typically larger) context window and its own deployment.
        ProviderError::ContextLengthExceeded(_) | ProviderError::EndpointNotFound(_) => true,

        // Permanent, and identical for the regular model: same credentials, same account
        // balance, same safety policy, same malformed request.
        ProviderError::Authentication(_)
        | ProviderError::CreditsExhausted { .. }
        | ProviderError::Refusal { .. }
        | ProviderError::RequestFailed(_) => false,

        // Fail closed. `ExecutionError` is the catch-all that cancellation lands in, and
        // `NotImplemented`/`UsageError` describe the provider or its response shape rather than
        // the model — a second call would fail the same way.
        ProviderError::ExecutionError(_)
        | ProviderError::NotImplemented(_)
        | ProviderError::UsageError(_) => false,
    }
}

/// Base trait for AI providers (OpenAI, Anthropic, etc)
#[async_trait]
pub trait Provider: Send + Sync {
    /// Get the name of this provider instance
    fn get_name(&self) -> &str;

    /// Primary streaming method that all providers must implement.
    ///
    /// Note: Do not add `#[instrument]` here — the call sites (`complete` and
    /// `stream_response_from_provider`) create the telemetry span so that
    /// `session.id` is set once rather than in every provider.
    async fn stream(
        &self,
        model_config: &ModelConfig,
        session_id: &str,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError>;

    /// Complete with a specific model config.
    #[tracing::instrument(
        skip(self, model_config, session_id, system, messages, tools),
        fields(session.id = %session_id, gen_ai.request.model = %model_config.model_name)
    )]
    async fn complete(
        &self,
        model_config: &ModelConfig,
        session_id: &str,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<(Message, ProviderUsage), ProviderError> {
        let stream = self
            .stream(model_config, session_id, system, messages, tools)
            .await?;
        collect_stream(stream).await
    }

    /// Try fast model first, falling back to the regular model only for failures the regular
    /// model can plausibly recover from. See [`is_fallback_worthy`].
    async fn complete_fast(
        &self,
        session_id: &str,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<(Message, ProviderUsage), ProviderError> {
        let model_config = self.get_model_config();
        let fast_config = model_config.use_fast_model();

        let result = self
            .complete(&fast_config, session_id, system, messages, tools)
            .await;

        match result {
            Ok(response) => Ok(response),
            Err(e) => {
                if fast_config.model_name != model_config.model_name && is_fallback_worthy(&e) {
                    tracing::warn!(
                        "Fast model {} failed with error: {}. Falling back to regular model {}",
                        fast_config.model_name,
                        e,
                        model_config.model_name
                    );
                    self.complete(&model_config, session_id, system, messages, tools)
                        .await
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Get the model config from the provider
    fn get_model_config(&self) -> ModelConfig;

    fn retry_config(&self) -> RetryConfig {
        RetryConfig::default()
    }

    async fn fetch_supported_models(&self) -> Result<Vec<String>, ProviderError> {
        Ok(vec![])
    }

    async fn fetch_supported_model_info(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(self
            .fetch_supported_models()
            .await?
            .iter()
            .map(|model_name| model_info_for_provider_model(self.get_name(), model_name))
            .collect())
    }

    async fn fetch_model_info(&self, model_name: &str) -> Result<ModelInfo, ProviderError> {
        Ok(model_info_for_provider_model(self.get_name(), model_name))
    }

    fn skip_canonical_filtering(&self) -> bool {
        false
    }

    /// Fetch inventory models filtered by canonical registry and usability.
    async fn fetch_recommended_models(&self) -> Result<Vec<String>, ProviderError> {
        let all_models = self.fetch_supported_models().await?;

        if self.skip_canonical_filtering() {
            return Ok(all_models);
        }

        let registry = CanonicalModelRegistry::bundled().map_err(|e| {
            ProviderError::ExecutionError(format!("Failed to load canonical registry: {}", e))
        })?;

        let provider_name = self.get_name();

        // Get all text-capable models with their release dates
        let mut models_with_dates: Vec<(String, Option<String>)> = all_models
            .iter()
            .filter_map(|model| {
                let canonical_id = map_to_canonical_model(provider_name, model, registry)?;

                let (provider, model_name) = canonical_id.split_once('/')?;
                let canonical_model = registry.get(provider, model_name)?;

                if !canonical_model
                    .modalities
                    .input
                    .contains(&crate::canonical::Modality::Text)
                {
                    return None;
                }

                if !canonical_model.tool_call && !self.get_model_config().toolshim {
                    return None;
                }

                let release_date = canonical_model.release_date.clone();

                Some((model.clone(), release_date))
            })
            .collect();

        // Sort by release date (most recent first), then alphabetically for models without dates
        models_with_dates.sort_by(|a, b| match (&a.1, &b.1) {
            (Some(date_a), Some(date_b)) => date_b.cmp(date_a),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.0.cmp(&b.0),
        });

        let inventory_models: Vec<String> = models_with_dates
            .into_iter()
            .map(|(name, _)| name)
            .collect();

        if inventory_models.is_empty() {
            Ok(all_models)
        } else {
            Ok(inventory_models)
        }
    }

    async fn fetch_recommended_model_info(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(self
            .fetch_recommended_models()
            .await?
            .iter()
            .map(|model_name| model_info_for_provider_model(self.get_name(), model_name))
            .collect())
    }

    async fn map_to_canonical_model(
        &self,
        provider_model: &str,
    ) -> Result<Option<String>, ProviderError> {
        let registry = CanonicalModelRegistry::bundled().map_err(|e| {
            ProviderError::ExecutionError(format!("Failed to load canonical registry: {}", e))
        })?;

        Ok(map_to_canonical_model(
            self.get_name(),
            provider_model,
            registry,
        ))
    }

    /// Whether the provider manages its own conversation context (e.g. CLI
    /// wrappers like Claude Code or Gemini CLI). When true, goose-side
    /// context management such as tool-pair summarization is skipped because
    /// the provider's internal state is the source of truth.
    fn manages_own_context(&self) -> bool {
        false
    }

    async fn supports_cache_control(&self) -> bool {
        false
    }

    /// Configure OAuth authentication for this provider
    ///
    /// This method is called when a provider has configuration keys marked with oauth_flow = true.
    /// Providers that support OAuth should override this method to implement their specific OAuth flow.
    ///
    /// # Returns
    /// * `Ok(())` if OAuth configuration succeeds and credentials are saved
    /// * `Err(ProviderError)` if OAuth fails or is not supported by this provider
    ///
    /// # Default Implementation
    /// The default implementation returns an error indicating OAuth is not supported.
    async fn configure_oauth(&self) -> Result<(), ProviderError> {
        Err(ProviderError::ExecutionError(
            "OAuth configuration not supported by this provider".to_string(),
        ))
    }

    async fn refresh_credentials(&self) -> Result<(), ProviderError> {
        Err(ProviderError::NotImplemented(
            "credential refresh not supported by this provider".to_string(),
        ))
    }

    async fn update_mode(&self, _session_id: &str, _mode: GooseMode) -> Result<(), ProviderError> {
        Ok(())
    }

    fn permission_routing(&self) -> PermissionRouting {
        PermissionRouting::Noop
    }

    async fn handle_permission_confirmation(
        &self,
        _request_id: &str,
        _confirmation: &PermissionConfirmation,
    ) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use test_case::test_case;

    fn content_from_str(s: String) -> MessageContent {
        if let Some(img_data) = s.strip_prefix("*img:") {
            MessageContent::image(format!("http://example.com/{}", img_data), "image/png")
        } else if let Some(tool_name) = s.strip_prefix("*tool:") {
            let tool_call = Ok(
                rmcp::model::CallToolRequestParams::new(tool_name.to_string())
                    .with_arguments(serde_json::Map::new()),
            );
            MessageContent::tool_request(format!("tool_{}", tool_name), tool_call)
        } else {
            MessageContent::text(s)
        }
    }

    fn create_test_stream(
        items: Vec<String>,
    ) -> impl Stream<Item = Result<(Option<Message>, Option<ProviderUsage>), ProviderError>> {
        use futures::stream;
        stream::iter(items.into_iter().map(|item| {
            let content = content_from_str(item);
            let message = Message::new(
                rmcp::model::Role::Assistant,
                chrono::Utc::now().timestamp(),
                vec![content],
            );
            Ok((Some(message), None))
        }))
    }

    fn content_to_strings(msg: &Message) -> Vec<String> {
        msg.content
            .iter()
            .map(|c| match c {
                MessageContent::Text(t) => t.text.clone(),
                MessageContent::Image(_) => "*img".to_string(),
                MessageContent::ToolRequest(tr) => {
                    if let Ok(call) = &tr.tool_call {
                        format!("*tool:{}", call.name)
                    } else {
                        "*tool:error".to_string()
                    }
                }
                _ => "*other".to_string(),
            })
            .collect()
    }

    #[test_case(
        vec!["Hello", " ", "world"],
        vec!["Hello world"]
        ; "consecutive text coalesces"
    )]
    #[test_case(
        vec!["Hello", "*img:pic1", "world"],
        vec!["Hello", "*img", "world"]
        ; "non-text breaks coalescing"
    )]
    #[test_case(
        vec!["A", "B", "*img:pic1", "C", "D", "*tool:read", "E", "F"],
        vec!["AB", "*img", "CD", "*tool:read", "EF"]
        ; "multiple text groups"
    )]
    #[test_case(
        vec!["Text1", "*img:pic", "Text2"],
        vec!["Text1", "*img", "Text2"]
        ; "mixed content in chunk"
    )]
    #[tokio::test]
    async fn test_collect_stream_coalescing(input_items: Vec<&str>, expected: Vec<&str>) {
        let items: Vec<String> = input_items.into_iter().map(|s| s.to_string()).collect();
        let stream = create_test_stream(items);
        let (msg, _) = collect_stream(Box::pin(stream)).await.unwrap();
        assert_eq!(content_to_strings(&msg), expected);
    }

    #[tokio::test]
    async fn test_collect_stream_defaults_usage() {
        let stream = create_test_stream(vec!["Hello".to_string()]);
        let (msg, usage) = collect_stream(Box::pin(stream)).await.unwrap();
        assert_eq!(content_to_strings(&msg), vec!["Hello"]);
        assert_eq!(usage.model, "unknown");
    }

    #[test]
    fn test_model_info_creation() {
        // Test direct ModelInfo creation
        let info = ModelInfo {
            name: "test-model".to_string(),
            resolved_model: None,
            context_limit: 1000,
            input_token_cost: None,
            output_token_cost: None,
            currency: None,
            supports_cache_control: None,
            reasoning: false,
        };
        assert_eq!(info.context_limit, 1000);

        // Test equality
        let info2 = ModelInfo {
            name: "test-model".to_string(),
            resolved_model: None,
            context_limit: 1000,
            input_token_cost: None,
            output_token_cost: None,
            currency: None,
            supports_cache_control: None,
            reasoning: false,
        };
        assert_eq!(info, info2);

        // Test inequality
        let info3 = ModelInfo {
            name: "test-model".to_string(),
            resolved_model: None,
            context_limit: 2000,
            input_token_cost: None,
            output_token_cost: None,
            currency: None,
            supports_cache_control: None,
            reasoning: false,
        };
        assert_ne!(info, info3);
    }

    #[test]
    fn test_model_info_with_cost() {
        let info = ModelInfo::with_cost("gpt-4o", 128000, 0.0000025, 0.00001);
        assert_eq!(info.name, "gpt-4o");
        assert_eq!(info.context_limit, 128000);
        assert_eq!(info.input_token_cost, Some(0.0000025));
        assert_eq!(info.output_token_cost, Some(0.00001));
        assert_eq!(info.currency, Some("$".to_string()));
    }

    const REGULAR_MODEL: &str = "gpt-4o";
    const FAST_MODEL: &str = "gpt-4o-mini";

    /// Records every model it is asked to stream with, and fails the first call made with
    /// `FAST_MODEL`. Any other call succeeds, so the recorded models are exactly the fallback
    /// path complete_fast took.
    struct CountingProvider {
        model_config: ModelConfig,
        fast_model_error: Mutex<Option<ProviderError>>,
        calls: Mutex<Vec<String>>,
    }

    impl CountingProvider {
        fn new(model_config: ModelConfig, fast_model_error: ProviderError) -> Self {
            Self {
                model_config,
                fast_model_error: Mutex::new(Some(fast_model_error)),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn with_fast_model(fast_model_error: ProviderError) -> Self {
            let config = ModelConfig::new_or_fail(REGULAR_MODEL)
                .with_fast_model_config(ModelConfig::new_or_fail(FAST_MODEL));
            Self::new(config, fast_model_error)
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl Provider for CountingProvider {
        fn get_name(&self) -> &str {
            "counting"
        }

        async fn stream(
            &self,
            model_config: &ModelConfig,
            _session_id: &str,
            _system: &str,
            _messages: &[Message],
            _tools: &[Tool],
        ) -> Result<MessageStream, ProviderError> {
            self.calls
                .lock()
                .unwrap()
                .push(model_config.model_name.clone());

            if model_config.model_name == FAST_MODEL {
                let error = self
                    .fast_model_error
                    .lock()
                    .unwrap()
                    .take()
                    .expect("fast model called more than once");
                return Err(error);
            }

            let message = Message::new(
                rmcp::model::Role::Assistant,
                chrono::Utc::now().timestamp(),
                vec![MessageContent::text("regular model response")],
            );
            let usage = ProviderUsage::new(model_config.model_name.clone(), Usage::default());
            Ok(stream_from_single_message(message, usage))
        }

        fn get_model_config(&self) -> ModelConfig {
            self.model_config.clone()
        }
    }

    #[test_case(ProviderError::Authentication("invalid api key".to_string()) ; "authentication")]
    #[test_case(ProviderError::CreditsExhausted { details: "out of credits".to_string(), top_up_url: None } ; "credits exhausted")]
    #[test_case(ProviderError::Refusal { details: "declined".to_string(), category: None } ; "refusal")]
    #[test_case(ProviderError::RequestFailed("status: 400 Bad Request".to_string()) ; "invalid request")]
    #[test_case(ProviderError::ExecutionError("request cancelled".to_string()) ; "cancellation")]
    #[test_case(ProviderError::NotImplemented("streaming unsupported".to_string()) ; "not implemented")]
    #[test_case(ProviderError::UsageError("missing usage".to_string()) ; "usage error")]
    #[tokio::test]
    async fn test_complete_fast_permanent_error_makes_one_call(error: ProviderError) {
        let expected = error.to_string();
        let provider = CountingProvider::with_fast_model(error);

        let result = provider.complete_fast("session", "system", &[], &[]).await;

        assert_eq!(result.unwrap_err().to_string(), expected);
        assert_eq!(provider.calls(), vec![FAST_MODEL]);
    }

    #[test_case(ProviderError::RateLimitExceeded { details: "slow down".to_string(), retry_delay: None } ; "rate limit")]
    #[test_case(ProviderError::ServerError("500".to_string()) ; "server error")]
    #[test_case(ProviderError::NetworkError("connection reset".to_string()) ; "network error")]
    #[test_case(ProviderError::ContextLengthExceeded("too many tokens".to_string()) ; "context length exceeded")]
    #[test_case(ProviderError::EndpointNotFound("unknown model".to_string()) ; "endpoint not found")]
    #[tokio::test]
    async fn test_complete_fast_transient_error_falls_back_to_regular(error: ProviderError) {
        let provider = CountingProvider::with_fast_model(error);

        let (_, usage) = provider
            .complete_fast("session", "system", &[], &[])
            .await
            .unwrap();

        assert_eq!(usage.model, REGULAR_MODEL);
        assert_eq!(provider.calls(), vec![FAST_MODEL, REGULAR_MODEL]);
    }

    #[tokio::test]
    async fn test_complete_fast_without_fast_model_does_not_retry() {
        let provider = CountingProvider::new(
            ModelConfig::new_or_fail(FAST_MODEL),
            ProviderError::ServerError("500".to_string()),
        );

        let result = provider.complete_fast("session", "system", &[], &[]).await;

        assert!(matches!(result, Err(ProviderError::ServerError(_))));
        assert_eq!(provider.calls(), vec![FAST_MODEL]);
    }
}
