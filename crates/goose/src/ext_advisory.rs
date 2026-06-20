//! Installed-extensions advisory for the system prompt.
//!
//! When enabled (opt-in via `BHARATCODE_EXT_ADVISORY`), this surfaces a compact
//! `# Available extensions` block into the system prompt listing the names of
//! installed plugin skills and configured MCP servers, so the agent knows which
//! third-party tools it can lean on. The names are sourced entirely from the
//! existing plugin discovery functions ([`crate::plugins::installed_plugin_skill_dirs`]
//! and [`crate::plugins::mcp_servers::enabled_plugin_mcp_servers`]); no new
//! capability facts are introduced here.
//!
//! The feature is a strict no-op when the toggle is unset or when nothing is
//! installed: [`advisory_block`] returns `None` and the built system prompt is
//! byte-identical to default behaviour.
//!
//! Original BharatCode work; not ported from any third party.

use std::collections::BTreeSet;
use std::path::Path;

/// Opt-in toggle name, shared by env var and config file.
const ENABLE_KEY: &str = "BHARATCODE_EXT_ADVISORY";

/// Hard cap on the number of names listed, to keep the block compact.
const MAX_NAMES: usize = 24;

/// Hard cap on the rendered block size, in bytes.
const MAX_BLOCK_BYTES: usize = 600;

/// Whether the extensions advisory is enabled. Opt-in via the
/// `BHARATCODE_EXT_ADVISORY` environment variable (checked first, raw) or the
/// config value of the same name. Any truthy-ish value (`1`, `true`, `yes`,
/// `on`) enables it; default OFF.
pub fn is_enabled() -> bool {
    if let Ok(raw) = std::env::var(ENABLE_KEY) {
        return is_truthy(&raw);
    }
    crate::config::Config::global()
        .get_param::<String>(ENABLE_KEY)
        .map(|raw| is_truthy(&raw))
        .unwrap_or(false)
}

fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Render a compact extensions advisory block, or `None`.
///
/// Returns `None` when the feature is disabled or when nothing is installed
/// (no plugin skills and no configured MCP servers). Otherwise returns a short
/// markdown block listing the installed names, capped to stay under
/// [`MAX_BLOCK_BYTES`].
pub fn advisory_block() -> Option<String> {
    if !is_enabled() {
        return None;
    }

    let mut names: BTreeSet<String> = BTreeSet::new();

    for dir in crate::plugins::installed_plugin_skill_dirs() {
        if let Some(name) = basename(&dir) {
            names.insert(name);
        }
    }

    for ext in crate::plugins::mcp_servers::enabled_plugin_mcp_servers(None) {
        let name = ext.name();
        let name = name.trim();
        if !name.is_empty() {
            names.insert(name.to_string());
        }
    }

    let names: Vec<String> = names.into_iter().collect();
    render_block(&names)
}

/// Extract the final path component as a display name, skipping empty/blank
/// components.
fn basename(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().trim().to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Render the advisory block over a fixed list of names. Returns `None` when the
/// list is empty. The output is capped to [`MAX_NAMES`] entries and kept under
/// [`MAX_BLOCK_BYTES`] bytes; if the full list would overflow, entries are
/// dropped (oldest-first) and a trailing ellipsis marker is added.
fn render_block(names: &[String]) -> Option<String> {
    if names.is_empty() {
        return None;
    }

    let total = names.len();
    let mut shown: Vec<&str> = names.iter().take(MAX_NAMES).map(|s| s.as_str()).collect();
    let mut truncated = total > shown.len();

    loop {
        let block = format_block(&shown, truncated);
        if block.len() <= MAX_BLOCK_BYTES || shown.len() <= 1 {
            return Some(block);
        }
        shown.pop();
        truncated = true;
    }
}

fn format_block(shown: &[&str], truncated: bool) -> String {
    let mut list = shown.join(", ");
    if truncated {
        list.push_str(", ...");
    }
    format!(
        "# Available extensions\n\
         The following installed extensions are available: {list}.\n\
         Prefer leaning on these when a task matches one."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialise tests that mutate the shared process env so the
    /// `BHARATCODE_EXT_ADVISORY` / path-root toggles don't race each other.
    fn env_guard<'a>(
        enable: Option<&'a str>,
        path_root: Option<&'a str>,
    ) -> env_lock::EnvGuard<'a> {
        env_lock::lock_env([(ENABLE_KEY, enable), ("BHARATCODE_PATH_ROOT", path_root)])
    }

    #[test]
    fn disabled_yields_none() {
        let _guard = env_guard(None, None);
        assert!(!is_enabled());
        assert!(advisory_block().is_none());
    }

    #[test]
    fn enabled_with_empty_plugins_dir_yields_none() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().display().to_string();
        let _guard = env_guard(Some("1"), Some(root.as_str()));
        assert!(is_enabled());
        // No plugins installed under the temp root => nothing to advertise.
        assert!(advisory_block().is_none());
    }

    #[test]
    fn render_block_over_fixed_names_is_capped_and_clean() {
        let names = vec![
            "rust-analyzer".to_string(),
            "github".to_string(),
            "postgres".to_string(),
        ];
        let block = render_block(&names).expect("non-empty names yield a block");

        assert!(block.contains("# Available extensions"));
        assert!(block.contains("extensions"));
        for name in &names {
            assert!(block.contains(name.as_str()), "missing name: {name}");
        }
        assert!(
            block.len() <= MAX_BLOCK_BYTES,
            "block too long: {} bytes",
            block.len()
        );

        // Zero internal-brand leakage in user-facing output.
        let lower = block.to_lowercase();
        assert!(!lower.contains("goose"), "leaked goose: {block}");
        assert!(!lower.contains("block"), "leaked block: {block}");
    }

    #[test]
    fn render_block_empty_is_none() {
        assert!(render_block(&[]).is_none());
    }

    #[test]
    fn render_block_truncates_long_lists_under_cap() {
        let names: Vec<String> = (0..200).map(|i| format!("extension-name-{i:03}")).collect();
        let block = render_block(&names).expect("non-empty list yields a block");
        assert!(
            block.len() <= MAX_BLOCK_BYTES,
            "block too long: {} bytes",
            block.len()
        );
        assert!(
            block.contains("..."),
            "long list should be marked truncated"
        );
    }

    #[test]
    fn is_truthy_recognizes_common_values() {
        assert!(is_truthy("1"));
        assert!(is_truthy("TRUE"));
        assert!(is_truthy(" yes "));
        assert!(is_truthy("on"));
        assert!(!is_truthy("0"));
        assert!(!is_truthy("false"));
        assert!(!is_truthy(""));
    }
}
