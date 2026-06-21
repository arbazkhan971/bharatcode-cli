//! Curated, embedded registry of well-known MCP servers.
//!
//! This module ships a small, in-binary catalogue of widely-used Model Context
//! Protocol servers — filesystem, git, fetch, sqlite, time — alongside a couple
//! of India-relevant document/translation gateways. Each entry carries enough
//! metadata to materialise a ready-to-use [`ExtensionConfig`] that a recipe or
//! an interactive session can adopt directly, without the user hand-writing a
//! transport/command block.
//!
//! It is pure data plus a resolver: there are no network calls, no process
//! spawning, and no filesystem access. Nothing here runs unless a consumer
//! calls [`lookup`], [`all`], or [`to_extension_config`], so the default
//! behaviour of the binary is unchanged.
//!
//! The [`ExtensionConfig`] produced here is the same type used by
//! [`crate::plugins::mcp_servers`], so a registry-sourced extension is
//! indistinguishable from one declared in a plugin's `.mcp.json`.
//!
//! Original BharatCode work; not ported from any third party.

use crate::agents::extension::ExtensionConfig;
use crate::config::{DEFAULT_EXTENSION_DESCRIPTION, DEFAULT_EXTENSION_TIMEOUT};

/// Transport an MCP server entry speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpTransport {
    /// Local subprocess over standard I/O (`command` + `args`).
    Stdio,
    /// Remote server reached over a URL (`url`).
    Sse,
}

/// A single curated MCP server descriptor.
///
/// Pure metadata. For [`McpTransport::Stdio`] entries `command`/`args` are
/// populated and `url` is empty; for [`McpTransport::Sse`] entries `url` is
/// populated and `command`/`args` are empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct McpServerEntry {
    /// Stable, unique, lowercase short id (the lookup key).
    pub id: &'static str,
    /// Human-friendly title.
    pub title: &'static str,
    /// Transport this entry uses.
    pub transport: McpTransport,
    /// Launch command for stdio entries (empty for SSE entries).
    pub command: &'static str,
    /// Launch arguments for stdio entries (empty for SSE entries).
    pub args: &'static [&'static str],
    /// Endpoint URL for SSE entries (empty for stdio entries).
    pub url: &'static str,
    /// Short, free-form relevance tags (e.g. India-relevance markers).
    pub tags: &'static [&'static str],
}

/// Curated MCP server registry.
///
/// Ids are unique and lowercase; titles and commands deliberately avoid any
/// vendor branding. The first five entries are the canonical reference servers
/// shipped with the MCP ecosystem; the last two are India-relevant document and
/// translation gateways.
static REGISTRY: &[McpServerEntry] = &[
    McpServerEntry {
        id: "filesystem",
        title: "Filesystem",
        transport: McpTransport::Stdio,
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-filesystem", "."],
        url: "",
        tags: &["files", "local"],
    },
    McpServerEntry {
        id: "git",
        title: "Git",
        transport: McpTransport::Stdio,
        command: "uvx",
        args: &["mcp-server-git"],
        url: "",
        tags: &["vcs", "local"],
    },
    McpServerEntry {
        id: "fetch",
        title: "Fetch",
        transport: McpTransport::Stdio,
        command: "uvx",
        args: &["mcp-server-fetch"],
        url: "",
        tags: &["web", "http"],
    },
    McpServerEntry {
        id: "sqlite",
        title: "SQLite",
        transport: McpTransport::Stdio,
        command: "uvx",
        args: &["mcp-server-sqlite", "--db-path", "./data.db"],
        url: "",
        tags: &["database", "local"],
    },
    McpServerEntry {
        id: "time",
        title: "Time",
        transport: McpTransport::Stdio,
        command: "uvx",
        args: &["mcp-server-time", "--local-timezone", "Asia/Kolkata"],
        url: "",
        tags: &["utility", "india"],
    },
    McpServerEntry {
        id: "memory",
        title: "Knowledge Memory",
        transport: McpTransport::Stdio,
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-memory"],
        url: "",
        tags: &["memory", "local"],
    },
    McpServerEntry {
        id: "sarvam-doc",
        title: "Sarvam Document Gateway",
        transport: McpTransport::Sse,
        command: "",
        args: &[],
        url: "https://mcp.sarvam.ai/sse",
        tags: &["india", "documents", "translation"],
    },
    McpServerEntry {
        id: "krutrim-doc",
        title: "Krutrim Document Gateway",
        transport: McpTransport::Sse,
        command: "",
        args: &[],
        url: "https://mcp.olakrutrim.com/sse",
        tags: &["india", "documents", "gateway"],
    },
];

/// Returns every curated MCP server entry.
pub fn all() -> &'static [McpServerEntry] {
    REGISTRY
}

