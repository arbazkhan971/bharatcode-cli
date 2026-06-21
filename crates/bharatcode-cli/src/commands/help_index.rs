//! `bharatcode help-index` — a localized, grouped index of every BharatCode
//! subcommand and the key env-gated feature flags.
//!
//! The CLI surface has grown a number of India-focused, opt-in features that are
//! each guarded behind a `BHARATCODE_*` environment variable. Discovering them
//! by reading `--help` for every subcommand is tedious, so this module keeps a
//! single static [`INDEX`] that pairs each command (and each notable feature
//! flag) with a one-line description, its category, and — where relevant — the
//! environment variable that enables it.
//!
//! [`render_text`] prints a human-readable, category-grouped table for the
//! terminal; [`render_json`] emits the same data as JSON for tooling and tests.
//! Descriptions route through [`crate::tr!`] with an English fallback so the
//! table is localized while English output stays byte-for-byte stable.
//!
//! The index is intentionally read-only and side-effect free: it never touches
//! the filesystem, config, or network, so it is safe to call from any surface
//! (the `help-index` subcommand, the interactive `/help` footer, or tooling).

use std::collections::BTreeMap;

/// A single row in the help index.
///
/// `desc_key` is an i18n key resolved through [`crate::tr!`]; when the active
/// locale (or the English table) has no entry for it, the key's English default
/// is supplied inline by [`describe`] so the rendered output is never a bare
/// dotted key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelpEntry {
    /// The category this entry belongs to (one of the [`CATEGORIES`] labels).
    pub category: &'static str,
    /// The command name (`configure`, `cost`, …) or the feature label.
    pub command: &'static str,
    /// The `BHARATCODE_*` env var that enables the feature, when applicable.
    pub env_flag: Option<&'static str>,
    /// i18n key for the one-line description; see [`describe`].
    pub desc_key: &'static str,
}

/// Options for `bharatcode help-index`.
#[derive(Debug, Clone, Copy, Default)]
pub struct HelpIndexOptions {
    /// Emit the index as JSON instead of the human-readable table.
    pub json: bool,
}

/// Category labels, in the order they are rendered. The set is fixed so the
/// grouping in [`render_text`] is deterministic and the unit tests can assert
/// that every header appears.
pub const CATEGORIES: &[&str] = &[
    "Session",
    "Cost & Budget",
    "Privacy & Compliance",
    "i18n & UX",
    "Dev tools",
];

/// The complete, static command and feature-flag index.
///
/// Command names mirror the canonical names dispatched in `cli.rs`; env flags
/// mirror the real `BHARATCODE_*` variables read elsewhere in the crate.
pub static INDEX: &[HelpEntry] = &[
    // ---- Session ----
    HelpEntry {
        category: "Session",
        command: "configure",
        env_flag: None,
        desc_key: "help_index.configure",
    },
    HelpEntry {
        category: "Session",
        command: "session",
        env_flag: None,
        desc_key: "help_index.session",
    },
    HelpEntry {
        category: "Session",
        command: "onboard",
        env_flag: None,
        desc_key: "help_index.onboard",
    },
    HelpEntry {
        category: "Session",
        command: "recipes-library",
        env_flag: None,
        desc_key: "help_index.recipes_library",
    },
    HelpEntry {
        category: "Session",
        command: "presets",
        env_flag: None,
        desc_key: "help_index.presets",
    },
    // ---- Cost & Budget ----
    HelpEntry {
        category: "Cost & Budget",
        command: "cost",
        env_flag: None,
        desc_key: "help_index.cost",
    },
    HelpEntry {
        category: "Cost & Budget",
        command: "budget",
        env_flag: Some("BHARATCODE_BUDGET_INR"),
        desc_key: "help_index.budget_inr",
    },
    HelpEntry {
        category: "Cost & Budget",
        command: "cache",
        env_flag: Some("BHARATCODE_CACHE"),
        desc_key: "help_index.cache",
    },
    // ---- Privacy & Compliance ----
    HelpEntry {
        category: "Privacy & Compliance",
        command: "privacy",
        env_flag: None,
        desc_key: "help_index.privacy",
    },
    HelpEntry {
        category: "Privacy & Compliance",
        command: "offline",
        env_flag: Some("BHARATCODE_OFFLINE"),
        desc_key: "help_index.offline",
    },
    HelpEntry {
        category: "Privacy & Compliance",
        command: "residency",
        env_flag: Some("BHARATCODE_RESIDENCY"),
        desc_key: "help_index.residency",
    },
    HelpEntry {
        category: "Privacy & Compliance",
        command: "redact",
        env_flag: Some("BHARATCODE_REDACT"),
        desc_key: "help_index.redact",
    },
    HelpEntry {
        category: "Privacy & Compliance",
        command: "audit",
        env_flag: Some("BHARATCODE_AUDIT"),
        desc_key: "help_index.audit",
    },
    // ---- i18n & UX ----
    HelpEntry {
        category: "i18n & UX",
        command: "language",
        env_flag: Some("BHARATCODE_LANG"),
        desc_key: "help_index.lang",
    },
    HelpEntry {
        category: "i18n & UX",
        command: "theme",
        env_flag: Some("BHARATCODE_THEME"),
        desc_key: "help_index.theme",
    },
    HelpEntry {
        category: "i18n & UX",
        command: "accessibility",
        env_flag: Some("BHARATCODE_A11Y"),
        desc_key: "help_index.a11y",
    },
    // ---- Dev tools ----
    HelpEntry {
        category: "Dev tools",
        command: "doctor",
        env_flag: None,
        desc_key: "help_index.doctor",
    },
    HelpEntry {
        category: "Dev tools",
        command: "git",
        env_flag: None,
        desc_key: "help_index.git",
    },
    HelpEntry {
        category: "Dev tools",
        command: "review-diff",
        env_flag: None,
        desc_key: "help_index.review_diff",
    },
    HelpEntry {
        category: "Dev tools",
        command: "gen-tests",
        env_flag: None,
        desc_key: "help_index.gen_tests",
    },
    HelpEntry {
        category: "Dev tools",
        command: "gen-docs",
        env_flag: None,
        desc_key: "help_index.gen_docs",
    },
    HelpEntry {
        category: "Dev tools",
        command: "refactor",
        env_flag: None,
        desc_key: "help_index.refactor",
    },
];

