use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

#[cfg(feature = "aws-providers")]
use super::bedrock::BedrockProvider;
#[cfg(feature = "local-inference")]
use super::local_inference::LocalInferenceProvider;
#[cfg(feature = "aws-providers")]
use super::sagemaker_tgi::SageMakerTgiProvider;
use super::{
    amp_acp::AmpAcpProvider,
    anthropic::AnthropicProvider,
    avian::AvianProvider,
    azure::AzureProvider,
    base::{Provider, ProviderMetadata},
    chatgpt_codex::ChatGptCodexProvider,
    claude_acp::ClaudeAcpProvider,
    claude_code::ClaudeCodeProvider,
    codex::CodexProvider,
    codex_acp::CodexAcpProvider,
    copilot_acp::CopilotAcpProvider,
    cursor_agent::CursorAgentProvider,
    databricks::DatabricksProvider,
    databricks_v2::DatabricksV2Provider,
    gcpvertexai::GcpVertexAIProvider,
    gemini_cli::GeminiCliProvider,
    gemini_oauth::GeminiOAuthProvider,
    githubcopilot::GithubCopilotProvider,
    google::GoogleProvider,
    huggingface::HuggingFaceProvider,
    kimicode::KimiCodeProvider,
    litellm::LiteLLMProvider,
    nanogpt::NanoGptProvider,
    ollama::OllamaProvider,
    openai::OpenAiProvider,
    openrouter::OpenRouterProvider,
    pi_acp::PiAcpProvider,
    provider_registry::ProviderRegistry,
    snowflake::SnowflakeProvider,
    tetrate::TetrateProvider,
    xai::XaiProvider,
    xai_oauth::XaiOAuthProvider,
};
use crate::config::ExtensionConfig;
use crate::providers::base::ProviderType;
use crate::{
    config::declarative_providers::register_declarative_providers,
    providers::provider_registry::ProviderEntry,
};
use anyhow::Result;
use bharatcode_providers::model::ModelConfig;
use tokio::sync::OnceCell;

static REGISTRY: OnceCell<RwLock<ProviderRegistry>> = OnceCell::const_new();

