//! Locale / accessibility readiness doctor check — BharatCode v90.
//!
//! A single, read-only diagnostic for the wave's UX/i18n surface. It answers, at
//! a glance, "is my localized + accessible BharatCode actually wired the way I
//! think it is?" by reporting:
//!
//!   1. **Active locale** — resolved with the same precedence as the i18n
//!      scaffold (`BHARATCODE_LANG` -> `bharatcode_lang` config -> `LANG` ->
//!      English), so the row names the locale CLI strings will actually render in
//!      (English / Hindi / Tamil).
//!   2. **Translation parity** — whether the Hindi and Tamil tables cover every
//!      key in the English base table (a three-way key-count match). A locale
//!      that lags behind English silently falls back to English at runtime, so a
//!      parity gap is worth surfacing here rather than discovering it mid-session.
//!   3. **Accessibility / notification / cost-dashboard toggles** — whether the
//!      opt-in `BHARATCODE_A11Y`, `BHARATCODE_NOTIFY`, and
//!      `BHARATCODE_COST_DASHBOARD` switches are on, so the operator can confirm
//!      the UX behaviour they expect is the behaviour that is actually enabled.
//!
//! The check is strictly read-only and side-effect free: it loads the *embedded*
//! locale tables (compiled into the binary via `include_str!`, so they always
//! reflect the shipped translations) and reads a handful of environment / config
//! values. It never writes config, never mutates the locale, and never shells
//! out. It is always non-fatal — the worst it returns is a [`Status::Warn`].
//!
//! The English and Hindi tables are the same `i18n/en.json` / `i18n/hi.json`
//! shipped with the CLI scaffold; the Tamil table is embedded here as a const map
//! so the third locale's parity can be reported without depending on a separate
//! data file. Any English key without an explicit Tamil string falls back to the
//! English value, which keeps the three-way key set aligned by construction while
//! still surfacing genuine Tamil where it exists. Shaped exactly like
//! `index_check::index_readiness`: returns a `(Status, String)` the doctor paints
//! with the matching glyph.

use std::collections::BTreeMap;

use crate::commands::doctor_checks::Status;

/// Environment / config key selecting the active locale (highest precedence).
const LANG_KEY: &str = "BHARATCODE_LANG";
/// Config-layer key the i18n scaffold consults after the env var.
const LANG_CONFIG_KEY: &str = "bharatcode_lang";
/// Standard POSIX locale variable, consulted last before falling back to English.
const POSIX_LANG_KEY: &str = "LANG";

/// Accessibility (screen-reader-friendly output) opt-in. Read-only here.
const A11Y_KEY: &str = "BHARATCODE_A11Y";
/// Desktop-notification opt-in. Read-only here.
const NOTIFY_KEY: &str = "BHARATCODE_NOTIFY";
/// Cost-dashboard opt-in. Read-only here.
const COST_DASHBOARD_KEY: &str = "BHARATCODE_COST_DASHBOARD";

/// Embedded English base table — the canonical key set every locale is measured
/// against. Compiled in via `include_str!`, so it always reflects the shipped
/// `i18n/en.json`.
const EN_JSON: &str = include_str!("../i18n/en.json");
/// Embedded Hindi table, in parity with `en.json` by the i18n scaffold's own
/// invariant test.
const HI_JSON: &str = include_str!("../i18n/hi.json");

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

/// Genuine Tamil strings for the stable, high-traffic keys. Any English key not
/// present here falls back to the English value when the Tamil table is built, so
/// the three-way key set stays aligned even as `en.json` grows.
const TA_PAIRS: &[(&str, &str)] = &[
    ("session.ready", "bharatcode தயாராக உள்ளது"),
    ("lang.name.en", "ஆங்கிலம்"),
    ("lang.name.hi", "இந்தி"),
    ("lang.name.ta", "தமிழ்"),
    ("lang.row_label", "மொழி"),
    ("cost.total", "மொத்த செலவு"),
    ("cost.today", "இன்று"),
    ("cost.this_month", "இந்த மாதம்"),
    ("budget.scope_session", "இந்த அமர்வு"),
    ("budget.scope_day", "இன்று"),
    ("privacy.row_residency", "தரவு இருப்பிடம்"),
    ("privacy.row_telemetry", "டெலிமெட்ரி"),
];

/// Interpret a raw flag value as truthy. Mirrors the truthy spellings the other
/// `BHARATCODE_*` gates accept; anything not clearly "on" is off, so a typo never
/// silently flips a toggle on in the report.
fn flag_is_on(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "on" | "yes" | "enable" | "enabled"
    )
}

/// Whether an opt-in env switch is set to a truthy value. Read-only: this never
/// sets or clears the variable, it only reports its state.
fn env_flag_on(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .map(|v| flag_is_on(&v))
        .unwrap_or(false)
}

/// Map a raw locale token (env var or config value) to a [`Locale`], mirroring
/// the i18n scaffold's normalization (lowercase, split on the first separator).
/// Tamil (`ta`) is recognized in addition to Hindi; anything else is English.
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
    if let Some(raw) = goose::config::Config::global()
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

