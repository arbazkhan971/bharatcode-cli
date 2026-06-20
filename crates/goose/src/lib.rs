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
// BharatCode v95: canonical product/page manifest for the offline docs-site
// generator. The CLI's `docsite::generate_site` consumes
// `goose::doc_manifest::pages()` to seed the generated index, so this manifest
// is the single source of truth that the generator is driven by (its live wire).
pub mod doc_manifest;
// BharatCode v95: reproducible Markdown reference generator for the docs site.
// Exposed as goose-crate public API so the docs CI step / future `bharatcode
// docs` command can call `docs_gen::render_reference()` / `docs_gen::write_site`
// to rebuild `docs/generated` from source. The curated `BHARATCODE_*` flag table
// and the top-level subcommand list mirror the real product surface, so the
// generated reference is regenerable from code rather than hand-maintained.
pub mod docs_gen;
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
pub mod plugin_sdk;
pub mod plugins;
#[cfg(feature = "telemetry")]
pub mod posthog;
pub mod prompt_cache;
pub mod prompt_template;
pub mod providers;
// BharatCode v89: first-run quick-start splash + capability tour. The CLI
// session builder calls `quickstart::maybe_render` on the very first
// interactive launch; it returns `None` (default behavior unchanged) once the
// `.quickstart_shown` sentinel exists or when `BHARATCODE_NO_SPLASH` is set.
pub mod quickstart;
pub mod recipe;
pub mod recipe_deeplink;
// BharatCode v98: canonical 1.0 GA release identity. `release::resolve()` is the
// single source of truth for the product semantic version, the release channel
// (stable|beta|nightly, gated by `BHARATCODE_RELEASE_CHANNEL`), the compile-time
// build metadata, and the `release::ga_banner()` line. Exposed as reachable
// public API (`goose::release`, the product's `bharatcode::release` surface) so
// the version surface, `serve`, and update flows share one identity.
pub mod release;
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
