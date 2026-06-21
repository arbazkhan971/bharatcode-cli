//! Recipe import/export round-trip hardening at session build.
//!
//! Shared and imported recipes are only reproducible if everyone agrees on what
//! the recipe *is*. A recipe file can carry incidental noise — comment churn,
//! key reordering, trailing-whitespace drift — that doesn't change its meaning
//! but does change its bytes. To pin the *meaning* rather than the *bytes*, this
//! module canonicalizes a recipe (parse → normalize → re-serialize) and hashes
//! that canonical form. The hash, the canonical recipe text, and a timestamp are
//! persisted to a `.bharatcode/recipe.lock` sidecar next to the working
//! directory, so a later session can detect whether the live recipe has drifted
//! from the one that was locked.
//!
//! This is entirely opt-in. It activates only when `BHARATCODE_RECIPE_LOCK`
//! points at a recipe file; with the variable unset every entry point here is a
//! no-op and default behavior is unchanged.
//!
//! ## Outcomes
//!
//! [`lock_recipe`] returns a [`LockOutcome`]:
//!
//! - [`LockOutcome::Fresh`] — no prior lock existed; one was written.
//! - [`LockOutcome::Matched`] — a prior lock existed and the canonical hash
//!   still matches, so the recipe is reproducible.
//! - [`LockOutcome::Drifted`] — a prior lock existed but the canonical hash
//!   changed; the recorded lock is left untouched so the original pin survives,
//!   and the caller is expected to warn.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Environment variable that, when set to a recipe file path, enables recipe
/// locking for the session being built. Unset (the default) means disabled.
const RECIPE_LOCK_ENV: &str = "BHARATCODE_RECIPE_LOCK";

/// Directory (relative to the current working directory) that holds the sidecar.
const LOCK_DIR: &str = ".bharatcode";

/// File name of the persisted lock sidecar inside [`LOCK_DIR`].
const LOCK_FILE: &str = "recipe.lock";

/// Result of reconciling the live recipe against any previously persisted lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockOutcome {
    /// No prior lock existed; a fresh one was written.
    Fresh,
    /// A prior lock existed and the canonical hash still matches.
    Matched,
    /// A prior lock existed but the canonical hash changed (the live recipe
    /// drifted). The recorded lock is preserved.
    Drifted,
}

/// On-disk shape of the `.bharatcode/recipe.lock` sidecar.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LockFile {
    /// Hex SHA-256 of the canonicalized recipe text.
    hash: String,
    /// The canonicalized recipe text the hash was computed over.
    recipe: String,
    /// RFC 3339 timestamp of when the lock was first written.
    locked_at: String,
}

/// Whether recipe locking is enabled for this session.
///
/// Enabled only when [`RECIPE_LOCK_ENV`] is set to a non-empty path.
pub fn is_enabled() -> bool {
    recipe_path().is_some()
}

