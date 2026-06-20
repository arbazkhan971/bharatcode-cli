//! SBOM / dependency-license summary for `bharatcode doctor` (BharatCode v93).
//!
//! This module embeds the authoritative `THIRD_PARTY_LICENSES.md` manifest at
//! compile time via `include_str!` and exposes [`sbom_readiness`], a read-only
//! probe that the doctor command renders as one always-on flat readiness row.
//!
//! The manifest's `## Summary` block carries the canonical bold figures (the
//! number of third-party crates and the number of distinct SPDX license
//! expressions in the portable-default release tree). [`parse_summary`] reads
//! those figures; if the summary is ever reshaped, [`count_from_tables`] falls
//! back to recounting from the per-license table rows and `### <license>`
//! section headers.
//!
//! Purely informational and read-only: it only reads the compile-time embedded
//! string — no network, no filesystem, no config mutation — so default
//! behaviour is unchanged and no opt-in env var is required, consistent with the
//! other always-on doctor readiness rows.

use crate::commands::doctor_checks::Status;

/// The dependency-license manifest, embedded at compile time so the readiness
/// row needs no filesystem access at runtime.
const MANIFEST: &str = include_str!("../../../../THIRD_PARTY_LICENSES.md");

/// Look up a user-facing label through the i18n layer, falling back to the
/// English `default` when the active locale table has no entry for `key`.
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// Parse a bold integer figure (`**619**`) that follows `marker` on its line in
/// the manifest's `## Summary` block. Returns `None` if the line or the bold
/// figure is absent.
fn parse_bold_after(text: &str, marker: &str) -> Option<u64> {
    for line in text.lines() {
        if let Some(idx) = line.find(marker) {
            let rest = &line[idx + marker.len()..];
            if let Some(start) = rest.find("**") {
                let after = &rest[start + 2..];
                if let Some(end) = after.find("**") {
                    let digits: String =
                        after[..end].chars().filter(|c| c.is_ascii_digit()).collect();
                    if !digits.is_empty() {
                        if let Ok(n) = digits.parse::<u64>() {
                            return Some(n);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Read the canonical `(crate_count, license_count)` from the `## Summary`
/// block's bold figures. Returns `None` if either figure is missing.
fn parse_summary(text: &str) -> Option<(u64, u64)> {
    let crates = parse_bold_after(text, "portable-default tree:")?;
    let licenses = parse_bold_after(text, "Distinct SPDX license expressions:")?;
    Some((crates, licenses))
}

/// Fallback recount used only if the `## Summary` block is reshaped: sum the
/// per-license `| `SPDX` | N |` table rows for the crate count, and count the
/// distinct `### <license>` section headers (or, failing that, the distinct
/// table rows) for the license count.
fn count_from_tables(text: &str) -> (u64, u64) {
    let mut crate_total: u64 = 0;
    let mut table_rows: u64 = 0;
    let mut header_licenses: u64 = 0;

    for line in text.lines() {
        let trimmed = line.trim();

        // Distinct `### <license>` per-license section headers.
        if let Some(rest) = trimmed.strip_prefix("### ") {
            if !rest.trim().is_empty() {
                header_licenses += 1;
            }
            continue;
        }

        // Per-license summary rows: `| `SPDX` | N |`. Exclude the header row
        // (`| SPDX license expression | Crate count |`) and the `|---|---|`
        // separator row.
        if trimmed.starts_with('|') && trimmed.contains('`') {
            let cells: Vec<&str> = trimmed.trim_matches('|').split('|').collect();
            if let Some(last) = cells.last() {
                let digits: String = last.chars().filter(|c| c.is_ascii_digit()).collect();
                if !digits.is_empty() {
                    if let Ok(n) = digits.parse::<u64>() {
                        crate_total += n;
                        table_rows += 1;
                    }
                }
            }
        }
    }

    let licenses = if header_licenses > 0 {
        header_licenses
    } else {
        table_rows
    };
    (crate_total, licenses)
}

/// Resolve the `(crate_count, license_count)` for the embedded manifest,
/// preferring the authoritative summary figures and falling back to a recount.
fn resolve_counts(text: &str) -> (u64, u64) {
    parse_summary(text).unwrap_or_else(|| count_from_tables(text))
}

/// Read-only SBOM readiness row for the doctor command.
///
/// Always returns [`Status::Ok`] with a one-line rollup of how many third-party
/// crates the release links against and how many distinct SPDX license families
/// cover them, e.g. `Dependency licenses: 619 deps across 41 SPDX license
/// families`.
pub fn sbom_readiness() -> (Status, String) {
    let (crates, licenses) = resolve_counts(MANIFEST);
    let prefix = label("doctor.sbom.label", "Dependency licenses");
    let deps = label("doctor.sbom.deps", "deps across");
    let families = label("doctor.sbom.families", "SPDX license families");
    let msg = format!("{prefix}: {crates} {deps} {licenses} {families}");
    (Status::Ok, msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bold_summary_figures() {
        let doc = [
            "## Summary",
            "",
            "- Third-party crates in the `bharatcode` CLI portable-default tree: **619**",
            "- Distinct SPDX license expressions: **41**",
        ]
        .join("\n");
        assert_eq!(parse_summary(&doc), Some((619, 41)));
    }

    #[test]
    fn rejects_unrelated_lines() {
        let doc = "Some prose with no bold figures at all.";
        assert_eq!(parse_summary(doc), None);
    }

    #[test]
    fn fallback_sums_table_rows_and_headers() {
        let doc = [
            "| SPDX license expression | Crate count |",
            "|---|---|",
            "| `MIT OR Apache-2.0` | 10 |",
            "| `MIT` | 5 |",
            "### MIT OR Apache-2.0",
            "### MIT",
        ]
        .join("\n");
        let (crates, licenses) = count_from_tables(&doc);
        assert_eq!(crates, 15);
        assert_eq!(licenses, 2);
    }

    #[test]
    fn fallback_excludes_header_and_separator_rows() {
        // The header row has no backtick-quoted SPDX cell and the separator row
        // has no digits, so neither contributes to the crate total.
        let doc = [
            "| SPDX license expression | Crate count |",
            "|---|---|",
            "| `MIT` | 7 |",
        ]
        .join("\n");
        let (crates, _licenses) = count_from_tables(&doc);
        assert_eq!(crates, 7);
    }

    #[test]
    fn readiness_renders_thousands_free_rollup() {
        let (status, msg) = sbom_readiness();
        assert_eq!(status, Status::Ok);
        assert!(msg.contains("Dependency licenses"), "got: {msg}");
        assert!(msg.contains("SPDX license families"), "got: {msg}");
    }

    #[test]
    fn live_embedded_manifest_has_plausible_counts() {
        let (crates, licenses) = resolve_counts(MANIFEST);
        assert!(crates > 100, "expected many crates, got {crates}");
        assert!(licenses > 5, "expected several licenses, got {licenses}");
        // No upstream brand leakage in the rendered readiness row.
        let (_s, msg) = sbom_readiness();
        let lower = msg.to_lowercase();
        assert!(!lower.contains("goose"), "brand leak: {msg}");
        assert!(!lower.contains("block"), "brand leak: {msg}");
    }
}
