//! Packaging-matrix verifier doctor deep-check — BharatCode v94.
//!
//! A single, read-only diagnostic that confirms the release *packaging matrix*
//! is internally consistent. The matrix is the full cartesian view of what a
//! release publishes: every target triple crossed with every distribution
//! *format* it ships in (`deb` / `rpm` / `tar.bz2` / `zip` / Homebrew bottle),
//! each mapped to the exact `bharatcode-<triple>.<ext>` asset filename the
//! publisher uploads.
//!
//! Two things are verified:
//!
//!   1. **Static shape** (always): the embedded [`PACKAGING_MATRIX`] is
//!      non-empty and every `asset_name` carries the `bharatcode` product
//!      prefix, matches the `bharatcode-<triple>.<ext>` shape the self-updater's
//!      `asset_name()` in `commands/update.rs` resolves to, and never leaks the
//!      upstream donor product name.
//!   2. **Local dist integrity** (only when a dist directory exists): each
//!      declared artifact present in the dist directory is cross-checked against
//!      a `SHA256SUMS` checksum file — its recomputed SHA-256 must match the
//!      digest recorded for it. A missing artifact or a digest mismatch is
//!      surfaced as a non-blocking warning.
//!
//! The dist directory is `./dist` by default and can be overridden with the
//! `BHARATCODE_DIST_DIR` environment variable. When the directory is absent —
//! the normal state for a development checkout — the row reports a benign `Ok`
//! that names how many targets the matrix defines, so the operator can confirm
//! the matrix is wired without any release artifacts on disk.
//!
//! This probe is deliberately conservative and side-effect free: it only ever
//! *reads* the dist directory and its checksum file (with a hard ceiling on the
//! number of artifacts hashed so a pathological directory can never blow up
//! wall-clock time), and it never writes, mutates config, shells out, or
//! contacts the network. Critically, it **never** returns a hard failure — the
//! worst outcome is [`Status::Warn`] — so it can never block a `doctor` run.
//!
//! Original BharatCode work; not ported from any third party. The SHA-256
//! hashing reuses the already-vendored `sha2` crate (the same one
//! `commands/update.rs` uses for self-update verification).
//!
//! Declared `mod packaging_check` (private) inside `doctor.rs` so the readiness
//! row can reach it without a `pub mod` in `commands/mod.rs`. A handful of
//! accessors here (`format`/`asset_name`/`all_for_triple`) are part of the
//! matrix view consumed by tests and future release tooling rather than by the
//! doctor row itself, so they are allowed to be unused in a normal build.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::commands::doctor_checks::Status;

/// The product name every release artifact is prefixed with. Must match the
/// `bharatcode-` prefix the self-updater's `asset_name()` builds.
const PRODUCT: &str = "bharatcode";

/// Environment key that overrides the dist directory the verifier scans.
/// Read-only here: the verifier only ever reads what this points at.
const DIST_DIR_ENV: &str = "BHARATCODE_DIST_DIR";

/// Default dist directory, relative to the current working directory, scanned
/// when `BHARATCODE_DIST_DIR` is not set.
const DEFAULT_DIST_DIR: &str = "dist";

/// Conventional filename of the release SHA-256 checksum file, in GNU-coreutils
/// `sha256sum` format (`<hex>  <name>` per line).
const CHECKSUMS_FILE: &str = "SHA256SUMS";

/// Hard ceiling on the number of artifacts hashed in one scan, so a dist
/// directory with a pathological number of files can never make the verifier
/// run unbounded.
const MAX_ARTIFACTS_HASHED: usize = 256;

/// A distribution format a release artifact is published in.
///
/// The matrix spans native Linux packages (`deb` / `rpm`), the cross-platform
/// archive formats the self-updater downloads (`tar.bz2` on Unix, `zip` on
/// Windows), and the Homebrew bottle the macOS tap pours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Debian package (`.deb`).
    Deb,
    /// RPM package (`.rpm`).
    Rpm,
    /// bzip2-compressed tarball (`.tar.bz2`) — the Unix self-update archive.
    TarBz2,
    /// Zip archive (`.zip`) — the Windows self-update archive.
    Zip,
    /// Homebrew bottle (`.tar.gz`) poured by the macOS tap.
    Brew,
}

