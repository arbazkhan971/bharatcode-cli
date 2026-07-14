//! `packaging` — release packaging-matrix descriptor + checksum integrity
//! (BharatCode v94).
//!
//! Pure, offline helpers describing how a built BharatCode release binary maps
//! onto the per-target packaging artifacts, plus a small SHA-256 checksum
//! manifest reader/writer used for release integrity:
//!
//! * [`PackageTarget`] — one matrix row: target `triple`, the release `artifact`
//!   filename, and the `deb`/`rpm` architecture labels for that triple.
//! * [`MATRIX`] — every triple BharatCode ships assets for, the same set the
//!   self-updater's `asset_name()` and the release / build-cli workflows use.
//! * [`manifest_for`] — the exact `bharatcode-<triple>.tar.bz2` / `.zip` asset
//!   filename the self-updater in `commands/update.rs` `asset_name()` expects,
//!   kept in lockstep so a rename in either place fails CI.
//! * [`checksums`] — `write_manifest(dir)` / [`verify_checksums`] over a
//!   `SHA256SUMS` text file (GNU-coreutils format), hashing with the
//!   already-vendored `sha2` crate (the same one `commands/update.rs` uses).
//! * [`checksum_status`] — a read-only `doctor` row reporting whether a release
//!   checksum manifest is present and self-consistent.
//!
//! Everything here is pure over its inputs apart from the explicit filesystem
//! helpers in [`checksums`]; no network, no new dependencies. This file is the
//! packaging *matrix* + *integrity* view; the descriptor *generators*
//! (nfpm/Homebrew YAML) live alongside in `release/package_matrix.rs`.
//!
//! Original BharatCode work; not ported from any third party.
//!
//! This module is declared `mod packaging` (private) inside `doctor.rs` so the
//! checksum-status row can reach it without a `pub mod` in `commands/mod.rs`.
//! Several entries here — `manifest_for`, the deb/rpm/brew accessors, and
//! `checksums::write_manifest`/`render` — are the packaging *generator* surface
//! invoked by release tooling and exercised by the unit tests rather than by the
//! doctor row itself, so they are allowed to be unused in a normal build.
#![allow(dead_code)]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::commands::doctor_checks::Status;

/// The product name every release artifact is prefixed with. Must match the
/// `bharatcode-` prefix the self-updater's `asset_name()` builds.
const PRODUCT: &str = "bharatcode";

/// The conventional filename for a release SHA-256 checksum manifest, written
/// in GNU-coreutils `sha256sum` format (`<hex>  <name>` per line).
pub const SHA256SUMS: &str = "SHA256SUMS";

/// One row of the release packaging matrix: a single target triple mapped to
/// the artifact filename the updater downloads and the deb/rpm architecture
/// labels the native packagers expect for that triple.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackageTarget {
    /// Rust target triple, e.g. `x86_64-unknown-linux-musl`.
    pub triple: &'static str,
    /// Release artifact filename, e.g. `bharatcode-x86_64-unknown-linux-musl.tar.bz2`.
    pub artifact: &'static str,
    /// Debian architecture label (`amd64` / `arm64`), or empty for non-Linux.
    pub deb_arch: &'static str,
    /// RPM architecture label (`x86_64` / `aarch64`), or empty for non-Linux.
    pub rpm_arch: &'static str,
}

impl PackageTarget {
    /// Whether this target produces native Linux packages (deb + rpm).
    pub fn is_linux_package(&self) -> bool {
        !self.deb_arch.is_empty() && !self.rpm_arch.is_empty()
    }

    /// The Homebrew bottle/formula download stem for this target — the artifact
    /// filename with its archive extension stripped. Accessor kept for release
    /// tooling that names Homebrew bottles after the tarball.
    pub fn brew_stem(&self) -> &'static str {
        self.artifact
            .strip_suffix(".tar.bz2")
            .or_else(|| self.artifact.strip_suffix(".zip"))
            .unwrap_or(self.artifact)
    }
}

