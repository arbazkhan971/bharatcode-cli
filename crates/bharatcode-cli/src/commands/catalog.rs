//! `bharatcode catalog` — a curated, embedded catalog of installable extensions.
//!
//! Lists a static, India-relevant set of installable extensions (MCP servers,
//! plugins, and recipe packs) with a name, kind, source, and a one-line install
//! hint. It answers "what can I add, and how do I add it?" without hunting for a
//! remote registry.
//!
//! The command is intentionally **read-only and offline**: every entry is
//! embedded in the binary at compile time, so it never touches the network, the
//! filesystem, or any local plugin state. It therefore renders identically on a
//! fresh install with zero plugins configured.
//!
//! Subcommand behaviour:
//!   * no flags        — print the full catalog as a compact listing.
//!   * `--kind <KIND>` — restrict the listing to `mcp`, `plugin`, or `recipe`.
//!   * `--show <ID>`   — print the full details for a single entry by id.
//!
//! User-facing labels are routed through the i18n layer via [`label`], which
//! falls back to the English default when the active locale table has no entry
//! for the key (mirroring the helper in `git_helper.rs`). `tr!` echoes the key
//! back when it is missing, so an unchanged key is treated as "untranslated".
//!
//! Original BharatCode work; not ported from any third party.

use anyhow::Result;
use console::style;

use crate::commands::doctor_checks::Status;

/// The kind of a catalog entry. Kept as a string-classified enum-like field on
/// [`CatalogEntry`]; the set of valid values is `{mcp, plugin, recipe}`.
pub const KIND_MCP: &str = "mcp";
pub const KIND_PLUGIN: &str = "plugin";
pub const KIND_RECIPE: &str = "recipe";

/// A single installable catalog entry.
pub struct CatalogEntry {
    /// Stable, lowercase id used to select the entry on the command line.
    pub id: &'static str,
    /// One of `mcp`, `plugin`, or `recipe`.
    pub kind: &'static str,
    /// Human-readable title.
    pub title: &'static str,
    /// Where the entry comes from (a URL, package spec, or registry hint).
    pub source: &'static str,
    /// A copy-pasteable, one-line hint describing how to install / add it.
    pub install_hint: &'static str,
    /// Whether this entry is enabled by default on a fresh install (true for the
    /// bundled platform/builtin extensions that ship active, false for the
    /// opt-in MCP servers and recipe packs).
    pub enabled_default: bool,
    /// The extension name this entry maps to when comparing against the set of
    /// currently-enabled extensions for the doctor readiness row. Empty when the
    /// entry has no corresponding live extension (e.g. recipe packs).
    pub match_name: &'static str,
}

