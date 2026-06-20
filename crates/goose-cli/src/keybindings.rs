//! Customizable interactive keybindings.
//!
//! A small, self-contained description of the few interactive editor bindings
//! that users are allowed to override (submit, cancel, history navigation and
//! the multi-line newline key). The defaults reproduce the current built-in
//! behavior exactly, so this is purely additive: a config with no keybindings
//! set behaves identically to before.
//!
//! Overrides are read from config (env var `BHARATCODE_KEYS` or the matching
//! `BHARATCODE_KEYS` config-file key) as a list of `action=key` pairs,
//! separated by `;` or `,`. Example:
//!
//! ```text
//! BHARATCODE_KEYS="cancel=ctrl-g;history_prev=ctrl-p;history_next=ctrl-n"
//! ```
//!
//! Recognized actions: `submit`, `cancel`, `history_prev`, `history_next`,
//! `newline`. Recognized keys include `enter`, `tab`, `esc`, `up`, `down`,
//! `left`, `right`, `home`, `end`, `pageup`, `pagedown`, `backspace`,
//! `delete`, a single character, or a modified key such as `ctrl-c` /
//! `alt-b`.

use goose::config::Config;
use rustyline::{KeyCode, KeyEvent, Modifiers};

/// The interactive bindings that can be customized via config.
///
/// Each field stores the raw, lowercased key spec (e.g. `"ctrl-c"`); call the
/// corresponding accessor to turn it into a [`rustyline::KeyEvent`] for the
/// editor setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keybindings {
    /// Key that submits the current input. Default: `enter`.
    pub submit: String,
    /// Key that cancels / clears the current line. Default: `ctrl-c`.
    pub cancel: String,
    /// Key that recalls the previous history entry. Default: `up`.
    pub history_prev: String,
    /// Key that recalls the next history entry. Default: `down`.
    pub history_next: String,
    /// Key that inserts a literal newline in multi-line mode. Default: `ctrl-j`.
    pub newline: String,
}

impl Default for Keybindings {
    fn default() -> Self {
        Self {
            submit: "enter".to_string(),
            cancel: "ctrl-c".to_string(),
            history_prev: "up".to_string(),
            history_next: "down".to_string(),
            newline: "ctrl-j".to_string(),
        }
    }
}

impl Keybindings {
    /// Load keybindings from config, falling back to the defaults (current
    /// behavior) for anything not overridden.
    ///
    /// The legacy `bharatcode_cli_newline_key` single-character setting is still
    /// honored for the newline binding so existing configs keep working; an
    /// explicit `newline=` entry in `bharatcode_keys` takes precedence.
    pub fn from_config(config: &Config) -> Self {
        let mut bindings = Self::default();

        if let Ok(newline_key) = config.get_param::<String>("BHARATCODE_CLI_NEWLINE_KEY") {
            if let Some(c) = newline_key.chars().next() {
                bindings.newline = format!("ctrl-{}", c.to_ascii_lowercase());
            }
        }

        if let Ok(spec) = config.get_param::<String>("BHARATCODE_KEYS") {
            bindings.apply_spec(&spec);
        }

        bindings
    }

    /// Apply an `action=key;action=key` override string onto these bindings.
    ///
    /// Unknown actions and empty entries are ignored so a malformed config can
    /// never break the editor.
    pub fn apply_spec(&mut self, spec: &str) {
        for entry in spec.split([';', ',']) {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            let Some((action, key)) = entry.split_once('=') else {
                continue;
            };
            let key = key.trim().to_lowercase();
            if key.is_empty() {
                continue;
            }
            match action.trim().to_lowercase().as_str() {
                "submit" => self.submit = key,
                "cancel" => self.cancel = key,
                "history_prev" | "history-prev" | "prev" => self.history_prev = key,
                "history_next" | "history-next" | "next" => self.history_next = key,
                "newline" => self.newline = key,
                _ => {}
            }
        }
    }

    /// The submit binding as a [`KeyEvent`], if it parses.
    pub fn submit_key(&self) -> Option<KeyEvent> {
        parse_key(&self.submit)
    }

    /// The cancel binding as a [`KeyEvent`], if it parses.
    pub fn cancel_key(&self) -> Option<KeyEvent> {
        parse_key(&self.cancel)
    }

