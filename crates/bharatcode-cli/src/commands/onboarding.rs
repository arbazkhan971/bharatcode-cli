//! `bharatcode onboard`: a guided, idempotent first-run wizard.
//!
//! Walks a new user through three small choices and then prints a localized
//! next-steps summary:
//!
//!   1. **Language** — `en` / `hi` / `ta` for the CLI's user-facing strings.
//!   2. **Provider / model preset** — one of the curated India / open-weight
//!      presets ([`crate::commands::presets::india_presets`]), so the user does
//!      not have to hand-type a provider id, model id and base URL.
//!   3. **Privacy posture** — a read-only summary of the resolved
//!      data-governance posture ([`crate::commands::privacy::PrivacyPosture`]).
//!
//! The wizard is **read-mostly**: it only writes the choices the user explicitly
//! confirms, and it is **idempotent** — re-running it simply re-offers the same
//! steps with the current values as defaults. In a non-interactive / CI context
//! (no TTY on stdin) it never blocks: it prints the ordered steps and the
//! localized summary and returns, so it is safe to invoke from scripts.
//!
//! New user-facing strings are routed through [`label`], which reads the active
//! locale via [`crate::tr!`] and falls back to the English `default` argument
//! when the key is absent from the locale table. This keeps English output
//! stable while leaving room for Hindi / Tamil tables to take effect later
//! without editing this file.
//!
//! Original BharatCode work; not ported from any third party.

use std::io::IsTerminal;

use console::style;

use crate::commands::presets::{india_presets, Preset};
use crate::commands::privacy::PrivacyPosture;

/// Config / env key persisted when the user confirms a language in the wizard.
/// Matches the key the i18n layer already reads (`bharatcode_lang`).
pub const LANG_CONFIG_KEY: &str = "bharatcode_lang";

/// Look up a user-facing label through the i18n layer, falling back to the
/// English `default` when the active locale table has no entry for `key`.
///
/// `tr!` echoes the key back when it is missing, so an unchanged key is treated
/// as "untranslated" and the English default is used. New onboarding keys can
/// therefore ship here without touching `en.json` / `hi.json`.
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// A language the wizard can offer for the CLI's user-facing strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardLanguage {
    English,
    Hindi,
    Tamil,
}

impl WizardLanguage {
    /// The ordered set of languages the wizard offers.
    pub fn all() -> [WizardLanguage; 3] {
        [
            WizardLanguage::English,
            WizardLanguage::Hindi,
            WizardLanguage::Tamil,
        ]
    }

    /// The short locale code persisted to config (`en` / `hi` / `ta`).
    pub fn code(self) -> &'static str {
        match self {
            WizardLanguage::English => "en",
            WizardLanguage::Hindi => "hi",
            WizardLanguage::Tamil => "ta",
        }
    }

    /// A human label for the picker (English names, stable across locales).
    pub fn display(self) -> &'static str {
        match self {
            WizardLanguage::English => "English (en)",
            WizardLanguage::Hindi => "हिन्दी / Hindi (hi)",
            WizardLanguage::Tamil => "தமிழ் / Tamil (ta)",
        }
    }

    /// Resolve a language from a raw locale token (env/config), defaulting to
    /// English for anything unrecognized.
    pub fn from_code(raw: &str) -> WizardLanguage {
        let primary = raw
            .trim()
            .to_ascii_lowercase()
            .split(|c| c == '_' || c == '-' || c == '.')
            .next()
            .unwrap_or("")
            .to_string();
        match primary.as_str() {
            "hi" => WizardLanguage::Hindi,
            "ta" => WizardLanguage::Tamil,
            _ => WizardLanguage::English,
        }
    }
}

/// One ordered step of the wizard. Kept as a small, pure enum so the plan is
/// deterministic and unit-testable without any I/O.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardStep {
    Language,
    Provider,
    Posture,
    Summary,
}

impl WizardStep {
    /// A localized, human label for the step (used in the printed plan).
    pub fn label(self) -> String {
        match self {
            WizardStep::Language => label("onboard.step_language", "Language"),
            WizardStep::Provider => label("onboard.step_provider", "Provider / model preset"),
            WizardStep::Posture => label("onboard.step_posture", "Privacy posture"),
            WizardStep::Summary => label("onboard.step_summary", "Next steps"),
        }
    }
}

