//! First-run quick-start splash + capability tour (BharatCode v89).
//!
//! On the very first interactive launch — before any config or session DB has
//! been written — brand-new users benefit from a one-screen orientation that
//! points at the handful of commands worth knowing first (`configure`,
//! `presets`, `cost`, `tutorial`). This module renders that splash as a plain
//! string and tracks "have we shown it yet?" with a tiny sentinel file under the
//! config directory.
//!
//! Design constraints that keep the default experience unchanged:
//!
//! * The splash is shown **at most once**. After the first run a
//!   `.quickstart_shown` sentinel is written under the config dir; established
//!   users (sentinel present, or an existing session DB / config) never see it
//!   again. The caller decides "is this the first run?" and passes that in.
//! * It can be suppressed entirely by setting `BHARATCODE_NO_SPLASH` to any
//!   non-empty value, which makes [`should_show`] return `false` regardless of
//!   first-run state. This gives scripted / CI launches a hard off switch.
//! * Output is colorized only when stdout colors are appropriate. The `NO_COLOR`
//!   convention (any non-empty `NO_COLOR`) yields a plain-text splash so the
//!   string is safe to capture, pipe, or snapshot in tests.
//!
//! Localization uses a small embedded table rather than the CLI's i18n crate so
//! this lives entirely inside the core `goose` crate with no extra dependency.
//! `render_splash("hi")` returns Hindi blurbs; unknown locales fall back to
//! English, so output is never the bare key and never leaks the locale token.
//!
//! This module is original work; nothing here is ported from third-party
//! sources.

use std::path::{Path, PathBuf};

/// Hard off-switch env var. Any non-empty value suppresses the splash.
const NO_SPLASH_ENV: &str = "BHARATCODE_NO_SPLASH";

/// Sentinel file name written under the config dir once the splash has shown.
const SENTINEL_NAME: &str = ".quickstart_shown";

/// One quick-start row: the command to type and a one-line blurb describing it.
struct Tip {
    /// The command the user types, e.g. `configure`. Not localized.
    command: &'static str,
    /// English blurb, used directly for `en` and as the fallback for any locale
    /// that lacks a translation.
    blurb_en: &'static str,
    /// Hindi (Devanagari) blurb. Empty falls back to [`Tip::blurb_en`].
    blurb_hi: &'static str,
    /// Tamil blurb. Empty falls back to [`Tip::blurb_en`].
    blurb_ta: &'static str,
}

/// The curated 4–5 most useful commands for a brand-new user, in priority order.
const TIPS: &[Tip] = &[
    Tip {
        command: "configure",
        blurb_en: "Pick a model and connect a provider to get started.",
        blurb_hi: "शुरू करने के लिए एक मॉडल चुनें और प्रोवाइडर कनेक्ट करें।",
        blurb_ta: "தொடங்க ஒரு மாதிரியைத் தேர்ந்தெடுத்து வழங்குநரை இணைக்கவும்.",
    },
    Tip {
        command: "presets",
        blurb_en: "Switch between ready-made setups for common tasks.",
        blurb_hi: "सामान्य कार्यों के लिए तैयार सेटअप के बीच स्विच करें।",
        blurb_ta: "பொதுவான பணிகளுக்கான தயார் அமைப்புகளுக்கு இடையே மாறவும்.",
    },
    Tip {
        command: "cost",
        blurb_en: "See token usage and spend for your sessions.",
        blurb_hi: "अपने सत्रों के लिए टोकन उपयोग और खर्च देखें।",
        blurb_ta: "உங்கள் அமர்வுகளுக்கான டோக்கன் பயன்பாடு மற்றும் செலவைப் பார்க்கவும்.",
    },
    Tip {
        command: "tutorial",
        blurb_en: "Take a short guided tour of the basics.",
        blurb_hi: "मूल बातों का एक छोटा निर्देशित दौरा करें।",
        blurb_ta: "அடிப்படைகளின் ஒரு குறுகிய வழிகாட்டப்பட்ட சுற்றுப்பயணத்தை மேற்கொள்ளவும்.",
    },
];

/// Localized header / footer chrome for the splash, keyed by locale.
struct Chrome {
    title: &'static str,
    intro: &'static str,
    footer: &'static str,
}

