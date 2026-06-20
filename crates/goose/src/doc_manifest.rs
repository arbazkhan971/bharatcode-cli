//! Canonical documentation manifest: the single source of truth for the product
//! identity and the set of top-level "landing" pages that the offline docs-site
//! generator renders.
//!
//! This module is deliberately tiny and pure — no I/O, no clock, no environment
//! reads — so the page list is deterministic and trivially unit-testable. It is
//! owned here, in the core library, so that *both* the library and the CLI
//! docs-site generator are driven by one list rather than two copies that could
//! drift. The CLI's `docsite::generate_site` consumes [`pages`] to seed the
//! generated `index.md` table of contents; that consumption is the live wire
//! that keeps this manifest from being dead code.
//!
//! The product strings here are brand-clean by construction: they never surface
//! the upstream project name. Apache-2.0 attribution lives in `NOTICE` /
//! `MODIFICATIONS.md`, not in user-facing documentation chrome.
//!
//! Original BharatCode work; not ported from any third party.

/// User-facing product name. Single-sourced here so generated documentation,
/// the CLI banner area, and any future consumer all agree on the brand string.
pub const PRODUCT_NAME: &str = "BharatCode";

/// The CLI invocation binary name, used in command examples.
pub const BINARY_NAME: &str = "bharatcode";

/// One-line product tagline rendered on the documentation landing page.
pub const PRODUCT_TAGLINE: &str =
    "An offline-first AI coding agent for the Indian developer ecosystem.";

/// A single canonical documentation landing page.
///
/// These are the curated, hand-reviewed top-level pages (overview / getting
/// started / configuration / skills) that frame the auto-generated command
/// reference. The command pages themselves are derived live from the clap
/// command tree by the generator; this list is the stable scaffolding around
/// them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocPage {
    /// URL/file slug, e.g. `getting-started`. Stable; used as the `.md` stem.
    pub slug: &'static str,
    /// Human-readable page title shown in the table of contents and `# H1`.
    pub title: &'static str,
    /// One-line summary shown beside the title in the table of contents.
    pub summary: &'static str,
}

impl DocPage {
    /// File name this page renders to, e.g. `getting-started.md`.
    pub fn file_name(&self) -> String {
        format!("{}.md", self.slug)
    }
}

/// Curated, stable list of top-level documentation pages.
///
/// Ordering is significant: pages render in this order in the table of
/// contents. The list is guaranteed non-empty and contains no upstream brand
/// tokens, which the unit tests enforce.
pub fn pages() -> Vec<DocPage> {
    vec![
        DocPage {
            slug: "overview",
            title: "Overview",
            summary: "What BharatCode is and how the CLI is organized.",
        },
        DocPage {
            slug: "getting-started",
            title: "Getting started",
            summary: "Install, configure a provider, and run your first session.",
        },
        DocPage {
            slug: "configuration",
            title: "Configuration",
            summary: "BHARATCODE_* environment variables and config files.",
        },
        DocPage {
            slug: "commands",
            title: "Command reference",
            summary: "Every CLI command and subcommand, generated from the live surface.",
        },
        DocPage {
            slug: "skills",
            title: "Built-in skills",
            summary: "The skills bundled with the binary and how to invoke them.",
        },
        DocPage {
            slug: "offline",
            title: "Offline operation",
            summary: "Running fully offline with local models and no network egress.",
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_is_non_empty_and_stable() {
        let a = pages();
        let b = pages();
        assert!(!a.is_empty(), "doc manifest must list at least one page");
        assert_eq!(a, b, "pages() must be deterministic");
    }

    #[test]
    fn slugs_are_unique_and_filename_safe() {
        let pages = pages();
        let mut seen = std::collections::HashSet::new();
        for p in &pages {
            assert!(seen.insert(p.slug), "duplicate slug: {}", p.slug);
            assert!(
                p.slug
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '-' || c.is_ascii_digit()),
                "slug not filename-safe: {}",
                p.slug
            );
            assert_eq!(p.file_name(), format!("{}.md", p.slug));
            assert!(!p.title.is_empty());
            assert!(!p.summary.is_empty());
        }
    }

    #[test]
    fn manifest_is_brand_clean() {
        // Concatenate every string the manifest can surface and assert none of
        // it leaks an upstream brand token.
        let mut blob = String::new();
        blob.push_str(PRODUCT_NAME);
        blob.push(' ');
        blob.push_str(BINARY_NAME);
        blob.push(' ');
        blob.push_str(PRODUCT_TAGLINE);
        for p in pages() {
            blob.push(' ');
            blob.push_str(p.slug);
            blob.push(' ');
            blob.push_str(p.title);
            blob.push(' ');
            blob.push_str(p.summary);
        }
        let lower = blob.to_lowercase();
        assert!(
            !lower.contains("goose"),
            "manifest leaked an upstream token"
        );
        assert!(
            !lower.contains("block, inc"),
            "manifest leaked an upstream token"
        );
        assert!(lower.contains("bharatcode"));
    }
}
