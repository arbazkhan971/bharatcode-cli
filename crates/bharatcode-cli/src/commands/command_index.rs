//! Opt-in, grouped in-app command index (BharatCode v87).
//!
//! When `BHARATCODE_HELP_INDEX` is truthy, [`render_index`] is printed once at
//! the start of an interactive/built session (from `session/builder.rs`) so new
//! users can discover the ~25 subcommands and the key interactive slash-commands
//! without leaving the terminal.
//!
//! Design constraints honoured here:
//!   * **Default OFF.** [`is_enabled`] reads the *raw* `BHARATCODE_HELP_INDEX`
//!     environment variable (not Goose's config layer) and is OFF when the
//!     variable is unset, so `build_session` output is byte-identical by default.
//!   * **Static, side-effect free.** [`CATALOG`] is a `const` table of
//!     `(category, &[(command, i18n-key)])` rows; rendering only reads it and
//!     formats strings, so the index is cheap and deterministic.
//!   * **Localized.** Category headers and per-command descriptions route
//!     through [`crate::tr!`] / the i18n layer. The `en`/`hi`/`ta` values are
//!     added in the i18n files owned by the v81/v82/en-parity siblings; until
//!     those land, the i18n layer falls back to the *key string*, so this module
//!     renders cleanly on its own (the command names are always shown verbatim).
//!   * **No Goose/Block leakage.** Every user-facing token is either a
//!     `bharatcode` subcommand, a `/slash` command, or a localized description.

/// A single category in the command index, in render order.
#[derive(Debug, Clone, Copy)]
pub struct Category {
    /// i18n key for the category header (e.g. `command_index.cat.setup`).
    pub header_key: &'static str,
    /// Stable English fallback header shown when the i18n key is missing.
    ///
    /// Used as the *visible* header so the five categories are always present
    /// in the output even before the localized keys are added. Required by the
    /// unit test that asserts all five headers render.
    pub header_fallback: &'static str,
    /// The commands listed under this category: `(command, description-key)`.
    pub commands: &'static [(&'static str, &'static str)],
}

/// The complete, static command index, grouped into the five v87 categories:
/// Setup, Coding, Cost & Privacy, Sessions, Advanced.
///
/// Command names mirror the canonical `bharatcode` subcommands dispatched in
/// `cli.rs` and the interactive `/slash` commands; description keys follow the
/// crate's `command_index.<command>` i18n convention and fall back to the key
/// string until the localized values land.
pub const CATALOG: &[Category] = &[
    Category {
        header_key: "command_index.cat.setup",
        header_fallback: "Setup",
        commands: &[
            ("onboard", "command_index.onboard"),
            ("configure", "command_index.configure"),
            ("presets", "command_index.presets"),
            ("model-pack", "command_index.model_pack"),
            ("doctor", "command_index.doctor"),
        ],
    },
    Category {
        header_key: "command_index.cat.coding",
        header_fallback: "Coding",
        commands: &[
            ("session", "command_index.session"),
            ("run", "command_index.run"),
            ("review", "command_index.review"),
            ("gen-tests", "command_index.gen_tests"),
            ("gen-docs", "command_index.gen_docs"),
            ("refactor", "command_index.refactor"),
        ],
    },
    Category {
        header_key: "command_index.cat.cost_privacy",
        header_fallback: "Cost & Privacy",
        commands: &[
            ("cost", "command_index.cost"),
            ("budget", "command_index.budget"),
            ("privacy", "command_index.privacy"),
            ("audit", "command_index.audit"),
        ],
    },
    Category {
        header_key: "command_index.cat.sessions",
        header_fallback: "Sessions",
        commands: &[
            ("session list", "command_index.session_list"),
            ("recipe", "command_index.recipe"),
            ("recipes-library", "command_index.recipes_library"),
            ("schedule", "command_index.schedule"),
            ("project", "command_index.project"),
        ],
    },
    Category {
        header_key: "command_index.cat.advanced",
        header_fallback: "Advanced",
        commands: &[
            ("mcp-registry", "command_index.mcp_registry"),
            ("catalog", "command_index.catalog"),
            ("skills", "command_index.skills"),
            ("gateway", "command_index.gateway"),
            ("db", "command_index.db"),
            ("tutorial", "command_index.tutorial"),
            ("/help", "command_index.slash_help"),
            ("/mode", "command_index.slash_mode"),
        ],
    },
];

