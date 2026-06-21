//! Structured, searchable in-app help index for BharatCode.
//!
//! A single, read-only catalog of every BharatCode subcommand and the headline
//! `BHARATCODE_*` environment toggles, each with a stable lookup `key`, an i18n
//! key for a localized one-line summary, and an English default summary used as
//! the fallback. The index is exposed as a library API so the interactive
//! slash-`/help` command and a future TUI can render the same data instead of
//! each maintaining its own hand-rolled list.
//!
//! This module is pure and side-effect free: it reads no files, touches no
//! network, and mutates nothing. It is additive — nothing in the existing CLI
//! changes behavior until a call site chooses to render the index.
//!
//! Usage:
//!
//! ```
//! use bharatcode_cli::help_index;
//!
//! // Full index, grouped Commands then Env, painted with the active theme.
//! let all = help_index::render(None);
//! assert!(all.contains("Commands"));
//!
//! // Filtered to entries mentioning "cost".
//! let filtered = help_index::render(Some("cost"));
//! assert!(filtered.to_lowercase().contains("cost"));
//! ```

use crate::theme;

/// Whether a help entry describes an invocable subcommand or an environment
/// toggle that changes behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A `bharatcode <name>` subcommand.
    Command,
    /// A `BHARATCODE_*` environment variable toggle.
    EnvToggle,
}

/// A single row in the help index.
#[derive(Debug, Clone, Copy)]
pub struct HelpEntry {
    /// Stable, user-facing identifier (the subcommand name or the env var name).
    pub name: &'static str,
    /// Whether this is a command or an environment toggle.
    pub kind: Kind,
    /// Machine-readable lookup key (kebab-case for commands, the literal env
    /// var name for toggles). Distinct from `name` only when a command's
    /// invocation token differs from its display name; today they match.
    pub key: &'static str,
    /// i18n key used to look up the localized summary via [`crate::tr!`].
    pub summary_i18n_key: &'static str,
    /// English one-line summary, used as the fallback when no translation is
    /// registered for `summary_i18n_key`.
    pub default_summary: &'static str,
}

impl HelpEntry {
    /// The localized one-line summary, falling back to [`Self::default_summary`]
    /// when the i18n table has no (or an empty) entry for this key.
    pub fn summary(&self) -> String {
        let translated = crate::tr!(self.summary_i18n_key);
        // `tr!` returns the key itself when no translation exists; treat that
        // (and any empty value) as "no translation" and use the default.
        if translated.is_empty() || translated == self.summary_i18n_key {
            self.default_summary.to_string()
        } else {
            translated
        }
    }
}