impl Format {
    /// Short, stable identifier used in the rendered doctor row.
    pub fn id(self) -> &'static str {
        match self {
            Format::Deb => "deb",
            Format::Rpm => "rpm",
            Format::TarBz2 => "tar.bz2",
            Format::Zip => "zip",
            Format::Brew => "brew",
        }
    }

    /// The filename extension an artifact of this format ends with.
    pub fn extension(self) -> &'static str {
        match self {
            Format::Deb => ".deb",
            Format::Rpm => ".rpm",
            Format::TarBz2 => ".tar.bz2",
            Format::Zip => ".zip",
            Format::Brew => ".tar.gz",
        }
    }
}

/// One row of the packaging matrix: a target triple crossed with a single
/// distribution format and the exact asset filename it publishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatrixEntry {
    /// Rust target triple, e.g. `x86_64-unknown-linux-musl`.
    pub target_triple: &'static str,
    /// Distribution format this row describes.
    pub format: Format,
    /// Published asset filename, e.g.
    /// `bharatcode-x86_64-unknown-linux-musl.tar.bz2`.
    pub asset_name: &'static str,
}

/// The embedded packaging matrix: every `{target_triple, format, asset_name}`
/// row a release publishes.
///
/// The `tar.bz2` / `zip` rows are kept in lockstep with the `asset_name()`
/// match arms in `commands/update.rs` (a unit test asserts the
/// `bharatcode-<triple>.<ext>` shape), so a rename in either place is caught.
/// The `deb` / `rpm` rows describe the native Linux packages the publisher
/// emits for the Linux triples; the `brew` row describes the macOS bottle.
pub static PACKAGING_MATRIX: &[MatrixEntry] = &[
    // macOS — archive + Homebrew bottle.
    MatrixEntry {
        target_triple: "aarch64-apple-darwin",
        format: Format::TarBz2,
        asset_name: "bharatcode-aarch64-apple-darwin.tar.bz2",
    },
    MatrixEntry {
        target_triple: "aarch64-apple-darwin",
        format: Format::Brew,
        asset_name: "bharatcode-aarch64-apple-darwin.tar.gz",
    },
    MatrixEntry {
        target_triple: "x86_64-apple-darwin",
        format: Format::TarBz2,
        asset_name: "bharatcode-x86_64-apple-darwin.tar.bz2",
    },
    MatrixEntry {
        target_triple: "x86_64-apple-darwin",
        format: Format::Brew,
        asset_name: "bharatcode-x86_64-apple-darwin.tar.gz",
    },
    // Linux gnu — archive + deb + rpm.
    MatrixEntry {
        target_triple: "x86_64-unknown-linux-gnu",
        format: Format::TarBz2,
        asset_name: "bharatcode-x86_64-unknown-linux-gnu.tar.bz2",
    },
    MatrixEntry {
        target_triple: "x86_64-unknown-linux-gnu",
        format: Format::Deb,
        asset_name: "bharatcode-x86_64-unknown-linux-gnu.deb",
    },
    MatrixEntry {
        target_triple: "x86_64-unknown-linux-gnu",
        format: Format::Rpm,
        asset_name: "bharatcode-x86_64-unknown-linux-gnu.rpm",
    },
    MatrixEntry {
        target_triple: "aarch64-unknown-linux-gnu",
        format: Format::TarBz2,
        asset_name: "bharatcode-aarch64-unknown-linux-gnu.tar.bz2",
    },
    MatrixEntry {
        target_triple: "aarch64-unknown-linux-gnu",
        format: Format::Deb,
        asset_name: "bharatcode-aarch64-unknown-linux-gnu.deb",
    },
    MatrixEntry {
        target_triple: "aarch64-unknown-linux-gnu",
        format: Format::Rpm,
        asset_name: "bharatcode-aarch64-unknown-linux-gnu.rpm",
    },
    // Linux musl — archive + deb + rpm.
    MatrixEntry {
        target_triple: "x86_64-unknown-linux-musl",
        format: Format::TarBz2,
        asset_name: "bharatcode-x86_64-unknown-linux-musl.tar.bz2",
    },
    MatrixEntry {
        target_triple: "x86_64-unknown-linux-musl",
        format: Format::Deb,
        asset_name: "bharatcode-x86_64-unknown-linux-musl.deb",
    },
    MatrixEntry {
        target_triple: "x86_64-unknown-linux-musl",
        format: Format::Rpm,
        asset_name: "bharatcode-x86_64-unknown-linux-musl.rpm",
    },
    MatrixEntry {
        target_triple: "aarch64-unknown-linux-musl",
        format: Format::TarBz2,
        asset_name: "bharatcode-aarch64-unknown-linux-musl.tar.bz2",
    },
    MatrixEntry {
        target_triple: "aarch64-unknown-linux-musl",
        format: Format::Deb,
        asset_name: "bharatcode-aarch64-unknown-linux-musl.deb",
    },
    MatrixEntry {
        target_triple: "aarch64-unknown-linux-musl",
        format: Format::Rpm,
        asset_name: "bharatcode-aarch64-unknown-linux-musl.rpm",
    },
    // Windows — zip archive.
    MatrixEntry {
        target_triple: "x86_64-pc-windows-msvc",
        format: Format::Zip,
        asset_name: "bharatcode-x86_64-pc-windows-msvc.zip",
    },
];