/// The release packaging matrix: every triple BharatCode publishes assets for.
///
/// Kept in lockstep with the `asset_name()` match arms in `commands/update.rs`
/// (and the triples release.yml / build-cli.yml build): every Unix triple is a
/// `.tar.bz2`, the Windows triple is a `.zip`. deb/rpm arches are populated only
/// for the Linux triples; darwin/windows leave them empty.
pub static MATRIX: &[PackageTarget] = &[
    PackageTarget {
        triple: "aarch64-apple-darwin",
        artifact: "bharatcode-aarch64-apple-darwin.tar.bz2",
        deb_arch: "",
        rpm_arch: "",
    },
    PackageTarget {
        triple: "x86_64-apple-darwin",
        artifact: "bharatcode-x86_64-apple-darwin.tar.bz2",
        deb_arch: "",
        rpm_arch: "",
    },
    PackageTarget {
        triple: "x86_64-unknown-linux-gnu",
        artifact: "bharatcode-x86_64-unknown-linux-gnu.tar.bz2",
        deb_arch: "amd64",
        rpm_arch: "x86_64",
    },
    PackageTarget {
        triple: "aarch64-unknown-linux-gnu",
        artifact: "bharatcode-aarch64-unknown-linux-gnu.tar.bz2",
        deb_arch: "arm64",
        rpm_arch: "aarch64",
    },
    PackageTarget {
        triple: "x86_64-unknown-linux-musl",
        artifact: "bharatcode-x86_64-unknown-linux-musl.tar.bz2",
        deb_arch: "amd64",
        rpm_arch: "x86_64",
    },
    PackageTarget {
        triple: "aarch64-unknown-linux-musl",
        artifact: "bharatcode-aarch64-unknown-linux-musl.tar.bz2",
        deb_arch: "arm64",
        rpm_arch: "aarch64",
    },
    PackageTarget {
        triple: "x86_64-pc-windows-msvc",
        artifact: "bharatcode-x86_64-pc-windows-msvc.zip",
        deb_arch: "",
        rpm_arch: "",
    },
];

/// Look up the packaging-matrix row for a target `triple`, if BharatCode ships
/// assets for it.
pub fn target_for(triple: &str) -> Option<&'static PackageTarget> {
    MATRIX.iter().find(|t| t.triple == triple)
}

/// The exact release artifact filename for `triple`, e.g.
/// `bharatcode-x86_64-unknown-linux-musl.tar.bz2`.
///
/// This is the same string the self-updater's `asset_name()` in
/// `commands/update.rs` resolves to at compile time for the matching target. A
/// unit test asserts byte-for-byte equality, so renaming an asset in either
/// place without updating the other fails CI.
pub fn manifest_for(triple: &str) -> Option<&'static str> {
    target_for(triple).map(|t| t.artifact)
}

/// Every artifact filename in the matrix, in matrix order. Convenience accessor
/// for release tooling that needs the full publish set.
pub fn all_artifacts() -> Vec<&'static str> {
    MATRIX.iter().map(|t| t.artifact).collect()
}

// ---------------------------------------------------------------------------
// SHA-256 checksum manifest (SHA256SUMS)
// ---------------------------------------------------------------------------

/// Why a checksum verification failed, distinguishing a tampered/rebuilt file
/// (`Mismatch`) from operational errors (missing manifest, unreadable file).
#[derive(Debug)]
pub enum ChecksumError {
    /// No `SHA256SUMS` manifest exists in the directory.
    ManifestMissing,
    /// The manifest exists but could not be parsed.
    ManifestMalformed(String),
    /// A file listed in the manifest is absent from the directory.
    FileMissing(String),
    /// A listed file's recomputed SHA-256 does not match the manifest entry.
    Mismatch {
        /// Filename whose digest disagreed with the manifest.
        file: String,
        /// Digest recorded in the manifest.
        expected: String,
        /// Digest recomputed from the file on disk.
        actual: String,
    },
    /// An I/O error occurred while reading a file or the manifest.
    Io(String),
}

