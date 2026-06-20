//! Offline documentation-site generator for the `bharatcode` CLI.
//!
//! This module is a *pure renderer*: given a captured description of every
//! `bharatcode` subcommand (name, summary, flags) and every `BHARATCODE_*`
//! configuration key (name, default, one-line doc), it emits deterministic,
//! byte-stable Markdown pages suitable for a static documentation site. There
//! are no network calls, no clock reads, and no environment reads in the render
//! path, so the same input always produces the same bytes — which is what makes
//! the output safe to check into a docs tree and diff in CI.
//!
//! The three renderers are independent and side-effect free:
//!   * [`render_index`]    — the landing page, embedding the release version.
//!   * [`render_commands`] — the CLI command reference, one `##` section per
//!                           command plus a bullet per flag.
//!   * [`render_config`]   — the `BHARATCODE_*` config-key index as a table,
//!                           one row per key with its default and doc.
//!
//! [`write_site`] is the only function that touches the filesystem. It writes
//! `index.md`, `commands.md` and `config.md` under the caller-supplied output
//! directory, each file written atomically (temp file + rename) so a partial
//! write never leaves a half-rendered page behind. The output directory is
//! supplied by the caller (the CLI docs target / release CI defaults it to
//! `docs/`); this module reads no environment variable of its own.
//!
//! A small [`CONFIG_KEYS`] seed table lists the headline `BHARATCODE_*` keys so
//! `config.md` is populated even when no live reflection of the config surface
//! is available to the caller.
//!
//! Original BharatCode work; not ported from any third party.

use std::fmt::Write as _;
use std::path::Path;

/// A single CLI command (or subcommand) to document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandDoc {
    /// Invocation name as the user types it, e.g. `session` or `mcp-registry`.
    pub name: String,
    /// One-line summary of what the command does.
    pub summary: String,
    /// Flags accepted by the command, rendered in the order given.
    pub flags: Vec<FlagDoc>,
}

/// A single flag belonging to a [`CommandDoc`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagDoc {
    /// Flag spelling as typed, e.g. `--resume` or `-v, --verbose`.
    pub flag: String,
    /// One-line description of the flag.
    pub doc: String,
}

/// A single `BHARATCODE_*` configuration key to document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigKeyDoc {
    /// Environment-variable name, e.g. `BHARATCODE_OFFLINE`.
    pub key: String,
    /// Default value when the key is unset (shown verbatim).
    pub default: String,
    /// One-line description of the key's effect.
    pub doc: String,
}

impl CommandDoc {
    /// Convenience constructor that accepts anything string-like.
    pub fn new(name: impl Into<String>, summary: impl Into<String>, flags: Vec<FlagDoc>) -> Self {
        Self {
            name: name.into(),
            summary: summary.into(),
            flags,
        }
    }
}

impl FlagDoc {
    /// Convenience constructor that accepts anything string-like.
    pub fn new(flag: impl Into<String>, doc: impl Into<String>) -> Self {
        Self {
            flag: flag.into(),
            doc: doc.into(),
        }
    }
}

impl ConfigKeyDoc {
    /// Convenience constructor that accepts anything string-like.
    pub fn new(key: impl Into<String>, default: impl Into<String>, doc: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            default: default.into(),
            doc: doc.into(),
        }
    }
}

/// File name of the rendered landing page.
pub const INDEX_FILE: &str = "index.md";
/// File name of the rendered command reference.
pub const COMMANDS_FILE: &str = "commands.md";
/// File name of the rendered config-key index.
pub const CONFIG_FILE: &str = "config.md";

/// Headline `BHARATCODE_*` configuration keys.
///
/// This seed lets `config.md` render a useful page even when the caller has no
/// live reflection of the configuration surface to hand. The list is curated,
/// stable, and sorted by key name so the rendered table is deterministic.
pub static CONFIG_KEYS: &[(&str, &str, &str)] = &[
    (
        "BHARATCODE_ANALYTICS",
        "off",
        "Opt-in local usage analytics; no data leaves the machine unless enabled.",
    ),
    (
        "BHARATCODE_AUDIT",
        "off",
        "Append an immutable audit log of tool calls and approvals.",
    ),
    (
        "BHARATCODE_BUDGET_INR",
        "0",
        "Per-session spend ceiling in Indian rupees; 0 disables the cap.",
    ),
    (
        "BHARATCODE_OFFLINE",
        "off",
        "Force fully offline operation; refuse any network egress.",
    ),
    (
        "BHARATCODE_RESIDENCY",
        "in",
        "Preferred data-residency region for hosted providers (e.g. `in`).",
    ),
    (
        "BHARATCODE_SANDBOX",
        "off",
        "Run shell and tool execution inside the restricted sandbox.",
    ),
];

