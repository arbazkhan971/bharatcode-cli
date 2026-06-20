//! Curated India / open-weight model presets.
//!
//! These let users pick a recommended model quickly during onboarding without
//! hand-typing a provider id, model id and base URL. Presets are pure data and
//! map onto existing providers:
//!   - Local presets use the bundled `ollama` provider (localhost:11434), which
//!     needs no API key.
//!   - Hosted India/Asia presets use OpenAI-compatible declarative providers
//!     (Sarvam, Krutrim, Alibaba/Qwen, DeepSeek), each gated by its own API-key
//!     environment variable. No secrets are embedded here.
//!
//! Original BharatCode work; not ported from any third party.

use console::style;

/// A single recommended model preset.
pub struct Preset {
    /// Short human label shown in the picker.
    pub label: &'static str,
    /// The provider id this preset maps to (e.g. `ollama`, `sarvam`).
    pub provider: &'static str,
    /// The model id to activate for the provider.
    pub model_id: &'static str,
    /// Optional base URL hint (informational; the provider config owns the real URL).
    pub base_url: Option<&'static str>,
    /// Whether the preset runs locally (no API key needed).
    pub local: bool,
    /// One-line note describing the preset.
    pub note: &'static str,
}

/// The curated list of India / open-weight presets, local first.
pub fn india_presets() -> Vec<Preset> {
    vec![
        Preset {
            label: "Qwen2.5 Coder (local)",
            provider: "ollama",
            model_id: "qwen2.5-coder",
            base_url: Some("http://localhost:11434"),
            local: true,
            note: "Open-weight coding model, runs locally via Ollama. No API key.",
        },
        Preset {
            label: "Qwen2.5 Coder 7B (local)",
            provider: "ollama",
            model_id: "qwen2.5-coder:7b",
            base_url: Some("http://localhost:11434"),
            local: true,
            note: "Lighter 7B variant for modest hardware, via Ollama. No API key.",
        },
        Preset {
            label: "DeepSeek Coder (local)",
            provider: "ollama",
            model_id: "deepseek-coder",
            base_url: Some("http://localhost:11434"),
            local: true,
            note: "Open-weight DeepSeek coding model, runs locally via Ollama. No API key.",
        },
        Preset {
            label: "Sarvam-M (India-hosted)",
            provider: "sarvam",
            model_id: "sarvam-m",
            base_url: Some("https://api.sarvam.ai/v1"),
            local: false,
            note: "India-hosted Sarvam model. Needs SARVAM_API_KEY.",
        },
        Preset {
            label: "Krutrim Llama-3.3 70B (India-hosted)",
            provider: "krutrim",
            model_id: "Llama-3.3-70B-Instruct",
            base_url: Some("https://cloud.krutrim.com/v1"),
            local: false,
            note: "Ola Krutrim cloud, India-hosted. Needs KRUTRIM_API_KEY.",
        },
        Preset {
            label: "Qwen3 Coder Plus (Alibaba/DashScope)",
            provider: "alibaba",
            model_id: "qwen3-coder-plus",
            base_url: Some("https://dashscope-intl.aliyuncs.com/compatible-mode/v1"),
            local: false,
            note: "Asia-hosted Qwen via DashScope. Needs DASHSCOPE_API_KEY.",
        },
        Preset {
            label: "DeepSeek Chat (DeepSeek API)",
            provider: "custom_deepseek",
            model_id: "deepseek-chat",
            base_url: Some("https://api.deepseek.com"),
            local: false,
            note: "Hosted DeepSeek API. Needs DEEPSEEK_API_KEY.",
        },
    ]
}

/// Print the curated presets as a simple, human-readable listing.
pub fn print_presets() {
    println!("{}", style(crate::tr!("presets.header")).cyan().bold());
    println!();

    let presets = india_presets();
    let label_width = presets
        .iter()
        .map(|p| p.label.len())
        .max()
        .unwrap_or(0)
        .max(12);

    for preset in presets {
        let kind = if preset.local {
            style("local").green().to_string()
        } else {
            style("hosted").yellow().to_string()
        };
        println!(
            "  {:<label_width$}  [{}] {} / {}",
            style(preset.label).bold(),
            kind,
            preset.provider,
            preset.model_id,
            label_width = label_width,
        );
        println!("  {:<label_width$}  {}", "", style(preset.note).dim());
        if let Some(url) = preset.base_url {
            println!("  {:<label_width$}  {}", "", style(url).dim());
        }
        println!();
    }

    println!("{}", style(crate::tr!("presets.footer")).dim());
}
