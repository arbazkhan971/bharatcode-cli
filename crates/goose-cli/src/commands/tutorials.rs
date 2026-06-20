//! `bharatcode tutorial` — offline, embedded, locale-aware tutorials.
//!
//! A small, curated set of short walkthroughs that ship inside the binary. No
//! network access, no files written, no side effects: every tutorial is a
//! `&'static` slice of body lines compiled into the executable, so the listing
//! and each walkthrough render identically on a fresh install and in air-gapped
//! environments.
//!
//! Surfaces:
//!   * [`handle_tutorial`] — the `tutorial` subcommand entry point.
//!       * no argument            — list every tutorial's id + title (one per
//!         line), themed via [`crate::theme::heading`] / `muted`.
//!       * `--show <id>`          — print the chosen tutorial's body lines.
//!       * `--show <unknown-id>`  — print a localized "unknown tutorial" line
//!         followed by the listing, then return an error.
//!   * [`first_run_nudge`] — a one-line, localized pointer to these tutorials,
//!     shown the first time the CLI is run (no session database yet). It is
//!     wired into the session build path (`session/builder.rs`) and is
//!     suppressible with the `BHARATCODE_NO_NUDGE` environment variable, so
//!     default behaviour is unchanged for any established user.
//!   * [`list`] / [`show`] — pure helpers returning rendered text, reused by the
//!     nudge surface and easy to assert against in tests.
//!
//! Titles route through the i18n layer by key via [`title_of`], falling back to
//! the embedded `default_title` when the active locale has no entry for the key
//! (mirroring the `label` helper in `mcp_registry.rs` / `catalog.rs`). This keeps
//! the module self-contained: it needs no edits to the shared locale tables, and
//! a missing key degrades gracefully to readable English.
//!
//! Original BharatCode work; not ported from any third party.

use anyhow::Result;

/// A single embedded tutorial: a stable id, an i18n key for its title, an
/// English default title used when that key is absent, and the body rendered
/// when the tutorial is shown.
pub struct Tutorial {
    /// Stable, lowercase id used to select the tutorial on the command line.
    pub id: &'static str,
    /// i18n key looked up for the localized title; falls back to `default_title`.
    pub title_i18n_key: &'static str,
    /// English title used when `title_i18n_key` has no locale entry.
    pub default_title: &'static str,
    /// The walkthrough body, one entry per rendered line.
    pub body_lines: &'static [&'static str],
}

