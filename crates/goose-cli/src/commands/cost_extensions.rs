//! Opt-in "Extensions in use" footer for `bharatcode cost`.
//!
//! A user's LLM spend often flows through third-party surface: installed
//! plugins and the MCP extensions they ship. This module renders a compact,
//! name-only footer so a user can see *what* extensions were available to the
//! agent without having to inspect their plugin directory by hand. It lists
//! names only — never commands, arguments, env, or paths — so the footer leaks
//! no configuration or secrets.
//!
//! Design:
//!   * **Default OFF.** The footer is only appended when
//!     `BHARATCODE_COST_EXTENSIONS` is truthy. With it unset, the `cost` output
//!     is byte-identical to before this module existed.
//!   * **Best-effort & read-only.** Names are gathered from the existing
//!     [`goose::plugins`] discovery helpers; nothing is installed, mutated, or
//!     network-touched here.
//!   * **Nothing to show => nothing printed.** When no plugins or MCP
//!     extensions are configured, [`extensions_footer`] returns `None` even
//!     when enabled, so an empty install produces no footer.
//!
//! Original BharatCode work; not ported from any third party.

use std::collections::BTreeSet;

use goose::config::paths::Paths;

/// Environment key that turns the extensions footer on. Absent / falsey =>
/// fully disabled (default OFF — `cost` output unchanged).
pub const COST_EXTENSIONS_ENABLED_KEY: &str = "BHARATCODE_COST_EXTENSIONS";

/// Whether the extensions footer is enabled for this process.
///
/// Reads `BHARATCODE_COST_EXTENSIONS` as a raw environment string first so a
/// bare `1` is honoured (mirrors [`crate::commands::audit::is_enabled`], which
/// avoids the config layer coercing `1` into a JSON number and reporting OFF).
/// Accepts the usual truthy spellings (`1`, `true`, `yes`, `on`); anything else
/// — including absence — is OFF.
pub fn is_enabled() -> bool {
    match std::env::var(COST_EXTENSIONS_ENABLED_KEY) {
        Ok(raw) => is_truthy(&raw),
        Err(_) => false,
    }
}

