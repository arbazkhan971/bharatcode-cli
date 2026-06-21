//! `bharatcode docs-gen` — a deterministic Markdown docs-set generator built
//! from BharatCode's own command and feature-flag metadata.
//!
//! Where the sibling [`crate::docsite`] generator walks the *live* `clap` command
//! tree, this module is a self-contained, pure-string generator over two embedded
//! static tables — [`COMMANDS`] (one row per subcommand with a one-line summary)
//! and [`FEATURE_FLAGS`] (the `BHARATCODE_*` opt-in gates, every one of which is
//! **default OFF**). It needs no parsed CLI, no filesystem, and no network to
//! produce its output, which makes it cheap to call from anywhere and trivial to
//! unit-test byte-for-byte.
//!
//! Three pure functions form the core API, each returning an owned `String`:
//!
//!   * [`cli_reference`] — a CLI command reference: a leading `# ` heading
//!     followed by one bullet per subcommand (`name` + summary).
//!   * [`feature_flag_table`] — a Markdown table of the `BHARATCODE_*` env gates,
//!     each rendered with its **`default: off`** posture so the opt-in contract is
//!     visible in the docs.
//!   * [`index_page`] — a static-site landing page that links the other two
//!     pages, suitable as the `index.md` of a docs site.
//!
//! [`write_site`] is the only function that touches the filesystem: it renders
//! the three pages plus a small README and writes them atomically (write to a
//! temp file in the same directory, then rename into place) so a second run over
//! the same directory is idempotent and never leaves a half-written file.
//!
//! Product and binary names route through [`bharatcode_core::doc_manifest`] so the brand
//! string stays in one place; nothing here emits any upstream brand token. The
//! page titles route through [`crate::tr!`] with English fallbacks so the docs
//! can be localized later while English output stays byte-stable.
//!
//! Original BharatCode work; not ported from any third party.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// A single subcommand row in the CLI reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandDoc {
    /// The canonical subcommand name, e.g. `doctor` or `mcp-registry`.
    pub name: &'static str,
    /// A one-line summary of what the subcommand does.
    pub summary: &'static str,
}

/// A single `BHARATCODE_*` feature-flag row.
///
/// Every gate in [`FEATURE_FLAGS`] is opt-in and OFF unless its environment
/// variable is set to a truthy value, so the rendered posture is always
/// `default: off`; [`DEFAULT_POSTURE`] is the single source of that label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatureFlagDoc {
    /// The `BHARATCODE_*` environment variable that enables the feature.
    pub env_var: &'static str,
    /// A one-line summary of what the gate turns on when set.
    pub summary: &'static str,
}

/// The rendered default-posture label shared by every feature-flag row. Kept as
/// a constant so the unit tests and the renderer agree on the exact wording.
pub const DEFAULT_POSTURE: &str = "default: off";

/// The static CLI command reference table.
///
/// Names mirror the canonical `bharatcode` subcommands dispatched in `cli.rs`;
/// summaries are concise and brand-clean. The list is intentionally curated
/// rather than reflected from the live clap tree so this generator stays pure and
/// deterministic.
pub static COMMANDS: &[CommandDoc] = &[
    CommandDoc {
        name: "session",
        summary: "Start, resume, list, or export an interactive coding session.",
    },
    CommandDoc {
        name: "run",
        summary: "Run a single instruction or recipe non-interactively.",
    },
    CommandDoc {
        name: "configure",
        summary: "Configure providers, models, and stored credentials.",
    },
    CommandDoc {
        name: "onboard",
        summary: "Guided first-run setup for a new workspace.",
    },
    CommandDoc {
        name: "doctor",
        summary: "Run environment and configuration health checks.",
    },
    CommandDoc {
        name: "cost",
        summary: "Show token cost, in USD and INR, for recent sessions.",
    },
    CommandDoc {
        name: "privacy",
        summary: "Show the resolved data-governance and privacy posture.",
    },
    CommandDoc {
        name: "presets",
        summary: "List recommended India and open-weight model presets.",
    },
    CommandDoc {
        name: "recipes-library",
        summary: "Browse the bundled recipe library.",
    },
    CommandDoc {
        name: "mcp-registry",
        summary: "List, search, or show entries in the curated MCP-server registry.",
    },
    CommandDoc {
        name: "gen-docs",
        summary: "Draft documentation for a source file in one pass.",
    },
    CommandDoc {
        name: "gen-tests",
        summary: "Draft unit tests for a source file in one pass.",
    },
    CommandDoc {
        name: "review-diff",
        summary: "Review the staged or working diff for issues.",
    },
    CommandDoc {
        name: "refactor",
        summary: "Propose a focused refactor of a source file.",
    },
    CommandDoc {
        name: "skills",
        summary: "List available skills, or enable skills by name.",
    },
    CommandDoc {
        name: "help-index",
        summary: "Show the grouped command and feature-flag index.",
    },
];

