//! Hindi (hi) i18n parity guard for the BharatCode CLI command surfaces.
//!
//! BharatCode v81 deepens Hindi coverage so that, under `BHARATCODE_LANG=hi`,
//! the user-facing command screens shipped across v8-v60 (cost, budget, privacy,
//! recipes-library, doctor deep-checks, presets, review-diff, audit) render in
//! Hindi instead of falling back to English. English output stays byte-identical
//! because `en.json` is never touched by this wave.
//!
//! This integration test loads the two bundled locale tables and asserts the
//! three properties that make Hindi a genuine first-class locale rather than an
//! English shadow copy:
//!
//!   (a) coverage: every key in `en.json` also exists in `hi.json`;
//!   (b) translation: no human-readable `hi.json` value is byte-identical to its
//!       English counterpart (a purely-symbolic allowlist — locale-code lists and
//!       the like — is exempt because those values are meant to match across
//!       locales);
//!   (c) script: every human-readable `hi.json` value contains at least one
//!       character in the Devanagari block (U+0900..U+097F), proving the string
//!       is actually written in Hindi.
//!
//! The locale tables are embedded with `include_str!` so the test exercises the
//! exact bytes the running binary bundles, with no dependency on the working
//! directory at test time.

use std::collections::BTreeMap;

/// The bundled English locale table (owned by a sibling wave; read-only here).
const EN_JSON: &str = include_str!("../src/i18n/en.json");
/// The bundled Hindi locale table (deepened by BharatCode v81).
const HI_JSON: &str = include_str!("../src/i18n/hi.json");

/// Keys whose value is a purely-symbolic token (e.g. a comma-separated list of
/// locale codes) rather than human-readable prose. Such values are intentionally
/// identical across locales and carry no Devanagari, so they are exempt from the
/// "must differ" and "must contain Devanagari" checks. Coverage (property (a))
/// is still enforced for these keys.
const SYMBOLIC_KEY_ALLOWLIST: &[&str] = &["meta.locales"];

fn parse(raw: &str, name: &str) -> BTreeMap<String, String> {
    serde_json::from_str(raw).unwrap_or_else(|e| panic!("{name} is not valid JSON: {e}"))
}

fn is_symbolic(key: &str) -> bool {
    SYMBOLIC_KEY_ALLOWLIST.contains(&key)
}

/// True when `s` contains at least one character in the Devanagari Unicode block
/// (U+0900..U+097F), i.e. the script Hindi is written in.
fn has_devanagari(s: &str) -> bool {
    s.chars().any(|c| ('\u{0900}'..='\u{097F}').contains(&c))
}

/// (a) Every English key must have a Hindi counterpart so no command surface can
/// silently fall back to English under `BHARATCODE_LANG=hi`.
#[test]
fn every_english_key_has_a_hindi_value() {
    let en = parse(EN_JSON, "en.json");
    let hi = parse(HI_JSON, "hi.json");

    let missing: Vec<&String> = en.keys().filter(|k| !hi.contains_key(*k)).collect();
    assert!(
        missing.is_empty(),
        "hi.json is missing Hindi values for keys present in en.json: {missing:?}"
    );
}

/// (b) No human-readable Hindi value may be a byte-for-byte copy of its English
/// value: that would mean the screen still renders English under `hi`.
#[test]
fn hindi_values_differ_from_english() {
    let en = parse(EN_JSON, "en.json");
    let hi = parse(HI_JSON, "hi.json");

    let untranslated: Vec<&String> = en
        .keys()
        .filter(|k| !is_symbolic(k))
        .filter(|k| matches!((en.get(*k), hi.get(*k)), (Some(e), Some(h)) if e == h))
        .collect();
    assert!(
        untranslated.is_empty(),
        "these hi.json values are byte-identical to en.json (still English): {untranslated:?}"
    );
}

/// (c) Every human-readable Hindi value must actually be written in Devanagari,
/// proving the deepened coverage is real Hindi and not a transliteration stub.
#[test]
fn hindi_values_contain_devanagari() {
    let hi = parse(HI_JSON, "hi.json");

    let no_script: Vec<&String> = hi
        .iter()
        .filter(|(k, _)| !is_symbolic(k))
        .filter(|(_, v)| !has_devanagari(v))
        .map(|(k, _)| k)
        .collect();
    assert!(
        no_script.is_empty(),
        "these hi.json values contain no Devanagari (U+0900..U+097F): {no_script:?}"
    );
}

/// Every allowlisted symbolic key must actually exist in both tables; a stale
/// allowlist entry would silently weaken properties (b) and (c).
#[test]
fn symbolic_allowlist_is_not_stale() {
    let en = parse(EN_JSON, "en.json");
    let hi = parse(HI_JSON, "hi.json");

    for key in SYMBOLIC_KEY_ALLOWLIST {
        assert!(
            en.contains_key(*key) && hi.contains_key(*key),
            "symbolic-allowlist key {key:?} is missing from en.json or hi.json (stale allowlist)"
        );
    }
}
