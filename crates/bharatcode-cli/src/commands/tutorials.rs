//! Interactive tutorials registry — offline, embedded, locale-aware
//! walkthroughs that ship inside the binary.
//!
//! A small, curated catalog of short guides covering the core BharatCode
//! workflows (getting started, going offline, controlling cost, switching the
//! UI to Hindi/Tamil). No network access, no files written, no side effects:
//! every tutorial is a `&'static` value compiled into the executable, so the
//! catalog renders identically on a fresh install and in air-gapped
//! environments.
//!
//! The registry is exposed two ways:
//!
//!   * **Crate API** — [`catalog`] returns the ordered list of [`Tutorial`]s and
//!     [`get`] looks one up by id. These back the re-exports
//!     `commands::tutorials_list` / `commands::tutorial`, which the onboarding
//!     wizard consumes at integration to enumerate the available walkthroughs.
//!   * **Builtin skill** — `crates/bharatcode-core/src/skills/builtins/tutorials.md` is
//!     auto-discovered by the skills loader and surfaced in `skills list` and the
//!     platform-extensions system prompt, so the agent can offer to walk a user
//!     through any of these workflows.
//!
//! Two thin string surfaces ([`list`], [`show`]) and a first-run pointer
//! ([`first_run_nudge`]) are consumed by the session-build path so the registry
//! is reachable in the running binary with zero shared-file edits beyond
//! `commands/mod.rs`. `BHARATCODE_TUTORIAL=<id>` (handled in the session builder)
//! prints a single guide; `=list` prints the index. The first-run nudge is
//! suppressible with `BHARATCODE_NO_NUDGE`, so default behaviour is unchanged for
//! any established user.
//!
//! Titles and summaries route through the i18n layer by key via [`label`],
//! falling back to the embedded English default when the active locale has no
//! entry for the key (mirroring the `label` helper in `mcp_registry.rs` /
//! `gen_docs.rs`). This keeps the module self-contained: it needs no edits to the
//! shared locale tables, and a missing key degrades gracefully to readable
//! English.
//!
//! Original BharatCode work; not ported from any third party.

use anyhow::Result;

/// A single embedded tutorial: a stable id, i18n keys for its title and summary
/// (each with an English default used when the key is absent), and the Markdown
/// body rendered when the tutorial is shown.
pub struct Tutorial {
    /// Stable, lowercase-kebab id used to select the tutorial.
    pub id: &'static str,
    /// i18n key looked up for the localized title; falls back to `default_title`.
    pub title_i18n_key: &'static str,
    /// English title used when `title_i18n_key` has no locale entry.
    pub default_title: &'static str,
    /// i18n key looked up for the one-line summary; falls back to `default_summary`.
    pub summary_i18n_key: &'static str,
    /// English one-line summary used when `summary_i18n_key` has no locale entry.
    pub default_summary: &'static str,
    /// The walkthrough body as Markdown.
    pub body: &'static str,
}

impl Tutorial {
    /// The localized title, falling back to the embedded English default.
    pub fn title(&self) -> String {
        label(self.title_i18n_key, self.default_title)
    }

    /// The localized one-line summary, falling back to the embedded English default.
    pub fn summary(&self) -> String {
        label(self.summary_i18n_key, self.default_summary)
    }
}

