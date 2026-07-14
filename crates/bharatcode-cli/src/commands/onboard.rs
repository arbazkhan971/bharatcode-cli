//! `bharatcode onboard`: a guided, idempotent first-run onboarding wizard.
//!
//! Walks a brand-new user through four small steps and then prints a localized
//! next-steps block:
//!
//!   1. **Locale** — pick the CLI's user-facing language (`en` / `hi` / `ta`).
//!   2. **Preset** — pick a curated local-first or India-hosted provider/model
//!      preset ([`crate::commands::presets::india_presets`]) so the user never
//!      hand-types a provider id, model id or base URL.
//!   3. **Privacy** — confirm the resolved data-governance posture
//!      (offline / residency / telemetry) read live from
//!      [`crate::commands::privacy::PrivacyPosture`].
//!   4. **Next steps** — print the exact commands to start working, including a
//!      ready-to-copy `bharatcode run` line.
//!
//! The wizard only persists the two durable choices the user explicitly
//! confirms — the interface language (`bharatcode_lang`) and the active
//! provider/model — via [`bharatcode_core::config::Config`]. It is **idempotent**:
//! re-running it simply re-offers the same steps using the current values as
//! defaults. With `--non-interactive` (or when no TTY is attached, e.g. in CI)
//! it never blocks: it renders the scripted plan from the current defaults and
//! returns, so it is safe to invoke from scripts and from the test-suite.
//!
//! New user-facing strings are routed through [`label`], which reads the active
//! locale via [`crate::tr!`] and falls back to the English `default` when the
//! locale table has no entry for the key. This keeps the English output stable
//! while leaving room for Hindi / Tamil tables to take effect later without
//! editing this file. No upstream project name is ever emitted.
//!
//! Original BharatCode work; not ported from any third party.

use std::io::IsTerminal;

use console::style;

use crate::commands::presets::{india_presets, Preset};
use crate::commands::privacy::PrivacyPosture;

/// Config / env key persisted when the user confirms a language. Matches the key
/// the i18n layer already reads (`bharatcode_lang`).
pub const LANG_CONFIG_KEY: &str = "bharatcode_lang";

/// Config keys the wizard writes when the user confirms a provider/model preset.
/// These mirror the env vars `bharatcode run` already resolves.
pub const PROVIDER_CONFIG_KEY: &str = "BHARATCODE_PROVIDER";
pub const MODEL_CONFIG_KEY: &str = "BHARATCODE_MODEL";

/// Look up a user-facing label through the i18n layer, falling back to the
/// English `default` when the active locale table has no entry for `key`.
///
/// `tr!` echoes the key back when it is missing, so an unchanged key is treated
/// as "untranslated" and the English default is used. New onboarding keys can
/// therefore ship here without first touching `en.json` / `hi.json` / `ta.json`.
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// Options for the onboarding wizard. Kept as a struct to mirror the other
/// `#[path]`-declared subcommands and to leave room for future flags without
/// churning the call site.
#[derive(Debug, Clone, Default)]
pub struct OnboardOptions {
    /// Skip all prompts (even on a TTY) and just render the scripted plan from
    /// the current defaults. Idempotent and read-only; nothing is persisted.
    /// Used by tests / CI.
    pub non_interactive: bool,
}

/// A locale the wizard can offer for the CLI's user-facing strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardLocale {
    English,
    Hindi,
    Tamil,
}

impl WizardLocale {
    /// The ordered set of locales the wizard offers.
    pub fn all() -> [WizardLocale; 3] {
        [
            WizardLocale::English,
            WizardLocale::Hindi,
            WizardLocale::Tamil,
        ]
    }

    /// The short locale code persisted to config (`en` / `hi` / `ta`).
    pub fn code(self) -> &'static str {
        match self {
            WizardLocale::English => "en",
            WizardLocale::Hindi => "hi",
            WizardLocale::Tamil => "ta",
        }
    }

    /// A human label for the picker (stable across locales).
    pub fn display(self) -> &'static str {
        match self {
            WizardLocale::English => "English (en)",
            WizardLocale::Hindi => "हिन्दी / Hindi (hi)",
            WizardLocale::Tamil => "தமிழ் / Tamil (ta)",
        }
    }

    /// Resolve a locale from a raw token (env / config / `LANG`), defaulting to
    /// English for anything unrecognized.
    pub fn from_code(raw: &str) -> WizardLocale {
        let primary = raw
            .trim()
            .to_ascii_lowercase()
            .split(['_', '-', '.'])
            .next()
            .unwrap_or("")
            .to_string();
        match primary.as_str() {
            "hi" => WizardLocale::Hindi,
            "ta" => WizardLocale::Tamil,
            _ => WizardLocale::English,
        }
    }
}

