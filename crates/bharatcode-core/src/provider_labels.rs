//! Localized provider/model picker labels (BharatCode v88).
//!
//! The onboarding / `configure` provider picker and the planner-preset surface
//! normally render raw provider ids (`ollama`, `sarvam`, `krutrim`, ...). For a
//! non-English audience that is opaque. This module turns a provider id into a
//! friendly, India-context display label in the active regional language, plus
//! an optional one-line residency hint (`India-hosted` / `local / offline`).
//!
//! Design constraints:
//!   * **Pure & table-driven.** [`display_label`] and [`residency_hint`] are
//!     pure functions over a static table; no I/O, no allocation beyond the
//!     returned `String`. This keeps them trivially testable and safe to call
//!     from any picker render loop.
//!   * **Additive & non-breaking.** Unknown providers fall back to the raw id,
//!     so nothing breaks when a new provider appears that this table has not
//!     learned about yet.
//!   * **English is unchanged.** The `en` locale path returns the raw id
//!     (matching the existing raw-id-with-default picker behavior). Only when a
//!     regional locale is active (via `BHARATCODE_LANG` → `bharatcode_lang`
//!     config → `LANG`) does the picker switch to localized names. Default
//!     behavior is therefore byte-for-byte unchanged.
//!
//! Locale resolution mirrors the project's existing scaffold used by
//! `desktop_notify` / `verify` (`BHARATCODE_LANG` env → `bharatcode_lang`
//! config → `LANG` env → English) so the picker honors the same switch as the
//! rest of the localized surfaces.
//!
//! Original BharatCode work; not ported from any third party.

/// A localized name for a single provider, keyed by locale.
///
/// `hi` / `ta` / `mr` hold the Hindi / Tamil / Marathi display names. Any of
/// them may be empty (`""`), in which case [`display_label`] falls back to the
/// raw provider id for that locale — so a partially-translated row never emits
/// a blank label.
#[derive(Debug, Clone, Copy)]
struct LabelRow {
    /// Canonical provider id (matches the declarative provider `name`).
    id: &'static str,
    /// Hindi display name (Devanagari).
    hi: &'static str,
    /// Tamil display name.
    ta: &'static str,
    /// Marathi display name (Devanagari).
    mr: &'static str,
    /// Residency hint, if this provider has a meaningful one.
    residency: Option<&'static str>,
}

/// India-hosted residency hint.
const HINT_INDIA: &str = "India-hosted";
/// Local / offline residency hint (no network egress).
const HINT_LOCAL: &str = "local / offline";

/// Curated provider label table.
///
/// Biased toward the India-context providers the rest of BharatCode surfaces
/// (Sarvam, Krutrim) and the local/offline runners (Ollama, LM Studio). Display
/// names use the provider's own brand transliterated into each script; the
/// residency hint mirrors the declarative provider descriptions
/// (`India-hosted ...`) and the planner-preset notes.
const LABELS: &[LabelRow] = &[
    LabelRow {
        id: "sarvam",
        hi: "सर्वम् एआई (भारत)",
        ta: "சர்வம் AI (இந்தியா)",
        mr: "सर्वम् एआय (भारत)",
        residency: Some(HINT_INDIA),
    },
    LabelRow {
        id: "krutrim",
        hi: "क्रुट्रिम (ओला, भारत)",
        ta: "க்ருட்ரிம் (ஓலா, இந்தியா)",
        mr: "क्रुट्रिम (ओला, भारत)",
        residency: Some(HINT_INDIA),
    },
    LabelRow {
        id: "ollama",
        hi: "ओलामा (स्थानीय / ऑफ़लाइन)",
        ta: "ஒல்லாமா (அக / ஆஃப்லைன்)",
        mr: "ओलामा (स्थानिक / ऑफलाइन)",
        residency: Some(HINT_LOCAL),
    },
    LabelRow {
        id: "lmstudio",
        hi: "एलएम स्टूडियो (स्थानीय / ऑफ़लाइन)",
        ta: "எல்எம் ஸ்டுடியோ (அக / ஆஃப்லைன்)",
        mr: "एलएम स्टुडिओ (स्थानिक / ऑफलाइन)",
        residency: Some(HINT_LOCAL),
    },
    LabelRow {
        id: "llama_swap",
        hi: "लामा-स्वैप (स्थानीय / ऑफ़लाइन)",
        ta: "லாமா-ஸ்வாப் (அக / ஆஃப்லைன்)",
        mr: "लामा-स्वॅप (स्थानिक / ऑफलाइन)",
        residency: Some(HINT_LOCAL),
    },
    LabelRow {
        id: "openai",
        hi: "ओपनएआई",
        ta: "ஓப்பன்ஏஐ",
        mr: "ओपनएआय",
        residency: None,
    },
    LabelRow {
        id: "anthropic",
        hi: "एंथ्रोपिक",
        ta: "ஆந்த்ரோபிக்",
        mr: "अँथ्रोपिक",
        residency: None,
    },
    LabelRow {
        id: "google",
        hi: "गूगल (जेमिनी)",
        ta: "கூகுள் (ஜெமினி)",
        mr: "गूगल (जेमिनी)",
        residency: None,
    },
    LabelRow {
        id: "groq",
        hi: "ग्रोक",
        ta: "க்ரோக்",
        mr: "ग्रोक",
        residency: None,
    },
    LabelRow {
        id: "deepseek",
        hi: "डीपसीक",
        ta: "டீப்சீக்",
        mr: "डीपसीक",
        residency: None,
    },
    LabelRow {
        id: "mistral",
        hi: "मिस्ट्रल",
        ta: "மிஸ்ட்ரல்",
        mr: "मिस्ट्रल",
        residency: None,
    },
];