/// The curated catalog of India-relevant installable extensions.
///
/// MCP servers first, then plugins, then recipe packs. Every id is unique and
/// every `kind` is one of `{mcp, plugin, recipe}` (enforced by the unit tests).
static CATALOG: &[CatalogEntry] = &[
    // Bundled platform / builtin extensions: these ship active on a fresh
    // install, so `enabled_default` is true and `match_name` carries the live
    // extension name the doctor readiness row compares against.
    CatalogEntry {
        id: "developer",
        kind: KIND_PLUGIN,
        title: "Developer tools (shell, editor, file operations)",
        source: "builtin: developer",
        install_hint: "bharatcode session --with-builtin developer",
        enabled_default: true,
        match_name: "developer",
    },
    CatalogEntry {
        id: "computercontroller",
        kind: KIND_PLUGIN,
        title: "Computer controller (web scraping, automation, caching)",
        source: "builtin: computercontroller",
        install_hint: "bharatcode session --with-builtin computercontroller",
        enabled_default: false,
        match_name: "computercontroller",
    },
    CatalogEntry {
        id: "memory",
        kind: KIND_PLUGIN,
        title: "Memory (persist facts and preferences across sessions)",
        source: "builtin: memory",
        install_hint: "bharatcode session --with-builtin memory",
        enabled_default: false,
        match_name: "memory",
    },
    CatalogEntry {
        id: "tutorial",
        kind: KIND_PLUGIN,
        title: "Tutorial (guided, interactive walkthroughs)",
        source: "builtin: tutorial",
        install_hint: "bharatcode session --with-builtin tutorial",
        enabled_default: false,
        match_name: "tutorial",
    },
    CatalogEntry {
        id: "autovisualiser",
        kind: KIND_PLUGIN,
        title: "Auto visualiser (render charts and diagrams inline)",
        source: "builtin: autovisualiser",
        install_hint: "bharatcode session --with-builtin autovisualiser",
        enabled_default: false,
        match_name: "autovisualiser",
    },
    // Builtin skills surfaced as catalog entries so they are discoverable.
    CatalogEntry {
        id: "ultracode",
        kind: KIND_PLUGIN,
        title: "Ultracode skill (structured multi-step engineering workflows)",
        source: "builtin-skill: ultracode",
        install_hint: "bharatcode skills --show ultracode",
        enabled_default: false,
        match_name: "ultracode",
    },
    CatalogEntry {
        id: "framework-migration",
        kind: KIND_PLUGIN,
        title: "Framework migration skill (port between frameworks safely)",
        source: "builtin-skill: framework-migration",
        install_hint: "bharatcode skills --show framework-migration",
        enabled_default: false,
        match_name: "framework-migration",
    },
    // Opt-in MCP servers: installable, but never active by default.
    CatalogEntry {
        id: "filesystem-mcp",
        kind: KIND_MCP,
        title: "Filesystem MCP server",
        source: "npm: @modelcontextprotocol/server-filesystem",
        install_hint:
            "bharatcode session --with-extension 'npx -y @modelcontextprotocol/server-filesystem .'",
        enabled_default: false,
        match_name: "",
    },
    CatalogEntry {
        id: "git-mcp",
        kind: KIND_MCP,
        title: "Git MCP server (read-only repo context)",
        source: "pypi: mcp-server-git",
        install_hint: "bharatcode session --with-extension 'uvx mcp-server-git --repository .'",
        enabled_default: false,
        match_name: "",
    },
    CatalogEntry {
        id: "sqlite-mcp",
        kind: KIND_MCP,
        title: "SQLite MCP server (query local databases)",
        source: "pypi: mcp-server-sqlite",
        install_hint:
            "bharatcode session --with-extension 'uvx mcp-server-sqlite --db-path ./app.db'",
        enabled_default: false,
        match_name: "",
    },
    CatalogEntry {
        id: "fetch-mcp",
        kind: KIND_MCP,
        title: "Fetch MCP server (fetch and convert web pages)",
        source: "pypi: mcp-server-fetch",
        install_hint: "bharatcode session --with-extension 'uvx mcp-server-fetch'",
        enabled_default: false,
        match_name: "",
    },
    CatalogEntry {
        id: "postgres-mcp",
        kind: KIND_MCP,
        title: "PostgreSQL MCP server (inspect schemas, run read queries)",
        source: "npm: @modelcontextprotocol/server-postgres",
        install_hint:
            "bharatcode session --with-extension 'npx -y @modelcontextprotocol/server-postgres postgresql://localhost/mydb'",
        enabled_default: false,
        match_name: "",
    },
    // India-relevant recipe library packs.
    CatalogEntry {
        id: "india-recipe-library",
        kind: KIND_RECIPE,
        title: "India recipe library (curated India-relevant recipe packs)",
        source: "library: india-recipe-library",
        install_hint: "bharatcode recipes-library",
        enabled_default: false,
        match_name: "",
    },
    CatalogEntry {
        id: "upi-payment-review",
        kind: KIND_RECIPE,
        title: "UPI payment integration review pack",
        source: "library: upi-payment-review",
        install_hint: "bharatcode recipes-library --show upi-payment-review",
        enabled_default: false,
        match_name: "",
    },
    CatalogEntry {
        id: "aadhaar-pii-audit",
        kind: KIND_RECIPE,
        title: "Aadhaar / PII data-handling audit pack",
        source: "library: aadhaar-pii-audit",
        install_hint: "bharatcode recipes-library --show aadhaar-pii-audit",
        enabled_default: false,
        match_name: "",
    },
    CatalogEntry {
        id: "gst-invoice-helper",
        kind: KIND_RECIPE,
        title: "GST invoice generation helper pack",
        source: "library: gst-invoice-helper",
        install_hint: "bharatcode recipes-library --show gst-invoice-helper",
        enabled_default: false,
        match_name: "",
    },
    CatalogEntry {
        id: "sarvam-indic-pack",
        kind: KIND_RECIPE,
        title: "Sarvam Indic NLP recipe pack (Indian-language tasks)",
        source: "registry: sarvam-indic",
        install_hint: "bharatcode recipe validate sarvam-indic-pack",
        enabled_default: false,
        match_name: "",
    },
    CatalogEntry {
        id: "krutrim-localization-pack",
        kind: KIND_RECIPE,
        title: "Krutrim localization recipe pack (Indic UI strings)",
        source: "registry: krutrim-localization",
        install_hint: "bharatcode recipe validate krutrim-localization-pack",
        enabled_default: false,
        match_name: "",
    },
];

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

/// Return `true` if `kind` is one of the recognised catalog kinds.
fn is_valid_kind(kind: &str) -> bool {
    matches!(kind, KIND_MCP | KIND_PLUGIN | KIND_RECIPE)
}

