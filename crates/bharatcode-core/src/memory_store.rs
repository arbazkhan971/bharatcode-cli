//! Persistent cross-session memory.
//!
//! A tiny JSON-backed store for user-tagged facts that should survive across
//! sessions. Facts are persisted under the config directory and can be recalled
//! into the system prompt as a compact `# Memory` block.
//!
//! The feature is opt-in: recall only contributes to the prompt when memory is
//! enabled (via the `BHARATCODE_MEMORY` config value or environment variable).
//! When disabled, [`recall_block`] returns `None` and the prompt is unchanged.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::paths::Paths;

const MEMORY_FILE: &str = "memory.json";
/// Cap how many facts we recall so the prompt block stays compact and cheap.
const MAX_RECALL_FACTS: usize = 25;
/// Opt-in toggle name, shared by env var and config file.
const ENABLE_KEY: &str = "BHARATCODE_MEMORY";

/// A single user-tagged fact to remember across sessions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryFact {
    /// Short category/tag the user filed this fact under (e.g. "preferences").
    pub tag: String,
    /// The remembered content.
    pub content: String,
}

impl MemoryFact {
    pub fn new(tag: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            tag: tag.into(),
            content: content.into(),
        }
    }
}

/// In-memory view of the persisted store.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryStore {
    #[serde(default)]
    facts: Vec<MemoryFact>,
}

impl MemoryStore {
    /// Path to the JSON file backing the store, under the config directory.
    pub fn path() -> PathBuf {
        Paths::in_config_dir(MEMORY_FILE)
    }

    /// Load the store from disk, returning an empty store if the file is
    /// missing or unreadable. Never errors so callers can recall best-effort.
    pub fn load() -> Self {
        let path = Self::path();
        match fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Persist the store to disk, creating the config directory if needed.
    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let serialized = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        fs::write(&path, serialized)
    }

    /// Add a fact. Trims whitespace and skips exact duplicates (tag + content).
    /// Returns true if a new fact was actually stored.
    pub fn add(&mut self, tag: impl Into<String>, content: impl Into<String>) -> bool {
        let fact = MemoryFact::new(
            tag.into().trim().to_string(),
            content.into().trim().to_string(),
        );
        if fact.content.is_empty() {
            return false;
        }
        if self.facts.contains(&fact) {
            return false;
        }
        self.facts.push(fact);
        true
    }

    /// All stored facts, in insertion order.
    pub fn list(&self) -> &[MemoryFact] {
        &self.facts
    }

    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }

    pub fn len(&self) -> usize {
        self.facts.len()
    }

    /// Render a compact recall block for injection into the system prompt, or
    /// `None` when there is nothing to recall. The block is plain text; the
    /// most recent facts are kept when over the recall cap.
    pub fn recall_block(&self) -> Option<String> {
        if self.facts.is_empty() {
            return None;
        }

        let start = self.facts.len().saturating_sub(MAX_RECALL_FACTS);
        let mut body = String::from("# Memory\n\nRemembered facts from previous sessions:\n");
        for fact in &self.facts[start..] {
            if fact.tag.is_empty() {
                body.push_str(&format!("- {}\n", fact.content));
            } else {
                body.push_str(&format!("- [{}] {}\n", fact.tag, fact.content));
            }
        }
        Some(body)
    }
}

/// Whether persistent memory recall is enabled. Opt-in via the
/// `BHARATCODE_MEMORY` environment variable or the config value of the same
/// name. Any truthy-ish value (`1`, `true`, `yes`, `on`) enables it.
pub fn is_enabled() -> bool {
    if let Ok(raw) = std::env::var(ENABLE_KEY) {
        return is_truthy(&raw);
    }
    crate::config::Config::global()
        .get_param::<String>(ENABLE_KEY)
        .map(|raw| is_truthy(&raw))
        .unwrap_or(false)
}

fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Best-effort recall used by prompt assembly. Returns the compact memory block
/// only when the feature is enabled and there are facts to recall; otherwise
/// `None`, leaving the prompt untouched.
pub fn recall_for_prompt() -> Option<String> {
    if !is_enabled() {
        return None;
    }
    MemoryStore::load().recall_block()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_skips_empty_and_duplicates() {
        let mut store = MemoryStore::default();
        assert!(store.add("pref", "likes rust"));
        assert!(!store.add("pref", "likes rust"), "exact duplicate ignored");
        assert!(!store.add("pref", "   "), "empty content ignored");
        assert!(store.add("pref", "likes vim"));
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn add_trims_whitespace() {
        let mut store = MemoryStore::default();
        assert!(store.add("  pref  ", "  likes rust  "));
        let fact = &store.list()[0];
        assert_eq!(fact.tag, "pref");
        assert_eq!(fact.content, "likes rust");
    }

    #[test]
    fn recall_block_is_none_when_empty() {
        let store = MemoryStore::default();
        assert!(store.recall_block().is_none());
    }

    #[test]
    fn recall_block_renders_tagged_and_untagged() {
        let mut store = MemoryStore::default();
        store.add("preferences", "uses tabs");
        store.add("", "no tag here");

        let block = store.recall_block().expect("non-empty store recalls");
        assert!(block.starts_with("# Memory"));
        assert!(block.contains("- [preferences] uses tabs"));
        assert!(block.contains("- no tag here"));
    }

    #[test]
    fn recall_block_caps_to_most_recent() {
        let mut store = MemoryStore::default();
        for i in 0..(MAX_RECALL_FACTS + 5) {
            store.add("n", format!("fact {i}"));
        }
        let block = store.recall_block().unwrap();
        let line_count = block.lines().filter(|l| l.starts_with("- ")).count();
        assert_eq!(line_count, MAX_RECALL_FACTS);
        // Oldest facts are dropped, newest are kept.
        assert!(!block.contains("fact 0"));
        assert!(block.contains(&format!("fact {}", MAX_RECALL_FACTS + 4)));
    }

    #[test]
    fn serde_round_trips_through_json() {
        let mut store = MemoryStore::default();
        store.add("a", "one");
        store.add("b", "two");
        let json = serde_json::to_string(&store).unwrap();
        let restored: MemoryStore = serde_json::from_str(&json).unwrap();
        assert_eq!(store, restored);
    }

    #[test]
    fn save_and_load_persists_facts() {
        let tmp = tempfile::tempdir().unwrap();
        let temp_root = tmp.path().display().to_string();
        let _guard = env_lock::lock_env([("BHARATCODE_PATH_ROOT", Some(temp_root.as_str()))]);

        let mut store = MemoryStore::default();
        store.add("project", "ships on fridays");
        store.save().unwrap();

        let loaded = MemoryStore::load();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.list()[0].content, "ships on fridays");
    }

    #[test]
    fn is_truthy_recognizes_common_values() {
        assert!(is_truthy("1"));
        assert!(is_truthy("TRUE"));
        assert!(is_truthy(" yes "));
        assert!(is_truthy("on"));
        assert!(!is_truthy("0"));
        assert!(!is_truthy("false"));
        assert!(!is_truthy(""));
    }
}
