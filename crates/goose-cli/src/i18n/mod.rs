//! Lightweight i18n scaffold for BharatCode CLI user-facing strings.
//!
//! Locale resolution order (first non-empty wins):
//!   1. `BHARATCODE_LANG` environment variable
//!   2. `bharatcode_lang` config parameter (`goose::config::Config`)
//!   3. `LANG` environment variable
//!   4. fallback to English (`en`)
//!
//! Each candidate is passed through [`normalize_locale`]. Only a small starter
//! set of high-traffic strings currently routes through [`t`]; English output is
//! unchanged because `en.json` holds the exact original English strings.

pub mod ecosystem_keys;
pub mod hi_coverage;

use std::collections::HashMap;
use std::sync::{LazyLock, OnceLock};

/// Supported locales for the starter i18n table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    En,
    Hi,
    Ta,
}

/// Map a raw locale token (env var or config value) to a [`Locale`].
///
/// Lowercases the input and splits on the first locale separator so values like
/// `hi_IN.UTF-8`, `hi-IN` or `HI` resolve to [`Locale::Hi`], and `ta_IN.UTF-8`,
/// `ta-IN` or `TA` resolve to [`Locale::Ta`]. Anything that is not recognized as
/// a supported regional language falls back to [`Locale::En`].
fn normalize_locale(raw: &str) -> Locale {
    let lowered = raw.trim().to_ascii_lowercase();
    let primary = lowered
        .split(|c| c == '_' || c == '-' || c == '.')
        .next()
        .unwrap_or("");
    match primary {
        "hi" => Locale::Hi,
        "ta" => Locale::Ta,
        _ => Locale::En,
    }
}

static EN: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("en.json")).expect("i18n: en.json is not valid JSON")
});

static HI: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("hi.json")).expect("i18n: hi.json is not valid JSON")
});

static TA: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("ta.json")).expect("i18n: ta.json is not valid JSON")
});

static CURRENT: OnceLock<Locale> = OnceLock::new();

/// Resolve (and cache) the active locale for this process.
///
/// Cached in a [`OnceLock`], so the locale is resolved exactly once per process.
fn current_locale() -> Locale {
    *CURRENT.get_or_init(resolve_locale)
}

/// Resolve the active locale for this process (BharatCode v81).
///
/// Thin public wrapper over the cached [`current_locale`] so call sites outside
/// this module (e.g. the doctor summary's "Language" row) can report which
/// locale the resolver selected without duplicating the env/config probe order.
pub fn active_locale() -> Locale {
    current_locale()
}

fn resolve_locale() -> Locale {
    if let Some(loc) = std::env::var("BHARATCODE_LANG")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        return normalize_locale(&loc);
    }

    if let Some(loc) = goose::config::Config::global()
        .get_param::<String>("bharatcode_lang")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        return normalize_locale(&loc);
    }

    if let Some(loc) = std::env::var("LANG").ok().filter(|s| !s.trim().is_empty()) {
        return normalize_locale(&loc);
    }

    Locale::En
}

/// Translate `key` into the active locale, falling back to English, then to the
/// key itself if no translation exists.
pub fn t(key: &str) -> String {
    translate_in(current_locale(), key)
}

/// Translate `key` in an explicit `locale`, falling back to English, then to the
/// key itself. Kept separate from [`t`] so the locale can be forced in tests
/// without relying on the process-cached [`current_locale`].
pub(crate) fn translate_in(locale: Locale, key: &str) -> String {
    let table = match locale {
        Locale::En => &*EN,
        Locale::Hi => &*HI,
        Locale::Ta => &*TA,
    };
    if let Some(value) = table.get(key) {
        return value.clone();
    }
    if let Some(value) = EN.get(key) {
        return value.clone();
    }
    key.to_string()
}

/// Translate a key through the active locale table.
///
/// Returns an owned `String`, which keeps lifetimes trivial at `style(...)` and
/// `render_error(&...)` call sites.
#[macro_export]
macro_rules! tr {
    ($key:expr) => {
        $crate::i18n::t($key)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_locale_maps_hindi_variants() {
        assert_eq!(normalize_locale("hi"), Locale::Hi);
        assert_eq!(normalize_locale("HI"), Locale::Hi);
        assert_eq!(normalize_locale("hi_IN"), Locale::Hi);
        assert_eq!(normalize_locale("hi-IN"), Locale::Hi);
        assert_eq!(normalize_locale("hi_IN.UTF-8"), Locale::Hi);
        assert_eq!(normalize_locale("  hi  "), Locale::Hi);
    }

    #[test]
    fn normalize_locale_maps_tamil_variants() {
        assert_eq!(normalize_locale("ta"), Locale::Ta);
        assert_eq!(normalize_locale("TA"), Locale::Ta);
        assert_eq!(normalize_locale("ta_IN"), Locale::Ta);
        assert_eq!(normalize_locale("ta-IN"), Locale::Ta);
        assert_eq!(normalize_locale("ta_IN.UTF-8"), Locale::Ta);
        assert_eq!(normalize_locale("  ta  "), Locale::Ta);
    }

    #[test]
    fn normalize_locale_defaults_to_english() {
        assert_eq!(normalize_locale("en"), Locale::En);
        assert_eq!(normalize_locale("en_US"), Locale::En);
        assert_eq!(normalize_locale("en_US.UTF-8"), Locale::En);
        assert_eq!(normalize_locale(""), Locale::En);
        assert_eq!(normalize_locale("fr_FR"), Locale::En);
        assert_eq!(normalize_locale("hindi"), Locale::En);
    }

    #[test]
    fn english_table_holds_exact_strings() {
        assert_eq!(
            EN.get("session.ready").map(String::as_str),
            Some("bharatcode is ready")
        );
        assert_eq!(
            EN.get("error.no_provider").map(String::as_str),
            Some("No provider configured. Run 'bharatcode configure' first.")
        );
    }

    #[test]
    fn tri_locale_tables_cover_all_english_keys() {
        for key in EN.keys() {
            assert!(
                HI.contains_key(key),
                "hi.json is missing key present in en.json: {key}"
            );
            assert!(
                TA.contains_key(key),
                "ta.json is missing key present in en.json: {key}"
            );
        }
    }

    #[test]
    fn tamil_translation_differs_from_english() {
        let key = "cost.title";
        let en = translate_in(Locale::En, key);
        let ta = translate_in(Locale::Ta, key);
        assert_eq!(en, "BharatCode cost ledger (INR)");
        assert_ne!(
            ta, en,
            "ta.json must provide a real Tamil translation for {key}, not the English string"
        );
        assert_eq!(translate_in(Locale::Ta, key), TA[key]);
    }

    #[test]
    fn unknown_key_returns_key_itself() {
        assert_eq!(t("this.key.does.not.exist"), "this.key.does.not.exist");
    }
}