/// Supported display locales for the picker labels.
///
/// Kept private and self-contained (mirroring `desktop_notify` / `verify`) so
/// this module has no cross-module locale dependency; the resolution *order* is
/// identical to the shared scaffold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Locale {
    En,
    Hi,
    Ta,
    Mr,
}

/// Map a raw locale token to a [`Locale`].
///
/// Lowercases and splits on the first locale separator so `hi_IN.UTF-8`,
/// `ta-IN`, `MR`, etc. resolve correctly. Anything unrecognized is English.
fn normalize_locale(raw: &str) -> Locale {
    let lowered = raw.trim().to_ascii_lowercase();
    let primary = lowered.split(['_', '-', '.']).next().unwrap_or("");
    match primary {
        "hi" => Locale::Hi,
        "ta" => Locale::Ta,
        "mr" => Locale::Mr,
        _ => Locale::En,
    }
}

/// Resolve the active picker locale (`BHARATCODE_LANG` → `bharatcode_lang`
/// config → `LANG` → English), mirroring the project's existing scaffold.
fn resolve_locale() -> Locale {
    if let Some(loc) = std::env::var("BHARATCODE_LANG")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        return normalize_locale(&loc);
    }
    if let Some(loc) = crate::config::Config::global()
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

/// Look up the label row for a provider id (case-insensitive on the id).
fn lookup(provider_id: &str) -> Option<&'static LabelRow> {
    LABELS
        .iter()
        .find(|row| row.id.eq_ignore_ascii_case(provider_id))
}

/// Pick the localized name for `row` in `locale`, falling back to the raw id
/// when that locale's cell is empty (partial translation guard).
fn localized_name(row: &LabelRow, locale: Locale, provider_id: &str) -> String {
    let candidate = match locale {
        Locale::En => "",
        Locale::Hi => row.hi,
        Locale::Ta => row.ta,
        Locale::Mr => row.mr,
    };
    if candidate.trim().is_empty() {
        provider_id.to_string()
    } else {
        candidate.to_string()
    }
}

/// Render the display label for `provider_id` in the given `locale` string.
///
/// * `en` (or any unsupported locale) returns the raw provider id unchanged,
///   preserving the existing picker behavior.
/// * A supported regional locale (`hi`, `ta`, `mr`, including variants like
///   `hi_IN.UTF-8`) returns the localized display name. When a residency hint
///   exists it is appended in parentheses so the picker row reads, e.g.,
///   `सर्वम् एआई (भारत) — India-hosted`.
/// * Unknown providers fall back to the raw id so nothing breaks.
///
/// Pure: the `locale` is taken as an argument (no env read), so the function is
/// fully deterministic and unit-testable. Call sites that want the *active*
/// process locale should use [`display_label_active`].
pub fn display_label(provider_id: &str, locale: &str) -> String {
    let loc = normalize_locale(locale);
    if loc == Locale::En {
        return provider_id.to_string();
    }
    let Some(row) = lookup(provider_id) else {
        return provider_id.to_string();
    };
    let name = localized_name(row, loc, provider_id);
    match row.residency {
        Some(hint) => format!("{name} — {hint}"),
        None => name,
    }
}

/// Residency hint for a provider id, if it has a meaningful one.
///
/// `sarvam` / `krutrim` → `India-hosted`; `ollama` / `lmstudio` / `llama_swap`
/// → `local / offline`. Unknown or hint-less providers return `None`. Pure and
/// locale-independent (the hint is an English-stable tag the picker can colorize
/// or further localize at the call site).
pub fn residency_hint(provider_id: &str) -> Option<&'static str> {
    lookup(provider_id).and_then(|row| row.residency)
}

