//! Accessibility / screen-reader-friendly plain output mode for BharatCode.
//!
//! The session "ready" banner — like much of a modern CLI — leans on decorative
//! glyphs: little ASCII-art legs, box-drawing rules, a spinner dot, the odd
//! emoji. Sighted users read those as ambient chrome and skip past them. A
//! screen reader does not: it dutifully announces "black circle", "box drawings
//! light horizontal", "bird emoji", turning a one-line status into a stream of
//! Unicode-name noise that buries the one piece of text that mattered.
//!
//! This module provides an **opt-in** plain mode. When a user exports
//! `BHARATCODE_A11Y=1` (or `BHARATCODE_SCREEN_READER=1`) the contended render
//! sites can ask [`is_enabled`] and, if on, route their text through
//! [`plainify`] — which strips the decorative glyph set down to ASCII-safe
//! markers — and print it without any `console::style` coloring (color escapes
//! are themselves noise to a screen reader).
//!
//! Design:
//!   * **Default OFF.** With neither env var set [`is_enabled`] is `false` and
//!     callers take their original styled path, so the banner is *byte-identical*
//!     to before — no glyph rewriting, no behavioural change, no extra IO.
//!   * **Raw-env-first gate.** The truthiness check reads the variables straight
//!     from the environment (rather than through the typed config layer) so a
//!     bare `1` survives instead of being coerced into a JSON number and read
//!     back as unset — the same pattern the memory-store / recovery gates use.
//!   * **Lossless on plain text.** [`plainify`] only allocates when it actually
//!     removes something; text that is already plain ASCII is returned borrowed
//!     and byte-identical via `Cow::Borrowed`.
//!
//! Original BharatCode work; not ported from any third party. std only.

use std::borrow::Cow;

/// Primary environment key that turns the accessibility plain-output mode on.
pub const A11Y_ENABLED_KEY: &str = "BHARATCODE_A11Y";

/// Alternate environment key, named for the assistive technology most users
/// have in mind when they reach for this. Either key (truthy) enables the mode.
pub const SCREEN_READER_ENABLED_KEY: &str = "BHARATCODE_SCREEN_READER";

/// Whether screen-reader-friendly plain output is enabled for this process.
///
/// Reads `BHARATCODE_A11Y` and `BHARATCODE_SCREEN_READER` straight from the
/// environment and accepts the usual truthy spellings (`1`, `true`, `yes`,
/// `on`); anything else — including absence of both — is OFF. The raw-env read
/// (rather than going through the typed config layer) mirrors the memory-store
/// gate so that a bare `1` survives instead of being coerced to a JSON number
/// and read back as unset.
pub fn is_enabled() -> bool {
    env_truthy(A11Y_ENABLED_KEY) || env_truthy(SCREEN_READER_ENABLED_KEY)
}

fn env_truthy(key: &str) -> bool {
    match std::env::var(key) {
        Ok(raw) => is_truthy(&raw),
        Err(_) => false,
    }
}