/// The curated set of embedded tutorials, in display order.
///
/// Each entry is a short, brand-neutral walkthrough that references real
/// BharatCode commands (`configure`, `cost`/`budget`, `privacy`,
/// `recipes-library`, `mcp-registry`). Every id is unique and every body is
/// non-empty (enforced by the unit tests).
pub const TUTORIALS: &[Tutorial] = &[
    Tutorial {
        id: "first-session",
        title_i18n_key: "tutorial.first_session.title",
        default_title: "Your first session",
        body_lines: &[
            "First session",
            "",
            "Welcome! This walks you through running your first interactive",
            "session from the terminal.",
            "",
            "1. Configure a provider and model so BharatCode can talk to an LLM:",
            "     bharatcode configure",
            "   Pick a provider, paste an API key (or point at a local model), and",
            "   choose a default model. Settings are saved to your local config.",
            "",
            "2. Start an interactive chat session:",
            "     bharatcode session",
            "   Type a request in plain language (English or Hindi). Ask it to read",
            "   a file, run a command, or explain code in the current directory.",
            "",
            "3. Resume your most recent session later:",
            "     bharatcode session --resume",
            "",
            "Tip: keep prompts concrete. \"Add input validation to handlers/auth.rs\"",
            "works better than \"make the code better\".",
        ],
    },
    Tutorial {
        id: "cost-and-budget",
        title_i18n_key: "tutorial.cost_and_budget.title",
        default_title: "Tracking cost and staying on budget",
        body_lines: &[
            "Cost and budget",
            "",
            "BharatCode records token usage per session so you can see what each",
            "run costs before the bill arrives.",
            "",
            "1. Review spend from the recorded usage ledger:",
            "     bharatcode cost",
            "   This summarizes input/output tokens and the estimated cost per",
            "   model, entirely from local data.",
            "",
            "2. Set a budget guardrail so a runaway session warns you early:",
            "     bharatcode budget",
            "   Define a per-session or daily ceiling; you are alerted as you",
            "   approach it instead of discovering the overage afterwards.",
            "",
            "Tip: a cheaper model for routine edits and a stronger model for hard",
            "reasoning is usually the best cost/quality trade-off.",
        ],
    },
    Tutorial {
        id: "privacy-mode",
        title_i18n_key: "tutorial.privacy_mode.title",
        default_title: "Working with privacy mode",
        body_lines: &[
            "Privacy mode",
            "",
            "When you work with sensitive code or regulated data, privacy mode",
            "reduces what leaves your machine and flags risky content.",
            "",
            "1. Inspect and toggle privacy settings:",
            "     bharatcode privacy",
            "   Review what is redacted, what telemetry is disabled, and which",
            "   local-only guards are active.",
            "",
            "2. Keep secrets out of prompts. Redaction helps, but the safest input",
            "   is the one you never paste: reference files by path and let the",
            "   agent read only what it needs.",
            "",
            "Tip: pair privacy mode with a local model when handling personal data",
            "such as Aadhaar, PAN, or UPI identifiers so nothing leaves the host.",
        ],
    },
    Tutorial {
        id: "recipes",
        title_i18n_key: "tutorial.recipes.title",
        default_title: "Reusable recipes",
        body_lines: &[
            "Recipes",
            "",
            "Recipes are reusable, parameterized task templates. Instead of typing",
            "the same multi-step prompt again, you save it once and replay it.",
            "",
            "1. Browse the bundled, India-focused template library:",
            "     bharatcode recipes-library",
            "   List curated starting points (for example UPI review, PII audits,",
            "   GST helpers, Indic localization).",
            "",
            "2. Print one template to inspect or save it:",
            "     bharatcode recipes-library --show <id>",
            "   Redirect the output to a YAML file, adjust the parameters, then run",
            "   it with the recipe/run commands.",
            "",
            "Tip: parameterize anything that changes between runs (paths, ticket",
            "ids, environments) so one recipe serves many tasks.",
        ],
    },
    Tutorial {
        id: "extensions",
        title_i18n_key: "tutorial.extensions.title",
        default_title: "Adding tools with extensions",
        body_lines: &[
            "Extensions",
            "",
            "Extensions give the agent new abilities by wiring in MCP servers:",
            "file access, Git, databases, web fetch, and India-specific helpers.",
            "",
            "1. Add or manage extensions interactively:",
            "     bharatcode configure",
            "   Choose \"Add Extension\" and follow the prompts to enable a built-in",
            "   or external MCP server.",
            "",
            "2. Browse the curated registry for a ready-to-paste config:",
            "     bharatcode mcp-registry list",
            "     bharatcode mcp-registry show <id>",
            "",
            "Tip: enable only the extensions a task needs. Fewer tools means a",
            "tighter, faster, and safer session.",
        ],
    },
];

const NUDGE_ENV: &str = "BHARATCODE_NO_NUDGE";

/// Resolve a tutorial's display title through the i18n layer, falling back to
/// its embedded `default_title` when the active locale has no entry for the key.
fn title_of(tutorial: &Tutorial) -> String {
    label(tutorial.title_i18n_key, tutorial.default_title)
}

/// Look up a user-facing label by key, falling back to `default` when the
/// active locale has no entry. The [`crate::tr!`] macro returns the key
/// unchanged on a miss, which we never want to surface to a user.
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// Find a single tutorial by its exact id.
pub fn get(id: &str) -> Option<&'static Tutorial> {
    TUTORIALS.iter().find(|t| t.id == id)
}

/// Render one tutorial's body as a single string by id, or `None` if unknown.
///
/// This is the pure, side-effect-free core that [`handle_tutorial`] prints and
/// the unit tests assert against.
pub fn render_one(id: &str) -> Option<String> {
    let tutorial = get(id)?;
    let mut out = String::new();
    out.push_str(&title_of(tutorial));
    out.push('\n');
    for line in tutorial.body_lines {
        out.push_str(line);
        out.push('\n');
    }
    Some(out)
}

/// Return the rendered body of the tutorial with this `id`, or `None` if no such
/// tutorial exists. Thin alias kept for the nudge/listing callers.
pub fn show(id: &str) -> Option<String> {
    render_one(id)
}

