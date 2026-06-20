//! Static model registry with cost & capability metadata for BharatCode.
//!
//! BharatCode users frequently mix India-built and open-weight models
//! (Sarvam, Krutrim, Llama / Qwen / DeepSeek) with a handful of common cloud
//! models. To surface useful, rupee-denominated cost information (see
//! `bharatcode cost`) we keep a small, curated, in-binary table describing each
//! model's provider, context window, list price (USD per 1M input / output
//! tokens) and broad capabilities.
//!
//! The table is intentionally static and conservative: prices and context
//! windows are published list figures captured for reference and are *not*
//! billing-authoritative. They give the cost surface a sensible default to show
//! when a model is recognised; when a model is unknown, callers simply skip the
//! registry section and default behaviour is unchanged.
//!
//! Original BharatCode work; not ported from any third party.

/// Broad capability flags for a model. Kept coarse on purpose: this is metadata
/// for display and rough routing hints, not a precise feature matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// Model accepts/produces tool (function) calls.
    pub tools: bool,
    /// Model accepts image input (multimodal vision).
    pub vision: bool,
    /// Model is distributed as open weights (self-hostable).
    pub open_weights: bool,
}

impl Capabilities {
    const fn new(tools: bool, vision: bool, open_weights: bool) -> Self {
        Self {
            tools,
            vision,
            open_weights,
        }
    }
}

/// Static metadata describing a single known model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelInfo {
    /// Canonical model identifier (lower-case, as commonly configured).
    pub name: &'static str,
    /// Human-friendly provider / origin label.
    pub provider: &'static str,
    /// Maximum context window in tokens.
    pub context_window: u32,
    /// List price in USD per 1,000,000 input tokens.
    pub input_per_1m_usd: f64,
    /// List price in USD per 1,000,000 output tokens.
    pub output_per_1m_usd: f64,
    /// Coarse capability flags.
    pub capabilities: Capabilities,
}

impl ModelInfo {
    /// USD price for 1,000 input tokens.
    pub fn input_per_1k_usd(&self) -> f64 {
        self.input_per_1m_usd / 1_000.0
    }

    /// USD price for 1,000 output tokens.
    pub fn output_per_1k_usd(&self) -> f64 {
        self.output_per_1m_usd / 1_000.0
    }

    /// INR price for 1,000 input tokens at the given USD->INR `rate`.
    pub fn input_per_1k_inr(&self, rate: f64) -> f64 {
        self.input_per_1k_usd() * rate
    }

    /// INR price for 1,000 output tokens at the given USD->INR `rate`.
    pub fn output_per_1k_inr(&self, rate: f64) -> f64 {
        self.output_per_1k_usd() * rate
    }
}

const fn info(
    name: &'static str,
    provider: &'static str,
    context_window: u32,
    input_per_1m_usd: f64,
    output_per_1m_usd: f64,
    capabilities: Capabilities,
) -> ModelInfo {
    ModelInfo {
        name,
        provider,
        context_window,
        input_per_1m_usd,
        output_per_1m_usd,
        capabilities,
    }
}

/// The static registry table.
///
/// Curated, reference-only list prices (USD / 1M tokens) and published context
/// windows. India-built and open-weight models first, then a few common cloud
/// models. Open-weight "prices" reflect representative hosted-inference list
/// figures; self-hosted cost will differ.
static REGISTRY: &[ModelInfo] = &[
    // --- India-built ---
    info(
        "sarvam-m",
        "Sarvam AI (India)",
        32_768,
        0.50,
        1.50,
        Capabilities::new(true, false, true),
    ),
    info(
        "sarvam-2b",
        "Sarvam AI (India)",
        8_192,
        0.10,
        0.20,
        Capabilities::new(false, false, true),
    ),
    info(
        "krutrim-2",
        "Krutrim / Ola (India)",
        32_768,
        0.40,
        1.20,
        Capabilities::new(true, false, false),
    ),
    info(
        "krutrim-spectre",
        "Krutrim / Ola (India)",
        16_384,
        0.30,
        0.90,
        Capabilities::new(true, false, false),
    ),
    // --- Open-weight (Llama / Qwen / DeepSeek) ---
    info(
        "llama-3.1-8b",
        "Meta (open weights)",
        131_072,
        0.05,
        0.08,
        Capabilities::new(true, false, true),
    ),
    info(
        "llama-3.1-70b",
        "Meta (open weights)",
        131_072,
        0.40,
        0.40,
        Capabilities::new(true, false, true),
    ),
    info(
        "llama-3.3-70b",
        "Meta (open weights)",
        131_072,
        0.40,
        0.40,
        Capabilities::new(true, false, true),
    ),
    info(
        "qwen2.5-7b",
        "Alibaba Qwen (open weights)",
        131_072,
        0.05,
        0.10,
        Capabilities::new(true, false, true),
    ),
    info(
        "qwen2.5-72b",
        "Alibaba Qwen (open weights)",
        131_072,
        0.40,
        0.40,
        Capabilities::new(true, true, true),
    ),
    info(
        "qwen2.5-coder-32b",
        "Alibaba Qwen (open weights)",
        131_072,
        0.18,
        0.18,
        Capabilities::new(true, false, true),
    ),
    info(
        "deepseek-v3",
        "DeepSeek (open weights)",
        65_536,
        0.27,
        1.10,
        Capabilities::new(true, false, true),
    ),
    info(
        "deepseek-r1",
        "DeepSeek (open weights)",
        65_536,
        0.55,
        2.19,
        Capabilities::new(true, false, true),
    ),
    info(
        "deepseek-coder-v2",
        "DeepSeek (open weights)",
        131_072,
        0.14,
        0.28,
        Capabilities::new(true, false, true),
    ),
    // --- Common cloud models ---
    info(
        "gpt-4o",
        "OpenAI",
        128_000,
        2.50,
        10.00,
        Capabilities::new(true, true, false),
    ),
    info(
        "gpt-4o-mini",
        "OpenAI",
        128_000,
        0.15,
        0.60,
        Capabilities::new(true, true, false),
    ),
    info(
        "claude-3-5-sonnet",
        "Anthropic",
        200_000,
        3.00,
        15.00,
        Capabilities::new(true, true, false),
    ),
    info(
        "claude-3-5-haiku",
        "Anthropic",
        200_000,
        0.80,
        4.00,
        Capabilities::new(true, true, false),
    ),
    info(
        "gemini-1.5-pro",
        "Google",
        2_000_000,
        1.25,
        5.00,
        Capabilities::new(true, true, false),
    ),
    info(
        "gemini-1.5-flash",
        "Google",
        1_000_000,
        0.075,
        0.30,
        Capabilities::new(true, true, false),
    ),
];

