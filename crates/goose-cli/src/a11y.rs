//! Opt-in accessibility / screen-reader-friendly output mode for BharatCode CLI.
//!
//! When enabled, the streaming renderer suppresses spinners and decorative
//! glyphs and instead emits labelled plain-text lines (`Tool call: shell`,
//! `Result:`) that screen readers can announce cleanly. The mode is opt-in via
//! the `BHARATCODE_A11Y` environment variable (any truthy value), and is also
//! implicitly enabled when `NO_COLOR` is set (a widely-honoured convention that
//! signals a plain-text-preferring environment).
//!
//! Default behaviour is unchanged: when neither variable is set,
//! [`is_enabled`] returns `false` and every helper here yields the original
//! styled output, keeping default sessions byte-identical.
//!
//! String labels live in `i18n/a11y_en.json` (and a parity copy in
//! `i18n/a11y_hi.json`) and are loaded directly here so the a11y keys stay
//! disjoint from the main i18n tables.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::LazyLock;

/// Environment variable that opts into accessibility mode.
const ENABLE_KEY: &str = "BHARATCODE_A11Y";

/// Environment variable that, when present, implies plain-text / a11y output.
const NO_COLOR_KEY: &str = "NO_COLOR";

static LABELS_EN: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("i18n/a11y_en.json"))
        .expect("a11y: a11y_en.json is not valid JSON")
});

static LABELS_HI: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("i18n/a11y_hi.json"))
        .expect("a11y: a11y_hi.json is not valid JSON")
});

/// Whether the active locale (mirroring the i18n resolution order, scoped to the
/// env layer used by this opt-in mode) resolves to Hindi.
fn locale_is_hindi() -> bool {
    let raw = std::env::var("BHARATCODE_LANG")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("LANG").ok().filter(|s| !s.trim().is_empty()))
        .unwrap_or_default();
    raw.trim()
        .to_ascii_lowercase()
        .split(|c| c == '_' || c == '-' || c == '.')
        .next()
        .map(|primary| primary == "hi")
        .unwrap_or(false)
}

fn labels() -> &'static HashMap<String, String> {
    if locale_is_hindi() {
        &LABELS_HI
    } else {
        &LABELS_EN
    }
}

/// Mirrors the `is_truthy` helper used by `goose::memory_store` and the privacy
/// command: a small, fixed set of affirmative spellings, case-insensitive and
/// whitespace-trimmed.
fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Whether accessibility / screen-reader output mode is active.
///
/// Resolution order (mirroring `goose::verify::enabled`): the raw
/// `BHARATCODE_A11Y` environment variable wins when set (so a bare `1` is not
/// coerced through the typed config layer and read back as unset); otherwise the
/// `BHARATCODE_A11Y` config parameter is consulted; finally the `NO_COLOR`
/// convention (a widely-honoured plain-text-preference signal) is honoured. With
/// none of these present the function returns `false`, keeping default sessions
/// byte-identical.
pub fn is_enabled() -> bool {
    if let Ok(raw) = std::env::var(ENABLE_KEY) {
        return is_truthy(&raw);
    }
    if goose::config::Config::global()
        .get_param::<bool>(ENABLE_KEY)
        .unwrap_or(false)
    {
        return true;
    }
    std::env::var_os(NO_COLOR_KEY).is_some()
}

/// Whether spinners / animations should be suppressed. In a11y mode, animated
/// output confuses screen readers, so it is suppressed entirely.
pub fn suppress_animations() -> bool {
    is_enabled()
}

fn label_text(key: &'static str) -> &'static str {
    labels()
        .get(key)
        .map(String::as_str)
        .or_else(|| LABELS_EN.get(key).map(String::as_str))
        .unwrap_or(key)
}

/// Machine-readable, screen-reader-friendly role label for a session-chrome
/// `role`, e.g. `label("assistant") == "Assistant:"`.
///
/// The returned label is stable, ASCII, and trailing-colon-terminated so that
/// screen readers announce a linear `Role: content` stream. Unknown roles fall
/// back to a generic `Output:` marker rather than leaking the raw role token.
pub fn label(role: &str) -> &'static str {
    match role.trim().to_ascii_lowercase().as_str() {
        "assistant" | "model" => "Assistant:",
        "user" => "User:",
        "tool" | "tool_call" | "tool_request" => "Tool:",
        "tool_result" | "tool_response" | "result" => "Result:",
        "thinking" | "reasoning" => "Thinking:",
        "system" => "System:",
        "error" => "Error:",
        _ => "Output:",
    }
}

/// A labelled, screen-reader-friendly announcement of a tool request, e.g.
/// `Tool call: shell`.
pub fn announce_tool_request(name: &str) -> String {
    format!("{}: {}", label_text("a11y.tool_call"), name)
}