/// Build the listing text: every tutorial's id and resolved title, one per line.
///
/// Plain (un-themed) so it is easy to assert against in tests; [`handle_tutorial`]
/// paints the same content through the theme for interactive output.
pub fn list_text() -> String {
    let id_width = TUTORIALS.iter().map(|t| t.id.len()).max().unwrap_or(0);
    let mut out = String::new();
    for tutorial in TUTORIALS {
        out.push_str(&format!(
            "{:<width$}  {}\n",
            tutorial.id,
            title_of(tutorial),
            width = id_width,
        ));
    }
    out
}

/// Render the localized list of available tutorials with a header and a hint.
///
/// Each tutorial `id` is mentioned, so the list is a complete index of what
/// [`show`] / `--show` accept.
pub fn list() -> String {
    let header = label("tutorial.list_header", "BharatCode tutorials");
    let hint = label(
        "tutorial.footer",
        "Run 'tutorial --show <id>' to read a walkthrough.",
    );

    let mut out = String::new();
    out.push_str(&header);
    out.push('\n');
    out.push_str(&list_text());
    out.push_str(&hint);
    out
}

/// Print the themed listing of every tutorial (ids + titles).
fn print_listing() {
    println!();
    println!(
        "{}",
        crate::theme::heading(label("tutorial.title", "BharatCode tutorials"))
    );
    println!();

    let id_width = TUTORIALS
        .iter()
        .map(|t| t.id.len())
        .max()
        .unwrap_or(0)
        .max(8);
    for tutorial in TUTORIALS {
        println!(
            "  {}  {}",
            crate::theme::accent(format!("{:<width$}", tutorial.id, width = id_width)),
            title_of(tutorial),
        );
    }

    println!();
    println!(
        "{}",
        crate::theme::muted(label(
            "tutorial.footer",
            "Run 'tutorial --show <id>' to read a walkthrough."
        ))
    );
    println!();
}

/// Entry point for `bharatcode tutorial`.
///
/// * `None`             — print the listing of ids + titles.
/// * `Some(id)` known   — print that tutorial's body.
/// * `Some(id)` unknown — print a localized "unknown tutorial" line, then the
///   listing, and return an error.
pub fn handle_tutorial(show: Option<String>) -> Result<()> {
    match show {
        None => {
            print_listing();
            Ok(())
        }
        Some(id) => match render_one(&id) {
            Some(body) => {
                println!();
                print!("{}", body);
                println!();
                Ok(())
            }
            None => {
                println!();
                println!(
                    "{}",
                    crate::theme::warning(format!(
                        "{} '{}'",
                        label("tutorial.unknown", "Unknown tutorial"),
                        id
                    ))
                );
                print_listing();
                Err(anyhow::anyhow!(
                    "{} '{}'",
                    label("tutorial.unknown", "Unknown tutorial"),
                    id
                ))
            }
        },
    }
}

/// Whether this looks like the very first run of the CLI.
///
/// Heuristic: no session database has been created yet. The session store lives
/// at `<data_dir>/sessions/sessions.db`; if that file is absent, no session has
/// ever been started. Any I/O ambiguity resolves to "not first run" so we never
/// nag an established user.
fn is_first_run() -> bool {
    let db_path = goose::config::paths::Paths::data_dir()
        .join("sessions")
        .join("sessions.db");
    !db_path.exists()
}