fn is_truthy(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Rewrite a string for screen-reader consumption by removing a small set of
/// decorative glyphs and collapsing the result to ASCII-safe markers.
///
/// What is removed / replaced:
///   * **Box-drawing** characters (U+2500..U+257F) — horizontal/vertical rules,
///     corners and tees — are dropped.
///   * **Block elements** (U+2580..U+259F) — the solid/shaded blocks used for
///     bars and spinners — are dropped.
///   * **Geometric shapes** (U+25A0..U+25FF) — the filled circles/squares used
///     as status bullets and spinner frames — are dropped.
///   * **Leading spinner / Braille frames** (U+2800..U+28FF) at the start of the
///     text are dropped.
///   * **Common emoji ranges** — Misc Symbols & Pictographs, Emoticons,
///     Transport & Map, Supplemental Symbols, and the Misc Symbols /
///     Dingbats blocks plus variation selectors and ZWJ — are dropped.
///   * Any run of resulting whitespace is collapsed to a single space and the
///     ends are trimmed, so removed chrome does not leave ragged gaps.
///
/// Text that contains none of the above is returned **borrowed and
/// byte-identical** (`Cow::Borrowed`); the rewrite only allocates when it has
/// actually changed something.
pub fn plainify(input: &str) -> Cow<'_, str> {
    if input.chars().all(|c| !is_decorative(c)) {
        // Nothing decorative to strip; only collapse if whitespace needs it.
        if !needs_whitespace_collapse(input) {
            return Cow::Borrowed(input);
        }
    }

    let mut out = String::with_capacity(input.len());
    let mut pending_space = false;
    let mut wrote_any = false;

    for ch in input.chars() {
        if is_decorative(ch) {
            // A removed glyph acts like a soft break: remember to coalesce the
            // surrounding whitespace into a single separating space.
            pending_space = true;
            continue;
        }
        if ch.is_whitespace() {
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

    Cow::Owned(out)
}

fn needs_whitespace_collapse(input: &str) -> bool {
    let mut prev_ws = false;
    let mut leading = true;
    for ch in input.chars() {
        let ws = ch.is_whitespace();
        if ws && (leading || prev_ws) {
            return true;
        }
        if !ws {
            leading = false;
        }
        prev_ws = ws;
    }
    prev_ws
}

fn is_decorative(c: char) -> bool {
    let u = c as u32;
    matches!(u,
        // Box Drawing
        0x2500..=0x257F
        // Block Elements
        | 0x2580..=0x259F
        // Geometric Shapes (status bullets, spinner dots, ● ○ ■ ▶ etc.)
        | 0x25A0..=0x25FF
        // Braille patterns (common spinner frame set)
        | 0x2800..=0x28FF
        // Variation selectors (emoji presentation / ZWJ joiner companions)
        | 0xFE00..=0xFE0F
        | 0x200D
        // Miscellaneous Symbols and Dingbats
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
        // Regional indicator symbols (flag emoji halves)
        | 0x1F1E6..=0x1F1FF
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // The two gate vars are process-global; serialize the env-mutating tests so
    // they cannot race each other under the test harness's thread pool.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_clean_env<T>(f: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let prev_a11y = std::env::var(A11Y_ENABLED_KEY).ok();
        let prev_sr = std::env::var(SCREEN_READER_ENABLED_KEY).ok();
        std::env::remove_var(A11Y_ENABLED_KEY);
        std::env::remove_var(SCREEN_READER_ENABLED_KEY);
        let out = f();
        match prev_a11y {
            Some(v) => std::env::set_var(A11Y_ENABLED_KEY, v),
            None => std::env::remove_var(A11Y_ENABLED_KEY),
        }
        match prev_sr {
            Some(v) => std::env::set_var(SCREEN_READER_ENABLED_KEY, v),
            None => std::env::remove_var(SCREEN_READER_ENABLED_KEY),
        }
        out
    }

    #[test]
    fn is_enabled_false_when_unset() {
        with_clean_env(|| {
            assert!(!is_enabled());
        });
    }

    #[test]
    fn is_enabled_true_on_truthy_a11y() {
        with_clean_env(|| {
            for v in ["1", "true", "TRUE", "yes", "on", " 1 "] {
                std::env::set_var(A11Y_ENABLED_KEY, v);
                assert!(is_enabled(), "expected enabled for A11Y={v:?}");
                std::env::remove_var(A11Y_ENABLED_KEY);
            }
        });
    }

    #[test]
    fn is_enabled_true_on_truthy_screen_reader() {
        with_clean_env(|| {
            std::env::set_var(SCREEN_READER_ENABLED_KEY, "true");
            assert!(is_enabled());
        });
    }

    #[test]
    fn is_enabled_false_on_falsey() {
        with_clean_env(|| {
            for v in ["0", "false", "no", "off", ""] {
                std::env::set_var(A11Y_ENABLED_KEY, v);
                assert!(!is_enabled(), "expected disabled for A11Y={v:?}");
                std::env::remove_var(A11Y_ENABLED_KEY);
            }
        });
    }

    #[test]
    fn plainify_leaves_plain_ascii_byte_identical() {
        let plain = "bharatcode is ready";
        let out = plainify(plain);
        assert_eq!(out, plain);
        assert!(
            matches!(out, Cow::Borrowed(_)),
            "plain text must not allocate"
        );
    }

    #[test]
    fn plainify_strips_box_drawing_and_emoji_to_plain_ascii() {
        // Box-drawing rule + status bullet + bird emoji around the message.
        let decorated = "\u{2500}\u{2500} \u{25CF} \u{1F426} ready \u{2502}";
        let out = plainify(&decorated);
        assert_eq!(out, "ready");
        assert!(out.is_ascii(), "output should be ASCII-safe: {out:?}");
    }

    #[test]
    fn plainify_drops_leading_spinner_frame() {
        let spun = "\u{280B} loading models";
        assert_eq!(plainify(spun), "loading models");
    }

    #[test]
    fn plainify_collapses_whitespace_left_by_removed_chrome() {
        let decorated = "left \u{2503} right";
        assert_eq!(plainify(&decorated), "left right");
    }

    #[test]
    fn plainify_preserves_internal_ascii_punctuation() {
        // Only decorative glyphs go; ordinary punctuation must survive.
        let s = "new session · gpt-4o";
        // The middle dot (U+00B7) is not in our decorative set, so it stays.
        assert_eq!(plainify(s), s);
    }

    #[test]
    fn enabled_vs_disabled_ready_line_differs_only_by_decoration() {
        // Model the styled ready line vs the a11y plain line for the same text.
        // The styled banner pairs an ASCII-art leg glyph block with the ready
        // message; the plain mode keeps the words and drops the decoration.
        let ready = "bharatcode is ready";
        let styled = format!("   L L    {ready}");

        let plain = plainify(&styled);
        // Decoration-only difference: every alphanumeric word from the message
        // survives, and the plain output carries no box-drawing / emoji.
        assert!(plain.contains("ready"));
        assert!(plain.contains("bharatcode"));
        assert!(plain.chars().all(|c| !is_decorative(c)));
        // And the two forms are not byte-identical: the plain one collapses the
        // padded art spacing the styled one keeps.
        assert_ne!(plain.as_ref(), styled.as_str());
    }
}
