#[cfg(all(feature = "rustls-tls", feature = "native-tls"))]
compile_error!("Features `rustls-tls` and `native-tls` are mutually exclusive");

pub mod acp;
pub use goose_sdk_types::{custom_notifications, custom_requests};
pub mod action_required_manager;
pub mod agents;
pub mod automation;
pub mod builtin_extension;
pub mod checks;
pub mod codebase_context;
pub mod config;
pub mod context_mgmt;
pub mod context_optimizer;
pub mod cost_routing;
pub mod conversation {
    pub use goose_providers::conversation::*;
}
pub mod dictation;
pub mod doctor;
pub mod download_manager;
pub mod elicitation;
pub mod exec_policy;
pub mod execution;
pub mod gateway;
pub mod goose_apps;
pub mod hints;
pub mod hooks;
pub mod instance_id;
pub mod logging;
pub mod mcp_utils;
pub mod memory_store;
pub mod model_config;
pub mod model_registry;
pub mod oauth;
pub mod offline;
#[cfg(feature = "otel")]
pub mod otel;
pub mod permission;
pub mod plugins;
#[cfg(feature = "telemetry")]
pub mod posthog;
pub mod prompt_cache;
pub mod prompt_template;
pub mod providers;
pub mod recipe;
pub mod recipe_deeplink;
pub mod residency;
pub mod scheduler;
pub mod scheduler_trait;
pub mod security;
pub mod session;
pub mod session_context;
pub mod skills;
pub mod slash_commands;
pub mod source_roots;
pub mod sources;
pub mod subagent_profiles;
pub mod subprocess;
pub mod token_counter;
pub mod tool_inspection;
pub mod tool_monitor;
pub mod tracing;
pub mod turn_checkpoint;
pub mod utils;
