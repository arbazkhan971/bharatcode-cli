//! Trademark / compliance self-scan on agent turn finalization (opt-in).
//!
//! When enabled, the agent loop runs this scanner over the *just-produced*
//! finalized assistant text and, if any residual third-party trademarks/marks
//! survive into user-facing output, emits a single inline compliance advisory
//! naming the distinct marks found. This is the final user-facing trademark
//! gate, running live in the agent loop rather than only in offline tests.
//!
//! The whole feature is gated behind [`is_enabled`] and is **off by default**,
//! so when the operator does nothing the finalization path is byte-identical to
//! before: [`scan_output`] is only ever consulted behind the env gate, and the
//! advisory is emitted only when there is at least one genuine hit.
//!
//! Scope and intent
//! ----------------
//! This is distinct from the sibling [`crate::compliance`] module:
//!   * `compliance` carries the always-on Apache-2.0 attribution footer plus a
//!     *phrase*-based denylist (`created by Block`, `You are Goose`, …).
//!   * `compliance_gate` (this module) is a *whole-word, token*-level scan over
//!     the bare marks (Goose / Block / Square / CashApp / Codex / OpenAI /
//!     ChatGPT) so a stray brand token that leaks into a code comment, an error
//!     echo, or a quoted upstream string is caught even when it is not part of
//!     one of the curated phrases.
//!
//! Allow / deny matrix (documented and asserted in the unit tests)
//! --------------------------------------------------------------
//! Matching is whole-word (`\b…\b`) and case-insensitive. Word boundaries alone
//! already exclude legitimate technical tokens that merely *contain* a mark as a
//! substring of a larger identifier, and an explicit allow-list handles the
//! remaining generic English words:
//!
//! | Input fragment        | Flagged? | Why                                       |
//! |-----------------------|----------|-------------------------------------------|
//! | `Use Goose to ...`    | yes      | bare `Goose` brand token                  |
//! | `created by Block`    | yes      | bare `Block` brand token                  |
//! | `ContentBlock`        | no       | `Block` is a substring, no word boundary  |
//! | `Blockchain`          | no       | `Block` is a substring, no word boundary  |
//! | `a registry block`    | no       | generic lowercase `block` (allow-list)    |
//! | `blocked`             | no       | `blocked` != `block`; no boundary match   |
//! | `OpenAI` / `ChatGPT`  | yes      | bare provider/product marks               |
//! | `CashApp` / `Square`  | yes      | bare Block-family product marks           |
//!
//! Enable it with the `BHARATCODE_COMPLIANCE_GATE` boolean (truthy env value).
//!
//! This module is original work; nothing here is ported from third-party
//! sources.

use regex::Regex;
use std::sync::LazyLock;

/// Opt-in toggle name, read raw from the process environment. Off by default:
/// only a truthy value turns the gate on.
const ENABLE_KEY: &str = "BHARATCODE_COMPLIANCE_GATE";

/// One canonical mark plus the whole-word, case-insensitive pattern that
/// detects it. The `label` is the form shown in the advisory; the `pattern`
/// drives detection.
struct Mark {
    label: &'static str,
    pattern: Regex,
}

/// The set of third-party marks this gate guards against. Each pattern is
/// anchored on word boundaries so it never matches inside a larger identifier
/// (e.g. `ContentBlock`, `Blockchain`), and is case-insensitive so `goose`,
/// `Goose` and `GOOSE` are all caught. `CashApp`/`ChatGPT`/`OpenAI` are matched
/// as their compact brand spellings.
static MARKS: LazyLock<Vec<Mark>> = LazyLock::new(|| {
    fn mark(label: &'static str, body: &str) -> Mark {
        Mark {
            label,
            // (?i) case-insensitive; \b…\b whole-word so substrings of larger
            // identifiers are never flagged.
            pattern: Regex::new(&format!(r"(?i)\b{body}\b")).expect("static compliance regex"),
        }
    }
    vec![
        mark("Goose", "goose"),
        mark("Block", "block"),
        mark("Square", "square"),
        mark("CashApp", "cash ?app"),
        mark("Codex", "codex"),
        mark("OpenAI", "open ?ai"),
        mark("ChatGPT", "chat ?gpt"),
    ]
});

/// Lowercased generic words/phrases that *look like* a mark under whole-word,
/// case-insensitive matching but are legitimate technical or English usage and
/// must never be flagged. Word boundaries already exempt `ContentBlock` and
/// `Blockchain` (the mark is a substring there, not a whole word); this list
/// covers the standalone generic words such as the lowercase noun `block`.
///
/// A candidate hit is suppressed when the *exact matched span* (lowercased) is
/// one of these tokens. Because the brand forms we care about are capitalized
/// (`Block`, the company) we suppress only the all-lowercase generic spelling,
/// so `created by Block` is still flagged while `a registry block` is not.
const ALLOW_LIST: &[&str] = &["block", "square"];

