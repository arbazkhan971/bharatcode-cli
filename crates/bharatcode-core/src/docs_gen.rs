//! Reproducible Markdown reference generator for the documentation site.
//!
//! This module turns two curated registries that mirror the *real* product
//! surface into a deterministic set of Markdown pages, so the docs site can be
//! rebuilt from source on every CI run instead of being hand-maintained (and
//! drifting away from the code):
//!
//!   * the `BHARATCODE_*` configuration knobs — the curated [`FLAGS`] table,
//!     covering the headline analytics / sandbox / offline / residency /
//!     telemetry / budget surface an operator actually sets; and
//!   * the top-level CLI subcommand names — mirrored from the CLI's `Command`
//!     enum in [`SUBCOMMANDS`].
//!
//! Everything here is pure: no network, no model, no clock. The only function
//! that touches the filesystem is [`write_site`], and it writes byte-identical
//! output on every run (stable ordering, no timestamps), which is exactly what
//! makes the emitted tree safe to check in and diff in CI.
//!
//! The public surface ([`render_reference`], [`write_site`]) is reachable as
//! `goose`-crate public API and is consumed by the docs CI step / the future
//! `bharatcode docs` command — that consumption is the live wire that keeps this
//! generator from being dead code.
//!
//! Brand-clean by construction: only internal `goose-*` crate identifiers ever
//! appear; no upstream donor product name is ever emitted into a generated page.
//!
//! Original BharatCode work; not ported from any third party.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Optional environment override for the output directory used by [`default_out_dir`].
/// When unset, generation targets the in-repo `docs/generated` directory.
pub const OUT_DIR_ENV: &str = "BHARATCODE_DOCS_OUT";

/// Default, in-repo output directory for the generated documentation tree.
pub const DEFAULT_OUT_DIR: &str = "docs/generated";

/// File name of the generated landing page.
pub const INDEX_FILE: &str = "index.md";
/// File name of the generated `BHARATCODE_*` flag reference.
pub const FLAGS_FILE: &str = "flags.md";
/// File name of the generated subcommand reference.
pub const COMMANDS_FILE: &str = "commands.md";

/// Curated `BHARATCODE_*` configuration knobs, each paired with a one-line
/// description. Covers the headline analytics / sandbox / offline / residency /
/// telemetry / budget surface an operator is most likely to set.
///
/// This is intentionally a hand-picked, readable subset of every `BHARATCODE_*`
/// key the binary reads — the goal is a usable reference, not an exhaustive
/// dump. Keep this table sorted by name so the rendered page stays byte-stable.
pub static FLAGS: &[(&str, &str)] = &[
    (
        "BHARATCODE_ANALYTICS",
        "Opt-in local usage analytics; nothing leaves the machine unless enabled.",
    ),
    (
        "BHARATCODE_AUDIT",
        "Append an immutable audit log of tool calls and approval decisions.",
    ),
    (
        "BHARATCODE_BUDGET_INR",
        "Per-session spend ceiling in Indian rupees; the run halts once exceeded.",
    ),
    (
        "BHARATCODE_LANG",
        "Interface language for user-facing strings (e.g. en, hi, ta, mr).",
    ),
    (
        "BHARATCODE_MODE",
        "Default tool-approval mode the agent runs in.",
    ),
    (
        "BHARATCODE_OFFLINE",
        "Force fully offline operation; refuse any outbound network egress.",
    ),
    (
        "BHARATCODE_PROVIDER",
        "Default model provider the agent connects to.",
    ),
    (
        "BHARATCODE_RESIDENCY",
        "Data-residency mode restricting which endpoints may be used.",
    ),
    (
        "BHARATCODE_SANDBOX",
        "Run shell and tool execution inside the restricted sandbox.",
    ),
    (
        "BHARATCODE_TELEMETRY_OFF",
        "Kill-switch that force-disables anonymous telemetry.",
    ),
];

/// Top-level CLI subcommand names, mirrored from the CLI `Command` enum.
///
/// Sourced from the real command surface so the generated reference lists every
/// shipped verb. Kept sorted for a stable, byte-identical render; hidden helper
/// verbs are intentionally excluded from the published reference.
pub static SUBCOMMANDS: &[&str] = &[
    "acp",
    "catalog",
    "completion",
    "configure",
    "cost",
    "db",
    "doctor",
    "gateway",
    "gen-docs",
    "gen-tests",
    "git",
    "info",
    "local-models",
    "mcp",
    "mcp-registry",
    "model-pack",
    "onboard",
    "plugin",
    "presets",
    "privacy",
    "project",
    "projects",
    "recipe",
    "recipes-library",
    "refactor",
    "review",
    "review-diff",
    "run",
    "schedule",
    "serve",
    "serve-sessions",
    "session",
    "skills",
    "term",
    "tui",
    "update",
    "welcome",
];

