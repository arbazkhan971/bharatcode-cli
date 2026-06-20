// Derived from OpenAI Codex `codex-apply-patch` (https://github.com/openai/codex),
// Apache-2.0, Copyright 2025 OpenAI. See LICENSES/LICENSE-codex and NOTICE.
//
// This crate vendors the pure parser/streaming-parser/seek-sequence core of
// Codex's `codex-apply-patch` crate and re-implements the patch-application step
// on top of synchronous `std::fs`, dropping Codex's async `ExecutorFileSystem`
// sandbox layer (codex-exec-server), `codex-utils-path-uri`, and the
// tree-sitter shell-heredoc detection (invocation.rs).

//! Streaming parser and applier for the `*** Begin Patch` apply-patch format.
//!
//! The entry point most callers want is [`apply_patch_to_disk`], which parses a
//! patch and applies every hunk to the filesystem rooted at a working directory.
//! For streaming use cases, [`StreamingPatchParser`] surfaces hunks as the patch
//! text arrives, and [`parse_patch`] parses a complete patch into [`Hunk`]s.

mod apply;
mod parser;
mod seek_sequence;
mod streaming_parser;

pub use apply::ApplyPatchError;
pub use apply::ApplySummary;
pub use apply::apply_hunks_to_disk;
pub use apply::apply_patch_to_disk;
pub use parser::Hunk;
pub use parser::ParseError;
pub use parser::UpdateFileChunk;
pub use parser::parse_patch;
pub use streaming_parser::StreamingPatchParser;

/// Both the raw PATCH argument to `apply_patch` as well as the PATCH argument
/// parsed into hunks.
#[derive(Debug, PartialEq)]
pub struct ApplyPatchArgs {
    pub patch: String,
    pub hunks: Vec<Hunk>,
    pub workdir: Option<String>,
    pub environment_id: Option<String>,
}