/// Convenience wrapper: render the display label for `provider_id` using the
/// *active* process locale (`BHARATCODE_LANG` → config → `LANG` → English).
///
/// This is the entry point the configure picker and the planner-preset surface
/// call, so neither has to plumb the locale through itself. With no locale set
/// it returns the raw id, leaving default English output unchanged.
pub fn display_label_active(provider_id: &str) -> String {
    match resolve_locale() {
        Locale::En => provider_id.to_string(),
        Locale::Hi => display_label(provider_id, "hi"),
        Locale::Ta => display_label(provider_id, "ta"),
        Locale::Mr => display_label(provider_id, "mr"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialize env-touching tests so parallel tests never observe each
    /// other's `BHARATCODE_*` / `LANG` values (the process env is global).
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> EnvGuard {
            let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let prev = std::env::var(key).ok();
            std::env::set_var(key, value);
            EnvGuard {
                key,
                prev,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn ollama_hindi_label_is_non_empty_and_differs_from_raw_id() {
        let label = display_label("ollama", "hi");
        assert!(!label.trim().is_empty(), "label must be non-empty");
        assert_ne!(label, "ollama", "hindi label must differ from raw id");
    }

    #[test]
    fn sarvam_residency_hint_is_india_hosted() {
        assert_eq!(residency_hint("sarvam"), Some("India-hosted"));
    }

    #[test]
    fn krutrim_residency_hint_is_india_hosted() {
        assert_eq!(residency_hint("krutrim"), Some("India-hosted"));
    }

    #[test]
    fn ollama_residency_hint_is_local_offline() {
        assert_eq!(residency_hint("ollama"), Some("local / offline"));
    }

    #[test]
    fn unknown_provider_has_no_residency_hint() {
        assert_eq!(residency_hint("definitely-not-a-provider"), None);
    }

    #[test]
    fn english_returns_raw_id_unchanged() {
        assert_eq!(display_label("ollama", "en"), "ollama");
        assert_eq!(display_label("sarvam", "en"), "sarvam");
        // Unsupported locale also falls back to raw-id (English) behavior.
        assert_eq!(display_label("ollama", "fr"), "ollama");
    }

    #[test]
    fn unknown_provider_falls_back_to_raw_id_in_every_locale() {
        assert_eq!(display_label("unknown", "en"), "unknown");
        assert_eq!(display_label("unknown", "hi"), "unknown");
        assert_eq!(display_label("unknown", "ta"), "unknown");
        assert_eq!(display_label("unknown", "mr"), "unknown");
    }

    #[test]
    fn localized_label_appends_residency_hint() {
        let label = display_label("sarvam", "hi");
        assert!(
            label.contains("India-hosted"),
            "hindi sarvam label should carry the residency hint: {label}"
        );
    }

    #[test]
    fn locale_variants_normalize() {
        assert_eq!(normalize_locale("hi_IN.UTF-8"), Locale::Hi);
        assert_eq!(normalize_locale("ta-IN"), Locale::Ta);
        assert_eq!(normalize_locale("MR"), Locale::Mr);
        assert_eq!(normalize_locale("en_US"), Locale::En);
        assert_eq!(normalize_locale("fr"), Locale::En);
    }

    #[test]
    fn provider_id_lookup_is_case_insensitive() {
        // Krutrim/Sarvam declarative ids are lowercase, but a caller may pass a
        // differently-cased id; the label must still resolve.
        assert_eq!(residency_hint("OLLAMA"), Some("local / offline"));
        assert!(display_label("SARVAM", "hi").contains("India-hosted"));
    }

    #[test]
    fn active_label_defaults_to_raw_id_when_locale_unset() {
        let _lang = EnvGuard::set("BHARATCODE_LANG", "en_US.UTF-8");
        assert_eq!(display_label_active("ollama"), "ollama");
    }

    #[test]
    fn active_label_localizes_when_hindi_is_active() {
        let _lang = EnvGuard::set("BHARATCODE_LANG", "hi_IN.UTF-8");
        let label = display_label_active("sarvam");
        assert_ne!(label, "sarvam");
        assert!(label.contains("India-hosted"));
    }

    #[test]
    fn no_brand_leakage_in_labels() {
        for row in LABELS {
            for s in [row.id, row.hi, row.ta, row.mr] {
                let lower = s.to_ascii_lowercase();
                assert!(!lower.contains("goose"), "brand leak in {s}");
                assert!(!lower.contains("block"), "brand leak in {s}");
            }
        }
    }
}
