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

use std::collections::HashMap;
use std::sync::{LazyLock, OnceLock};

/// Supported locales for the starter i18n table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    En,
    Hi,
}

/// Map a raw locale token (env var or config value) to a [`Locale`].
///
/// Lowercases the input and splits on the first locale separator so values like
/// `hi_IN.UTF-8`, `hi-IN` or `HI` all resolve to [`Locale::Hi`]. Anything that
/// is not recognized as Hindi falls back to [`Locale::En`].
fn normalize_locale(raw: &str) -> Locale {
    let lowered = raw.trim().to_ascii_lowercase();
    let primary = lowered
        .split(|c| c == '_' || c == '-' || c == '.')
        .next()
        .unwrap_or("");
    match primary {
        "hi" => Locale::Hi,
        _ => Locale::En,
    }
}

static EN: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("en.json")).expect("i18n: en.json is not valid JSON")
});

static HI: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("hi.json")).expect("i18n: hi.json is not valid JSON")
});

static CURRENT: OnceLock<Locale> = OnceLock::new();

/// Resolve (and cache) the active locale for this process.
///
/// Cached in a [`OnceLock`], so the locale is resolved exactly once per process.
fn current_locale() -> Locale {
    *CURRENT.get_or_init(resolve_locale)
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
    let table = match current_locale() {
        Locale::En => &*EN,
        Locale::Hi => &*HI,
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
    fn hindi_table_covers_all_english_keys() {
        for key in EN.keys() {
            assert!(
                HI.contains_key(key),
                "hi.json is missing key present in en.json: {key}"
            );
        }
    }

    #[test]
    fn unknown_key_returns_key_itself() {
        assert_eq!(t("this.key.does.not.exist"), "this.key.does.not.exist");
    }
}
