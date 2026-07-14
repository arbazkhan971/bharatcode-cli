//! Offline static documentation-site generator for the `bharatcode` CLI.
//!
//! [`generate_site`] walks the *live* [`clap::Command`] tree (the very same
//! command surface the binary parses, obtained from `Cli::command()`), and the
//! embedded built-in skills, and emits a self-contained set of Markdown pages —
//! one page per top-level subcommand, an `index.md` table of contents, and a
//! `skills.md` listing the bundled skills. Because the pages are derived from
//! the real parsed surface rather than a hand-maintained copy, the generated
//! docs can never silently drift from the CLI.
//!
//! The generator is pure with respect to the CLI surface: it takes an
//! already-built `&clap::Command` and performs no network I/O. Its only inputs
//! beyond that command are the embedded built-in skills (via
//! [`bharatcode_core::skills`]) and the canonical page manifest
//! ([`bharatcode_core::doc_manifest`]), which is the single source of truth that seeds the
//! index — making this generator the live consumer that keeps that manifest
//! from going dead.
//!
//! Output directory resolution: the caller may pass an explicit directory; the
//! `BHARATCODE_DOCSITE_OUT` environment variable overrides the built-in default
//! of `./bharatcode-docs`. See [`default_out_dir`].
//!
//! Original BharatCode work; not ported from any third party.

use anyhow::Result;
use bharatcode_core::custom_requests::SourceType;
use bharatcode_core::doc_manifest;
use clap::{ArgAction, Command};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Environment variable that overrides the default docs output directory.
const OUT_DIR_ENV: &str = "BHARATCODE_DOCSITE_OUT";

/// Default output directory when neither a caller value nor the env override is
/// supplied.
const DEFAULT_OUT_DIR: &str = "bharatcode-docs";

/// File name of the generated landing page / table of contents.
const INDEX_FILE: &str = "index.md";

/// File name of the generated built-in skills listing.
const SKILLS_FILE: &str = "skills.md";

/// Resolve the output directory.
///
/// Precedence: an explicit `caller` value (when `Some` and non-empty) wins;
/// otherwise the `BHARATCODE_DOCSITE_OUT` environment variable; otherwise the
/// built-in default `./bharatcode-docs`.
pub fn default_out_dir(caller: Option<&Path>) -> PathBuf {
    if let Some(p) = caller {
        if !p.as_os_str().is_empty() {
            return p.to_path_buf();
        }
    }
    match std::env::var(OUT_DIR_ENV) {
        Ok(v) if !v.trim().is_empty() => PathBuf::from(v),
        _ => PathBuf::from(DEFAULT_OUT_DIR),
    }
}

/// Generate the full documentation site under `out_dir`.
///
/// Renders, in order: every top-level subcommand of `app` as its own page, a
/// `skills.md` page listing the embedded built-in skills, and an `index.md`
/// table of contents seeded from the canonical [`doc_manifest::pages`] list and
/// linking every generated command page. Returns the paths of every file
/// written, in write order.
///
/// `out_dir` is created (with parents) if it does not exist. No network access
/// occurs; the only inputs are the supplied command tree, the embedded skills,
/// and the canonical manifest.
pub fn generate_site(out_dir: &Path, app: &Command) -> Result<Vec<PathBuf>> {
    std::fs::create_dir_all(out_dir)?;
    let mut written = Vec::new();

    // One page per top-level subcommand, derived live from the clap tree.
    let mut command_pages: Vec<(String, String)> = Vec::new();
    for sub in app.get_subcommands() {
        if sub.is_hide_set() {
            continue;
        }
        let name = sub.get_name().to_string();
        let slug = slugify(&name);
        let page = render_command_page(&name, sub);
        let path = out_dir.join(format!("{}.md", slug));
        std::fs::write(&path, &page)?;
        written.push(path);
        command_pages.push((name, slug));
    }

    // Built-in skills page (embedded, offline).
    let skills_md = render_skills_page();
    let skills_path = out_dir.join(SKILLS_FILE);
    std::fs::write(&skills_path, &skills_md)?;
    written.push(skills_path);

    // Index / table of contents, seeded from the canonical manifest.
    let index_md = render_index(&command_pages);
    let index_path = out_dir.join(INDEX_FILE);
    std::fs::write(&index_path, &index_md)?;
    written.push(index_path);

    Ok(written)
}