    /// The previous-history binding as a [`KeyEvent`], if it parses.
    pub fn history_prev_key(&self) -> Option<KeyEvent> {
        parse_key(&self.history_prev)
    }

    /// The next-history binding as a [`KeyEvent`], if it parses.
    pub fn history_next_key(&self) -> Option<KeyEvent> {
        parse_key(&self.history_next)
    }

    /// The newline binding as a [`KeyEvent`], if it parses.
    pub fn newline_key(&self) -> Option<KeyEvent> {
        parse_key(&self.newline)
    }
}

/// Parse a single key spec (e.g. `"enter"`, `"ctrl-c"`, `"alt-b"`, `"x"`) into
/// a [`rustyline::KeyEvent`]. Returns `None` for specs we don't understand.
fn parse_key(spec: &str) -> Option<KeyEvent> {
    let spec = spec.trim().to_lowercase();
    if spec.is_empty() {
        return None;
    }

    let (mods, name) = match spec.split_once('-') {
        Some(("ctrl", rest)) => (Modifiers::CTRL, rest),
        Some(("alt", rest)) => (Modifiers::ALT, rest),
        Some(("shift", rest)) => (Modifiers::SHIFT, rest),
        _ => (Modifiers::NONE, spec.as_str()),
    };

    let code = match name {
        "enter" | "return" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "backtab" => KeyCode::BackTab,
        "esc" | "escape" => KeyCode::Esc,
        "backspace" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "insert" | "ins" => KeyCode::Insert,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        other => {
            let mut chars = other.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            KeyCode::Char(c)
        }
    };

    Some(KeyEvent(code, mods))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_current_behavior() {
        let kb = Keybindings::default();
        assert_eq!(
            kb.submit_key(),
            Some(KeyEvent(KeyCode::Enter, Modifiers::NONE))
        );
        assert_eq!(
            kb.cancel_key(),
            Some(KeyEvent(KeyCode::Char('c'), Modifiers::CTRL))
        );
        assert_eq!(
            kb.history_prev_key(),
            Some(KeyEvent(KeyCode::Up, Modifiers::NONE))
        );
        assert_eq!(
            kb.history_next_key(),
            Some(KeyEvent(KeyCode::Down, Modifiers::NONE))
        );
        assert_eq!(
            kb.newline_key(),
            Some(KeyEvent(KeyCode::Char('j'), Modifiers::CTRL))
        );
    }

    #[test]
    fn apply_spec_overrides_selected_actions() {
        let mut kb = Keybindings::default();
        kb.apply_spec("cancel=ctrl-g; history_prev=ctrl-p , history_next=ctrl-n");
        assert_eq!(
            kb.cancel_key(),
            Some(KeyEvent(KeyCode::Char('g'), Modifiers::CTRL))
        );
        assert_eq!(
            kb.history_prev_key(),
            Some(KeyEvent(KeyCode::Char('p'), Modifiers::CTRL))
        );
        assert_eq!(
            kb.history_next_key(),
            Some(KeyEvent(KeyCode::Char('n'), Modifiers::CTRL))
        );
        // Untouched bindings keep their defaults.
        assert_eq!(
            kb.submit_key(),
            Some(KeyEvent(KeyCode::Enter, Modifiers::NONE))
        );
    }

    #[test]
    fn malformed_entries_are_ignored() {
        let mut kb = Keybindings::default();
        let before = kb.clone();
        kb.apply_spec(";;  ; bogus ; unknown=ctrl-x ; submit= ");
        assert_eq!(kb, before);
    }

    #[test]
    fn parse_key_handles_modifiers_and_named_keys() {
        assert_eq!(
            parse_key("alt-b"),
            Some(KeyEvent(KeyCode::Char('b'), Modifiers::ALT))
        );
        assert_eq!(
            parse_key("esc"),
            Some(KeyEvent(KeyCode::Esc, Modifiers::NONE))
        );
        assert_eq!(
            parse_key("pageup"),
            Some(KeyEvent(KeyCode::PageUp, Modifiers::NONE))
        );
        assert_eq!(parse_key(""), None);
        assert_eq!(parse_key("ctrl-ab"), None);
    }
}