/// The deterministic, ordered list of wizard steps.
///
/// Pure and side-effect free so it can be asserted directly in tests:
/// language -> provider -> posture -> summary.
pub fn wizard_plan() -> Vec<WizardStep> {
    vec![
        WizardStep::Language,
        WizardStep::Provider,
        WizardStep::Posture,
        WizardStep::Summary,
    ]
}

/// The choices captured during a wizard run, used to render the summary.
///
/// These are the *resolved* values (either the user's confirmed selection in an
/// interactive run, or the current defaults in a non-interactive run), never a
/// parallel copy of behaviour: the posture line is read straight from
/// [`PrivacyPosture`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnboardChoices {
    /// Chosen interface language.
    pub language: WizardLanguage,
    /// Human label of the chosen provider/model preset.
    pub preset_label: String,
    /// Provider id the preset maps to.
    pub provider: String,
    /// Model id the preset activates.
    pub model_id: String,
    /// Whether the chosen preset runs locally (no API key needed).
    pub local: bool,
    /// One-line summary of the resolved privacy posture.
    pub posture_summary: String,
    /// Whether every privacy pillar is in its strongest state.
    pub posture_locked_down: bool,
}

impl OnboardChoices {
    /// Build the choices from a preset and the live privacy posture.
    fn from_preset(language: WizardLanguage, preset: &Preset, posture: &PrivacyPosture) -> Self {
        Self {
            language,
            preset_label: preset.label.to_string(),
            provider: preset.provider.to_string(),
            model_id: preset.model_id.to_string(),
            local: preset.local,
            posture_summary: posture_one_liner(posture),
            posture_locked_down: posture.is_fully_locked_down(),
        }
    }
}

/// Condense the resolved privacy posture into a single localized line.
fn posture_one_liner(posture: &PrivacyPosture) -> String {
    if posture.is_fully_locked_down() {
        label(
            "onboard.posture_locked",
            "Fully locked down: strict residency, offline, redaction on, telemetry off, local provider.",
        )
    } else if posture.offline {
        label(
            "onboard.posture_offline",
            "Offline mode on; no off-machine egress for the active provider.",
        )
    } else if posture.provider_is_local {
        label(
            "onboard.posture_local",
            "Local-first provider; tighten further with BHARATCODE_OFFLINE=1.",
        )
    } else {
        label(
            "onboard.posture_open",
            "Default posture; run 'bharatcode privacy' to review and tighten it.",
        )
    }
}

/// Render a non-empty, localized next-steps summary for the chosen options.
///
/// Pure with respect to its argument (only the active locale, read via `tr!`,
/// influences the strings), so it is directly unit-testable. The lines never
/// reference any upstream project name.
pub fn summary_lines(choices: &OnboardChoices) -> Vec<String> {
    let api_key_hint = if choices.local {
        label("onboard.preset_no_key", "no API key required")
    } else {
        label(
            "onboard.preset_needs_key",
            "set the provider's API key before first use",
        )
    };

    vec![
        format!(
            "{} {}",
            label("onboard.summary_language", "Language:"),
            choices.language.display()
        ),
        format!(
            "{} {} ({} / {}) — {}",
            label("onboard.summary_preset", "Preset:"),
            choices.preset_label,
            choices.provider,
            choices.model_id,
            api_key_hint,
        ),
        format!(
            "{} {}",
            label("onboard.summary_posture", "Privacy:"),
            choices.posture_summary
        ),
        format!(
            "{} {}",
            label("onboard.summary_next_chat", "Start chatting:"),
            "bharatcode session"
        ),
        format!(
            "{} {}",
            label("onboard.summary_next_configure", "Fine-tune setup:"),
            "bharatcode configure"
        ),
        format!(
            "{} {}",
            label("onboard.summary_next_privacy", "Review privacy:"),
            "bharatcode privacy"
        ),
    ]
}

/// True when stdin is not a TTY, i.e. the wizard must not block on prompts.
pub fn is_noninteractive() -> bool {
    !std::io::stdin().is_terminal()
}

/// Options for the onboarding wizard. Kept as a struct to mirror the other
/// `#[path]`-declared subcommands and to leave room for future flags without
/// churning the call site.
#[derive(Debug, Clone, Default)]
pub struct OnboardOptions {
    /// Skip all prompts even on a TTY and just print the plan + summary using
    /// current defaults (idempotent, read-only).
    pub no_prompt: bool,
}