/// The full, ordered help index: real BharatCode subcommands first, then the
/// headline `BHARATCODE_*` environment toggles.
///
/// Summaries are intentionally vendor-neutral — they never mention the upstream
/// project — and each describes a real, shipping surface of the CLI.
pub const ENTRIES: &[HelpEntry] = &[
    // ---- Commands ----
    HelpEntry {
        name: "configure",
        kind: Kind::Command,
        key: "configure",
        summary_i18n_key: "help_index.cmd.configure",
        default_summary: "Set up providers, models, and bharatcode settings.",
    },
    HelpEntry {
        name: "doctor",
        kind: Kind::Command,
        key: "doctor",
        summary_i18n_key: "help_index.cmd.doctor",
        default_summary: "Run environment health checks and surface fixes.",
    },
    HelpEntry {
        name: "cost",
        kind: Kind::Command,
        key: "cost",
        summary_i18n_key: "help_index.cmd.cost",
        default_summary: "Show recorded LLM spend per session, in USD and INR.",
    },
    HelpEntry {
        name: "privacy",
        kind: Kind::Command,
        key: "privacy",
        summary_i18n_key: "help_index.cmd.privacy",
        default_summary: "Report the resolved data-governance and privacy posture.",
    },
    HelpEntry {
        name: "catalog",
        kind: Kind::Command,
        key: "catalog",
        summary_i18n_key: "help_index.cmd.catalog",
        default_summary: "Browse a curated, offline catalog of installable extensions.",
    },
    HelpEntry {
        name: "db",
        kind: Kind::Command,
        key: "db",
        summary_i18n_key: "help_index.cmd.db",
        default_summary: "Inspect and optionally compact the session database.",
    },
    HelpEntry {
        name: "recipes-library",
        kind: Kind::Command,
        key: "recipes-library",
        summary_i18n_key: "help_index.cmd.recipes_library",
        default_summary: "Browse the India developer recipe template library.",
    },
    HelpEntry {
        name: "refactor",
        kind: Kind::Command,
        key: "refactor",
        summary_i18n_key: "help_index.cmd.refactor",
        default_summary: "Apply a guided, agent-assisted refactor to a file or path.",
    },
    HelpEntry {
        name: "gen-tests",
        kind: Kind::Command,
        key: "gen-tests",
        summary_i18n_key: "help_index.cmd.gen_tests",
        default_summary: "Generate unit tests for a file or directory.",
    },
    HelpEntry {
        name: "gen-docs",
        kind: Kind::Command,
        key: "gen-docs",
        summary_i18n_key: "help_index.cmd.gen_docs",
        default_summary: "Generate or update documentation for source code.",
    },
    HelpEntry {
        name: "review-diff",
        kind: Kind::Command,
        key: "review-diff",
        summary_i18n_key: "help_index.cmd.review_diff",
        default_summary: "Review a working-tree or staged diff and flag issues.",
    },
    // ---- Environment toggles ----
    HelpEntry {
        name: "BHARATCODE_THEME",
        kind: Kind::EnvToggle,
        key: "BHARATCODE_THEME",
        summary_i18n_key: "help_index.env.theme",
        default_summary: "Select the CLI color theme (default, tiranga, or none).",
    },
    HelpEntry {
        name: "BHARATCODE_LANG",
        kind: Kind::EnvToggle,
        key: "BHARATCODE_LANG",
        summary_i18n_key: "help_index.env.lang",
        default_summary: "Choose the interface language for user-facing strings (en, hi).",
    },
    HelpEntry {
        name: "BHARATCODE_USD_INR",
        kind: Kind::EnvToggle,
        key: "BHARATCODE_USD_INR",
        summary_i18n_key: "help_index.env.usd_inr",
        default_summary: "Override the USD-to-INR rate used to display cost in rupees.",
    },
    HelpEntry {
        name: "BHARATCODE_BUDGET_INR",
        kind: Kind::EnvToggle,
        key: "BHARATCODE_BUDGET_INR",
        summary_i18n_key: "help_index.env.budget_inr",
        default_summary: "Set a per-session spend budget, in INR, for warnings.",
    },
    HelpEntry {
        name: "BHARATCODE_COST_EXTENSIONS",
        kind: Kind::EnvToggle,
        key: "BHARATCODE_COST_EXTENSIONS",
        summary_i18n_key: "help_index.env.cost_extensions",
        default_summary: "Append an extensions-in-use footer to the cost report.",
    },
    HelpEntry {
        name: "BHARATCODE_OFFLINE",
        kind: Kind::EnvToggle,
        key: "BHARATCODE_OFFLINE",
        summary_i18n_key: "help_index.env.offline",
        default_summary: "Force offline mode and skip network-dependent features.",
    },
    HelpEntry {
        name: "BHARATCODE_KEYS",
        kind: Kind::EnvToggle,
        key: "BHARATCODE_KEYS",
        summary_i18n_key: "help_index.env.keys",
        default_summary: "Override interactive editor keybindings (action=key pairs).",
    },
    HelpEntry {
        name: "BHARATCODE_AUDIT",
        kind: Kind::EnvToggle,
        key: "BHARATCODE_AUDIT",
        summary_i18n_key: "help_index.env.audit",
        default_summary: "Write a local audit log of agent actions for review.",
    },
];

/// Case-insensitive substring search over each entry's `name` and localized
/// `summary`. An empty (or whitespace-only) query returns every entry, in the
/// canonical [`ENTRIES`] order.
///
/// The search is read-only and allocation-light; it borrows directly from the
/// static [`ENTRIES`] table.
pub fn search(q: &str) -> Vec<&'static HelpEntry> {
    let needle = q.trim().to_lowercase();
    if needle.is_empty() {
        return ENTRIES.iter().collect();
    }
    ENTRIES
        .iter()
        .filter(|e| {
            e.name.to_lowercase().contains(&needle) || e.summary().to_lowercase().contains(&needle)
        })
        .collect()
}

