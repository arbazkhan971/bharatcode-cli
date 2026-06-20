//! Capability-aware preflight guard for the developer `read_image` tool.
//!
//! When the `BHARATCODE_VISION_GUARD` environment variable is set to a truthy
//! value, the developer extension checks the *active* model's vision capability
//! (sourced from the static model registry) before handing a `read_image`
//! result back to the model. If the active model is recognised and known to be
//! text-only, a single clear advisory line is prepended to the tool result so
//! the user is not left wondering why an attached image was silently ignored.
//!
//! The guard is intentionally conservative:
//!
//! * It is opt-in. When the gate is unset (the default), `read_image` output is
//!   returned byte-for-byte unchanged and no registry lookup is performed.
//! * It only warns when the model is *known* to lack vision. Unknown models
//!   resolve to `None`, so we never emit a false warning for a model whose
//!   capabilities we cannot vouch for.
//!
//! This reuses the existing model-registry capability flags; it does not
//! introduce a new capability table or touch the multimodal image pipeline.
//!
//! Original BharatCode work; not ported from any third party.

/// Name of the environment variable that opts in to the vision preflight guard.
pub const VISION_GUARD_ENV: &str = "BHARATCODE_VISION_GUARD";

/// Returns true when the vision preflight guard is enabled via
/// `BHARATCODE_VISION_GUARD`.
///
/// Accepted truthy values (case-insensitive): `1`, `true`, `yes`, `on`.
/// Anything else (including unset) leaves the guard disabled, so `read_image`
/// output is unchanged.
pub fn is_enabled() -> bool {
    matches!(
        std::env::var(VISION_GUARD_ENV)
            .ok()
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("1") | Some("true") | Some("yes") | Some("on")
    )
}

/// The name of the currently configured active model, if any.
///
/// Returns `None` when no model is configured, in which case the guard makes no
/// claim about vision support and leaves `read_image` output unchanged.
pub fn active_model_name() -> Option<String> {
    crate::config::Config::global().get_bharatcode_model().ok()
}

/// Build a vision advisory for `model`, if one is warranted.
///
/// Looks `model` up in the static model registry. When the model is recognised
/// and its registry entry reports no vision capability, returns a one-line
/// advisory describing the limitation. For unknown models (registry miss) or
/// models that do support vision, returns `None` so no advisory is emitted and
/// we never false-warn.
pub fn vision_advisory(model: &str) -> Option<String> {
    let info = crate::model_registry::lookup(model)?;
    if info.capabilities.vision {
        return None;
    }
    Some(format!(
        "Note: the active model '{model}' has no vision capability ({provider}); \
         the attached image may be ignored.",
        provider = info.provider,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_only_registry_model_yields_advisory() {
        // `sarvam-m` is in the registry with vision=false.
        let advisory = vision_advisory("sarvam-m");
        assert!(advisory.is_some());
        let text = advisory.unwrap();
        assert!(text.contains("sarvam-m"));
        assert!(text.contains("no vision capability"));
    }

    #[test]
    fn vision_capable_registry_model_yields_none() {
        // `gpt-4o` is in the registry with vision=true.
        assert_eq!(vision_advisory("gpt-4o"), None);
    }

    #[test]
    fn unknown_model_yields_none() {
        // Not in the registry => no warning, so we never false-warn.
        assert_eq!(vision_advisory("totally-unknown-xyz"), None);
    }

    #[test]
    fn is_enabled_reflects_env() {
        // Note: relies on env not being globally set in the test process.
        std::env::remove_var(VISION_GUARD_ENV);
        assert!(!is_enabled());
        std::env::set_var(VISION_GUARD_ENV, "1");
        assert!(is_enabled());
        std::env::set_var(VISION_GUARD_ENV, "false");
        assert!(!is_enabled());
        std::env::remove_var(VISION_GUARD_ENV);
    }
}