/// The static `BHARATCODE_*` feature-flag table.
///
/// Each entry is an opt-in gate read from the environment; all are OFF by
/// default so the binary's default behavior is unchanged unless a gate is set.
pub static FEATURE_FLAGS: &[FeatureFlagDoc] = &[
    FeatureFlagDoc {
        env_var: "BHARATCODE_BUDGET_INR",
        summary: "Enforce a per-session spend cap, in rupees.",
    },
    FeatureFlagDoc {
        env_var: "BHARATCODE_OFFLINE",
        summary: "Deny all network egress and require a local provider.",
    },
    FeatureFlagDoc {
        env_var: "BHARATCODE_RESIDENCY",
        summary: "Pin data residency to an approved region.",
    },
    FeatureFlagDoc {
        env_var: "BHARATCODE_REDACT",
        summary: "Redact secrets and PII before they leave the machine.",
    },
    FeatureFlagDoc {
        env_var: "BHARATCODE_AUDIT",
        summary: "Write a tamper-evident audit log of agent actions.",
    },
    FeatureFlagDoc {
        env_var: "BHARATCODE_CACHE",
        summary: "Reuse cached prompt prefixes to cut token spend.",
    },
    FeatureFlagDoc {
        env_var: "BHARATCODE_HELP_INDEX",
        summary: "Print the grouped command index at the start of a session.",
    },
];

/// File name of the generated CLI command reference page.
pub const CLI_REFERENCE_FILE: &str = "cli-reference.md";

/// File name of the generated feature-flag reference page.
pub const FEATURE_FLAGS_FILE: &str = "feature-flags.md";

/// File name of the generated landing page / table of contents.
pub const INDEX_FILE: &str = "index.md";

