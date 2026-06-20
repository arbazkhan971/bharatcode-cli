pub mod audit;
pub mod budget;
pub mod configure;
pub mod cost;
pub mod cost_ledger;
pub mod doctor;
pub mod doctor_checks;
pub mod gateway;
pub mod gen_docs;
pub mod git_helper;
pub mod help_index;
pub mod info;
pub mod mcp_registry;
pub mod plugin;
pub mod presets;
pub mod privacy;
pub mod project;
pub mod recipe;
pub mod recipe_share;
pub mod recipes_library;
pub mod review;
pub mod review_cmd;
pub mod schedule;
pub mod session;
pub mod skills;
pub mod term;
#[cfg(feature = "tui")]
pub mod tui;
pub mod tutorials;
#[cfg(feature = "update")]
pub mod update;

// Re-export the `gen-docs` entry point so the documentation-draft command is
// reachable as crate API. The CLI dispatch lives in `cli.rs` (owned by a
// sibling in this wave) and wires `bharatcode gen-docs` to this handler.
pub use gen_docs::{doc_guide_section, handle_gen_docs, GenDocsOptions};

// Re-export the `help-index` entry point so the localized, grouped command and
// feature-flag index is reachable as crate API. The `help_index` table is
// static and side-effect free; `help_index::help_footer_line` is invoked from
// the interactive `/help` footer in `session/input.rs` so the index is reachable
// in the running binary, and `handle_help_index` wires `bharatcode help-index`
// (text or `--json`) once `cli.rs`, owned by a sibling in this wave, dispatches
// to it. Both surfaces are read-only and leave default behavior unchanged.
pub use help_index::{handle_help_index, help_footer_line, HelpEntry, HelpIndexOptions};

// Re-export the `recipe-share` entry point so the recipe export/import bundle
// flow is reachable as crate API. The CLI dispatch lives in `cli.rs` (owned by
// a sibling in this wave) and wires `bharatcode recipe-share <export|import>` to
// this handler; `recipe_share::run` applies the `BHARATCODE_RECIPE_SHARE` opt-in
// gate so default behavior is unchanged.
pub use recipe_share::{export as recipe_share_export, import as recipe_share_import};
pub use recipe_share::{run as run_recipe_share, RecipeBundle};

// Re-export the curated `mcp-registry` entry point so the read-only MCP-server
// registry is reachable as crate API. The live CLI dispatch for
// `bharatcode mcp-registry [list|search|show]` lives in `cli.rs` (owned by a
// sibling in this wave), which calls `handle_mcp_registry` with the parsed
// `McpRegistryAction`; the listing is offline, embedded, and has no side
// effects, so default behavior is unchanged.
pub use mcp_registry::{handle_mcp_registry, McpRegistryAction};

// Re-export the interactive tutorials registry so the offline, embedded,
// locale-aware walkthroughs (getting-started, going-offline, controlling-cost,
// hindi-tamil-ui) are first-class crate API. `tutorials_list` enumerates the
// catalog and `tutorial` looks one up by id; the onboarding wizard consumes
// these at integration to list and open walkthroughs. The module is already live
// in the running binary: `session/builder.rs` path-includes `tutorials.rs` and
// calls `list()`/`show()` (for `BHARATCODE_TUTORIAL`) and `first_run_nudge()` on
// the session-build path, and the builtin `tutorials.md` skill surfaces the
// registry to the agent. Every tutorial is embedded and side-effect free, so
// default behavior is unchanged.
pub use tutorials::{catalog as tutorials_list, get as tutorial, Tutorial, TUTORIALS};
