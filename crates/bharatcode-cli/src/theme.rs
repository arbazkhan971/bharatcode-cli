//! Selectable CLI color themes for BharatCode.
//!
//! A theme is a small set of [`console::Style`] values keyed by semantic role
//! (heading, accent, success, error, warning, muted, neutral). Callers paint
//! output through the role helpers (e.g. [`error`], [`success`]) so the palette
//! can be swapped wholesale.
//!
//! Theme resolution order (cached once per process):
//!   1. `NO_COLOR` (https://no-color.org): any non-empty value forces the
//!      plain, no-color theme regardless of other settings.
//!   2. `BHARATCODE_THEME` environment variable, case-insensitive:
//!        - `none` / `off` / `mono` / `plain` / `nocolor` / `no-color` -> plain
//!        - `tiranga` / `bharat` / `india` -> the Tiranga (saffron/white/green) palette
//!        - `default` / `stock` -> the stable default palette
//!        - anything else -> the Tiranga (saffron/white/green) brand palette
//!   3. Unset -> the Tiranga (saffron/white/green) brand palette (the default look).
//!
//! The default brand palette is Tiranga (saffron/white/green): once a call
//! site routes through one of the helpers below, the saffron brand accent shows
//! by default. Setting `BHARATCODE_THEME=default` (or `stock`) restores the
//! stable cyan palette, and `none` / `NO_COLOR` force plain, unstyled output.

use console::{Style, StyledObject};
use std::sync::OnceLock;

/// 256-color palette indices approximating the Indian national flag (Tiranga).
const SAFFRON: u8 = 208; // ~#ff8700
const FLAG_WHITE: u8 = 231; // pure white
const INDIA_GREEN: u8 = 28; // ~#008700

/// A named collection of styles keyed by semantic role.
#[derive(Debug)]
pub struct Theme {
    /// Machine-readable theme name (e.g. `"tiranga"`).
    pub name: &'static str,
    /// Section headings and primary emphasis.
    pub heading: Style,
    /// Secondary emphasis / informational highlights.
    pub accent: Style,
    /// Positive / success messages.
    pub success: Style,
    /// Error messages.
    pub error: Style,
    /// Warnings and cautions.
    pub warning: Style,
    /// De-emphasized / supplementary text.
    pub muted: Style,
    /// Neutral body text (no decoration by default).
    pub neutral: Style,
}

/// A fully un-styled style that forces ANSI codes off, for the no-color theme.
const PLAIN: Style = Style::new().force_styling(false);

/// The stable (legacy) palette, selectable via `BHARATCODE_THEME=default`.
/// Mirrors the original cyan-accent look; the brand default is now [`TIRANGA`].
pub static DEFAULT: Theme = Theme {
    name: "default",
    heading: Style::new().cyan().bold(),
    accent: Style::new().cyan(),
    success: Style::new().green(),
    error: Style::new().red().bold(),
    warning: Style::new().yellow(),
    muted: Style::new().dim(),
    neutral: Style::new(),
};

/// The Tiranga palette: saffron headings, white neutral, green success.
pub static TIRANGA: Theme = Theme {
    name: "tiranga",
    heading: Style::new().color256(SAFFRON).bold(),
    accent: Style::new().color256(SAFFRON),
    success: Style::new().color256(INDIA_GREEN).bold(),
    error: Style::new().red().bold(),
    warning: Style::new().yellow(),
    muted: Style::new().dim(),
    neutral: Style::new().color256(FLAG_WHITE),
};

/// The no-color palette: every role emits plain, unstyled text.
pub static NONE: Theme = Theme {
    name: "none",
    heading: PLAIN,
    accent: PLAIN,
    success: PLAIN,
    error: PLAIN,
    warning: PLAIN,
    muted: PLAIN,
    neutral: PLAIN,
};

static ACTIVE: OnceLock<&'static Theme> = OnceLock::new();

/// Names of the selectable themes, for help text and discovery.
pub fn theme_names() -> &'static [&'static str] {
    &["default", "tiranga", "none"]
}

/// The active theme for this process (resolved once and cached).
pub fn active_theme() -> &'static Theme {
    *ACTIVE.get_or_init(resolve_theme)
}

fn resolve_theme() -> &'static Theme {
    let no_color = std::env::var_os("NO_COLOR")
        .map(|v| !v.is_empty())
        .unwrap_or(false);

    let requested = std::env::var("BHARATCODE_THEME")
        .ok()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty());

    pick(requested.as_deref(), no_color)
}

/// Pure theme selection, separated from environment reads for testability.
fn pick(requested: Option<&str>, no_color: bool) -> &'static Theme {
    match requested {
        Some("none") | Some("off") | Some("mono") | Some("plain") | Some("nocolor")
        | Some("no-color") => &NONE,
        _ if no_color => &NONE,
        Some("tiranga") | Some("bharat") | Some("india") => &TIRANGA,
        Some("default") | Some("stock") => &DEFAULT,
        Some(_) | None => &TIRANGA,
    }
}

/// Paint `val` with the active theme's heading style.
pub fn heading<D>(val: D) -> StyledObject<D> {
    active_theme().heading.apply_to(val)
}

/// Paint `val` with the active theme's accent style.
pub fn accent<D>(val: D) -> StyledObject<D> {
    active_theme().accent.apply_to(val)
}

/// Paint `val` with the active theme's success style.
pub fn success<D>(val: D) -> StyledObject<D> {
    active_theme().success.apply_to(val)
}

/// Paint `val` with the active theme's error style.
pub fn error<D>(val: D) -> StyledObject<D> {
    active_theme().error.apply_to(val)
}

/// Paint `val` with the active theme's warning style.
pub fn warning<D>(val: D) -> StyledObject<D> {
    active_theme().warning.apply_to(val)
}

/// Paint `val` with the active theme's muted style.
pub fn muted<D>(val: D) -> StyledObject<D> {
    active_theme().muted.apply_to(val)
}

/// Paint `val` with the active theme's neutral style.
pub fn neutral<D>(val: D) -> StyledObject<D> {
    active_theme().neutral.apply_to(val)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_resolves_to_tiranga() {
        assert_eq!(pick(None, false).name, "tiranga");
    }

    #[test]
    fn explicit_default_is_still_selectable() {
        assert_eq!(pick(Some("default"), false).name, "default");
        assert_eq!(pick(Some("stock"), false).name, "default");
    }

    #[test]
    fn tiranga_is_selectable() {
        assert_eq!(pick(Some("tiranga"), false).name, "tiranga");
        assert_eq!(pick(Some("bharat"), false).name, "tiranga");
        assert_eq!(pick(Some("india"), false).name, "tiranga");
    }

    #[test]
    fn explicit_none_selects_plain() {
        for name in ["none", "off", "mono", "plain", "nocolor", "no-color"] {
            assert_eq!(pick(Some(name), false).name, "none", "{name}");
        }
    }

    #[test]
    fn no_color_env_forces_plain_even_for_tiranga() {
        assert_eq!(pick(Some("tiranga"), true).name, "none");
        assert_eq!(pick(None, true).name, "none");
        assert_eq!(pick(Some("default"), true).name, "none");
    }

    #[test]
    fn unknown_theme_falls_back_to_tiranga() {
        assert_eq!(pick(Some("rainbow"), false).name, "tiranga");
    }

    #[test]
    fn no_color_is_disabled_when_unset_or_empty() {
        // The helper that reads NO_COLOR treats only a non-empty value as set;
        // here we assert the selection path used when it is not active.
        assert_eq!(pick(Some("tiranga"), false).name, "tiranga");
    }
}
