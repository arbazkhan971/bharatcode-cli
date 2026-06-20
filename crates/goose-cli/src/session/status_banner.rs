//! BharatCode v89: localized, theme-aware session status banner / footer.
//!
//! At the start of an interactive session this renders a single, compact line
//! summarizing the session's posture — the active locale (in its own script),
//! the provider/model, the active color theme, and a one-glyph privacy summary —
//! and at the end of the session a matching, shorter footer line. Both are
//! purely additive: they are only emitted on an interactive TTY with screen-
//! reader mode off (see the call site in `mod.rs`), so default non-interactive
//! output and piped/redirected output are byte-for-byte unchanged.
//!
//! Behavioural guarantees, derived purely from the environment (no opt-in flag):
//!   * `NO_COLOR` (any value) and the `none` theme => the line carries zero ANSI
//!     escape bytes and uses no box-drawing / decorative glyphs, so it stays
//!     legible when color is off.
//!   * Field captions route through [`tr_or`], which consults the active locale
//!     table and falls back to a supplied English default, so the banner is
//!     Hindi/Tamil-ready while leaving default English output untouched.
//!   * No upstream brand names ever appear (the privacy/theme/locale data is all
//!     local), satisfying the no-leak invariant.
//!
//! Original BharatCode work; not ported from any third party. It leans only on
//! the in-crate [`crate::theme`] role helpers and the [`crate::i18n`] locale.

use crate::i18n::Locale;
use crate::theme;

/// Inputs needed to render the start banner and end footer.
///
/// All fields are owned so the call site can build one cheaply from live
/// session state (provider/model strings, the resolved locale and theme) and
/// hand it to both [`render_start`] and [`render_end`].
pub struct BannerCtx {
    /// Localized label for the active locale, in its own script
    /// (e.g. `"English"`, `"हिन्दी"`, `"தமிழ்"`). Built via [`locale_label`].
    pub locale_label: String,
    /// Active provider name, e.g. `"tetrate"`.
    pub provider: String,
    /// Active model name, e.g. `"gpt-4o"`.
    pub model: String,
    /// Active color theme name, e.g. `"default"` / `"tiranga"` / `"none"`.
    pub theme: String,
    /// One-glyph privacy posture summary (see [`privacy_glyph`]).
    pub privacy: char,
}

impl BannerCtx {
    /// Build a [`BannerCtx`] from live session state.
    ///
    /// `provider` / `model` come from the active provider; the locale, theme and
    /// privacy glyph are resolved from the process environment so the banner
    /// reflects exactly the posture the rest of the CLI is running under.
    pub fn from_session(provider: &str, model: &str) -> Self {
        BannerCtx {
            locale_label: locale_label(crate::i18n::active_locale()),
            provider: provider.to_string(),
            model: model.to_string(),
            theme: theme::active_theme().name.to_string(),
            privacy: privacy_glyph(),
        }
    }
}

/// Localized display name for `locale`, written in the language's own script.
///
/// Kept here (rather than in i18n) because it is purely a presentation label for
/// this banner; the i18n layer keys off the [`Locale`] enum, which this mirrors.
pub fn locale_label(locale: Locale) -> String {
    match locale {
        Locale::En => "English".to_string(),
        Locale::Hi => "\u{0939}\u{093f}\u{0928}\u{094d}\u{0926}\u{0940}".to_string(), // हिन्दी
        Locale::Ta => "\u{0ba4}\u{0bae}\u{0bbf}\u{0bb4}\u{0bcd}".to_string(),         // தமிழ்
    }
}

/// One-glyph summary of the session's privacy posture.
///
/// Reads the same environment kill-switches the rest of the CLI honors and
/// collapses them to a single, color-free marker:
///   * `*` — hardened: offline AND redaction on (no data leaves the box and
///     secrets are scrubbed).
///   * `+` — guarded: at least one of offline / redaction / telemetry-off is on.
///   * `.` — standard posture (nothing extra enabled).
///
/// The glyph is deliberately ASCII so it survives `NO_COLOR` and screen-reader
/// plain output without becoming Unicode-name noise.
pub fn privacy_glyph() -> char {
    let offline = env_truthy("BHARATCODE_OFFLINE");
    let redact = env_truthy("BHARATCODE_REDACT");
    let telemetry_off = std::env::var_os("BHARATCODE_TELEMETRY_OFF").is_some()
        || (std::env::var_os("BHARATCODE_TELEMETRY_ENABLED").is_some()
            && !env_truthy("BHARATCODE_TELEMETRY_ENABLED"));

    if offline && redact {
        '*'
    } else if offline || redact || telemetry_off {
        '+'
    } else {
        '.'
    }
}

