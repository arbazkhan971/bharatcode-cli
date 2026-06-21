//! `bharatcode model-pack`: an offline, air-gap-friendly bundle manifest of
//! recommended India / open-weight local models.
//!
//! On a metered or fully offline connection it is useful to know *exactly* what
//! to fetch up front. This command prints a curated manifest of recommended
//! local models (Ollama tags) with an approximate download size and the exact
//! copy-pasteable `ollama pull` command per entry, plus a couple of India-hosted
//! API-only options (Sarvam / Krutrim) that cannot be pulled offline but are
//! worth noting alongside.
//!
//! The command is read-only and informational: it never touches the network, so
//! it is safe to run on an air-gapped box to plan a download list.
//!
//! Where an entry's id is recognised by the static [`model_registry`], its
//! published context window is shown for additional context.
//!
//! Original BharatCode work; not ported from any third party.

use bharatcode_core::model_registry;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// A single entry in the offline model pack manifest.
///
/// Fields are `Cow<'static, str>` so the [`PACK`] can be built from cheap
/// `'static` string literals while still round-tripping through
/// `Deserialize` (which yields owned strings).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelPackEntry {
    /// Canonical short id for the model (used to cross-reference the registry).
    pub id: Cow<'static, str>,
    /// The Ollama tag to pull (empty for API-only, non-pullable entries).
    pub ollama_tag: Cow<'static, str>,
    /// Human-readable approximate download size (e.g. `"~4.7 GB"`).
    pub approx_size: Cow<'static, str>,
    /// One-line note describing the entry.
    pub note: Cow<'static, str>,
    /// Exact, copy-pasteable command to fetch this entry.
    pub pull_cmd: Cow<'static, str>,
}

/// The curated offline model pack: open-weight local models first (pullable via
/// Ollama), then India-hosted API-only options that are noted for completeness.
pub static PACK: &[ModelPackEntry] = &[
    ModelPackEntry {
        id: Cow::Borrowed("qwen2.5-coder"),
        ollama_tag: Cow::Borrowed("qwen2.5-coder:7b"),
        approx_size: Cow::Borrowed("~4.7 GB"),
        note: Cow::Borrowed(
            "Open-weight coding model (7B). Strong default for local coding. No API key.",
        ),
        pull_cmd: Cow::Borrowed("ollama pull qwen2.5-coder:7b"),
    },
    ModelPackEntry {
        id: Cow::Borrowed("qwen2.5-coder-32b"),
        ollama_tag: Cow::Borrowed("qwen2.5-coder:32b"),
        approx_size: Cow::Borrowed("~20 GB"),
        note: Cow::Borrowed("Larger 32B Qwen coder for capable hardware. No API key."),
        pull_cmd: Cow::Borrowed("ollama pull qwen2.5-coder:32b"),
    },
    ModelPackEntry {
        id: Cow::Borrowed("deepseek-coder"),
        ollama_tag: Cow::Borrowed("deepseek-coder-v2:16b"),
        approx_size: Cow::Borrowed("~8.9 GB"),
        note: Cow::Borrowed("Open-weight DeepSeek Coder V2 (16B MoE). No API key."),
        pull_cmd: Cow::Borrowed("ollama pull deepseek-coder-v2:16b"),
    },
    ModelPackEntry {
        id: Cow::Borrowed("llama-3.1-8b"),
        ollama_tag: Cow::Borrowed("llama3.1:8b"),
        approx_size: Cow::Borrowed("~4.9 GB"),
        note: Cow::Borrowed("Meta Llama 3.1 8B general-purpose instruct model. No API key."),
        pull_cmd: Cow::Borrowed("ollama pull llama3.1:8b"),
    },
    ModelPackEntry {
        id: Cow::Borrowed("sarvam-m"),
        ollama_tag: Cow::Borrowed(""),
        approx_size: Cow::Borrowed("API-only (no local download)"),
        note: Cow::Borrowed(
            "India-hosted Sarvam-M. API-only; set SARVAM_API_KEY. Not pullable offline.",
        ),
        pull_cmd: Cow::Borrowed("bharatcode configure   # select Sarvam, set SARVAM_API_KEY"),
    },
    ModelPackEntry {
        id: Cow::Borrowed("krutrim-llama-3.3-70b"),
        ollama_tag: Cow::Borrowed(""),
        approx_size: Cow::Borrowed("API-only (no local download)"),
        note: Cow::Borrowed(
            "Ola Krutrim (India-hosted) Llama-3.3-70B. API-only; set KRUTRIM_API_KEY.",
        ),
        pull_cmd: Cow::Borrowed("bharatcode configure   # select Krutrim, set KRUTRIM_API_KEY"),
    },
];

