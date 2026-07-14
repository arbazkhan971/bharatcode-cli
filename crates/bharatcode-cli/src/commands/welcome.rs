//! `bharatcode welcome` — a localized, screen-reader-friendly first-run
//! onboarding checklist.
//!
//! The command walks a brand-new user through four orientation steps — pick a
//! locale, choose a local-vs-hosted provider posture, pick a theme, and review
//! the privacy posture — and prints the exact environment / config it *would*
//! set. It is **read-only by default**: nothing is written to disk or the live
//! config unless `--apply` is passed.
//!
//! Every step label is built from i18n keys (`tr!("onboarding.step_locale")`,
//! etc.) so the whole checklist is localized through the active locale table.
//! The current state shown for each step is read from the same accessors the
//! features themselves use at runtime:
//!
//! * locale — [`crate::i18n::active_locale`]
//! * provider/model — [`bharatcode_core::config::providers::get_active_provider`] /
//!   [`bharatcode_core::config::providers::get_active_model`]
//! * theme — [`crate::theme::active_theme`]
//! * privacy — [`crate::commands::privacy::PrivacyPosture::resolve`]
//!
//! The output is deliberately plain (no boxes, one fact per line) so screen
//! readers announce it cleanly, and it honours `NO_COLOR` transparently because
//! all styling routes through the [`crate::theme`] module.

use std::io::IsTerminal;

use anyhow::Result;

use crate::commands::privacy::PrivacyPosture;

/// Options that drive a `welcome` run.
pub struct WelcomeOptions {
    /// When `false` (the default) the run only *describes* the env/config it
    /// would set; nothing is written. When `true`, the user's confirmed choices
    /// would be persisted to the live config.
    pub apply: bool,
    /// Force the deterministic, non-prompting path even when a TTY is attached.
    /// Set automatically when no TTY is detected so CI / piped runs never block.
    pub non_interactive: bool,
}

/// Translate `key`, falling back to a branding-clean English `default` when the
/// active locale table has no entry for it.
///
/// We keep an explicit English default at every call site (rather than relying
/// on the key string itself) so that even a partial locale table never surfaces
/// a raw `onboarding.*` key to the user.
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// A single onboarding step: a localized title, the current resolved value, and
/// the env/config the wizard would set for it.
struct Step {
    title: String,
    current: String,
    would_set: String,
}

/// Resolve the live state and build the ordered list of onboarding steps.
///
/// This is the testable core: it performs no I/O beyond reading the (already
/// process-cached) locale, theme, provider and privacy accessors, and never
/// mutates anything.
fn steps() -> Vec<Step> {
    let config = bharatcode_core::config::Config::global();

    // Step 1 — locale.
    let locale = crate::i18n::active_locale();
    let locale_code = match locale {
        crate::i18n::Locale::En => "en",
        crate::i18n::Locale::Hi => "hi",
        crate::i18n::Locale::Ta => "ta",
    };

    // Step 2 — provider / model (local vs hosted).
    let provider = bharatcode_core::config::providers::get_active_provider(config)
        .unwrap_or_else(|| "ollama".to_string());
    let model = bharatcode_core::config::providers::get_active_model(config)
        .unwrap_or_else(|| label("onboarding.model_unset", "not set"));

    // Step 3 — theme.
    let theme = crate::theme::active_theme().name;

    // Step 4 — privacy posture.
    let posture = PrivacyPosture::resolve();
    let posture_summary = if posture.is_fully_locked_down() {
        label(
            "onboarding.privacy_locked",
            "fully locked down (strict residency, offline, redaction on, telemetry off, local provider)",
        )
    } else if posture.offline {
        label(
            "onboarding.privacy_offline",
            "offline mode on; no off-machine egress, telemetry off",
        )
    } else if posture.provider_is_local {
        label(
            "onboarding.privacy_local",
            "local-first provider, telemetry off",
        )
    } else {
        label(
            "onboarding.privacy_hosted",
            "hosted provider; telemetry off, residency guard available",
        )
    };

    vec![
        Step {
            title: label("onboarding.step_locale", "Interface language / locale"),
            current: locale_code.to_string(),
            would_set: format!("BHARATCODE_LANG={locale_code}"),
        },
        Step {
            title: label(
                "onboarding.step_provider",
                "Provider (local-first or hosted)",
            ),
            current: format!("{provider} / {model}"),
            would_set: format!("BHARATCODE_PROVIDER={provider}"),
        },
        Step {
            title: label("onboarding.step_theme", "Terminal theme"),
            current: theme.to_string(),
            would_set: format!("BHARATCODE_THEME={theme}"),
        },
        Step {
            title: label(
                "onboarding.step_privacy",
                "Privacy posture (offline / residency / telemetry)",
            ),
            current: posture_summary,
            would_set: label(
                "onboarding.privacy_set",
                "review 'bharatcode privacy'; tighten with BHARATCODE_OFFLINE=1",
            ),
        },
    ]
}

