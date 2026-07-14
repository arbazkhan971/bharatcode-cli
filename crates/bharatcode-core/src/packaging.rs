//! Single source of truth for the release packaging matrix.
//!
//! The v94 packaging feature centralises the artifact names release/doctor
//! tooling previously had to re-derive by hand:
//!
//! * the **packaging matrix** - for each supported target triple, the exact
//!   artifact filenames the build emits (`tar.bz2`, `zip`, `deb`, `rpm`), all
//!   derived from the v5 `bharatcode-<triple>` naming the self-updater already
//!   expects.
//!
//! The matrix is a `static` table so it is a compile-time constant. Test-only
//! helpers validate Homebrew rendering and checksum-manifest classification
//! without adding unused release-tooling APIs to the runtime binary.
//!
//! Matrix summary lines are reached from `Config::packaging_summary` (wired in
//! `config/base.rs`).

#[cfg(test)]
use std::path::{Path, PathBuf};

#[cfg(test)]
use sha2::{Digest, Sha256};

#[cfg(test)]
use crate::utils::bytes_to_hex;

/// Default `dist/` directory the verifier targets when `BHARATCODE_DIST_DIR` is
/// unset. Relative to the current working directory, matching how release
/// tooling invokes the build.
#[cfg(test)]
const DEFAULT_DIST_DIR: &str = "./dist";

/// Name of the checksum manifest the verifier reads inside `dist/`.
#[cfg(test)]
const SHA256SUMS_NAME: &str = "SHA256SUMS";

/// One row of the release packaging matrix: a target triple and the four
/// artifact filenames the build emits for it.
///
/// Every name is derived from the v5 `bharatcode-<triple>` base the
/// self-updater already expects, so the updater, the brew formula, and this
/// verifier all agree on a single naming scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackageTarget {
    /// Rust target triple, e.g. `x86_64-unknown-linux-gnu`.
    pub triple: &'static str,
    /// `tar.bz2` artifact filename.
    pub tar_name: &'static str,
    /// `zip` artifact filename.
    pub zip_name: &'static str,
    /// Debian `.deb` package filename.
    pub deb_name: &'static str,
    /// RPM `.rpm` package filename.
    pub rpm_name: &'static str,
}

/// The release packaging matrix. The triples mirror the slices the self-updater
/// and CI release job build; every filename keeps the `bharatcode-` prefix so a
/// brand-leak assertion can scan these names directly.
pub static MATRIX: &[PackageTarget] = &[
    PackageTarget {
        triple: "x86_64-unknown-linux-gnu",
        tar_name: "bharatcode-x86_64-unknown-linux-gnu.tar.bz2",
        zip_name: "bharatcode-x86_64-unknown-linux-gnu.zip",
        deb_name: "bharatcode-x86_64-unknown-linux-gnu.deb",
        rpm_name: "bharatcode-x86_64-unknown-linux-gnu.rpm",
    },
    PackageTarget {
        triple: "aarch64-unknown-linux-gnu",
        tar_name: "bharatcode-aarch64-unknown-linux-gnu.tar.bz2",
        zip_name: "bharatcode-aarch64-unknown-linux-gnu.zip",
        deb_name: "bharatcode-aarch64-unknown-linux-gnu.deb",
        rpm_name: "bharatcode-aarch64-unknown-linux-gnu.rpm",
    },
    PackageTarget {
        triple: "x86_64-apple-darwin",
        tar_name: "bharatcode-x86_64-apple-darwin.tar.bz2",
        zip_name: "bharatcode-x86_64-apple-darwin.zip",
        deb_name: "bharatcode-x86_64-apple-darwin.deb",
        rpm_name: "bharatcode-x86_64-apple-darwin.rpm",
    },
    PackageTarget {
        triple: "aarch64-apple-darwin",
        tar_name: "bharatcode-aarch64-apple-darwin.tar.bz2",
        zip_name: "bharatcode-aarch64-apple-darwin.zip",
        deb_name: "bharatcode-aarch64-apple-darwin.deb",
        rpm_name: "bharatcode-aarch64-apple-darwin.rpm",
    },
    PackageTarget {
        triple: "x86_64-pc-windows-msvc",
        tar_name: "bharatcode-x86_64-pc-windows-msvc.tar.bz2",
        zip_name: "bharatcode-x86_64-pc-windows-msvc.zip",
        deb_name: "bharatcode-x86_64-pc-windows-msvc.deb",
        rpm_name: "bharatcode-x86_64-pc-windows-msvc.rpm",
    },
];