async fn init_registry() -> RwLock<ProviderRegistry> {
    // Install the composed residency / offline egress guard into the shared
    // provider HTTP client before any provider can issue a request, so every
    // provider (including declarative ones) is screened in one central place.
    crate::offline::install_egress_guard();

    let tls_config =
        crate::config::tls::provider_tls_config_from_config(crate::config::Config::global())
            .expect("failed to load provider TLS config");
    let mut registry = ProviderRegistry::new(tls_config).with_providers(|registry| {
        use super::inventory::registrations;

        registry.register_with_inventory::<AmpAcpProvider>(
            false,
            Some(registrations::amp_acp_inventory()),
        );
        registry.register_with_inventory::<AnthropicProvider>(
            true,
            Some(registrations::anthropic_inventory()),
        );
        registry.register::<AvianProvider>(false);
        registry.register::<AzureProvider>(false);
        #[cfg(feature = "aws-providers")]
        registry.register::<BedrockProvider>(false);
        #[cfg(feature = "local-inference")]
        registry.register::<LocalInferenceProvider>(false);
        registry.register_with_inventory::<ChatGptCodexProvider>(
            true,
            Some(registrations::chatgpt_codex_inventory()),
        );
        registry.register_with_inventory::<ClaudeAcpProvider>(
            false,
            Some(registrations::claude_acp_inventory()),
        );
        registry.register::<ClaudeCodeProvider>(true);
        registry.register_with_inventory::<CodexAcpProvider>(
            false,
            Some(registrations::codex_acp_inventory()),
        );
        registry.register_with_inventory::<CopilotAcpProvider>(
            false,
            Some(registrations::copilot_acp_inventory()),
        );
        registry.register::<CodexProvider>(true);
        registry.register::<CursorAgentProvider>(false);
        registry.register_with_inventory::<DatabricksProvider>(
            true,
            Some(registrations::refresh_only()),
        );
        registry.register_with_inventory::<DatabricksV2Provider>(
            false,
            Some(registrations::refresh_only()),
        );
        registry.register::<GcpVertexAIProvider>(false);
        registry.register::<GeminiCliProvider>(false);
        registry.register::<GeminiOAuthProvider>(true);
        registry.register::<GithubCopilotProvider>(false);
        registry.register_with_inventory::<GoogleProvider>(
            true,
            Some(registrations::google_inventory()),
        );
        registry.register_with_inventory::<HuggingFaceProvider>(
            true,
            Some(registrations::huggingface_inventory()),
        );
        registry.register::<KimiCodeProvider>(true);
        registry.register::<LiteLLMProvider>(false);
        registry.register::<NanoGptProvider>(true);
        registry.register_with_inventory::<OllamaProvider>(
            true,
            Some(registrations::ollama_inventory()),
        );
        registry.register_with_inventory::<OpenAiProvider>(
            true,
            Some(registrations::openai_inventory()),
        );
        registry.register::<OpenRouterProvider>(true);
        registry.register_with_inventory::<PiAcpProvider>(
            false,
            Some(registrations::pi_acp_inventory()),
        );
        #[cfg(feature = "aws-providers")]
        registry.register::<SageMakerTgiProvider>(false);
        registry.register::<SnowflakeProvider>(false);
        registry.register::<TetrateProvider>(true);
        registry.register::<XaiProvider>(false);
        registry.register_with_inventory::<XaiOAuthProvider>(
            true,
            Some(registrations::xai_oauth_inventory()),
        );
    });
    // Register cleanup functions for providers with cached state
    registry.set_cleanup(
        "github_copilot",
        Arc::new(|| Box::pin(GithubCopilotProvider::cleanup())),
    );
    registry.set_cleanup(
        "databricks",
        Arc::new(|| Box::pin(DatabricksProvider::cleanup())),
    );
    registry.set_cleanup(
        "databricks_v2",
        Arc::new(|| Box::pin(DatabricksV2Provider::cleanup())),
    );
    registry.set_cleanup(
        "kimi_code",
        Arc::new(|| Box::pin(KimiCodeProvider::cleanup())),
    );
    registry.set_cleanup(
        "chatgpt_codex",
        Arc::new(|| Box::pin(ChatGptCodexProvider::cleanup())),
    );
    registry.set_cleanup(
        "gemini_oauth",
        Arc::new(|| Box::pin(GeminiOAuthProvider::cleanup())),
    );
    registry.set_cleanup(
        "xai_oauth",
        Arc::new(|| Box::pin(XaiOAuthProvider::cleanup())),
    );
    registry.set_cleanup(
        "huggingface",
        Arc::new(|| Box::pin(HuggingFaceProvider::cleanup())),
    );

    if let Err(e) = register_declarative_providers(&mut registry) {
        tracing::warn!("Failed to load custom providers: {}", e);
    }
    RwLock::new(registry)
}

/// Loads custom providers into a staging registry so a malformed config fails
/// before the live registry is touched. The fixed declarative providers staged
/// alongside them are discarded, since they are compiled in rather than read
/// from disk and the live registry already holds them.
fn build_custom_provider_entries() -> Result<HashMap<String, ProviderEntry>> {
    let tls_config =
        crate::config::tls::provider_tls_config_from_config(crate::config::Config::global())?;
    let mut staged = ProviderRegistry::new(tls_config);
    register_declarative_providers(&mut staged)?;

    Ok(staged
        .entries
        .into_iter()
        .filter(|(_, entry)| entry.provider_type() == ProviderType::Custom)
        .collect())
}

fn replace_custom_provider_entries(
    registry: &mut ProviderRegistry,
    custom_entries: HashMap<String, ProviderEntry>,
) -> Result<()> {
    if let Some(name) = custom_entries.keys().find(|name| {
        registry
            .entries
            .get(name.as_str())
            .is_some_and(|entry| entry.provider_type() != ProviderType::Custom)
    }) {
        anyhow::bail!("Custom provider '{name}' conflicts with an existing provider");
    }

    registry
        .entries
        .retain(|_, entry| entry.provider_type() != ProviderType::Custom);
    registry.entries.extend(custom_entries);
    Ok(())
}

async fn get_registry() -> &'static RwLock<ProviderRegistry> {
    REGISTRY.get_or_init(init_registry).await
}

pub async fn providers() -> Vec<(ProviderMetadata, ProviderType)> {
    get_registry()
        .await
        .read()
        .unwrap()
        .all_metadata_with_types()
}

