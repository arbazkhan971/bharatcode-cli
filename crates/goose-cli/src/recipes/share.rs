//! Portable recipe sharing: `recipe export` / `recipe import`.
//!
//! [`export_recipe`] packs a recipe together with the local sub-recipe files it
//! references into a single self-describing, checksummed JSON bundle (a `.bcr`
//! file). [`import_recipe`] re-materializes that bundle on another machine,
//! writing the recipe and its attachments back into the local recipe library so
//! the recipe is immediately usable.
//!
//! The bundle round-trips byte-stably: the recipe and every attachment are
//! stored verbatim, and a SHA-256 digest is computed over a canonical
//! serialization of those bytes. Import recomputes the digest and refuses any
//! bundle whose checksum does not match, so tampered or corrupted bundles are
//! rejected before anything is written to disk.
//!
//! This is a regular, opt-in subcommand (`bharatcode recipe export|import`); it
//! is read-only except for import, which only writes under the recipe library
//! directory. Default CLI behavior is therefore unchanged.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::recipes::search_recipe::load_recipe_file;
use goose::recipe::local_recipes::get_recipe_library_dir;
use goose::recipe::read_recipe_file_content::read_recipe_file;
use goose::recipe::validate_recipe::validate_recipe_template_from_file;
use goose::recipe::Recipe;
use goose::utils::bytes_to_hex;

/// Current bundle format version. Bump on incompatible layout changes.
pub const BUNDLE_FORMAT_VERSION: u32 = 1;

/// File extension for exported recipe bundles ("BharatCode Recipe").
pub const BUNDLE_EXTENSION: &str = "bcr";

/// A single file packed alongside the primary recipe (e.g. a referenced local
/// sub-recipe). The `relative_path` is interpreted relative to the recipe
/// directory on import and is always sanitized to stay within it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BundledFile {
    /// Path relative to the recipe directory, used both as the sub-recipe
    /// reference inside the recipe and as the on-disk destination on import.
    pub relative_path: String,
    /// Verbatim file content, stored as UTF-8 for byte-identical round-trips.
    pub content: String,
}

/// A self-describing, checksummed, portable recipe bundle.
///
/// The recipe itself is stored as a parsed JSON value so the bundle is fully
/// self-describing; `attachments` carries the referenced local sub-recipe files
/// so the bundle is complete on its own. `sha256` is computed over a canonical
/// serialization of the recipe plus attachments and is verified on import.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecipeBundle {
    /// Bundle layout version, for forward/backward compatibility checks.
    pub format_version: u32,
    /// Logical recipe name (file stem of the source recipe).
    pub name: String,
    /// The recipe, parsed into JSON so the bundle is self-describing.
    pub recipe: serde_json::Value,
    /// Local sub-recipe files referenced by the recipe, packed verbatim.
    pub attachments: Vec<BundledFile>,
    /// Lowercase hex SHA-256 digest over the canonical bundle payload.
    pub sha256: String,
}

impl RecipeBundle {
    /// Compute the canonical SHA-256 digest (lowercase hex) over the bundle's
    /// content. The digest covers the recipe and every attachment via a stable,
    /// sorted serialization so the value is deterministic across machines.
    fn compute_digest(
        name: &str,
        recipe: &serde_json::Value,
        attachments: &[BundledFile],
    ) -> String {
        let mut hasher = Sha256::new();
        hasher.update(name.as_bytes());
        hasher.update([0u8]);
        // serde_json sorts object keys deterministically only when configured;
        // to_string preserves the value's internal map order, which is stable
        // for a given value, so digest is reproducible for that value.
        hasher.update(recipe.to_string().as_bytes());

        let mut sorted: Vec<&BundledFile> = attachments.iter().collect();
        sorted.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        for file in sorted {
            hasher.update([0u8]);
            hasher.update(file.relative_path.as_bytes());
            hasher.update([0u8]);
            hasher.update(file.content.as_bytes());
        }
        bytes_to_hex(hasher.finalize())
    }

