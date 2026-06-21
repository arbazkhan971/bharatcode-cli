//! Curated recipe/template library for common Indian developer workflows.
//!
//! These are pure-data recipe templates embedded into the binary via
//! [`include_str!`]. They give users a quick starting point for India-specific
//! tasks (UPI payment review, Aadhaar/PII audits, GST invoicing, Indic
//! localization) without hunting for a remote registry.
//!
//! The thin CLI here only lists and prints the bundled templates; it does not
//! execute them. Run a template with the existing `recipe`/`run` commands by
//! saving its YAML to a file first.
//!
//! Original BharatCode work; not ported from any third party.

use console::style;
use serde::Deserialize;

/// A single embedded library template: a stable id plus its raw YAML.
pub struct LibraryRecipe {
    /// Stable, lowercase id used to select the template on the command line.
    pub id: &'static str,
    /// The full recipe YAML, embedded at compile time.
    pub yaml: &'static str,
}

/// Lightweight view of the recipe header used only for listing.
#[derive(Debug, Deserialize)]
struct RecipeMeta {
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
}

/// The curated set of India-focused recipe templates.
pub fn library_recipes() -> Vec<LibraryRecipe> {
    vec![
        LibraryRecipe {
            id: "upi-payment-review",
            yaml: include_str!("../recipes/library/upi_payment_review.yaml"),
        },
        LibraryRecipe {
            id: "aadhaar-pii-audit",
            yaml: include_str!("../recipes/library/aadhaar_pii_audit.yaml"),
        },
        LibraryRecipe {
            id: "gst-invoice-helper",
            yaml: include_str!("../recipes/library/gst_invoice_helper.yaml"),
        },
        LibraryRecipe {
            id: "indic-localization",
            yaml: include_str!("../recipes/library/indic_localization.yaml"),
        },
    ]
}

fn parse_meta(yaml: &str) -> RecipeMeta {
    serde_yaml::from_str(yaml).unwrap_or(RecipeMeta {
        title: String::new(),
        description: String::new(),
    })
}

/// Print the curated recipe library as a human-readable listing.
pub fn print_library() {
    println!(
        "{}",
        style(crate::tr!("recipes_library.header")).color256(208).bold()
    );
    println!();

    let recipes = library_recipes();
    let id_width = recipes
        .iter()
        .map(|r| r.id.len())
        .max()
        .unwrap_or(0)
        .max(12);

    for recipe in &recipes {
        let meta = parse_meta(recipe.yaml);
        println!(
            "  {:<id_width$}  {}",
            style(recipe.id).bold(),
            style(&meta.title).green(),
            id_width = id_width,
        );
        let description = meta
            .description
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if !description.is_empty() {
            println!(
                "  {:<id_width$}  {}",
                "",
                style(description).dim(),
                id_width = id_width
            );
        }
        println!();
    }

    println!("{}", style(crate::tr!("recipes_library.footer")).dim());
}

/// Print the raw YAML of a single template by id, returning an error if unknown.
pub fn show_recipe(id: &str) -> anyhow::Result<()> {
    let recipes = library_recipes();
    match recipes.iter().find(|r| r.id == id) {
        Some(recipe) => {
            print!("{}", recipe.yaml);
            Ok(())
        }
        None => {
            let known = recipes.iter().map(|r| r.id).collect::<Vec<_>>().join(", ");
            Err(anyhow::anyhow!(
                "{} '{}'. {}: {}",
                crate::tr!("recipes_library.unknown"),
                id,
                crate::tr!("recipes_library.available"),
                known
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_template_has_unique_id_and_parses() {
        let recipes = library_recipes();
        assert!(!recipes.is_empty());

        let mut ids = std::collections::HashSet::new();
        for recipe in &recipes {
            assert!(ids.insert(recipe.id), "duplicate id: {}", recipe.id);
            let meta = parse_meta(recipe.yaml);
            assert!(!meta.title.is_empty(), "{} has no title", recipe.id);
            assert!(
                !meta.description.is_empty(),
                "{} has no description",
                recipe.id
            );
        }
    }

    #[test]
    fn show_recipe_unknown_id_errors() {
        assert!(show_recipe("does-not-exist").is_err());
    }

    #[test]
    fn show_recipe_known_id_ok() {
        let id = library_recipes()[0].id;
        assert!(show_recipe(id).is_ok());
    }
}
