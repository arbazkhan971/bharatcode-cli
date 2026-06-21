//! `bharatcode mcp-registry` — a curated, embedded registry of MCP servers.
//!
//! Holds a static, compile-time catalog of well-known and India-relevant MCP
//! servers. Each entry carries a stable id, a human-readable name, a transport,
//! the launch command + args, environment hints (the variables a user is likely
//! to need to set), and a category. The command answers "which MCP servers can
//! I wire up, and what does the config look like?" entirely offline.
//!
//! The registry is intentionally **read-only** and has **no install side
//! effects**: nothing is fetched, written, or spawned. Every entry is embedded
//! in the binary, so the listing renders identically on a fresh install.
//!
//! Subcommands:
//!   * `list`        — print the whole registry as a compact table.
//!   * `search <q>`  — filter entries by id, name, or category (case-insensitive).
//!   * `show <id>`   — print one entry's full details plus a ready-to-paste
//!                     extension-config snippet (the `mcpServers` document shape
//!                     validated by `bharatcode_core::plugins::mcp_servers`).
//!
//! User-facing labels route through the i18n layer via [`label`], which falls
//! back to the English default when the active locale has no entry for the key
//! (mirroring the helper in `catalog.rs`).
//!
//! Original BharatCode work; not ported from any third party.

use anyhow::Result;
use console::style;
use serde_json::{json, Value};

/// Transport classification for an MCP server. Kept as string constants so the
/// registry stays a plain `static` table; the recognised set is `{stdio}`
/// today, with room to grow without touching call sites.
pub const TRANSPORT_STDIO: &str = "stdio";

/// Category buckets used by `search` and the listing's grouping hint.
pub const CATEGORY_FILESYSTEM: &str = "filesystem";
pub const CATEGORY_VCS: &str = "vcs";
pub const CATEGORY_DATABASE: &str = "database";
pub const CATEGORY_WEB: &str = "web";
pub const CATEGORY_UTILITY: &str = "utility";
pub const CATEGORY_INDIA: &str = "india";

/// A single curated MCP-server entry.
pub struct McpServerEntry {
    /// Stable, lowercase id used to select the entry on the command line.
    pub id: &'static str,
    /// Human-readable display name.
    pub name: &'static str,
    /// Transport classification (currently always `stdio`).
    pub transport: &'static str,
    /// Launch command (the executable name).
    pub command: &'static str,
    /// Command arguments, in order.
    pub args: &'static [&'static str],
    /// Environment variables the user is likely to need to set, with a short
    /// hint describing each. Emitted into the snippet with empty values.
    pub env_hints: &'static [(&'static str, &'static str)],
    /// Category bucket (see the `CATEGORY_*` constants).
    pub category: &'static str,
    /// One-line description of what the server provides.
    pub description: &'static str,
}