/// Normalise a configured model name for matching: lower-case, trimmed, and
/// with a leading `provider/` prefix (e.g. `openai/gpt-4o`) stripped.
fn normalise(name: &str) -> String {
    let lower = name.trim().to_ascii_lowercase();
    match lower.rsplit_once('/') {
        Some((_, tail)) if !tail.is_empty() => tail.to_string(),
        _ => lower,
    }
}

/// Look up a model by name.
///
/// Matching is tolerant: it is case-insensitive, ignores a leading
/// `provider/` prefix, and falls back to a prefix match against the registry
/// key (so e.g. `llama-3.1-8b-instruct` resolves to `llama-3.1-8b`). Returns
/// `None` when nothing reasonable matches, so callers can preserve default
/// behaviour for unknown models.
pub fn lookup(name: &str) -> Option<&'static ModelInfo> {
    let key = normalise(name);
    if key.is_empty() {
        return None;
    }
    // Exact (normalised) match first.
    if let Some(m) = REGISTRY.iter().find(|m| m.name == key) {
        return Some(m);
    }
    // Then prefix match: the configured name starts with a known key, picking
    // the longest such key to avoid e.g. `llama-3.1-8b` shadowing `...-70b`.
    REGISTRY
        .iter()
        .filter(|m| key.starts_with(m.name))
        .max_by_key(|m| m.name.len())
}

/// Return every model in the registry (declaration order).
pub fn all() -> &'static [ModelInfo] {
    REGISTRY
}

/// Number of models in the registry.
pub fn len() -> usize {
    REGISTRY.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_non_empty_and_covers_focus_models() {
        assert!(len() >= 12);
        // India-built + open-weight families must be present.
        assert!(lookup("sarvam-m").is_some());
        assert!(lookup("krutrim-2").is_some());
        assert!(lookup("llama-3.1-8b").is_some());
        assert!(lookup("qwen2.5-72b").is_some());
        assert!(lookup("deepseek-v3").is_some());
        // A couple of common cloud models.
        assert!(lookup("gpt-4o").is_some());
        assert!(lookup("claude-3-5-sonnet").is_some());
    }

    #[test]
    fn lookup_is_case_insensitive_and_strips_provider_prefix() {
        let a = lookup("GPT-4o").unwrap();
        let b = lookup("openai/gpt-4o").unwrap();
        let c = lookup("  gpt-4o  ").unwrap();
        assert_eq!(a.name, "gpt-4o");
        assert_eq!(a, b);
        assert_eq!(a, c);
    }

    #[test]
    fn lookup_prefix_matches_longest_key() {
        // An instruct variant should resolve to its base key.
        let m = lookup("llama-3.1-8b-instruct").unwrap();
        assert_eq!(m.name, "llama-3.1-8b");
        // The 70b variant must not be shadowed by the 8b key.
        let m = lookup("llama-3.1-70b-instruct").unwrap();
        assert_eq!(m.name, "llama-3.1-70b");
    }

    #[test]
    fn unknown_model_returns_none() {
        assert!(lookup("totally-made-up-model").is_none());
        assert!(lookup("").is_none());
        assert!(lookup("   ").is_none());
    }

    #[test]
    fn per_1k_usd_is_per_1m_over_thousand() {
        let m = lookup("gpt-4o").unwrap();
        assert!((m.input_per_1k_usd() - 0.0025).abs() < 1e-12);
        assert!((m.output_per_1k_usd() - 0.010).abs() < 1e-12);
    }

    #[test]
    fn per_1k_inr_scales_with_rate() {
        let m = lookup("gpt-4o").unwrap();
        let rate = 88.0;
        // 2.50 USD / 1M input => 0.0025 USD / 1k => * 88 = 0.22 INR / 1k.
        assert!((m.input_per_1k_inr(rate) - 0.22).abs() < 1e-9);
        // 10.00 USD / 1M output => 0.010 USD / 1k => * 88 = 0.88 INR / 1k.
        assert!((m.output_per_1k_inr(rate) - 0.88).abs() < 1e-9);
    }

    #[test]
    fn all_prices_and_windows_are_sane() {
        for m in all() {
            assert!(!m.name.is_empty());
            assert!(!m.provider.is_empty());
            assert!(m.context_window > 0);
            assert!(m.input_per_1m_usd >= 0.0 && m.input_per_1m_usd.is_finite());
            assert!(m.output_per_1m_usd >= 0.0 && m.output_per_1m_usd.is_finite());
        }
    }
}