/// Returns the seed [`CONFIG_KEYS`] as a vector of [`ConfigKeyDoc`].
///
/// Useful for callers that want to start from the headline keys and then merge
/// in additional reflected keys before rendering.
pub fn seed_config_keys() -> Vec<ConfigKeyDoc> {
    CONFIG_KEYS
        .iter()
        .map(|(key, default, doc)| ConfigKeyDoc::new(*key, *default, *doc))
        .collect()
}

/// Escape the pipe and backslash characters that would otherwise break a
/// Markdown table cell. Newlines are flattened to a single space so a cell
/// never spills across table rows.
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

/// Render the landing page, embedding the supplied release `version`.
///
/// The page is intentionally small: a title, the version line, and links to the
/// two reference pages. It contains no clock- or environment-derived content so
/// it renders identically on every machine.
pub fn render_index(version: &str) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# bharatcode documentation");
    let _ = writeln!(s);
    let _ = writeln!(s, "Reference documentation for the `bharatcode` CLI.");
    let _ = writeln!(s);
    let _ = writeln!(s, "- Version: `{}`", version.trim());
    let _ = writeln!(s);
    let _ = writeln!(s, "## Contents");
    let _ = writeln!(s);
    let _ = writeln!(s, "- [Command reference]({})", COMMANDS_FILE);
    let _ = writeln!(s, "- [Configuration keys]({})", CONFIG_FILE);
    let _ = writeln!(s);
    let _ = writeln!(
        s,
        "Every configuration key is an environment variable prefixed with `BHARATCODE_`."
    );
    s
}

/// Render the CLI command reference.
///
/// Each command becomes a `##` section carrying its summary and, when present,
/// a `Flags` sub-list with one bullet per flag. Commands are rendered in the
/// order supplied by the caller so the page reflects the caller's intended
/// grouping.
pub fn render_commands(cmds: &[CommandDoc]) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# Command reference");
    let _ = writeln!(s);
    let _ = writeln!(
        s,
        "All commands are invoked as `bharatcode <command> [flags]`."
    );
    let _ = writeln!(s);

    if cmds.is_empty() {
        let _ = writeln!(s, "_No commands documented._");
        return s;
    }

    for cmd in cmds {
        let _ = writeln!(s, "## `bharatcode {}`", cmd.name.trim());
        let _ = writeln!(s);
        let _ = writeln!(s, "{}", cmd.summary.trim());
        let _ = writeln!(s);
        if cmd.flags.is_empty() {
            let _ = writeln!(s, "_No flags._");
            let _ = writeln!(s);
        } else {
            let _ = writeln!(s, "Flags:");
            let _ = writeln!(s);
            for flag in &cmd.flags {
                let _ = writeln!(s, "- `{}` — {}", flag.flag.trim(), flag.doc.trim());
            }
            let _ = writeln!(s);
        }
    }
    s
}

/// Render the `BHARATCODE_*` configuration-key index as a Markdown table.
///
/// One row per key, in the order supplied by the caller. Each cell is escaped so
/// a default value or doc string containing a pipe never corrupts the table.
pub fn render_config(keys: &[ConfigKeyDoc]) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# Configuration keys");
    let _ = writeln!(s);
    let _ = writeln!(
        s,
        "Configuration is supplied through `BHARATCODE_*` environment variables."
    );
    let _ = writeln!(s);

    if keys.is_empty() {
        let _ = writeln!(s, "_No configuration keys documented._");
        return s;
    }

    let _ = writeln!(s, "| Key | Default | Description |");
    let _ = writeln!(s, "| --- | --- | --- |");
    for key in keys {
        let _ = writeln!(
            s,
            "| `{}` | `{}` | {} |",
            escape_cell(key.key.trim()),
            escape_cell(key.default.trim()),
            escape_cell(key.doc.trim()),
        );
    }
    s
}

/// Render all three pages and write them atomically under `out`.
///
/// Creates `out` (and any missing parents) if needed, then writes
/// `index.md`, `commands.md` and `config.md`. Each file is written to a sibling
/// temporary file and renamed into place, so a reader never observes a
/// partially written page. The output directory is supplied by the caller; this
/// function reads no environment variable.
pub fn write_site(
    out: &Path,
    version: &str,
    cmds: &[CommandDoc],
    keys: &[ConfigKeyDoc],
) -> std::io::Result<()> {
    std::fs::create_dir_all(out)?;
    write_atomic(&out.join(INDEX_FILE), render_index(version).as_bytes())?;
    write_atomic(&out.join(COMMANDS_FILE), render_commands(cmds).as_bytes())?;
    write_atomic(&out.join(CONFIG_FILE), render_config(keys).as_bytes())?;
    Ok(())
}

