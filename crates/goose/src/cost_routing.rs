//! Cost-aware model routing (BharatCode v18).
//!
//! Opt-in routing that, when enabled, prefers the cheapest *capable* model —
//! and local / open-weight models in particular — from a configured set of
//! candidates. Routing is **off by default**: with the flag unset, every
//! helper returns the caller's original choice unchanged.
//!
//! Enable with the `BHARATCODE_COST_ROUTING` flag (env var or config key).
//! Extra candidate models may be supplied via `BHARATCODE_COST_ROUTING_CANDIDATES`
//! as a comma-separated list of model names (same provider).
//!
//! The module is purely additive and side-effect free: it only reads
//! configuration and the bundled canonical model registry (cost metadata) to
//! rank models. It never performs network calls and never mutates state.

use crate::config::Config;
use goose_providers::canonical::maybe_get_canonical_model;

/// Flag (env var / config key) that turns cost-aware routing on. Default: off.
pub const COST_ROUTING_KEY: &str = "BHARATCODE_COST_ROUTING";

/// Optional comma-separated list of extra candidate model names that routing
/// may pick from, in addition to the caller-provided default.
pub const COST_ROUTING_CANDIDATES_KEY: &str = "BHARATCODE_COST_ROUTING_CANDIDATES";

/// Returns `true` only when cost-aware routing has been explicitly enabled.
///
/// Accepts the usual truthy spellings (`true`, `1`, `yes`, `on`) from either an
/// environment variable or the persisted config. Any error, absence, or falsey
/// value leaves routing disabled.
pub fn cost_routing_enabled() -> bool {
    Config::global()
        .get_param::<serde_yaml::Value>(COST_ROUTING_KEY)
        .ok()
        .map(|v| flag_is_truthy(&v))
        .unwrap_or(false)
}

