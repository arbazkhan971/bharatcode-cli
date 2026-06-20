mod acp_tooling;
pub mod amp_acp;
pub mod anthropic;
pub mod api_client {
    pub use goose_providers::api_client::*;
}
pub mod avian;
pub mod azure;
pub mod azureauth;
pub mod base;
#[cfg(feature = "aws-providers")]
pub mod bedrock;
pub mod canonical {
    pub use goose_providers::canonical::*;
}
mod catalog_util;
pub mod catalog {
    pub use super::catalog_util::*;
}
pub mod catalog_index;
pub mod chatgpt_codex;
pub mod claude_acp;
pub mod claude_code;
pub(crate) mod cli_common;
pub mod coalesce;
pub mod codex;
pub mod codex_acp;
pub mod copilot_acp;
pub mod cursor_agent;
pub mod databricks;
pub mod databricks_auth;
pub mod databricks_v2;
pub mod deadline;
pub mod embeddings;
pub mod fallback;
pub mod formats;
mod gcpauth;
pub mod gcpvertexai;
pub mod gemini_cli;
pub mod gemini_oauth;
pub mod githubcopilot;
pub mod google;
pub mod http_status;
pub mod huggingface;
pub mod huggingface_auth;
mod init;
pub mod inventory;
pub mod kimicode;
pub mod litellm;
#[cfg(feature = "local-inference")]
pub mod local_inference;
pub mod mcp_registry;
pub mod nanogpt;
pub mod oauth;
pub mod oauth_device_flow;
pub mod ollama;
pub mod openai;
pub mod openai_compatible;
pub mod openrouter;
// Perf-release runtime profile (v97): one named switch
// (`BHARATCODE_PERF_PROFILE`, default `balanced`) resolving to a clamped HTTP
// connection-pool / concurrency tuning bundle (`pool_max_idle`,
// `pool_idle_timeout_secs`, `max_concurrency`) plus per-knob overrides
// (`BHARATCODE_HTTP_POOL_MAX` / `BHARATCODE_HTTP_IDLE_SECS`). It is wired into
// the shared provider reqwest client built lazily in this module and consumed
// on the real `create()` provider-construction path (see
// `shared_provider_client` below): the `balanced` default produces a
// byte-identical conservative client, and only a non-default profile feeds
// `pool_max_idle_per_host` / `pool_idle_timeout` into the `ClientBuilder`.
pub mod perf_profile;
pub mod pi_acp;
// Security hardening self-audit (v94): a reachable, inert, read-only inspector
// that snapshots the effective security posture through each feature's own
// canonical accessor and renders a per-pillar report plus a 0..=100 hardening
// score. Mutates nothing, so default behaviour is unchanged. Reachable public
// API, same posture as the sibling `perf_profile` module.
pub mod planner_presets;
pub mod security_audit;
// Localized provider/model picker labels (v88): turns a raw provider id into a
// friendly, India-context display name + residency hint in the active regional
// locale. The module file lives at `src/provider_labels.rs`; it is wired in
// here (rather than via lib.rs) with `#[path]` so it is reachable from the
// running binary and importable by both the CLI `configure` provider picker and
// the planner-preset surface. English / unset-locale output is the raw id, so
// default behavior is unchanged.
#[path = "../provider_labels.rs"]
pub mod provider_labels;
pub use provider_labels::{display_label, display_label_active, residency_hint};
pub mod provider_registry;
pub mod provider_test;
mod retry {
    pub use goose_providers::retry::*;
}
#[cfg(feature = "aws-providers")]
pub mod sagemaker_tgi;
pub mod snowflake;
pub mod testprovider;
pub mod tetrate;
pub mod toolshim;
pub mod usage_estimator;
pub mod utils;

pub mod xai;
pub mod xai_oauth;

pub use coalesce::RequestCoalescer;
pub use embeddings::EmbeddingClient;
pub use init::{
    cleanup_provider, get_from_registry, inventory_identity, providers, refresh_custom_providers,
};
// `create*` are wrapped (not re-exported verbatim) so the perf-release profile
// is applied to the shared provider client on the real provider-construction
// path — see `shared_provider_client` and the wrappers below.
pub use create_entrypoints::{
    create, create_with_default_model, create_with_named_model, create_with_working_dir,
};
pub use retry::{retry_operation, RetryConfig};