/// One-line localized nudge pointing new users at the tutorials, shown only on a
/// first run.
///
/// Returns `None` when this is not a first run, or when the user has opted out
/// via the `BHARATCODE_NO_NUDGE` environment variable (any non-empty value).
pub fn first_run_nudge() -> Option<String> {
    if std::env::var(NUDGE_ENV)
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
    {
        return None;
    }
    if !is_first_run() {
        return None;
    }
    Some(label(
        "tutorial.nudge",
        "New here? Run 'bharatcode tutorial' for a quick-start guide.",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_lower_kebab(s: &str) -> bool {
        !s.is_empty()
            && !s.starts_with('-')
            && !s.ends_with('-')
            && !s.contains("--")
            && s.chars().all(|c| c.is_ascii_lowercase() || c == '-')
    }

    #[test]
    fn tutorials_is_non_empty() {
        assert!(!TUTORIALS.is_empty());
    }

    #[test]
    fn all_ids_are_unique_and_lower_kebab() {
        let mut seen = std::collections::HashSet::new();
        for tutorial in TUTORIALS {
            assert!(
                is_lower_kebab(tutorial.id),
                "tutorial id is not lower-kebab: {:?}",
                tutorial.id
            );
            assert!(seen.insert(tutorial.id), "duplicate id: {}", tutorial.id);
        }
    }

    #[test]
    fn every_body_is_non_empty() {
        for tutorial in TUTORIALS {
            assert!(
                !tutorial.body_lines.is_empty(),
                "tutorial '{}' has an empty body",
                tutorial.id
            );
            assert!(
                tutorial.body_lines.iter().any(|l| !l.trim().is_empty()),
                "tutorial '{}' has only blank body lines",
                tutorial.id
            );
        }
    }

    #[test]
    fn every_id_and_title_is_present() {
        for tutorial in TUTORIALS {
            assert!(!tutorial.id.trim().is_empty(), "an id is empty");
            assert!(
                !tutorial.default_title.trim().is_empty(),
                "tutorial '{}' has an empty default title",
                tutorial.id
            );
            assert!(
                !tutorial.title_i18n_key.trim().is_empty(),
                "tutorial '{}' has an empty i18n key",
                tutorial.id
            );
        }
    }

    #[test]
    fn no_donor_or_upstream_branding_leaks() {
        for tutorial in TUTORIALS {
            let mut fields = vec![tutorial.id, tutorial.default_title, tutorial.title_i18n_key];
            fields.extend(tutorial.body_lines.iter().copied());
            for field in fields {
                let lower = field.to_lowercase();
                assert!(
                    !lower.contains("goose"),
                    "tutorial '{}' leaks upstream brand in: {}",
                    tutorial.id,
                    field
                );
                assert!(
                    !lower.contains("block"),
                    "tutorial '{}' leaks upstream brand in: {}",
                    tutorial.id,
                    field
                );
            }
        }
    }

    #[test]
    fn render_one_known_id_returns_body() {
        let id = TUTORIALS[0].id;
        let body = render_one(id).expect("known id should render");
        assert!(!body.trim().is_empty());
        // The rendered body begins with the resolved title.
        assert!(body.starts_with(&title_of(&TUTORIALS[0])));
    }

    #[test]
    fn render_one_unknown_id_returns_none() {
        assert!(render_one("does-not-exist").is_none());
    }

    #[test]
    fn show_alias_matches_render_one() {
        let id = TUTORIALS[0].id;
        assert_eq!(show(id), render_one(id));
        assert!(show("nope").is_none());
    }

    #[test]
    fn list_text_contains_every_id() {
        let listing = list_text();
        for tutorial in TUTORIALS {
            assert!(
                listing.contains(tutorial.id),
                "listing is missing id: {}",
                tutorial.id
            );
        }
    }

    #[test]
    fn list_mentions_every_id() {
        let listed = list();
        for tutorial in TUTORIALS {
            assert!(
                listed.contains(tutorial.id),
                "list() is missing tutorial id: {:?}",
                tutorial.id
            );
        }
    }

    #[test]
    fn title_falls_back_to_default_when_key_missing() {
        // These embedded keys are not present in the shared locale tables, so
        // `title_of` must degrade to the embedded English default title.
        for tutorial in TUTORIALS {
            assert_eq!(
                title_of(tutorial),
                tutorial.default_title,
                "tutorial '{}' should fall back to its default title",
                tutorial.id
            );
        }
    }

    #[test]
    fn get_known_and_unknown_ids() {
        assert!(get("first-session").is_some());
        assert!(get("nope").is_none());
    }

    #[test]
    fn handle_tutorial_unknown_id_errors() {
        assert!(handle_tutorial(Some("does-not-exist".to_string())).is_err());
    }

    #[test]
    fn handle_tutorial_known_and_list_ok() {
        assert!(handle_tutorial(None).is_ok());
        assert!(handle_tutorial(Some(TUTORIALS[0].id.to_string())).is_ok());
    }

    #[test]
    fn nudge_suppressed_by_env() {
        std::env::set_var(NUDGE_ENV, "1");
        assert!(
            first_run_nudge().is_none(),
            "nudge must be None when {NUDGE_ENV} is set"
        );
        std::env::remove_var(NUDGE_ENV);
    }
}
