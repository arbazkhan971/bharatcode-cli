use anyhow::Result;
use futures::future::BoxFuture;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::acp::{
    extension_configs_to_mcp_servers, AcpProvider, AcpProviderConfig, ACP_CURRENT_MODEL,
};
use crate::config::search_path::SearchPaths;
use crate::config::{Config, GooseMode};
use crate::providers::base::{current_working_dir, ProviderDef, ProviderMetadata};
use bharatcode_providers::model::ModelConfig;

pub(crate) const COPILOT_ACP_PROVIDER_NAME: &str = "copilot-acp";
const COPILOT_ACP_DOC_URL: &str = "https://github.com/github/copilot-cli";
pub(crate) const COPILOT_ACP_BINARY: &str = "copilot";

const MODE_AGENT: &str = "https://agentclientprotocol.com/protocol/session-modes#agent";
const MODE_PLAN: &str = "https://agentclientprotocol.com/protocol/session-modes#plan";

pub struct CopilotAcpProvider;

impl ProviderDef for CopilotAcpProvider {
    type Provider = AcpProvider;

    fn metadata() -> ProviderMetadata {
        ProviderMetadata::new(
            COPILOT_ACP_PROVIDER_NAME,
            "GitHub Copilot CLI (ACP)",
            "Use bharatcode with your GitHub Copilot subscription via the Copilot CLI.",
            ACP_CURRENT_MODEL,
            vec![],
            COPILOT_ACP_DOC_URL,
            vec![],
        )
        .with_setup_steps(vec![
            "Install the Copilot CLI: `npm install -g @github/copilot`",
            "Run `copilot login` to authenticate with your GitHub account",
            "Add to your bharatcode config file (`~/.config/bharatcode/config.yaml` on macOS/Linux):\n  BHARATCODE_PROVIDER: copilot-acp\n  BHARATCODE_MODEL: current\n  copilot-acp_configured: true",
            "Restart bharatcode for changes to take effect",
        ])
    }

    fn from_env(
        model: ModelConfig,
        extensions: Vec<crate::config::ExtensionConfig>,
        tls_config: Option<crate::providers::api_client::TlsConfig>,
    ) -> BoxFuture<'static, Result<AcpProvider>> {
        Self::from_env_with_working_dir(model, extensions, current_working_dir(), tls_config)
    }

    fn from_env_with_working_dir(
        model: ModelConfig,
        extensions: Vec<crate::config::ExtensionConfig>,
        working_dir: PathBuf,
        _tls_config: Option<crate::providers::api_client::TlsConfig>,
    ) -> BoxFuture<'static, Result<AcpProvider>> {
        Box::pin(async move {
            let config = Config::global();
            // with_npm() includes npm global bin dir (desktop app PATH may not)
            let resolved_command = SearchPaths::builder()
                .with_npm()
                .resolve(COPILOT_ACP_BINARY)?;
            let goose_mode = config.get_bharatcode_mode().unwrap_or_default();

            let mut args = vec!["--acp".to_string()];
            if model.model_name != ACP_CURRENT_MODEL {
                args.push("--model".to_string());
                args.push(model.model_name.clone());
            }

            let mode_mapping = mode_mapping();

            let provider_config = AcpProviderConfig {
                command: resolved_command,
                args,
                env: vec![],
                env_remove: vec![],
                work_dir: working_dir,
                mcp_servers: extension_configs_to_mcp_servers(&extensions),
                session_mode_id: Some(mode_mapping[&goose_mode].clone()),
                mode_mapping,
                notification_callback: None,
            };

            let metadata = Self::metadata();
            AcpProvider::connect(metadata.name, model, goose_mode, provider_config).await
        })
    }
}

// Copilot modes are full protocol URIs.
// No approve-specific mode; permissions are handled separately.
fn mode_mapping() -> HashMap<GooseMode, String> {
    HashMap::from([
        (GooseMode::Auto, MODE_AGENT.to_string()),
        (GooseMode::Approve, MODE_AGENT.to_string()),
        (GooseMode::SmartApprove, MODE_AGENT.to_string()),
        (GooseMode::Chat, MODE_PLAN.to_string()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Copilot has no full-access session mode: every non-Chat mode is MODE_AGENT and
    /// approvals ride on the GooseMode handed to the provider, so the default mode is
    /// gated by SmartApprove rather than by the session mode id.
    #[test]
    fn test_default_mode_uses_agent_mode() {
        assert_eq!(mode_mapping()[&GooseMode::default()], MODE_AGENT);
        assert_eq!(mode_mapping()[&GooseMode::Chat], MODE_PLAN);
    }
}
