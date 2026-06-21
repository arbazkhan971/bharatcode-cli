//! Planner / sub-task model preset advisory.
//!
//! Plan-mode and sub-agents benefit from a reasoning-strong model that is
//! often *different* from the chat model in the foreground session. This
//! module ships a small, curated table of "planner-grade" presets — biased
//! towards open-weight and India-hosted reasoning models — together with a
//! resolver that lets a consumer pick a sensible planner provider/model.
//!
//! It is pure, in-binary metadata: no network calls, no provider construction.
//! The preset ids are cross-checked against [`crate::model_registry`] and the
//! declarative India provider definitions (Krutrim, Sarvam) where they overlap,
//! so a planner pick lines up with the cost/capability surface those tables
//! describe.
//!
//! The feature is default-inert: [`resolve_planner`] returns `None` until a
//! user explicitly opts in via the `BHARATCODE_PLANNER_MODEL` environment
//! variable (env-first), so default behaviour is unchanged. When set, a
//! `provider/model` value selects the planner pair directly; a bare preset id
//! resolves through [`PLANNER_PRESETS`].
//!
//! Original BharatCode work; not ported from any third party.

/// Environment variable selecting the planner model (env-first opt-in).
///
/// Accepts either a fully-qualified `provider/model` pair (e.g.
/// `ollama/qwen2.5-coder`) or a bare preset id from [`PLANNER_PRESETS`]
/// (e.g. `local-qwen-coder`). Unset / blank leaves the feature inert.
pub const ENV_VAR: &str = "BHARATCODE_PLANNER_MODEL";

/// A single curated planner-grade model preset.
///
/// Pure metadata: `provider`/`model` are the identifiers a consumer would pass
/// to the provider layer, and `note` is a short human-friendly rationale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlannerPreset {
    /// Stable, unique short id for this preset (used as a bare selector).
    pub id: &'static str,
    /// Provider identifier (e.g. `ollama`, `krutrim`, `sarvam`).
    pub provider: &'static str,
    /// Model identifier as configured against `provider`.
    pub model: &'static str,
    /// Short rationale for why this is a sensible planner choice.
    pub note: &'static str,
}

/// Curated planner presets, ordered from most-local/open to hosted.
///
/// Ids deliberately overlap the model knowledge already in the binary:
/// `qwen2.5-coder` and `deepseek-r1` mirror [`crate::providers::ollama`] and
/// [`crate::model_registry`]; `DeepSeek-R1` / `sarvam-m` mirror the declarative
/// Krutrim and Sarvam provider definitions.
pub static PLANNER_PRESETS: &[PlannerPreset] = &[
    PlannerPreset {
        id: "local-qwen-coder",
        provider: "ollama",
        model: "qwen2.5-coder",
        note: "Local open-weight coder model via Ollama; private, no per-token cost.",
    },
    PlannerPreset {
        id: "local-deepseek-r1",
        provider: "ollama",
        model: "deepseek-r1",
        note: "Local DeepSeek-R1 reasoning model via Ollama; strong step-by-step planning.",
    },
    PlannerPreset {
        id: "krutrim-deepseek-r1",
        provider: "krutrim",
        model: "DeepSeek-R1",
        note: "India-hosted (Krutrim/Ola) DeepSeek-R1 reasoning model.",
    },
    PlannerPreset {
        id: "sarvam-m",
        provider: "sarvam",
        model: "sarvam-m",
        note: "India-hosted Sarvam reasoning model.",
    },
];

/// All curated planner presets.
pub fn list_presets() -> &'static [PlannerPreset] {
    PLANNER_PRESETS
}

/// Look up a preset by its stable `id`.
pub fn preset_by_id(id: &str) -> Option<&'static PlannerPreset> {
    PLANNER_PRESETS.iter().find(|p| p.id == id)
}

/// Resolve the configured planner model, if any.
///
/// Reads `BHARATCODE_PLANNER_MODEL` (env-first). Returns `None` when the
/// variable is unset or blank, keeping the feature inert by default. When set,
/// the value is interpreted as:
///
/// * a `provider/model` pair (split on the first `/`), or
/// * a bare preset id resolved through [`PLANNER_PRESETS`].
///
/// Returns `(provider, model)` on a successful parse.
pub fn resolve_planner() -> Option<(String, String)> {
    let raw = std::env::var(ENV_VAR).ok()?;
    resolve_planner_value(&raw)
}

/// Pure resolver over an explicit value (testable without touching the env).
pub fn resolve_planner_value(raw: &str) -> Option<(String, String)> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    match raw.split_once('/') {
        Some((provider, model)) => {
            let provider = provider.trim();
            let model = model.trim();
            if provider.is_empty() || model.is_empty() {
                return None;
            }
            Some((provider.to_string(), model.to_string()))
        }
        None => preset_by_id(raw).map(|p| (p.provider.to_string(), p.model.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialize tests that mutate the shared process environment.
    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|p| p.into_inner())
    }

    #[test]
    fn presets_non_empty_and_well_formed() {
        let presets = list_presets();
        assert!(!presets.is_empty(), "expected at least one planner preset");
        for p in presets {
            assert!(!p.id.is_empty(), "preset id must be non-empty");
            assert!(
                !p.provider.is_empty(),
                "preset {} must have a non-empty provider",
                p.id
            );
            assert!(
                !p.model.is_empty(),
                "preset {} must have a non-empty model",
                p.id
            );
        }
    }

    #[test]
    fn preset_ids_are_unique() {
        let mut ids: Vec<&str> = list_presets().iter().map(|p| p.id).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "preset ids must be unique");
    }

    #[test]
    fn resolve_planner_inert_when_env_unset() {
        let _guard = env_guard();
        std::env::remove_var(ENV_VAR);
        assert_eq!(resolve_planner(), None);
    }

    #[test]
    fn resolve_planner_parses_provider_model_pair() {
        let _guard = env_guard();
        std::env::set_var(ENV_VAR, "ollama/qwen2.5-coder");
        assert_eq!(
            resolve_planner(),
            Some(("ollama".to_string(), "qwen2.5-coder".to_string()))
        );
        std::env::remove_var(ENV_VAR);
    }

    #[test]
    fn resolve_value_handles_bare_preset_id_and_junk() {
        assert_eq!(
            resolve_planner_value("local-qwen-coder"),
            Some(("ollama".to_string(), "qwen2.5-coder".to_string()))
        );
        assert_eq!(resolve_planner_value("   "), None);
        assert_eq!(resolve_planner_value("not-a-known-preset"), None);
        assert_eq!(
            resolve_planner_value("  krutrim/DeepSeek-R1  "),
            Some(("krutrim".to_string(), "DeepSeek-R1".to_string()))
        );
    }
}