/// The distinct target triples the matrix publishes assets for, in first-seen
/// matrix order. Used to report the target *count* in the no-dist case.
pub fn matrix_targets() -> Vec<&'static str> {
    let mut seen: Vec<&'static str> = Vec::new();
    for entry in PACKAGING_MATRIX {
        if !seen.contains(&entry.target_triple) {
            seen.push(entry.target_triple);
        }
    }
    seen
}

/// Every matrix row for a given target `triple`, in matrix order.
pub fn all_for_triple(triple: &str) -> Vec<&'static MatrixEntry> {
    PACKAGING_MATRIX
        .iter()
        .filter(|e| e.target_triple == triple)
        .collect()
}

/// Resolve the dist directory the verifier scans: the `BHARATCODE_DIST_DIR`
/// override when set and non-empty, otherwise `<cwd>/dist`.
fn dist_dir(cwd: &Path) -> PathBuf {
    match std::env::var(DIST_DIR_ENV) {
        Ok(v) if !v.trim().is_empty() => PathBuf::from(v),
        _ => cwd.join(DEFAULT_DIST_DIR),
    }
}

/// Compute the lowercase-hex SHA-256 of an on-disk file, streaming it in
/// fixed-size chunks so a large artifact never loads fully into memory.
///
/// Reused by the doctor row and exercised directly by a unit test against a
/// known fixture, so it carries its own narrow error type.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(goose::utils::bytes_to_hex(hasher.finalize()))
}

/// Parse a `SHA256SUMS` body into `(filename, lowercase-hex)` pairs. Accepts
/// both the text separator (`<hex>  <name>`) and the binary marker
/// (`<hex> *<name>`); blank and malformed lines are skipped.
fn parse_checksums(body: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some((hex, rest)) = trimmed.split_once(' ') else {
            continue;
        };
        let name = rest.trim_start_matches(|c| c == ' ' || c == '*').trim();
        if hex.is_empty() || name.is_empty() {
            continue;
        }
        out.push((name.to_string(), hex.to_ascii_lowercase()));
    }
    out
}

/// Outcome of checking the artifacts present in a dist directory against the
/// checksum file, per the matrix.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct VerifyReport {
    /// Declared matrix artifacts present in the dist dir whose recomputed
    /// SHA-256 matched the digest recorded in the checksum file.
    pub matched: Vec<String>,
    /// Declared matrix artifacts present on disk but absent from the checksum
    /// file (nothing to verify against).
    pub unlisted: Vec<String>,
    /// Declared matrix artifacts whose recomputed SHA-256 disagreed with the
    /// digest recorded in the checksum file.
    pub mismatched: Vec<String>,
    /// Whether a `SHA256SUMS` file was present in the dist directory at all.
    pub checksums_present: bool,
}