/// A labelled, screen-reader-friendly announcement of a tool response, e.g.
/// `Result:`.
pub fn announce_tool_response() -> String {
    format!("{}:", label_text("a11y.result"))
}

/// Decorative glyphs that screen readers either mispronounce or drop entirely,
/// paired with the ASCII word they are replaced by in a11y mode.
const GLYPH_WORDS: &[(char, &str)] = &[
    ('\u{2713}', "OK"),     // ✓
    ('\u{2717}', "FAILED"), // ✗
    ('\u{2718}', "FAILED"), // ✘
    ('\u{2705}', "OK"),     // ✅
    ('\u{274C}', "FAILED"), // ❌
    ('\u{25CF}', "*"),      // ●
    ('\u{25CB}', "o"),      // ○
    ('\u{2192}', "->"),     // →
    ('\u{25B8}', ">"),      // ▸
    ('\u{25B6}', ">"),      // ▶
    ('\u{2026}', "..."),    // …
    ('\u{2022}', "-"),      // •
];

/// Strip ANSI escape sequences and map decorative unicode glyphs to ASCII words,
/// yielding a linear, screen-reader-announceable string.
///
/// Returns `Cow::Borrowed` when `s` contains neither ANSI escapes nor any
/// decorative glyph, so non-decorated lines incur zero allocation and stay
/// byte-identical. This helper does not consult [`is_enabled`]; call sites gate
/// on the mode and only reach for `plain` once a11y output is requested.
pub fn plain(s: &str) -> Cow<'_, str> {
    let stripped = console::strip_ansi_codes(s);

    if !stripped
        .chars()
        .any(|c| GLYPH_WORDS.iter().any(|(g, _)| *g == c))
    {
        return stripped;
    }

    let mut out = String::with_capacity(stripped.len());
    for ch in stripped.chars() {
        match GLYPH_WORDS.iter().find(|(g, _)| *g == ch) {
            Some((_, word)) => out.push_str(word),
            None => out.push(ch),
        }
    }
    Cow::Owned(out)
}

/// Return the spelled-out `word` form of a decorative `symbol` when a11y mode
/// is enabled, otherwise the original `symbol`. Keeps call sites terse while
/// guaranteeing OFF == byte-identical output.
pub fn glyph_or_word(symbol: &str, word: &str) -> String {
    if is_enabled() {
        word.to_string()
    } else {
        symbol.to_string()
    }
}

/// Whether `c` is a purely decorative glyph that a screen reader either
/// mispronounces (announcing its long Unicode name) or drops entirely: spinner
/// frames (Braille patterns), box-drawing, block elements and the geometric
/// shapes used as status bullets/spinner dots.
///
/// These are *dropped* by [`plainify`] (as opposed to the small set in
/// [`GLYPH_WORDS`], which are *rewritten* to a meaningful ASCII word like
/// `✓`→`OK`).
fn is_decorative(c: char) -> bool {
    matches!(c as u32,
        // Box Drawing
        0x2500..=0x257F
        // Block Elements
        | 0x2580..=0x259F
        // Geometric Shapes (● ○ ■ ▶ status bullets / spinner dots)
        | 0x25A0..=0x25FF
        // Braille patterns (the common ⠋⠙⠹… spinner frame set)
        | 0x2800..=0x28FF
        // Variation selectors + ZWJ (emoji presentation joiners)
        | 0xFE00..=0xFE0F
        | 0x200D
        // Misc Symbols and Dingbats
        | 0x2600..=0x27BF
        // Misc Symbols and Pictographs
        | 0x1F300..=0x1F5FF
        // Emoticons
        | 0x1F600..=0x1F64F
        // Transport and Map Symbols
        | 0x1F680..=0x1F6FF
        // Supplemental Symbols and Pictographs
        | 0x1F900..=0x1F9FF
        // Symbols and Pictographs Extended-A
        | 0x1FA00..=0x1FAFF
        // Regional indicator symbols (flag-emoji halves)
        | 0x1F1E6..=0x1F1FF
    )
}

/// Decorative bullet / pointer glyphs that [`GLYPH_WORDS`] maps to an ASCII
/// marker (`*`/`o`/`>`/`-`) for layout-preserving [`plain`] output, but which
/// [`plainify`] drops entirely: their meaning is positional chrome, not content,
/// so a screen reader is better served by the surrounding prose alone.
fn is_plainify_drop(c: char) -> bool {
    matches!(
        c as u32,
        0x25CF // ●
        | 0x25CB // ○
        | 0x25B8 // ▸
        | 0x25B6 // ▶
        | 0x2022 // •
    )
}