/// English fallback descriptions, keyed by [`HelpEntry::desc_key`].
///
/// [`describe`] tries the active-locale i18n table first via [`crate::tr!`] and
/// only falls back to these strings when the key is untranslated, so a localized
/// table can override any of them without code changes.
fn english_default(desc_key: &str) -> &'static str {
    match desc_key {
        "help_index.configure" => "Configure providers, models, and credentials",
        "help_index.session" => "Start, list, resume, or export chat sessions",
        "help_index.onboard" => "Guided first-run setup for a new workspace",
        "help_index.recipes_library" => "Browse the bundled recipe library",
        "help_index.presets" => "List recommended India / open-weight model presets",
        "help_index.cost" => "Show token cost, in USD and INR, for recent sessions",
        "help_index.budget_inr" => "Enforce a per-session spend cap, in rupees",
        "help_index.cache" => "Reuse cached prompt prefixes to cut token spend",
        "help_index.privacy" => "Show the resolved data-governance and privacy posture",
        "help_index.offline" => "Deny all network egress and require a local provider",
        "help_index.residency" => "Pin data residency to an approved region",
        "help_index.redact" => "Redact secrets and PII before they leave the machine",
        "help_index.audit" => "Write a tamper-evident audit log of agent actions",
        "help_index.lang" => "Pick the interface language (for example hi for Hindi)",
        "help_index.theme" => "Select the terminal colour theme",
        "help_index.a11y" => "Turn on the high-contrast, screen-reader-friendly mode",
        "help_index.doctor" => "Run environment and configuration health checks",
        "help_index.git" => "Summarise recent git history for the agent",
        "help_index.review_diff" => "Review the staged or working diff for issues",
        "help_index.gen_tests" => "Draft unit tests for a source file in one pass",
        "help_index.gen_docs" => "Draft documentation for a source file in one pass",
        "help_index.refactor" => "Propose a focused refactor of a source file",
        _ => "",
    }
}

/// Resolve the one-line description for an entry through the i18n layer.
///
/// `tr!` echoes the key back when it has no translation, so an unchanged key is
/// treated as "untranslated" and the English default from [`english_default`]
/// is used instead. The result is always a real sentence, never a dotted key.
fn describe(desc_key: &str) -> String {
    let translated = crate::tr!(desc_key);
    if translated == desc_key {
        english_default(desc_key).to_string()
    } else {
        translated
    }
}

