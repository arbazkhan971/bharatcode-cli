//! Locale metadata for the BharatCode CLI i18n scaffold (BharatCode v81).
//!
//! This module holds two small, dependency-light building blocks that describe
//! the *set* of supported locales rather than any single translation table:
//!
//!   * [`SUPPORTED`] — the canonical `(tag, native_name)` list, in resolver
//!     order (`en`, `hi`, `ta`). Native names are written in each locale's own
//!     script so a "pick your language" prompt can render them verbatim.
//!   * [`ta_table`] — the parsed Tamil (`ta`) translation table, built once from
//!     the embedded `ta.json` via [`LazyLock`].
//!
//! The active-locale resolution, the `Locale` enum, `normalize_locale` and the
//! `t()` fall-through all live in [`crate::i18n`]; this module only mirrors the
//! `ta.json` table so callers that want the locale catalogue (without going
//! through `t()`) have a stable, std-typed entry point. Loading Tamil at runtime
//! stays opt-in behind `BHARATCODE_LANG=ta`, so default English / Hindi output
//! is byte-for-byte unchanged.

use std::collections::HashMap;
use std::sync::LazyLock;

/// Canonical list of supported locales as `(tag, native_name)` pairs, in the
/// same order the resolver prefers them (`en`, then `hi`, then `ta`).
///
/// Native names are intentionally rendered in each locale's own script
/// (`English`, `हिन्दी`, `தமிழ்`) so a language picker can display them as-is.
pub const SUPPORTED: &[(&str, &str)] = &[("en", "English"), ("hi", "हिन्दी"), ("ta", "தமிழ்")];

/// The embedded Tamil translation table, parsed once on first access.
static TA_TABLE: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("ta.json")).expect("i18n: ta.json is not valid JSON")
});

/// Borrow the process-wide Tamil (`ta`) translation table.
///
/// Built lazily from the `ta.json` embedded at compile time, so there is no I/O
/// at call time and the table is parsed at most once per process.
pub fn ta_table() -> &'static HashMap<String, String> {
    &TA_TABLE
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The supported-locale catalogue lists exactly `en`, `hi`, `ta` in resolver
    /// order, each paired with its own-script native name.
    #[test]
    fn supported_lists_three_locales_in_order() {
        assert_eq!(
            SUPPORTED,
            &[("en", "English"), ("hi", "हिन्दी"), ("ta", "தமிழ்")]
        );
    }

    /// The Tamil table parses cleanly and is non-empty.
    #[test]
    fn ta_table_loads_non_empty() {
        assert!(!ta_table().is_empty());
    }

    /// Every locale tag advertised by [`SUPPORTED`] is a recognisable token and
    /// none of the native names is blank.
    #[test]
    fn supported_entries_are_well_formed() {
        for (tag, native) in SUPPORTED {
            assert!(!tag.is_empty(), "locale tag must not be empty");
            assert!(
                !native.trim().is_empty(),
                "native name for {tag} must not be empty"
            );
        }
    }
}