/// Scan `text` and return the distinct third-party marks it contains, in the
/// canonical order of [`MARKS`]. Returns an empty vector for clean text.
///
/// Matching is whole-word and case-insensitive; allow-listed generic lowercase
/// spellings (see [`ALLOW_LIST`]) are not reported. This is pure and has no
/// dependency on the enable flag, so it is cheap to unit-test directly.
pub fn scan(text: &str) -> Vec<String> {
    let mut hits: Vec<String> = Vec::new();
    for mark in MARKS.iter() {
        let mut counts_as_hit = false;
        for m in mark.pattern.find_iter(text) {
            let span = m.as_str().to_ascii_lowercase();
            // Suppress allow-listed generic lowercase spellings (e.g. the noun
            // "block" in "a registry block"). A capitalized brand spelling in
            // the source text (e.g. "Block") is *not* all-lowercase and so is
            // still counted.
            if ALLOW_LIST.contains(&span.as_str()) && m.as_str() == span {
                continue;
            }
            counts_as_hit = true;
            break;
        }
        if counts_as_hit && !hits.iter().any(|h| h == mark.label) {
            hits.push(mark.label.to_string());
        }
    }
    hits
}

/// Whether the finalization-time compliance gate is enabled. **Off by default.**
///
/// Reads the raw `BHARATCODE_COMPLIANCE_GATE` environment variable first
/// (truthy values only), then falls back to the global config parameter of the
/// same name, then defaults to `false`.
pub fn is_enabled() -> bool {
    if let Ok(raw) = std::env::var(ENABLE_KEY) {
        return is_truthy(&raw);
    }
    crate::config::Config::global()
        .get_param::<bool>(ENABLE_KEY)
        .unwrap_or(false)
}

fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Build the single user-facing advisory line for a non-empty set of marks.
///
/// The advisory deliberately contains the literal word `trademark` and the
/// comma-joined hit list, and nothing else — in particular it never names the
/// gate itself or any upstream beyond the marks actually found, so the advisory
/// introduces no spurious self-reference.
fn advisory_line(marks: &[String]) -> String {
    format!("{} {}", label(Label::Prefix), marks.join(", "))
}

/// Scan finalized assistant `text` for residual third-party trademarks and
/// return a ready-to-emit advisory string, or `None` when the output is clean.
///
/// This is the single entry point wired into the agent finalization path: the
/// caller guards it with [`is_enabled`] and emits the returned string (when
/// `Some`) as one `InlineMessage`. Returning `None` for clean text means the
/// common case adds no output at all.
pub fn scan_output(text: &str) -> Option<String> {
    let marks = scan(text);
    if marks.is_empty() {
        return None;
    }
    Some(advisory_line(&marks))
}

// ----------------------------------------------------------------------------
// Localization for the single user-facing advisory prefix. Mirrors the
// project's existing self-contained locale scaffold
// (`BHARATCODE_LANG` → `bharatcode_lang` config → `LANG` → English).
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
enum Locale {
    En,
    Hi,
}

#[derive(Debug, Clone, Copy)]
enum Label {
    Prefix,
}

fn label(which: Label) -> String {
    let s = match (active_locale(), which) {
        (Locale::En, Label::Prefix) => {
            "Compliance advisory: output references third-party trademark(s):"
        }
        (Locale::Hi, Label::Prefix) => "अनुपालन सूचना: आउटपुट में तृतीय-पक्ष ट्रेडमार्क (trademark) मौजूद है:",
    };
    s.to_string()
}

fn normalize_locale(raw: &str) -> Locale {
    let lowered = raw.trim().to_ascii_lowercase();
    let primary = lowered
        .split(|c| c == '_' || c == '-' || c == '.')
        .next()
        .unwrap_or("");
    match primary {
        "hi" => Locale::Hi,
        _ => Locale::En,
    }
}

