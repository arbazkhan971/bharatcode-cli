//! BharatCode v82: Tamil (`ta`) i18n parity tests.
//!
//! Integration-level guarantees for the third locale shipped by the CLI:
//!   * the embedded `ta.json` covers every key in `en.json` (en == ta parity),
//!   * every human-facing Tamil value carries at least one character from the
//!     Tamil Unicode block (U+0B80..=U+0BFF), with the lone structural
//!     `meta.locales` locale-code list exempt,
//!   * the shared `meta.locales` hook advertises `en,hi,ta` identically in both
//!     tables, and
//!   * the public translator [`goose_cli::i18n::t`] returns an unknown key
//!     unchanged (its terminal fallback), while a real key renders in Tamil.
//!
//! The resolver/`normalize_locale` mapping (`ta_IN.UTF-8` -> `Locale::Ta`) and
//! the `translate_in(Locale::Ta, ..)` table lookup are exercised by the unit
//! tests inside `src/i18n/mod.rs`, which can reach those module-private items;
//! this file pins the data-shape invariants from outside the crate so the
//! shipped JSON cannot drift out of parity.

use std::collections::BTreeMap;

use goose_cli::i18n::t;

const EN_JSON: &str = include_str!("../src/i18n/en.json");
const TA_JSON: &str = include_str!("../src/i18n/ta.json");

/// Keys whose values are structural locale-code data rather than prose, and so
/// are deliberately identical across every locale table.
const STRUCTURAL_KEYS: &[&str] = &["meta.locales", "version.product"];

fn load(label: &str, raw: &str) -> BTreeMap<String, String> {
    serde_json::from_str(raw).unwrap_or_else(|e| panic!("i18n: {label} is not valid JSON: {e}"))
}

fn contains_tamil_block_char(s: &str) -> bool {
    s.chars().any(|c| ('\u{0B80}'..='\u{0BFF}').contains(&c))
}

#[test]
fn ta_covers_every_en_key() {
    let en = load("en.json", EN_JSON);
    let ta = load("ta.json", TA_JSON);

    for key in en.keys() {
        assert!(
            ta.contains_key(key),
            "ta.json is missing key present in en.json: {key}"
        );
    }
}

#[test]
fn ta_has_no_keys_absent_from_en() {
    let en = load("en.json", EN_JSON);
    let ta = load("ta.json", TA_JSON);

    for key in ta.keys() {
        assert!(
            en.contains_key(key),
            "ta.json carries a key that en.json does not define: {key}"
        );
    }
}

#[test]
fn every_prose_ta_value_carries_a_tamil_char() {
    let ta = load("ta.json", TA_JSON);

    for (key, value) in &ta {
        if STRUCTURAL_KEYS.contains(&key.as_str()) {
            continue;
        }
        assert!(
            contains_tamil_block_char(value),
            "ta.json value for {key:?} has no Tamil-block (U+0B80..U+0BFF) char: {value:?}"
        );
    }
}

#[test]
fn meta_locales_advertises_en_hi_ta_in_both_tables() {
    let en = load("en.json", EN_JSON);
    let ta = load("ta.json", TA_JSON);

    assert_eq!(
        en.get("meta.locales").map(String::as_str),
        Some("en,hi,ta"),
        "en.json must expose the meta.locales hook used by the locale lister"
    );
    assert_eq!(
        ta.get("meta.locales").map(String::as_str),
        Some("en,hi,ta"),
        "ta.json must mirror the structural meta.locales value byte-for-byte"
    );
}

#[test]
fn ta_translations_differ_from_english_prose() {
    let en = load("en.json", EN_JSON);
    let ta = load("ta.json", TA_JSON);

    // A spot-check key that must be genuinely localized, not the English string.
    let key = "cost.title";
    assert_eq!(
        en.get(key).map(String::as_str),
        Some("BharatCode cost ledger (INR)")
    );
    assert_ne!(
        en.get(key),
        ta.get(key),
        "ta.json must provide a real Tamil translation for {key}, not the English string"
    );
    assert!(contains_tamil_block_char(
        ta.get(key).expect("ta cost.title")
    ));
}

#[test]
fn public_translator_returns_unknown_key_unchanged() {
    // The terminal fallback of `t`: a key absent from every table renders as the
    // key itself, regardless of the process-resolved locale.
    assert_eq!(
        t("this.key.does.not.exist.v82"),
        "this.key.does.not.exist.v82"
    );
}