    fn new(name: String, recipe: serde_json::Value, attachments: Vec<BundledFile>) -> Self {
        let sha256 = Self::compute_digest(&name, &recipe, &attachments);
        RecipeBundle {
            format_version: BUNDLE_FORMAT_VERSION,
            name,
            recipe,
            attachments,
            sha256,
        }
    }

    /// Verify the bundle's format version and content integrity.
    ///
    /// Returns an error if the format version is unsupported or if the stored
    /// digest does not match a freshly computed digest.
    pub fn verify(&self) -> Result<()> {
        if self.format_version > BUNDLE_FORMAT_VERSION {
            bail!(
                "unsupported recipe bundle format version (bundle: {}, supported: {})",
                self.format_version,
                BUNDLE_FORMAT_VERSION
            );
        }
        let recomputed = Self::compute_digest(&self.name, &self.recipe, &self.attachments);
        if recomputed != self.sha256 {
            bail!(
                "recipe bundle checksum verification failed (expected: {}, computed: {}); refusing tampered bundle",
                self.sha256,
                recomputed
            );
        }
        Ok(())
    }
}

/// Derive the logical recipe name (file stem) from a recipe reference.
fn recipe_name_for(reference: &str, file_path: &Path) -> String {
    file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| reference.to_string())
}

/// Collect the local sub-recipe files referenced by a recipe as attachments.
///
/// Only relative, in-tree references are packed; absolute paths, `..` escapes,
/// and references that fail to resolve are skipped (the recipient is expected to
/// supply those out of band). The collected reference's `path` doubles as the
/// attachment's `relative_path`.
fn collect_attachments(recipe: &Recipe, recipe_dir: &Path) -> Vec<BundledFile> {
    let mut attachments = Vec::new();
    let Some(sub_recipes) = &recipe.sub_recipes else {
        return attachments;
    };

    for sub in sub_recipes {
        let rel = sub.path.trim();
        if rel.is_empty() || is_unsafe_relative(rel) {
            continue;
        }
        let candidate = recipe_dir.join(rel);
        if let Ok(file) = read_recipe_file(&candidate) {
            if !attachments
                .iter()
                .any(|a: &BundledFile| a.relative_path == rel)
            {
                attachments.push(BundledFile {
                    relative_path: rel.to_string(),
                    content: file.content,
                });
            }
        }
    }
    attachments
}

/// Reject references that are absolute or escape the recipe directory.
fn is_unsafe_relative(rel: &str) -> bool {
    let path = Path::new(rel);
    if path.is_absolute() || rel.starts_with('~') {
        return true;
    }
    path.components().any(|c| {
        matches!(
            c,
            std::path::Component::ParentDir | std::path::Component::Prefix(_)
        )
    })
}

/// Build a [`RecipeBundle`] from a recipe reference.
///
/// The recipe is loaded and validated via the shared recipe validator before
/// packaging; the parsed recipe is stored as JSON and its referenced local
/// sub-recipes are packed as attachments.
pub fn build_bundle(recipe_name: &str) -> Result<RecipeBundle> {
    let recipe_file = load_recipe_file(recipe_name)
        .with_context(|| format!("could not load recipe '{}'", recipe_name))?;

    let recipe = validate_recipe_template_from_file(&recipe_file)
        .map_err(|err| anyhow!("recipe '{}' is invalid: {}", recipe_name, err))?;

    let recipe_json = serde_json::to_value(&recipe)
        .map_err(|err| anyhow!("failed to serialize recipe '{}': {}", recipe_name, err))?;

    let attachments = collect_attachments(&recipe, &recipe_file.parent_dir);
    let name = recipe_name_for(recipe_name, &recipe_file.file_path);

    Ok(RecipeBundle::new(name, recipe_json, attachments))
}