/// Flatten a status line into linear, screen-reader-friendly text.
///
/// Unlike [`plain`] (which returns a borrowed `Cow` and is the zero-allocation
/// fast path for render sites), `plainify` always returns an owned `String` and
/// performs the full flattening that the accessibility profile asks for:
///
///   1. ANSI / SGR escape sequences are stripped (color is noise to a reader).
///   2. The meaning-bearing glyphs in [`GLYPH_WORDS`] are rewritten to their
///      ASCII word (`✓`→`OK`, `…`→`...`, `→`→`->`).
///   3. Purely decorative glyphs — spinner frames, box-drawing, block elements,
///      bullet dots and emoji (see [`is_decorative`]) — are dropped.
///   4. Whitespace left behind by removed chrome is collapsed to single spaces
///      and the ends are trimmed, so stripped decoration leaves no ragged gaps.
///
/// `plainify("⠋ working…") == "working..."`.
pub fn plainify(s: &str) -> String {
    let stripped = console::strip_ansi_codes(s);

    let mut out = String::with_capacity(stripped.len());
    let mut pending_space = false;
    let mut wrote_any = false;

    for ch in stripped.chars() {
        // `plain` rewrites every GLYPH_WORDS entry (including decorative bullets
        // like ● → "*") because it preserves layout. `plainify` is the stronger
        // screen-reader flattener: it only *rewrites* meaning-bearing glyphs
        // (status checks/crosses, arrows, ellipsis) and *drops* purely
        // decorative bullets / spinner dots (●○▸▶•) so a status line collapses
        // to its prose. The drop set is the GLYPH_WORDS entries whose meaning is
        // carried by position, not the glyph itself.
        if !is_plainify_drop(ch) {
            if let Some((_, word)) = GLYPH_WORDS.iter().find(|(g, _)| *g == ch) {
                if pending_space && wrote_any {
                    out.push(' ');
                }
                pending_space = false;
                out.push_str(word);
                wrote_any = true;
                continue;
            }
        }
        if is_decorative(ch) || is_plainify_drop(ch) || ch.is_whitespace() {
            // A removed glyph or run of whitespace becomes a single soft break;
            // coalesce so chrome removal does not leave double spaces.
            pending_space = true;
            continue;
        }
        if pending_space && wrote_any {
            out.push(' ');
        }
        pending_space = false;
        out.push(ch);
        wrote_any = true;
    }

    out
}

/// Resolved accessibility preferences for a session, computed once at session
/// build and stashed in the render layer so hot paths consult a plain struct
/// rather than re-reading the environment on every line.
///
/// Default (all fields `false`) is the unset / opt-out state and keeps output
/// byte-identical.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct A11yProfile {
    /// Emit flat, labelled, screen-reader-announceable text instead of styled
    /// chrome. Mirrors [`is_enabled`].
    pub screen_reader: bool,
    /// Suppress spinners / animated progress (they read as a stream of noise to
    /// assistive technology). Implied by `screen_reader`.
    pub no_spinner: bool,
}

impl A11yProfile {
    /// Resolve the profile from the environment / config, using the same gate as
    /// [`is_enabled`]. When accessibility mode is off, every field is `false`.
    pub fn from_env() -> Self {
        let on = is_enabled();
        A11yProfile {
            screen_reader: on,
            no_spinner: on,
        }
    }