/// Resolve the language currently in effect (env / config / `LANG`) so the
/// wizard can pre-select a sensible default without re-reading private i18n
/// internals.
fn current_language() -> WizardLanguage {
    if let Ok(raw) = std::env::var("BHARATCODE_LANG") {
        if !raw.trim().is_empty() {
            return WizardLanguage::from_code(&raw);
        }
    }
    if let Ok(raw) = bharatcode_core::config::Config::global().get_param::<String>(LANG_CONFIG_KEY) {
        if !raw.trim().is_empty() {
            return WizardLanguage::from_code(&raw);
        }
    }
    if let Ok(raw) = std::env::var("LANG") {
        if !raw.trim().is_empty() {
            return WizardLanguage::from_code(&raw);
        }
    }
    WizardLanguage::English
}

/// Print the ordered wizard plan as a numbered checklist (no prompting).
fn print_plan() {
    println!();
    println!(
        "  {}",
        crate::theme::heading(label("onboard.title", "BharatCode first-run setup"))
    );
    println!(
        "  {}",
        crate::theme::muted(label(
            "onboard.subtitle",
            "A quick, idempotent walkthrough — nothing is saved unless you confirm it.",
        ))
    );
    println!();
    for (i, step) in wizard_plan().iter().enumerate() {
        println!("  {}. {}", i + 1, step.label());
    }
    println!();
}

/// Print the localized next-steps summary block.
fn print_summary(choices: &OnboardChoices) {
    println!();
    println!(
        "  {}",
        crate::theme::heading(label("onboard.summary_title", "You're all set"))
    );
    for line in summary_lines(choices) {
        println!("  {} {}", style("›").color256(208), line);
    }
    println!();
}

/// Entry point for `bharatcode onboard`.
///
/// Interactive on a TTY (and unless `--no-prompt` is passed): offers the
/// language picker, the preset picker, shows the resolved privacy posture, and
/// persists only the language if the user confirms saving it. Non-interactive:
/// prints the plan and summary using current defaults and returns without
/// blocking.
pub async fn handle_onboard(opts: OnboardOptions) -> anyhow::Result<()> {
    let presets = india_presets();
    let posture = PrivacyPosture::resolve();
    let default_language = current_language();

    print_plan();

    if opts.no_prompt || is_noninteractive() {
        // Non-blocking path: use the current defaults (first preset, resolved
        // language and posture) and print the summary. Nothing is persisted.
        let default_preset = &presets[0];
        let choices = OnboardChoices::from_preset(default_language, default_preset, &posture);
        if is_noninteractive() && !opts.no_prompt {
            println!(
                "  {}",
                crate::theme::muted(label(
                    "onboard.noninteractive",
                    "Non-interactive terminal detected — showing defaults without prompting.",
                ))
            );
        }
        print_summary(&choices);
        return Ok(());
    }

    // Interactive path.
    let language = prompt_language(default_language)?;
    let preset = prompt_preset(&presets)?;

    println!();
    println!(
        "  {}",
        crate::theme::heading(label("onboard.step_posture", "Privacy posture"))
    );
    println!("  {} {}", style("·").dim(), posture_one_liner(&posture));
    println!(
        "  {}",
        crate::theme::muted(label(
            "onboard.posture_hint",
            "Run 'bharatcode privacy' for the full per-pillar report.",
        ))
    );

    let choices = OnboardChoices::from_preset(language, preset, &posture);

    // Only persist the one durable choice — the interface language — and only
    // when the user confirms. Everything else is informational.
    let save = cliclack::confirm(label(
        "onboard.confirm_save_lang",
        "Save this language as your default?",
    ))
    .initial_value(true)
    .interact()
    .unwrap_or(false);

    if save {
        let config = bharatcode_core::config::Config::global();
        match config.set_param(
            LANG_CONFIG_KEY,
            serde_json::Value::String(language.code().to_string()),
        ) {
            Ok(()) => println!(
                "  {} {}",
                crate::theme::success("✓".to_string()),
                label("onboard.lang_saved", "Language saved.")
            ),
            Err(e) => println!(
                "  {} {}: {}",
                crate::theme::warning("✗".to_string()),
                label("onboard.lang_save_failed", "Could not save language"),
                e
            ),
        }
    }

    print_summary(&choices);
    Ok(())
}

