//! Opt-in on-disk prompt/response cache.
//!
//! When the `BHARATCODE_CACHE` environment variable is set to a truthy value,
//! identical non-streaming model calls are short-circuited by serving a
//! previously stored completion from disk. The cache is keyed by
//! `(provider, model, request hash)` where the request hash covers the system
//! prompt, the conversation messages and the available tools.
//!
//! The cache is **off by default**: when the environment variable is unset the
//! public hook (`cached_complete`) is a transparent pass-through, so observable
//! behavior is unchanged. Lookups and writes are best-effort; any I/O or
//! deserialization failure degrades gracefully to a cache miss rather than
//! surfacing an error to the caller.

use std::future::Future;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::paths::Paths;
use crate::conversation::message::Message;
use crate::utils::bytes_to_hex;
use bharatcode_providers::conversation::token_usage::{ProviderUsage, Usage};
use bharatcode_providers::errors::ProviderError;
use rmcp::model::Tool;

/// Environment variable that opts the cache in. Default behavior (unset) is OFF.
const ENV_VAR: &str = "BHARATCODE_CACHE";

/// Sub-directory under the config dir where cached completions live.
const CACHE_SUBDIR: &str = "prompt_cache";

/// Returns `true` when the cache has been explicitly opted in via the
/// `BHARATCODE_CACHE` environment variable.
///
/// Truthy values are `1`, `true`, `yes`, `on` (case-insensitive). Any other
/// value, including unset, leaves the cache disabled.
pub fn is_enabled() -> bool {
    matches!(
        std::env::var(ENV_VAR).ok().as_deref().map(str::trim),
        Some("1")
            | Some("true")
            | Some("TRUE")
            | Some("True")
            | Some("yes")
            | Some("YES")
            | Some("Yes")
            | Some("on")
            | Some("ON")
            | Some("On")
    )
}

/// A cached model completion: the assistant message plus reported usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedCompletion {
    pub message: Message,
    pub usage: ProviderUsage,
}

#[derive(Serialize)]
struct KeyInput<'a> {
    provider: &'a str,
    model: &'a str,
    system: &'a str,
    messages: &'a [Message],
    tools: &'a [Tool],
}

/// Compute a stable cache key for a request.
///
/// The key is a hex-encoded SHA-256 digest over a canonical serialization of
/// `(provider, model, system, messages, tools)`. The provider and model are
/// folded into the digest so distinct providers/models never collide even when
/// the prompt is identical.
pub fn cache_key(
    provider: &str,
    model: &str,
    system: &str,
    messages: &[Message],
    tools: &[Tool],
) -> String {
    let input = KeyInput {
        provider,
        model,
        system,
        messages,
        tools,
    };
    let serialized = serde_json::to_vec(&input).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(&serialized);
    bytes_to_hex(hasher.finalize())
}

fn cache_root() -> PathBuf {
    Paths::in_config_dir(CACHE_SUBDIR)
}

fn entry_path(root: &Path, key: &str) -> PathBuf {
    root.join(format!("{key}.json"))
}

