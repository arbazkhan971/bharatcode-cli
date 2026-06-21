//! Doctor i18n + accessibility readiness check — BharatCode v86.
//!
//! A single, read-only diagnostic for the wave's UX/i18n surface. It answers, at
//! a glance, "is my localized + accessible BharatCode actually wired the way I
//! think it is?" by reporting:
//!
//!   1. **Active locale** — resolved with the same precedence as the i18n
//!      scaffold (`BHARATCODE_LANG` -> `bharatcode_lang` config -> `LANG` ->
//!      English), so the row names the locale CLI strings will actually render in
//!      (English / Hindi / Tamil).
//!   2. **Three-way translation parity** — whether the Hindi and Tamil tables
//!      cover every key in the English base table (an `en == hi == ta` key-count
//!      match). A locale that lags behind English silently falls back to English
//!      at runtime, so a parity gap is worth surfacing here rather than
//!      discovering it mid-session.
//!   3. **Plain / no-color accessibility mode** — whether the screen-reader-
//!      friendly plain-text mode is active (`BHARATCODE_A11Y`, or the `NO_COLOR`
//!      convention), so the operator can confirm the accessibility behaviour they
//!      expect is the behaviour actually enabled.
//!   4. **Terminal-color hint** — whether `NO_COLOR` is set (forcing the plain
//!      theme) or colored output is available, so the rendered styling is never a
//!      surprise.
//!
//! The check is strictly read-only and side-effect free: it loads the *embedded*
//! locale tables (compiled into the binary via `include_str!`, so they always
//! reflect the shipped translations) and reads a handful of environment values.
//! It never writes config, never mutates the locale, and never shells out. It is
//! always non-fatal — the worst it returns is a [`Status::Warn`].
//!
//! The en/hi/ta tables are the exact `i18n/en.json` / `i18n/hi.json` /
//! `i18n/ta.json` shipped with the CLI scaffold, so the parity figure reported
//! here is the real shipped parity rather than a synthesized one. Shaped exactly
//! like `index_check::index_readiness`: returns a `(Status, String)` the doctor
//! paints with the matching glyph.

use std::collections::BTreeMap;

use crate::commands::doctor_checks::Status;

/// Environment / config key selecting the active locale (highest precedence).
const LANG_KEY: &str = "BHARATCODE_LANG";
/// Config-layer key the i18n scaffold consults after the env var.
const LANG_CONFIG_KEY: &str = "bharatcode_lang";
/// Standard POSIX locale variable, consulted last before falling back to English.
const POSIX_LANG_KEY: &str = "LANG";

/// Accessibility (screen-reader-friendly / plain-text output) opt-in.
const A11Y_KEY: &str = "BHARATCODE_A11Y";
/// The `NO_COLOR` convention (https://no-color.org): when present, color is
/// suppressed and plain-text output is preferred. Read-only here.
const NO_COLOR_KEY: &str = "NO_COLOR";

/// Embedded English base table — the canonical key set every locale is measured
/// against. Compiled in via `include_str!`, so it always reflects the shipped
/// `i18n/en.json`.
const EN_JSON: &str = include_str!("../i18n/en.json");
/// Embedded Hindi table, in parity with `en.json` by the i18n scaffold's own
/// invariant test.
const HI_JSON: &str = include_str!("../i18n/hi.json");
/// Embedded Tamil table, in parity with `en.json` by the i18n scaffold's own
/// invariant test.
const TA_JSON: &str = include_str!("../i18n/ta.json");

/// The three locales whose readiness this check reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Locale {
    En,
    Hi,
    Ta,
}

impl Locale {
    /// Short BCP-47-ish code used in the rendered row.
    fn code(self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::Hi => "hi",
            Locale::Ta => "ta",
        }
    }

    /// English display name (also the i18n key suffix `lang.name.<code>`).
    fn english_name(self) -> &'static str {
        match self {
            Locale::En => "English",
            Locale::Hi => "Hindi",
            Locale::Ta => "Tamil",
        }
    }
}