/// The curated registry of MCP servers.
///
/// Well-known servers first, then India-relevant stubs. Every id is unique and
/// every entry round-trips to a valid `mcpServers` document (enforced by the
/// unit tests). No entry references upstream branding.
static REGISTRY: &[McpServerEntry] = &[
    McpServerEntry {
        id: "filesystem",
        name: "Filesystem",
        transport: TRANSPORT_STDIO,
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-filesystem", "."],
        env_hints: &[],
        category: CATEGORY_FILESYSTEM,
        description: "Read and write files within an allowed directory.",
    },
    McpServerEntry {
        id: "git",
        name: "Git",
        transport: TRANSPORT_STDIO,
        command: "uvx",
        args: &["mcp-server-git", "--repository", "."],
        env_hints: &[],
        category: CATEGORY_VCS,
        description: "Inspect a Git repository: status, log, diffs, and blame.",
    },
    McpServerEntry {
        id: "github",
        name: "GitHub",
        transport: TRANSPORT_STDIO,
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-github"],
        env_hints: &[(
            "GITHUB_PERSONAL_ACCESS_TOKEN",
            "Personal access token used to call the GitHub API",
        )],
        category: CATEGORY_VCS,
        description: "Manage issues, pull requests, and repositories on GitHub.",
    },
    McpServerEntry {
        id: "fetch",
        name: "Fetch",
        transport: TRANSPORT_STDIO,
        command: "uvx",
        args: &["mcp-server-fetch"],
        env_hints: &[],
        category: CATEGORY_WEB,
        description: "Fetch a URL and convert the page to model-friendly text.",
    },
    McpServerEntry {
        id: "sqlite",
        name: "SQLite",
        transport: TRANSPORT_STDIO,
        command: "uvx",
        args: &["mcp-server-sqlite", "--db-path", "./app.db"],
        env_hints: &[],
        category: CATEGORY_DATABASE,
        description: "Run read and write queries against a local SQLite database.",
    },
    McpServerEntry {
        id: "postgres",
        name: "PostgreSQL",
        transport: TRANSPORT_STDIO,
        command: "npx",
        args: &[
            "-y",
            "@modelcontextprotocol/server-postgres",
            "postgresql://localhost/mydb",
        ],
        env_hints: &[],
        category: CATEGORY_DATABASE,
        description: "Inspect schemas and run read-only queries against PostgreSQL.",
    },
    McpServerEntry {
        id: "time",
        name: "Time",
        transport: TRANSPORT_STDIO,
        command: "uvx",
        args: &["mcp-server-time", "--local-timezone", "Asia/Kolkata"],
        env_hints: &[],
        category: CATEGORY_UTILITY,
        description: "Current time and timezone conversions (defaults to IST).",
    },
    McpServerEntry {
        id: "memory",
        name: "Memory",
        transport: TRANSPORT_STDIO,
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-memory"],
        env_hints: &[],
        category: CATEGORY_UTILITY,
        description: "Persistent knowledge graph the agent can read and update.",
    },
    McpServerEntry {
        id: "sequential-thinking",
        name: "Sequential Thinking",
        transport: TRANSPORT_STDIO,
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-sequential-thinking"],
        env_hints: &[],
        category: CATEGORY_UTILITY,
        description: "Structured, step-by-step reasoning scaffold for hard tasks.",
    },
    McpServerEntry {
        id: "everything",
        name: "Everything (reference)",
        transport: TRANSPORT_STDIO,
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-everything"],
        env_hints: &[],
        category: CATEGORY_UTILITY,
        description: "Reference server exercising prompts, tools, and resources.",
    },
    McpServerEntry {
        id: "gst-docs",
        name: "GST Documentation (India)",
        transport: TRANSPORT_STDIO,
        command: "uvx",
        args: &["bharatcode-mcp-gst-docs"],
        env_hints: &[(
            "GST_API_BASE_URL",
            "Base URL for the GST reference data source",
        )],
        category: CATEGORY_INDIA,
        description: "Look up Indian GST rates, HSN codes, and filing references.",
    },
    McpServerEntry {
        id: "upi-docs",
        name: "UPI Reference (India)",
        transport: TRANSPORT_STDIO,
        command: "uvx",
        args: &["bharatcode-mcp-upi-docs"],
        env_hints: &[(
            "UPI_SPEC_VERSION",
            "Pin a specific UPI / NPCI spec revision (optional)",
        )],
        category: CATEGORY_INDIA,
        description: "UPI deep-link, intent, and collect-request reference helper.",
    },
    McpServerEntry {
        id: "aadhaar-validate",
        name: "Aadhaar Validation (India)",
        transport: TRANSPORT_STDIO,
        command: "uvx",
        args: &["bharatcode-mcp-aadhaar-validate"],
        env_hints: &[],
        category: CATEGORY_INDIA,
        description: "Offline checksum (Verhoeff) validation for Aadhaar numbers.",
    },
    McpServerEntry {
        id: "ifsc-lookup",
        name: "IFSC Lookup (India)",
        transport: TRANSPORT_STDIO,
        command: "uvx",
        args: &["bharatcode-mcp-ifsc-lookup"],
        env_hints: &[],
        category: CATEGORY_INDIA,
        description: "Resolve Indian bank IFSC codes to branch and bank details.",
    },
    McpServerEntry {
        id: "pincode-lookup",
        name: "Pincode Lookup (India)",
        transport: TRANSPORT_STDIO,
        command: "uvx",
        args: &["bharatcode-mcp-pincode-lookup"],
        env_hints: &[],
        category: CATEGORY_INDIA,
        description: "Map Indian postal PIN codes to district and state.",
    },
];

/// Action selected on the command line for `bharatcode mcp-registry`.
pub enum McpRegistryAction {
    /// Print the whole registry.
    List,
    /// Filter by id, name, or category.
    Search { query: String },
    /// Show one entry's details plus its config snippet.
    Show { id: String },
}

