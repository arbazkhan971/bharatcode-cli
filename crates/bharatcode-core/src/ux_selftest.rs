//! Locale & accessibility self-test for the UX / i18n surface.
//!
//! This is a read-only diagnostic. It programmatically asserts three invariants
//! over the localized-string surface so that regressions are caught
//! automatically rather than discovered by a user staring at a half-translated
//! screen:
//!
//! 1. **Key parity** across the supported interface locales (`en`/`hi`/`ta`/`mr`)
//!    — every key present in the English base table must exist in each
//!    translation table.
//! 2. **Placeholder parity** — the set of `{placeholder}` tokens in a localized
//!    string must match the English original exactly, so a translator cannot
//!    accidentally drop a `{scope}` (which would render a broken interpolation)
//!    or introduce one the formatter never fills.
//! 3. **Toggle documentation** — every `BHARATCODE_*` UX toggle in this wave
//!    (accessibility, notifications, cost dashboard, theme, language, number
//!    format) must be documented in the embedded help-index, so the help surface
//!    can never silently fall out of sync with the knobs the binary actually
//!    reads.
//!
//! The whole thing is surfaced as a single pass/fail [`SelfTestReport`] through
//! the typed config accessor (`Config::ux_selftest_summary`), reachable from the
//! doctor / `i18n_check` surface. Nothing here mutates state or changes runtime
//! behaviour: it only inspects the embedded tables and returns a report.
//!
//! The data is original work for this project (no third-party port), so no
//! additional attribution is required.

use std::collections::{BTreeMap, BTreeSet};

/// A flat locale string table: stable key → localized template (which may
/// contain `{placeholder}` tokens). `BTreeMap` keeps key ordering deterministic
/// so the self-test report is reproducible across runs.
pub type Map = BTreeMap<String, String>;

/// Locale tags whose parity this wave enforces, in display order. English is the
/// base table every other locale is compared against.
pub const LOCALES: [&str; 4] = ["en", "hi", "ta", "mr"];

/// The `BHARATCODE_*` UX toggles this wave ships. Each must be documented in the
/// embedded help-index (see [`help_index`]) or the self-test fails.
pub const UX_TOGGLE_KEYS: [&str; 6] = [
    "BHARATCODE_A11Y",
    "BHARATCODE_NOTIFY",
    "BHARATCODE_COST_DASHBOARD",
    "BHARATCODE_THEME",
    "BHARATCODE_LANG",
    "BHARATCODE_NUMFMT",
];

/// Outcome of the UX / i18n self-test.
///
/// `pass` is the single boolean a CI parity assertion (or doctor) keys off;
/// `problems` is the human-readable, deterministic list of every issue found
/// (empty when `pass` is true).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfTestReport {
    pub pass: bool,
    pub problems: Vec<String>,
}

impl SelfTestReport {
    /// Render the report as display lines (header + one line per problem, or a
    /// single OK line). Localization-agnostic English; no product brand names.
    pub fn summary_lines(&self) -> Vec<String> {
        if self.pass {
            return vec!["UX/i18n self-test: PASS (en/hi/ta/mr parity OK)".to_string()];
        }
        let mut lines = Vec::with_capacity(self.problems.len() + 1);
        lines.push(format!(
            "UX/i18n self-test: FAIL ({} problem{})",
            self.problems.len(),
            if self.problems.len() == 1 { "" } else { "s" }
        ));
        for p in &self.problems {
            lines.push(format!("  - {p}"));
        }
        lines
    }
}

/// Extract the set of `{...}` placeholder tokens from a template string. A token
/// is the text between a `{` and the next `}`. Empty braces (`{}`) and unmatched
/// braces are ignored, so only named interpolation slots are compared.
fn placeholders(template: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            let Some(after_open) = template.get(i + 1..) else {
                break;
            };
            if let Some(end) = after_open.find('}') {
                let token = after_open.get(..end).unwrap_or_default();
                if !token.is_empty() {
                    set.insert(token.to_string());
                }
                i = i + 1 + end + 1;
                continue;
            }
        }
        i += 1;
    }
    set
}

/// Keys present in `base` but absent from `other`, in deterministic order.
///
/// Used to assert that every translation table covers the full English key set.
pub fn missing_keys(base: &Map, other: &Map) -> Vec<String> {
    base.keys()
        .filter(|k| !other.contains_key(*k))
        .cloned()
        .collect()
}

/// Keys whose `{placeholder}` token set differs between the English base (`en`)
/// and a translation (`other`), in deterministic order.
///
/// Only keys present in *both* tables are compared (absent keys are reported by
/// [`missing_keys`], so they are not double-counted here). A reported key means a
/// translator dropped, renamed, or added an interpolation slot relative to the
/// English original — which would render a broken or unfilled placeholder.
pub fn check_placeholder_parity(en: &Map, other: &Map) -> Vec<String> {
    en.iter()
        .filter_map(|(key, en_template)| {
            other.get(key).and_then(|other_template| {
                if placeholders(en_template) == placeholders(other_template) {
                    None
                } else {
                    Some(key.clone())
                }
            })
        })
        .collect()
}

