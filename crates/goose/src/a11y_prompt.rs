//! Screen-reader system-prompt advisory.
//!
//! When accessibility mode is active, prompt assembly injects a small,
//! low-priority advisory asking the model to favour plain text and avoid
//! visually-dense output (ASCII art, box-drawing, wide tables) that screen
//! readers narrate poorly. The guidance steers the assistant's prose to be
//! screen-reader friendly end-to-end.
//!
//! The feature is opt-in: [`advisory_block`] returns `None` unless the
//! `BHARATCODE_A11Y` environment variable is truthy, so the default system
//! prompt is byte-identical. The env gate mirrors the env-first toggles in
//! `agent_caps` / `memory_store`.

/// Opt-in toggle name, read from the environment.
const ENABLE_KEY: &str = "BHARATCODE_A11Y";

/// Whether the screen-reader advisory is enabled. Opt-in via the
/// `BHARATCODE_A11Y` environment variable; any truthy-ish value (`1`, `true`,
/// `yes`, `on`) enables it. Mirrors the raw-env truthiness table in
/// `memory_store::is_truthy`.
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

/// The screen-reader guidance injected into the system prompt when enabled,
/// or `None` when disabled (leaving the prompt unchanged). The text is
/// product-neutral plain text and kept compact (under 500 chars).
pub fn advisory_block() -> Option<String> {
    if !is_enabled() {
        return None;
    }
    Some(
        "# Accessibility\n\n\
         A screen reader is in use. Write replies that read well aloud:\n\
         - Prefer plain text and short sentences; explain things in prose.\n\
         - Avoid ASCII art, box-drawing characters, and dense or wide tables; \
         use simple bullet or numbered lists instead.\n\
         - Describe diagrams, charts, and layouts in words rather than drawing them.\n\
         - Announce code blocks before showing them and summarise what they do.\n"
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    // Serialise env mutation across tests in this module so toggling
    // BHARATCODE_A11Y in one test never races another.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn lock_env() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn advisory_is_none_when_unset() {
        let _guard = lock_env();
        std::env::remove_var(ENABLE_KEY);
        assert!(advisory_block().is_none());
        assert!(!is_enabled());
    }

    #[test]
    fn advisory_is_some_when_enabled() {
        let _guard = lock_env();
        std::env::set_var(ENABLE_KEY, "1");
        let block = advisory_block().expect("advisory present when enabled");
        std::env::remove_var(ENABLE_KEY);

        let lower = block.to_lowercase();
        assert!(lower.contains("plain text"));
        assert!(lower.contains("screen reader"));
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
    fn is_enabled_reflects_env_truthiness() {
        let _guard = lock_env();
        for truthy in ["1", "true", "TRUE", " yes ", "on"] {
            std::env::set_var(ENABLE_KEY, truthy);
            assert!(is_enabled(), "expected {truthy:?} to enable");
        }
        for falsy in ["0", "false", "no", "off", ""] {
            std::env::set_var(ENABLE_KEY, falsy);
            assert!(!is_enabled(), "expected {falsy:?} to stay disabled");
        }
        std::env::remove_var(ENABLE_KEY);
        assert!(!is_enabled());
    }
}
