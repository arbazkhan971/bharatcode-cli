//! Curated, embedded catalog of installable extensions and providers.
//!
//! Onboarding, `configure`, the planner presets advisory and the MCP registry
//! each surface their own slice of "things you can wire up". This module ships
//! a single, in-binary index that ties those slices together: one curated list
//! of installable providers, MCP extensions and built-in tools, biased towards
//! the open-weight and India-hosted options BharatCode cares about, so a
//! consumer can present *one* discoverable catalog instead of stitching three
//! tables together by hand.
//!
//! It is pure, read-only metadata: no network calls, no filesystem access, no
//! provider construction. Nothing here runs unless a consumer calls [`all`],
//! [`find`] or [`india_hosted`], so the default behaviour of the binary is
//! unchanged and there is no env gate to flip.
//!
//! The ids deliberately line up with tables already shipped in the binary: the
//! `sarvam`, `krutrim` and `ollama` provider entries mirror the declarative
//! provider definitions and [`crate::providers::planner_presets`]; the MCP
//! extension entries mirror ids in [`crate::providers::mcp_registry`]. Keeping
//! the ids stable lets a consumer resolve a catalog entry straight through to
//! the concrete provider/extension machinery.
//!
//! Original BharatCode work; not ported from any third party.

use std::borrow::Cow;

/// What kind of installable thing a catalog entry describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogKind {
    /// A model provider (resolves to a provider id used by the provider layer).
    Provider,
    /// An MCP server / extension (resolves to an id in [`crate::providers::mcp_registry`]).
    McpExtension,
    /// A built-in tool that ships in the binary (no install step required).
    Builtin,
}

impl CatalogKind {
    /// Stable, lowercase label for this kind (for display / serialization).
    pub fn as_str(self) -> &'static str {
        match self {
            CatalogKind::Provider => "provider",
            CatalogKind::McpExtension => "mcp-extension",
            CatalogKind::Builtin => "builtin",
        }
    }
}

/// A single curated catalog entry.
///
/// Pure metadata. The fields use [`Cow<'static, str>`] so the static table can
/// hold borrowed string literals at zero cost while still allowing an owned
/// value to be constructed at runtime if a consumer ever needs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    /// Stable, unique, lowercase short id (the lookup key).
    pub id: Cow<'static, str>,
    /// What kind of installable thing this entry is.
    pub kind: CatalogKind,
    /// Short, vendor-neutral description of the entry.
    pub description: Cow<'static, str>,
    /// One-line hint on how to obtain / enable the entry.
    pub install_hint: Cow<'static, str>,
    /// Residency tag: `true` when the option is hosted in India.
    pub india_hosted: bool,
}

/// Convenience constructor for a static catalog entry from borrowed literals.
const fn entry(
    id: &'static str,
    kind: CatalogKind,
    description: &'static str,
    install_hint: &'static str,
    india_hosted: bool,
) -> CatalogEntry {
    CatalogEntry {
        id: Cow::Borrowed(id),
        kind,
        description: Cow::Borrowed(description),
        install_hint: Cow::Borrowed(install_hint),
        india_hosted,
    }
}

/// The curated catalog.
///
/// Ids are unique and lowercase. India-hosted providers come first, then
/// open-weight/local options, then the curated MCP extensions, then built-ins.
/// Descriptions deliberately carry no vendor branding.
static CATALOG: &[CatalogEntry] = &[
    entry(
        "sarvam",
        CatalogKind::Provider,
        "India-hosted Sarvam models (OpenAI-compatible chat and reasoning).",
        "Set SARVAM_API_KEY from dashboard.sarvam.ai, then select the sarvam provider.",
        true,
    ),
    entry(
        "krutrim",
        CatalogKind::Provider,
        "India-hosted Krutrim Cloud (Ola) models, OpenAI-compatible.",
        "Set KRUTRIM_API_KEY from cloud.olakrutrim.com, then select the krutrim provider.",
        true,
    ),
    entry(
        "ollama",
        CatalogKind::Provider,
        "Local open-weight models served on your own machine; private, no per-token cost.",
        "Install Ollama and pull a model (e.g. `ollama pull qwen2.5-coder`); runs at localhost:11434.",
        false,
    ),
    entry(
        "filesystem",
        CatalogKind::McpExtension,
        "Local filesystem access scoped to a working directory.",
        "Requires Node; launches via `npx -y @modelcontextprotocol/server-filesystem`.",
        false,
    ),
    entry(
        "git",
        CatalogKind::McpExtension,
        "Read and inspect a local Git repository.",
        "Requires uv; launches via `uvx mcp-server-git`.",
        false,
    ),
    entry(
        "fetch",
        CatalogKind::McpExtension,
        "Fetch and convert web pages to text for grounding.",
        "Requires uv; launches via `uvx mcp-server-fetch`.",
        false,
    ),
    entry(
        "sarvam-doc",
        CatalogKind::McpExtension,
        "India-hosted document and translation gateway over a remote endpoint.",
        "Remote SSE gateway at mcp.sarvam.ai; no local install required.",
        true,
    ),
    entry(
        "krutrim-doc",
        CatalogKind::McpExtension,
        "India-hosted document gateway over a remote endpoint.",
        "Remote SSE gateway at mcp.olakrutrim.com; no local install required.",
        true,
    ),
    entry(
        "developer",
        CatalogKind::Builtin,
        "Built-in shell, file editing and code tools; available out of the box.",
        "Enabled by default; no install step required.",
        false,
    ),
];

