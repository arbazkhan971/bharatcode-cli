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
pub mod info;
pub mod plugin;
pub mod presets;
pub mod privacy;
pub mod project;
pub mod recipe;
pub mod recipes_library;
pub mod review;
pub mod review_cmd;
pub mod schedule;
pub mod session;
pub mod skills;
pub mod term;
#[cfg(feature = "tui")]
pub mod tui;
#[cfg(feature = "update")]
pub mod update;

// Re-export the `gen-docs` entry point so the documentation-draft command is
// reachable as crate API. The CLI dispatch lives in `cli.rs` (owned by a
// sibling in this wave) and wires `bharatcode gen-docs` to this handler.
pub use gen_docs::{doc_guide_section, handle_gen_docs, GenDocsOptions};