/// Render the index as a localized, category-grouped plain-text table.
///
/// Entries keep their declaration order within each category, and categories
/// follow [`CATEGORIES`]. Each row shows the command, its enabling env var (when
/// any), and the localized description. The `locale` argument is accepted for a
/// stable, explicit signature; the active translation table is resolved by the
/// i18n layer, so passing a value here documents intent at the call site.
pub fn render_text(_locale: &str) -> String {
    let mut by_category: BTreeMap<&'static str, Vec<&HelpEntry>> = BTreeMap::new();
    for entry in INDEX {
        by_category.entry(entry.category).or_default().push(entry);
    }

    let title = {
        let key = "help_index.title";
        let translated = crate::tr!(key);
        if translated == key {
            "BharatCode command index".to_string()
        } else {
            translated
        }
    };

    let mut out = String::new();
    out.push_str(&title);
    out.push('\n');

    for &category in CATEGORIES {
        let Some(entries) = by_category.get(category) else {
            continue;
        };
        out.push('\n');
        out.push_str(category);
        out.push('\n');
        for entry in entries {
            let flag = entry
                .env_flag
                .map(|f| format!(" [{f}]"))
                .unwrap_or_default();
            out.push_str(&format!(
                "  {:<16} {}{}\n",
                entry.command,
                describe(entry.desc_key),
                flag
            ));
        }
    }

    out
}

/// Render the index as a stable JSON array for tooling.
///
/// Each element carries `category`, `command`, `env_flag` (nullable), and the
/// localized `description`. Element order matches [`INDEX`].
pub fn render_json() -> String {
    let items: Vec<serde_json::Value> = INDEX
        .iter()
        .map(|e| {
            serde_json::json!({
                "category": e.category,
                "command": e.command,
                "env_flag": e.env_flag,
                "description": describe(e.desc_key),
            })
        })
        .collect();

    serde_json::to_string_pretty(&items).unwrap_or_else(|_| "[]".to_string())
}

/// Entry point for `bharatcode help-index`.
///
/// Prints the JSON form when `opts.json` is set, otherwise the localized text
/// table, then returns. The index is static and side-effect free, so this never
/// fails; the `Result` keeps the handler signature uniform with the other
/// subcommand handlers dispatched from `cli.rs`.
pub fn handle_help_index(opts: HelpIndexOptions) -> anyhow::Result<()> {
    if opts.json {
        println!("{}", render_json());
    } else {
        print!("{}", render_text("en"));
    }
    Ok(())
}

/// A one-line pointer printed in the interactive `/help` footer so the full
/// index stays discoverable from inside a session. Routed through the i18n
/// layer with an English fallback.
pub fn help_footer_line() -> String {
    let key = "help_index.footer_hint";
    let translated = crate::tr!(key);
    if translated == key {
        "Run 'bharatcode help-index' for the full command and feature index.".to_string()
    } else {
        translated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_duplicate_command_names() {
        let mut seen = std::collections::HashSet::new();
        for entry in INDEX {
            assert!(
                seen.insert(entry.command),
                "duplicate command name in INDEX: {}",
                entry.command
            );
        }
    }

    #[test]
    fn every_env_flag_uses_the_bharatcode_prefix() {
        for entry in INDEX {
            if let Some(flag) = entry.env_flag {
                assert!(
                    flag.starts_with("BHARATCODE_"),
                    "env_flag does not start with BHARATCODE_: {flag}"
                );
            }
        }
    }

    #[test]
    fn every_entry_belongs_to_a_known_category() {
        for entry in INDEX {
            assert!(
                CATEGORIES.contains(&entry.category),
                "entry {} has unknown category {}",
                entry.command,
                entry.category
            );
        }
    }

    #[test]
    fn every_description_key_has_an_english_default() {
        for entry in INDEX {
            assert!(
                !english_default(entry.desc_key).is_empty(),
                "missing English default for desc_key {}",
                entry.desc_key
            );
        }
    }

    #[test]
    fn render_json_roundtrips_to_the_same_entry_count() {
        let parsed: serde_json::Value = serde_json::from_str(&render_json()).unwrap();
        let arr = parsed.as_array().expect("render_json emits a JSON array");
        assert_eq!(arr.len(), INDEX.len());
    }

    #[test]
    fn render_text_contains_all_category_headers() {
        let text = render_text("en");
        for category in CATEGORIES {
            assert!(
                text.contains(category),
                "render_text is missing category header {category}"
            );
        }
    }

    #[test]
    fn render_text_lists_every_command() {
        let text = render_text("en");
        for entry in INDEX {
            assert!(
                text.contains(entry.command),
                "render_text is missing command {}",
                entry.command
            );
        }
    }

    #[test]
    fn output_has_no_upstream_branding() {
        let text = render_text("en").to_lowercase();
        assert!(!text.contains("goose"), "render_text leaks 'goose'");
        assert!(!text.contains("block"), "render_text leaks 'Block'");

        let json = render_json().to_lowercase();
        assert!(!json.contains("goose"), "render_json leaks 'goose'");
        assert!(!json.contains("block"), "render_json leaks 'Block'");
    }

    #[test]
    fn footer_hint_is_localized_and_clean() {
        let line = help_footer_line();
        assert!(line.contains("help-index"));
        assert!(!line.to_lowercase().contains("goose"));
    }
}