/// Return every entry in the registry.
pub fn all() -> &'static [McpServerEntry] {
    REGISTRY
}

/// Find a single entry by its exact id.
pub fn get(id: &str) -> Option<&'static McpServerEntry> {
    REGISTRY.iter().find(|e| e.id == id)
}

/// Return all entries whose id, name, or category contains `q`
/// (case-insensitive). An empty query matches everything.
pub fn search(q: &str) -> Vec<&'static McpServerEntry> {
    let needle = q.trim().to_lowercase();
    if needle.is_empty() {
        return REGISTRY.iter().collect();
    }
    REGISTRY
        .iter()
        .filter(|e| {
            e.id.to_lowercase().contains(&needle)
                || e.name.to_lowercase().contains(&needle)
                || e.category.to_lowercase().contains(&needle)
        })
        .collect()
}

/// Build a ready-to-paste extension-config snippet for one entry, in the
/// `mcpServers` document shape that `bharatcode_core::plugins::mcp_servers` validates.
///
/// Environment hints are emitted as empty-string placeholders under `env` so a
/// user can paste the snippet and fill in their own values.
pub fn to_extension_snippet(entry: &McpServerEntry) -> Value {
    let env: serde_json::Map<String, Value> = entry
        .env_hints
        .iter()
        .map(|(key, _hint)| ((*key).to_string(), Value::String(String::new())))
        .collect();

    let mut server = serde_json::Map::new();
    server.insert("command".to_string(), json!(entry.command));
    server.insert("args".to_string(), json!(entry.args));
    if !env.is_empty() {
        server.insert("env".to_string(), Value::Object(env));
    }

    json!({
        "mcpServers": {
            entry.id: Value::Object(server),
        }
    })
}

/// Look up a user-facing label through the i18n layer, falling back to the
/// English `default` when the active locale has no entry for `key`.
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// Entry point for `bharatcode mcp-registry`.
pub fn handle_mcp_registry(action: McpRegistryAction) -> Result<()> {
    match action {
        McpRegistryAction::List => {
            print_listing(&all().iter().collect::<Vec<_>>());
            Ok(())
        }
        McpRegistryAction::Search { query } => {
            let matches = search(&query);
            print_listing(&matches);
            Ok(())
        }
        McpRegistryAction::Show { id } => show_entry(&id),
    }
}

/// Print a compact, human-readable listing of the given entries.
fn print_listing(entries: &[&McpServerEntry]) {
    println!();
    println!(
        "{}",
        crate::theme::heading(label(
            "mcp_registry.title",
            "BharatCode MCP Server Registry"
        ))
    );
    println!();

    if entries.is_empty() {
        println!(
            "  {}",
            style(label("mcp_registry.empty", "No MCP servers match.")).dim()
        );
        println!();
        return;
    }

    let id_width = entries
        .iter()
        .map(|e| e.id.len())
        .max()
        .unwrap_or(0)
        .max(10);
    let cat_width = entries
        .iter()
        .map(|e| e.category.len())
        .max()
        .unwrap_or(0)
        .max(8);

    for entry in entries {
        println!(
            "  {:<id_width$}  {:<cat_width$}  {}",
            style(entry.id).bold(),
            style(entry.category).cyan(),
            style(entry.name).green(),
            id_width = id_width,
            cat_width = cat_width,
        );
        println!(
            "  {:<id_width$}  {:<cat_width$}  {}",
            "",
            "",
            style(entry.description).dim(),
            id_width = id_width,
            cat_width = cat_width,
        );
    }

    println!();
    println!(
        "{}",
        style(label(
            "mcp_registry.footer",
            "Run 'mcp-registry show <id>' for a config snippet, or 'search <query>' to filter."
        ))
        .dim()
    );
    println!();
}

