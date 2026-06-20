//! Screen-reader-friendly tool-output advisory for the system prompt.
//!
//! When accessibility mode is active, prompt assembly injects a small,
//! low-priority advisory asking the model to render tool and command output in
//! a way that screen readers narrate well: linear plain text, no ASCII-art or
//! box-drawing tables, explicit per-row labels instead of aligned columns, and
//! a short summary in front of any large output dump.
//!
//! The feature is opt-in behind its own dedicated toggle so it stays
//! independent of the CLI-side accessibility knobs: [`advisory_block`] returns
//! `None` unless the `BHARATCODE_A11Y_PROMPT` environment variable is truthy,
//! so the default system prompt is byte-identical. The env gate is read raw and
//! env-first, mirroring the truthiness tables in the sibling `repo_digest` /
//! `plan_mode` prompt modules. This module is original work; nothing here is
//! ported from third-party sources.

/// Opt-in toggle name, read raw from the process environment. Deliberately
/// distinct from the broader `BHARATCODE_A11Y` CLI toggle so enabling the
/// terminal accessibility affordances does not silently grow the system prompt.
const ENABLE_KEY: &str = "BHARATCODE_A11Y_PROMPT";

/// Whether the screen-reader tool-output advisory is enabled. Opt-in via the
/// `BHARATCODE_A11Y_PROMPT` environment variable; any truthy-ish value (`1`,
/// `true`, `yes`, `on`) enables it. Defaults to `false` when unset. The lookup
/// is raw-env-first to match the sibling prompt modules.
pub fn is_enabled() -> bool {
    std::env::var(ENABLE_KEY)
        .map(|raw| is_truthy(&raw))
        .unwrap_or(false)
}

fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// The screen-reader guidance injected into the system prompt when enabled, or
/// `None` when disabled (leaving the prompt byte-identical). The text is
/// product-neutral plain text and kept compact (under 500 chars).
pub fn advisory_block() -> Option<String> {
    if !is_enabled() {
        return None;
    }
    Some(
        "# Accessibility\n\n\
         A screen reader is in use. Format tool and command output so it reads \
         aloud cleanly:\n\
         - Present results as linear plain text; do not use ASCII-art, \
         box-drawing, or aligned-column tables.\n\
         - Label each row or field explicitly (e.g. \"name: value\") instead of \
         relying on column position.\n\
         - For large output, give a short summary first, then the key lines.\n"
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialise tests that mutate the shared process env so the
    /// `BHARATCODE_A11Y_PROMPT` toggle does not race across threads.
    fn env_guard(value: Option<&str>) -> env_lock::EnvGuard<'_> {
        env_lock::lock_env([(ENABLE_KEY, value)])
    }

    #[test]
    fn advisory_is_none_when_unset() {
        let _guard = env_guard(None);
        assert!(!is_enabled());
        assert!(advisory_block().is_none());
    }

    #[test]
    fn advisory_is_some_when_enabled() {
        let _guard = env_guard(Some("1"));
        assert!(is_enabled());

        let block = advisory_block().expect("advisory present when enabled");
        let lower = block.to_lowercase();
        // Carries the screen-reader tool-output guidance the spec calls for.
        assert!(lower.contains("screen reader"));
        assert!(lower.contains("linear plain text"));
        assert!(lower.contains("label each row"));
        assert!(lower.contains("summary"));
        // Stays compact so it remains a low-priority hint.
        assert!(
            block.len() < 500,
            "advisory must stay compact: {}",
            block.len()
        );
        // Product-neutral: no donor/upstream brand leakage.
        assert!(!block.contains("goose"));
        assert!(!block.contains("Goose"));
        assert!(!block.contains("Block"));
    }

    #[test]
    fn falsey_value_stays_disabled() {
        let _guard = env_guard(Some("0"));
        assert!(!is_enabled());
        assert!(advisory_block().is_none());
    }

    #[test]
    fn is_enabled_reflects_env_truthiness() {
        for truthy in ["1", "true", "TRUE", " yes ", "on"] {
            let _guard = env_guard(Some(truthy));
            assert!(is_enabled(), "expected {truthy:?} to enable");
        }
        for falsy in ["0", "false", "no", "off", ""] {
            let _guard = env_guard(Some(falsy));
            assert!(!is_enabled(), "expected {falsy:?} to stay disabled");
        }
        let _guard = env_guard(None);
        assert!(!is_enabled());
    }

    #[test]
    fn is_truthy_recognizes_common_values() {
        assert!(is_truthy("1"));
        assert!(is_truthy("TRUE"));
        assert!(is_truthy(" yes "));
        assert!(is_truthy("on"));
        assert!(!is_truthy("0"));
        assert!(!is_truthy("false"));
        assert!(!is_truthy(""));
    }
}
