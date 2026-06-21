//! Portable recipe sharing: export a validated recipe into a self-describing,
//! versioned JSON bundle and import it back out with integrity verification.
//!
//! A bundle captures the recipe's original file content verbatim so that an
//! export → import round-trip reproduces byte-identical recipe content. The
//! bundle also carries a SHA-256 digest over that content; import recomputes the
//! digest and refuses any bundle whose recipe content has been tampered with.
//!
//! This module is opt-in: the `run` dispatcher only activates when the
//! `BHARATCODE_RECIPE_SHARE` environment variable is set, so default CLI
//! behavior is unchanged.

use anyhow::{anyhow, bail, Context, Result};
use console::style;
use bharatcode_core::utils::bytes_to_hex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::recipes::search_recipe::load_recipe_file;
use bharatcode_core::recipe::validate_recipe::validate_recipe_template_from_file;

/// Environment variable that opts the `recipe-share` dispatcher in.
pub const RECIPE_SHARE_ENV: &str = "BHARATCODE_RECIPE_SHARE";

/// Current bundle schema version. Bump on incompatible bundle layout changes.
pub const BUNDLE_SCHEMA_VERSION: u32 = 1;

/// Attribution marker written into every bundle this binary produces.
pub const EXPORTED_BY: &str = "bharatcode";

const USAGE: &str =
    "usage: recipe-share export <recipe> [-o bundle.json] | recipe-share import <bundle.json> [--dir <out>]";

/// A self-describing, versioned, integrity-checked recipe bundle.
///
/// The bundle is intentionally portable: it carries the recipe content verbatim
/// (`recipe_yaml`) alongside a SHA-256 digest over those exact bytes so that a
/// recipient can verify the content has not been altered in transit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecipeBundle {
    /// Bundle layout version, for forward/backward compatibility checks.
    pub schema_version: u32,
    /// Logical recipe name (file stem of the source recipe).
    pub name: String,
    /// The recipe file content, stored verbatim for byte-identical round-trips.
    pub recipe_yaml: String,
    /// Lowercase hex SHA-256 digest over `recipe_yaml`'s UTF-8 bytes.
    pub sha256: String,
    /// Attribution marker identifying the producing tool.
    pub exported_by: String,
    /// RFC 3339 timestamp of when the bundle was produced.
    pub exported_at: String,
}

impl RecipeBundle {
    /// Compute the canonical SHA-256 digest (lowercase hex) over recipe content.
    pub fn digest(recipe_yaml: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(recipe_yaml.as_bytes());
        bytes_to_hex(hasher.finalize())
    }