impl VerifyReport {
    /// Whether every artifact that was checked verified cleanly: a checksum file
    /// is present, nothing mismatched, and nothing on disk went unlisted.
    fn is_clean(&self) -> bool {
        self.checksums_present && self.mismatched.is_empty() && self.unlisted.is_empty()
    }
}

/// Verify the matrix artifacts present in `dir` against `dir/SHA256SUMS`.
///
/// Only filenames that appear in [`PACKAGING_MATRIX`] *and* exist on disk are
/// considered; unrelated files are ignored. For each such artifact the
/// recomputed SHA-256 is compared against the digest recorded in the checksum
/// file. The scan is bounded to [`MAX_ARTIFACTS_HASHED`] artifacts.
///
/// This is read-only and never fails the program — an unreadable file is simply
/// treated as un-matchable and reported as `unlisted`/`mismatched` rather than
/// propagated.
pub fn verify_checksums(dir: &Path) -> VerifyReport {
    let mut report = VerifyReport::default();

    let recorded: Vec<(String, String)> = match std::fs::read_to_string(dir.join(CHECKSUMS_FILE)) {
        Ok(body) => {
            report.checksums_present = true;
            parse_checksums(&body)
        }
        Err(_) => Vec::new(),
    };

    let mut hashed = 0usize;
    for entry in PACKAGING_MATRIX {
        let name = entry.asset_name;
        let path = dir.join(name);
        if !path.is_file() {
            continue;
        }
        if hashed >= MAX_ARTIFACTS_HASHED {
            break;
        }
        hashed += 1;

        let expected = recorded
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, hex)| hex.clone());
        match expected {
            None => report.unlisted.push(name.to_string()),
            Some(expected) => match sha256_file(&path) {
                Ok(actual) if actual == expected => report.matched.push(name.to_string()),
                _ => report.mismatched.push(name.to_string()),
            },
        }
    }

    report
}

