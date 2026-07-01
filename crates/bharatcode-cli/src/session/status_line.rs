//! Locale- and accessibility-aware status-line / footer formatter for
//! interactive sessions.
//!
//! This renders a single, aligned footer line summarizing the active model,
//! provider, context-usage percentage and (optionally) the amount spent in
//! rupees. It is intentionally self-contained so the live session loop can call
//! it once per turn boundary to append one muted status line without disturbing
//! the existing banner.
//!
//! Behavioural guarantees, all derived purely from the environment (no opt-in
//! flag needed):
//!   * `NO_COLOR` (any value) => zero ANSI escape bytes in the output.
//!   * Field captions route through [`crate::tr`] so Hindi/Tamil translations
//!     can override them, while an English default is always supplied as a
//!     fallback (so output is unchanged until a locale table provides one).
//!   * The whole line is truncated to a width budget with an ellipsis and never
//!     exceeds it, keeping narrow terminals tidy.
//!
//! Field captions are looked up via [`tr_or`], which consults the active locale
//! table (honoring `BHARATCODE_LANG`) and falls back to the supplied English
//! default when no translation exists.

use console::style;

use super::terminal_width;

/// Inputs needed to render a single status line.
///
/// All borrowed so callers can build it cheaply from existing session state
/// without cloning model/provider strings.
pub struct StatusCtx<'a> {
    /// Active model name, e.g. `"gpt-4o"`.
    pub model: &'a str,
    /// Active provider name, e.g. `"tetrate"`.
    pub provider: &'a str,
    /// Percent of the context window used (0..=100).
    pub context_pct: u8,
    /// Amount spent so far, in rupees. `None` hides the spend field.
    pub rupees_spent: Option<f64>,
    /// Maximum display width (columns) the rendered line may occupy.
    pub width_budget: usize,
}

impl Default for StatusCtx<'_> {
    fn default() -> Self {
        StatusCtx {
            model: "",
            provider: "",
            context_pct: 0,
            rupees_spent: None,
            width_budget: 80,
        }
    }
}

/// `true` when ANSI styling must be suppressed because `NO_COLOR` is set.
///
/// Follows the de-facto standard: the variable disables color when present with
/// any value (https://no-color.org/).
fn no_color() -> bool {
    std::env::var_os("NO_COLOR").is_some()
}

/// Look up a field caption in the active locale table, falling back to the
/// supplied English default when no localized string exists.
///
/// [`crate::i18n::t`] returns the lookup key verbatim when a key is missing, so
/// we pass `default` itself as the key and treat an identity result as "no
/// translation", yielding the default. This keeps captions Hindi/Tamil-ready
/// (a locale table can map the English caption to its own string) while leaving
/// default English output untouched.
fn tr_or(default: &str) -> String {
    let translated = crate::tr!(default);
    if translated == default {
        default.to_string()
    } else {
        translated
    }
}

/// Render the status line for `ctx`.
///
/// The result is a single line (no trailing newline) of the form:
///   `model: <m>  •  provider: <p>  •  context: NN%  •  ₹<spend>`
///
/// styled with muted (dim) ANSI unless `NO_COLOR` is active, and truncated to
/// `ctx.width_budget` columns with an ellipsis if necessary.
pub fn format_status(ctx: StatusCtx) -> String {
    let pct = ctx.context_pct.min(100);

    let model_label = tr_or("model");
    let provider_label = tr_or("provider");
    let context_label = tr_or("context");

    let mut fields: Vec<String> = vec![
        format!("{}: {}", model_label, ctx.model),
        format!("{}: {}", provider_label, ctx.provider),
        format!("{}: {}%", context_label, pct),
    ];

    if let Some(rupees) = ctx.rupees_spent {
        // Always emit the ₹ symbol so spend is unambiguous across locales.
        fields.push(format!("\u{20B9}{:.2}", rupees.max(0.0)));
    }

    // Plain (un-styled) joined line; this is what we measure and truncate so the
    // width budget is honored regardless of ANSI bytes.
    let separator = "  \u{2022}  ";
    let plain = fields.join(separator);
    let plain = truncate_to_width(&plain, ctx.width_budget);

    if no_color() {
        plain
    } else {
        // Mute the whole line; the styled string measures the same visible width
        // because the truncation above operated on the plain text.
        style(plain).dim().to_string()
    }
}