/// Looks up a curated entry by id (case-insensitive). Returns `None` if no
/// entry matches.
pub fn lookup(id: &str) -> Option<&'static McpServerEntry> {
    REGISTRY
        .iter()
        .find(|entry| entry.id.eq_ignore_ascii_case(id))
}

/// Builds a ready-to-adopt [`ExtensionConfig`] for the entry with `id`
/// (case-insensitive), or `None` if there is no such entry.
///
/// Stdio entries become [`ExtensionConfig::Stdio`]; SSE entries become
/// [`ExtensionConfig::Sse`]. The extension `name` is the entry id, so the
/// resulting extension is stable and addressable.
pub fn to_extension_config(id: &str) -> Option<ExtensionConfig> {
    lookup(id).map(entry_to_extension_config)
}

fn entry_to_extension_config(entry: &McpServerEntry) -> ExtensionConfig {
    let description = if entry.title.is_empty() {
        DEFAULT_EXTENSION_DESCRIPTION.to_string()
    } else {
        entry.title.to_string()
    };

    match entry.transport {
        McpTransport::Stdio => ExtensionConfig::Stdio {
            name: entry.id.to_string(),
            description,
            cmd: entry.command.to_string(),
            args: entry.args.iter().map(|arg| arg.to_string()).collect(),
            envs: Default::default(),
            env_keys: Vec::new(),
            timeout: Some(DEFAULT_EXTENSION_TIMEOUT),
            cwd: None,
            bundled: Some(false),
            available_tools: Vec::new(),
        },
        McpTransport::Sse => ExtensionConfig::Sse {
            name: entry.id.to_string(),
            description,
            uri: Some(entry.url.to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_non_empty() {
        assert!(!all().is_empty());
    }

    #[test]
    fn ids_are_unique_and_lowercase() {
        let mut seen = std::collections::HashSet::new();
        for entry in all() {
            assert_eq!(
                entry.id,
                entry.id.to_ascii_lowercase(),
                "id `{}` is not lowercase",
                entry.id
            );
            assert!(
                seen.insert(entry.id),
                "duplicate registry id `{}`",
                entry.id
            );
        }
    }

    #[test]
    fn lookup_is_case_insensitive() {
        let upper = lookup("GIT").expect("GIT should resolve");
        assert_eq!(upper.id, "git");
        let mixed = lookup("Git").expect("Git should resolve");
        assert_eq!(mixed.id, "git");
    }

    #[test]
    fn lookup_unknown_is_none() {
        assert!(lookup("nope").is_none());
    }

    #[test]
    fn to_extension_config_git_is_stdio() {
        let config = to_extension_config("git").expect("git config");
        match config {
            ExtensionConfig::Stdio {
                name, cmd, args, ..
            } => {
                assert_eq!(name, "git");
                assert_eq!(cmd, "uvx");
                assert_eq!(args, vec!["mcp-server-git".to_string()]);
            }
            other => panic!("expected Stdio config, got {other:?}"),
        }
    }

    #[test]
    fn to_extension_config_sse_entry_is_sse() {
        let config = to_extension_config("sarvam-doc").expect("sarvam-doc config");
        match config {
            ExtensionConfig::Sse { name, uri, .. } => {
                assert_eq!(name, "sarvam-doc");
                assert_eq!(uri.as_deref(), Some("https://mcp.sarvam.ai/sse"));
            }
            other => panic!("expected Sse config, got {other:?}"),
        }
    }

    #[test]
    fn unknown_id_yields_no_config() {
        assert!(to_extension_config("does-not-exist").is_none());
    }

    #[test]
    fn every_entry_produces_a_valid_config() {
        for entry in all() {
            let config = to_extension_config(entry.id)
                .unwrap_or_else(|| panic!("entry `{}` produced no config", entry.id));
            match (entry.transport, &config) {
                (McpTransport::Stdio, ExtensionConfig::Stdio { name, cmd, .. }) => {
                    assert_eq!(name, entry.id);
                    assert!(!cmd.is_empty(), "stdio entry `{}` has empty cmd", entry.id);
                }
                (McpTransport::Sse, ExtensionConfig::Sse { name, uri, .. }) => {
                    assert_eq!(name, entry.id);
                    assert!(
                        uri.as_deref().is_some_and(|u| !u.is_empty()),
                        "sse entry `{}` has empty uri",
                        entry.id
                    );
                }
                (transport, other) => panic!(
                    "entry `{}` transport {:?} mapped to unexpected config {:?}",
                    entry.id, transport, other
                ),
            }
        }
    }

    #[test]
    fn no_vendor_branding_in_entries() {
        for entry in all() {
            for needle in ["goose", "block"] {
                assert!(
                    !entry.title.to_ascii_lowercase().contains(needle),
                    "entry `{}` title contains `{}`",
                    entry.id,
                    needle
                );
                assert!(
                    !entry.command.to_ascii_lowercase().contains(needle),
                    "entry `{}` command contains `{}`",
                    entry.id,
                    needle
                );
            }
        }
    }
}
