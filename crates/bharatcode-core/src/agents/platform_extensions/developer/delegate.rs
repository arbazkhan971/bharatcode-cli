//! Subagent delegation tool (BharatCode v41).
//!
//! A real `delegate` developer tool that lets the main agent hand off a bounded,
//! isolated sub-task to a fresh subagent and get back a single text result. It is
//! a thin, model-invoked entry point onto the existing subagent infrastructure:
//! it builds a one-shot [`Recipe`] from the supplied instructions, assembles a
//! [`TaskConfig`](crate::agents::subagent_task_config::TaskConfig) from the
//! parent session, and runs it through
//! [`run_subagent_task`](crate::agents::subagent_handler::run_subagent_task).
//!
//! Each delegated run is hard-capped: the per-run turn budget comes from
//! `BHARATCODE_SUBAGENT_MAX_TURNS` (read by `TaskConfig::new`) and can be lowered
//! further by the optional `max_turns` argument. The tool is opt-in purely by the
//! model choosing to invoke it; there is no separate feature flag, and default
//! agent behaviour is unchanged because the model simply never has to call it.

use crate::agents::extension::PlatformExtensionContext;
use crate::agents::subagent_handler::{run_subagent_task, SubagentRunParams};
use crate::agents::subagent_task_config::TaskConfig;
use crate::agents::{AgentConfig, GoosePlatform};
use crate::config::permission::PermissionManager;
use crate::config::{Config, GooseMode};
use crate::model_config::model_config_from_user_config;
use crate::recipe::Recipe;
use crate::session::SessionType;
use rmcp::model::{CallToolResult, Content};
use schemars::JsonSchema;
use serde::Deserialize;

/// Arguments accepted by the `delegate` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct DelegateParams {
    /// The self-contained instructions for the delegated sub-task. The subagent
    /// has no access to the parent conversation, so include all context it needs.
    pub instructions: String,
    /// Optional per-run turn cap for the subagent. When omitted, the budget from
    /// `BHARATCODE_SUBAGENT_MAX_TURNS` (or the built-in default) applies.
    #[serde(default)]
    pub max_turns: Option<usize>,
}

/// Model-invoked tool that runs a bounded sub-task on a fresh subagent.
pub struct DelegateTool {
    context: PlatformExtensionContext,
}

impl DelegateTool {
    pub fn new(context: PlatformExtensionContext) -> Self {
        Self { context }
    }

    /// Run the delegated sub-task and return its single text result.
    ///
    /// On success the subagent's final text is returned as a single text content
    /// block; any failure (missing session, no provider, recipe/build error, or a
    /// subagent execution error) is mapped to an error [`CallToolResult`].
    pub async fn delegate(&self, params: DelegateParams, session_id: &str) -> CallToolResult {
        match self.run(params, session_id).await {
            Ok(text) => CallToolResult::success(vec![Content::text(text).with_priority(0.0)]),
            Err(error) => {
                CallToolResult::error(vec![
                    Content::text(format!("Delegation failed: {error}")).with_priority(0.0)
                ])
            }
        }
    }

    async fn run(&self, params: DelegateParams, session_id: &str) -> Result<String, String> {
        let instructions = params.instructions.trim();
        if instructions.is_empty() {
            return Err("instructions cannot be empty".to_string());
        }
        if let Some(max) = params.max_turns {
            if max < 1 {
                return Err("max_turns must be at least 1".to_string());
            }
        }

        let session = self
            .context
            .session_manager
            .get_session(session_id, false)
            .await
            .map_err(|e| format!("failed to load parent session: {e}"))?;

        if session.session_type == SessionType::SubAgent {
            return Err("delegated tasks cannot spawn further delegations".to_string());
        }

        let provider = self.resolve_provider(&session).await?;

        let recipe = Recipe::builder()
            .version("1.0.0")
            .title("Delegated Task")
            .description("Ad-hoc delegated sub-task")
            .prompt(instructions)
            .build()
            .map_err(|e| format!("failed to build recipe: {e}"))?;

        let extensions = Vec::new();
        let task_config = TaskConfig::new(provider, &session.id, &session.working_dir, extensions)
            .with_max_turns(params.max_turns);

        // Subagents run in Auto mode: approval-gated modes would block on a
        // confirmation channel the parent never reads.
        let agent_config = AgentConfig::new(
            self.context.session_manager.clone(),
            PermissionManager::instance(),
            None,
            GooseMode::Auto,
            true,
            GoosePlatform::GooseCli,
        )
        .with_use_login_shell_path(self.context.use_login_shell_path);

        let subagent_session = self
            .context
            .session_manager
            .create_session(
                task_config.parent_working_dir.clone(),
                "Delegated task".to_string(),
                SessionType::SubAgent,
                GooseMode::Auto,
            )
            .await
            .map_err(|e| format!("failed to create subagent session: {e}"))?;

        run_subagent_task(SubagentRunParams {
            config: agent_config,
            recipe,
            task_config,
            return_last_only: true,
            session_id: subagent_session.id,
            cancellation_token: None,
            on_message: None,
            notification_tx: None,
        })
        .await
        .map_err(|e| e.to_string())
    }

    async fn resolve_provider(
        &self,
        session: &crate::session::Session,
    ) -> Result<std::sync::Arc<dyn crate::providers::base::Provider>, String> {
        let provider_name = Config::global()
            .get_param::<String>("BHARATCODE_SUBAGENT_PROVIDER")
            .ok()
            .or_else(|| session.provider_name.clone())
            .ok_or_else(|| "no provider configured for delegation".to_string())?;

        let model_config = match &session.model_config {
            Some(config) => config.clone(),
            None => model_config_from_user_config(&provider_name, "default")
                .map_err(|e| format!("failed to resolve model config: {e}"))?,
        };

        crate::providers::create(&provider_name, model_config, Vec::new())
            .await
            .map_err(|e| format!("failed to create provider: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::schema_for;

    #[test]
    fn delegate_params_defaults_max_turns_to_none() {
        let params: DelegateParams =
            serde_json::from_value(serde_json::json!({"instructions": "x"})).unwrap();
        assert_eq!(params.instructions, "x");
        assert_eq!(params.max_turns, None);
    }

    #[test]
    fn delegate_params_accepts_explicit_max_turns() {
        let params: DelegateParams =
            serde_json::from_value(serde_json::json!({"instructions": "x", "max_turns": 3}))
                .unwrap();
        assert_eq!(params.instructions, "x");
        assert_eq!(params.max_turns, Some(3));
    }

    #[test]
    fn delegate_params_schema_describes_instructions() {
        let schema = serde_json::to_string(&schema_for!(DelegateParams)).unwrap();
        assert!(schema.contains("instructions"));
        assert!(schema.contains("max_turns"));
    }
}
