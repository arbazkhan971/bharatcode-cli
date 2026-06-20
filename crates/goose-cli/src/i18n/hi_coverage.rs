//! Canonical list of the Hindi-deepened high-traffic CLI keys and a parity guard.
//!
//! v82 deepens Hindi coverage across three high-traffic CLI surfaces whose
//! user-facing strings already route through a `default`-bearing translation
//! helper in the running binary:
//!
//!   * the `/help` group headings and title (`help_tr(key, default)` in
//!     [`crate::session::input`]),
//!   * the doctor "Deep checks" labels (`label(key, default)` in the
//!     `doctor_checks` command), and
//!   * the doctor session/banner words such as provider/model and the config
//!     directory row (`label(key, default)` in the `doctor` command).
//!
//! Each of those helpers echoes the `key` back when the active locale table has
//! no entry, so the call sites render English from their `default` argument
//! today. Adding the Hindi values to `hi.json` (this module's sibling file) makes
//! them switch to Hindi under `BHARATCODE_LANG=hi` WITHOUT touching the call
//! sites — that is the real wiring.
//!
//! Activating Hindi for these strings is purely opt-in: set `BHARATCODE_LANG=hi`
//! (the locale resolver in [`crate::i18n`] reads it first). With the default
//! locale the English `default` values are emitted byte-for-byte unchanged.
//!
//! This module is the single place that enumerates the deepened keys so a test
//! can assert `hi.json` carries a real Hindi value for each one and that the two
//! locale tables stay in lockstep.

/// Every key whose Hindi value v82 adds to `hi.json` so it stops echoing the
/// English `default` under `BHARATCODE_LANG=hi`.
///
/// Keep this list in sync with the entries added to `hi.json`; the parity test
/// below fails loudly if it drifts. The matching English defaults flow through
/// the `help_tr`/`label` call sites named in the module docs, so this version
/// deliberately does NOT add these keys to `en.json` (a sibling version owns it).
pub const HINDI_DEEPENED_KEYS: &[&str] = &[
    "help.title",
    "help.group.session",
    "help.group.conversation",
    "help.group.model_mode",
    "help.group.extensions",
    "help.group.display",
    "help.group.navigation",
    "doctor.checks_title",
    "doctor.check.git",
    "doctor.check.local_provider",
    "doctor.check.config_writable",
    "doctor.check.session_db",
    "doctor.provider_model",
    "doctor.local_engine",
    "doctor.config_dir",
];

/// Parse a locale JSON payload into a flat key/value map.
///
/// Reads the tables the exact same way [`crate::i18n`] does at runtime so the
/// test asserts against the real bundled data.
#[cfg(test)]
fn parse_table(raw: &str) -> std::collections::HashMap<String, String> {
    serde_json::from_str(raw).expect("i18n: locale table is not valid JSON")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deepened_keys_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for key in HINDI_DEEPENED_KEYS {
            assert!(seen.insert(*key), "duplicate deepened key: {key}");
        }
    }

    /// Each deepened key must carry a non-empty Hindi value that differs from the
    /// English value, so `BHARATCODE_LANG=hi` shows materially more Hindi.
    #[test]
    fn deepened_keys_have_distinct_hindi_values() {
        let en = parse_table(include_str!("en.json"));
        let hi = parse_table(include_str!("hi.json"));

        for key in HINDI_DEEPENED_KEYS {
            let hi_val = hi
                .get(*key)
                .unwrap_or_else(|| panic!("hi.json is missing deepened key: {key}"));
            assert!(
                !hi_val.trim().is_empty(),
                "hi.json has an empty value for deepened key: {key}"
            );

            let en_val = en
                .get(*key)
                .unwrap_or_else(|| panic!("en.json is missing deepened key: {key}"));
            assert_ne!(
                hi_val, en_val,
                "hi.json value for {key} must differ from its en.json value (untranslated)"
            );
        }
    }

    /// The two locale tables must agree on every key: no key may exist in one
    /// table but not the other. v82 adds the Hindi side; the sibling that owns
    /// `en.json` adds the matching English side in the same wave.
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
}