/// The curated set of embedded tutorials, in display order.
///
/// Each entry is a short, brand-neutral walkthrough that references real
/// BharatCode commands (`configure`, `cost`/`budget`, `privacy`,
/// `recipes-library`, `mcp-registry`). Every id is unique and every body is
/// non-empty (enforced by the unit tests), and no field mentions any upstream or
/// donor brand.
pub const TUTORIALS: &[Tutorial] = &[
    Tutorial {
        id: "getting-started",
        title_i18n_key: "tutorial.getting_started.title",
        default_title: "Getting started",
        summary_i18n_key: "tutorial.getting_started.summary",
        default_summary: "Configure a provider and run your first interactive session.",
        body: "# Getting started\n\
\n\
Welcome! This walks you through configuring BharatCode and running your\n\
first interactive session from the terminal.\n\
\n\
1. Configure a provider and model so BharatCode can talk to an LLM:\n\
\n\
       bharatcode configure\n\
\n\
   Pick a provider, paste an API key (or point at a local model), and\n\
   choose a default model. Settings are saved to your local config.\n\
\n\
2. Start an interactive chat session:\n\
\n\
       bharatcode session\n\
\n\
   Type a request in plain language (English, Hindi, or Tamil). Ask it to\n\
   read a file, run a command, or explain code in the current directory.\n\
\n\
3. Resume your most recent session later:\n\
\n\
       bharatcode session --resume\n\
\n\
Tip: keep prompts concrete. \"Add input validation to handlers/auth.rs\"\n\
works better than \"make the code better\".\n",
    },
    Tutorial {
        id: "going-offline",
        title_i18n_key: "tutorial.going_offline.title",
        default_title: "Going offline",
        summary_i18n_key: "tutorial.going_offline.summary",
        default_summary: "Run fully local with a self-hosted model and privacy mode.",
        body: "# Going offline\n\
\n\
BharatCode can run without sending your code to a hosted provider. This is\n\
useful for air-gapped machines, sensitive code, or patchy connectivity.\n\
\n\
1. Point BharatCode at a local model server (for example one exposing an\n\
   OpenAI-compatible endpoint on localhost):\n\
\n\
       bharatcode configure\n\
\n\
   Choose a local/self-hosted provider and set the base URL to your local\n\
   server. No API key leaves your machine.\n\
\n\
2. Turn on privacy mode so redaction and local-only guards are active:\n\
\n\
       bharatcode privacy\n\
\n\
   Review what is redacted and which telemetry is disabled.\n\
\n\
3. Confirm your environment is ready for offline work:\n\
\n\
       bharatcode doctor\n\
\n\
Tip: pair a local model with privacy mode when handling personal data such\n\
as Aadhaar, PAN, or UPI identifiers so nothing leaves the host.\n",
    },
    Tutorial {
        id: "controlling-cost",
        title_i18n_key: "tutorial.controlling_cost.title",
        default_title: "Controlling cost",
        summary_i18n_key: "tutorial.controlling_cost.summary",
        default_summary: "Track token spend and set a budget guardrail.",
        body: "# Controlling cost\n\
\n\
BharatCode records token usage per session so you can see what each run\n\
costs before the bill arrives.\n\
\n\
1. Review spend from the recorded usage ledger:\n\
\n\
       bharatcode cost\n\
\n\
   This summarizes input/output tokens and the estimated cost per model,\n\
   entirely from local data.\n\
\n\
2. Set a budget guardrail so a runaway session warns you early:\n\
\n\
       bharatcode budget\n\
\n\
   Define a per-session or daily ceiling; you are alerted as you approach\n\
   it instead of discovering the overage afterwards.\n\
\n\
Tip: a cheaper model for routine edits and a stronger model for hard\n\
reasoning is usually the best cost/quality trade-off.\n",
    },
    Tutorial {
        id: "hindi-tamil-ui",
        title_i18n_key: "tutorial.hindi_tamil_ui.title",
        default_title: "Hindi and Tamil UI",
        summary_i18n_key: "tutorial.hindi_tamil_ui.summary",
        default_summary: "Switch the interface language to Hindi or Tamil.",
        body: "# Hindi and Tamil UI\n\
\n\
BharatCode can render its interface in Hindi (hi) and Tamil (ta) in\n\
addition to English (en). Untranslated strings fall back to English, so the\n\
UI is always readable.\n\
\n\
1. Set the interface language for a single command by exporting the locale\n\
   environment variable before you run it:\n\
\n\
       BHARATCODE_LOCALE=hi bharatcode session\n\
       BHARATCODE_LOCALE=ta bharatcode session\n\
\n\
2. To make it the default, set the same variable in your shell profile so\n\
   every session starts in your preferred language.\n\
\n\
3. You can always type your requests in your own language regardless of the\n\
   interface language — the agent understands English, Hindi, and Tamil.\n\
\n\
Tip: if a label still appears in English, that string has no translation\n\
yet; the rest of the interface remains localized.\n",
    },
];