fn flag_is_truthy(value: &serde_yaml::Value) -> bool {
    match value {
        serde_yaml::Value::Bool(b) => *b,
        serde_yaml::Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        serde_yaml::Value::String(s) => {
            matches!(
                s.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        }
        _ => false,
    }
}

/// How a candidate ranks for routing. Lower sorts first (more preferred).
///
/// Ordered lexicographically by:
/// 1. `capability_penalty` — `1` when the model is known to lack tool calling
///    but tool calling was required, otherwise `0`. Keeps incapable models as a
///    last resort rather than dropping them entirely.
/// 2. `locality_tier` — `0` for local / open-weight / zero-cost models, `1`
///    for priced cloud models. This is what gives local open-weight models the
///    edge the goal calls for.
/// 3. `effective_cost` — combined input + output price per million tokens.
///    Models with no cost metadata are scored as [`f64::MAX`] so a known-cheap
///    capable model always wins over an unknown one.
#[derive(Debug, Clone, Copy)]
struct RouteScore {
    capability_penalty: u8,
    locality_tier: u8,
    effective_cost: f64,
}

fn score_candidate(provider: &str, model: &str, require_tool_call: bool) -> RouteScore {
    match maybe_get_canonical_model(provider, model) {
        Some(canonical) => {
            let capability_penalty = if require_tool_call && !canonical.tool_call {
                1
            } else {
                0
            };

            // `maybe_get_canonical_model` zeroes out cost for local providers,
            // so a `None` input price means "free to run" here.
            let is_free = canonical.cost.input.unwrap_or(0.0) <= 0.0;
            let is_open_weight = canonical.open_weights.unwrap_or(false);
            let locality_tier = if is_free || is_open_weight { 0 } else { 1 };

            let effective_cost =
                canonical.cost.input.unwrap_or(0.0) + canonical.cost.output.unwrap_or(0.0);

            RouteScore {
                capability_penalty,
                locality_tier,
                effective_cost,
            }
        }
        // No metadata: keep as a viable fallback but never prefer it over a
        // model we can actually price.
        None => RouteScore {
            capability_penalty: 0,
            locality_tier: 1,
            effective_cost: f64::MAX,
        },
    }
}

fn is_more_preferred(a: &RouteScore, b: &RouteScore) -> std::cmp::Ordering {
    a.capability_penalty
        .cmp(&b.capability_penalty)
        .then(a.locality_tier.cmp(&b.locality_tier))
        .then(
            a.effective_cost
                .partial_cmp(&b.effective_cost)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
}

/// Pick the preferred model name from `default_model` plus `extra_candidates`.
///
/// When routing is disabled, returns `default_model` unchanged. Otherwise ranks
/// every distinct candidate (the default is always included and acts as the
/// stable fallback on ties) and returns the most preferred one for `provider`.
pub fn route_model(
    provider: &str,
    default_model: &str,
    extra_candidates: &[String],
    require_tool_call: bool,
) -> String {
    if !cost_routing_enabled() {
        return default_model.to_string();
    }

    let mut candidates: Vec<String> = Vec::with_capacity(extra_candidates.len() + 1);
    let mut push_unique = |name: &str| {
        let trimmed = name.trim();
        if !trimmed.is_empty() && !candidates.iter().any(|c| c.as_str() == trimmed) {
            candidates.push(trimmed.to_string());
        }
    };

    // The caller's choice goes first so it wins any exact tie (stable order).
    push_unique(default_model);
    for candidate in extra_candidates {
        push_unique(candidate);
    }

    candidates
        .into_iter()
        .map(|name| {
            let score = score_candidate(provider, &name, require_tool_call);
            (name, score)
        })
        .reduce(|best, next| {
            if is_more_preferred(&next.1, &best.1) == std::cmp::Ordering::Less {
                next
            } else {
                best
            }
        })
        .map(|(name, _)| name)
        .unwrap_or_else(|| default_model.to_string())
}

/// Read the configured list of extra routing candidates (comma-separated).
pub fn configured_candidates() -> Vec<String> {
    Config::global()
        .get_param::<String>(COST_ROUTING_CANDIDATES_KEY)
        .ok()
        .map(|raw| {
            raw.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Convenience entry point for the fast / worker model selection path.
///
/// Routes among the already-selected fast model plus any configured extra
/// candidates, requiring tool-calling capability (workers must call tools).
/// Returns `selected` unchanged when routing is disabled.
pub fn route_fast_model(provider: &str, selected: &str) -> String {
    if !cost_routing_enabled() {
        return selected.to_string();
    }
    route_model(provider, selected, &configured_candidates(), true)
}

/// Convenience entry point for the lead / primary model selection path.
///
/// Mirrors [`route_fast_model`] for the agent's main model so cost routing can
/// move the primary model too, not only the fast/worker model. Requires
/// tool-calling capability because the lead model drives the agent's tool use.
/// Returns `selected` unchanged when routing is disabled.
pub fn route_lead_model(provider: &str, selected: &str) -> String {
    if !cost_routing_enabled() {
        return selected.to_string();
    }
    route_model(provider, selected, &configured_candidates(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_routing_returns_default() {
        // Routing defaults to off, so the caller's choice is preserved even
        // when cheaper candidates exist.
        let chosen = route_model(
            "anthropic",
            "claude-3-5-sonnet-20241022",
            &["claude-3-5-haiku-20241022".to_string()],
            true,
        );
        // Without the flag set, `cost_routing_enabled()` is false.
        if !cost_routing_enabled() {
            assert_eq!(chosen, "claude-3-5-sonnet-20241022");
        }
    }

    #[test]
    fn free_local_beats_priced_cloud() {
        // Ordering is independent of the on/off flag: a free/local tier-0 model
        // must rank ahead of a priced cloud model.
        let free = RouteScore {
            capability_penalty: 0,
            locality_tier: 0,
            effective_cost: 0.0,
        };
        let priced = RouteScore {
            capability_penalty: 0,
            locality_tier: 1,
            effective_cost: 3.0,
        };
        assert_eq!(is_more_preferred(&free, &priced), std::cmp::Ordering::Less);
    }

    #[test]
    fn cheaper_priced_beats_pricier_priced() {
        let cheap = RouteScore {
            capability_penalty: 0,
            locality_tier: 1,
            effective_cost: 1.0,
        };
        let dear = RouteScore {
            capability_penalty: 0,
            locality_tier: 1,
            effective_cost: 30.0,
        };
        assert_eq!(is_more_preferred(&cheap, &dear), std::cmp::Ordering::Less);
    }

    #[test]
    fn incapable_model_is_deprioritized() {
        let capable = RouteScore {
            capability_penalty: 0,
            locality_tier: 1,
            effective_cost: 50.0,
        };
        let incapable = RouteScore {
            capability_penalty: 1,
            locality_tier: 0,
            effective_cost: 0.0,
        };
        // Even free + local loses if it cannot call tools when tools are required.
        assert_eq!(
            is_more_preferred(&capable, &incapable),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn truthy_flag_parsing() {
        assert!(flag_is_truthy(&serde_yaml::Value::Bool(true)));
        assert!(flag_is_truthy(&serde_yaml::Value::String("on".into())));
        assert!(flag_is_truthy(&serde_yaml::Value::String("YES".into())));
        assert!(!flag_is_truthy(&serde_yaml::Value::Bool(false)));
        assert!(!flag_is_truthy(&serde_yaml::Value::String("off".into())));
    }
}
