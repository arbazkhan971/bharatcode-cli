//! Hindi coverage manifest for the v82 "deepen Hindi" wave (BharatCode).
//!
//! This wave widens Hindi past the original starter strings into the long tail of
//! high-traffic, user-facing CLI surfaces that other v8x siblings introduce in
//! the same wave:
//!
//!   * `onboarding.*` — the guided first-run wizard steps,
//!   * `helpindex.*`  — the `bharatcode help-index` category headings,
//!   * `a11y.*`       — the accessibility / screen-reader status labels,
//!   * `tutorials.*`  — the quickstart tutorial steps,
//!   * `dashboard.*`  — the cost-dashboard headings, and
//!   * `notify.*`     — the desktop / terminal notification labels.
//!
//! Every key listed in [`hindi_coverage_keys`] already carries a genuine Hindi
//! value in the sibling `hi.json` table (compiled into the binary via
//! `include_str!` by [`crate::i18n`]). The values only *surface* when the active
//! locale resolves to Hindi (`BHARATCODE_LANG=hi`, `bharatcode_lang=hi`, or
//! `LANG=hi_IN`); with any other locale the English strings render byte-for-byte
//! unchanged, so this module changes no default behaviour. It is data + a small
//! accessor, with no env gate of its own.
//!
//! [`count_translated`] reports how many of those keys a given locale table fills
//! with a real Hindi value (a value that carries at least one Devanagari
//! codepoint), so the help/doctor coverage surface can report Hindi depth without
//! re-deriving the canonical key list. The unit tests assert each canonical key
//! exists in the shipped `hi.json` and that its value is genuinely translated —
//! not an English echo.

use std::collections::HashMap;

/// The canonical list of Hindi-deepened keys this wave covers, grouped by the
/// stable namespaces it introduces.
///
/// Each entry must exist in `hi.json` with a genuinely-translated (Devanagari)
/// value; the unit tests below fail loudly if any key is missing or still echoes
/// English. The matching English defaults flow through the `tr!`-based call sites
/// (the onboarding wizard, `help-index`, the accessibility banner, the tutorials
/// runner, the cost dashboard, and the notifier), so deepening Hindi here needs
/// no change at those call sites — only the locale table.
const HINDI_COVERAGE_KEYS: &[&str] = &[
    // onboarding wizard
    "onboarding.title",
    "onboarding.step_locale",
    "onboarding.step_provider",
    "onboarding.step_theme",
    "onboarding.step_privacy",
    "onboarding.apply_hint",
    // help-index category headings
    "helpindex.header",
    "helpindex.cat_session",
    "helpindex.cat_conversation",
    "helpindex.cat_model",
    "helpindex.cat_extensions",
    "helpindex.cat_display",
    "helpindex.cat_navigation",
    // accessibility status labels
    "a11y.enabled",
    "a11y.spinner_label",
    // quickstart tutorials
    "tutorials.quickstart_title",
    "tutorials.quickstart_step1",
    "tutorials.quickstart_step2",
    "tutorials.next_hint",
    // cost dashboard headings
    "dashboard.title",
    "dashboard.bar_legend",
    "dashboard.top_models",
    // notification labels
    "notify.turn_done",
    "notify.verify_failed",
];

/// The canonical list of namespaces this wave deepens into Hindi.
///
/// Returned as a `'static` slice so the help/doctor coverage surface and the
/// parity tests share one source of truth. Order is stable (namespace-grouped) so
/// rendered output is deterministic.
pub fn hindi_coverage_keys() -> &'static [&'static str] {
    HINDI_COVERAGE_KEYS
}

/// Whether `value` carries at least one Devanagari codepoint (`U+0900..=U+097F`),
/// i.e. it is a genuine Hindi translation rather than an English echo.
fn is_devanagari(value: &str) -> bool {
    value.chars().any(|c| matches!(c, '\u{0900}'..='\u{097F}'))
}

/// Count how many of the canonical coverage keys `table` fills with a genuinely
/// translated (Devanagari) Hindi value.
///
/// `table` is a flat key/value locale map shaped exactly like the one
/// [`crate::i18n`] builds from `hi.json`. A key that is absent, empty, or still
/// holding the English string does not count, so the returned figure is true
/// Hindi depth across this wave's surfaces — never an inflated parity count.
pub fn count_translated(table: &HashMap<String, String>) -> usize {
    HINDI_COVERAGE_KEYS
        .iter()
        .filter(|key| table.get(**key).is_some_and(|value| is_devanagari(value)))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hindi_table() -> HashMap<String, String> {
        serde_json::from_str(include_str!("hi.json")).expect("i18n: hi.json is not valid JSON")
    }

    #[test]
    fn coverage_keys_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for key in hindi_coverage_keys() {
            assert!(seen.insert(*key), "duplicate coverage key: {key}");
        }
    }

    /// The wave's core guarantee: every canonical key exists in the shipped
    /// `hi.json` and its value contains a Devanagari codepoint, proving the value
    /// is a real Hindi translation and not an English echo.
    #[test]
    fn every_coverage_key_has_a_devanagari_hindi_value() {
        let hi = hindi_table();
        for key in hindi_coverage_keys() {
            let value = hi
                .get(*key)
                .unwrap_or_else(|| panic!("hi.json is missing Hindi-coverage key: {key}"));
            assert!(
                !value.trim().is_empty(),
                "hi.json has an empty value for coverage key: {key}"
            );
            assert!(
                is_devanagari(value),
                "hi.json value for {key} carries no Devanagari codepoint (English echo): {value:?}"
            );
        }
    }

    /// `count_translated` over the real shipped `hi.json` must report full Hindi
    /// depth across every canonical coverage key.
    #[test]
    fn count_translated_reports_full_depth_for_shipped_hindi() {
        let hi = hindi_table();
        assert_eq!(
            count_translated(&hi),
            hindi_coverage_keys().len(),
            "every coverage key must be genuinely translated in hi.json"
        );
    }

    /// English (or otherwise non-Devanagari) values never inflate the count, so a
    /// future English echo would lower the reported Hindi depth rather than hide.
    #[test]
    fn count_translated_ignores_english_echoes() {
        let mut table: HashMap<String, String> = HashMap::new();
        // First key translated, the rest left as English echoes.
        let keys = hindi_coverage_keys();
        table.insert(keys[0].to_string(), "नमस्ते".to_string());
        for key in &keys[1..] {
            table.insert(key.to_string(), "English text".to_string());
        }
        assert_eq!(count_translated(&table), 1);
    }

    #[test]
    fn devanagari_detector_distinguishes_scripts() {
        assert!(is_devanagari("हिंदी"));
        assert!(is_devanagari("BharatCode डैशबोर्ड"));
        assert!(!is_devanagari("English only"));
        assert!(!is_devanagari("en,hi,ta"));
        assert!(!is_devanagari(""));
    }
}