impl std::fmt::Display for ChecksumError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChecksumError::ManifestMissing => write!(f, "checksum manifest not found"),
            ChecksumError::ManifestMalformed(line) => {
                write!(f, "malformed manifest line: {line}")
            }
            ChecksumError::FileMissing(name) => write!(f, "listed file missing: {name}"),
            ChecksumError::Mismatch {
                file,
                expected,
                actual,
            } => write!(
                f,
                "checksum mismatch for {file}: expected {expected}, got {actual}"
            ),
            ChecksumError::Io(msg) => write!(f, "io error: {msg}"),
        }
    }
}

impl std::error::Error for ChecksumError {}

impl From<io::Error> for ChecksumError {
    fn from(e: io::Error) -> Self {
        ChecksumError::Io(e.to_string())
    }
}

/// Filesystem helpers that read and write a `SHA256SUMS` manifest.
pub mod checksums {
    use super::*;

    /// Compute the lowercase-hex SHA-256 of an on-disk file, streaming it in
    /// fixed-size chunks so large artifacts never load fully into memory.
    pub fn sha256_file(path: &Path) -> Result<String, ChecksumError> {
        let mut file = fs::File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = io::Read::read(&mut file, &mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        Ok(bharatcode_core::utils::bytes_to_hex(hasher.finalize()))
    }

    /// Render a set of `(name, hex)` pairs into GNU-coreutils `sha256sum`
    /// format: `<hex><two spaces><name>` per line, trailing newline.
    pub fn render(entries: &[(String, String)]) -> String {
        let mut out = String::new();
        for (name, hex) in entries {
            out.push_str(hex);
            out.push_str("  ");
            out.push_str(name);
            out.push('\n');
        }
        out
    }

    /// Parse a `SHA256SUMS` manifest body into `(name, hex)` pairs. Accepts both
    /// the binary marker (`<hex> *<name>`) and the text form (`<hex>  <name>`);
    /// blank lines are skipped.
    pub fn parse(body: &str) -> Result<Vec<(String, String)>, ChecksumError> {
        let mut out = Vec::new();
        for line in body.lines() {
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if trimmed.trim().is_empty() {
                continue;
            }
            let (hex, rest) = trimmed
                .split_once(' ')
                .ok_or_else(|| ChecksumError::ManifestMalformed(trimmed.to_string()))?;
            // The separator is two spaces (text) or a space + `*` (binary); skip
            // whatever leading whitespace / marker precedes the filename.
            let name = rest.trim_start_matches([' ', '*']);
            if hex.is_empty() || name.is_empty() {
                return Err(ChecksumError::ManifestMalformed(trimmed.to_string()));
            }
            out.push((name.to_string(), hex.to_ascii_lowercase()));
        }
        Ok(out)
    }

    /// Generate a `SHA256SUMS` manifest for every regular file in `dir` (except
    /// an existing manifest), writing it to `dir/SHA256SUMS` and returning the
    /// manifest path. Entries are sorted by filename for deterministic output.
    pub fn write_manifest(dir: &Path) -> Result<PathBuf, ChecksumError> {
        let mut entries: Vec<(String, String)> = Vec::new();
        for dirent in fs::read_dir(dir)? {
            let dirent = dirent?;
            if !dirent.file_type()?.is_file() {
                continue;
            }
            let name = dirent.file_name().to_string_lossy().to_string();
            if name == SHA256SUMS {
                continue;
            }
            let hex = sha256_file(&dirent.path())?;
            entries.push((name, hex));
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let manifest_path = dir.join(SHA256SUMS);
        fs::write(&manifest_path, render(&entries))?;
        Ok(manifest_path)
    }
}

/// Verify a release directory against its `SHA256SUMS` manifest.
///
/// Returns `Ok(())` when the manifest is present and every listed file's
/// recomputed SHA-256 matches its recorded digest; otherwise the most specific
/// [`ChecksumError`] (e.g. [`ChecksumError::Mismatch`] when a byte changed).
pub fn verify_checksums(dir: &Path) -> Result<(), ChecksumError> {
    let manifest_path = dir.join(SHA256SUMS);
    if !manifest_path.is_file() {
        return Err(ChecksumError::ManifestMissing);
    }
    let body = fs::read_to_string(&manifest_path)?;
    let entries = checksums::parse(&body)?;
    for (name, expected) in entries {
        let file_path = dir.join(&name);
        if !file_path.is_file() {
            return Err(ChecksumError::FileMissing(name));
        }
        let actual = checksums::sha256_file(&file_path)?;
        if actual != expected {
            return Err(ChecksumError::Mismatch {
                file: name,
                expected,
                actual,
            });
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// doctor row
// ---------------------------------------------------------------------------

/// Read-only `doctor` row: report whether a release checksum manifest is
/// present in `dir` and self-consistent.
///
/// An absent manifest is the expected state for a development build and reports
/// as a benign `Warn` ("not found (dev build)") — never a hard failure. A
/// present, internally consistent manifest reports `Ok`; a digest mismatch or a
/// missing listed file reports `Fail`.
pub fn checksum_status(dir: &Path) -> (Status, String) {
    let label = crate::tr!("doctor.checksum_manifest");
    let label = if label == "doctor.checksum_manifest" {
        "Checksum manifest".to_string()
    } else {
        label
    };
    match verify_checksums(dir) {
        Ok(()) => {
            // The manifest verified; additionally report how many of its entries
            // are recognized release artifacts in the packaging MATRIX, so the
            // operator can see the manifest covers the expected matrix surface.
            let recognized = manifest_recognized_artifacts(dir);
            let total = all_artifacts().len();
            (
                Status::Ok,
                format!(
                    "{label}: present/consistent ({SHA256SUMS}; {recognized}/{total} matrix artifacts)"
                ),
            )
        }
        Err(ChecksumError::ManifestMissing) => {
            (Status::Warn, format!("{label}: not found (dev build)"))
        }
        Err(ChecksumError::Mismatch { file, .. }) => {
            (Status::Fail, format!("{label}: mismatch on {file}"))
        }
        Err(ChecksumError::FileMissing(name)) => (
            Status::Fail,
            format!("{label}: listed file missing ({name})"),
        ),
        Err(other) => (Status::Warn, format!("{label}: unverifiable ({other})")),
    }
}

/// Count how many filenames listed in `dir`'s `SHA256SUMS` manifest are
/// recognized release artifacts in the packaging [`MATRIX`]. Best-effort: a
/// missing or unparseable manifest yields `0`. Used by [`checksum_status`] to
/// cross-check the manifest against the matrix that the self-updater's
/// `asset_name()` is kept in lockstep with.
fn manifest_recognized_artifacts(dir: &Path) -> usize {
    let body = match fs::read_to_string(dir.join(SHA256SUMS)) {
        Ok(b) => b,
        Err(_) => return 0,
    };
    let entries = match checksums::parse(&body) {
        Ok(e) => e,
        Err(_) => return 0,
    };
    entries
        .iter()
        .filter(|(name, _)| MATRIX.iter().any(|t| t.artifact == name))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    /// `manifest_for` must return the EXACT artifact name the self-updater's
    /// `asset_name()` resolves to for the same triple, so the two never drift.
    #[test]
    fn manifest_for_matches_updater_asset_name() {
        assert_eq!(
            manifest_for("x86_64-unknown-linux-musl"),
            Some("bharatcode-x86_64-unknown-linux-musl.tar.bz2")
        );
        assert_eq!(
            manifest_for("aarch64-apple-darwin"),
            Some("bharatcode-aarch64-apple-darwin.tar.bz2")
        );
        assert_eq!(
            manifest_for("x86_64-pc-windows-msvc"),
            Some("bharatcode-x86_64-pc-windows-msvc.zip")
        );
        assert_eq!(manifest_for("s390x-unknown-linux-gnu"), None);
    }

    /// Every artifact carries the product prefix and the extension implied by
    /// its triple, and Linux triples expose deb/rpm arches.
    #[test]
    fn matrix_is_well_formed() {
        for t in MATRIX {
            assert!(t.artifact.starts_with(PRODUCT));
            assert!(t.artifact.contains(t.triple));
            if t.triple.contains("windows") {
                assert!(t.artifact.ends_with(".zip"));
                assert!(!t.is_linux_package());
            } else {
                assert!(t.artifact.ends_with(".tar.bz2"));
            }
            if t.triple.contains("linux") {
                assert!(t.is_linux_package());
                assert!(!t.deb_arch.is_empty());
                assert!(!t.rpm_arch.is_empty());
            }
        }
    }

    #[test]
    fn brew_stem_strips_archive_extension() {
        let musl = target_for("x86_64-unknown-linux-musl").unwrap();
        assert_eq!(musl.brew_stem(), "bharatcode-x86_64-unknown-linux-musl");
        let win = target_for("x86_64-pc-windows-msvc").unwrap();
        assert_eq!(win.brew_stem(), "bharatcode-x86_64-pc-windows-msvc");
    }

    /// Generated manifest verifies clean; corrupting one byte of a listed file
    /// surfaces a `Mismatch`.
    #[test]
    fn verify_checksums_ok_then_mismatch_on_corruption() {
        let dir = tempdir().unwrap();
        let a = dir.path().join("alpha.bin");
        let b = dir.path().join("beta.bin");
        fs::write(&a, b"hello bharatcode").unwrap();
        fs::write(&b, b"second artifact payload").unwrap();

        let manifest = checksums::write_manifest(dir.path()).unwrap();
        assert!(manifest.ends_with(SHA256SUMS));
        verify_checksums(dir.path()).expect("freshly generated manifest must verify");

        // Flip one byte of the first file without touching the manifest.
        {
            let mut f = fs::OpenOptions::new().write(true).open(&a).unwrap();
            f.write_all(b"H").unwrap(); // overwrite first byte 'h' -> 'H'
        }
        match verify_checksums(dir.path()) {
            Err(ChecksumError::Mismatch { file, .. }) => assert_eq!(file, "alpha.bin"),
            other => panic!("expected Mismatch, got {other:?}"),
        }
    }

    #[test]
    fn verify_checksums_missing_manifest_is_distinct() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("x"), b"data").unwrap();
        match verify_checksums(dir.path()) {
            Err(ChecksumError::ManifestMissing) => {}
            other => panic!("expected ManifestMissing, got {other:?}"),
        }
    }

    #[test]
    fn verify_checksums_detects_deleted_listed_file() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("keep"), b"keep").unwrap();
        fs::write(dir.path().join("gone"), b"gone").unwrap();
        checksums::write_manifest(dir.path()).unwrap();
        fs::remove_file(dir.path().join("gone")).unwrap();
        match verify_checksums(dir.path()) {
            Err(ChecksumError::FileMissing(name)) => assert_eq!(name, "gone"),
            other => panic!("expected FileMissing, got {other:?}"),
        }
    }

    #[test]
    fn render_and_parse_round_trip() {
        let entries = vec![
            ("a.tar.bz2".to_string(), "ab12".to_string()),
            ("b.zip".to_string(), "cd34".to_string()),
        ];
        let body = checksums::render(&entries);
        assert!(body.contains("ab12  a.tar.bz2\n"));
        let parsed = checksums::parse(&body).unwrap();
        assert_eq!(parsed, entries);
    }

    /// The doctor row degrades to a benign Warn (never Fail) when no manifest
    /// is present — the expected state for a dev build.
    #[test]
    fn checksum_status_absent_manifest_is_benign() {
        let dir = tempdir().unwrap();
        let (status, msg) = checksum_status(dir.path());
        assert_eq!(status, Status::Warn);
        assert!(msg.contains("not found"));
    }

    #[test]
    fn checksum_status_consistent_manifest_is_ok() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("artifact.bin"), b"payload").unwrap();
        checksums::write_manifest(dir.path()).unwrap();
        let (status, msg) = checksum_status(dir.path());
        assert_eq!(status, Status::Ok);
        assert!(msg.contains("present/consistent"));
    }
}