/// The two macOS triples the Homebrew formula ships, used by
/// [`brew_formula_stanza`] to label the Intel / Apple-Silicon download blocks.
#[cfg(test)]
const BREW_X86_TRIPLE: &str = "x86_64-apple-darwin";
#[cfg(test)]
const BREW_ARM_TRIPLE: &str = "aarch64-apple-darwin";

/// Look up the matrix row for a triple, if present.
#[cfg(test)]
fn target_for(triple: &str) -> Option<&'static PackageTarget> {
    MATRIX.iter().find(|t| t.triple == triple)
}

/// All artifact filenames for a triple, in `tar / zip / deb / rpm` order.
///
/// Returns an empty vec for an unknown triple so a caller iterating user-supplied
/// triples never panics.
#[cfg(test)]
fn artifact_names(triple: &str) -> Vec<String> {
    match target_for(triple) {
        Some(t) => vec![
            t.tar_name.to_string(),
            t.zip_name.to_string(),
            t.deb_name.to_string(),
            t.rpm_name.to_string(),
        ],
        None => Vec::new(),
    }
}

/// Render a Homebrew formula `on_macos` stanza for a release, embedding the
/// version and the per-arch SHA-256 of the `tar.bz2` artifacts.
///
/// The Intel block uses the `x86_64-apple-darwin` artifact and `sha_x86`; the
/// Apple-Silicon block uses `aarch64-apple-darwin` and `sha_arm`.
#[cfg(test)]
fn brew_formula_stanza(version: &str, sha_x86: &str, sha_arm: &str) -> String {
    let x86 = target_for(BREW_X86_TRIPLE).expect("x86_64-apple-darwin is in MATRIX");
    let arm = target_for(BREW_ARM_TRIPLE).expect("aarch64-apple-darwin is in MATRIX");
    let base = "https://github.com/lineupx/bharatcode/releases/download";
    let x86_tar = x86.tar_name;
    let arm_tar = arm.tar_name;
    [
        format!("  version \"{version}\""),
        String::new(),
        "  on_macos do".to_string(),
        "    on_intel do".to_string(),
        format!("      url \"{base}/v{version}/{x86_tar}\""),
        format!("      sha256 \"{sha_x86}\""),
        "    end".to_string(),
        "    on_arm do".to_string(),
        format!("      url \"{base}/v{version}/{arm_tar}\""),
        format!("      sha256 \"{sha_arm}\""),
        "    end".to_string(),
        "  end".to_string(),
        String::new(),
    ]
    .join("\n")
}

/// Human-readable rows describing the packaging matrix: one header line plus one
/// row per triple listing its four artifact filenames. The single typed source
/// `Config::packaging_summary` delegates to for doctor / release tooling.
pub fn matrix_summary_lines() -> Vec<String> {
    let mut lines = Vec::with_capacity(MATRIX.len() + 1);
    lines.push(format!("packaging matrix = {} targets", MATRIX.len()));
    for t in MATRIX {
        lines.push(format!(
            "{} -> {}, {}, {}, {}",
            t.triple, t.tar_name, t.zip_name, t.deb_name, t.rpm_name
        ));
    }
    lines
}

/// Verification outcome for a single artifact listed in `SHA256SUMS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(test)]
pub enum ArtifactStatus {
    /// Artifact is present and its recomputed SHA-256 matches the manifest.
    Ok,
    /// Artifact is listed in the manifest but not present in `dist/`.
    Missing,
    /// Artifact is present but its recomputed SHA-256 differs from the manifest.
    Mismatch,
}