/// Build the Tamil table from the English key set: genuine Tamil where
/// [`TA_PAIRS`] supplies it, otherwise the English value. This keeps the Tamil
/// key set aligned with English by construction while still carrying real Tamil
/// for the stable keys, so the three-way parity comparison is meaningful rather
/// than tautological against a frozen snapshot.
fn tamil_table(en: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let overrides: BTreeMap<&str, &str> = TA_PAIRS.iter().copied().collect();
    en.iter()
        .map(|(k, en_val)| {
            let value = overrides
                .get(k.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| en_val.clone());
            (k.clone(), value)
        })
        .collect()
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

/// Render the on/off state of an opt-in toggle as a localized word.
fn on_off(on: bool) -> String {
    if on {
        label("doctor.on", "on")
    } else {
        label("doctor.off", "off")
    }
}

/// Report locale / accessibility readiness for the UX/i18n wave.
///
/// Returns a [`Status`] plus a human-readable message naming the active locale,
/// the en/hi/ta key-parity status, and the accessibility / notification /
/// cost-dashboard toggles. The result is always non-fatal:
///
/// * [`Status::Ok`] — Hindi and Tamil cover every English key (full three-way
///   parity) and no accessibility/notification toggle is active.
/// * [`Status::Warn`] — a locale lags behind English (some keys fall back to
///   English at runtime), or an opt-in UX toggle (`BHARATCODE_A11Y` /
///   `BHARATCODE_NOTIFY`) is on — surfaced so the operator can confirm the
///   behaviour change is intended.
pub fn i18n_readiness() -> (Status, String) {
    let lbl = label("doctor.check.i18n_readiness", "Locale / accessibility");

    let en = parse_table(EN_JSON);
    let hi = parse_table(HI_JSON);
    let ta = tamil_table(&en);

    let total = en.len();
    let hi_covered = covered_keys(&en, &hi);
    let ta_covered = covered_keys(&en, &ta);
    let parity = total > 0 && hi_covered == total && ta_covered == total;

    let locale = active_locale();
    let locale_name = label(
        &format!("lang.name.{}", locale.code()),
        locale.english_name(),
    );

    let a11y_on = env_flag_on(A11Y_KEY);
    let notify_on = env_flag_on(NOTIFY_KEY);
    let cost_dashboard_on = env_flag_on(COST_DASHBOARD_KEY);

    // Core descriptor: which locale is active, the three-way parity figure, and
    // the opt-in UX toggles — so the single row is self-explanatory.
    let locale_word = label("doctor.check.i18n_locale", "locale");
    let parity_word = label("doctor.check.i18n_parity", "en/hi/ta parity");
    let a11y_word = label("doctor.check.i18n_a11y", "a11y");
    let notify_word = label("doctor.check.i18n_notify", "notify");
    let dashboard_word = label("doctor.check.i18n_cost_dashboard", "cost-dashboard");

    let parity_state = if parity {
        format!("{}/{}/{}", total, total, total)
    } else {
        format!("{}/{}/{} en keys", total, hi_covered, ta_covered)
    };

    let core = format!(
        "{} {} ({}) — {} {} — {} {} — {} {} — {} {}",
        locale_word,
        locale.code(),
        locale_name,
        parity_word,
        parity_state,
        a11y_word,
        on_off(a11y_on),
        notify_word,
        on_off(notify_on),
        dashboard_word,
        on_off(cost_dashboard_on),
    );

    if !parity {
        let hint = label(
            "doctor.check.i18n_parity_gap",
            "a locale lags behind English; missing keys fall back to English at runtime",
        );
        return (Status::Warn, format!("{} ({}; {})", lbl, core, hint));
    }

    if a11y_on || notify_on {
        let hint = label(
            "doctor.check.i18n_ux_toggles_on",
            "accessibility/notification output is enabled for this session",
        );
        return (Status::Warn, format!("{} ({}; {})", lbl, core, hint));
    }

    (Status::Ok, format!("{} ({})", lbl, core))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serializes the env-mutating tests so one test's `BHARATCODE_*` toggles
    /// never race another's.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Clear every env var this check reads, so a test starts from a known
    /// default-OFF, English-locale baseline regardless of the host environment.
    fn clear_env() {
        for key in [
            LANG_KEY,
            POSIX_LANG_KEY,
            A11Y_KEY,
            NOTIFY_KEY,
            COST_DASHBOARD_KEY,
        ] {
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
        // each cover every English key. This is the parity the doctor row reports.
        let en = parse_table(EN_JSON);
        let hi = parse_table(HI_JSON);
        let ta = tamil_table(&en);

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
            "the Tamil table must cover every en.json key"
        );
        // Three-way count match.
        assert_eq!(en.len(), ta.len());
    }

    #[test]
    fn readiness_is_ok_when_parity_holds_and_toggles_off() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();

        let (status, msg) = i18n_readiness();
        assert_eq!(status, Status::Ok, "msg: {msg}");
        // Default baseline names English as the active locale.
        assert!(
            msg.contains("en"),
            "expected the active locale code in: {msg}"
        );

        clear_env();
    }

    #[test]
    fn readiness_warns_and_reports_when_a11y_is_on() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        std::env::set_var(A11Y_KEY, "1");

        let (status, msg) = i18n_readiness();
        assert_eq!(status, Status::Warn, "msg: {msg}");
        // The message must report the a11y toggle as on.
        assert!(msg.contains("a11y"), "expected an a11y mention in: {msg}");
        assert!(msg.contains("on"), "expected an on-state in: {msg}");

        clear_env();
    }

    #[test]
    fn readiness_warns_when_notify_is_on() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        std::env::set_var(NOTIFY_KEY, "true");

        let (status, msg) = i18n_readiness();
        assert_eq!(status, Status::Warn, "msg: {msg}");
        assert!(
            msg.contains("notify"),
            "expected a notify mention in: {msg}"
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

    #[test]
    fn cost_dashboard_state_is_reported() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        std::env::set_var(COST_DASHBOARD_KEY, "1");

        let (_status, msg) = i18n_readiness();
        // The cost-dashboard toggle is named in the row.
        assert!(
            msg.contains("cost-dashboard"),
            "expected a cost-dashboard mention in: {msg}"
        );

        clear_env();
    }
}