    /// Build a bundle from a recipe name and its verbatim content.
    fn new(name: String, recipe_yaml: String) -> Self {
        let sha256 = Self::digest(&recipe_yaml);
        RecipeBundle {
            schema_version: BUNDLE_SCHEMA_VERSION,
            name,
            recipe_yaml,
            sha256,
            exported_by: EXPORTED_BY.to_string(),
            exported_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    /// Verify the bundle's schema version and content integrity.
    ///
    /// Returns an error if the schema version is unsupported or if the stored
    /// digest does not match a freshly computed digest over `recipe_yaml`.
    pub fn verify(&self) -> Result<()> {
        if self.schema_version > BUNDLE_SCHEMA_VERSION {
            bail!(
                "unsupported recipe bundle schema version (bundle: {}, supported: {})",
                self.schema_version,
                BUNDLE_SCHEMA_VERSION
            );
        }

        let recomputed = Self::digest(&self.recipe_yaml);
        if recomputed != self.sha256 {
            bail!(
                "recipe bundle integrity check failed: recipe content does not match recorded \
                 sha256 (expected: {}, computed: {})",
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

/// Load and validate a recipe, then package it into a [`RecipeBundle`].
///
/// The recipe is validated (via the shared recipe validator) before packaging,
/// but the *original* file content is stored verbatim so import reproduces
/// byte-identical content.
pub fn build_bundle(recipe_name: &str) -> Result<RecipeBundle> {
    let recipe_file = load_recipe_file(recipe_name)
        .with_context(|| format!("could not load recipe '{}'", recipe_name))?;

    validate_recipe_template_from_file(&recipe_file)
        .map_err(|err| anyhow!("recipe '{}' is invalid: {}", recipe_name, err))?;

    let name = recipe_name_for(recipe_name, &recipe_file.file_path);
    Ok(RecipeBundle::new(name, recipe_file.content))
}

/// Export a validated recipe to a JSON bundle file.
///
/// When `out` is `None`, the bundle is written to `<recipe-name>.bundle.json`
/// in the current working directory. Returns the path the bundle was written to.
pub fn export(recipe_name: &str, out: Option<PathBuf>) -> Result<PathBuf> {
    let bundle = build_bundle(recipe_name)?;

    let out_path = out.unwrap_or_else(|| PathBuf::from(format!("{}.bundle.json", bundle.name)));

    let serialized = serde_json::to_string_pretty(&bundle)
        .map_err(|err| anyhow!("failed to serialize bundle: {}", err))?;

    std::fs::write(&out_path, serialized)
        .with_context(|| format!("failed to write bundle to {}", out_path.display()))?;

    println!(
        "{} exported recipe bundle to {}",
        style("✓").green().bold(),
        out_path.display()
    );
    Ok(out_path)
}

/// Import a recipe bundle: verify integrity, then write the recipe back out.
///
/// Reads the bundle at `bundle_path`, verifies its schema version and SHA-256
/// integrity, and writes the verbatim recipe content into `out_dir` (defaulting
/// to the current working directory) as `<name>.yaml`. Returns the path written.
pub fn import(bundle_path: &Path, out_dir: Option<PathBuf>) -> Result<PathBuf> {
    let raw = std::fs::read_to_string(bundle_path)
        .with_context(|| format!("failed to read bundle {}", bundle_path.display()))?;

    let bundle: RecipeBundle = serde_json::from_str(&raw)
        .map_err(|err| anyhow!("bundle {} is not valid: {}", bundle_path.display(), err))?;

    bundle.verify()?;

    let dir = out_dir.unwrap_or_else(|| PathBuf::from("."));
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create output directory {}", dir.display()))?;

    let out_path = dir.join(format!("{}.yaml", bundle.name));
    std::fs::write(&out_path, &bundle.recipe_yaml)
        .with_context(|| format!("failed to write recipe to {}", out_path.display()))?;

    println!(
        "{} imported recipe to {}",
        style("✓").green().bold(),
        out_path.display()
    );
    Ok(out_path)
}

/// Clap-free dispatcher for `recipe-share` so the feature can be wired from a
/// single one-line call site without depending on the (separately owned) clap
/// command tree in `cli.rs`.
///
/// Recognized invocations:
///   - `export <recipe> [-o <bundle.json>]`
///   - `import <bundle.json> [--dir <out>]`
///
/// Opt-in: returns `Ok(())` without doing anything unless
/// `BHARATCODE_RECIPE_SHARE` is set, keeping default behavior unchanged.
pub fn run(args: &[String]) -> Result<()> {
    if std::env::var(RECIPE_SHARE_ENV).is_err() {
        return Ok(());
    }
    dispatch(args).map(|_| ())
}

/// Parse and execute a `recipe-share` subcommand, returning the path produced.
///
/// Separated from [`run`] (which applies the env gate and discards the path) so
/// tests can exercise dispatch directly.
pub fn dispatch(args: &[String]) -> Result<PathBuf> {
    let (sub, rest) = args.split_first().ok_or_else(|| anyhow!("{}", USAGE))?;

    match sub.as_str() {
        "export" => {
            let (recipe, out) = parse_export_args(rest)?;
            export(&recipe, out)
        }
        "import" => {
            let (bundle, dir) = parse_import_args(rest)?;
            import(Path::new(&bundle), dir)
        }
        other => bail!("unknown recipe-share subcommand: '{}'\n{}", other, USAGE),
    }
}

fn parse_export_args(rest: &[String]) -> Result<(String, Option<PathBuf>)> {
    let mut recipe: Option<String> = None;
    let mut out: Option<PathBuf> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-o" | "--output" => {
                i += 1;
                let value = rest
                    .get(i)
                    .ok_or_else(|| anyhow!("-o/--output requires a file path"))?;
                out = Some(PathBuf::from(value));
            }
            value if recipe.is_none() => recipe = Some(value.to_string()),
            value => bail!("unexpected argument: {}", value),
        }
        i += 1;
    }
    let recipe = recipe.ok_or_else(|| anyhow!("a recipe name or path is required for export"))?;
    Ok((recipe, out))
}

fn parse_import_args(rest: &[String]) -> Result<(String, Option<PathBuf>)> {
    let mut bundle: Option<String> = None;
    let mut dir: Option<PathBuf> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--dir" => {
                i += 1;
                let value = rest
                    .get(i)
                    .ok_or_else(|| anyhow!("--dir requires a directory path"))?;
                dir = Some(PathBuf::from(value));
            }
            value if bundle.is_none() => bundle = Some(value.to_string()),
            value => bail!("unexpected argument: {}", value),
        }
        i += 1;
    }
    let bundle = bundle.ok_or_else(|| anyhow!("a bundle path is required for import"))?;
    Ok((bundle, dir))
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
    fn digest_is_stable_sha256_hex() {
        let d = RecipeBundle::digest("hello");
        // SHA-256 of "hello"
        assert_eq!(
            d,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(d.len(), 64);
        assert!(d.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn export_then_import_round_trips_byte_identical() {
        let src = TempDir::new().unwrap();
        let recipe_path = write_recipe(&src, "sample.yaml", SAMPLE_RECIPE);

        let out = TempDir::new().unwrap();
        let bundle_path = out.path().join("sample.bundle.json");

        let written =
            export(recipe_path.to_str().unwrap(), Some(bundle_path.clone())).expect("export");
        assert_eq!(written, bundle_path);

        let import_dir = TempDir::new().unwrap();
        let imported = import(&bundle_path, Some(import_dir.path().to_path_buf())).expect("import");

        let imported_content = fs::read_to_string(&imported).unwrap();
        assert_eq!(
            imported_content, SAMPLE_RECIPE,
            "round-trip must reproduce byte-identical recipe content"
        );
    }

    #[test]
    fn bundle_carries_bharatcode_attribution_and_schema() {
        let src = TempDir::new().unwrap();
        let recipe_path = write_recipe(&src, "sample.yaml", SAMPLE_RECIPE);

        let bundle = build_bundle(recipe_path.to_str().unwrap()).expect("build bundle");
        assert_eq!(bundle.exported_by, "bharatcode");
        assert_eq!(bundle.schema_version, BUNDLE_SCHEMA_VERSION);
        assert_eq!(bundle.name, "sample");
        assert_eq!(bundle.recipe_yaml, SAMPLE_RECIPE);
        assert_eq!(bundle.sha256, RecipeBundle::digest(SAMPLE_RECIPE));
        assert!(!bundle.exported_at.is_empty());
        bundle.verify().expect("freshly built bundle must verify");
    }

    #[test]
    fn tampered_recipe_content_fails_integrity() {
        let src = TempDir::new().unwrap();
        let recipe_path = write_recipe(&src, "sample.yaml", SAMPLE_RECIPE);

        let out = TempDir::new().unwrap();
        let bundle_path = out.path().join("sample.bundle.json");
        export(recipe_path.to_str().unwrap(), Some(bundle_path.clone())).expect("export");

        // Flip one byte of the recipe content inside the serialized bundle while
        // leaving the recorded sha256 untouched, then attempt import.
        let mut bundle: RecipeBundle =
            serde_json::from_str(&fs::read_to_string(&bundle_path).unwrap()).unwrap();
        let mut bytes = bundle.recipe_yaml.into_bytes();
        bytes[0] ^= 0x01;
        bundle.recipe_yaml = String::from_utf8(bytes).unwrap();
        fs::write(&bundle_path, serde_json::to_string_pretty(&bundle).unwrap()).unwrap();

        let import_dir = TempDir::new().unwrap();
        let result = import(&bundle_path, Some(import_dir.path().to_path_buf()));
        assert!(
            result.is_err(),
            "tampered bundle must fail integrity verification"
        );
    }

    #[test]
    fn verify_rejects_future_schema_version() {
        let mut bundle = RecipeBundle::new("x".to_string(), "title: x\n".to_string());
        bundle.schema_version = BUNDLE_SCHEMA_VERSION + 1;
        assert!(bundle.verify().is_err());
    }

    #[test]
    fn dispatch_export_then_import_round_trips() {
        let src = TempDir::new().unwrap();
        let recipe_path = write_recipe(&src, "flow.yaml", SAMPLE_RECIPE);
        let out = TempDir::new().unwrap();
        let bundle_path = out.path().join("flow.bundle.json");

        dispatch(&[
            "export".to_string(),
            recipe_path.to_string_lossy().into_owned(),
            "-o".to_string(),
            bundle_path.to_string_lossy().into_owned(),
        ])
        .expect("dispatch export");
        assert!(bundle_path.exists());

        let import_dir = TempDir::new().unwrap();
        dispatch(&[
            "import".to_string(),
            bundle_path.to_string_lossy().into_owned(),
            "--dir".to_string(),
            import_dir.path().to_string_lossy().into_owned(),
        ])
        .expect("dispatch import");

        let imported = fs::read_to_string(import_dir.path().join("flow.yaml")).unwrap();
        assert_eq!(imported, SAMPLE_RECIPE);
    }

    #[test]
    fn dispatch_rejects_unknown_subcommand() {
        let err = dispatch(&["frobnicate".to_string()]).unwrap_err();
        assert!(err.to_string().to_lowercase().contains("frobnicate"));
    }

    #[test]
    fn run_is_inert_without_env_gate() {
        // Without the opt-in env var, run must be a no-op even with bad args.
        let prev = std::env::var(RECIPE_SHARE_ENV).ok();
        std::env::remove_var(RECIPE_SHARE_ENV);
        let result = run(&["export".to_string()]);
        if let Some(v) = prev {
            std::env::set_var(RECIPE_SHARE_ENV, v);
        }
        assert!(result.is_ok());
    }
}