fn read_entry(path: &Path) -> Option<CachedCompletion> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_entry(path: &Path, entry: &CachedCompletion) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_vec_pretty(entry)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    // Write to a unique temp file then rename so concurrent writers and readers
    // never observe a partially written entry.
    let tmp = path.with_extension(format!(
        "tmp.{}.{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(&tmp, &content)?;
    std::fs::rename(&tmp, path)
}

/// Return a copy of `usage` with all token counts zeroed.
///
/// A completion served from the on-disk cache costs nothing — no tokens are sent
/// to or returned from the provider — so reporting the originally-recorded token
/// counts would double-charge the ₹ ledger for work that never ran. The model
/// name is preserved so downstream consumers still attribute the (zero-cost)
/// usage to the right model.
pub fn zero_cost_usage(usage: &ProviderUsage) -> ProviderUsage {
    ProviderUsage::new(usage.model.clone(), Usage::default())
}

/// Look up a cached completion by key. Returns `None` on a miss or on any error.
pub fn lookup(key: &str) -> Option<CachedCompletion> {
    read_entry(&entry_path(&cache_root(), key))
}

/// Store a completion under the given key. Best-effort: failures are logged at
/// debug level and otherwise ignored.
pub fn store(key: &str, entry: &CachedCompletion) {
    let path = entry_path(&cache_root(), key);
    if let Err(e) = write_entry(&path, entry) {
        tracing::debug!(target: "prompt_cache", error = %e, "failed to write prompt cache entry");
    }
}

/// Thin opt-in caching wrapper around a non-streaming model call.
///
/// When the cache is disabled this simply awaits `compute`, leaving behavior
/// unchanged. When enabled it serves a stored completion on a hit, otherwise it
/// runs `compute`, stores the result, and returns it.
pub async fn cached_complete<F, Fut>(
    provider: &str,
    model: &str,
    system: &str,
    messages: &[Message],
    tools: &[Tool],
    compute: F,
) -> Result<(Message, ProviderUsage), ProviderError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<(Message, ProviderUsage), ProviderError>>,
{
    if !is_enabled() {
        return compute().await;
    }

    let key = cache_key(provider, model, system, messages, tools);

    if let Some(hit) = lookup(&key) {
        tracing::debug!(target: "prompt_cache", %key, "prompt cache hit");
        // A cache hit makes no provider call, so report it as zero cost rather
        // than replaying the original token counts (which would inflate the
        // ₹ ledger for work that never ran).
        return Ok((hit.message, zero_cost_usage(&hit.usage)));
    }

    let (message, usage) = compute().await?;
    store(
        &key,
        &CachedCompletion {
            message: message.clone(),
            usage: usage.clone(),
        },
    );
    Ok((message, usage))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bharatcode_providers::conversation::token_usage::Usage;

    fn sample_messages() -> Vec<Message> {
        vec![Message::user().with_text("hello world")]
    }

    #[test]
    fn key_is_deterministic_for_identical_input() {
        let msgs = sample_messages();
        let a = cache_key("openai", "gpt-4o", "be helpful", &msgs, &[]);
        let b = cache_key("openai", "gpt-4o", "be helpful", &msgs, &[]);
        assert_eq!(a, b);
        // SHA-256 hex is 64 characters.
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn key_changes_with_provider_model_or_prompt() {
        let msgs = sample_messages();
        let base = cache_key("openai", "gpt-4o", "be helpful", &msgs, &[]);

        assert_ne!(
            base,
            cache_key("anthropic", "gpt-4o", "be helpful", &msgs, &[])
        );
        assert_ne!(
            base,
            cache_key("openai", "gpt-4o-mini", "be helpful", &msgs, &[])
        );
        assert_ne!(
            base,
            cache_key("openai", "gpt-4o", "be concise", &msgs, &[])
        );

        let other_msgs = vec![Message::user().with_text("different")];
        assert_ne!(
            base,
            cache_key("openai", "gpt-4o", "be helpful", &other_msgs, &[])
        );
    }

    #[test]
    fn store_and_lookup_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let key = "abc123";
        let path = entry_path(dir.path(), key);

        assert!(read_entry(&path).is_none());

        let entry = CachedCompletion {
            message: Message::assistant().with_text("cached answer"),
            usage: ProviderUsage::new("gpt-4o".to_string(), Usage::default()),
        };
        write_entry(&path, &entry).expect("write entry");

        let loaded = read_entry(&path).expect("entry should be readable");
        assert_eq!(loaded.usage.model, "gpt-4o");
        assert_eq!(
            serde_json::to_string(&loaded.message).unwrap(),
            serde_json::to_string(&entry.message).unwrap()
        );
    }

    #[test]
    fn lookup_miss_on_corrupt_entry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = entry_path(dir.path(), "corrupt");
        std::fs::create_dir_all(dir.path()).unwrap();
        std::fs::write(&path, b"not json").unwrap();
        assert!(read_entry(&path).is_none());
    }
}