/// Replaces the registered custom providers with the ones currently on disk.
///
/// The new set is built before any lock is taken, so a failed load leaves the
/// previously registered providers in place. The removal and the insertion then
/// happen under a single write lock, so readers never observe a registry that is
/// missing its custom providers.
pub async fn refresh_custom_providers() -> Result<()> {
    let custom_entries = match build_custom_provider_entries() {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!(
                "Failed to refresh custom providers, keeping previously loaded ones: {}",
                e
            );
            return Err(e);
        }
    };

    let mut registry = get_registry().await.write().unwrap();
    replace_custom_provider_entries(&mut registry, custom_entries)?;

    tracing::info!("Custom providers refreshed");
    Ok(())
}

pub async fn get_from_registry(name: &str) -> Result<ProviderEntry> {
    let guard = get_registry().await.read().unwrap();
    guard
        .entries
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("Unknown provider: {}", name))
        .cloned()
}

pub async fn inventory_identity(name: &str) -> Result<super::inventory::InventoryIdentityInput> {
    get_from_registry(name).await?.inventory_identity()
}

pub async fn create(
    name: &str,
    model: ModelConfig,
    extensions: Vec<ExtensionConfig>,
) -> Result<Arc<dyn Provider>> {
    let entry = get_from_registry(name).await?;
    entry.create(model, extensions).await
}

pub async fn create_with_working_dir(
    name: &str,
    model: ModelConfig,
    extensions: Vec<ExtensionConfig>,
    working_dir: PathBuf,
) -> Result<Arc<dyn Provider>> {
    let entry = get_from_registry(name).await?;
    entry
        .create_with_working_dir(model, extensions, working_dir)
        .await
}

pub async fn create_with_default_model(
    name: impl AsRef<str>,
    extensions: Vec<ExtensionConfig>,
) -> Result<Arc<dyn Provider>> {
    get_from_registry(name.as_ref())
        .await?
        .create_with_default_model(extensions)
        .await
}

pub async fn cleanup_provider(name: &str) -> Result<()> {
    let cleanup_fn = {
        let registry = get_registry().await.read().unwrap();
        registry
            .entries
            .get(name)
            .and_then(|entry| entry.cleanup.clone())
    };
    if let Some(cleanup) = cleanup_fn {
        return cleanup().await;
    }
    Ok(())
}

