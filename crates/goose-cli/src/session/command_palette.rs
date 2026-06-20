//! Inline command palette: a fuzzy, intent-first finder for slash-commands and
//! help-index entries.
//!
//! Users rarely remember exact command names. This module lets them type a few
//! characters of *intent* — `/find cst`, `/? spend` — and get back the closest
//! matching commands and environment toggles, ranked best-first, each with its
//! localized one-line summary. It is the discovery counterpart to the
//! exact-match [`crate::help_index`] catalog.
//!
//! The matching core is a pure, dependency-free subsequence scorer: a query
//! matches a candidate when its characters appear in order (not necessarily
//! adjacently) in the candidate, and shorter, earlier, more-contiguous matches
//! score higher. This is the same family of heuristic used by editor "fuzzy
//! file finders", kept deliberately small so it needs no new crates.
//!
//! Descriptions are pulled from the shared [`crate::help_index`] table via
//! [`entries_from_help_index`], so summaries stay localized through
//! [`crate::tr!`] exactly like the `/help` index. The module is pure and
//! side-effect free; the interactive call site in `session::mod` renders
//! [`search`] results through the active theme.

use crate::help_index;
use crate::theme;

/// One row in the palette: a command (or env-toggle) name and its localized
/// one-line summary. Owned strings keep the type self-contained and easy to
/// build from either the static help index or an ad-hoc list in tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteEntry {
    /// User-facing identifier — the slash-command/subcommand name or env var.
    pub name: String,
    /// Localized one-line description of what the entry does.
    pub summary: String,
}

impl PaletteEntry {
    /// Convenience constructor used in tests and call sites.
    pub fn new(name: impl Into<String>, summary: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            summary: summary.into(),
        }
    }
}

/// Build the palette's candidate set from the shared [`help_index::ENTRIES`]
/// catalog, so every command and `BHARATCODE_*` toggle is discoverable and each
/// summary is localized through [`crate::tr!`] (via [`help_index`]'s own
/// `summary()`), in the canonical index order.
pub fn entries_from_help_index() -> Vec<PaletteEntry> {
    help_index::ENTRIES
        .iter()
        .map(|e| PaletteEntry::new(e.name, e.summary()))
        .collect()
}

/// Score a single candidate string against a lowercased query using an
/// order-preserving subsequence match.
///
/// Returns `None` when `query`'s characters do not all appear, in order, within
/// `candidate`. Otherwise returns a non-negative score where **higher is
/// better**: contiguous runs and matches near the start of the candidate are
/// rewarded, and longer candidates are gently penalized so a tight match on a
/// short name outranks a scattered match buried in a long summary.
///
/// Both inputs are compared case-insensitively; `query` is expected to already
/// be lowercased by the caller (see [`search`]).
fn subsequence_score(query_lower: &str, candidate: &str) -> Option<i32> {
    if query_lower.is_empty() {
        return Some(0);
    }
    let cand_lower = candidate.to_lowercase();
    let cand_chars: Vec<char> = cand_lower.chars().collect();
    let query_chars: Vec<char> = query_lower.chars().collect();

    let mut score: i32 = 0;
    let mut qi = 0usize;
    let mut last_match: Option<usize> = None;

    for (ci, &cc) in cand_chars.iter().enumerate() {
        if qi >= query_chars.len() {
            break;
        }
        if cc == query_chars[qi] {
            // Base reward for a matched character.
            score += 10;
            match last_match {
                // Adjacent match (a contiguous run) is worth more.
                Some(prev) if prev + 1 == ci => score += 15,
                _ => {}
            }
            // Matching at the very start of the candidate is a strong signal.
            if ci == 0 {
                score += 20;
            }
            last_match = Some(ci);
            qi += 1;
        }
    }

    if qi < query_chars.len() {
        return None;
    }

    // Gently prefer shorter candidates so an exact-ish short name beats a long
    // summary that merely happens to contain the subsequence.
    score -= cand_chars.len() as i32 / 8;
    Some(score.max(0))
}

/// Best score of `query_lower` against an entry, taking the stronger of its
/// `name` and `summary` matches so intent words in the description still surface
/// the entry even when the name is cryptic.
fn entry_score(query_lower: &str, entry: &PaletteEntry) -> Option<i32> {
    let name = subsequence_score(query_lower, &entry.name);
    // A summary match is real but less authoritative than a name match.
    let summary = subsequence_score(query_lower, &entry.summary).map(|s| s - 5);
    match (name, summary) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

/// Fuzzy-rank `entries` against `query`, best match first.
///
/// Matching is case-insensitive and order-preserving (subsequence). An empty or
/// whitespace-only `query` is treated as "show everything" and returns a clone
/// of `entries` in their original, stable order. Otherwise only entries whose
/// name or summary contains the query as a subsequence are returned, sorted by
/// descending score; ties break on the entry's original index, so the result is
/// fully deterministic.
pub fn search(query: &str, entries: &[PaletteEntry]) -> Vec<PaletteEntry> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return entries.to_vec();
    }

    let mut scored: Vec<(usize, i32, &PaletteEntry)> = entries
        .iter()
        .enumerate()
        .filter_map(|(idx, e)| entry_score(&needle, e).map(|score| (idx, score, e)))
        .collect();

    // Higher score first; stable tie-break on original position.
    scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    scored.into_iter().map(|(_, _, e)| e.clone()).collect()
}