/// Whether the opt-in startup command index is enabled.
///
/// Reads the *raw* `BHARATCODE_HELP_INDEX` environment variable (not Goose's
/// config layer) so the gate is independent of any config file. Truthy values
/// are `1`, `true`, `yes`, `on` (case-insensitive, surrounding whitespace
/// ignored); everything else — including the variable being unset — is OFF.
pub fn is_enabled() -> bool {
    match std::env::var("BHARATCODE_HELP_INDEX") {
        Ok(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
}

/// Localize a category header, falling back to the stable English label when
/// the i18n key has not yet been added to the locale tables.
///
/// The i18n layer already falls back to the *key string* for unknown keys, so
/// we detect that case (translation == key) and substitute the human-readable
/// `header_fallback` instead. This guarantees the five headers are always
/// readable, even before the localized values land.
fn header_label(cat: &Category) -> String {
    let translated = crate::tr!(cat.header_key);
    if translated == cat.header_key {
        cat.header_fallback.to_string()
    } else {
        translated
    }
}

/// Localize a per-command description. When the i18n key is missing the i18n
/// layer returns the key itself; we suppress that so a missing description
/// simply renders the command on its own line (the command name is the stable,
/// always-present token).
fn describe(key: &str) -> Option<String> {
    let translated = crate::tr!(key);
    if translated == key {
        None
    } else {
        Some(translated)
    }
}

/// Render the grouped command index as a plain, multi-line `String`.
///
/// The output is category-grouped, with a localized header per category and one
/// line per command (`  <command> — <localized description>`). It is printed via
/// `eprintln!` from the session builder so it never pollutes piped stdout.
pub fn render_index() -> String {
    let mut out = String::new();

    // A localized, leading title line. Falls back to a neutral English title so
    // the block is self-describing even before the i18n key is added.
    let title_key = "command_index.title";
    let title = crate::tr!(title_key);
    if title == title_key {
        out.push_str("BharatCode commands");
    } else {
        out.push_str(&title);
    }
    out.push('\n');

    for cat in CATALOG {
        out.push('\n');
        out.push_str(&header_label(cat));
        out.push('\n');
        for (command, desc_key) in cat.commands {
            match describe(desc_key) {
                Some(desc) => out.push_str(&format!("  {command} — {desc}\n")),
                None => out.push_str(&format!("  {command}\n")),
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn is_enabled_is_off_when_env_unset() {
        // Snapshot and clear the gate so the assertion is deterministic
        // regardless of the ambient environment, then restore it.
        let saved = std::env::var("BHARATCODE_HELP_INDEX").ok();
        std::env::remove_var("BHARATCODE_HELP_INDEX");
        assert!(!is_enabled(), "unset BHARATCODE_HELP_INDEX must be OFF");
        if let Some(v) = saved {
            std::env::set_var("BHARATCODE_HELP_INDEX", v);
        }
    }

    #[test]
    fn render_index_contains_all_five_category_headers() {
        let rendered = render_index();
        for header in ["Setup", "Coding", "Cost & Privacy", "Sessions", "Advanced"] {
            assert!(
                rendered.contains(header),
                "rendered index is missing the {header:?} category header"
            );
        }
        // The catalog must declare exactly the five v87 categories.
        assert_eq!(CATALOG.len(), 5, "expected exactly five categories");
    }

    #[test]
    fn render_index_lists_at_least_twenty_distinct_commands() {
        // Distinct command names declared in the catalog itself.
        let distinct: HashSet<&str> = CATALOG
            .iter()
            .flat_map(|c| c.commands.iter().map(|(cmd, _)| *cmd))
            .collect();
        assert!(
            distinct.len() >= 20,
            "expected at least 20 distinct commands, found {}",
            distinct.len()
        );

        // And every one of them must actually appear in the rendered text.
        let rendered = render_index();
        for cmd in &distinct {
            assert!(
                rendered.contains(cmd),
                "rendered index is missing command {cmd:?}"
            );
        }
    }

    #[test]
    fn render_index_includes_key_discovery_commands() {
        let rendered = render_index();
        for cmd in ["onboard", "cost", "privacy"] {
            assert!(
                rendered.contains(cmd),
                "rendered index must mention {cmd:?}"
            );
        }
    }

    #[test]
    fn render_index_is_leak_free() {
        let rendered = render_index().to_lowercase();
        for banned in ["goose", "block.xyz", "block/goose"] {
            assert!(
                !rendered.contains(banned),
                "rendered index leaked the banned token {banned:?}"
            );
        }
    }
}
