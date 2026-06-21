//! Marathi (`mr`) locale pack for the BharatCode CLI i18n scaffold
//! (BharatCode v81).
//!
//! The shared resolver in [`crate::i18n`] (the `Locale` enum, the embedded
//! `en.json` / `hi.json` / `ta.json` tables and the `normalize_locale` arms) is
//! frozen and is NOT edited by this version. Marathi is therefore added as a
//! self-contained locale table layered *beside* that scaffold: this module
//! embeds `mr.json` and exposes a single [`lookup`] entry point that the
//! onboarding / help label fallback chain consults before it falls back to the
//! English string baked into `en.json`.
//!
//! Wiring: this file is pulled into the CLI crate the same way the sibling
//! `ta_locale` is — through a `#[path = "../i18n/mr_locale.rs"] mod mr_locale;`
//! declaration at the first-time-setup call site — and the binary calls
//! [`active_lang_name`] to render the resolved interface-language name. Because
//! [`lookup`] returns `None` for any key the Marathi table does not carry, every
//! caller transparently falls back to English, so the default (Marathi-off)
//! output is byte-for-byte unchanged.
//!
//! Activating Marathi is purely opt-in: set `BHARATCODE_LANG=mr` (or
//! `bharatcode_lang=mr` in config, or `LANG=mr_IN`). With any other locale the
//! English / Hindi / Tamil output is unchanged because [`is_marathi_active`]
//! only reports `true` once the active locale token normalizes to Marathi.

use std::collections::HashMap;
use std::sync::LazyLock;

/// i18n key prefix for the human-readable display name of each locale, shared
/// with the rest of the i18n scaffold.
const LANG_NAME_PREFIX: &str = "lang.name.";

/// Stable BCP-47-ish primary tag for the Marathi locale.
const MR_TAG: &str = "mr";

/// The embedded Marathi locale table, parsed once from `mr.json`.
///
/// Mirrors every key in `en.json` (enforced by [`tests::mr_covers_all_en_keys`])
/// so any user-facing string routed through [`lookup`] has a Marathi rendering.
static MR: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("mr.json")).expect("i18n: mr.json is not valid JSON")
});

/// Look up the Marathi translation for `key`.
///
/// Returns `Some(marathi_string)` when the Marathi table carries `key`, and
/// `None` otherwise so the caller can fall back to the English baseline. This is
/// the real call site through which the onboarding / help label chain routes
/// Marathi without the shared `i18n/mod.rs` resolver having to learn a new
/// `Locale` arm.
pub fn lookup(key: &str) -> Option<String> {
    MR.get(key).filter(|v| !v.trim().is_empty()).cloned()
}

/// Normalize a raw locale token (env var / config value) and report whether it
/// selects Marathi.
///
/// Splits on the first locale separator so `mr`, `MR`, `mr-IN`, `mr_IN` and
/// `mr_IN.UTF-8` all map to Marathi, matching the separator handling used by the
/// shared resolver.
fn token_is_marathi(raw: &str) -> bool {
    raw.trim()
        .to_ascii_lowercase()
        .split(|c| c == '_' || c == '-' || c == '.')
        .next()
        .map(|primary| primary == MR_TAG)
        .unwrap_or(false)
}

/// Whether Marathi is the active interface locale for this process.
///
/// Probes the same opt-in sources as the shared resolver, in the same order:
/// `BHARATCODE_LANG`, then the `bharatcode_lang` config parameter, then `LANG`.
/// Returns `false` (English/Hindi/Tamil behaviour unchanged) unless one of those
/// resolves to a Marathi token.
pub fn is_marathi_active() -> bool {
    if let Some(loc) = std::env::var("BHARATCODE_LANG")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        return token_is_marathi(&loc);
    }

    if let Some(loc) = bharatcode_core::config::Config::global()
        .get_param::<String>("bharatcode_lang")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        return token_is_marathi(&loc);
    }

    if let Some(loc) = std::env::var("LANG").ok().filter(|s| !s.trim().is_empty()) {
        return token_is_marathi(&loc);
    }

    false
}

