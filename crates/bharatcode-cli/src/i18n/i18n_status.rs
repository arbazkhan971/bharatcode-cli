//! BharatCode v90: i18n coverage report (`bharatcode i18n-status`).
//!
//! Measures how completely each regional locale (`hi`, `ta`) translates the
//! canonical English key set in `en.json`, so en/hi/ta parity is *measurable*
//! and a missing translation is caught loudly instead of silently falling back
//! to English at runtime.
//!
//! The report shares the exact same `LazyLock` locale tables the running CLI
//! consults through [`crate::i18n::t`] (via the [`crate::i18n::locale_table_keys`]
//! and [`crate::i18n::locale_has_key`] accessors), so what this command prints is
//! precisely what the binary would resolve — there is no second copy of the
//! bundled data.
//!
//! Wiring: this is reachable in the running binary as the interactive `/i18n`
//! slash command (see `crate::session::input`); the user-facing command name is
//! `i18n-status`. It is purely a read-only report — it does not mutate any
//! config or locale state — so it carries no env gate and does not alter default
//! behaviour.

use crate::i18n::{locale_has_key, locale_table_keys, Locale};

/// The regional locales whose coverage against English this report measures.
///
/// English is the source of truth (always 100% of itself), so it is excluded;
/// the parity story is about how much of `en.json` `hi`/`ta` translate.
const REPORTED_LOCALES: &[(Locale, &str)] = &[(Locale::Hi, "hi"), (Locale::Ta, "ta")];

/// Per-locale translation coverage against the canonical English key set.
///
/// Returns one `(locale_tag, translated, total)` row per regional locale, where
/// `total` is the number of keys defined in `en.json` and `translated` is how
/// many of those keys the locale's own table carries with a non-empty value.
/// `translated == total` means full parity (100%).
pub fn coverage() -> Vec<(&'static str, usize, usize)> {
    let en_keys = locale_table_keys(Locale::En);
    let total = en_keys.len();
    REPORTED_LOCALES
        .iter()
        .map(|(locale, tag)| {
            let translated = en_keys
                .iter()
                .filter(|key| locale_has_key(*locale, key))
                .count();
            (*tag, translated, total)
        })
        .collect()
}

/// English keys that `locale` does NOT translate (missing or empty in its own
/// table), sorted for stable output.
fn missing_keys(locale: Locale) -> Vec<&'static str> {
    locale_table_keys(Locale::En)
        .into_iter()
        .filter(|key| !locale_has_key(locale, key))
        .collect()
}

/// Format a coverage ratio as a whole-number percentage string (e.g. `100%`).
fn pct(translated: usize, total: usize) -> String {
    if total == 0 {
        return "100%".to_string();
    }
    format!("{}%", (translated * 100) / total)
}