/// Per-artifact verification result.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(test)]
pub struct ArtifactResult {
    /// Artifact filename as listed in the manifest.
    pub name: String,
    /// Classification of this artifact.
    pub status: ArtifactStatus,
}

/// Aggregate report from a `dist/` verification.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg(test)]
pub struct VerifyReport {
    /// One result per manifest entry, in manifest order.
    pub results: Vec<ArtifactResult>,
    /// True when the manifest itself could not be read (no `SHA256SUMS`).
    pub manifest_missing: bool,
}

#[cfg(test)]
impl VerifyReport {
    /// Count of artifacts with a given status.
    pub fn count(&self, status: ArtifactStatus) -> usize {
        self.results.iter().filter(|r| r.status == status).count()
    }

    /// True when the manifest was read and every listed artifact verified `Ok`.
    pub fn all_ok(&self) -> bool {
        !self.manifest_missing
            && !self.results.is_empty()
            && self.results.iter().all(|r| r.status == ArtifactStatus::Ok)
    }

    /// Human-readable `name: STATUS` rows, one per result, for doctor output.
    pub fn summary_lines(&self) -> Vec<String> {
        if self.manifest_missing {
            return vec![format!("{SHA256SUMS_NAME}: missing")];
        }
        self.results
            .iter()
            .map(|r| {
                let status = match r.status {
                    ArtifactStatus::Ok => "OK",
                    ArtifactStatus::Missing => "MISSING",
                    ArtifactStatus::Mismatch => "MISMATCH",
                };
                format!("{}: {status}", r.name)
            })
            .collect()
    }
}

/// Pure classifier over captured directory contents.
///
/// `expected` is the parsed `SHA256SUMS` manifest as `(name, lowercase-hex)`
/// pairs; `present` is the set of files actually found, as `(name, bytes)`. For
/// each expected entry this recomputes SHA-256 over the matching present bytes
/// and classifies the artifact. No file I/O, so tests can drive it with
/// fixtures.
#[cfg(test)]
fn verify_listing(
    expected: &[(String, String)],
    present: &[(String, Vec<u8>)],
) -> Vec<ArtifactResult> {
    expected
        .iter()
        .map(|(name, want_sha)| {
            let status = match present.iter().find(|(n, _)| n == name) {
                None => ArtifactStatus::Missing,
                Some((_, bytes)) => {
                    if sha256_hex(bytes) == want_sha.trim().to_ascii_lowercase() {
                        ArtifactStatus::Ok
                    } else {
                        ArtifactStatus::Mismatch
                    }
                }
            };
            ArtifactResult {
                name: name.clone(),
                status,
            }
        })
        .collect()
}

/// Recompute the lowercase-hex SHA-256 of a byte slice via the crate's
/// `bytes_to_hex` helper, matching the manifest's digest format.
#[cfg(test)]
fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    bytes_to_hex(hasher.finalize())
}

/// Parse a `SHA256SUMS` manifest into `(name, hex)` pairs.
///
/// Accepts the standard `coreutils` layout: `<hex>  <name>` (two spaces, or a
/// single space plus `*` binary marker). Blank lines and lines without a split
/// are skipped. The name is taken verbatim after the separator, with a leading
/// `*` binary marker and surrounding whitespace stripped.
#[cfg(test)]
fn parse_sha256sums(contents: &str) -> Vec<(String, String)> {
    contents
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let (hex, rest) = line.split_once(char::is_whitespace)?;
            let name = rest.trim_start().trim_start_matches('*').trim();
            if hex.is_empty() || name.is_empty() {
                return None;
            }
            Some((name.to_string(), hex.to_ascii_lowercase()))
        })
        .collect()
}