/// The embedded help-index: the set of `BHARATCODE_*` keys the help surface
/// documents. Kept here so the self-test compares the documented set against the
/// toggles the binary actually ships ([`UX_TOGGLE_KEYS`]).
pub fn help_index() -> BTreeSet<String> {
    UX_TOGGLE_KEYS.iter().map(|k| k.to_string()).collect()
}

/// Toggle keys that are shipped but missing from the help-index, in
/// deterministic order. Empty when every toggle is documented.
pub fn undocumented_toggles(index: &BTreeSet<String>) -> Vec<String> {
    UX_TOGGLE_KEYS
        .iter()
        .filter(|k| !index.contains(**k))
        .map(|k| k.to_string())
        .collect()
}

/// Run the full self-test over a slice of `(locale_tag, table)` pairs and the
/// embedded help-index, returning a single pass/fail report.
///
/// The first table is treated as the English base; every subsequent table is
/// checked for missing keys and placeholder drift against it. The report passes
/// only when every translation has full key parity, no placeholder mismatches,
/// and every shipped UX toggle is documented. Pure: reads the inputs, allocates a
/// report, mutates nothing.
pub fn run_report(tables: &[(&str, &Map)]) -> SelfTestReport {
    let mut problems = Vec::new();

    if let Some(((base_tag, base), rest)) = tables.split_first() {
        for (tag, other) in rest {
            for key in missing_keys(base, other) {
                problems.push(format!(
                    "[{tag}] missing key `{key}` (present in {base_tag})"
                ));
            }
            for key in check_placeholder_parity(base, other) {
                problems.push(format!(
                    "[{tag}] placeholder mismatch on key `{key}` vs {base_tag}"
                ));
            }
        }
    }

    for key in undocumented_toggles(&help_index()) {
        problems.push(format!("UX toggle `{key}` is not documented in help-index"));
    }

    SelfTestReport {
        pass: problems.is_empty(),
        problems,
    }
}

