//! Active-model capability advisory for the system prompt.
//!
//! When enabled (opt-in via `BHARATCODE_MODEL_CAPS`), this surfaces a compact,
//! model-aware capability summary into the system prompt so the agent can
//! self-adjust its plans -- for example, avoiding image-analysis proposals on a
//! text-only model. The capability data is sourced entirely from the static
//! [`crate::model_registry`]; no new capability facts are introduced here.
//!
//! The feature is a strict no-op when the toggle is unset or the active model is
//! not recognised in the registry: [`capability_block`] returns `None` and the
//! built system prompt is byte-identical to default behaviour. We never
//! fabricate capabilities for unknown models.
//!
//! Original BharatCode work; not ported from any third party.

/// Opt-in toggle name, shared by env var and config file.
const ENABLE_KEY: &str = "BHARATCODE_MODEL_CAPS";

/// Whether the capability advisory is enabled. Opt-in via the
/// `BHARATCODE_MODEL_CAPS` environment variable or the config value of the same
/// name. Any truthy-ish value (`1`, `true`, `yes`, `on`) enables it; default OFF.
pub fn is_enabled() -> bool {
    if let Ok(raw) = std::env::var(ENABLE_KEY) {
        return is_truthy(&raw);
    }
    crate::config::Config::global()
        .get_param::<String>(ENABLE_KEY)
        .map(|raw| is_truthy(&raw))
        .unwrap_or(false)
}

fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// The currently configured active model name, if any.
pub fn active_model_name() -> Option<String> {
    crate::config::Config::global().get_bharatcode_model().ok()
}

fn yes_no(flag: bool) -> &'static str {
    if flag {
        "yes"
    } else {
        "no"
    }
}

/// Render a compact capability advisory block for the active model, or `None`.
///
/// Returns `None` when the feature is disabled, when there is no active model,
/// or when the active model is not found in the registry (we never fabricate
/// capabilities). On a registry hit, returns a short markdown block summarising
/// the model's coarse capabilities sourced from [`crate::model_registry`].
pub fn capability_block() -> Option<String> {
    if !is_enabled() {
        return None;
    }
    let name = active_model_name()?;
    let info = crate::model_registry::lookup(&name)?;
    let caps = &info.capabilities;
    Some(format!(
        "# Model capabilities\n\
         - model: {} ({})\n\
         - tools: {}\n\
         - vision: {}\n\
         - open weights: {}\n\
         - context window: {} tokens\n\
         Prefer plans that fit these capabilities.",
        info.name,
        info.provider,
        yes_no(caps.tools),
        yes_no(caps.vision),
        yes_no(caps.open_weights),
        info.context_window,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialise tests that mutate the shared process env so the
    /// `BHARATCODE_MODEL_CAPS` / active-model toggles don't race each other.
    /// `BHARATCODE_MODEL` takes precedence in `get_active_model`, so setting it
    /// here is enough to drive `active_model_name` without touching config.
    fn env_guard<'a>(caps: Option<&'a str>, model: Option<&'a str>) -> env_lock::EnvGuard<'a> {
        env_lock::lock_env([("BHARATCODE_MODEL_CAPS", caps), ("BHARATCODE_MODEL", model)])
    }

    #[test]
    fn disabled_yields_none_even_with_known_model() {
        let _guard = env_guard(None, Some("gpt-4o"));
        assert!(!is_enabled());
        assert!(capability_block().is_none());
    }

    #[test]
    fn enabled_known_vision_model_reports_caps() {
        let _guard = env_guard(Some("1"), Some("gpt-4o"));
        assert!(is_enabled());

        let block = capability_block().expect("known model yields a block");
        assert!(block.contains("# Model capabilities"));
        assert!(block.contains("gpt-4o"));
        assert!(block.contains("vision: yes"));
        assert!(block.contains("tools: yes"));
        // Keep the block compact to avoid prompt bloat.
        assert!(block.len() < 500, "block too long: {} chars", block.len());
    }

    #[test]
    fn enabled_unknown_model_yields_none() {
        let _guard = env_guard(Some("1"), Some("totally-made-up-model"));
        assert!(is_enabled());
        assert!(capability_block().is_none());
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