/// Export a validated recipe to a `.bcr` bundle file.
///
/// When `output` is `None`, the bundle is written to `<recipe-name>.bcr` in the
/// current working directory. Returns the path the bundle was written to.
pub fn export_recipe(name: &str, output: Option<PathBuf>) -> Result<PathBuf> {
    let bundle = build_bundle(name)?;

    let out_path =
        output.unwrap_or_else(|| PathBuf::from(format!("{}.{}", bundle.name, BUNDLE_EXTENSION)));

    let serialized = serde_json::to_string_pretty(&bundle)
        .map_err(|err| anyhow!("failed to serialize bundle: {}", err))?;

    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
    }
    std::fs::write(&out_path, serialized)
        .with_context(|| format!("failed to write bundle to {}", out_path.display()))?;

    println!(
        "exported recipe '{}' -> {}",
        bundle.name,
        out_path.display()
    );
    if !bundle.attachments.is_empty() {
        println!(
            "bundled {} sub-recipe attachment(s)",
            bundle.attachments.len()
        );
    }
    Ok(out_path)
}

/// Read bundle bytes from a local path or an `http(s)` URL.
fn read_bundle_source(src: &str) -> Result<String> {
    if src.starts_with("http://") || src.starts_with("https://") {
        let response = reqwest::blocking::get(src)
            .with_context(|| format!("failed to fetch bundle from {}", src))?;
        if !response.status().is_success() {
            bail!(
                "failed to fetch bundle from {}: HTTP {}",
                src,
                response.status()
            );
        }
        response
            .text()
            .with_context(|| format!("failed to read bundle body from {}", src))
    } else {
        std::fs::read_to_string(src).with_context(|| format!("failed to read bundle {}", src))
    }
}

/// Import a recipe bundle from a local `.bcr` file or a URL.
///
/// Verifies the bundle's format version and SHA-256 integrity, then writes the
/// recipe and its attachments into the global recipe library directory. All
/// writes are confined to that directory. Returns the path of the imported
/// recipe file.
pub fn import_recipe(src: &str) -> Result<PathBuf> {
    let raw = read_bundle_source(src)?;

    let bundle: RecipeBundle = serde_json::from_str(&raw)
        .map_err(|err| anyhow!("bundle {} is not valid: {}", src, err))?;

    bundle.verify()?;

    let recipe_dir = get_recipe_library_dir(true);
    std::fs::create_dir_all(&recipe_dir)
        .with_context(|| format!("failed to create {}", recipe_dir.display()))?;

    materialize_bundle(&bundle, &recipe_dir)
}