/// The choices captured during a wizard run, used to render the plan/summary.
///
/// These are the *resolved* values — either the user's confirmed selection in
/// an interactive run, or the current defaults in a non-interactive run — never
/// a parallel copy of behaviour: the privacy line is read straight from
/// [`PrivacyPosture`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnboardChoices {
    /// Chosen interface locale.
    pub locale: WizardLocale,
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
}

impl OnboardChoices {
    /// Build the choices from a preset and the live privacy posture.
    fn from_preset(locale: WizardLocale, preset: &Preset, posture: &PrivacyPosture) -> Self {
        Self {
            locale,
            preset_label: preset.label.to_string(),
            provider: preset.provider.to_string(),
            model_id: preset.model_id.to_string(),
            local: preset.local,
            posture_summary: posture_one_liner(posture),
        }
    }
}

/// Condense the resolved privacy posture into a single localized line covering
/// the offline / residency / telemetry pillars.
fn posture_one_liner(posture: &PrivacyPosture) -> String {
    if posture.is_fully_locked_down() {
        label(
            "onboard.posture_locked",
            "Fully locked down: strict residency, offline, redaction on, telemetry off, local provider.",
        )
    } else if posture.offline {
        label(
            "onboard.posture_offline",
            "Offline mode on; no off-machine egress for the active provider. Telemetry off.",
        )
    } else if posture.provider_is_local {
        label(
            "onboard.posture_local",
            "Local-first provider; telemetry off. Tighten further with BHARATCODE_OFFLINE=1.",
        )
    } else {
        label(
            "onboard.posture_open",
            "Default posture; telemetry off. Run 'bharatcode privacy' to review residency and tighten it.",
        )
    }
}

/// Render the full, ordered onboarding plan as a single localized string.
///
/// This is the scripted output for the `--non-interactive` path and the exact
/// text a test can assert against. It is **pure** with respect to its
/// arguments (only the active locale, read via `tr!`, influences the strings),
/// so it never performs I/O and never references any upstream project name.
///
/// The plan always contains the three section headers (locale / preset /
/// privacy) plus a next-steps section whose first command is a ready-to-copy
/// `bharatcode run` line.
pub fn render_plan(choices: &OnboardChoices) -> String {
    let mut out = String::new();

    out.push_str(&label("onboard.title", "BharatCode first-run setup"));
    out.push('\n');
    out.push_str(&label(
        "onboard.subtitle",
        "A quick, idempotent walkthrough — nothing is saved unless you confirm it.",
    ));
    out.push_str("\n\n");

    // 1. Locale section.
    out.push_str(&label("onboard.section_locale", "1. Language / locale"));
    out.push('\n');
    out.push_str(&format!(
        "   {} {} ({})\n",
        label("onboard.locale_chosen", "Selected:"),
        choices.locale.display(),
        choices.locale.code(),
    ));
    out.push_str(&format!(
        "   {} {}\n",
        label("onboard.locale_options", "Available:"),
        WizardLocale::all()
            .iter()
            .map(|l| l.code())
            .collect::<Vec<_>>()
            .join(" / "),
    ));
    out.push('\n');

    // 2. Preset section.
    let key_hint = if choices.local {
        label("onboard.preset_no_key", "no API key required")
    } else {
        label(
            "onboard.preset_needs_key",
            "set the provider's API key before first use",
        )
    };
    let kind = if choices.local {
        label("onboard.preset_local", "local-first")
    } else {
        label("onboard.preset_hosted", "India-hosted")
    };
    out.push_str(&label(
        "onboard.section_preset",
        "2. Provider / model preset",
    ));
    out.push('\n');
    out.push_str(&format!(
        "   {} {} [{}]\n",
        label("onboard.preset_chosen", "Selected:"),
        choices.preset_label,
        kind,
    ));
    out.push_str(&format!(
        "   {} {} / {} ({})\n",
        label("onboard.preset_provider", "Provider / model:"),
        choices.provider,
        choices.model_id,
        key_hint,
    ));
    out.push('\n');

    // 3. Privacy section.
    out.push_str(&label(
        "onboard.section_privacy",
        "3. Privacy posture (offline / residency / telemetry)",
    ));
    out.push('\n');
    out.push_str(&format!(
        "   {} {}\n",
        style("·").dim(),
        choices.posture_summary
    ));
    out.push_str(&format!(
        "   {} {}\n",
        label("onboard.privacy_hint", "Full report:"),
        "bharatcode privacy",
    ));
    out.push('\n');

    // 4. Next steps — the first command is a copy-pasteable `bharatcode run`.
    out.push_str(&label("onboard.section_next", "4. Next steps"));
    out.push('\n');
    for line in next_step_lines(choices) {
        out.push_str(&format!("   {} {}\n", style("›").color256(208), line));
    }

    out
}