fn is_truthy(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Look up a user-facing label through the i18n layer, falling back to the
/// English `default` when the active locale table has no entry for `key`.
///
/// `tr!` echoes the key back when it is missing, so an unchanged key is treated
/// as "untranslated" and the English default is used.
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// Derive a human-readable plugin name from one of its skill directories.
///
/// [`goose::plugins::installed_plugin_skill_dirs`] returns directories nested
/// under the plugins root (`<plugins_dir>/<plugin>/...`). The first path
/// component beneath the plugins root is the plugin's directory name, which we
/// surface verbatim. Falls back to the directory's own file name when the path
/// does not sit under the plugins root.
fn plugin_name_from_skill_dir(
    skill_dir: &std::path::Path,
    plugins_dir: &std::path::Path,
) -> Option<String> {
    if let Ok(rel) = skill_dir.strip_prefix(plugins_dir) {
        if let Some(first) = rel.components().next() {
            let name = first.as_os_str().to_string_lossy().trim().to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    skill_dir
        .file_name()
        .map(|n| n.to_string_lossy().trim().to_string())
        .filter(|n| !n.is_empty())
}

/// Collect the de-duplicated, sorted set of extension names worth attributing:
/// installed plugins (by directory name) and configured plugin MCP servers (by
/// `plugin:server` name). Names only — no commands, args, env, or paths.
fn collect_extension_names() -> Vec<String> {
    let plugins_dir = Paths::plugins_dir();

    let mut names: BTreeSet<String> = BTreeSet::new();

    for skill_dir in goose::plugins::installed_plugin_skill_dirs() {
        if let Some(name) = plugin_name_from_skill_dir(&skill_dir, &plugins_dir) {
            names.insert(name);
        }
    }

    for ext in goose::plugins::mcp_servers::enabled_plugin_mcp_servers(None) {
        let name = ext.name();
        let name = name.trim();
        if !name.is_empty() {
            names.insert(name.to_string());
        }
    }

    names.into_iter().collect()
}

/// Render the muted "Extensions in use" block for a fixed list of names.
///
/// Pulled out from [`extensions_footer`] so the rendering can be unit-tested
/// without touching the filesystem. Returns `None` for an empty list so an
/// empty install never produces a header with no body.
fn render_extensions_block(names: &[String]) -> Option<String> {
    if names.is_empty() {
        return None;
    }

    let header = label("cost.extensions", "Extensions in use:");
    let note = label(
        "cost.extensions_note",
        "third-party surface your spend ran through (names only)",
    );

    let mut out = String::new();
    out.push_str(&format!("  {}\n", crate::theme::muted(header)));
    out.push_str(&format!("  {}", crate::theme::muted(note)));
    for name in names {
        out.push_str(&format!(
            "\n    {}",
            crate::theme::muted(format!("- {name}"))
        ));
    }
    Some(out)
}

/// Build the opt-in "Extensions in use" footer for `bharatcode cost`.
///
/// Returns `None` when the feature is disabled (`BHARATCODE_COST_EXTENSIONS`
/// unset / falsey) **or** when nothing is installed, so the default `cost`
/// output is unchanged in both cases. When enabled and at least one plugin or
/// MCP extension is present, returns a compact muted block listing each name.
pub fn extensions_footer() -> Option<String> {
    if !is_enabled() {
        return None;
    }
    render_extensions_block(&collect_extension_names())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_truthy_accepts_common_spellings() {
        for v in ["1", "true", "TRUE", "yes", "on", "  On  "] {
            assert!(is_truthy(v), "expected {v:?} to be truthy");
        }
        for v in ["", "0", "false", "no", "off", "nope"] {
            assert!(!is_truthy(v), "expected {v:?} to be falsey");
        }
    }

    #[test]
    fn is_enabled_false_when_env_unset() {
        let _guard = env_lock::lock_env([(COST_EXTENSIONS_ENABLED_KEY, None::<&str>)]);
        assert!(!is_enabled());
    }

    #[test]
    fn footer_none_when_disabled() {
        let _guard = env_lock::lock_env([(COST_EXTENSIONS_ENABLED_KEY, None::<&str>)]);
        assert!(extensions_footer().is_none());
    }

    #[test]
    fn footer_none_when_enabled_but_no_plugins_installed() {
        let temp = tempfile::tempdir().unwrap();
        // Hold the shared workspace env lock so this never races another
        // BHARATCODE_PATH_ROOT mutator elsewhere in the crate.
        let _guard = env_lock::lock_env([
            (COST_EXTENSIONS_ENABLED_KEY, Some("1")),
            ("BHARATCODE_PATH_ROOT", temp.path().to_str()),
        ]);

        let footer = extensions_footer();

        assert!(
            footer.is_none(),
            "empty plugins root should yield no footer, got {footer:?}"
        );
    }

    #[test]
    fn render_block_contains_each_name_and_no_upstream_branding() {
        let names = vec![
            "payments".to_string(),
            "search-tools".to_string(),
            "deploy:remote".to_string(),
        ];
        let block = render_extensions_block(&names).expect("non-empty names render a block");

        for name in &names {
            assert!(
                block.contains(name.as_str()),
                "rendered block should mention {name:?}: {block}"
            );
        }
        // Zero user-facing upstream branding leakage.
        assert!(!block.contains("goose"), "footer must not leak 'goose'");
        assert!(!block.contains("Goose"), "footer must not leak 'Goose'");
        assert!(!block.contains("Block"), "footer must not leak 'Block'");
    }

    #[test]
    fn render_block_none_for_empty() {
        assert!(render_extensions_block(&[]).is_none());
    }

    #[test]
    fn plugin_name_taken_from_first_component_under_root() {
        let root = std::path::Path::new("/tmp/plugins");
        let skill_dir = std::path::Path::new("/tmp/plugins/payments/skills");
        assert_eq!(
            plugin_name_from_skill_dir(skill_dir, root).as_deref(),
            Some("payments")
        );
    }
}