/// The shared, lazily-built provider reqwest client whose connection pool is
/// tuned by the resolved [`perf_profile`].
///
/// On the default (`balanced`) profile the builder is left completely untouched,
/// so the client is byte-for-byte identical to reqwest's stock defaults and
/// behaviour is unchanged. Only a non-default profile feeds
/// `pool_max_idle_per_host` / `pool_idle_timeout` into the builder. Built once
/// and cached; the [`PerfProfile`] is resolved a single time at first use.
///
/// [`PerfProfile`]: perf_profile::PerfProfile
static SHARED_PROVIDER_CLIENT: std::sync::LazyLock<reqwest::Client> =
    std::sync::LazyLock::new(build_shared_provider_client);

/// Build the shared provider client, applying the resolved performance profile.
///
/// Reads [`perf_profile::resolve`] (pure config resolution; clamped values) and,
/// only when the profile diverges from the conservative `balanced` default,
/// applies the tuned `pool_max_idle_per_host` / `pool_idle_timeout` to the
/// `reqwest::ClientBuilder`. Falls back to the default client if a build ever
/// fails, so this can never break provider construction.
fn build_shared_provider_client() -> reqwest::Client {
    let profile = perf_profile::resolve();

    let mut builder = reqwest::Client::builder();
    if profile.diverges_from_default() {
        builder = builder
            .pool_max_idle_per_host(profile.pool_max_idle)
            .pool_idle_timeout(profile.pool_idle_timeout());
        tracing::debug!(
            profile = profile.profile().label(),
            pool_max_idle = profile.pool_max_idle,
            pool_idle_timeout_secs = profile.pool_idle_timeout_secs,
            max_concurrency = profile.max_concurrency,
            "applied perf-release runtime profile to shared provider client"
        );
    }

    builder.build().unwrap_or_default()
}

/// Accessor for the shared, perf-profile-tuned provider client.
///
/// Cloning a `reqwest::Client` is cheap (it is `Arc`-backed) and shares the same
/// underlying connection pool, so callers get the tuned pool for free.
pub fn shared_provider_client() -> reqwest::Client {
    SHARED_PROVIDER_CLIENT.clone()
}

/// Thin wrappers around the registry creation entry points that ensure the
/// perf-release profile is materialised on the real, binary-reachable
/// provider-construction path before any provider is built. Touching the shared
/// client here force-resolves [`perf_profile`] exactly once; on the default
/// profile this is a no-op beyond building the stock client.
mod create_entrypoints {
    use super::{init, shared_provider_client};
    use crate::config::ExtensionConfig;
    use anyhow::Result;
    use goose_providers::model::ModelConfig;
    use std::path::PathBuf;
    use std::sync::Arc;

    /// Materialise the shared, perf-tuned provider client (idempotent).
    ///
    /// The first call builds the client, which resolves [`super::perf_profile`]
    /// once and applies the profile to the `reqwest::ClientBuilder`; later calls
    /// just clone the cached client.
    #[inline]
    fn ensure_perf_profile_applied() {
        let _ = shared_provider_client();
    }

    pub async fn create(
        name: &str,
        model: ModelConfig,
        extensions: Vec<ExtensionConfig>,
    ) -> Result<Arc<dyn super::base::Provider>> {
        ensure_perf_profile_applied();
        init::create(name, model, extensions).await
    }

    pub async fn create_with_working_dir(
        name: &str,
        model: ModelConfig,
        extensions: Vec<ExtensionConfig>,
        working_dir: PathBuf,
    ) -> Result<Arc<dyn super::base::Provider>> {
        ensure_perf_profile_applied();
        init::create_with_working_dir(name, model, extensions, working_dir).await
    }

    pub async fn create_with_default_model(
        name: impl AsRef<str>,
        extensions: Vec<ExtensionConfig>,
    ) -> Result<Arc<dyn super::base::Provider>> {
        ensure_perf_profile_applied();
        init::create_with_default_model(name, extensions).await
    }

    pub async fn create_with_named_model(
        provider_name: &str,
        model_name: &str,
        extensions: Vec<ExtensionConfig>,
    ) -> Result<Arc<dyn super::base::Provider>> {
        ensure_perf_profile_applied();
        init::create_with_named_model(provider_name, model_name, extensions).await
    }
}