/// Resolve the output directory: the `BHARATCODE_DOCS_OUT` override when set and
/// non-empty, otherwise the in-repo [`DEFAULT_OUT_DIR`].
pub fn default_out_dir() -> PathBuf {
    match std::env::var(OUT_DIR_ENV) {
        Ok(v) if !v.trim().is_empty() => PathBuf::from(v),
        _ => PathBuf::from(DEFAULT_OUT_DIR),
    }
}

/// Escape the pipe and backslash characters that would otherwise break a
/// Markdown table cell; newlines are flattened to a space so a cell never spills
/// across rows.
fn escape_cell(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '|' => out.push_str("\\|"),
            '\n' | '\r' => out.push(' '),
            other => out.push(other),
        }
    }
    out
}

/// Render the landing page that links to the flag and command references.
fn render_index() -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# BharatCode documentation reference");
    let _ = writeln!(s);
    let _ = writeln!(
        s,
        "Generated reference for the `bharatcode` CLI, rebuilt from source."
    );
    let _ = writeln!(s);
    let _ = writeln!(s, "## Contents");
    let _ = writeln!(s);
    let _ = writeln!(s, "- [Configuration flags]({})", FLAGS_FILE);
    let _ = writeln!(s, "- [Command reference]({})", COMMANDS_FILE);
    let _ = writeln!(s);
    let _ = writeln!(
        s,
        "Every configuration flag is an environment variable prefixed with `BHARATCODE_`."
    );
    s
}

/// Render the `BHARATCODE_*` flag reference as a sorted Markdown table.
fn render_flags() -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# Configuration flags");
    let _ = writeln!(s);
    let _ = writeln!(
        s,
        "Configuration is supplied through `BHARATCODE_*` environment variables."
    );
    let _ = writeln!(s);
    let _ = writeln!(s, "| Flag | Description |");
    let _ = writeln!(s, "| --- | --- |");
    for (name, desc) in FLAGS {
        let _ = writeln!(
            s,
            "| `{}` | {} |",
            escape_cell(name.trim()),
            escape_cell(desc.trim()),
        );
    }
    s
}

/// Render the top-level subcommand reference as a sorted bullet list.
fn render_commands() -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# Command reference");
    let _ = writeln!(s);
    let _ = writeln!(
        s,
        "All commands are invoked as `bharatcode <command> [flags]`."
    );
    let _ = writeln!(s);
    let _ = writeln!(s, "## Commands");
    let _ = writeln!(s);
    for name in SUBCOMMANDS {
        let _ = writeln!(s, "- `bharatcode {}`", name);
    }
    s
}

/// Build the full Markdown reference: the flag table and the subcommand list in
/// one deterministic document. Pure — no I/O, no environment reads, no clock.
///
/// The returned string contains every curated `BHARATCODE_*` flag and every
/// top-level subcommand name, so a single call is enough to verify the generator
/// covers the whole surface.
pub fn render_reference() -> String {
    let mut s = String::new();
    s.push_str(&render_index());
    let _ = writeln!(s);
    s.push_str("---\n\n");
    s.push_str(&render_flags());
    let _ = writeln!(s);
    s.push_str("---\n\n");
    s.push_str(&render_commands());
    s
}