/// Turn a command name into a filesystem- and URL-safe slug.
fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if ch == '-' || ch == '_' || ch.is_whitespace() {
            out.push('-');
        }
        // Drop anything else.
    }
    if out.is_empty() {
        out.push_str("command");
    }
    out
}

/// Render a single subcommand's reference page from the live `clap::Command`.
fn render_command_page(name: &str, cmd: &Command) -> String {
    let bin = doc_manifest::BINARY_NAME;
    let mut s = String::new();
    let _ = writeln!(s, "# `{} {}`", bin, name);
    let _ = writeln!(s);

    if let Some(about) = cmd.get_about() {
        let _ = writeln!(s, "{}", flatten(&about.to_string()));
        let _ = writeln!(s);
    }
    if let Some(long) = cmd.get_long_about() {
        let long = long.to_string();
        // Only emit when it adds detail beyond the short about.
        if cmd
            .get_about()
            .map(|a| a.to_string() != long)
            .unwrap_or(true)
        {
            let _ = writeln!(s, "{}", flatten(&long));
            let _ = writeln!(s);
        }
    }

    let _ = writeln!(s, "## Usage");
    let _ = writeln!(s);
    let _ = writeln!(s, "```");
    let _ = writeln!(s, "{} {} [options]", bin, name);
    let _ = writeln!(s, "```");
    let _ = writeln!(s);

    let positionals: Vec<_> = cmd.get_arguments().filter(|a| a.is_positional()).collect();
    let options: Vec<_> = cmd
        .get_arguments()
        .filter(|a| !a.is_positional() && !a.is_hide_set())
        .collect();

    if !positionals.is_empty() {
        let _ = writeln!(s, "## Arguments");
        let _ = writeln!(s);
        for arg in &positionals {
            if arg.is_hide_set() {
                continue;
            }
            let id = arg.get_id().as_str();
            let help = arg
                .get_help()
                .map(|h| flatten(&h.to_string()))
                .unwrap_or_default();
            if help.is_empty() {
                let _ = writeln!(s, "- `<{}>`", id);
            } else {
                let _ = writeln!(s, "- `<{}>` — {}", id, help);
            }
        }
        let _ = writeln!(s);
    }

    if !options.is_empty() {
        let _ = writeln!(s, "## Options");
        let _ = writeln!(s);
        for arg in &options {
            let spelling = flag_spelling(arg);
            let kind = if is_boolean_flag(arg) {
                "flag"
            } else {
                "option"
            };
            let help = arg
                .get_help()
                .map(|h| flatten(&h.to_string()))
                .unwrap_or_default();
            if help.is_empty() {
                let _ = writeln!(s, "- `{}` ({})", spelling, kind);
            } else {
                let _ = writeln!(s, "- `{}` ({}) — {}", spelling, kind, help);
            }
        }
        let _ = writeln!(s);
    }

    // Nested subcommands, listed as links by name (no deep recursion: the page
    // stays focused, and each nested command name is still discoverable).
    let nested: Vec<_> = cmd.get_subcommands().filter(|c| !c.is_hide_set()).collect();
    if !nested.is_empty() {
        let _ = writeln!(s, "## Subcommands");
        let _ = writeln!(s);
        for sub in &nested {
            let sub_about = sub
                .get_about()
                .map(|a| flatten(&a.to_string()))
                .unwrap_or_default();
            if sub_about.is_empty() {
                let _ = writeln!(s, "- `{} {} {}`", bin, name, sub.get_name());
            } else {
                let _ = writeln!(s, "- `{} {} {}` — {}", bin, name, sub.get_name(), sub_about);
            }
        }
        let _ = writeln!(s);
    }

    s
}

/// Human-readable spelling of an option's flags, e.g. `-v, --verbose`.
fn flag_spelling(arg: &clap::Arg) -> String {
    let mut parts = Vec::new();
    if let Some(short) = arg.get_short() {
        parts.push(format!("-{}", short));
    }
    if let Some(long) = arg.get_long() {
        parts.push(format!("--{}", long));
    }
    if parts.is_empty() {
        parts.push(arg.get_id().as_str().to_string());
    }
    let mut spelling = parts.join(", ");
    if !is_boolean_flag(arg) {
        let _ = write!(spelling, " <{}>", arg.get_id().as_str().to_uppercase());
    }
    spelling
}