/// Interpret a raw flag value as truthy. Mirrors the truthy spellings the a11y
/// gate accepts; anything not clearly "on" is off, so a typo never silently flips
/// the report into a11y mode.
fn flag_is_on(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "on" | "yes"
    )
}

/// Whether the plain-text / screen-reader accessibility mode is active.
///
/// Enabled when `BHARATCODE_A11Y` holds a truthy value, or when `NO_COLOR` is
/// present (the `NO_COLOR` convention implies a plain-text preference). This is
/// the same condition `crate::a11y::is_enabled` uses, recomputed locally so the
/// check has no ordering dependency on that module's process-cached state.
fn a11y_is_active() -> bool {
    if let Ok(raw) = std::env::var(A11Y_KEY) {
        if flag_is_on(&raw) {
            return true;
        }
    }
    no_color_is_set()
}

/// Whether `NO_COLOR` is present in the environment. Mirrors the `NO_COLOR`
/// convention used by `theme::resolve_theme`: presence (even empty) signals a
/// preference, though the theme layer only forces plain on a non-empty value.
fn no_color_is_set() -> bool {
    std::env::var_os(NO_COLOR_KEY).is_some()
}

/// Map a raw locale token (env var or config value) to a [`Locale`], mirroring
/// the i18n scaffold's normalization (lowercase, split on the first separator).
/// Tamil (`ta`) and Hindi (`hi`) are recognized; anything else is English.
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

/// Resolve the active locale using the same precedence as the i18n scaffold:
/// `BHARATCODE_LANG` -> `bharatcode_lang` config -> `LANG` -> English.
fn active_locale() -> Locale {
    if let Some(raw) = std::env::var(LANG_KEY)
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        return normalize_locale(&raw);
    }
    if let Some(raw) = bharatcode_core::config::Config::global()
        .get_param::<String>(LANG_CONFIG_KEY)
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        return normalize_locale(&raw);
    }
    if let Some(raw) = std::env::var(POSIX_LANG_KEY)
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        return normalize_locale(&raw);
    }
    Locale::En
}

/// Parse an embedded locale JSON table into a key->value map. The shipped tables
/// are validated by the i18n scaffold's own tests, so a parse failure here would
/// be a build-time data error; we degrade to an empty map rather than panic so a
/// diagnostic can never take the whole `doctor` run down.
fn parse_table(json: &str) -> BTreeMap<String, String> {
    serde_json::from_str(json).unwrap_or_default()
}

/// Count how many of `base`'s keys are present in `other`.
fn covered_keys(base: &BTreeMap<String, String>, other: &BTreeMap<String, String>) -> usize {
    base.keys().filter(|k| other.contains_key(*k)).count()
}

/// Look up a user-facing string through the i18n layer, falling back to the
/// English `default` when the active locale table has no entry for `key`.
///
/// `t()` echoes the key back when it is missing, so an unchanged key is treated as
/// "untranslated". Mirrors the helper in `doctor.rs` / `index_check.rs` so the row
/// renders in English without depending on the i18n table being populated.
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// Render the on/off state of a toggle as a localized word.
fn on_off(on: bool) -> String {
    if on {
        label("doctor.on", "on")
    } else {
        label("doctor.off", "off")
    }
}