/// The localized next-step command lines. The first entry is always a
/// ready-to-run `bharatcode run` invocation so a new user can act immediately.
pub fn next_step_lines(choices: &OnboardChoices) -> Vec<String> {
    vec![
        format!(
            "{} bharatcode run -t \"{}\"",
            label("onboard.next_run", "Try a one-shot task:"),
            label("onboard.next_run_prompt", "summarize this repository"),
        ),
        format!(
            "{} bharatcode session",
            label("onboard.next_chat", "Start an interactive chat:"),
        ),
        format!(
            "{} bharatcode configure",
            label("onboard.next_configure", "Fine-tune providers / keys:"),
        ),
        format!(
            "{} bharatcode privacy",
            label("onboard.next_privacy", "Review the privacy posture:"),
        ),
        format!(
            "{} ({} / {})",
            label("onboard.next_active", "Active preset:"),
            choices.provider,
            choices.model_id,
        ),
    ]
}

/// True when stdin is not a TTY, i.e. the wizard must not block on prompts.
pub fn is_noninteractive() -> bool {
    !std::io::stdin().is_terminal()
}

/// Resolve the locale currently in effect (env / config / `LANG`) so the wizard
/// can pre-select a sensible default without re-reading private i18n internals.
fn current_locale() -> WizardLocale {
    if let Ok(raw) = std::env::var("BHARATCODE_LANG") {
        if !raw.trim().is_empty() {
            return WizardLocale::from_code(&raw);
        }
    }
    if let Ok(raw) = bharatcode_core::config::Config::global().get_param::<String>(LANG_CONFIG_KEY)
    {
        if !raw.trim().is_empty() {
            return WizardLocale::from_code(&raw);
        }
    }
    if let Ok(raw) = std::env::var("LANG") {
        if !raw.trim().is_empty() {
            return WizardLocale::from_code(&raw);
        }
    }
    WizardLocale::English
}

/// Entry point for `bharatcode onboard`.
///
/// Interactive on a TTY (and unless `--non-interactive` is passed): offers the
/// locale picker, the preset picker, shows the resolved privacy posture, and
/// persists the locale + provider/model only if the user confirms saving them.
/// Non-interactive: renders the scripted plan from the current defaults and
/// returns without blocking or persisting anything.
pub async fn handle_onboard(opts: OnboardOptions) -> anyhow::Result<()> {
    let presets = india_presets();
    let posture = PrivacyPosture::resolve();
    let default_locale = current_locale();

    if opts.non_interactive || is_noninteractive() {
        // Non-blocking path: render the plan from current defaults (first
        // preset, resolved locale and live posture). Nothing is persisted.
        let default_preset = &presets[0];
        let choices = OnboardChoices::from_preset(default_locale, default_preset, &posture);
        if is_noninteractive() && !opts.non_interactive {
            println!(
                "{}",
                crate::theme::muted(label(
                    "onboard.noninteractive",
                    "Non-interactive terminal detected — showing the scripted plan without prompting.",
                ))
            );
        }
        print!("{}", render_plan(&choices));
        return Ok(());
    }

    // Interactive path.
    let locale = prompt_locale(default_locale)?;
    let preset = prompt_preset(&presets)?;
    let choices = OnboardChoices::from_preset(locale, preset, &posture);

    println!();
    println!(
        "{}",
        crate::theme::heading(label(
            "onboard.section_privacy",
            "3. Privacy posture (offline / residency / telemetry)",
        ))
    );
    println!("  {} {}", style("·").dim(), choices.posture_summary);

    let save = cliclack::confirm(label(
        "onboard.confirm_save",
        "Save this language and provider/model as your defaults?",
    ))
    .initial_value(true)
    .interact()
    .unwrap_or(false);

    if save {
        persist_choices(&choices);
    }

    println!();
    println!(
        "{}",
        crate::theme::heading(label("onboard.summary_title", "You're all set"))
    );
    print!("{}", render_plan(&choices));
    Ok(())
}

/// Persist the durable choices (interface locale + active provider/model) to the
/// shared config, reporting success/failure per write without aborting.
fn persist_choices(choices: &OnboardChoices) {
    let config = bharatcode_core::config::Config::global();
    let writes = [
        (LANG_CONFIG_KEY, choices.locale.code().to_string()),
        (PROVIDER_CONFIG_KEY, choices.provider.clone()),
        (MODEL_CONFIG_KEY, choices.model_id.clone()),
    ];
    for (key, value) in writes {
        match config.set_param(key, serde_json::Value::String(value)) {
            Ok(()) => println!(
                "  {} {} {}",
                crate::theme::success("✓".to_string()),
                label("onboard.saved", "Saved"),
                key,
            ),
            Err(e) => println!(
                "  {} {} {}: {}",
                crate::theme::warning("✗".to_string()),
                label("onboard.save_failed", "Could not save"),
                key,
                e,
            ),
        }
    }
}