/// Print a localized i18n coverage table for every regional locale.
///
/// Read-only: reports the current parity between `en.json` and each locale table
/// and, when a locale is below 100%, lists the specific keys it is missing so the
/// gap can be closed. Returns `Ok(())` unconditionally — an incomplete locale is
/// reported, not treated as a process error.
pub fn handle_i18n_status() -> anyhow::Result<()> {
    println!("{}", crate::theme::heading(crate::tr!("i18n_status.title")));
    println!();

    let rows = coverage();

    // Header row: width specifiers are applied to the *plain* (uncolored)
    // localized labels, then the whole line is muted as one styled object, so the
    // column widths stay correct regardless of whether colour is active. (Applying
    // a width specifier directly to an already-ANSI-painted string would count the
    // escape bytes and misalign the columns — see crate::commands::cost.)
    let header = format!(
        "  {:<8} {:>14} {:>14}   {}",
        crate::tr!("i18n_status.col_locale"),
        crate::tr!("i18n_status.col_translated"),
        crate::tr!("i18n_status.col_total"),
        crate::tr!("i18n_status.col_coverage"),
    );
    println!("{}", crate::theme::muted(header));

    let mut all_complete = true;
    for (tag, translated, total) in &rows {
        let ratio = pct(*translated, *total);
        // The plain numeric columns carry the width; the coloured percentage is
        // printed last with no width specifier so its ANSI codes never skew the
        // alignment.
        let painted_ratio = if translated == total {
            format!("{}", crate::theme::success(&ratio))
        } else {
            all_complete = false;
            format!("{}", crate::theme::warning(&ratio))
        };
        println!(
            "  {:<8} {:>14} {:>14}   {}",
            tag, translated, total, painted_ratio
        );
    }

    println!();
    if all_complete {
        println!(
            "{}",
            crate::theme::success(crate::tr!("i18n_status.all_complete"))
        );
    } else {
        let header = crate::tr!("i18n_status.missing_header");
        for (locale, tag) in REPORTED_LOCALES {
            let missing = missing_keys(*locale);
            if missing.is_empty() {
                continue;
            }
            println!("{} [{}]:", crate::theme::warning(&header), tag);
            for key in missing {
                println!("  - {key}");
            }
        }
        println!("{}", crate::theme::muted(crate::tr!("i18n_status.hint")));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The new v90 wave keys this version adds to `en.json` must all be present
    /// and non-empty in the English source-of-truth table, so the report's
    /// denominator is the real key set.
    #[test]
    fn new_wave_en_keys_present_and_non_empty() {
        let expected = [
            "onboarding.apply_hint",
            "helpindex.header",
            "helpindex.cat_session",
            "helpindex.cat_conversation",
            "helpindex.cat_model",
            "helpindex.cat_extensions",
            "helpindex.cat_display",
            "helpindex.cat_navigation",
            "a11y.enabled",
            "a11y.spinner_label",
            "tutorials.quickstart_title",
            "tutorials.quickstart_step1",
            "tutorials.quickstart_step2",
            "tutorials.next_hint",
            "dashboard.title",
            "dashboard.bar_legend",
            "dashboard.top_models",
            "notify.turn_done",
            "notify.verify_failed",
            "i18n_status.title",
            "i18n_status.col_locale",
            "i18n_status.col_translated",
            "i18n_status.col_total",
            "i18n_status.col_coverage",
            "i18n_status.all_complete",
            "i18n_status.missing_header",
            "i18n_status.hint",
        ];
        let en_keys = locale_table_keys(Locale::En);
        for key in expected {
            assert!(
                en_keys.contains(&key),
                "en.json is missing the v90 wave key: {key}"
            );
            assert!(
                locale_has_key(Locale::En, key),
                "en.json has an empty value for the v90 wave key: {key}"
            );
        }
    }

    /// Parity invariant (mirrors `hindi_table_covers_all_english_keys`): every
    /// regional locale must translate 100% of the English key set. If a key is
    /// missing the assertion fails loudly and names the exact key(s), so a
    /// regression cannot slip through as a silent English fall-through.
    #[test]
    fn coverage_reports_hindi_and_tamil_at_full_parity() {
        let rows = coverage();
        assert_eq!(rows.len(), 2, "expected exactly hi and ta rows");

        let tags: Vec<&str> = rows.iter().map(|(tag, _, _)| *tag).collect();
        assert!(tags.contains(&"hi"), "coverage() must report the hi locale");
        assert!(tags.contains(&"ta"), "coverage() must report the ta locale");

        for (tag, translated, total) in rows {
            let locale = match tag {
                "hi" => Locale::Hi,
                "ta" => Locale::Ta,
                other => panic!("unexpected locale tag in coverage(): {other}"),
            };
            if translated != total {
                let missing = missing_keys(locale);
                panic!(
                    "{tag}.json is below parity: {translated}/{total} keys translated; \
                     missing {} key(s): {missing:?}",
                    missing.len()
                );
            }
            assert_eq!(
                translated, total,
                "{tag} must translate 100% of the English keys"
            );
        }
    }

    /// `pct` rounds down to a whole-number percentage and treats an empty key set
    /// as fully covered (avoids a divide-by-zero and a misleading 0%).
    #[test]
    fn pct_formats_whole_percent() {
        assert_eq!(pct(0, 0), "100%");
        assert_eq!(pct(100, 100), "100%");
        assert_eq!(pct(0, 10), "0%");
        assert_eq!(pct(1, 3), "33%");
        assert_eq!(pct(2, 3), "66%");
    }

    /// With both locales at full parity there are no missing keys to list.
    #[test]
    fn no_missing_keys_when_at_parity() {
        assert!(
            missing_keys(Locale::Hi).is_empty(),
            "hi.json must not be missing any English key: {:?}",
            missing_keys(Locale::Hi)
        );
        assert!(
            missing_keys(Locale::Ta).is_empty(),
            "ta.json must not be missing any English key: {:?}",
            missing_keys(Locale::Ta)
        );
    }

    /// The localized report renders without panicking under the default locale.
    #[test]
    fn handle_i18n_status_runs() {
        handle_i18n_status().expect("i18n-status report should succeed");
    }
}