/// Find a single entry by its id.
fn find(id: &str) -> Option<&'static CatalogEntry> {
    CATALOG.iter().find(|e| e.id == id)
}

/// Return all entries whose kind equals `kind`.
fn filter_by_kind(kind: &str) -> Vec<&'static CatalogEntry> {
    CATALOG.iter().filter(|e| e.kind == kind).collect()
}

/// Return the full embedded catalog as a slice. The order is stable across the
/// process lifetime, so callers can rely on it for deterministic listings.
pub fn all() -> &'static [CatalogEntry] {
    CATALOG
}

/// Look up a single catalog entry by its stable id, returning `None` when no
/// entry matches.
pub fn get(id: &str) -> Option<&'static CatalogEntry> {
    find(id)
}

/// Collect the names of the extensions that are currently enabled, best-effort.
///
/// Reads the live extension configuration via `bharatcode_core::config`; any failure (an
/// unreadable or absent config) yields an empty set so the readiness row falls
/// back to reporting the catalog total only, never erroring.
fn enabled_extension_names() -> std::collections::HashSet<String> {
    std::panic::catch_unwind(|| {
        bharatcode_core::config::extensions::get_enabled_extensions()
            .into_iter()
            .map(|ext| ext.name().to_ascii_lowercase())
            .collect::<std::collections::HashSet<String>>()
    })
    .unwrap_or_default()
}

/// Doctor readiness row for the extension catalog.
///
/// Reports how many catalog entries are known and how many of the entries that
/// map to a live extension (those carrying a non-empty `match_name`) are
/// currently enabled. Returns a [`Status`] glyph + message in the same shape the
/// other doctor deep checks (`index_check::index_readiness`,
/// `repo_profile::readiness_line`) use, so the caller renders it uniformly.
///
/// Best-effort and read-only: if the enabled-extension list cannot be read it
/// reports the catalog total only, and it never mutates anything. The result is
/// always non-fatal — an empty active set is reported as [`Status::Warn`] (the
/// catalog exists but nothing it tracks is live yet), never a failure.
pub fn catalog_readiness() -> (Status, String) {
    let lbl = label("doctor.check.catalog_readiness", "Extensions catalog");

    let total = CATALOG.len();
    let enabled = enabled_extension_names();

    // Only entries that map to a live extension can ever be "active"; recipe
    // packs and MCP servers without a running extension never count here.
    let active = CATALOG
        .iter()
        .filter(|e| !e.match_name.is_empty())
        .filter(|e| enabled.contains(&e.match_name.to_ascii_lowercase()))
        .count();

    let entries_word = label("doctor.check.catalog_entries", "entries");
    let active_word = label("doctor.check.catalog_active", "active");
    let core = format!("{} {}, {} {}", total, entries_word, active, active_word);

    // Warn only when nothing the catalog tracks is live; otherwise the catalog
    // is healthy. The catalog itself is always non-empty (enforced by tests), so
    // a zero total never occurs.
    let status = if active == 0 {
        Status::Warn
    } else {
        Status::Ok
    };

    (status, format!("{} ({})", lbl, core))
}

/// Entry point for `bharatcode catalog`.
///
/// * `show` — when `Some(id)`, print details for that one entry.
/// * `kind` — when `Some(kind)`, restrict the listing to that kind.
pub fn handle_catalog(show: Option<String>, kind: Option<String>) -> Result<()> {
    if let Some(id) = show {
        return show_entry(&id);
    }

    if let Some(kind) = kind {
        let kind = kind.to_lowercase();
        if !is_valid_kind(&kind) {
            return Err(anyhow::anyhow!(
                "{} '{}'. {}: {}, {}, {}",
                label("catalog.unknown_kind", "Unknown kind"),
                kind,
                label("catalog.valid_kinds", "Valid kinds are"),
                KIND_MCP,
                KIND_PLUGIN,
                KIND_RECIPE,
            ));
        }
        print_listing(&filter_by_kind(&kind));
        return Ok(());
    }

    print_listing(&CATALOG.iter().collect::<Vec<_>>());
    Ok(())
}

