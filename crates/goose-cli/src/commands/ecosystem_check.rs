//! Ecosystem health for `bharatcode doctor` (BharatCode v73).
//!
//! A read-only, always-visible section that reports the user's extensibility
//! surface so they can confirm it is actually wired:
//!
//! - how many plugin skill directories are installed,
//! - how many MCP extensions enabled plugins contribute,
//! - whether plugin hooks are registered for the tool-use lifecycle
//!   (`PostToolUse` / `PreToolUse`).
//!
//! Like the other deep checks, every probe is best-effort and never blocks: an
//! empty or missing config simply reports zero / not-wired. Nothing here mutates
//! config, the filesystem, or the network, and it never gates the doctor run.

use crate::commands::doctor_checks::Status;
use goose::hooks::{HookEvent, HookManager};
use goose::plugins;

/// One line in the Ecosystem section: a status glyph, a human label, and a
/// short detail string rendered next to it by the doctor command.
pub struct EcoRow {
    pub status: Status,
    pub label: String,
    pub detail: String,
}

/// Look up a user-facing string through the i18n layer, falling back to the
/// English `default` when the active locale table has no entry for `key`.
///
/// `tr!` echoes the key back when it is missing, so an unchanged key is treated
/// as "untranslated". This mirrors the helper in `doctor.rs`, keeping these rows
/// renderable in English without depending on the i18n table (owned elsewhere).
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// Build the Ecosystem section rows in display order.
///
/// Always returns exactly three rows. A count of zero is reported as
/// [`Status::Warn`] (the surface exists but nothing is wired yet) rather than a
/// failure — extensibility is optional, so a clean install is not an error.
pub fn ecosystem_rows() -> Vec<EcoRow> {
    let plugin_count = plugins::installed_plugin_skill_dirs().len();
    let mcp_count = plugins::mcp_servers::enabled_plugin_mcp_servers(None).len();

    let manager = HookManager::load(None, false);
    let post = manager.has_hooks(HookEvent::PostToolUse);
    let pre = manager.has_hooks(HookEvent::PreToolUse);

    vec![
        count_row(
            plugin_count,
            label("ecosystem.plugins", "Installed plugins"),
            label("ecosystem.skill_dirs", "skill dir(s)"),
        ),
        count_row(
            mcp_count,
            label("ecosystem.mcp", "MCP extensions"),
            label("ecosystem.configured", "configured"),
        ),
        hooks_row(pre, post),
    ]
}

/// A row for a simple "how many are wired" count. Zero warns; any positive
/// count is healthy.
fn count_row(count: usize, label: String, unit: String) -> EcoRow {
    let status = if count > 0 { Status::Ok } else { Status::Warn };
    EcoRow {
        status,
        label,
        detail: format!("{} {}", count, unit),
    }
}

/// A row describing tool-use hook coverage. Healthy when either pre- or
/// post-tool-use hooks are registered; otherwise a non-blocking warning.
fn hooks_row(pre: bool, post: bool) -> EcoRow {
    let status = if pre || post {
        Status::Ok
    } else {
        Status::Warn
    };
    let detail = match (pre, post) {
        (true, true) => label("ecosystem.hooks_both", "PreToolUse, PostToolUse"),
        (true, false) => label("ecosystem.hooks_pre", "PreToolUse"),
        (false, true) => label("ecosystem.hooks_post", "PostToolUse"),
        (false, false) => label("ecosystem.hooks_none", "none registered"),
    };
    EcoRow {
        status,
        label: label("ecosystem.hooks", "Plugin tool-use hooks"),
        detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Render a row the way the doctor command would, exercising the
    /// `Status::glyph()` mapping so it must compile and produce a glyph.
    fn render(row: &EcoRow) -> String {
        format!("{} {}: {}", row.status.glyph(), row.label, row.detail)
    }

    #[test]
    fn empty_environment_reports_three_rows_all_zero() {
        let empty = tempfile::tempdir().unwrap();
        // The shared workspace env lock serializes every BHARATCODE_PATH_ROOT
        // mutator across the whole crate so this never races another test.
        let _guard = env_lock::lock_env([("BHARATCODE_PATH_ROOT", empty.path().to_str())]);

        let rows = ecosystem_rows();

        // Exactly three rows in the section.
        assert_eq!(rows.len(), 3, "ecosystem section must have exactly 3 rows");

        // Plugin and MCP rows report zero and warn (nothing wired in an empty
        // config); our rule is Warn on zero, Ok otherwise.
        assert!(rows[0].detail.starts_with('0'), "plugin count should be 0");
        assert!(rows[1].detail.starts_with('0'), "mcp count should be 0");
        assert_eq!(rows[0].status, Status::Warn);
        assert_eq!(rows[1].status, Status::Warn);

        // Hooks row exists; with no plugins it cannot be Ok, so it warns.
        assert_eq!(rows[2].status, Status::Warn);

        // The glyph()/Status mapping must compile and yield a non-empty glyph
        // for every row, and no row may leak an upstream brand name.
        for row in &rows {
            let line = render(row);
            assert!(!row.status.glyph().is_empty());
            let lower = line.to_ascii_lowercase();
            assert!(
                !lower.contains("goose") && !lower.contains("block"),
                "ecosystem row leaked an upstream brand: {line}"
            );
        }
    }
}