/// Verify a built `dist/` directory against its `SHA256SUMS` manifest.
///
/// Reads `dist/SHA256SUMS`, then for each listed entry reads the artifact bytes
/// (when present) and classifies it via [`verify_listing`]. A missing manifest
/// yields a report with `manifest_missing = true` and no results, so callers can
/// distinguish "nothing to verify" from "all artifacts failed".
#[cfg(test)]
fn verify_dir(dist: &Path) -> VerifyReport {
    let manifest_path = dist.join(SHA256SUMS_NAME);
    let contents = match std::fs::read_to_string(&manifest_path) {
        Ok(c) => c,
        Err(_) => {
            return VerifyReport {
                results: Vec::new(),
                manifest_missing: true,
            };
        }
    };

    let expected = parse_sha256sums(&contents);
    let present: Vec<(String, Vec<u8>)> = expected
        .iter()
        .filter_map(|(name, _)| {
            std::fs::read(dist.join(name))
                .ok()
                .map(|bytes| (name.clone(), bytes))
        })
        .collect();

    VerifyReport {
        results: verify_listing(&expected, &present),
        manifest_missing: false,
    }
}

/// Resolve the `dist/` directory to verify from a raw `BHARATCODE_DIST_DIR`
/// value, falling back to [`DEFAULT_DIST_DIR`] when `None` or empty.
#[cfg(test)]
fn dist_dir_from(raw: Option<&str>) -> PathBuf {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => PathBuf::from(s),
        None => PathBuf::from(DEFAULT_DIST_DIR),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha_of(bytes: &[u8]) -> String {
        sha256_hex(bytes)
    }

    #[test]
    fn artifact_names_for_linux_x86_contains_expected() {
        let names = artifact_names("x86_64-unknown-linux-gnu");
        assert!(names.contains(&"bharatcode-x86_64-unknown-linux-gnu.tar.bz2".to_string()));
        assert!(names.contains(&"bharatcode-x86_64-unknown-linux-gnu.zip".to_string()));
        assert!(names.contains(&"bharatcode-x86_64-unknown-linux-gnu.deb".to_string()));
        assert!(names.contains(&"bharatcode-x86_64-unknown-linux-gnu.rpm".to_string()));
        assert_eq!(names.len(), 4);
    }

    #[test]
    fn artifact_names_for_unknown_triple_is_empty() {
        assert!(artifact_names("sparc-unknown-none").is_empty());
    }

    #[test]
    fn brew_stanza_embeds_both_shas_and_version() {
        let sha_x86 = "a".repeat(64);
        let sha_arm = "b".repeat(64);
        let stanza = brew_formula_stanza("1.2.3", &sha_x86, &sha_arm);
        assert!(stanza.contains("1.2.3"));
        assert!(stanza.contains(&sha_x86));
        assert!(stanza.contains(&sha_arm));
        // Each arch references its own tar.bz2 artifact.
        assert!(stanza.contains("bharatcode-x86_64-apple-darwin.tar.bz2"));
        assert!(stanza.contains("bharatcode-aarch64-apple-darwin.tar.bz2"));
    }

    #[test]
    fn verify_listing_classifies_ok_mismatch_missing() {
        let good = b"artifact-one-bytes".to_vec();
        let bad_actual = b"different-bytes".to_vec();
        let expected = vec![
            ("ok.tar.bz2".to_string(), sha_of(&good)),
            ("bad.tar.bz2".to_string(), sha_of(b"the-expected-bytes")),
            ("gone.tar.bz2".to_string(), sha_of(b"whatever")),
        ];
        let present = vec![
            ("ok.tar.bz2".to_string(), good.clone()),
            ("bad.tar.bz2".to_string(), bad_actual),
            // gone.tar.bz2 deliberately absent
        ];

        let results = verify_listing(&expected, &present);
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].status, ArtifactStatus::Ok);
        assert_eq!(results[1].status, ArtifactStatus::Mismatch);
        assert_eq!(results[2].status, ArtifactStatus::Missing);
    }

    #[test]
    fn verify_listing_is_case_insensitive_on_expected_hex() {
        let bytes = b"case-test".to_vec();
        let upper = sha_of(&bytes).to_ascii_uppercase();
        let expected = vec![("c.zip".to_string(), upper)];
        let present = vec![("c.zip".to_string(), bytes)];
        let results = verify_listing(&expected, &present);
        assert_eq!(results[0].status, ArtifactStatus::Ok);
    }

    #[test]
    fn parse_sha256sums_handles_two_space_and_binary_marker() {
        let hex = "d".repeat(64);
        let contents =
            format!("{hex}  plain.tar.bz2\n{hex} *binary.zip\n\n  \n{hex}  spaced.deb\n");
        let parsed = parse_sha256sums(&contents);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0], ("plain.tar.bz2".to_string(), hex.clone()));
        assert_eq!(parsed[1], ("binary.zip".to_string(), hex.clone()));
        assert_eq!(parsed[2], ("spaced.deb".to_string(), hex));
    }

    #[test]
    fn verify_dir_roundtrip_with_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let dist = dir.path();

        let tar_bytes = b"real-tar-contents".to_vec();
        let tar_name = "bharatcode-x86_64-unknown-linux-gnu.tar.bz2";
        let deb_bytes = b"real-deb-contents".to_vec();
        let deb_name = "bharatcode-x86_64-unknown-linux-gnu.deb";
        let rpm_name = "bharatcode-x86_64-unknown-linux-gnu.rpm";

        std::fs::write(dist.join(tar_name), &tar_bytes).unwrap();
        std::fs::write(dist.join(deb_name), b"tampered-deb").unwrap();
        // rpm listed but not written -> Missing.

        let manifest = format!(
            "{}  {tar_name}\n{}  {deb_name}\n{}  {rpm_name}\n",
            sha_of(&tar_bytes),
            sha_of(&deb_bytes),
            sha_of(b"original-rpm"),
        );
        std::fs::write(dist.join(SHA256SUMS_NAME), manifest).unwrap();

        let report = verify_dir(dist);
        assert!(!report.manifest_missing);
        assert_eq!(report.count(ArtifactStatus::Ok), 1);
        assert_eq!(report.count(ArtifactStatus::Mismatch), 1);
        assert_eq!(report.count(ArtifactStatus::Missing), 1);
        assert!(!report.all_ok());
    }

    #[test]
    fn verify_dir_reports_missing_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let report = verify_dir(dir.path());
        assert!(report.manifest_missing);
        assert!(report.results.is_empty());
        assert!(!report.all_ok());
        assert_eq!(
            report.summary_lines(),
            vec!["SHA256SUMS: missing".to_string()]
        );
    }

    #[test]
    fn dist_dir_defaults_when_unset_or_empty() {
        assert_eq!(dist_dir_from(None), PathBuf::from(DEFAULT_DIST_DIR));
        assert_eq!(dist_dir_from(Some("  ")), PathBuf::from(DEFAULT_DIST_DIR));
        assert_eq!(dist_dir_from(Some("/tmp/dist")), PathBuf::from("/tmp/dist"));
    }

    #[test]
    fn matrix_summary_lines_has_header_and_one_row_per_target() {
        let lines = matrix_summary_lines();
        assert_eq!(lines.len(), MATRIX.len() + 1);
        assert!(lines[0].contains(&MATRIX.len().to_string()));
        for (i, t) in MATRIX.iter().enumerate() {
            assert!(lines[i + 1].contains(t.triple));
            assert!(lines[i + 1].contains(t.tar_name));
        }
    }

    #[test]
    fn no_brand_leak_in_matrix_names() {
        const BANNED: &[&str] = &["goose", "Goose", "block", "Block"];
        for t in MATRIX {
            for name in [t.tar_name, t.zip_name, t.deb_name, t.rpm_name] {
                assert!(
                    name.starts_with("bharatcode-"),
                    "artifact {name} must keep the bharatcode- prefix"
                );
                for banned in BANNED {
                    assert!(
                        !name.contains(banned),
                        "artifact {name} leaks brand token {banned}"
                    );
                }
            }
        }
    }
}