/// Prompt for the interface locale, defaulting to the resolved current one.
fn prompt_locale(default: WizardLocale) -> anyhow::Result<WizardLocale> {
    println!();
    let mut select = cliclack::select(label("onboard.pick_locale", "Choose your language"));
    for locale in WizardLocale::all() {
        select = select.item(locale.code(), locale.display(), "");
    }
    let code = select
        .initial_value(default.code())
        .interact()
        .unwrap_or(default.code());
    Ok(WizardLocale::from_code(code))
}

/// Prompt for a provider/model preset from the curated India list.
fn prompt_preset(presets: &[Preset]) -> anyhow::Result<&Preset> {
    println!();
    let mut select = cliclack::select(label(
        "onboard.pick_preset",
        "Choose a local-first or India-hosted preset",
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

    fn sample_choices(local: bool) -> OnboardChoices {
        OnboardChoices {
            locale: WizardLocale::Hindi,
            preset_label: "Qwen2.5 Coder (local)".to_string(),
            provider: "ollama".to_string(),
            model_id: "qwen2.5-coder".to_string(),
            local,
            posture_summary: "Local-first provider; telemetry off.".to_string(),
        }
    }

    /// Strip ANSI styling so assertions match the underlying text regardless of
    /// whether `console` decides to colorize in the test environment.
    fn plain(s: &str) -> String {
        console::strip_ansi_codes(s).to_string()
    }

    #[test]
    fn render_plan_has_all_section_headers_and_a_run_next_step() {
        let plan = plain(&render_plan(&sample_choices(true)));

        // The three confirmation sections plus next steps.
        assert!(
            plan.contains("Language / locale"),
            "missing locale header: {plan}"
        );
        assert!(
            plan.contains("Provider / model preset"),
            "missing preset header: {plan}"
        );
        assert!(
            plan.contains("Privacy posture (offline / residency / telemetry)"),
            "missing privacy header: {plan}"
        );
        assert!(
            plan.contains("Next steps"),
            "missing next-steps header: {plan}"
        );

        // A copy-pasteable `bharatcode run` next step must be present.
        assert!(
            plan.contains("bharatcode run"),
            "missing 'bharatcode run' next step: {plan}"
        );
    }

    #[test]
    fn render_plan_reflects_the_chosen_preset() {
        let plan = plain(&render_plan(&sample_choices(true)));
        assert!(plan.contains("Qwen2.5 Coder (local)"));
        assert!(plan.contains("ollama"));
        assert!(plan.contains("qwen2.5-coder"));
    }

    #[test]
    fn local_and_hosted_presets_render_different_key_hints() {
        let local = plain(&render_plan(&sample_choices(true)));
        let hosted = plain(&render_plan(&sample_choices(false)));
        assert!(local.contains("no API key"));
        assert!(hosted.contains("API key"));
        assert_ne!(local, hosted);
    }

    #[test]
    fn render_plan_has_no_upstream_branding() {
        let blob = plain(&render_plan(&sample_choices(true)).to_lowercase());
        assert!(!blob.contains("goose"), "plan leaked upstream name: {blob}");
        assert!(!blob.contains("block"), "plan leaked upstream name: {blob}");
    }

    #[test]
    fn next_step_lines_lead_with_a_run_command() {
        let lines = next_step_lines(&sample_choices(true));
        assert!(!lines.is_empty());
        assert!(
            lines[0].contains("bharatcode run"),
            "first next step is not a run command: {:?}",
            lines[0]
        );
        for line in &lines {
            assert!(!line.trim().is_empty());
        }
    }

    #[test]
    fn locale_code_round_trips() {
        for locale in WizardLocale::all() {
            assert_eq!(WizardLocale::from_code(locale.code()), locale);
        }
        assert_eq!(WizardLocale::from_code("hi_IN.UTF-8"), WizardLocale::Hindi);
        assert_eq!(WizardLocale::from_code("ta-IN"), WizardLocale::Tamil);
        assert_eq!(WizardLocale::from_code("fr"), WizardLocale::English);
        assert_eq!(WizardLocale::from_code(""), WizardLocale::English);
    }

    #[tokio::test]
    async fn handle_onboard_non_interactive_returns_ok() {
        // The non-interactive path must complete without blocking and return Ok,
        // making `bharatcode onboard --non-interactive` safe for CI / scripts.
        let result = handle_onboard(OnboardOptions {
            non_interactive: true,
        })
        .await;
        assert!(result.is_ok(), "non-interactive onboard failed: {result:?}");
    }
}
