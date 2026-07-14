//! BharatCode v82: right-prompt language indicator + quick locale switch.
//!
//! Renders a compact, themed locale badge (e.g. `[hi]`/`[ta]`/`[mr]`) at the
//! interactive prompt so users always know which language the CLI is rendering,
//! and parses an interactive `/lang <code>` slash command that switches the
//! active locale for the remainder of the session.
//!
//! The whole feature is gated on a non-`en` locale being active: a default
//! English session shows no badge, so its prompt is byte-for-byte identical to
//! upstream. The active locale is read live from the `BHARATCODE_LANG`
//! environment variable (the same key the i18n resolver consults first), so a
//! `/lang` switch performed mid-session is reflected on the next prompt render
//! without restarting the process.

/// Normalize a raw locale token to its primary subtag in lowercase.
///
/// Splits on the first locale separator so values like `hi_IN.UTF-8`, `ta-IN`
/// or `MR` collapse to `hi`, `ta`, `mr`. An empty or whitespace-only token
/// yields an empty string (treated as "unset" by [`badge`]).
fn primary_subtag(raw: &str) -> String {
    raw.trim()
        .to_ascii_lowercase()
        .split(['_', '-', '.'])
        .next()
        .unwrap_or("")
        .to_string()
}

/// Build the compact prompt badge for `locale_tag`.
///
/// Returns an empty string for English or an unset/blank locale so the default
/// English prompt is unchanged. For any other locale it returns a
/// `NO_COLOR`-respecting `[xx]` painted via [`crate::theme::muted`], where `xx`
/// is the normalized primary subtag (e.g. `[hi]`, `[ta]`, `[mr]`).
pub fn badge(locale_tag: &str) -> String {
    let code = primary_subtag(locale_tag);
    if code.is_empty() || code == "en" {
        return String::new();
    }
    // `theme::muted` already collapses to a plain (uncolored) style when
    // `NO_COLOR` is set or the active theme is `none`, so the badge respects
    // NO_COLOR for free.
    format!("{}", crate::theme::muted(format!("[{code}]")))
}

/// Parse an interactive `/lang <code>` slash command.
///
/// Recognizes a line whose first whitespace-delimited token is exactly `/lang`
/// followed by a single locale code, and returns the normalized primary subtag
/// (e.g. `parse_lang_command("/lang ta") == Some("ta")`). Returns `None` for any
/// non-matching line, including a bare `/lang` with no argument.
pub fn parse_lang_command(line: &str) -> Option<String> {
    let mut parts = line.split_whitespace();
    if parts.next()? != "/lang" {
        return None;
    }
    let code = primary_subtag(parts.next()?);
    if code.is_empty() || parts.next().is_some() {
        return None;
    }
    Some(code)
}

/// Resolve the locale tag the prompt should currently advertise.
///
/// Reads `BHARATCODE_LANG` live (not the process-cached i18n resolver) so a
/// `/lang` switch performed earlier in this session is reflected immediately.
/// Returns an empty string when unset, which [`badge`] renders as no badge.
fn active_locale_tag() -> String {
    std::env::var("BHARATCODE_LANG").unwrap_or_default()
}

/// Decorate the inline prompt `label` with the active locale badge.
///
/// For an English/unset session this returns `label` unchanged (no allocation
/// of trailing decoration is observable), keeping the default prompt identical.
/// Otherwise the badge is appended after the label, e.g. `"> "` -> `"> [hi] "`.
pub fn prompt_with_badge(label: &str) -> String {
    let badge = badge(&active_locale_tag());
    if badge.is_empty() {
        return label.to_string();
    }
    format!("{label}{badge} ")
}

/// Apply a `/lang <code>` switch for the remainder of the session.
///
/// Writes the normalized `code` into the `BHARATCODE_LANG` environment variable
/// so the existing locale lookups (and [`prompt_with_badge`]) pick it up on the
/// next turn, and returns a short confirmation line (display name plus the new
/// `[code]` badge) to echo back to the user.
pub fn apply_lang_switch(code: &str) -> String {
    std::env::set_var("BHARATCODE_LANG", code);
    confirmation(code)
}

/// Build a "switched to <language>" confirmation for `code`.
///
/// Reuses the existing `lang.name.<code>` i18n keys (present in en/hi/ta). When
/// no display-name key is registered for `code`, [`crate::i18n::t`] returns the
/// key itself, in which case we fall back to the raw code so the message still
/// reads sensibly. No new translation keys are required.
fn confirmation(code: &str) -> String {
    let name_key = format!("lang.name.{code}");
    let name = crate::i18n::t(&name_key);
    let name = if name == name_key {
        code.to_string()
    } else {
        name
    };
    let label = crate::tr!("lang.row_label");
    format!("{}: {name} {}", crate::theme::muted(label), badge(code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lang_command_recognizes_codes() {
        assert_eq!(parse_lang_command("/lang mr"), Some("mr".to_string()));
        assert_eq!(parse_lang_command("/lang ta"), Some("ta".to_string()));
        assert_eq!(parse_lang_command("  /lang hi  "), Some("hi".to_string()));
        assert_eq!(
            parse_lang_command("/lang hi_IN.UTF-8"),
            Some("hi".to_string())
        );
        assert_eq!(parse_lang_command("/lang EN"), Some("en".to_string()));
    }

    #[test]
    fn parse_lang_command_rejects_non_matches() {
        assert_eq!(parse_lang_command("hello"), None);
        assert_eq!(parse_lang_command("/lang"), None);
        assert_eq!(parse_lang_command("/lang "), None);
        assert_eq!(parse_lang_command("/language ta"), None);
        assert_eq!(parse_lang_command("/lang ta mr"), None);
        assert_eq!(parse_lang_command(""), None);
    }

    #[test]
    fn badge_is_empty_for_english_and_unset() {
        assert_eq!(badge("en"), "");
        assert_eq!(badge("en_US.UTF-8"), "");
        assert_eq!(badge(""), "");
        assert_eq!(badge("   "), "");
    }

    #[test]
    fn badge_contains_subtag_for_regional_locale() {
        assert!(badge("ta").contains("ta"));
        assert!(badge("hi").contains("hi"));
        assert!(badge("mr").contains("mr"));
        // Region/encoding suffixes collapse to the primary subtag.
        assert!(badge("ta_IN.UTF-8").contains("ta"));
        assert!(!badge("ta_IN.UTF-8").contains("IN"));
    }

    #[test]
    fn prompt_with_badge_leaves_english_prompt_unchanged() {
        // Force an English/unset locale for this assertion.
        std::env::remove_var("BHARATCODE_LANG");
        assert_eq!(prompt_with_badge("> "), "> ");
    }
}