/// Write a verified bundle's recipe and attachments under `recipe_dir`.
///
/// Split out so tests can re-materialize into a temp directory without touching
/// the user's real recipe library.
fn materialize_bundle(bundle: &RecipeBundle, recipe_dir: &Path) -> Result<PathBuf> {
    for attachment in &bundle.attachments {
        if is_unsafe_relative(&attachment.relative_path) {
            bail!(
                "refusing to import attachment with unsafe path: {}",
                attachment.relative_path
            );
        }
        let dest = recipe_dir.join(&attachment.relative_path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        std::fs::write(&dest, &attachment.content)
            .with_context(|| format!("failed to write attachment {}", dest.display()))?;
    }

    let recipe_path = recipe_dir.join(format!("{}.json", bundle.name));
    let serialized = serde_json::to_string_pretty(&bundle.recipe)
        .map_err(|err| anyhow!("failed to serialize recipe: {}", err))?;
    std::fs::write(&recipe_path, serialized)
        .with_context(|| format!("failed to write recipe to {}", recipe_path.display()))?;

    println!(
        "imported recipe '{}' -> {}",
        bundle.name,
        recipe_path.display()
    );
    Ok(recipe_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    const SAMPLE_RECIPE: &str = r#"version: 1.0.0
title: Share Sample
description: A recipe used to exercise the share bundle round-trip
instructions: Do the thing carefully on {{ target }}
prompt: Start the session for {{ target }}
parameters:
  - key: target
    input_type: string
    requirement: required
    description: The target to operate on
"#;

    fn write_recipe(dir: &TempDir, name: &str, content: &str) -> PathBuf {
        let path = dir.path().join(name);
        fs::write(&path, content).expect("write recipe");
        path
    }

    #[test]
    fn build_bundle_validates_and_packs() {
        let src = TempDir::new().unwrap();
        let recipe_path = write_recipe(&src, "sample.yaml", SAMPLE_RECIPE);

        let bundle = build_bundle(recipe_path.to_str().unwrap()).expect("build bundle");
        assert_eq!(bundle.format_version, BUNDLE_FORMAT_VERSION);
        assert_eq!(bundle.name, "sample");
        assert_eq!(bundle.recipe["title"], "Share Sample");
        assert!(bundle.attachments.is_empty());
        bundle.verify().expect("freshly built bundle must verify");
    }

    #[test]
    fn export_then_import_round_trips_recipe_json() {
        let src = TempDir::new().unwrap();
        let recipe_path = write_recipe(&src, "sample.yaml", SAMPLE_RECIPE);

        let out = TempDir::new().unwrap();
        let bundle_path = out.path().join("sample.bcr");
        let written = export_recipe(recipe_path.to_str().unwrap(), Some(bundle_path.clone()))
            .expect("export");
        assert_eq!(written, bundle_path);
        assert!(bundle_path.exists());

        // Re-read the exported bundle and re-materialize into a fresh recipe dir.
        let raw = fs::read_to_string(&bundle_path).unwrap();
        let bundle: RecipeBundle = serde_json::from_str(&raw).unwrap();
        bundle.verify().expect("exported bundle verifies");

        let import_dir = TempDir::new().unwrap();
        let imported =
            materialize_bundle(&bundle, import_dir.path()).expect("materialize into temp dir");

        // The imported recipe JSON must be byte-identical to the bundle's value.
        let imported_value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&imported).unwrap()).unwrap();
        assert_eq!(
            imported_value, bundle.recipe,
            "round-trip must reproduce identical recipe JSON"
        );
    }

    #[test]
    fn sub_recipe_is_packed_and_restored() {
        let src = TempDir::new().unwrap();
        let sub_content = "version: 1.0.0\ntitle: Sub\ndescription: sub\ninstructions: do sub\n";
        write_recipe(&src, "child.yaml", sub_content);

        let parent = format!(
            "{}sub_recipes:\n  - name: child\n    path: child.yaml\n",
            SAMPLE_RECIPE
        );
        let parent_path = write_recipe(&src, "parent.yaml", &parent);

        let bundle = build_bundle(parent_path.to_str().unwrap()).expect("build bundle");
        assert_eq!(bundle.attachments.len(), 1);
        assert_eq!(bundle.attachments[0].relative_path, "child.yaml");
        assert_eq!(bundle.attachments[0].content, sub_content);

        let import_dir = TempDir::new().unwrap();
        materialize_bundle(&bundle, import_dir.path()).expect("materialize");

        let restored = fs::read_to_string(import_dir.path().join("child.yaml")).unwrap();
        assert_eq!(restored, sub_content, "sub-recipe must round-trip verbatim");
    }

    #[test]
    fn tampered_checksum_is_rejected() {
        let src = TempDir::new().unwrap();
        let recipe_path = write_recipe(&src, "sample.yaml", SAMPLE_RECIPE);

        let mut bundle = build_bundle(recipe_path.to_str().unwrap()).expect("build bundle");
        // Mutate the recipe payload but leave the recorded sha256 untouched.
        bundle.recipe["title"] = serde_json::Value::String("Hijacked".to_string());

        let err = bundle
            .verify()
            .expect_err("tampered bundle must be rejected");
        assert!(
            err.to_string().to_lowercase().contains("checksum")
                || err.to_string().to_lowercase().contains("integrity"),
            "error should mention the integrity failure: {}",
            err
        );
    }

    #[test]
    fn verify_rejects_future_format_version() {
        let src = TempDir::new().unwrap();
        let recipe_path = write_recipe(&src, "sample.yaml", SAMPLE_RECIPE);
        let mut bundle = build_bundle(recipe_path.to_str().unwrap()).expect("build bundle");
        bundle.format_version = BUNDLE_FORMAT_VERSION + 1;
        assert!(bundle.verify().is_err());
    }

    #[test]
    fn unsafe_relative_paths_are_detected() {
        assert!(is_unsafe_relative("../escape.yaml"));
        assert!(is_unsafe_relative("/abs/path.yaml"));
        assert!(is_unsafe_relative("nested/../../escape.yaml"));
        assert!(!is_unsafe_relative("child.yaml"));
        assert!(!is_unsafe_relative("nested/child.yaml"));
    }
}