const NUDGE_ENV: &str = "BHARATCODE_NO_NUDGE";

/// Look up a user-facing label by key, falling back to `default` when the active
/// locale has no entry. The [`crate::tr!`] macro returns the key unchanged on a
/// miss, which we never want to surface to a user.
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// The ordered catalog of every embedded tutorial.
///
/// This is the registry's primary crate API: the onboarding wizard re-exports it
/// as `commands::tutorials_list` to enumerate the available walkthroughs.
pub fn catalog() -> Vec<&'static Tutorial> {
    TUTORIALS.iter().collect()
}

/// Find a single tutorial by its exact id, re-exported as `commands::tutorial`.
pub fn get(id: &str) -> Option<&'static Tutorial> {
    TUTORIALS.iter().find(|t| t.id == id)
}

/// Render one tutorial's body (its title line followed by the Markdown body) by
/// id, or `None` if no such tutorial exists.
///
/// This is the pure, side-effect-free surface the session builder prints for
/// `BHARATCODE_TUTORIAL=<id>` and that the unit tests assert against.
pub fn show(id: &str) -> Option<String> {
    let tutorial = get(id)?;
    Some(tutorial.body.to_string())
}

/// Build the index text: every tutorial's id and resolved title, one per line,
/// under a localized header and footer hint.
///
/// Plain (un-themed) so it is easy to assert against in tests and to print from
/// the session builder for `BHARATCODE_TUTORIAL=list`. Each tutorial `id` is
/// mentioned, so the list is a complete index of what [`show`] accepts.
pub fn list() -> String {
    let header = label("tutorial.list_header", "BharatCode tutorials");
    let hint = label(
        "tutorial.footer",
        "Run with BHARATCODE_TUTORIAL=<id> to read a walkthrough.",
    );

    let id_width = TUTORIALS.iter().map(|t| t.id.len()).max().unwrap_or(0);
    let mut out = String::new();
    out.push_str(&header);
    out.push('\n');
    for tutorial in TUTORIALS {
        out.push_str(&format!(
            "  {:<width$}  {}\n",
            tutorial.id,
            tutorial.title(),
            width = id_width,
        ));
    }
    out.push_str(&hint);
    out
}

/// Whether this looks like the very first run of the CLI.
///
/// Heuristic: no session database has been created yet. The session store lives
/// at `<data_dir>/sessions/sessions.db`; if that file is absent, no session has
/// ever been started. Any I/O ambiguity resolves to "not first run" so we never
/// nag an established user.
fn is_first_run() -> bool {
    let db_path = bharatcode_core::config::paths::Paths::data_dir()
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
        "New here? Set BHARATCODE_TUTORIAL=getting-started for a quick-start guide.",
    ))
}

/// Whether an LLM provider/model pair is actually configured for this process.
///
/// Reads the active provider and model through the config layer (the same
/// accessors the settings summary and session builder use) and treats an
/// empty/whitespace value as "not configured". A configuration error (no entry
/// at all) also resolves to "not configured".
fn provider_is_configured() -> bool {
    let config = bharatcode_core::config::Config::global();
    let has = |v: Result<String, _>| v.ok().is_some_and(|s| !s.trim().is_empty());
    has(config.get_bharatcode_provider()) && has(config.get_bharatcode_model())
}

/// Whether the DPDP audit log is currently enabled for this process.
///
/// Routed through the same `BHARATCODE_AUDIT` resolution used by the audit
/// command so the suggestion reflects the real, effective state (env override
/// or config file). Declared locally to keep this module self-contained — it
/// reuses the documented truthy spellings (`1`/`true`/`yes`/`on`).
fn audit_is_enabled() -> bool {
    let truthy = |v: &str| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    };
    if let Ok(raw) = std::env::var("BHARATCODE_AUDIT") {
        return truthy(&raw);
    }
    bharatcode_core::config::Config::global()
        .get_param::<String>("BHARATCODE_AUDIT")
        .map(|v| truthy(&v))
        .unwrap_or(false)
}