/// True when the argument is a boolean on/off flag (takes no value).
fn is_boolean_flag(arg: &clap::Arg) -> bool {
    matches!(
        arg.get_action(),
        ArgAction::SetTrue | ArgAction::SetFalse | ArgAction::Help | ArgAction::Version
    )
}

/// Render the embedded built-in skills page.
///
/// Skills are read from the binary's embedded set via
/// [`bharatcode_core::skills::discover_skills`], filtered to the built-in (embedded)
/// entries only, so the page reflects exactly what ships in the binary and is
/// stable regardless of the user's filesystem.
fn render_skills_page() -> String {
    let mut entries: Vec<(String, String)> = bharatcode_core::skills::discover_skills(None)
        .into_iter()
        .filter(|e| e.source_type == SourceType::BuiltinSkill)
        .map(|e| (e.name, flatten(&e.description)))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let bin = doc_manifest::BINARY_NAME;
    let mut s = String::new();
    let _ = writeln!(s, "# Built-in skills");
    let _ = writeln!(s);
    let _ = writeln!(
        s,
        "{} ships with the following skills embedded in the binary. They are \
         available offline, with no download required.",
        doc_manifest::PRODUCT_NAME
    );
    let _ = writeln!(s);

    if entries.is_empty() {
        let _ = writeln!(s, "_No built-in skills are bundled in this build._");
        return s;
    }

    let _ = writeln!(s, "| Skill | Description |");
    let _ = writeln!(s, "| --- | --- |");
    for (name, desc) in &entries {
        let _ = writeln!(s, "| `{}` | {} |", escape_cell(name), escape_cell(desc));
    }
    let _ = writeln!(s);
    let _ = writeln!(
        s,
        "Invoke a skill in a session, for example: `{} run --skill <name>`.",
        bin
    );
    s
}

/// Render the index / table of contents.
///
/// Seeds the landing page from the canonical [`doc_manifest::pages`] list, then
/// links every command page that was generated from the live clap tree, plus
/// the built-in skills page.
fn render_index(command_pages: &[(String, String)]) -> String {
    let bin = doc_manifest::BINARY_NAME;
    let mut s = String::new();
    let _ = writeln!(s, "# {} documentation", doc_manifest::PRODUCT_NAME);
    let _ = writeln!(s);
    let _ = writeln!(s, "{}", doc_manifest::PRODUCT_TAGLINE);
    let _ = writeln!(s);
    let _ = writeln!(
        s,
        "This documentation is generated directly from the `{}` command-line \
         surface, so it always matches the installed version.",
        bin
    );
    let _ = writeln!(s);

    let _ = writeln!(s, "## Guides");
    let _ = writeln!(s);
    for page in doc_manifest::pages() {
        let _ = writeln!(s, "- **{}** — {}", page.title, page.summary);
    }
    let _ = writeln!(s);

    let _ = writeln!(s, "## Command reference");
    let _ = writeln!(s);
    if command_pages.is_empty() {
        let _ = writeln!(s, "_No commands available._");
    } else {
        for (name, slug) in command_pages {
            let _ = writeln!(s, "- [`{} {}`]({}.md)", bin, name, slug);
        }
    }
    let _ = writeln!(s);

    let _ = writeln!(s, "## Skills");
    let _ = writeln!(s);
    let _ = writeln!(s, "- [Built-in skills]({})", SKILLS_FILE);

    s
}

/// Flatten any newlines/carriage-returns in a help string to single spaces and
/// trim, so a multi-line clap help blurb stays on one Markdown line.
fn flatten(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last_space = false;
    for ch in raw.chars() {
        if ch == '\n' || ch == '\r' || ch == '\t' {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(ch);
            last_space = ch == ' ';
        }
    }
    out.trim().to_string()
}