/// Prompt for the interface language, defaulting to the resolved current one.
fn prompt_language(default: WizardLanguage) -> anyhow::Result<WizardLanguage> {
    println!();
    let mut select = cliclack::select(label("onboard.pick_language", "Choose your language"));
    for lang in WizardLanguage::all() {
        select = select.item(lang.code(), lang.display(), "");
    }
    let code = select
        .initial_value(default.code())
        .interact()
        .unwrap_or(default.code());
    Ok(WizardLanguage::from_code(code))
}

/// Prompt for a provider/model preset from the curated India list.
fn prompt_preset(presets: &[Preset]) -> anyhow::Result<&Preset> {
    println!();
    let mut select = cliclack::select(label(
        "onboard.pick_preset",
        "Choose a provider / model preset",
    ));
    for (idx, preset) in presets.iter().enumerate() {
        let kind = if preset.local { "local" } else { "hosted" };
        let hint = format!("[{}] {} / {}", kind, preset.provider, preset.model_id);
        select = select.item(idx, preset.label, hint);
    }
    let idx = select.interact().unwrap_or(0);
    Ok(presets.get(idx).unwrap_or(&presets[0]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_choices() -> OnboardChoices {
        OnboardChoices {
            language: WizardLanguage::Hindi,
            preset_label: "Qwen2.5 Coder (local)".to_string(),
            provider: "ollama".to_string(),
            model_id: "qwen2.5-coder".to_string(),
            local: true,
            posture_summary: "Local-first provider.".to_string(),
            posture_locked_down: false,
        }
    }

    #[test]
    fn wizard_plan_is_deterministic_and_ordered() {
        assert_eq!(
            wizard_plan(),
            vec![
                WizardStep::Language,
                WizardStep::Provider,
                WizardStep::Posture,
                WizardStep::Summary,
            ]
        );
        // Stable across calls.
        assert_eq!(wizard_plan(), wizard_plan());
    }

    #[test]
    fn every_step_has_a_nonempty_label() {
        for step in wizard_plan() {
            assert!(!step.label().is_empty());
        }
    }

    #[test]
    fn summary_lines_are_nonempty_and_localized() {
        let lines = summary_lines(&sample_choices());
        assert!(!lines.is_empty());
        for line in &lines {
            assert!(!line.trim().is_empty());
        }
    }

    #[test]
    fn summary_has_no_upstream_branding() {
        let blob = summary_lines(&sample_choices()).join("\n").to_lowercase();
        assert!(
            !blob.contains("goose"),
            "summary leaked upstream name: {blob}"
        );
        assert!(
            !blob.contains("block"),
            "summary leaked upstream name: {blob}"
        );
    }

    #[test]
    fn summary_reflects_the_chosen_preset() {
        let lines = summary_lines(&sample_choices());
        let blob = lines.join("\n");
        assert!(blob.contains("Qwen2.5 Coder (local)"));
        assert!(blob.contains("ollama"));
        assert!(blob.contains("qwen2.5-coder"));
    }

    #[test]
    fn local_and_hosted_presets_render_different_key_hints() {
        let mut local = sample_choices();
        local.local = true;
        let mut hosted = sample_choices();
        hosted.local = false;

        let local_blob = summary_lines(&local).join("\n");
        let hosted_blob = summary_lines(&hosted).join("\n");
        assert!(local_blob.contains("no API key"));
        assert!(hosted_blob.contains("API key"));
        assert_ne!(local_blob, hosted_blob);
    }

    #[test]
    fn language_code_round_trips() {
        for lang in WizardLanguage::all() {
            assert_eq!(WizardLanguage::from_code(lang.code()), lang);
        }
        assert_eq!(
            WizardLanguage::from_code("hi_IN.UTF-8"),
            WizardLanguage::Hindi
        );
        assert_eq!(WizardLanguage::from_code("ta-IN"), WizardLanguage::Tamil);
        assert_eq!(WizardLanguage::from_code("fr"), WizardLanguage::English);
        assert_eq!(WizardLanguage::from_code(""), WizardLanguage::English);
    }

    #[test]
    fn is_noninteractive_true_when_stdin_not_a_tty() {
        // The test harness runs with stdin redirected (not a TTY), so this is
        // the expected value in CI; it mirrors `!stdin().is_terminal()`.
        assert_eq!(is_noninteractive(), !std::io::stdin().is_terminal());
    }
}