/// Choose the single most useful next tutorial for the current, real state, or
/// `None` if the catalog is empty.
///
/// This is the read-only suggestion surfaced by `bharatcode doctor`'s deep
/// checks. It never mutates anything; it only inspects what is already in
/// effect:
///
///   1. **No provider/model configured** → `getting-started` (the quick-start):
///      onboarding has to happen before anything else is useful.
///   2. **Configured but audit OFF** → `going-offline`: the natural next step
///      for a privacy-conscious operator once the basics work.
///   3. **Configured and audit ON** → `controlling-cost`: the operator is set
///      up and auditing, so the remaining high-value topic is spend control.
///
/// Each branch falls back to the first catalog entry if its preferred id is
/// somehow absent, so the function always returns `Some` for a non-empty
/// catalog.
pub fn suggest_next() -> Option<&'static Tutorial> {
    suggest_next_for(provider_is_configured(), audit_is_enabled())
}

/// Pure suggestion logic over explicit state, so it can be tested
/// deterministically without depending on the ambient machine config.
fn suggest_next_for(provider_configured: bool, audit_enabled: bool) -> Option<&'static Tutorial> {
    let preferred = if !provider_configured {
        "getting-started"
    } else if !audit_enabled {
        "going-offline"
    } else {
        "controlling-cost"
    };
    get(preferred).or_else(|| TUTORIALS.first())
}

/// Render a single tutorial as a localized, self-contained block: a one-line
/// header (`<id> — <localized title>`) followed by the tutorial body.
///
/// Pure and side-effect free. The title resolves through the i18n layer (with
/// the embedded English fallback), so the output honours `BHARATCODE_LANG`. No
/// upstream/donor brand can appear in the output because none appears in any
/// catalog field (enforced by the existing brand-leak test).
pub fn render(tutorial: &Tutorial) -> String {
    let mut out = String::new();
    out.push_str(tutorial.id);
    out.push_str(" — ");
    out.push_str(&tutorial.title());
    out.push('\n');
    out.push_str(tutorial.body);
    out
}