/// The display name of the active interface language, rendered in its own
/// script.
///
/// This is the real, wired call site for the Marathi locale pack: the
/// first-time-setup banner prints it as the "Language" row. When Marathi is
/// active it resolves the `lang.name.mr` key (`मराठी`) from the embedded table;
/// otherwise it defers to the shared scaffold via [`crate::i18n::active_locale`]
/// + [`crate::tr!`], so English/Hindi/Tamil output is unchanged.
pub fn active_lang_name() -> String {
    if is_marathi_active() {
        if let Some(name) = lookup(&format!("{LANG_NAME_PREFIX}{MR_TAG}")) {
            return name;
        }
    }

    let tag = match crate::i18n::active_locale() {
        crate::i18n::Locale::En => "en",
        crate::i18n::Locale::Hi => "hi",
        crate::i18n::Locale::Ta => "ta",
    };
    crate::tr!(&format!("{LANG_NAME_PREFIX}{tag}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(raw: &str) -> HashMap<String, String> {
        serde_json::from_str(raw).expect("i18n: locale table is not valid JSON")
    }

    fn en() -> HashMap<String, String> {
        parse(include_str!("en.json"))
    }

    /// Every key in the source-of-truth `en.json` must exist in `mr.json` with a
    /// non-empty Marathi value, and a sentinel key (`cost.title`) must differ
    /// from the English string so the table is a real translation, not an echo.
    #[test]
    fn mr_covers_all_en_keys() {
        let en = en();
        for key in en.keys() {
            let mr_value = MR
                .get(key)
                .unwrap_or_else(|| panic!("mr.json is missing key present in en.json: {key}"));
            assert!(
                !mr_value.trim().is_empty(),
                "mr.json has an empty value for key: {key}"
            );
        }

        let sentinel = "cost.title";
        assert_eq!(
            en.get(sentinel).map(String::as_str),
            Some("BharatCode cost ledger (INR)"),
            "en.json sentinel changed; update this assertion"
        );
        assert_ne!(
            MR.get(sentinel),
            en.get(sentinel),
            "mr.json must provide a real Marathi translation for {sentinel}, not the English string"
        );
    }

    /// The Marathi table must not introduce keys the English baseline does not
    /// define, so the two tables stay in lockstep in both directions.
    #[test]
    fn mr_has_no_keys_beyond_english() {
        let en = en();
        for key in MR.keys() {
            assert!(
                en.contains_key(key),
                "mr.json carries a key absent from en.json: {key}"
            );
        }
    }

    /// A `normalize`-style `mr_IN.UTF-8` token must route through the Marathi
    /// table to a non-empty Marathi string, proving the locale is genuinely
    /// selectable and not just a parked file.
    #[test]
    fn mr_in_utf8_maps_to_marathi_string() {
        assert!(token_is_marathi("mr"));
        assert!(token_is_marathi("MR"));
        assert!(token_is_marathi("mr-IN"));
        assert!(token_is_marathi("mr_IN"));
        assert!(token_is_marathi("mr_IN.UTF-8"));
        assert!(token_is_marathi("  mr  "));

        assert!(!token_is_marathi("en"));
        assert!(!token_is_marathi("hi_IN"));
        assert!(!token_is_marathi("ta_IN.UTF-8"));
        assert!(!token_is_marathi("marathi"));

        let name = lookup(&format!("{LANG_NAME_PREFIX}{MR_TAG}"))
            .expect("mr.json must carry the Marathi display name");
        assert!(!name.trim().is_empty());
        assert_eq!(name, "मराठी");
    }

    /// `lookup` returns the Marathi value for a known key and `None` for an
    /// unknown one so callers fall back to English.
    #[test]
    fn lookup_returns_marathi_or_none() {
        assert_eq!(lookup("session.ready"), Some(MR["session.ready"].clone()));
        assert_ne!(lookup("session.ready"), Some(en()["session.ready"].clone()));
        assert_eq!(lookup("this.key.does.not.exist"), None);
    }

    /// The `locale.mr_name` display key the shared `en.json` edit adds is carried
    /// by the Marathi table too (it is an ASCII endonym-in-English, deliberately
    /// identical across tables).
    #[test]
    fn locale_mr_name_present() {
        assert_eq!(lookup("locale.mr_name").as_deref(), Some("Marathi"));
        assert_eq!(
            en().get("locale.mr_name").map(String::as_str),
            Some("Marathi")
        );
    }
}