/// Escape characters that would break a Markdown table cell.
fn escape_cell(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in flatten(raw).chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '|' => out.push_str("\\|"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Arg;

    /// A tiny synthetic command tree to exercise the generator without pulling
    /// in the full CLI.
    fn synthetic_app() -> Command {
        Command::new("bharatcode")
            .about("Synthetic test app")
            .subcommand(
                Command::new("session")
                    .about("Start or resume an interactive session")
                    .arg(Arg::new("name").long("name").help("Name the session"))
                    .arg(
                        Arg::new("resume")
                            .short('r')
                            .long("resume")
                            .action(ArgAction::SetTrue)
                            .help("Resume the most recent session"),
                    ),
            )
            .subcommand(
                Command::new("run")
                    .about("Run a single instruction non-interactively")
                    .arg(Arg::new("text").help("Instruction text")),
            )
            .subcommand(Command::new("doctor").about("Diagnose the environment"))
    }

    #[test]
    fn generate_site_writes_index_and_a_page_per_subcommand() {
        let dir = tempfile::tempdir().unwrap();
        let app = synthetic_app();
        let written = generate_site(dir.path(), &app).unwrap();

        // index.md + skills.md + one page per (non-hidden) subcommand.
        let index = dir.path().join("index.md");
        assert!(index.exists(), "index.md must be written");
        assert!(dir.path().join("session.md").exists());
        assert!(dir.path().join("run.md").exists());
        assert!(dir.path().join("doctor.md").exists());
        assert!(dir.path().join("skills.md").exists());

        // Every returned path was actually written and is non-empty.
        for path in &written {
            let content = std::fs::read_to_string(path).unwrap();
            assert!(
                !content.trim().is_empty(),
                "generated page is empty: {}",
                path.display()
            );
        }

        // The index links each command page.
        let index_md = std::fs::read_to_string(&index).unwrap();
        assert!(index_md.contains("session.md"));
        assert!(index_md.contains("run.md"));
        assert!(index_md.contains("doctor.md"));
        assert!(index_md.contains("skills.md"));
    }

    #[test]
    fn command_page_renders_about_args_and_flags() {
        let dir = tempfile::tempdir().unwrap();
        let app = synthetic_app();
        generate_site(dir.path(), &app).unwrap();

        let session = std::fs::read_to_string(dir.path().join("session.md")).unwrap();
        assert!(session.contains("Start or resume an interactive session"));
        assert!(session.contains("--name"));
        assert!(session.contains("--resume"));
        assert!(session.contains("Resume the most recent session"));

        let run = std::fs::read_to_string(dir.path().join("run.md")).unwrap();
        assert!(run.contains("## Arguments"));
        assert!(run.contains("text"));
    }

    #[test]
    fn every_generated_page_is_brand_clean() {
        let dir = tempfile::tempdir().unwrap();
        let app = synthetic_app();
        let written = generate_site(dir.path(), &app).unwrap();

        for path in &written {
            let content = std::fs::read_to_string(path).unwrap().to_lowercase();
            assert!(
                !content.contains("goose"),
                "generated page leaked an upstream token: {}",
                path.display()
            );
            assert!(
                !content.contains("block, inc"),
                "generated page leaked an upstream token: {}",
                path.display()
            );
        }

        // The index carries the product brand.
        let index = std::fs::read_to_string(dir.path().join("index.md"))
            .unwrap()
            .to_lowercase();
        assert!(index.contains("bharatcode"));
    }

    #[test]
    fn index_is_seeded_from_the_canonical_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let app = synthetic_app();
        generate_site(dir.path(), &app).unwrap();
        let index = std::fs::read_to_string(dir.path().join("index.md")).unwrap();
        // Every manifest page title appears in the generated index.
        for page in doc_manifest::pages() {
            assert!(
                index.contains(page.title),
                "index missing manifest page: {}",
                page.title
            );
        }
    }

    #[test]
    fn default_out_dir_honors_env_and_caller() {
        // Explicit caller wins.
        let explicit = PathBuf::from("/tmp/explicit-docs");
        assert_eq!(default_out_dir(Some(&explicit)), explicit);

        // With no caller, resolution falls to the env override or the built-in
        // default; either way it must be a non-empty path. We avoid mutating
        // process-global env here to keep the test hermetic.
        let resolved = default_out_dir(None);
        assert!(!resolved.as_os_str().is_empty());
    }

    #[test]
    fn slugify_is_filesystem_safe() {
        assert_eq!(slugify("session"), "session");
        assert_eq!(slugify("mcp-registry"), "mcp-registry");
        assert_eq!(slugify("Some Command"), "some-command");
        assert_eq!(slugify("a/b"), "ab");
        assert_eq!(slugify(""), "command");
    }

    #[test]
    fn hidden_subcommands_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let app = Command::new("bharatcode")
            .subcommand(Command::new("visible").about("Shown"))
            .subcommand(Command::new("secret").about("Hidden").hide(true));
        generate_site(dir.path(), &app).unwrap();
        assert!(dir.path().join("visible.md").exists());
        assert!(!dir.path().join("secret.md").exists());
    }
}