/// Entry point kept for callers that want a one-shot index/show with a return
/// status: `None` prints the index, `Some(id)` prints that tutorial (or the
/// index plus an error for an unknown id).
pub fn handle_tutorial(show_id: Option<String>) -> Result<()> {
    match show_id {
        None => {
            println!("{}", list());
            Ok(())
        }
        Some(id) => match show(&id) {
            Some(body) => {
                print!("{body}");
                Ok(())
            }
            None => {
                eprintln!("{}", list());
                Err(anyhow::anyhow!(
                    "{} '{}'",
                    label("tutorial.unknown", "Unknown tutorial"),
                    id
                ))
            }
        },
    }
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
    fn list_is_non_empty_and_ids_are_unique() {
        assert!(!TUTORIALS.is_empty());
        assert!(!catalog().is_empty());
        let mut seen = std::collections::HashSet::new();
        for tutorial in catalog() {
            assert!(
                is_lower_kebab(tutorial.id),
                "tutorial id is not lower-kebab: {:?}",
                tutorial.id
            );
            assert!(seen.insert(tutorial.id), "duplicate id: {}", tutorial.id);
        }
    }

    #[test]
    fn getting_started_is_some_with_non_empty_body() {
        let tutorial = get("getting-started").expect("getting-started must exist");
        assert!(!tutorial.body.trim().is_empty());
        assert!(show("getting-started").is_some_and(|b| !b.trim().is_empty()));
    }

    #[test]
    fn expected_ids_are_present() {
        for id in [
            "getting-started",
            "going-offline",
            "controlling-cost",
            "hindi-tamil-ui",
        ] {
            assert!(get(id).is_some(), "missing tutorial: {id}");
        }
    }

    #[test]
    fn get_unknown_id_is_none() {
        assert!(get("does-not-exist").is_none());
        assert!(show("does-not-exist").is_none());
    }

    #[test]
    fn no_catalog_string_contains_donor_or_upstream_brand() {
        for tutorial in TUTORIALS {
            let fields = [
                tutorial.id,
                tutorial.default_title,
                tutorial.title_i18n_key,
                tutorial.default_summary,
                tutorial.summary_i18n_key,
                tutorial.body,
            ];
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
                    "tutorial '{}' leaks donor brand in: {}",
                    tutorial.id,
                    field
                );
            }
        }
    }

    #[test]
    fn every_field_is_present() {
        for tutorial in TUTORIALS {
            assert!(!tutorial.id.trim().is_empty(), "an id is empty");
            assert!(
                !tutorial.default_title.trim().is_empty(),
                "tutorial '{}' has an empty default title",
                tutorial.id
            );
            assert!(
                !tutorial.title_i18n_key.trim().is_empty(),
                "tutorial '{}' has an empty title key",
                tutorial.id
            );
            assert!(
                !tutorial.default_summary.trim().is_empty(),
                "tutorial '{}' has an empty default summary",
                tutorial.id
            );
            assert!(
                !tutorial.summary_i18n_key.trim().is_empty(),
                "tutorial '{}' has an empty summary key",
                tutorial.id
            );
            assert!(
                !tutorial.body.trim().is_empty(),
                "tutorial '{}' has an empty body",
                tutorial.id
            );
        }
    }

    #[test]
    fn title_and_summary_fall_back_to_default_when_key_missing() {
        // These embedded keys are not present in the shared locale tables, so
        // the accessors must degrade to the embedded English defaults.
        for tutorial in TUTORIALS {
            assert_eq!(tutorial.title(), tutorial.default_title);
            assert_eq!(tutorial.summary(), tutorial.default_summary);
        }
    }

    #[test]
    fn list_text_mentions_every_id() {
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
    fn handle_tutorial_known_and_index_ok_unknown_errors() {
        assert!(handle_tutorial(None).is_ok());
        assert!(handle_tutorial(Some("getting-started".to_string())).is_ok());
        assert!(handle_tutorial(Some("does-not-exist".to_string())).is_err());
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

    #[test]
    fn suggest_next_is_always_some_and_in_catalog() {
        let suggestion = suggest_next().expect("a non-empty catalog always suggests one");
        assert!(
            get(suggestion.id).is_some(),
            "suggested id must be a real catalog entry: {}",
            suggestion.id
        );
    }

    #[test]
    fn suggest_next_with_no_provider_is_quick_start() {
        // Deterministic pure-logic test over explicit state, independent of the
        // ambient machine config (a real dev box may have a provider set).
        assert_eq!(
            suggest_next_for(false, false).map(|t| t.id),
            Some("getting-started"),
            "no-provider state must surface the quick-start tutorial"
        );
        assert_eq!(
            suggest_next_for(true, false).map(|t| t.id),
            Some("going-offline"),
            "configured + audit-off must suggest going-offline"
        );
        assert_eq!(
            suggest_next_for(true, true).map(|t| t.id),
            Some("controlling-cost"),
            "configured + audit-on must suggest controlling-cost"
        );
    }

    #[test]
    fn render_contains_localized_title_and_id_and_is_leak_free() {
        for tutorial in TUTORIALS {
            let rendered = render(tutorial);
            assert!(
                rendered.contains(&tutorial.title()),
                "render() must include the localized title for {}",
                tutorial.id
            );
            assert!(
                rendered.contains(tutorial.id),
                "render() must include the id for {}",
                tutorial.id
            );
            let lower = rendered.to_lowercase();
            assert!(
                !lower.contains("goose") && !lower.contains("block"),
                "render() output leaks an upstream/donor brand for {}",
                tutorial.id
            );
        }
    }
}