/// Render the help index as a themed, human-readable block.
///
/// Entries are grouped into a `Commands` section then an `Env` section, each
/// with a themed heading; rows are painted with the active [`theme`] palette and
/// summaries are localized via [`HelpEntry::summary`]. When `filter` is `Some`,
/// only matching entries (see [`search`]) appear, and empty groups are omitted.
///
/// This function is pure: it reads the active theme and i18n locale (both cached
/// process-wide) but performs no I/O and has no side effects.
pub fn render(filter: Option<&str>) -> String {
    let entries = match filter {
        Some(q) => search(q),
        None => ENTRIES.iter().collect(),
    };

    // Longest name across the *selected* rows, so columns align per render.
    let width = entries.iter().map(|e| e.name.len()).max().unwrap_or(0);

    let mut out = String::new();

    let mut render_group = |label: &str, kind: Kind, out: &mut String| {
        let group: Vec<&HelpEntry> = entries.iter().copied().filter(|e| e.kind == kind).collect();
        if group.is_empty() {
            return;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("{}\n", theme::heading(label)));
        for e in group {
            out.push_str(&format!(
                "  {:<width$}  {}\n",
                theme::accent(e.name),
                theme::neutral(e.summary()),
                width = width,
            ));
        }
    };

    render_group("Commands", Kind::Command, &mut out);
    render_group("Env", Kind::EnvToggle, &mut out);

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_non_empty() {
        assert!(!ENTRIES.is_empty());
    }

    #[test]
    fn names_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for e in ENTRIES {
            assert!(seen.insert(e.name), "duplicate help entry name: {}", e.name);
        }
    }

    #[test]
    fn no_upstream_vendor_leak_in_summaries() {
        // Default summaries and i18n keys must never surface the upstream
        // project names to users.
        for e in ENTRIES {
            let lowered = e.default_summary.to_lowercase();
            assert!(
                !lowered.contains("goose"),
                "summary for {} leaks 'goose': {}",
                e.name,
                e.default_summary
            );
            assert!(
                !lowered.contains("block"),
                "summary for {} leaks 'block': {}",
                e.name,
                e.default_summary
            );
            // The rendered (localized) summary must be clean too.
            let rendered = e.summary().to_lowercase();
            assert!(
                !rendered.contains("goose"),
                "rendered summary leaks 'goose'"
            );
            assert!(
                !rendered.contains("block"),
                "rendered summary leaks 'block'"
            );
        }
    }

    #[test]
    fn every_key_has_non_empty_default_summary() {
        for e in ENTRIES {
            assert!(
                !e.summary_i18n_key.is_empty(),
                "{} has an empty i18n key",
                e.name
            );
            assert!(
                !e.default_summary.trim().is_empty(),
                "{} has an empty default summary",
                e.name
            );
        }
    }

    #[test]
    fn search_cost_returns_cost_and_cost_dashboard_entries() {
        let hits = search("cost");
        let names: Vec<&str> = hits.iter().map(|e| e.name).collect();
        // The `cost` command itself...
        assert!(
            names.contains(&"cost"),
            "search('cost') missing the cost command"
        );
        // ...and the cost-related (cost dashboard) toggles.
        assert!(
            names.contains(&"BHARATCODE_COST_EXTENSIONS"),
            "search('cost') missing the cost-extensions toggle"
        );
        assert!(
            names.contains(&"BHARATCODE_USD_INR"),
            "search('cost') missing the cost-currency toggle"
        );
        // Every hit must genuinely mention cost.
        for e in &hits {
            let hay = format!("{} {}", e.name, e.summary()).to_lowercase();
            assert!(hay.contains("cost"), "{} matched but has no 'cost'", e.name);
        }
    }

    #[test]
    fn search_is_case_insensitive() {
        assert_eq!(search("COST").len(), search("cost").len());
        assert!(!search("Configure").is_empty());
    }

    #[test]
    fn empty_query_returns_all() {
        assert_eq!(search("").len(), ENTRIES.len());
        assert_eq!(search("   ").len(), ENTRIES.len());
    }

    #[test]
    fn render_none_has_commands_heading_and_env_row() {
        let out = render(None);
        assert!(
            out.contains("Commands"),
            "render(None) missing 'Commands' heading"
        );
        assert!(
            out.contains("BHARATCODE_"),
            "render(None) missing a BHARATCODE_ env row"
        );
    }

    #[test]
    fn render_filtered_omits_empty_groups() {
        // A command-only query should not emit the Env heading.
        let out = render(Some("configure"));
        assert!(out.contains("configure"));
        assert!(!out.contains("\nEnv\n") && !out.starts_with("Env"));
    }

    #[test]
    fn render_unknown_filter_is_empty() {
        assert!(render(Some("zzz-no-such-entry-zzz")).is_empty());
    }
}