fn env_truthy(key: &str) -> bool {
    match std::env::var(key) {
        Ok(raw) => matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
}

/// `true` when ANSI styling must be suppressed because `NO_COLOR` is set.
///
/// Follows the de-facto standard: the variable disables color when present with
/// any value (https://no-color.org/).
fn no_color() -> bool {
    std::env::var_os("NO_COLOR").is_some()
}

/// Look up a field caption in the active locale table, falling back to the
/// supplied English default when no localized string exists.
///
/// [`crate::i18n::t`] returns the lookup key verbatim when a key is missing, so
/// we pass `default` itself as the key and treat an identity result as "no
/// translation", yielding the default. This keeps captions Hindi/Tamil-ready
/// (a locale table can map the English caption to its own string) while leaving
/// default English output untouched.
fn tr_or(default: &str) -> String {
    let translated = crate::tr!(default);
    if translated == default {
        default.to_string()
    } else {
        translated
    }
}

/// Paint `text` with the active theme's accent role unless color is off.
///
/// Both [`no_color`] and the `none` theme already collapse to plain output, so
/// this is a no-op (uncolored) in those cases; the explicit `no_color` guard
/// keeps the output free of any escape bytes even if a future theme forgot to.
fn accent(text: &str) -> String {
    if no_color() {
        text.to_string()
    } else {
        format!("{}", theme::accent(text.to_string()))
    }
}

/// Paint `text` with the active theme's muted role unless color is off.
fn muted(text: &str) -> String {
    if no_color() {
        text.to_string()
    } else {
        format!("{}", theme::muted(text.to_string()))
    }
}

/// Render the start-of-session banner for `ctx`.
///
/// A single line (no trailing newline) of the form:
///   `<glyph> <locale> · <provider>/<model> · <theme-label>: <theme> · <privacy-label> <p>`
///
/// with field captions localized and values accented via the active theme. The
/// separator is a middle dot (not a box-drawing rule), so the no-box-drawing
/// guarantee holds under `NO_COLOR`.
pub fn render_start(ctx: &BannerCtx) -> String {
    let lead = muted("\u{2022}"); // a plain bullet, never a box-drawing char
    let locale = accent(&ctx.locale_label);
    let pm = accent(&format!("{}/{}", ctx.provider, ctx.model));
    let theme_field = format!("{}: {}", muted(&tr_or("theme")), ctx.theme);
    let privacy_field = format!("{} {}", muted(&tr_or("privacy")), ctx.privacy);
    let sep = muted(" \u{00b7} ");

    format!("{lead} {locale}{sep}{pm}{sep}{theme_field}{sep}{privacy_field}")
}

/// Render the end-of-session footer for `ctx`.
///
/// A shorter, distinct line summarizing where the session leaves the user — the
/// locale and provider/model only — so it reads as a sign-off rather than a
/// repeat of the start banner. Distinct text guarantees `render_end != render_start`.
pub fn render_end(ctx: &BannerCtx) -> String {
    let lead = muted("\u{2022}");
    let label = muted(&tr_or("session ended"));
    let locale = accent(&ctx.locale_label);
    let pm = accent(&format!("{}/{}", ctx.provider, ctx.model));
    let sep = muted(" \u{00b7} ");

    format!("{lead} {label}{sep}{locale}{sep}{pm}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // The tests below toggle the process-global `NO_COLOR`, so they must not run
    // concurrently with each other. Serialize them through one mutex.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn sample() -> BannerCtx {
        BannerCtx {
            locale_label: locale_label(Locale::Hi),
            provider: "tetrate".to_string(),
            model: "gpt-4o".to_string(),
            theme: "tiranga".to_string(),
            privacy: '*',
        }
    }

    /// Run `f` with `NO_COLOR` forced to the given state, restoring it after.
    fn with_no_color<T>(enabled: bool, f: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var_os("NO_COLOR");
        if enabled {
            std::env::set_var("NO_COLOR", "1");
        } else {
            std::env::remove_var("NO_COLOR");
        }
        let out = f();
        match prev {
            Some(v) => std::env::set_var("NO_COLOR", v),
            None => std::env::remove_var("NO_COLOR"),
        }
        out
    }

    /// Guard the no-leak invariant: no user-facing upstream brand names.
    fn assert_no_brand_leak(s: &str) {
        let lower = s.to_lowercase();
        assert!(
            !lower.contains("goose"),
            "banner leaked an upstream ident: {s}"
        );
        assert!(
            !lower.contains("block"),
            "banner leaked an upstream ident: {s}"
        );
    }

    #[test]
    fn start_contains_provider_model_and_localized_locale() {
        let out = with_no_color(true, || render_start(&sample()));
        assert!(out.contains("tetrate"), "missing provider: {out}");
        assert!(out.contains("gpt-4o"), "missing model: {out}");
        // The localized Hindi label (हिन्दी) must appear verbatim.
        assert!(
            out.contains(&locale_label(Locale::Hi)),
            "missing localized locale label: {out}"
        );
        assert_no_brand_leak(&out);
    }

    #[test]
    fn no_color_has_no_box_drawing_or_escape_bytes() {
        let out = with_no_color(true, || render_start(&sample()));
        // No ANSI escape (ESC, 0x1B) bytes.
        assert!(
            !out.as_bytes().contains(&0x1b),
            "NO_COLOR output contained an ANSI escape: {out:?}"
        );
        // No box-drawing (U+2500..U+257F), block (U+2580..U+259F) or geometric
        // shape (U+25A0..U+25FF) glyphs.
        for ch in out.chars() {
            let c = ch as u32;
            assert!(
                !(0x2500..=0x25FF).contains(&c),
                "NO_COLOR output contained a box/shape glyph U+{c:04X}: {out:?}"
            );
        }
        assert_no_brand_leak(&out);
    }

    #[test]
    fn end_is_non_empty_and_distinct_from_start() {
        let ctx = sample();
        let start = with_no_color(true, || render_start(&ctx));
        let end = with_no_color(true, || render_end(&ctx));
        assert!(!end.is_empty(), "end footer was empty");
        assert_ne!(start, end, "end footer must differ from start banner");
        assert_no_brand_leak(&end);
    }

    #[test]
    fn locale_labels_are_in_own_script() {
        assert_eq!(locale_label(Locale::En), "English");
        assert!(locale_label(Locale::Hi).chars().any(|c| {
            let v = c as u32;
            (0x0900..=0x097F).contains(&v) // Devanagari
        }));
        assert!(locale_label(Locale::Ta).chars().any(|c| {
            let v = c as u32;
            (0x0B80..=0x0BFF).contains(&v) // Tamil
        }));
    }

    #[test]
    fn privacy_glyph_is_ascii() {
        // Whatever the environment, the glyph must be a single ASCII char so it
        // survives NO_COLOR and screen-reader plain output.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        assert!(privacy_glyph().is_ascii());
    }
}
