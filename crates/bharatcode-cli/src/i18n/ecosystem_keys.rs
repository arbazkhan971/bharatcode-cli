//! Canonical list of the ecosystem-surface i18n keys and a parity guard.
//!
//! The v72/v77 ecosystem handlers (recipe export/import, the MCP extension
//! registry header, and the CI-readiness footer) route their user-facing status
//! lines through [`crate::tr!`]. Those keys are defined in both `en.json` and
//! `hi.json`; this module is the single place that enumerates them so a test can
//! assert the two locale tables stay in lockstep.
//!
//! Activating Hindi for these strings is purely opt-in: set `BHARATCODE_LANG=hi`
//! (the locale resolver in [`crate::i18n`] reads it first). With the default
//! locale the English values are emitted byte-for-byte unchanged.

/// Every ecosystem-surface key that v79 guarantees exists in both locale tables.
///
/// Keep this list in sync with the `ecosystem.*` entries added to `en.json` and
/// `hi.json`; the parity test below fails loudly if it drifts.
pub const ECOSYSTEM_KEYS: &[&str] = &[
    "ecosystem.recipe_exported",
    "ecosystem.recipe_imported",
    "ecosystem.checksum_failed",
    "ecosystem.mcp_registry_header",
    "ecosystem.ci_ready",
    "ecosystem.ci_no_step",
    "ecosystem.automation_on",
];

/// Parse a locale JSON payload into a flat key/value map.
///
/// Shared by [`assert_parity`] and the unit tests so both read the tables the
/// exact same way [`crate::i18n`] does at runtime.
#[cfg(test)]
fn parse_table(raw: &str) -> std::collections::HashMap<String, String> {
    serde_json::from_str(raw).expect("i18n: locale table is not valid JSON")
}

/// Assert that the bundled `en.json` and `hi.json` tables agree on every key and
/// that each [`ECOSYSTEM_KEYS`] entry resolves to a non-empty value in both.
///
/// Intended as a reusable test helper: it panics with a precise message on the
/// first discrepancy rather than returning an error, so it reads cleanly inside
/// a `#[test]`.
#[cfg(test)]
pub fn assert_parity() {
    let en = parse_table(include_str!("en.json"));
    let hi = parse_table(include_str!("hi.json"));

    for key in ECOSYSTEM_KEYS {
        let en_val = en
            .get(*key)
            .unwrap_or_else(|| panic!("en.json is missing ecosystem key: {key}"));
        assert!(
            !en_val.trim().is_empty(),
            "en.json has an empty value for ecosystem key: {key}"
        );

        let hi_val = hi
            .get(*key)
            .unwrap_or_else(|| panic!("hi.json is missing ecosystem key: {key}"));
        assert!(
            !hi_val.trim().is_empty(),
            "hi.json has an empty value for ecosystem key: {key}"
        );
    }

    for key in en.keys() {
        assert!(
            hi.contains_key(key),
            "hi.json is missing key present in en.json: {key}"
        );
    }
    for key in hi.keys() {
        assert!(
            en.contains_key(key),
            "en.json is missing key present in hi.json: {key}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ecosystem_keys_present_in_both_tables() {
        assert_parity();
    }

    #[test]
    fn ecosystem_keys_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for key in ECOSYSTEM_KEYS {
            assert!(seen.insert(*key), "duplicate ecosystem key: {key}");
        }
    }

    #[test]
    fn en_and_hi_have_identical_key_sets() {
        let en = parse_table(include_str!("en.json"));
        let hi = parse_table(include_str!("hi.json"));
        let en_keys: std::collections::BTreeSet<&String> = en.keys().collect();
        let hi_keys: std::collections::BTreeSet<&String> = hi.keys().collect();
        assert_eq!(
            en_keys, hi_keys,
            "en.json and hi.json key sets differ (no missing/extra keys allowed)"
        );
    }

    #[test]
    fn pre_existing_en_values_are_unchanged_ascii() {
        let en = parse_table(include_str!("en.json"));
        let expected = [
            ("session.ready", "bharatcode is ready"),
            (
                "error.no_provider",
                "No provider configured. Run 'bharatcode configure' first.",
            ),
            ("cost.total", "Total spend"),
            ("recipes_library.unknown", "Unknown recipe template"),
        ];
        for (key, value) in expected {
            assert_eq!(
                en.get(key).map(String::as_str),
                Some(value),
                "pre-existing en.json value changed for key: {key}"
            );
            assert!(
                value.is_ascii(),
                "pre-existing en.json value is not ASCII for key: {key}"
            );
        }
    }
}