/// Report i18n + accessibility readiness for the UX/i18n wave.
///
/// Returns a [`Status`] plus a human-readable message naming the active locale,
/// the three-way en/hi/ta key-parity status, whether plain/no-color a11y mode is
/// active, and a `NO_COLOR`/terminal-color hint. The result is always non-fatal:
///
/// * [`Status::Ok`] — Hindi and Tamil cover every English key (full three-way
///   parity) and plain/no-color accessibility mode is not active.
/// * [`Status::Warn`] — a locale lags behind English (some keys fall back to
///   English at runtime), or plain/no-color accessibility mode is active —
///   surfaced so the operator can confirm the behaviour change is intended.
pub fn i18n_readiness() -> (Status, String) {
    let lbl = label("doctor.check.i18n_readiness", "i18n / accessibility");

    let en = parse_table(EN_JSON);
    let hi = parse_table(HI_JSON);
    let ta = parse_table(TA_JSON);

    let total = en.len();
    let hi_covered = covered_keys(&en, &hi);
    let ta_covered = covered_keys(&en, &ta);
    // Three-way parity: every English key is present in both hi and ta, and the
    // tables are the same size (no extra keys on either side either).
    let parity = total > 0
        && hi_covered == total
        && ta_covered == total
        && hi.len() == total
        && ta.len() == total;

    let locale = active_locale();
    let locale_name = label(
        &format!("lang.name.{}", locale.code()),
        locale.english_name(),
    );

    let a11y_on = a11y_is_active();
    let no_color_on = no_color_is_set();

    // Core descriptor: which locale is active, the three-way parity figure, the
    // plain-text accessibility state, and the terminal-color hint — so the single
    // row is self-explanatory.
    let locale_word = label("doctor.check.i18n_locale", "locale");
    let parity_word = label("doctor.check.i18n_parity", "en/hi/ta parity");
    let a11y_word = label("doctor.check.i18n_a11y", "plain/no-color a11y");
    let color_word = label("doctor.check.i18n_color", "color");

    // Hindi long-tail depth (v82): how many of the deepened onboarding / help-index
    // / a11y / tutorials / dashboard / notify keys carry a genuine Devanagari value
    // in the shipped hi.json. Surfaced here so the doctor i18n row reports Hindi
    // depth, not just bare key parity.
    let hindi_word = label("doctor.check.i18n_hindi_depth", "hi depth");
    let (hindi_translated, hindi_total) = crate::i18n::hindi_coverage();

    let parity_state = if parity {
        format!("{}/{}/{}", total, total, total)
    } else {
        format!("{}/{}/{} en keys", total, hi_covered, ta_covered)
    };

    // The terminal-color hint distinguishes "NO_COLOR forces plain" from the
    // default "colored output available" so the styling shown is never a surprise.
    let color_state = if no_color_on {
        label("doctor.check.i18n_no_color", "NO_COLOR set (plain)")
    } else {
        label("doctor.check.i18n_color_on", "available")
    };

    let core = format!(
        "{} {} ({}) — {} {} — {} {}/{} — {} {} — {} {}",
        locale_word,
        locale.code(),
        locale_name,
        parity_word,
        parity_state,
        hindi_word,
        hindi_translated,
        hindi_total,
        a11y_word,
        on_off(a11y_on),
        color_word,
        color_state,
    );

    if !parity {
        let hint = label(
            "doctor.check.i18n_parity_gap",
            "a locale lags behind English; missing keys fall back to English at runtime",
        );
        return (Status::Warn, format!("{} ({}; {})", lbl, core, hint));
    }

    if a11y_on {
        let hint = label(
            "doctor.check.i18n_a11y_on",
            "plain-text accessibility output is enabled for this session",
        );
        return (Status::Warn, format!("{} ({}; {})", lbl, core, hint));
    }

    (Status::Ok, format!("{} ({})", lbl, core))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serializes the env-mutating tests so one test's toggles never race
    /// another's.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Clear every env var this check reads, so a test starts from a known
    /// default-OFF, English-locale baseline regardless of the host environment.
    fn clear_env() {
        for key in [LANG_KEY, POSIX_LANG_KEY, A11Y_KEY, NO_COLOR_KEY] {
            std::env::remove_var(key);
        }
    }

    #[test]
    fn flag_parsing_only_accepts_clear_on_values() {
        assert!(flag_is_on("1"));
        assert!(flag_is_on(" TRUE "));
        assert!(flag_is_on("on"));
        assert!(flag_is_on("yes"));
        assert!(!flag_is_on("0"));
        assert!(!flag_is_on(""));
        assert!(!flag_is_on("maybe"));
    }

    #[test]
    fn normalize_locale_recognizes_three_locales() {
        assert_eq!(normalize_locale("en_US.UTF-8"), Locale::En);
        assert_eq!(normalize_locale("hi"), Locale::Hi);
        assert_eq!(normalize_locale("hi-IN"), Locale::Hi);
        assert_eq!(normalize_locale("ta"), Locale::Ta);
        assert_eq!(normalize_locale("TA_IN.UTF-8"), Locale::Ta);
        assert_eq!(normalize_locale("ta-IN"), Locale::Ta);
        assert_eq!(normalize_locale("fr_FR"), Locale::En);
        assert_eq!(normalize_locale(""), Locale::En);
    }

    #[test]
    fn embedded_tables_hold_a_three_way_parity() {
        // The embedded en/hi/ta tables must have matching key counts: hi and ta
        // each cover every English key (en == hi == ta). This is the parity the
        // doctor row reports as ✓.
        let en = parse_table(EN_JSON);
        let hi = parse_table(HI_JSON);
        let ta = parse_table(TA_JSON);

        assert!(
            !en.is_empty(),
            "embedded en.json must parse to a non-empty map"
        );
        assert_eq!(
            covered_keys(&en, &hi),
            en.len(),
            "hi.json must cover every en.json key"
        );
        assert_eq!(
            covered_keys(&en, &ta),
            en.len(),
            "ta.json must cover every en.json key"
        );
        // Three-way count match: en == hi == ta.
        assert_eq!(en.len(), hi.len());
        assert_eq!(en.len(), ta.len());
    }

    #[test]
    fn readiness_message_is_non_empty_and_names_the_locale() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();

        let (_status, msg) = i18n_readiness();
        assert!(!msg.is_empty(), "the readiness message must be non-empty");
        // Default baseline names English as the active locale.
        assert!(
            msg.contains("en"),
            "expected the active locale code in: {msg}"
        );

        clear_env();
    }

    #[test]
    fn readiness_is_ok_when_parity_holds_and_a11y_off() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();

        // With all three bundled tables present, parity is ✓ and, with no a11y /
        // no-color toggle set, the row is OK.
        let (status, msg) = i18n_readiness();
        assert_eq!(status, Status::Ok, "msg: {msg}");
        assert_eq!(Status::Ok.glyph(), "\u{2713}", "OK maps to the ✓ glyph");

        clear_env();
    }

    #[test]
    fn readiness_warns_and_reports_when_a11y_is_on() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        std::env::set_var(A11Y_KEY, "1");

        let (status, msg) = i18n_readiness();
        assert_eq!(status, Status::Warn, "msg: {msg}");
        // The message must report the plain/no-color a11y toggle as on.
        assert!(msg.contains("a11y"), "expected an a11y mention in: {msg}");
        assert!(msg.contains("on"), "expected an on-state in: {msg}");

        clear_env();
    }

    #[test]
    fn readiness_reports_no_color_state() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        std::env::set_var(NO_COLOR_KEY, "1");

        // NO_COLOR both forces plain-text a11y mode and is named in the
        // terminal-color hint.
        let (status, msg) = i18n_readiness();
        assert_eq!(status, Status::Warn, "msg: {msg}");
        assert!(
            msg.contains("NO_COLOR"),
            "expected a NO_COLOR mention in: {msg}"
        );

        clear_env();
    }

    #[test]
    fn message_names_the_active_locale() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        std::env::set_var(LANG_KEY, "ta_IN.UTF-8");

        let (_status, msg) = i18n_readiness();
        // Active Tamil locale is named by its code.
        assert!(msg.contains("ta"), "expected the ta locale code in: {msg}");

        clear_env();
    }

    #[test]
    fn message_has_no_upstream_branding_leak() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        let (_status, msg) = i18n_readiness();
        let lowered = msg.to_ascii_lowercase();
        assert!(!lowered.contains("goose"), "branding leak in: {msg}");
        assert!(!lowered.contains("block"), "branding leak in: {msg}");
        clear_env();
    }

    #[test]
    fn status_glyph_mapping_is_distinct() {
        // The doctor row maps Ok/Warn/Fail to distinct glyphs.
        assert_eq!(Status::Ok.glyph(), "\u{2713}");
        assert_eq!(Status::Warn.glyph(), "\u{26a0}");
        assert_eq!(Status::Fail.glyph(), "\u{2717}");
        assert_ne!(Status::Ok.glyph(), Status::Warn.glyph());
        assert_ne!(Status::Warn.glyph(), Status::Fail.glyph());
    }
}
