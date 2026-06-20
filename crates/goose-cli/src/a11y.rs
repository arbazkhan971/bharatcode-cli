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
/// Enabled when `BHARATCODE_A11Y` holds a truthy value, or when `NO_COLOR` is
/// present in the environment (the `NO_COLOR` convention implies a plain-text
/// preference). Reads the raw environment directly so the mode can be toggled
/// per-process without touching persisted config.
pub fn is_enabled() -> bool {
    if let Ok(raw) = std::env::var(ENABLE_KEY) {
        if is_truthy(&raw) {
            return true;
        }
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

/// Map a streaming role to an ASCII, colon-terminated, screen-reader-announceable
/// marker. Unknown roles fall back to `Output:` so a raw token never leaks. (v83)
pub fn label(role: &str) -> &'static str {
    match role {
        "assistant" => "Assistant:",
        "tool" => "Tool:",
        "tool_result" | "result" => "Result:",
        "user" => "User:",
        "system" => "System:",
        "thinking" => "Thinking:",
        "error" => "Error:",
        _ => "Output:",
    }
}

/// Decorative glyphs paired with the ASCII word [`plain`] rewrites them to. (v83)
const GLYPH_WORDS: &[(char, &str)] = &[
    ('\u{2713}', "OK"),
    ('\u{2717}', "FAILED"),
    ('\u{2718}', "FAILED"),
    ('\u{2705}', "OK"),
    ('\u{274C}', "FAILED"),
    ('\u{2192}', "->"),
    ('\u{25B8}', ">"),
    ('\u{25B6}', ">"),
    ('\u{2026}', "..."),
    ('\u{25CF}', "*"),
    ('\u{25CB}', "o"),
    ('\u{2022}', "-"),
];

/// Strip ANSI escapes and rewrite decorative glyphs to ASCII so output is
/// linear and screen-reader-announceable. Borrows when nothing changes. (v83)
pub fn plain(s: &str) -> std::borrow::Cow<'_, str> {
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
    std::borrow::Cow::Owned(out)
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
}
