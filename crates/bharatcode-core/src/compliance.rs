//! Compliance + trademark self-gate for the running agent.
//!
//! This module carries two small, independent responsibilities that together
//! keep the agent's identity provenance-correct at runtime:
//!
//! 1. [`attribution_block`] — a stable, short markdown footer naming the
//!    Apache-2.0 licence and both upstreams (the Goose project © Block, Inc.
//!    and the Codex donor © OpenAI). Prompt assembly attaches it once as a
//!    system-prompt extra so the agent's self-description always carries the
//!    same trademark-clean provenance wording that ships in `NOTICE` /
//!    `MODIFICATIONS.md`. The wording attributes the *owners* of the upstream
//!    works for licence-compliance reasons; it never claims BharatCode *is*
//!    those products.
//!
//! 2. [`scan_user_facing`] — a leak-checker over a *curated* denylist of
//!    user-facing upstream phrases (e.g. `created by Block`, `goose run`,
//!    `You are Goose`). It exists so tests (and any future user-facing string
//!    audit) can assert that a string carries no upstream branding. It is
//!    deliberately phrase-based, not token-based: documented internal
//!    identifiers such as `GooseMode` / `goose_*` snake_case names are *not*
//!    flagged, because those are private Rust symbols, not user-facing brand.
//!
//! The prompt footer is gated by [`is_enabled`], reading `BHARATCODE_ATTRIBUTION`.
//! Attribution is a compliance affordance, so it is **default-on** for the GA
//! wave: it is suppressed only when the variable is explicitly set to a falsey
//! value (`0` / `off` / `false` / `no`). The attribution text is English-only.
//!
//! This module is original work; nothing here is ported from third-party
//! sources.

/// Opt-out toggle name, read raw from the process environment. Unlike the
/// sibling opt-in prompt modules, attribution is a compliance feature and is
/// **on by default**; only an explicit falsey value suppresses it.
const ENABLE_KEY: &str = "BHARATCODE_ATTRIBUTION";

/// The canonical Apache-2.0 / upstream attribution footer. Kept short and
/// stable so it is cache-friendly across sessions and mirrors the wording in
/// `NOTICE` and `MODIFICATIONS.md`. The owner names appear here intentionally
/// for licence compliance (Apache-2.0 Section 4); they are the only upstream
/// marks this string is permitted to carry.
const ATTRIBUTION_BLOCK: &str = "\
# Provenance

BharatCode is an open-source, India-first coding agent distributed under the \
Apache License, Version 2.0. It is a derivative work of the Goose project \
(Copyright 2024 Block, Inc.) and incorporates portions of OpenAI Codex \
(Copyright 2025 OpenAI), each licensed under Apache-2.0. Upstream names and \
logos are trademarks of their respective owners and are used here only for \
required attribution.";

/// Curated denylist of *user-facing* upstream phrases. These are full phrases
/// (not bare tokens) that would only appear in branded, user-facing copy — so
/// matching them flags real leakage while leaving internal Rust identifiers
/// (`GooseMode`, `goose_mode`, `goose_*` symbols) untouched. Matching is
/// case-insensitive on the whole phrase.
const USER_FACING_DENYLIST: &[&str] = &[
    "created by Block",
    "goose configure",
    "goose run",
    "You are Goose",
];

/// Whether the runtime attribution footer is enabled. Read from
/// `BHARATCODE_ATTRIBUTION`; **default-on**. An explicit falsey value
/// (`0`, `off`, `false`, `no`) suppresses the footer; anything else (including
/// an unset variable or an unrecognised value) leaves it enabled.
pub fn is_enabled() -> bool {
    std::env::var(ENABLE_KEY)
        .map(|raw| !is_falsey(&raw))
        .unwrap_or(true)
}

fn is_falsey(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "0" | "off" | "false" | "no"
    )
}

/// The canonical Apache-2.0 + upstream attribution footer as a stable markdown
/// string. Always returns the same content; gating is the caller's concern via
/// [`is_enabled`]. The returned string contains the literal `Apache-2.0`.
pub fn attribution_block() -> String {
    ATTRIBUTION_BLOCK.to_string()
}

/// Scan `s` for user-facing upstream brand phrases and return the list of
/// disallowed phrases found (each as its canonical `&'static str` form). The
/// result is empty for clean strings and non-empty when a curated phrase such
/// as `created by Block` appears. Internal Rust identifiers (e.g. `GooseMode`)
/// are intentionally *not* in the denylist and therefore never flagged.
pub fn scan_user_facing(s: &str) -> Vec<&'static str> {
    let haystack = s.to_ascii_lowercase();
    USER_FACING_DENYLIST
        .iter()
        .filter(|phrase| haystack.contains(&phrase.to_ascii_lowercase()))
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialise tests that mutate the shared process env so the
    /// `BHARATCODE_ATTRIBUTION` toggle does not race across threads.
    fn env_guard(value: Option<&str>) -> env_lock::EnvGuard<'_> {
        env_lock::lock_env([(ENABLE_KEY, value)])
    }

    #[test]
    fn attribution_is_on_by_default() {
        let _guard = env_guard(None);
        assert!(is_enabled());
    }

    #[test]
    fn explicit_falsey_suppresses_attribution() {
        for falsey in ["0", "off", "false", "no", " OFF "] {
            let _guard = env_guard(Some(falsey));
            assert!(!is_enabled(), "expected {falsey:?} to suppress attribution");
        }
    }

    #[test]
    fn truthy_or_unknown_values_keep_attribution_on() {
        for on in ["1", "true", "yes", "on", "anything"] {
            let _guard = env_guard(Some(on));
            assert!(is_enabled(), "expected {on:?} to keep attribution on");
        }
    }

    #[test]
    fn attribution_block_names_license_and_upstreams() {
        let block = attribution_block();
        assert!(block.contains("Apache-2.0"));
        // Both upstream owners are named for required attribution.
        assert!(block.contains("Block, Inc."));
        assert!(block.contains("OpenAI"));
    }

    #[test]
    fn scan_flags_user_facing_phrase() {
        let hits = scan_user_facing("created by Block");
        assert!(!hits.is_empty());
        assert!(hits.contains(&"created by Block"));
    }

    #[test]
    fn scan_is_clean_for_internal_identifier() {
        // A purely-internal Rust identifier must not be flagged.
        assert!(scan_user_facing("GooseMode internal").is_empty());
        assert!(scan_user_facing("let goose_mode = GooseMode::Auto;").is_empty());
        assert!(scan_user_facing("a perfectly clean sentence").is_empty());
    }

    #[test]
    fn scan_matches_case_insensitively() {
        assert!(!scan_user_facing("CREATED BY BLOCK").is_empty());
        assert!(!scan_user_facing("you are goose").is_empty());
    }

    #[test]
    fn attribution_block_is_leak_clean_except_owner_attributions() {
        // The footer intentionally names the owners (Block, Inc. / OpenAI) for
        // licence compliance, but it must not carry any of the *user-facing*
        // brand phrases the scanner guards against.
        assert!(scan_user_facing(&attribution_block()).is_empty());
    }
}