/// Render every page and write the deterministic documentation tree under
/// `out_dir`, returning the number of files written.
///
/// Writes [`INDEX_FILE`], [`FLAGS_FILE`] and [`COMMANDS_FILE`] (creating
/// `out_dir` and any missing parents). The output is byte-stable: re-running
/// with the same inputs overwrites each file with identical bytes, so a second
/// invocation is a no-op as far as the on-disk content is concerned.
pub fn write_site(out_dir: &Path) -> std::io::Result<usize> {
    std::fs::create_dir_all(out_dir)?;
    let pages: [(&str, String); 3] = [
        (INDEX_FILE, render_index()),
        (FLAGS_FILE, render_flags()),
        (COMMANDS_FILE, render_commands()),
    ];
    let mut written = 0usize;
    for (name, body) in &pages {
        std::fs::write(out_dir.join(name), body.as_bytes())?;
        written += 1;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_table_is_sorted_and_brand_clean() {
        let mut prev: Option<&str> = None;
        for (name, desc) in FLAGS {
            assert!(
                name.starts_with("BHARATCODE_"),
                "flag must be a BHARATCODE_* key: {name}"
            );
            assert!(!desc.trim().is_empty(), "{name} has empty description");
            if let Some(p) = prev {
                assert!(p < *name, "FLAGS must be sorted/unique: {p} then {name}");
            }
            prev = Some(name);
        }
    }

    #[test]
    fn subcommands_are_sorted_and_unique() {
        let mut prev: Option<&str> = None;
        for name in SUBCOMMANDS {
            assert!(!name.is_empty(), "subcommand name must not be empty");
            if let Some(p) = prev {
                assert!(
                    p < *name,
                    "SUBCOMMANDS must be sorted/unique: {p} then {name}"
                );
            }
            prev = Some(name);
        }
    }

    #[test]
    fn render_reference_covers_flags_and_a_subcommand() {
        let md = render_reference();

        // Sentinel flag from the curated configuration surface.
        assert!(
            md.contains("BHARATCODE_OFFLINE"),
            "reference must document the offline flag"
        );
        // Every curated flag appears.
        for (name, _desc) in FLAGS {
            assert!(md.contains(name), "reference missing flag {name}");
        }
        // Every top-level subcommand name appears.
        for name in SUBCOMMANDS {
            assert!(
                md.contains(&format!("`bharatcode {}`", name)),
                "reference missing subcommand {name}"
            );
        }
        // At least one concrete subcommand string (spec sentinel).
        assert!(md.contains("`bharatcode session`"));
    }

    #[test]
    fn no_user_facing_upstream_leak() {
        let md = render_reference().to_lowercase();
        // Only internal goose-* crate identifiers are allowed; no user-facing
        // upstream product token may surface in a generated page. The trailing
        // space guards against false positives on internal `goose-*` idents,
        // which never appear in rendered docs anyway.
        assert!(
            !md.contains("goose "),
            "rendered docs leaked an upstream token"
        );
        assert!(
            !md.contains("block, inc"),
            "rendered docs leaked an upstream token"
        );
        // The product name we DO expect.
        assert!(md.contains("bharatcode"));
    }

    #[test]
    fn render_reference_is_deterministic() {
        assert_eq!(render_reference(), render_reference());
    }

    #[test]
    fn escape_cell_neutralizes_pipes() {
        assert_eq!(escape_cell("a|b"), "a\\|b");
        assert_eq!(escape_cell("x\\y"), "x\\\\y");
        assert_eq!(escape_cell("one\ntwo"), "one two");
    }

    #[test]
    fn default_out_dir_honors_override() {
        std::env::remove_var(OUT_DIR_ENV);
        assert_eq!(default_out_dir(), PathBuf::from(DEFAULT_OUT_DIR));

        std::env::set_var(OUT_DIR_ENV, "/tmp/bharatcode-docs-xyz");
        assert_eq!(default_out_dir(), PathBuf::from("/tmp/bharatcode-docs-xyz"));
        std::env::remove_var(OUT_DIR_ENV);
    }

    #[test]
    fn write_site_writes_three_pages_and_is_byte_stable() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("generated");

        let count = write_site(&out).unwrap();
        assert!(count >= 3, "expected at least three pages, got {count}");

        let index = std::fs::read(out.join(INDEX_FILE)).unwrap();
        let flags = std::fs::read(out.join(FLAGS_FILE)).unwrap();
        let commands = std::fs::read(out.join(COMMANDS_FILE)).unwrap();

        // A second run produces byte-identical files (determinism).
        let count2 = write_site(&out).unwrap();
        assert_eq!(count, count2);
        assert_eq!(index, std::fs::read(out.join(INDEX_FILE)).unwrap());
        assert_eq!(flags, std::fs::read(out.join(FLAGS_FILE)).unwrap());
        assert_eq!(commands, std::fs::read(out.join(COMMANDS_FILE)).unwrap());

        // The rendered flag page documents the sentinel flag.
        let flags_str = String::from_utf8(flags).unwrap();
        assert!(flags_str.contains("BHARATCODE_OFFLINE"));
    }
}