/// Print a compact, human-readable listing of the given entries.
fn print_listing(entries: &[&CatalogEntry]) {
    println!();
    println!(
        "{}",
        crate::theme::heading(label("catalog.title", "BharatCode Extension Catalog"))
    );
    println!();

    if entries.is_empty() {
        println!(
            "  {}",
            style(label("catalog.empty", "No catalog entries match.")).dim()
        );
        println!();
        return;
    }

    let id_width = entries
        .iter()
        .map(|e| e.id.len())
        .max()
        .unwrap_or(0)
        .max(12);
    let kind_width = [KIND_MCP, KIND_PLUGIN, KIND_RECIPE]
        .iter()
        .map(|k| k.len())
        .max()
        .unwrap_or(6);

    for entry in entries {
        println!(
            "  {:<id_width$}  {:<kind_width$}  {}",
            style(entry.id).bold(),
            style(entry.kind).color256(208),
            style(entry.title).green(),
            id_width = id_width,
            kind_width = kind_width,
        );
        println!(
            "  {:<id_width$}  {:<kind_width$}  {}",
            "",
            "",
            style(format!(
                "{}: {}",
                label("catalog.source", "source"),
                entry.source
            ))
            .dim(),
            id_width = id_width,
            kind_width = kind_width,
        );
    }

    println!();
    println!(
        "{}",
        style(label(
            "catalog.footer",
            "Run 'catalog --show <id>' for install details, or '--kind mcp|plugin|recipe' to filter."
        ))
        .dim()
    );
    println!();
}

/// Print the full details for a single entry by id, erroring if unknown.
fn show_entry(id: &str) -> Result<()> {
    let Some(entry) = find(id) else {
        let known = CATALOG.iter().map(|e| e.id).collect::<Vec<_>>().join(", ");
        return Err(anyhow::anyhow!(
            "{} '{}'. {}: {}",
            label("catalog.unknown_id", "No catalog entry with id"),
            id,
            label("catalog.available", "Available ids"),
            known
        ));
    };

    println!();
    println!("{}", crate::theme::heading(entry.title));
    println!();
    print_row(&label("catalog.row_id", "Id"), entry.id);
    print_row(&label("catalog.row_kind", "Kind"), entry.kind);
    print_row(&label("catalog.row_source", "Source"), entry.source);
    println!();
    println!("{}", style(label("catalog.row_install", "Install:")).bold());
    println!("  {}", style(entry.install_hint).color256(208));
    println!();
    Ok(())
}

fn print_row(name: &str, value: &str) {
    println!("  {:<8} {}", format!("{}:", name), value);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_non_empty() {
        assert!(!CATALOG.is_empty());
    }

    #[test]
    fn all_ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for entry in CATALOG {
            assert!(seen.insert(entry.id), "duplicate id: {}", entry.id);
        }
    }

    #[test]
    fn every_kind_is_valid() {
        for entry in CATALOG {
            assert!(
                is_valid_kind(entry.kind),
                "entry '{}' has invalid kind '{}'",
                entry.id,
                entry.kind
            );
        }
    }

    #[test]
    fn filter_by_kind_returns_only_that_kind() {
        let mcp = filter_by_kind(KIND_MCP);
        assert!(!mcp.is_empty(), "expected at least one mcp entry");
        assert!(mcp.iter().all(|e| e.kind == KIND_MCP));

        // Sanity: the filtered count never exceeds the whole catalog.
        assert!(mcp.len() <= CATALOG.len());
    }

    #[test]
    fn find_known_and_unknown_ids() {
        assert!(find("filesystem-mcp").is_some());
        assert!(find("nope").is_none());
    }

    #[test]
    fn no_upstream_branding_leaks() {
        for entry in CATALOG {
            for field in [entry.title, entry.source, entry.install_hint, entry.id] {
                let lower = field.to_lowercase();
                assert!(
                    !lower.contains("goose"),
                    "entry '{}' leaks upstream brand in: {}",
                    entry.id,
                    field
                );
                assert!(
                    !lower.contains("block"),
                    "entry '{}' leaks upstream brand in: {}",
                    entry.id,
                    field
                );
            }
        }
    }

    #[test]
    fn all_returns_non_empty_catalog() {
        assert!(!all().is_empty());
        // `all()` mirrors the embedded slice exactly.
        assert_eq!(all().len(), CATALOG.len());
    }

    #[test]
    fn get_returns_some_for_known_and_none_for_unknown() {
        assert!(get("developer").is_some());
        assert!(get("india-recipe-library").is_some());
        assert!(get("definitely-not-a-real-id").is_none());
    }

    #[test]
    fn catalog_readiness_message_is_non_empty_and_reports_the_count() {
        let (status, msg) = catalog_readiness();

        assert!(!msg.is_empty(), "readiness message must not be empty");
        // The message must surface the catalog total so the operator can see
        // known-vs-active at a glance.
        let total = all().len().to_string();
        assert!(
            msg.contains(&total),
            "readiness message must contain the catalog count {total}: {msg}"
        );

        // Always non-fatal: the row never reports a hard failure.
        assert_ne!(status, Status::Fail, "msg: {msg}");

        // The glyph mapping must compile and yield a non-empty glyph.
        assert!(!status.glyph().is_empty());

        // No upstream brand may leak into the rendered row.
        let lower = msg.to_ascii_lowercase();
        assert!(
            !lower.contains("goose") && !lower.contains("block"),
            "readiness row leaked an upstream brand: {msg}"
        );
    }
}