/// Returns every curated catalog entry.
pub fn all() -> &'static [CatalogEntry] {
    CATALOG
}

/// Looks up a catalog entry by id (case-insensitive). Returns `None` if no
/// entry matches.
pub fn find(id: &str) -> Option<&'static CatalogEntry> {
    CATALOG
        .iter()
        .find(|entry| entry.id.eq_ignore_ascii_case(id))
}

/// Iterates over the catalog entries tagged as India-hosted.
pub fn india_hosted() -> impl Iterator<Item = &'static CatalogEntry> {
    CATALOG.iter().filter(|entry| entry.india_hosted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_non_empty() {
        assert!(!all().is_empty(), "expected at least one catalog entry");
    }

    #[test]
    fn ids_are_unique_non_empty_and_lowercase() {
        let mut seen = std::collections::HashSet::new();
        for e in all() {
            assert!(!e.id.is_empty(), "catalog entry has an empty id");
            assert_eq!(
                e.id.as_ref(),
                e.id.to_ascii_lowercase(),
                "id `{}` is not lowercase",
                e.id
            );
            assert!(
                seen.insert(e.id.as_ref()),
                "duplicate catalog id `{}`",
                e.id
            );
            assert!(
                !e.description.trim().is_empty(),
                "entry `{}` has an empty description",
                e.id
            );
            assert!(
                !e.install_hint.trim().is_empty(),
                "entry `{}` has an empty install hint",
                e.id
            );
        }
    }

    #[test]
    fn find_is_case_insensitive() {
        let lower = find("sarvam").expect("sarvam should resolve");
        assert_eq!(lower.id, "sarvam");
        let upper = find("SARVAM").expect("SARVAM should resolve");
        assert_eq!(upper.id, "sarvam");
        let mixed = find("Krutrim").expect("Krutrim should resolve");
        assert_eq!(mixed.id, "krutrim");
    }

    #[test]
    fn find_unknown_is_none() {
        assert!(find("does-not-exist").is_none());
    }

    #[test]
    fn declarative_providers_present_and_tagged() {
        let sarvam = find("sarvam").expect("sarvam provider present");
        assert_eq!(sarvam.kind, CatalogKind::Provider);
        assert!(sarvam.india_hosted, "sarvam must be tagged India-hosted");

        let krutrim = find("krutrim").expect("krutrim provider present");
        assert_eq!(krutrim.kind, CatalogKind::Provider);
        assert!(krutrim.india_hosted, "krutrim must be tagged India-hosted");

        let ollama = find("ollama").expect("ollama provider present");
        assert_eq!(ollama.kind, CatalogKind::Provider);
        assert!(
            !ollama.india_hosted,
            "ollama is local/open-weight, not India-hosted"
        );
    }

    #[test]
    fn india_hosted_filter_matches_tags() {
        let hosted: Vec<&str> = india_hosted().map(|e| e.id.as_ref()).collect();
        assert!(hosted.contains(&"sarvam"));
        assert!(hosted.contains(&"krutrim"));
        assert!(!hosted.contains(&"ollama"));
        assert_eq!(
            hosted.len(),
            all().iter().filter(|e| e.india_hosted).count(),
            "india_hosted() must yield exactly the tagged entries"
        );
    }

    #[test]
    fn provider_ids_align_with_planner_presets() {
        // Cross-check that provider catalog ids resolve through the planner
        // presets table shipped in the same binary, so the two tables agree.
        for preset in crate::providers::planner_presets::list_presets() {
            assert!(
                find(preset.provider).is_some(),
                "planner preset provider `{}` is missing from the catalog",
                preset.provider
            );
        }
    }

    #[test]
    fn mcp_extension_ids_align_with_registry() {
        // Every catalog entry tagged as an MCP extension must resolve in the
        // curated MCP registry, keeping ids stable across both tables.
        for e in all() {
            if e.kind == CatalogKind::McpExtension {
                assert!(
                    crate::providers::mcp_registry::lookup(e.id.as_ref()).is_some(),
                    "catalog MCP extension `{}` is not in the MCP registry",
                    e.id
                );
            }
        }
    }

    #[test]
    fn no_vendor_branding_leaks() {
        for e in all() {
            for needle in ["goose", "block"] {
                assert!(
                    !e.description.to_ascii_lowercase().contains(needle),
                    "entry `{}` description leaks `{}`",
                    e.id,
                    needle
                );
                assert!(
                    !e.install_hint.to_ascii_lowercase().contains(needle),
                    "entry `{}` install hint leaks `{}`",
                    e.id,
                    needle
                );
            }
        }
    }

    #[test]
    fn kind_labels_are_stable() {
        assert_eq!(CatalogKind::Provider.as_str(), "provider");
        assert_eq!(CatalogKind::McpExtension.as_str(), "mcp-extension");
        assert_eq!(CatalogKind::Builtin.as_str(), "builtin");
    }
}