/// Localize `key`, falling back to `default` when no translation is registered
/// (mirrors [`help_index::HelpEntry::summary`]'s behavior so a missing i18n
/// entry never leaks a raw key like `palette.title` to the user).
fn localized(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated.is_empty() || translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// Render up to `limit` palette matches for `query` as a themed, human-readable
/// block. When nothing matches, a single muted "no matches" line is returned so
/// the caller always has something to print. Pure: reads the active theme/locale
/// but performs no I/O.
pub fn render(query: &str, entries: &[PaletteEntry], limit: usize) -> String {
    let hits = search(query, entries);
    if hits.is_empty() {
        return format!(
            "{}\n",
            theme::muted(localized(
                "palette.no_matches",
                "No commands match that query.",
            ))
        );
    }

    let shown: Vec<&PaletteEntry> = hits.iter().take(limit.max(1)).collect();
    let width = shown.iter().map(|e| e.name.len()).max().unwrap_or(0);

    let mut out = String::new();
    out.push_str(&format!(
        "{}\n",
        theme::heading(localized("palette.title", "Matching commands"))
    ));
    for e in shown {
        out.push_str(&format!(
            "  {:<width$}  {}\n",
            theme::accent(&e.name),
            theme::neutral(&e.summary),
            width = width,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<PaletteEntry> {
        vec![
            PaletteEntry::new("cost", "Show recorded spend per session, in USD and INR."),
            PaletteEntry::new("configure", "Set up providers, models, and settings."),
            PaletteEntry::new("doctor", "Run environment health checks and surface fixes."),
            PaletteEntry::new("privacy", "Report the resolved data-governance posture."),
        ]
    }

    #[test]
    fn cst_ranks_cost_above_unrelated() {
        let entries = sample();
        let hits = search("cst", &entries);
        assert!(!hits.is_empty(), "fuzzy query 'cst' should match something");
        assert_eq!(
            hits[0].name,
            "cost",
            "'cst' should rank the cost entry first, got {:?}",
            hits.iter().map(|e| &e.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn empty_query_returns_all_in_stable_order() {
        let entries = sample();
        let hits = search("", &entries);
        assert_eq!(
            hits, entries,
            "empty query must return all entries in order"
        );
        // Whitespace-only behaves the same.
        assert_eq!(search("   ", &entries), entries);
    }

    #[test]
    fn matching_is_case_insensitive() {
        let entries = sample();
        let lower = search("cost", &entries);
        let upper = search("COST", &entries);
        let mixed = search("CoSt", &entries);
        assert_eq!(lower, upper);
        assert_eq!(lower, mixed);
        assert_eq!(lower[0].name, "cost");
    }

    #[test]
    fn no_match_returns_empty() {
        let entries = sample();
        assert!(search("zzqqxx", &entries).is_empty());
    }

    #[test]
    fn results_are_deterministic() {
        let entries = sample();
        let a = search("co", &entries);
        let b = search("co", &entries);
        assert_eq!(a, b, "search must be deterministic for identical inputs");
    }

    #[test]
    fn summary_intent_words_surface_entries() {
        let entries = sample();
        // "spend" only appears in the cost summary, not any name.
        let hits = search("spend", &entries);
        assert!(
            hits.iter().any(|e| e.name == "cost"),
            "intent word from the summary should surface the cost entry"
        );
    }

    #[test]
    fn entries_from_help_index_is_non_empty_and_clean() {
        let entries = entries_from_help_index();
        assert!(
            !entries.is_empty(),
            "help index should yield palette entries"
        );
        for e in &entries {
            let hay = format!("{} {}", e.name, e.summary).to_lowercase();
            assert!(!hay.contains("goose"), "palette entry leaks 'goose': {hay}");
            assert!(!hay.contains("block"), "palette entry leaks 'block': {hay}");
        }
        // The shared catalog includes the cost command; it must be findable.
        let hits = search("cst", &entries);
        assert!(
            hits.iter().any(|e| e.name == "cost"),
            "fuzzy 'cst' should find the cost command from the help index"
        );
    }

    #[test]
    fn render_lists_matches_and_handles_misses() {
        let entries = sample();
        let block = render("cost", &entries, 5);
        assert!(
            block.contains("cost"),
            "render should list the matched name"
        );
        // A guaranteed miss still produces a non-empty, single-line block.
        let miss = render("zzqqxx", &entries, 5);
        assert!(!miss.is_empty());
    }
}