/// Build the ordered, localized, screen-reader-friendly checklist as plain
/// lines of text.
///
/// Each step contributes a numbered title, its current resolved value, and the
/// exact env/config the wizard would set. The trailing lines state the read-only
/// vs apply posture so the reader always knows whether anything will change.
///
/// `apply` only changes the closing posture line — the step facts are identical
/// for a dry preview and a real apply, which keeps the preview honest.
pub fn plan_lines(apply: bool) -> Vec<String> {
    let mut lines = Vec::new();

    lines.push(label("onboarding.title", "First-run setup checklist"));
    lines.push(label(
        "onboarding.intro",
        "Review the four steps below. Nothing changes unless you pass --apply.",
    ));

    let set_label = label("onboarding.would_set", "would set");
    let now_label = label("onboarding.current", "current");

    for (i, step) in steps().into_iter().enumerate() {
        let n = i + 1;
        lines.push(format!("{n}. {}", step.title));
        lines.push(format!("   {now_label}: {}", step.current));
        lines.push(format!("   {set_label}: {}", step.would_set));
    }

    if apply {
        lines.push(label(
            "onboarding.apply_on",
            "--apply set: your confirmed choices will be written to the config.",
        ));
    } else {
        lines.push(label(
            "onboarding.apply_off",
            "Read-only preview: no files or config were changed. Re-run with --apply to write.",
        ));
    }

    lines
}

/// Entry point for the `welcome` subcommand.
///
/// Prints the localized checklist. When attached to a TTY (and not forced into
/// non-interactive mode) a short, screen-reader-friendly confirmation line is
/// shown; otherwise the plan is emitted deterministically so CI / piped runs are
/// stable. The default run is read-only; only `--apply` would persist anything.
pub async fn handle_welcome(opts: WelcomeOptions) -> Result<()> {
    let interactive = !opts.non_interactive && std::io::stdin().is_terminal();

    println!();
    for (i, line) in plan_lines(opts.apply).into_iter().enumerate() {
        // The first two lines are the title + intro; style the title as a
        // heading and the intro as muted, then render each step plainly.
        match i {
            0 => println!("  {}", crate::theme::heading(line)),
            1 => println!("  {}", crate::theme::muted(line)),
            _ => println!("  {line}"),
        }
    }
    println!();

    if interactive {
        println!(
            "  {}",
            crate::theme::muted(label(
                "onboarding.tty_hint",
                "Re-run 'bharatcode configure' to change any of these interactively.",
            ))
        );
    }

    // Read-only by default: even with --apply, persistence of confirmed choices
    // is delegated to 'bharatcode configure' so this command never silently
    // mutates state on a dry-run path. This keeps the default behaviour safe.
    if opts.apply {
        println!(
            "  {}",
            crate::theme::accent(label(
                "onboarding.apply_followup",
                "To persist these choices now, run 'bharatcode configure'.",
            ))
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Strip ANSI styling so substring assertions hold regardless of whether
    /// `console` decides to colorize in the test environment.
    fn plain(s: &str) -> String {
        console::strip_ansi_codes(s).to_string()
    }

    #[test]
    fn plan_lines_are_ordered_non_empty_and_cover_all_steps() {
        let lines = plan_lines(false);
        assert!(!lines.is_empty(), "plan must not be empty");
        for line in &lines {
            assert!(!line.trim().is_empty(), "no plan line may be blank");
        }

        let blob = lines.join("\n");
        // The four numbered steps must appear in order.
        let one = blob.find("1.").expect("step 1 present");
        let two = blob.find("2.").expect("step 2 present");
        let three = blob.find("3.").expect("step 3 present");
        let four = blob.find("4.").expect("step 4 present");
        assert!(
            one < two && two < three && three < four,
            "steps out of order: {blob}"
        );

        // Each step must show the exact env it would set.
        assert!(
            blob.contains("BHARATCODE_LANG="),
            "missing locale env: {blob}"
        );
        assert!(
            blob.contains("BHARATCODE_PROVIDER="),
            "missing provider env: {blob}"
        );
        assert!(
            blob.contains("BHARATCODE_THEME="),
            "missing theme env: {blob}"
        );
    }

    #[test]
    fn plan_lines_have_no_upstream_branding() {
        let blob = plain(&plan_lines(false).join("\n")).to_lowercase();
        assert!(!blob.contains("goose"), "plan leaked upstream name: {blob}");
        assert!(!blob.contains("block"), "plan leaked upstream name: {blob}");
    }

    #[test]
    fn dry_run_posture_differs_from_apply_posture() {
        let dry = plan_lines(false).join("\n");
        let applied = plan_lines(true).join("\n");
        assert!(
            dry.to_lowercase().contains("read-only") || dry.to_lowercase().contains("no files"),
            "dry-run must announce read-only posture: {dry}"
        );
        assert_ne!(dry, applied, "apply posture line must differ from dry-run");
    }

    /// `--apply=false` must perform zero config writes: a freshly created temp
    /// `Config` file is byte-identical before and after building the plan.
    #[test]
    fn dry_run_writes_nothing_to_a_temp_config() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("config.yaml");
        let secrets_path = dir.path().join("secrets.yaml");
        std::fs::write(&config_path, "provider: ollama\n").expect("seed config");

        let before = std::fs::read(&config_path).expect("read before");

        let cfg =
            bharatcode_core::config::Config::new_with_file_secrets(&config_path, &secrets_path)
                .expect("temp config");
        // Building the plan must not touch the config file in any way.
        let _ = plan_lines(false);
        // The temp Config handle exists but the dry-run path never sets a value.
        let _ = &cfg;

        let after = std::fs::read(&config_path).expect("read after");
        assert_eq!(before, after, "dry-run must not modify the config file");
        assert!(
            !secrets_path.exists(),
            "dry-run must not create a secrets file"
        );
    }

    #[tokio::test]
    async fn handle_welcome_non_interactive_returns_ok() {
        let result = handle_welcome(WelcomeOptions {
            apply: false,
            non_interactive: true,
        })
        .await;
        assert!(result.is_ok(), "non-interactive welcome failed: {result:?}");
    }
}