pub async fn create_with_named_model(
    provider_name: &str,
    model_name: &str,
    extensions: Vec<ExtensionConfig>,
) -> Result<Arc<dyn Provider>> {
    let config = crate::model_config::model_config_from_user_config(provider_name, model_name)?;
    create(provider_name, config, extensions).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::paths::Paths;
    use std::fs;

    #[tokio::test]
    async fn test_tanzu_declarative_provider_registry_wiring() {
        let providers_list = providers().await;
        let tanzu = providers_list
            .iter()
            .find(|(m, _)| m.name == "tanzu_ai")
            .expect("tanzu_ai provider should be registered");
        let (meta, provider_type) = tanzu;

        // Should be a Declarative (fixed) provider
        assert_eq!(*provider_type, ProviderType::Declarative);

        assert_eq!(meta.display_name, "VMware Tanzu Platform");
        assert_eq!(meta.default_model, "openai/gpt-oss-120b");

        // First config key should be TANZU_AI_API_KEY (secret, required)
        let api_key = meta
            .config_keys
            .iter()
            .find(|k| k.name == "TANZU_AI_API_KEY")
            .expect("TANZU_AI_API_KEY config key should exist");
        assert!(
            api_key.required,
            "API key should be required for fixed declarative provider"
        );
        assert!(api_key.secret, "API key should be secret");

        // Should have TANZU_AI_ENDPOINT config key (not secret, required)
        let endpoint = meta
            .config_keys
            .iter()
            .find(|k| k.name == "TANZU_AI_ENDPOINT")
            .expect("TANZU_AI_ENDPOINT config key should exist");
        assert!(endpoint.required, "Endpoint should be required");
        assert!(!endpoint.secret, "Endpoint should not be secret");
    }

    #[tokio::test]
    async fn test_huggingface_provider_registry_wiring() {
        let huggingface = get_from_registry("huggingface")
            .await
            .expect("huggingface provider should be registered");
        let meta = huggingface.metadata();

        assert_eq!(huggingface.provider_type(), ProviderType::Preferred);
        assert_eq!(meta.display_name, "Hugging Face");
        assert_eq!(meta.default_model, "Qwen/Qwen3-Coder-480B-A35B-Instruct");
        assert!(meta
            .config_keys
            .iter()
            .any(|key| key.name == "HF_TOKEN" && key.secret));
    }

    #[tokio::test]
    async fn test_nvidia_declarative_provider_registry_wiring() {
        let nvidia = get_from_registry("nvidia")
            .await
            .expect("nvidia provider should be registered");
        let meta = nvidia.metadata();

        assert_eq!(nvidia.provider_type(), ProviderType::Declarative);
        assert!(nvidia.supports_inventory_refresh());
        assert_eq!(meta.display_name, "NVIDIA");
        assert_eq!(meta.default_model, "z-ai/glm-4.7");
        assert_eq!(meta.model_doc_link, "https://build.nvidia.com/models");
        assert!(!meta.setup_steps.is_empty());

        let api_key = meta
            .config_keys
            .iter()
            .find(|k| k.name == "NVIDIA_API_KEY")
            .expect("NVIDIA_API_KEY config key should exist");
        assert!(api_key.required, "NVIDIA_API_KEY should be required");
        assert!(api_key.secret, "NVIDIA_API_KEY should be secret");
        assert!(api_key.primary, "NVIDIA_API_KEY should be primary");
        assert!(
            !meta.config_keys.iter().any(|k| k.name == "OPENAI_HOST"),
            "NVIDIA should not expose OpenAI host configuration"
        );
        assert!(
            !meta
                .config_keys
                .iter()
                .any(|k| k.name == "OPENAI_BASE_PATH"),
            "NVIDIA should not expose OpenAI base path configuration"
        );
    }

    #[tokio::test]
    async fn test_nearai_declarative_provider_registry_wiring() {
        let nearai = get_from_registry("nearai")
            .await
            .expect("nearai provider should be registered");
        let meta = nearai.metadata();

        assert_eq!(nearai.provider_type(), ProviderType::Declarative);
        assert!(nearai.supports_inventory_refresh());
        assert_eq!(meta.display_name, "NEAR AI Cloud");
        assert_eq!(meta.default_model, "zai-org/GLM-5.1-FP8");
        assert_eq!(meta.model_doc_link, "https://docs.near.ai/");
        assert!(!meta.setup_steps.is_empty());

        let api_key = meta
            .config_keys
            .iter()
            .find(|k| k.name == "NEARAI_API_KEY")
            .expect("NEARAI_API_KEY config key should exist");
        assert!(api_key.required, "NEARAI_API_KEY should be required");
        assert!(api_key.secret, "NEARAI_API_KEY should be secret");
        assert!(api_key.primary, "NEARAI_API_KEY should be primary");
    }

    #[tokio::test]
    async fn test_alibaba_declarative_provider_registry_wiring() {
        let alibaba = get_from_registry("alibaba")
            .await
            .expect("alibaba provider should be registered");
        let meta = alibaba.metadata();

        assert_eq!(alibaba.provider_type(), ProviderType::Declarative);
        assert!(alibaba.supports_inventory_refresh());
        assert_eq!(meta.display_name, "Alibaba (Qwen)");
        assert_eq!(meta.default_model, "qwen3.7-max");
        assert_eq!(
            meta.model_doc_link,
            "https://www.alibabacloud.com/help/en/model-studio/models"
        );
        assert!(!meta.setup_steps.is_empty());

        let api_key = meta
            .config_keys
            .iter()
            .find(|k| k.name == "DASHSCOPE_API_KEY")
            .expect("DASHSCOPE_API_KEY config key should exist");
        assert!(api_key.required, "DASHSCOPE_API_KEY should be required");
        assert!(api_key.secret, "DASHSCOPE_API_KEY should be secret");
        assert!(api_key.primary, "DASHSCOPE_API_KEY should be primary");
    }

    #[tokio::test]
    async fn test_openai_compatible_providers_config_keys() {
        let providers_list = providers().await;
        let required_api_key_cases = vec![
            ("groq", "GROQ_API_KEY"),
            ("mistral", "MISTRAL_API_KEY"),
            ("custom_deepseek", "DEEPSEEK_API_KEY"),
        ];
        for (name, expected_key) in required_api_key_cases {
            if let Some((meta, _)) = providers_list.iter().find(|(m, _)| m.name == name) {
                assert!(
                    !meta.config_keys.is_empty(),
                    "{name} provider should have config keys"
                );
                assert_eq!(
                    meta.config_keys[0].name, expected_key,
                    "First config key for {name} should be {expected_key}, got {}",
                    meta.config_keys[0].name
                );
                assert!(
                    meta.config_keys[0].required,
                    "{expected_key} should be required"
                );
                assert!(
                    meta.config_keys[0].secret,
                    "{expected_key} should be secret"
                );
            } else {
                // Provider not registered; skip test for this provider
                continue;
            }
        }

        if let Some((meta, _)) = providers_list.iter().find(|(m, _)| m.name == "openai") {
            assert!(
                !meta.config_keys.is_empty(),
                "openai provider should have config keys"
            );
            assert_eq!(
                meta.config_keys[0].name, "OPENAI_API_KEY",
                "First config key for openai should be OPENAI_API_KEY"
            );
            assert!(
                !meta.config_keys[0].required,
                "OPENAI_API_KEY should be optional for local server support"
            );
            assert!(
                meta.config_keys[0].secret,
                "OPENAI_API_KEY should be secret"
            );
        }
    }

    #[tokio::test]
    async fn test_custom_provider_context_limit_is_applied_from_file() {
        let _guard = env_lock::lock_env([("BHARATCODE_PATH_ROOT", None::<&str>)]);
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        std::env::set_var("BHARATCODE_PATH_ROOT", temp_dir.path());

        let custom_dir = Paths::config_dir().join("custom_providers");
        fs::create_dir_all(&custom_dir).expect("custom providers dir should be created");

        let custom_inf = r#"{
  "name": "custom_inf",
  "engine": "openai",
  "display_name": "Custom Inf",
  "description": "test provider",
  "api_key_env": "",
  "base_url": "https://example.invalid/v1/chat/completions",
  "models": [
    {"name": "kimi-k2.5", "context_limit": 256000}
  ],
  "requires_auth": false
}"#;
        fs::write(custom_dir.join("custom_inf.json"), custom_inf)
            .expect("custom_inf.json should be written");

        let custom_zero = r#"{
  "name": "custom_zero",
  "engine": "openai",
  "display_name": "Custom Zero",
  "description": "test provider",
  "api_key_env": "",
  "base_url": "https://example.invalid/v1/chat/completions",
  "models": [
    {"name": "zero-model", "context_limit": 0}
  ],
  "requires_auth": false
}"#;
        fs::write(custom_dir.join("custom_zero.json"), custom_zero)
            .expect("custom_zero.json should be written");

        refresh_custom_providers()
            .await
            .expect("custom providers should refresh");

        let provider = create_with_named_model("custom_inf", "kimi-k2.5", Vec::new())
            .await
            .expect("custom_inf provider should be creatable");
        assert_eq!(provider.get_model_config().context_limit, Some(256_000));

        let zero_provider = create_with_named_model("custom_zero", "zero-model", Vec::new())
            .await
            .expect("custom_zero provider should be creatable");
        assert_eq!(zero_provider.get_model_config().context_limit, None);

        std::env::remove_var("BHARATCODE_PATH_ROOT");
    }

    fn custom_provider_json(name: &str, model: &str) -> String {
        format!(
            r#"{{
  "name": "{name}",
  "engine": "openai",
  "display_name": "{name}",
  "description": "test provider",
  "api_key_env": "",
  "base_url": "https://example.invalid/v1/chat/completions",
  "models": [
    {{"name": "{model}", "context_limit": 128000}}
  ],
  "requires_auth": false
}}"#
        )
    }

    #[tokio::test]
    async fn test_failed_refresh_preserves_previously_loaded_custom_providers() {
        let _guard = env_lock::lock_env([("BHARATCODE_PATH_ROOT", None::<&str>)]);
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        std::env::set_var("BHARATCODE_PATH_ROOT", temp_dir.path());

        let custom_dir = Paths::config_dir().join("custom_providers");
        fs::create_dir_all(&custom_dir).expect("custom providers dir should be created");
        fs::write(
            custom_dir.join("custom_working.json"),
            custom_provider_json("custom_working", "working-model"),
        )
        .expect("custom_working.json should be written");

        refresh_custom_providers()
            .await
            .expect("custom providers should refresh");
        get_from_registry("custom_working")
            .await
            .expect("custom_working should be registered");

        fs::write(custom_dir.join("custom_broken.json"), "{ this is not json")
            .expect("custom_broken.json should be written");

        assert!(
            refresh_custom_providers().await.is_err(),
            "a malformed custom provider config should fail the refresh"
        );

        let preserved = get_from_registry("custom_working")
            .await
            .expect("custom_working should survive a failed refresh");
        assert_eq!(preserved.provider_type(), ProviderType::Custom);
        assert_eq!(preserved.metadata().default_model, "working-model");

        assert!(
            get_from_registry("custom_broken").await.is_err(),
            "the malformed provider should not be registered"
        );
        get_from_registry("anthropic")
            .await
            .expect("built-in providers should survive a failed refresh");

        std::env::remove_var("BHARATCODE_PATH_ROOT");
    }

    #[tokio::test]
    async fn test_successful_refresh_replaces_custom_providers_atomically() {
        let _guard = env_lock::lock_env([("BHARATCODE_PATH_ROOT", None::<&str>)]);
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        std::env::set_var("BHARATCODE_PATH_ROOT", temp_dir.path());

        let custom_dir = Paths::config_dir().join("custom_providers");
        fs::create_dir_all(&custom_dir).expect("custom providers dir should be created");
        fs::write(
            custom_dir.join("custom_before.json"),
            custom_provider_json("custom_before", "before-model"),
        )
        .expect("custom_before.json should be written");

        refresh_custom_providers()
            .await
            .expect("custom providers should refresh");
        get_from_registry("custom_before")
            .await
            .expect("custom_before should be registered");

        fs::remove_file(custom_dir.join("custom_before.json"))
            .expect("custom_before.json should be removed");
        fs::write(
            custom_dir.join("custom_after.json"),
            custom_provider_json("custom_after", "after-model"),
        )
        .expect("custom_after.json should be written");

        refresh_custom_providers()
            .await
            .expect("custom providers should refresh");

        let replacement = get_from_registry("custom_after")
            .await
            .expect("custom_after should replace the previous custom provider");
        assert_eq!(replacement.provider_type(), ProviderType::Custom);
        assert_eq!(replacement.metadata().default_model, "after-model");

        assert!(
            get_from_registry("custom_before").await.is_err(),
            "a custom provider whose config was deleted should be unregistered"
        );

        let providers_list = providers().await;
        assert!(
            providers_list
                .iter()
                .any(|(m, t)| m.name == "anthropic" && *t == ProviderType::Preferred),
            "built-in providers should be preserved across a refresh"
        );
        assert!(
            providers_list
                .iter()
                .any(|(m, t)| m.name == "tanzu_ai" && *t == ProviderType::Declarative),
            "fixed declarative providers should be preserved across a refresh"
        );

        std::env::remove_var("BHARATCODE_PATH_ROOT");
    }

    #[tokio::test]
    async fn test_refresh_rejects_custom_provider_that_shadows_builtin() {
        let _guard = env_lock::lock_env([("BHARATCODE_PATH_ROOT", None::<&str>)]);
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        std::env::set_var("BHARATCODE_PATH_ROOT", temp_dir.path());

        let anthropic = get_from_registry("anthropic")
            .await
            .expect("anthropic should initially be registered");
        assert_eq!(anthropic.provider_type(), ProviderType::Preferred);

        let custom_dir = Paths::config_dir().join("custom_providers");
        fs::create_dir_all(&custom_dir).expect("custom providers dir should be created");
        fs::write(
            custom_dir.join("anthropic.json"),
            custom_provider_json("anthropic", "shadow-model"),
        )
        .expect("anthropic.json should be written");

        assert!(
            refresh_custom_providers().await.is_err(),
            "a custom provider must not shadow a built-in provider"
        );

        let anthropic = get_from_registry("anthropic")
            .await
            .expect("anthropic should remain registered");
        assert_eq!(anthropic.provider_type(), ProviderType::Preferred);
        assert_ne!(anthropic.metadata().default_model, "shadow-model");

        std::env::remove_var("BHARATCODE_PATH_ROOT");
    }
}