/// Truncate `text` to at most `max_width` display columns, appending an ellipsis
/// when truncation occurs. The returned string's display width never exceeds
/// `max_width`.
fn truncate_to_width(text: &str, max_width: usize) -> String {
    terminal_width::truncate_to_width(text, max_width, "\u{2026}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // The tests below toggle the process-global `NO_COLOR`, so they must not run
    // concurrently with each other. Serialize them through one mutex.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn sample(width_budget: usize) -> StatusCtx<'static> {
        StatusCtx {
            model: "gpt-4o",
            provider: "tetrate",
            context_pct: 42,
            rupees_spent: Some(12.5),
            width_budget,
        }
    }

    /// Run `f` with `NO_COLOR` forced to the given state, restoring it after.
    fn with_no_color<T>(enabled: bool, f: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var_os("NO_COLOR");
        if enabled {
            std::env::set_var("NO_COLOR", "1");
        } else {
            std::env::remove_var("NO_COLOR");
        }
        let out = f();
        match prev {
            Some(v) => std::env::set_var("NO_COLOR", v),
            None => std::env::remove_var("NO_COLOR"),
        }
        out
    }

    /// Guard the no-leak invariant: no user-facing upstream brand names.
    fn assert_no_brand_leak(s: &str) {
        let lower = s.to_lowercase();
        assert!(
            !lower.contains("goose"),
            "status line leaked an upstream ident: {s}"
        );
        assert!(
            !lower.contains("block"),
            "status line leaked an upstream ident: {s}"
        );
    }

    #[test]
    fn contains_model_provider_percent_and_rupee() {
        // NO_COLOR-on keeps the output plain so the substring checks are robust.
        let out = with_no_color(true, || format_status(sample(120)));
        assert!(out.contains("gpt-4o"), "missing model: {out}");
        assert!(out.contains("tetrate"), "missing provider: {out}");
        assert!(out.contains("42%"), "missing context percent: {out}");
        assert!(out.contains('\u{20B9}'), "missing rupee symbol: {out}");
        assert_no_brand_leak(&out);
    }

    #[test]
    fn no_color_has_zero_ansi_escape_bytes() {
        let out = with_no_color(true, || format_status(sample(120)));
        // ANSI escapes start with the ESC byte (0x1B); there must be none.
        assert!(
            !out.as_bytes().contains(&0x1b),
            "NO_COLOR output contained an ANSI escape: {out:?}"
        );
        assert!(out.contains("gpt-4o") && out.contains("tetrate") && out.contains("42%"));
        assert_no_brand_leak(&out);
    }

    #[test]
    fn over_budget_truncates_with_ellipsis_within_budget() {
        let budget = 20;
        let out = with_no_color(true, || format_status(sample(budget)));
        assert!(
            terminal_width::display_width(&out) <= budget,
            "truncated line exceeded budget ({} > {budget}): {out:?}",
            terminal_width::display_width(&out)
        );
        assert!(out.contains('\u{2026}'), "expected an ellipsis: {out:?}");
        assert_no_brand_leak(&out);
    }

    #[test]
    fn rupees_omitted_when_absent() {
        let out = with_no_color(true, || {
            format_status(StatusCtx {
                rupees_spent: None,
                ..sample(120)
            })
        });
        assert!(
            !out.contains('\u{20B9}'),
            "rupee shown when spend absent: {out}"
        );
    }

    #[test]
    fn percent_is_clamped_to_100() {
        let out = with_no_color(true, || {
            format_status(StatusCtx {
                context_pct: 250,
                ..sample(120)
            })
        });
        assert!(out.contains("100%"), "percent not clamped: {out}");
    }

    #[test]
    fn truncate_never_exceeds_tiny_budgets() {
        for budget in 0..6usize {
            let out = truncate_to_width("a very long status line indeed", budget);
            assert!(
                terminal_width::display_width(&out) <= budget,
                "budget {budget} exceeded by {out:?}"
            );
        }
    }
}