/// The recipe file path requested via [`RECIPE_LOCK_ENV`], if any.
///
/// Surrounding whitespace is trimmed; an empty value is treated as unset.
pub fn recipe_path() -> Option<PathBuf> {
    let raw = std::env::var(RECIPE_LOCK_ENV).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

/// Compute the canonical form of a recipe at `path`: parse it, normalize it, and
/// re-serialize it to YAML so that semantically-identical recipes produce
/// byte-identical text (and therefore an identical hash).
fn canonicalize(path: &Path) -> Result<String> {
    let recipe = bharatcode_core::recipe::Recipe::from_file_path(path)
        .with_context(|| format!("could not read recipe at {}", path.display()))?;
    recipe.to_yaml()
}

/// Hex SHA-256 digest of the canonical recipe text.
fn hash_canonical(canonical: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    bharatcode_core::utils::bytes_to_hex(hasher.finalize())
}

/// Lock the recipe at `path`, persisting (or reconciling against) the
/// `.bharatcode/recipe.lock` sidecar.
///
/// Reads the recipe text, canonicalizes it, and hashes that canonical form. If
/// no lock exists yet the sidecar is written and [`LockOutcome::Fresh`] is
/// returned. If a lock exists and the hash matches, [`LockOutcome::Matched`] is
/// returned. If the hash differs the existing lock is left in place (so the
/// original pin is preserved) and [`LockOutcome::Drifted`] is returned.
pub fn lock_recipe(path: &Path) -> Result<LockOutcome> {
    lock_recipe_in(path, Path::new(LOCK_DIR))
}

/// Implementation of [`lock_recipe`] with an explicit lock directory, so tests
/// can isolate the sidecar without depending on the process working directory.
fn lock_recipe_in(path: &Path, lock_dir: &Path) -> Result<LockOutcome> {
    let canonical = canonicalize(path)?;
    let hash = hash_canonical(&canonical);

    let lock_path = lock_dir.join(LOCK_FILE);

    if let Some(existing) = read_lock(&lock_path)? {
        if existing.hash == hash {
            return Ok(LockOutcome::Matched);
        }
        return Ok(LockOutcome::Drifted);
    }

    let lock = LockFile {
        hash,
        recipe: canonical,
        locked_at: chrono::Utc::now().to_rfc3339(),
    };

    std::fs::create_dir_all(lock_dir)
        .with_context(|| format!("could not create lock dir {}", lock_dir.display()))?;
    let serialized = serde_json::to_string_pretty(&lock).context("could not serialize lock")?;
    std::fs::write(&lock_path, serialized)
        .with_context(|| format!("could not write lock {}", lock_path.display()))?;

    Ok(LockOutcome::Fresh)
}

/// Read and parse an existing lock sidecar, if present. A missing file yields
/// `Ok(None)`; a malformed file is an error so corruption is surfaced rather
/// than silently overwritten.
fn read_lock(lock_path: &Path) -> Result<Option<LockFile>> {
    match std::fs::read_to_string(lock_path) {
        Ok(text) => {
            let lock: LockFile = serde_json::from_str(&text)
                .with_context(|| format!("malformed lock {}", lock_path.display()))?;
            Ok(Some(lock))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("could not read lock {}", lock_path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    /// Serializes env-mutating tests so they can't clobber each other's
    /// process-global environment.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Restores [`RECIPE_LOCK_ENV`] on drop and holds the serialization lock.
    struct EnvGuard {
        _lock: MutexGuard<'static, ()>,
        prev: Option<String>,
    }

    impl EnvGuard {
        fn new() -> Self {
            let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let prev = std::env::var(RECIPE_LOCK_ENV).ok();
            std::env::remove_var(RECIPE_LOCK_ENV);
            Self { _lock: lock, prev }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => std::env::set_var(RECIPE_LOCK_ENV, v),
                None => std::env::remove_var(RECIPE_LOCK_ENV),
            }
        }
    }

    /// A minimal but valid recipe file. The body is parameterized so tests can
    /// produce a drifted variant.
    fn write_recipe(dir: &Path, instructions: &str) -> PathBuf {
        let path = dir.join("recipe.yaml");
        let body = format!(
            "version: 1.0.0\ntitle: Test Recipe\ndescription: A test recipe\ninstructions: {}\n",
            instructions
        );
        std::fs::write(&path, body).expect("write recipe");
        path
    }

    #[test]
    fn is_enabled_false_when_unset() {
        let _guard = EnvGuard::new();
        assert!(!is_enabled());
        assert_eq!(recipe_path(), None);
    }

    #[test]
    fn is_enabled_true_when_set() {
        let _guard = EnvGuard::new();
        std::env::set_var(RECIPE_LOCK_ENV, "/tmp/some/recipe.yaml");
        assert!(is_enabled());
        assert_eq!(recipe_path(), Some(PathBuf::from("/tmp/some/recipe.yaml")));
    }

    #[test]
    fn blank_env_is_treated_as_unset() {
        let _guard = EnvGuard::new();
        std::env::set_var(RECIPE_LOCK_ENV, "   ");
        assert!(!is_enabled());
        assert_eq!(recipe_path(), None);
    }

    #[test]
    fn fresh_then_matched_then_drifted() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let lock_dir = tmp.path().join(LOCK_DIR);
        let recipe = write_recipe(tmp.path(), "do the thing");

        // First lock: nothing on disk yet.
        let first = lock_recipe_in(&recipe, &lock_dir).expect("first lock");
        assert_eq!(first, LockOutcome::Fresh);
        assert!(
            lock_dir.join(LOCK_FILE).exists(),
            "sidecar should be written"
        );

        // Second lock, identical content: reproducible.
        let second = lock_recipe_in(&recipe, &lock_dir).expect("second lock");
        assert_eq!(second, LockOutcome::Matched);

        // Edit the recipe so its canonical form changes: drift.
        write_recipe(tmp.path(), "do a different thing entirely");
        let third = lock_recipe_in(&recipe, &lock_dir).expect("third lock");
        assert_eq!(third, LockOutcome::Drifted);
    }

    #[test]
    fn hash_is_stable_across_identical_inputs() {
        let tmp_a = tempfile::tempdir().expect("tempdir a");
        let tmp_b = tempfile::tempdir().expect("tempdir b");
        let recipe_a = write_recipe(tmp_a.path(), "identical instructions");
        let recipe_b = write_recipe(tmp_b.path(), "identical instructions");

        let canon_a = canonicalize(&recipe_a).expect("canon a");
        let canon_b = canonicalize(&recipe_b).expect("canon b");

        assert_eq!(hash_canonical(&canon_a), hash_canonical(&canon_b));
        // 64 hex chars for a SHA-256 digest.
        assert_eq!(hash_canonical(&canon_a).len(), 64);
    }

    #[test]
    fn outcomes_carry_no_upstream_branding() {
        // Guard against accidental branding leakage in any user-visible naming.
        for label in [
            format!("{:?}", LockOutcome::Fresh),
            format!("{:?}", LockOutcome::Matched),
            format!("{:?}", LockOutcome::Drifted),
        ] {
            let lower = label.to_lowercase();
            assert!(!lower.contains("goose"), "leaked upstream name: {label}");
            assert!(!lower.contains("block"), "leaked upstream name: {label}");
        }
    }
}