fn chrome_for(locale: &str) -> Chrome {
    match locale {
        "hi" => Chrome {
            title: "स्वागत है — त्वरित शुरुआत",
            intro: "यहाँ शुरू करने के लिए कुछ उपयोगी कमांड दिए गए हैं:",
            footer: "जारी रखने के लिए Enter दबाएँ…",
        },
        "ta" => Chrome {
            title: "வரவேற்பு — விரைவு தொடக்கம்",
            intro: "தொடங்குவதற்கு சில பயனுள்ள கட்டளைகள் இங்கே:",
            footer: "தொடர Enter ஐ அழுத்தவும்…",
        },
        _ => Chrome {
            title: "Welcome — Quick Start",
            intro: "Here are a few useful commands to get you going:",
            footer: "Press Enter to continue…",
        },
    }
}

/// Pick the blurb for a tip in the given locale, falling back to English.
fn blurb_for<'a>(tip: &'a Tip, locale: &str) -> &'a str {
    let candidate = match locale {
        "hi" => tip.blurb_hi,
        "ta" => tip.blurb_ta,
        _ => "",
    };
    if candidate.is_empty() {
        tip.blurb_en
    } else {
        candidate
    }
}

/// Normalize a raw locale token (e.g. `hi_IN.UTF-8`, `HI`, `ta-IN`) to one of the
/// short codes the embedded table understands (`hi`, `ta`, else `en`).
fn normalize_locale(raw: &str) -> &'static str {
    let lowered = raw.trim().to_ascii_lowercase();
    let primary = lowered
        .split(|c| c == '_' || c == '-' || c == '.')
        .next()
        .unwrap_or("");
    match primary {
        "hi" => "hi",
        "ta" => "ta",
        _ => "en",
    }
}

/// Whether colorized output is appropriate. Honors the `NO_COLOR` convention:
/// any non-empty `NO_COLOR` disables color. Pure function of the environment.
fn use_color() -> bool {
    !std::env::var("NO_COLOR")
        .map(|v| !v.is_empty())
        .unwrap_or(false)
}

/// Wrap `s` in an ANSI SGR sequence when color is enabled, otherwise return it
/// unchanged. Keeps the renderer free of `console`/`owo-colors` dependencies.
fn paint(s: &str, sgr: &str, color: bool) -> String {
    if color {
        format!("\u{1b}[{sgr}m{s}\u{1b}[0m")
    } else {
        s.to_string()
    }
}

/// Decide whether the quick-start splash should be shown this launch.
///
/// Returns `false` when [`NO_SPLASH_ENV`] is set to any non-empty value (hard off
/// switch for scripted / CI runs), otherwise mirrors the caller-supplied
/// `first_run` signal. Pure aside from the single env read, so it is trivial to
/// unit test by toggling the env var.
pub fn should_show(first_run: bool) -> bool {
    if std::env::var(NO_SPLASH_ENV)
        .map(|v| !v.is_empty())
        .unwrap_or(false)
    {
        return false;
    }
    first_run
}

/// Render the one-screen quick-start splash as a string for the given locale.
///
/// Accepts a raw locale token (`hi`, `hi_IN.UTF-8`, `en`, …); unknown locales
/// fall back to English so output is never the bare locale token. The result is
/// colorized only when [`use_color`] permits (i.e. `NO_COLOR` is unset), making
/// the plain-text form stable for snapshot tests. Ends with a localized
/// "press Enter to continue" affordance.
pub fn render_splash(locale: &str) -> String {
    let loc = normalize_locale(locale);
    let chrome = chrome_for(loc);
    let color = use_color();

    let mut out = String::new();
    out.push_str(&paint(chrome.title, "1;36", color));
    out.push('\n');
    out.push_str(&paint(chrome.intro, "2", color));
    out.push('\n');
    out.push('\n');

    let width = TIPS.iter().map(|t| t.command.len()).max().unwrap_or(0);
    for tip in TIPS {
        let cmd = format!("{:width$}", tip.command, width = width);
        out.push_str("  ");
        out.push_str(&paint(&cmd, "1;32", color));
        out.push_str("  ");
        out.push_str(blurb_for(tip, loc));
        out.push('\n');
    }

    out.push('\n');
    out.push_str(&paint(chrome.footer, "2", color));
    out
}

/// Path to the once-only sentinel under `dir` (typically the config dir).
fn sentinel_path(dir: &Path) -> PathBuf {
    dir.join(SENTINEL_NAME)
}

/// Whether the splash has already been shown, per the sentinel under `dir`.
pub fn already_shown(dir: &Path) -> bool {
    sentinel_path(dir).exists()
}

/// Record that the splash has been shown by writing the sentinel under `dir`.
///
/// Best-effort: creates `dir` if needed and ignores write errors, since failing
/// to persist the sentinel should never break an interactive launch (worst case
/// the splash shows once more on the next first-run check).
pub fn mark_shown(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(sentinel_path(dir), b"1\n");
}