/// Options for the `model-pack` subcommand.
#[derive(Debug, Clone, Default)]
pub struct ModelPackOptions {
    /// Emit the manifest as pretty-printed JSON instead of a table.
    pub json: bool,
}

/// Look up a user-facing label through the i18n layer, falling back to the
/// English `default` when the active locale has no entry for `key`.
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// Render the manifest as a human-readable table (theme-aware).
fn print_table() {
    println!();
    println!(
        "  {}",
        crate::theme::heading(label("model_pack.title", "BharatCode offline model pack",))
    );
    println!(
        "  {}",
        crate::theme::muted(label(
            "model_pack.note",
            "Fetch these up front on a fast/unmetered connection, then run offline.",
        ))
    );
    println!();

    for entry in PACK {
        let ctx = model_registry::lookup(&entry.id).map(|info| info.context_window);
        let size_line = match ctx {
            Some(window) => format!("{}   ({} ctx)", entry.approx_size, format_tokens(window)),
            None => entry.approx_size.to_string(),
        };

        println!("  {}", crate::theme::accent(entry.id.as_ref()).bold());
        println!(
            "    {:<10} {}",
            crate::theme::muted(label("model_pack.size", "size")),
            crate::theme::success(size_line),
        );
        if !entry.ollama_tag.is_empty() {
            println!(
                "    {:<10} {}",
                crate::theme::muted(label("model_pack.tag", "tag")),
                entry.ollama_tag,
            );
        }
        println!(
            "    {:<10} {}",
            crate::theme::muted(label("model_pack.note_label", "note")),
            crate::theme::muted(entry.note.as_ref()),
        );
        println!(
            "    {:<10} {}",
            crate::theme::muted(label("model_pack.pull", "pull")),
            crate::theme::accent(entry.pull_cmd.as_ref()),
        );
        println!();
    }
}

/// Format a token count compactly (e.g. `131k`, `2.0M`).
fn format_tokens(n: u32) -> String {
    let n = n as f64;
    if n >= 1.0e6 {
        format!("{:.1}M", n / 1.0e6)
    } else if n >= 1.0e3 {
        format!("{:.0}k", n / 1.0e3)
    } else {
        format!("{n:.0}")
    }
}

/// Entry point for `bharatcode model-pack`.
pub async fn handle_model_pack(opts: ModelPackOptions) -> anyhow::Result<()> {
    if opts.json {
        println!("{}", serde_json::to_string_pretty(PACK)?);
    } else {
        print_table();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_has_at_least_four_entries() {
        assert!(PACK.len() >= 4, "expected >= 4 entries, got {}", PACK.len());
    }

    #[test]
    fn every_entry_has_id_size_and_pull_cmd() {
        for entry in PACK {
            assert!(!entry.id.is_empty(), "entry id must be non-empty");
            assert!(
                !entry.approx_size.is_empty(),
                "entry {} must have a size",
                entry.id
            );
            assert!(
                !entry.pull_cmd.is_empty(),
                "entry {} must have a pull command",
                entry.id
            );
        }
    }

    #[test]
    fn pullable_entries_reference_their_ollama_tag() {
        // Every entry that ships an Ollama tag must embed that exact tag in its
        // copy-pasteable pull command, so the printed command is correct.
        let mut pullable = 0;
        for entry in PACK {
            if !entry.ollama_tag.is_empty() {
                pullable += 1;
                assert!(
                    entry.pull_cmd.contains(entry.ollama_tag.as_ref()),
                    "pull_cmd for {} must contain its ollama_tag '{}', got '{}'",
                    entry.id,
                    entry.ollama_tag,
                    entry.pull_cmd,
                );
            }
        }
        assert!(
            pullable >= 4,
            "expected >= 4 pullable (ollama) entries, got {pullable}"
        );
    }

    #[test]
    fn manifest_json_roundtrips() {
        let json = serde_json::to_string_pretty(PACK).expect("serialize");
        let parsed: Vec<ModelPackEntry> =
            serde_json::from_str(&json).expect("deserialize round-trip");
        assert_eq!(parsed.len(), PACK.len());
        for (a, b) in parsed.iter().zip(PACK.iter()) {
            assert_eq!(&a, &b);
        }
    }
}