/// Look up a localized title, falling back to the English `default` when the
/// active locale table has no entry for `key`.
///
/// `tr!` echoes the key back when it is missing, so an unchanged key is treated
/// as "untranslated" and the English default is used. This keeps English output
/// byte-stable while allowing translations to land later without code changes.
fn title(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// Escape the characters that would break a Markdown table cell.
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

/// Render the CLI command reference page.
///
/// Emits a single leading `# ` heading followed by one Markdown bullet per
/// [`COMMANDS`] entry, in declaration order, each pairing the `bharatcode
/// <name>` invocation with its one-line summary. Pure and deterministic.
pub fn cli_reference() -> String {
    let bin = bharatcode_core::doc_manifest::BINARY_NAME;
    let heading = title("docs_gen.cli_reference.title", "CLI command reference");

    let mut out = String::new();
    out.push_str("# ");
    out.push_str(&heading);
    out.push_str("\n\n");
    out.push_str(&format!(
        "Every `{bin}` subcommand, with a one-line summary.\n\n",
    ));

    for cmd in COMMANDS {
        out.push_str(&format!("- `{bin} {}` — {}\n", cmd.name, cmd.summary));
    }

    out
}

/// Render the `BHARATCODE_*` feature-flag reference page.
///
/// Emits a leading `# ` heading and a Markdown table with one row per
/// [`FEATURE_FLAGS`] entry. Every row carries the [`DEFAULT_POSTURE`]
/// (`default: off`) label so the opt-in contract is explicit in the docs. Pure
/// and deterministic.
pub fn feature_flag_table() -> String {
    let heading = title("docs_gen.feature_flags.title", "Feature flags");

    let mut out = String::new();
    out.push_str("# ");
    out.push_str(&heading);
    out.push_str("\n\n");
    out.push_str(
        "These optional features are gated behind environment variables. Each is \
         off unless its variable is set to a truthy value, so default behavior is \
         unchanged.\n\n",
    );
    out.push_str("| Environment variable | Default | Description |\n");
    out.push_str("| --- | --- | --- |\n");

    for flag in FEATURE_FLAGS {
        out.push_str(&format!(
            "| `{}` | {} | {} |\n",
            escape_cell(flag.env_var),
            DEFAULT_POSTURE,
            escape_cell(flag.summary),
        ));
    }

    out
}

/// Render the docs-site landing page / table of contents.
///
/// Emits a leading `# ` product heading, the tagline, and a short links section
/// that points at the CLI reference and feature-flag pages by their on-disk file
/// names ([`CLI_REFERENCE_FILE`], [`FEATURE_FLAGS_FILE`]), so the page works as
/// the `index.md` of a static docs site. Pure and deterministic.
pub fn index_page() -> String {
    let product = bharatcode_core::doc_manifest::PRODUCT_NAME;
    let tagline = bharatcode_core::doc_manifest::PRODUCT_TAGLINE;
    let cli_title = title("docs_gen.cli_reference.title", "CLI command reference");
    let flags_title = title("docs_gen.feature_flags.title", "Feature flags");

    let mut out = String::new();
    out.push_str(&format!("# {product} documentation\n\n"));
    out.push_str(tagline);
    out.push_str("\n\n");
    out.push_str("## Contents\n\n");
    out.push_str(&format!("- [{cli_title}]({CLI_REFERENCE_FILE})\n"));
    out.push_str(&format!("- [{flags_title}]({FEATURE_FLAGS_FILE})\n"));

    out
}

/// Atomically write `contents` to `path`.
///
/// Writes to a temporary file in the same directory (so the rename stays on one
/// filesystem) and then renames it into place, which is atomic on POSIX. A
/// second run with identical contents therefore overwrites cleanly, leaving the
/// destination byte-identical — making [`write_site`] idempotent.
fn atomic_write(path: &Path, contents: &str) -> Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "docs".to_string());
    let tmp = dir.join(format!(".{file_name}.tmp"));

    {
        let mut f = std::fs::File::create(&tmp)
            .with_context(|| format!("creating temp file {}", tmp.display()))?;
        f.write_all(contents.as_bytes())
            .with_context(|| format!("writing temp file {}", tmp.display()))?;
        f.flush().ok();
    }

    std::fs::rename(&tmp, path)
        .with_context(|| format!("renaming {} into {}", tmp.display(), path.display()))?;
    Ok(())
}