/// Full gated flow for a single call site: if this is the first run and the
/// splash has neither been suppressed nor already shown under `config_dir`,
/// return the rendered splash for `locale` and persist the sentinel so it never
/// shows again. Returns `None` when nothing should be printed, leaving default
/// behavior byte-identical for established users and `BHARATCODE_NO_SPLASH` runs.
///
/// This is the one-liner the CLI session builder wires in on first launch.
pub fn maybe_render(first_run: bool, config_dir: &Path, locale: &str) -> Option<String> {
    if !should_show(first_run) || already_shown(config_dir) {
        return None;
    }
    mark_shown(config_dir);
    Some(render_splash(locale))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    /// Serializes tests that mutate process-global env vars.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn lock_env() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn should_show_reflects_first_run_when_not_suppressed() {
        let _guard = lock_env();
        std::env::remove_var(NO_SPLASH_ENV);
        assert!(should_show(true));
        assert!(!should_show(false));
    }

    #[test]
    fn no_splash_env_forces_off() {
        let _guard = lock_env();
        std::env::set_var(NO_SPLASH_ENV, "1");
        assert!(!should_show(true));
        assert!(!should_show(false));
        std::env::remove_var(NO_SPLASH_ENV);
        // Empty value does not count as set.
        std::env::set_var(NO_SPLASH_ENV, "");
        assert!(should_show(true));
        std::env::remove_var(NO_SPLASH_ENV);
    }

    #[test]
    fn render_splash_localizes_and_includes_affordance() {
        let _guard = lock_env();
        std::env::set_var("NO_COLOR", "1");
        let en = render_splash("en");
        let hi = render_splash("hi");
        std::env::remove_var("NO_COLOR");

        // Localized output differs between Hindi and English.
        assert_ne!(en, hi);
        // Curated commands are present in both.
        for cmd in ["configure", "presets", "cost", "tutorial"] {
            assert!(en.contains(cmd), "en splash missing `{cmd}`");
            assert!(hi.contains(cmd), "hi splash missing `{cmd}`");
        }
        // The "press Enter to continue" affordance is present.
        assert!(en.contains("Press Enter"));
        // Hindi carries Devanagari text, not the bare English footer.
        assert!(hi.contains("Enter"));
        assert!(hi.chars().any(|c| ('\u{0900}'..='\u{097F}').contains(&c)));
    }

    #[test]
    fn unknown_locale_falls_back_to_english() {
        let _guard = lock_env();
        std::env::set_var("NO_COLOR", "1");
        let xx = render_splash("zz_ZZ.UTF-8");
        let en = render_splash("en");
        std::env::remove_var("NO_COLOR");
        assert_eq!(xx, en);
    }

    #[test]
    fn no_color_strips_ansi() {
        let _guard = lock_env();
        std::env::set_var("NO_COLOR", "1");
        let plain = render_splash("en");
        std::env::remove_var("NO_COLOR");
        assert!(!plain.contains('\u{1b}'), "NO_COLOR output must be plain");
    }

    #[test]
    fn already_shown_reflects_mark_shown() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!already_shown(dir.path()));
        mark_shown(dir.path());
        assert!(already_shown(dir.path()));
    }

    #[test]
    fn maybe_render_is_once_only_and_gated() {
        let _guard = lock_env();
        std::env::remove_var(NO_SPLASH_ENV);
        std::env::set_var("NO_COLOR", "1");
        let dir = tempfile::tempdir().unwrap();

        // Not a first run: nothing rendered, no sentinel written.
        assert!(maybe_render(false, dir.path(), "en").is_none());
        assert!(!already_shown(dir.path()));

        // First run: renders once and persists the sentinel.
        let first = maybe_render(true, dir.path(), "en");
        assert!(first.is_some());
        assert!(already_shown(dir.path()));

        // Second first-run check: suppressed by the sentinel.
        assert!(maybe_render(true, dir.path(), "en").is_none());

        std::env::remove_var("NO_COLOR");
    }

    #[test]
    fn maybe_render_respects_no_splash_env() {
        let _guard = lock_env();
        std::env::set_var(NO_SPLASH_ENV, "1");
        let dir = tempfile::tempdir().unwrap();
        assert!(maybe_render(true, dir.path(), "en").is_none());
        // Sentinel is not written when suppressed by env.
        assert!(!already_shown(dir.path()));
        std::env::remove_var(NO_SPLASH_ENV);
    }
}