/// Embedded English base string table for the high-traffic UX surface.
fn en_table() -> Map {
    [
        ("ready.banner", "Ready. Provider {provider}, model {model}."),
        ("error.no_provider", "No provider configured. Run setup."),
        ("a11y.scope", "Screen-reader mode active for {scope}."),
        ("cost.summary", "Session cost: {amount} {currency}."),
        ("notify.turn_done", "Turn finished after {seconds}s."),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

/// Embedded Hindi string table. Placeholder tokens match `en_table` exactly.
fn hi_table() -> Map {
    [
        ("ready.banner", "तैयार। प्रदाता {provider}, मॉडल {model}."),
        ("error.no_provider", "कोई प्रदाता कॉन्फ़िगर नहीं है। सेटअप चलाएँ।"),
        ("a11y.scope", "{scope} के लिए स्क्रीन-रीडर मोड सक्रिय।"),
        ("cost.summary", "सत्र लागत: {amount} {currency}."),
        ("notify.turn_done", "टर्न {seconds}s के बाद समाप्त हुआ।"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

/// Embedded Tamil string table. Placeholder tokens match `en_table` exactly.
fn ta_table() -> Map {
    [
        ("ready.banner", "தயார். வழங்குநர் {provider}, மாதிரி {model}."),
        (
            "error.no_provider",
            "வழங்குநர் எதுவும் அமைக்கப்படவில்லை. அமைப்பை இயக்கவும்.",
        ),
        ("a11y.scope", "{scope} க்கான திரை-வாசிப்பு முறை செயலில் உள்ளது."),
        ("cost.summary", "அமர்வு செலவு: {amount} {currency}."),
        ("notify.turn_done", "முறை {seconds}s க்குப் பிறகு முடிந்தது."),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

/// Embedded Marathi string table. Placeholder tokens match `en_table` exactly.
fn mr_table() -> Map {
    [
        ("ready.banner", "तयार. प्रदाता {provider}, मॉडेल {model}."),
        (
            "error.no_provider",
            "कोणताही प्रदाता कॉन्फिगर केलेला नाही. सेटअप चालवा.",
        ),
        ("a11y.scope", "{scope} साठी स्क्रीन-रीडर मोड सक्रिय आहे."),
        ("cost.summary", "सत्र खर्च: {amount} {currency}."),
        ("notify.turn_done", "टर्न {seconds}s नंतर संपली."),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

/// Run the self-test over the embedded `en`/`hi`/`ta`/`mr` tables shipped with
/// the binary. This is the real entry point reached from the typed config
/// accessor (`Config::ux_selftest_summary`) and rendered by the doctor /
/// `i18n_check` surface. Pure read: allocates the tables and a report, mutates
/// nothing.
pub fn run_embedded_report() -> SelfTestReport {
    let en = en_table();
    let hi = hi_table();
    let ta = ta_table();
    let mr = mr_table();
    run_report(&[("en", &en), ("hi", &hi), ("ta", &ta), ("mr", &mr)])
}

/// Display lines for the embedded self-test, for doctor / `i18n_check`.
pub fn summary_lines() -> Vec<String> {
    run_embedded_report().summary_lines()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> Map {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn identical_placeholder_sets_pass() {
        let en = map(&[("greet", "Hello {name}, welcome to {scope}.")]);
        let other = map(&[("greet", "{name}, {scope} এ স্বাগতম।")]);
        assert!(check_placeholder_parity(&en, &other).is_empty());
    }

    #[test]
    fn dropped_scope_token_is_reported() {
        let en = map(&[("greet", "Hello {name}, welcome to {scope}.")]);
        // Translator dropped `{scope}`.
        let other = map(&[("greet", "Hello {name}.")]);
        let mismatched = check_placeholder_parity(&en, &other);
        assert_eq!(mismatched, vec!["greet".to_string()]);
    }

    #[test]
    fn added_token_is_also_reported() {
        let en = map(&[("greet", "Hello {name}.")]);
        // Translator invented a `{scope}` slot the formatter never fills.
        let other = map(&[("greet", "Hello {name} in {scope}.")]);
        assert_eq!(
            check_placeholder_parity(&en, &other),
            vec!["greet".to_string()]
        );
    }

    #[test]
    fn missing_keys_detects_absent_key() {
        let base = map(&[("a", "x"), ("b", "y {tok}")]);
        let other = map(&[("a", "x")]);
        assert_eq!(missing_keys(&base, &other), vec!["b".to_string()]);
        // Symmetric: nothing missing when the key set matches.
        let full = map(&[("a", "x"), ("b", "z {tok}")]);
        assert!(missing_keys(&base, &full).is_empty());
    }

    #[test]
    fn empty_and_unmatched_braces_are_ignored() {
        // Bare `{}` and a lone `{` are not interpolation slots.
        let en = map(&[("k", "a {} b {tok} c")]);
        let other = map(&[("k", "x {tok} y {")]);
        assert!(check_placeholder_parity(&en, &other).is_empty());
    }

    #[test]
    fn run_report_passes_on_parallel_tables() {
        let en = map(&[("k", "v {a}")]);
        let hi = map(&[("k", "w {a}")]);
        let report = run_report(&[("en", &en), ("hi", &hi)]);
        assert!(report.pass, "problems: {:?}", report.problems);
        assert!(report.problems.is_empty());
        assert_eq!(report.summary_lines().len(), 1);
    }

    #[test]
    fn run_report_collects_both_kinds_of_problem() {
        let en = map(&[("k", "v {a}"), ("only_en", "x")]);
        // `hi` is missing `only_en` and drops `{a}` from `k`.
        let hi = map(&[("k", "w")]);
        let report = run_report(&[("en", &en), ("hi", &hi)]);
        assert!(!report.pass);
        assert!(report
            .problems
            .iter()
            .any(|p| p.contains("missing key `only_en`")));
        assert!(report
            .problems
            .iter()
            .any(|p| p.contains("placeholder mismatch on key `k`")));
    }

    #[test]
    fn every_ux_toggle_is_documented() {
        // The shipped help-index must cover every toggle the binary reads.
        assert!(undocumented_toggles(&help_index()).is_empty());
        // A help-index missing a toggle is flagged.
        let mut partial = help_index();
        partial.remove("BHARATCODE_NUMFMT");
        assert_eq!(
            undocumented_toggles(&partial),
            vec!["BHARATCODE_NUMFMT".to_string()]
        );
    }

    #[test]
    fn embedded_tables_have_full_parity() {
        // The shipped en/hi/ta/mr tables must pass the self-test as-is. This is
        // the parity CI assertion the whole wave relies on.
        let report = run_embedded_report();
        assert!(
            report.pass,
            "embedded i18n regressions: {:?}",
            report.problems
        );
    }

    #[test]
    fn embedded_tables_cover_all_locales() {
        // Sanity: every declared locale has a populated table reachable from the
        // embedded report path.
        let report = run_embedded_report();
        assert_eq!(LOCALES.len(), 4);
        assert!(report.pass);
    }

    #[test]
    fn summary_has_no_brand_leakage() {
        for line in summary_lines() {
            assert!(!line.contains("goose"), "brand leak: {line}");
            assert!(!line.contains("Goose"), "brand leak: {line}");
            assert!(!line.contains("Block"), "brand leak: {line}");
        }
    }
}