/// Render the full docs set and write it under `dir`, returning the written
/// paths in write order.
///
/// Creates `dir` (with parents) if needed, then atomically writes the index,
/// CLI reference, and feature-flag pages. Because each page is pure and each
/// write is atomic, running this twice over the same directory is idempotent:
/// the second run reproduces byte-identical files. This is the only function in
/// the module that performs filesystem I/O.
pub fn write_site(dir: &Path) -> Result<Vec<PathBuf>> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating docs directory {}", dir.display()))?;

    let pages: [(&str, String); 3] = [
        (INDEX_FILE, index_page()),
        (CLI_REFERENCE_FILE, cli_reference()),
        (FEATURE_FLAGS_FILE, feature_flag_table()),
    ];

    let mut written = Vec::with_capacity(pages.len());
    for (name, contents) in &pages {
        let path = dir.join(name);
        atomic_write(&path, contents)?;
        written.push(path);
    }

    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_reference_has_heading_and_known_command() {
        let md = cli_reference();
        assert!(
            md.starts_with("# "),
            "cli_reference must start with a leading '# ' heading"
        );
        assert!(
            md.contains("doctor"),
            "cli_reference must list the 'doctor' subcommand"
        );
    }

    #[test]
    fn cli_reference_lists_every_command() {
        let md = cli_reference();
        for cmd in COMMANDS {
            assert!(
                md.contains(cmd.name),
                "cli_reference is missing command {}",
                cmd.name
            );
        }
    }

    #[test]
    fn feature_flag_table_shows_bharatcode_gate_default_off() {
        let md = feature_flag_table();
        assert!(
            md.contains("BHARATCODE_"),
            "feature_flag_table must mention a BHARATCODE_ gate"
        );
        assert!(
            md.contains(DEFAULT_POSTURE),
            "feature_flag_table must render '{DEFAULT_POSTURE}'"
        );
        // A specific known gate is present with its default-off posture.
        assert!(md.contains("BHARATCODE_AUDIT"));
    }

    #[test]
    fn feature_flag_table_renders_default_off_for_every_gate() {
        let md = feature_flag_table();
        let rows = md.matches(DEFAULT_POSTURE).count();
        assert_eq!(
            rows,
            FEATURE_FLAGS.len(),
            "every feature flag row must carry the default-off posture"
        );
        for flag in FEATURE_FLAGS {
            assert!(
                flag.env_var.starts_with("BHARATCODE_"),
                "env var does not use the BHARATCODE_ prefix: {}",
                flag.env_var
            );
        }
    }

    #[test]
    fn index_page_links_both_reference_pages() {
        let md = index_page();
        assert!(md.starts_with("# "), "index_page needs a leading heading");
        assert!(
            md.contains(CLI_REFERENCE_FILE),
            "index_page must link the CLI reference page"
        );
        assert!(
            md.contains(FEATURE_FLAGS_FILE),
            "index_page must link the feature-flags page"
        );
    }

    #[test]
    fn generated_strings_have_no_upstream_branding() {
        for md in [cli_reference(), feature_flag_table(), index_page()] {
            let lower = md.to_lowercase();
            assert!(!lower.contains("goose"), "generated docs leaked 'goose'");
            assert!(!lower.contains("block"), "generated docs leaked 'block'");
        }
    }

    #[test]
    fn write_site_creates_index_and_at_least_two_files() {
        let dir = tempfile::tempdir().unwrap();
        let written = write_site(dir.path()).unwrap();

        assert!(
            written.len() >= 2,
            "write_site must emit at least two files, got {}",
            written.len()
        );
        let index = dir.path().join(INDEX_FILE);
        assert!(index.exists(), "index.md must be written");
        assert!(dir.path().join(CLI_REFERENCE_FILE).exists());
        assert!(dir.path().join(FEATURE_FLAGS_FILE).exists());

        for path in &written {
            let content = std::fs::read_to_string(path).unwrap();
            assert!(
                !content.trim().is_empty(),
                "generated page is empty: {}",
                path.display()
            );
        }
    }

    #[test]
    fn write_site_is_idempotent_on_a_second_run() {
        let dir = tempfile::tempdir().unwrap();

        let first = write_site(dir.path()).unwrap();
        let first_contents: Vec<String> = first
            .iter()
            .map(|p| std::fs::read_to_string(p).unwrap())
            .collect();

        let second = write_site(dir.path()).unwrap();
        let second_contents: Vec<String> = second
            .iter()
            .map(|p| std::fs::read_to_string(p).unwrap())
            .collect();

        assert_eq!(first, second, "write_site path set must be stable");
        assert_eq!(
            first_contents, second_contents,
            "write_site must be idempotent: second run differs"
        );

        // No leftover temp files from the atomic-write path.
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "atomic write left a temp file behind");
    }
}