    /// Whether any accessibility accommodation is active.
    pub fn any(&self) -> bool {
        self.screen_reader || self.no_spinner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Env mutation is process-global; serialize the env-touching tests.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard;
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            std::env::remove_var(ENABLE_KEY);
            std::env::remove_var(NO_COLOR_KEY);
        }
    }

    fn clean_env() -> EnvGuard {
        std::env::remove_var(ENABLE_KEY);
        std::env::remove_var(NO_COLOR_KEY);
        EnvGuard
    }

    #[test]
    fn is_truthy_matches_documented_spellings() {
        for v in ["1", "true", "TRUE", " yes ", "on", "On"] {
            assert!(is_truthy(v), "{v:?} should be truthy");
        }
        for v in ["0", "false", "", "maybe", "off"] {
            assert!(!is_truthy(v), "{v:?} should not be truthy");
        }
    }

    #[test]
    fn is_enabled_false_when_unset() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _g = clean_env();
        assert!(!is_enabled());
        assert!(!suppress_animations());
    }

    #[test]
    fn is_enabled_true_on_explicit_flag() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _g = clean_env();
        std::env::set_var(ENABLE_KEY, "1");
        assert!(is_enabled());
        assert!(suppress_animations());
    }

    #[test]
    fn is_enabled_true_on_no_color() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _g = clean_env();
        std::env::set_var(NO_COLOR_KEY, "");
        assert!(is_enabled());
    }

    #[test]
    fn is_enabled_false_on_falsey_flag_without_no_color() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _g = clean_env();
        std::env::set_var(ENABLE_KEY, "0");
        assert!(!is_enabled());
    }

    #[test]
    fn announcements_are_labelled_plain_text() {
        assert_eq!(announce_tool_request("shell"), "Tool call: shell");
        assert_eq!(announce_tool_response(), "Result:");
    }

    #[test]
    fn glyph_or_word_switches_on_mode() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _g = clean_env();

        assert_eq!(glyph_or_word("\u{25B8}", "tool"), "\u{25B8}");

        std::env::set_var(ENABLE_KEY, "1");
        assert_eq!(glyph_or_word("\u{25B8}", "tool"), "tool");
    }

    #[test]
    fn label_maps_roles_to_announceable_markers() {
        assert_eq!(label("assistant"), "Assistant:");
        assert_eq!(label("ASSISTANT"), "Assistant:");
        assert_eq!(label("tool"), "Tool:");
        assert_eq!(label("tool_result"), "Result:");
        assert_eq!(label("user"), "User:");
        // Unknown roles fall back to a generic marker, never the raw token.
        assert_eq!(label("banana"), "Output:");
    }

    #[test]
    fn plain_strips_ansi_escapes() {
        // A SGR-green colored string with a reset; plain() removes both escapes.
        let colored = format!("{}OK text{}", "\u{1b}[32m", "\u{1b}[0m");
        let cleaned = plain(&colored);
        assert_eq!(cleaned, "OK text");
        assert!(!cleaned.contains('\u{1b}'));
    }

    #[test]
    fn plain_maps_decorative_glyphs_to_ascii_words() {
        assert_eq!(plain("\u{2713} done"), "OK done");
        assert_eq!(plain("\u{2717} failed"), "FAILED failed");
        assert_eq!(plain("a \u{2192} b"), "a -> b");
        assert_eq!(plain("loading\u{2026}"), "loading...");
    }

    #[test]
    fn plain_combines_ansi_and_glyph_stripping() {
        let styled = format!("{}\u{2713} all good{}", "\u{1b}[1;32m", "\u{1b}[0m");
        assert_eq!(plain(&styled), "OK all good");
    }

    #[test]
    fn plain_borrows_when_nothing_to_change() {
        // No ANSI, no decorative glyphs => zero-copy borrow, byte-identical.
        let s = "plain ascii line";
        assert!(matches!(plain(s), Cow::Borrowed(_)));
        assert_eq!(plain(s), s);
    }

    #[test]
    fn plainify_drops_spinner_and_asciifies_ellipsis() {
        // The canonical v85 invariant: a Braille spinner frame (U+280B) plus a
        // unicode ellipsis (U+2026) flatten to plain ASCII with no glyph.
        assert_eq!(plainify("\u{280B} working\u{2026}"), "working...");
        assert!(plainify("\u{280B} working\u{2026}").is_ascii());
    }

    #[test]
    fn plainify_strips_box_drawing_and_emoji() {
        // Box-drawing rule + status bullet + bird emoji around the message.
        let decorated = "\u{2500}\u{2500} \u{25CF} \u{1F426} ready \u{2502}";
        assert_eq!(plainify(decorated), "ready");
    }

    #[test]
    fn plainify_rewrites_meaningful_glyphs() {
        assert_eq!(plainify("\u{2713} done"), "OK done");
        assert_eq!(plainify("a \u{2192} b"), "a -> b");
    }

    #[test]
    fn plainify_collapses_ansi_and_whitespace() {
        let styled = format!("{}  \u{25CF}   ready  {}", "\u{1b}[1;32m", "\u{1b}[0m");
        assert_eq!(plainify(&styled), "ready");
    }

    #[test]
    fn profile_from_env_unset_is_all_false() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _g = clean_env();
        let p = A11yProfile::from_env();
        assert!(!p.screen_reader);
        assert!(!p.no_spinner);
        assert!(!p.any());
        assert_eq!(p, A11yProfile::default());
    }

    #[test]
    fn profile_from_env_reflects_set_flag() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _g = clean_env();
        std::env::set_var(ENABLE_KEY, "1");
        let p = A11yProfile::from_env();
        assert!(p.screen_reader);
        assert!(p.no_spinner);
        assert!(p.any());
    }

    #[test]
    fn is_enabled_reflects_set_env_under_lock() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _g = clean_env();
        assert!(!is_enabled());
        std::env::set_var(ENABLE_KEY, "1");
        assert!(is_enabled());
        std::env::set_var(ENABLE_KEY, "0");
        assert!(!is_enabled());
    }
}