/// Look up a user-facing string through the i18n layer, falling back to the
/// English `default` when the active locale table has no entry for `key`.
///
/// `tr!` echoes the key back when it is missing, so an unchanged key is treated
/// as "untranslated". Mirrors the helper in `doctor.rs` / `ci_check.rs` so the
/// row renders in English without depending on the i18n table.
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// Read-only `doctor` row: verify the release packaging matrix is internally
/// consistent.
///
/// Resolves the dist directory (`BHARATCODE_DIST_DIR`, else `./dist`) against
/// the current working directory and reports:
///
/// * [`Status::Ok`] — no dist directory present (the normal dev state); the
///   message names how many targets the matrix defines.
/// * [`Status::Ok`] — a dist directory is present and every declared artifact
///   found there verified against the checksum file.
/// * [`Status::Warn`] — a dist directory is present but one or more declared
///   artifacts are missing from / unlisted in the checksum file, the checksum
///   file itself is absent, or an artifact's digest mismatched.
///
/// It never returns [`Status::Fail`]: a packaging inconsistency is worth
/// surfacing but must never block a `doctor` run.
pub fn packaging_readiness() -> (Status, String) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let lbl = label("doctor.check.packaging_matrix", "Packaging matrix");
    let target_count = matrix_targets().len();
    let format_count = PACKAGING_MATRIX.len();

    let dir = dist_dir(&cwd);
    if !dir.is_dir() {
        let none = label(
            "doctor.check.packaging_no_dist",
            "no local dist (manifest defines",
        );
        return (
            Status::Ok,
            format!("{lbl}: {none} {target_count} targets, {format_count} artifacts)"),
        );
    }

    let report = verify_checksums(&dir);
    let present = report.matched.len() + report.unlisted.len() + report.mismatched.len();

    if !report.checksums_present {
        let no_sums = label(
            "doctor.check.packaging_no_checksums",
            "dist present but no SHA256SUMS checksum file",
        );
        return (
            Status::Warn,
            format!("{lbl}: {no_sums} ({present} matrix artifacts on disk)"),
        );
    }

    if present == 0 {
        let empty = label(
            "doctor.check.packaging_empty_dist",
            "dist present but holds no matrix artifacts",
        );
        return (
            Status::Warn,
            format!("{lbl}: {empty} (matrix defines {format_count} artifacts)"),
        );
    }

    if report.is_clean() {
        let ok = label("doctor.check.packaging_ok", "verified");
        return (
            Status::Ok,
            format!(
                "{lbl}: {ok} {}/{} matrix artifacts against {CHECKSUMS_FILE}",
                report.matched.len(),
                present
            ),
        );
    }

    // Something is off but non-blocking: report the most actionable detail.
    if let Some(name) = report.mismatched.first() {
        let mismatch = label("doctor.check.packaging_mismatch", "checksum mismatch on");
        return (
            Status::Warn,
            format!(
                "{lbl}: {mismatch} {name} ({} of {present} verified)",
                report.matched.len()
            ),
        );
    }

    let unlisted = label(
        "doctor.check.packaging_unlisted",
        "artifact missing from checksum file",
    );
    let first = report
        .unlisted
        .first()
        .map(String::as_str)
        .unwrap_or("artifact");
    (
        Status::Warn,
        format!(
            "{lbl}: {unlisted}: {first} ({} of {present} verified)",
            report.matched.len()
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// Render a `(name, hex)` pair set into GNU-coreutils `sha256sum` text form.
    fn render_sums(entries: &[(&str, &str)]) -> String {
        let mut out = String::new();
        for (name, hex) in entries {
            out.push_str(hex);
            out.push_str("  ");
            out.push_str(name);
            out.push('\n');
        }
        out
    }

    /// Run a closure with `BHARATCODE_DIST_DIR` pointed at `dir`, restoring the
    /// previous value afterwards. The doctor env probes are process-global, so
    /// the few tests that touch the env are kept self-contained here.
    fn with_dist_dir<T>(dir: &Path, f: impl FnOnce() -> T) -> T {
        let prev = std::env::var(DIST_DIR_ENV).ok();
        std::env::set_var(DIST_DIR_ENV, dir);
        let out = f();
        match prev {
            Some(v) => std::env::set_var(DIST_DIR_ENV, v),
            None => std::env::remove_var(DIST_DIR_ENV),
        }
        out
    }

    #[test]
    fn matrix_is_non_empty() {
        assert!(!PACKAGING_MATRIX.is_empty());
        assert!(matrix_targets().len() >= 5);
    }

    /// Every asset name carries the product prefix, matches the
    /// `bharatcode-<triple>.<ext>` shape the self-updater's `asset_name()` uses,
    /// and never leaks the upstream donor product name.
    #[test]
    fn every_asset_name_is_well_formed_and_clean() {
        for entry in PACKAGING_MATRIX {
            let name = entry.asset_name;
            assert!(
                name.contains(PRODUCT),
                "asset name must contain product prefix: {name}"
            );
            // bharatcode-<triple>.<ext> shape.
            let stem = name
                .strip_prefix(&format!("{PRODUCT}-"))
                .unwrap_or_else(|| panic!("asset name must start with '{PRODUCT}-': {name}"));
            assert!(
                stem.starts_with(entry.target_triple),
                "asset stem must begin with the triple: {name}"
            );
            assert!(
                name.ends_with(entry.format.extension()),
                "asset name must end with the format extension: {name}"
            );
            // Zero upstream-donor leakage in any published filename.
            assert!(
                !name.contains("goose"),
                "asset name must not leak donor product name: {name}"
            );
        }
    }

    /// The archive (`tar.bz2` / `zip`) rows must reproduce the exact
    /// `bharatcode-<triple>.<ext>` filenames the self-updater downloads.
    #[test]
    fn archive_rows_match_updater_shape() {
        let musl = all_for_triple("x86_64-unknown-linux-musl");
        assert!(musl.iter().any(|e| e.format == Format::TarBz2
            && e.asset_name == "bharatcode-x86_64-unknown-linux-musl.tar.bz2"));

        let win = all_for_triple("x86_64-pc-windows-msvc");
        assert!(win
            .iter()
            .any(|e| e.format == Format::Zip
                && e.asset_name == "bharatcode-x86_64-pc-windows-msvc.zip"));
    }

    /// With no dist directory the row is a benign Ok naming the target count.
    #[test]
    fn readiness_without_dist_is_ok_and_names_target_count() {
        let dir = tempdir().unwrap();
        // Point the override at a path that does not exist.
        let missing = dir.path().join("nonexistent-dist");
        let (status, msg) = with_dist_dir(&missing, packaging_readiness);
        assert_eq!(status, Status::Ok, "msg: {msg}");
        let count = matrix_targets().len().to_string();
        assert!(
            msg.contains(&count),
            "message must name the target count {count}: {msg}"
        );
        assert!(msg.contains("no local dist"), "msg: {msg}");
    }

    /// A dist dir holding one declared artifact plus a correct checksum line
    /// verifies clean (Ok); tampering the recorded digest downgrades to Warn —
    /// never Fail.
    #[test]
    fn readiness_with_artifact_ok_then_tampered_warns() {
        let entry = &PACKAGING_MATRIX[0];
        let payload = b"bharatcode release artifact payload";

        // Correct checksum -> Ok.
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(entry.asset_name), payload).unwrap();
        let good_hex = sha256_file(&dir.path().join(entry.asset_name)).unwrap();
        fs::write(
            dir.path().join(CHECKSUMS_FILE),
            render_sums(&[(entry.asset_name, &good_hex)]),
        )
        .unwrap();
        let (status, msg) = with_dist_dir(dir.path(), packaging_readiness);
        assert_eq!(status, Status::Ok, "clean dist must be Ok: {msg}");

        // Tampered checksum -> Warn (and never Fail).
        let bad = tempdir().unwrap();
        fs::write(bad.path().join(entry.asset_name), payload).unwrap();
        let tampered = "0".repeat(64);
        fs::write(
            bad.path().join(CHECKSUMS_FILE),
            render_sums(&[(entry.asset_name, &tampered)]),
        )
        .unwrap();
        let (status, msg) = with_dist_dir(bad.path(), packaging_readiness);
        assert_eq!(status, Status::Warn, "tampered checksum must Warn: {msg}");
        assert_ne!(status, Status::Fail, "must never Fail: {msg}");
    }

    /// A dist dir with an artifact but no checksum file warns, never fails.
    #[test]
    fn readiness_with_artifact_but_no_checksums_warns() {
        let entry = &PACKAGING_MATRIX[0];
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(entry.asset_name), b"x").unwrap();
        let (status, msg) = with_dist_dir(dir.path(), packaging_readiness);
        assert_eq!(status, Status::Warn, "msg: {msg}");
        assert_ne!(status, Status::Fail, "must never Fail: {msg}");
    }

    /// `sha256_file` computes the standard SHA-256 of a known fixture.
    #[test]
    fn sha256_file_matches_known_vector() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("abc.bin");
        fs::write(&path, b"abc").unwrap();
        // SHA-256("abc") — canonical NIST test vector.
        let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(sha256_file(&path).unwrap(), expected);
    }

    /// `verify_checksums` matches a declared artifact and flags a tampered one.
    #[test]
    fn verify_checksums_matches_and_flags() {
        let entry = &PACKAGING_MATRIX[0];
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(entry.asset_name), b"payload").unwrap();
        let hex = sha256_file(&dir.path().join(entry.asset_name)).unwrap();

        // Correct entry -> matched.
        fs::write(
            dir.path().join(CHECKSUMS_FILE),
            render_sums(&[(entry.asset_name, &hex)]),
        )
        .unwrap();
        let report = verify_checksums(dir.path());
        assert!(report.checksums_present);
        assert_eq!(report.matched, vec![entry.asset_name.to_string()]);
        assert!(report.mismatched.is_empty());
        assert!(report.is_clean());

        // Tampered entry -> mismatched.
        fs::write(
            dir.path().join(CHECKSUMS_FILE),
            render_sums(&[(entry.asset_name, &"f".repeat(64))]),
        )
        .unwrap();
        let report = verify_checksums(dir.path());
        assert_eq!(report.mismatched, vec![entry.asset_name.to_string()]);
        assert!(!report.is_clean());
    }
}
