//! Tamil (`ta`) locale extension for the BharatCode CLI i18n scaffold
//! (BharatCode v81).
//!
//! The locale enum, `ta.json` table, the `normalize_locale` arm (`"ta" => Ta`)
//! and the `t()` fall-through (`ta -> en`) all live in [`crate::i18n`]. This
//! module deliberately does NOT re-declare the locale table; it only adds a free
//! function the CLI calls at a real site (the `doctor` settings summary) plus a
//! three-way parity guard so `en.json`, `hi.json` and `ta.json` never drift.
//!
//! Activating Tamil is purely opt-in: set `BHARATCODE_LANG=ta` (or
//! `bharatcode_lang=ta` in config, or `LANG=ta_IN`). With any other locale the
//! English / Hindi output is byte-for-byte unchanged because the `ta` arm only
//! fires once the resolver maps the active locale to [`crate::i18n::Locale::Ta`].

use crate::i18n::Locale;

/// i18n key prefix for the human-readable display name of each locale.
const LANG_NAME_PREFIX: &str = "lang.name.";

/// Stable BCP-47-ish tag for a [`Locale`], used to build its `lang.name.<tag>`
/// lookup key. Kept here (rather than on the enum in `mod.rs`) so the mapping
/// lives in the Tamil extension that introduced the `lang.name.*` baseline.
fn locale_tag(locale: Locale) -> &'static str {
    match locale {
        Locale::En => "en",
        Locale::Hi => "hi",
        Locale::Ta => "ta",
    }
}

/// The display name of the currently active locale, rendered in that locale's
/// own script (e.g. `தமிழ்` under Tamil, `हिंदी` under Hindi, `English` under
/// English).
///
/// This is the real, wired call site for the `lang.name.*` keys this wave adds
/// to `en.json`: the `doctor` command prints it as the "Language" row. Because
/// it routes through [`crate::i18n::t`], it honours the same resolver order and
/// `ta -> en` fall-through as every other translated string.
pub fn active_lang_name() -> String {
    let tag = locale_tag(crate::i18n::active_locale());
    crate::tr!(&format!("{LANG_NAME_PREFIX}{tag}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::translate_in;
    use std::collections::HashMap;

    fn parse(raw: &str) -> HashMap<String, String> {
        serde_json::from_str(raw).expect("i18n: locale table is not valid JSON")
    }

    fn en() -> HashMap<String, String> {
        parse(include_str!("en.json"))
    }
    fn hi() -> HashMap<String, String> {
        parse(include_str!("hi.json"))
    }
    fn ta() -> HashMap<String, String> {
        parse(include_str!("ta.json"))
    }

    /// Every key in the source-of-truth `en.json` must be translatable in BOTH
    /// `hi.json` and `ta.json` (three-way parity).
    #[test]
    fn every_english_key_exists_in_hindi_and_tamil() {
        let (en, hi, ta) = (en(), hi(), ta());
        for key in en.keys() {
            assert!(
                hi.contains_key(key),
                "hi.json is missing key present in en.json: {key}"
            );
            assert!(
                ta.contains_key(key),
                "ta.json is missing key present in en.json: {key}"
            );
        }
    }

    /// The Tamil table must not carry empty values for the keys en.json defines.
    #[test]
    fn tamil_values_are_non_empty_for_english_keys() {
        let (en, ta) = (en(), ta());
        for key in en.keys() {
            let value = ta
                .get(key)
                .unwrap_or_else(|| panic!("ta.json is missing key: {key}"));
            assert!(
                !value.trim().is_empty(),
                "ta.json has an empty value for key: {key}"
            );
        }
    }

    /// The Tamil-only wave keys (`lang.name.*`) resolve to the Tamil string
    /// under [`Locale::Ta`] and to the English string under [`Locale::En`],
    /// proving the `t()`/`translate_in` fall-through is wired both ways.
    #[test]
    fn lang_name_key_resolves_per_locale() {
        let key = "lang.name.ta";

        let under_ta = translate_in(Locale::Ta, key);
        assert_eq!(
            under_ta,
            ta()[key],
            "Ta locale must return the ta.json value"
        );
        assert_eq!(under_ta, "தமிழ்");

        let under_en = translate_in(Locale::En, key);
        assert_eq!(
            under_en,
            en()[key],
            "En locale must return the en.json value"
        );
        assert_eq!(under_en, "Tamil");

        // The two locales must genuinely differ for this key, otherwise the
        // Tamil table would just be echoing English.
        assert_ne!(under_ta, under_en);
    }

    /// `lang.name.en` is intentionally ASCII "English" in every table, so it is
    /// the same under En and (by design) under Ta's own entry for English.
    #[test]
    fn english_lang_name_is_stable() {
        assert_eq!(translate_in(Locale::En, "lang.name.en"), "English");
        assert_eq!(en()["lang.name.en"], "English");
    }

    /// The active-locale display-name helper maps each locale to its own-script
    /// name via the `lang.name.<tag>` key it builds.
    #[test]
    fn locale_tag_drives_lang_name_lookup() {
        assert_eq!(
            translate_in(
                Locale::Ta,
                &format!("{LANG_NAME_PREFIX}{}", locale_tag(Locale::Ta))
            ),
            "தமிழ்"
        );
        assert_eq!(
            translate_in(
                Locale::Hi,
                &format!("{LANG_NAME_PREFIX}{}", locale_tag(Locale::Hi))
            ),
            "हिंदी"
        );
        assert_eq!(
            translate_in(
                Locale::En,
                &format!("{LANG_NAME_PREFIX}{}", locale_tag(Locale::En))
            ),
            "English"
        );
    }
}