fn active_locale() -> Locale {
    if let Some(loc) = std::env::var("BHARATCODE_LANG")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        return normalize_locale(&loc);
    }
    if let Ok(loc) = crate::config::Config::global().get_param::<String>("bharatcode_lang") {
        if !loc.trim().is_empty() {
            return normalize_locale(&loc);
        }
    }
    if let Some(loc) = std::env::var("LANG").ok().filter(|s| !s.trim().is_empty()) {
        return normalize_locale(&loc);
    }
    Locale::En
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialise tests that mutate the shared process env so the toggle does not
    /// race across threads (and force English so locale env does not perturb
    /// advisory-string assertions).
    fn env_guard<'a>(enable: Option<&'a str>) -> env_lock::EnvGuard<'a> {
        env_lock::lock_env([
            (ENABLE_KEY, enable),
            ("BHARATCODE_LANG", Some("en")),
            ("LANG", Some("en_US.UTF-8")),
        ])
    }

    #[test]
    fn disabled_by_default_when_unset() {
        let _guard = env_guard(None);
        assert!(!is_enabled(), "gate must be off when the env var is unset");
    }

    #[test]
    fn enabled_only_for_truthy_values() {
        for on in ["1", "true", "yes", "on", " ON "] {
            let _guard = env_guard(Some(on));
            assert!(is_enabled(), "expected {on:?} to enable the gate");
        }
        for off in ["0", "false", "no", "off", ""] {
            let _guard = env_guard(Some(off));
            assert!(!is_enabled(), "expected {off:?} to leave the gate off");
        }
    }

    #[test]
    fn scan_flags_goose_brand_token() {
        assert_eq!(
            scan("Use Goose to do the thing."),
            vec!["Goose".to_string()]
        );
    }

    #[test]
    fn scan_is_case_insensitive_and_distinct() {
        // Mixed case + repeats collapse to one distinct, canonical label.
        let hits = scan("goose and GOOSE and Goose");
        assert_eq!(hits, vec!["Goose".to_string()]);
    }

    #[test]
    fn scan_allow_list_exempts_technical_and_generic_block() {
        // ContentBlock: Block is a substring, not a whole word -> not flagged.
        // "registry block": generic lowercase noun -> allow-listed -> not flagged.
        assert!(
            scan("ContentBlock and a registry block").is_empty(),
            "allow-listed/technical tokens must not be flagged"
        );
    }

    #[test]
    fn scan_does_not_flag_blockchain_or_blocked() {
        // Whole-word matching: `Block` is a substring of these, so neither is a hit.
        assert!(scan("Blockchain is blocked by the firewall").is_empty());
    }

    #[test]
    fn scan_still_flags_capitalized_block_company() {
        // The capitalized brand spelling is *not* the all-lowercase generic word,
        // so it is still reported.
        let hits = scan("created by Block");
        assert_eq!(hits, vec!["Block".to_string()]);
    }

    #[test]
    fn scan_flags_provider_and_product_marks() {
        let hits = scan("Powered by OpenAI's ChatGPT and the Codex donor; see CashApp.");
        for expected in ["Codex", "OpenAI", "ChatGPT", "CashApp"] {
            assert!(
                hits.iter().any(|h| h == expected),
                "expected {expected:?} in {hits:?}"
            );
        }
        // None of the technical/allow-listed tokens snuck in.
        assert!(!hits.iter().any(|h| h == "Block"));
    }

    #[test]
    fn scan_clean_text_returns_empty() {
        assert!(scan("A perfectly clean sentence about widgets and parsers.").is_empty());
    }

    #[test]
    fn scan_output_none_for_clean_text() {
        assert_eq!(
            scan_output("A perfectly clean sentence about widgets."),
            None
        );
    }

    #[test]
    fn advisory_contains_trademark_word_and_hit_list_without_self_reference() {
        let _guard = env_guard(None);
        let advisory = scan_output("Use Goose, see OpenAI.").expect("expected an advisory");
        // Contains the literal "trademark".
        assert!(
            advisory.to_ascii_lowercase().contains("trademark"),
            "advisory must mention 'trademark': {advisory}"
        );
        // Contains every hit mark.
        assert!(
            advisory.contains("Goose"),
            "advisory must list Goose: {advisory}"
        );
        assert!(
            advisory.contains("OpenAI"),
            "advisory must list OpenAI: {advisory}"
        );
        // No spurious self-reference: the gate never names itself or the env var.
        let lowered = advisory.to_ascii_lowercase();
        assert!(
            !lowered.contains("compliance_gate"),
            "no module self-ref: {advisory}"
        );
        assert!(
            !lowered.contains("bharatcode_compliance_gate"),
            "no env-var self-ref: {advisory}"
        );
    }

    #[test]
    fn normalize_locale_maps_hindi_variants() {
        assert!(matches!(normalize_locale("hi"), Locale::Hi));
        assert!(matches!(normalize_locale("hi_IN.UTF-8"), Locale::Hi));
        assert!(matches!(normalize_locale("en_US"), Locale::En));
        assert!(matches!(normalize_locale("fr"), Locale::En));
    }
}