/// Print one entry's details plus its config snippet, erroring if unknown.
fn show_entry(id: &str) -> Result<()> {
    let Some(entry) = get(id) else {
        let known = REGISTRY.iter().map(|e| e.id).collect::<Vec<_>>().join(", ");
        return Err(anyhow::anyhow!(
            "{} '{}'. {}: {}",
            label("mcp_registry.unknown_id", "No MCP server with id"),
            id,
            label("mcp_registry.available", "Available ids"),
            known
        ));
    };

    println!();
    println!("{}", crate::theme::heading(entry.name));
    println!();
    print_row(&label("mcp_registry.row_id", "Id"), entry.id);
    print_row(
        &label("mcp_registry.row_transport", "Transport"),
        entry.transport,
    );
    print_row(
        &label("mcp_registry.row_category", "Category"),
        entry.category,
    );
    print_row(
        &label("mcp_registry.row_command", "Command"),
        &format!("{} {}", entry.command, entry.args.join(" ")),
    );
    print_row(&label("mcp_registry.row_about", "About"), entry.description);

    if !entry.env_hints.is_empty() {
        println!();
        println!(
            "{}",
            style(label("mcp_registry.env_heading", "Environment hints:")).bold()
        );
        for (key, hint) in entry.env_hints {
            println!("  {}  {}", style(key).yellow(), style(hint).dim());
        }
    }

    println!();
    println!(
        "{}",
        style(label(
            "mcp_registry.snippet_heading",
            "Extension config snippet:"
        ))
        .bold()
    );
    let snippet = to_extension_snippet(entry);
    println!(
        "{}",
        serde_json::to_string_pretty(&snippet).unwrap_or_else(|_| "{}".to_string())
    );
    println!();
    Ok(())
}

fn print_row(name: &str, value: &str) {
    println!("  {:<11} {}", format!("{}:", name), value);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_non_empty() {
        assert!(!all().is_empty());
    }

    #[test]
    fn all_ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for entry in all() {
            assert!(seen.insert(entry.id), "duplicate id: {}", entry.id);
        }
    }

    #[test]
    fn search_matches_git_entry() {
        let results = search("git");
        assert!(
            results.iter().any(|e| e.id == "git"),
            "expected the git entry in search results"
        );
    }

    #[test]
    fn search_filters_by_category() {
        let results = search("india");
        assert!(!results.is_empty(), "expected India-category entries");
        assert!(
            results.iter().all(|e| e.category == CATEGORY_INDIA),
            "category search should return only that category"
        );
    }

    #[test]
    fn search_filters_by_name() {
        let results = search("PostgreSQL");
        assert!(
            results.iter().any(|e| e.id == "postgres"),
            "name search should be case-insensitive and match postgres"
        );
    }

    #[test]
    fn empty_search_returns_everything() {
        assert_eq!(search("   ").len(), all().len());
    }

    #[test]
    fn get_known_and_unknown_ids() {
        assert!(get("github").is_some());
        assert!(get("nope").is_none());
    }

    #[test]
    fn snippet_round_trips_to_valid_document() {
        for entry in all() {
            let snippet = to_extension_snippet(entry);
            bharatcode_core::plugins::mcp_servers::validate_mcp_server_document(&snippet).unwrap_or_else(
                |err| panic!("entry '{}' produced an invalid snippet: {}", entry.id, err),
            );
        }
    }

    #[test]
    fn snippet_keys_match_entry() {
        let entry = get("github").unwrap();
        let snippet = to_extension_snippet(entry);
        let servers = snippet
            .get("mcpServers")
            .and_then(|v| v.as_object())
            .expect("snippet has mcpServers object");
        assert!(servers.contains_key("github"));
        let server = servers.get("github").unwrap();
        assert_eq!(server.get("command").unwrap(), "npx");
        // env_hints are surfaced as empty placeholders in the snippet.
        assert!(server
            .get("env")
            .and_then(|v| v.as_object())
            .map(|o| o.contains_key("GITHUB_PERSONAL_ACCESS_TOKEN"))
            .unwrap_or(false));
    }

    #[test]
    fn every_command_is_non_empty() {
        for entry in all() {
            assert!(
                !entry.command.trim().is_empty(),
                "entry '{}' has an empty command",
                entry.id
            );
        }
    }

    #[test]
    fn no_upstream_branding_leaks() {
        for entry in all() {
            let mut fields = vec![entry.id, entry.name, entry.description, entry.command];
            fields.extend(entry.args.iter().copied());
            for (key, hint) in entry.env_hints {
                fields.push(key);
                fields.push(hint);
            }
            for field in fields {
                let lower = field.to_lowercase();
                assert!(
                    !lower.contains("goose"),
                    "entry '{}' leaks upstream brand in: {}",
                    entry.id,
                    field
                );
            }
        }
    }
}