/// Write `bytes` to `path` atomically: write a sibling temp file, flush it, then
/// rename it over the destination.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;

    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "doc".to_string());
    // A process-unique temp name; the destination directory is the same as the
    // final file so the rename stays on one filesystem (atomic on POSIX).
    let tmp = dir.join(format!(".{}.{}.tmp", file_name, std::process::id()));

    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.flush()?;
        f.sync_all()?;
    }

    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            // Best-effort cleanup of the temp file on failure.
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_cmds() -> Vec<CommandDoc> {
        vec![
            CommandDoc::new(
                "session",
                "Start or resume an interactive session.",
                vec![
                    FlagDoc::new("--resume", "Resume the most recent session."),
                    FlagDoc::new("--name <n>", "Name the session."),
                ],
            ),
            CommandDoc::new(
                "run",
                "Run a single instruction non-interactively.",
                vec![FlagDoc::new("-t, --text <t>", "Instruction text to run.")],
            ),
        ]
    }

    #[test]
    fn render_commands_is_stable_and_complete() {
        let cmds = sample_cmds();
        let md = render_commands(&cmds);

        // Each command name appears, each under a `##` heading.
        assert!(md.contains("## `bharatcode session`"));
        assert!(md.contains("## `bharatcode run`"));
        // Every flag appears.
        assert!(md.contains("--resume"));
        assert!(md.contains("--name <n>"));
        assert!(md.contains("-t, --text <t>"));
        // Summaries appear.
        assert!(md.contains("Start or resume an interactive session."));
        assert!(md.contains("Run a single instruction non-interactively."));
        // There is at least one `##` heading.
        assert!(md.contains("## "));
    }

    #[test]
    fn render_config_emits_a_row_per_key_with_default() {
        let keys = seed_config_keys();
        let md = render_config(&keys);

        // Table header is present.
        assert!(md.contains("| Key | Default | Description |"));

        // Every seed key renders a row carrying its default.
        for (key, default, _doc) in CONFIG_KEYS {
            assert!(md.contains(&format!("`{}`", key)), "missing key {key}");
            assert!(
                md.contains(&format!("`{}`", default)),
                "missing default for {key}"
            );
        }

        // One data row per key (plus title, blurb, header, separator lines).
        let rows = md
            .lines()
            .filter(|l| l.starts_with("| `BHARATCODE_"))
            .count();
        assert_eq!(rows, CONFIG_KEYS.len());
    }

    #[test]
    fn render_index_embeds_version() {
        let md = render_index("9.5.0");
        assert!(md.contains("9.5.0"));
        assert!(md.contains(COMMANDS_FILE));
        assert!(md.contains(CONFIG_FILE));
    }

    #[test]
    fn no_upstream_user_facing_tokens_leak() {
        let cmds = sample_cmds();
        let keys = seed_config_keys();
        let full = format!(
            "{}\n{}\n{}",
            render_index("1.2.3"),
            render_commands(&cmds),
            render_config(&keys),
        );
        let lower = full.to_lowercase();
        assert!(
            !lower.contains("goose"),
            "rendered docs leaked an upstream goose token"
        );
        assert!(
            !lower.contains("block, inc"),
            "rendered docs leaked an upstream Block token"
        );
        // The product name we DO expect.
        assert!(lower.contains("bharatcode"));
    }

    #[test]
    fn renderers_are_deterministic() {
        let cmds = sample_cmds();
        let keys = seed_config_keys();

        assert_eq!(render_index("2.0.0"), render_index("2.0.0"));
        assert_eq!(render_commands(&cmds), render_commands(&cmds));
        assert_eq!(render_config(&keys), render_config(&keys));
    }

    #[test]
    fn escape_cell_neutralizes_pipes() {
        let key = ConfigKeyDoc::new("BHARATCODE_X", "a|b", "left | right");
        let md = render_config(std::slice::from_ref(&key));
        // The raw pipe inside a cell is escaped so the table stays one row wide.
        assert!(md.contains("a\\|b"));
        assert!(md.contains("left \\| right"));
        // Still exactly one data row.
        let rows = md
            .lines()
            .filter(|l| l.starts_with("| `BHARATCODE_"))
            .count();
        assert_eq!(rows, 1);
    }

    #[test]
    fn write_site_creates_three_pages_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("docs");
        let cmds = sample_cmds();
        let keys = seed_config_keys();

        write_site(&out, "3.1.4", &cmds, &keys).unwrap();

        let index = std::fs::read_to_string(out.join(INDEX_FILE)).unwrap();
        let commands = std::fs::read_to_string(out.join(COMMANDS_FILE)).unwrap();
        let config = std::fs::read_to_string(out.join(CONFIG_FILE)).unwrap();

        assert_eq!(index, render_index("3.1.4"));
        assert_eq!(commands, render_commands(&cmds));
        assert_eq!(config, render_config(&keys));

        // No leftover temp files in the output directory.
        let leftovers: Vec<_> = std::fs::read_dir(&out)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp files were left behind");

        // Writing again is idempotent (byte-stable output).
        write_site(&out, "3.1.4", &cmds, &keys).unwrap();
        let index2 = std::fs::read_to_string(out.join(INDEX_FILE)).unwrap();
        assert_eq!(index, index2);
    }
}
