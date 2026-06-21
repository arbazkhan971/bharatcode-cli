//! Incremental per-message token-count cache.
//!
//! When `check_if_compaction_needed` cannot read a provider-reported total
//! (`session.usage.total_tokens` is `None`), it falls back to estimating the
//! context size by tokenizing every agent-visible message each turn. On large
//! conversations that re-tokenizes the entire history on every pre-check,
//! making the check O(history) when only a handful of messages are new.
//!
//! This module memoizes the per-message token count keyed by a cheap, stable
//! content hash so unchanged history is counted once. The pre-check then costs
//! O(new messages) instead of O(history). The cache is transparent: there is no
//! env gate, and the totals it produces are identical to the uncached path
//! because a hit simply replays the count that `count_chat_tokens` already
//! produced for byte-identical content.
//!
//! Original BharatCode work; not ported from any third party.

use crate::conversation::message::{Message, MessageContent};
use crate::token_counter::TokenCounter;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::slice;
use std::sync::{LazyLock, Mutex};

/// Upper bound on cached entries. Once exceeded the cache is dropped wholesale
/// rather than growing without limit; the next counts simply repopulate it.
const MAX_CACHE_ENTRIES: usize = 50_000;

/// Process-global memo from a stable content hash to its token count.
static TOKEN_CACHE: LazyLock<Mutex<HashMap<u64, usize>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Stable discriminant for the message role. `rmcp::model::Role` does not
/// guarantee a stable `Hash`, so map it explicitly to keep keys reproducible.
fn role_tag(msg: &Message) -> u8 {
    match msg.role {
        rmcp::model::Role::User => 0,
        rmcp::model::Role::Assistant => 1,
    }
}

/// Hash the parts of a message that influence its token count, mirroring how
/// `TokenCounter::count_chat_tokens` reads each content item (text, tool
/// request `id:name:args`, tool response text). Two messages with byte-identical
/// counted content and the same role hash equal, so they share one count.
fn content_hash(msg: &Message) -> u64 {
    let mut hasher = DefaultHasher::new();
    role_tag(msg).hash(&mut hasher);
    for content in &msg.content {
        match content {
            MessageContent::Text(text) => {
                0u8.hash(&mut hasher);
                text.text.hash(&mut hasher);
            }
            MessageContent::ToolRequest(req) => {
                if let Ok(call) = req.tool_call.as_ref() {
                    1u8.hash(&mut hasher);
                    req.id.hash(&mut hasher);
                    call.name.hash(&mut hasher);
                    format!("{:?}", call.arguments).hash(&mut hasher);
                }
            }
            _ => {
                if let Some(resp_text) = content.as_tool_response_text() {
                    2u8.hash(&mut hasher);
                    resp_text.hash(&mut hasher);
                }
            }
        }
    }
    hasher.finish()
}

/// Return the memoized per-message token count, computing and storing it on a
/// miss. The computed value is exactly
/// `counter.count_chat_tokens("", slice::from_ref(msg), &[])`, so summing
/// `count_cached` over a message set yields the same total as calling
/// `count_chat_tokens` per message directly.
pub fn count_cached(counter: &TokenCounter, msg: &Message) -> usize {
    let key = content_hash(msg);

    {
        let cache = TOKEN_CACHE.lock().expect("token cache mutex poisoned");
        if let Some(&count) = cache.get(&key) {
            return count;
        }
    }

    let count = counter.count_chat_tokens("", slice::from_ref(msg), &[]);

    let mut cache = TOKEN_CACHE.lock().expect("token cache mutex poisoned");
    if cache.len() >= MAX_CACHE_ENTRIES {
        cache.clear();
    }
    cache.insert(key, count);
    count
}

/// Empty the cache. Exposed so callers (and tests) can force recomputation.
pub fn clear() {
    TOKEN_CACHE
        .lock()
        .expect("token cache mutex poisoned")
        .clear();
}

#[cfg(test)]
fn cache_len() -> usize {
    TOKEN_CACHE
        .lock()
        .expect("token cache mutex poisoned")
        .len()
}

/// Whether the cache currently holds an entry for `msg`'s content hash. Used by
/// tests to assert per-message presence without depending on the *total* map
/// size, which other (production-path) callers in the same process may grow
/// concurrently.
#[cfg(test)]
fn cache_contains(msg: &Message) -> bool {
    TOKEN_CACHE
        .lock()
        .expect("token cache mutex poisoned")
        .contains_key(&content_hash(msg))
}

/// Process-wide serialization point for every test that touches the global
/// [`TOKEN_CACHE`]. The in-memory `token_cache` tests and the `token_cache_disk`
/// gate-off pass-through test both mutate the same global map, so they must run
/// under this single guard to stay independent. Poison-tolerant on purpose: an
/// earlier test panicking while holding the lock must not cascade into
/// `PoisonError` failures across the others.
#[cfg(test)]
pub(crate) static GLOBAL_CACHE_TEST_GUARD: Mutex<()> = Mutex::new(());

/// Acquire [`GLOBAL_CACHE_TEST_GUARD`], recovering from a poisoned lock.
#[cfg(test)]
pub(crate) fn lock_cache_tests() -> std::sync::MutexGuard<'static, ()> {
    GLOBAL_CACHE_TEST_GUARD
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token_counter::TokenCounter;

    async fn counter() -> TokenCounter {
        TokenCounter::new()
            .await
            .expect("token counter should initialize")
    }

    #[tokio::test]
    async fn same_message_counted_once() {
        let _guard = super::lock_cache_tests();
        clear();

        let counter = counter().await;
        let msg = Message::user().with_text("hello world, this is a repeated message");

        let first = count_cached(&counter, &msg);
        let second = count_cached(&counter, &msg);

        assert_eq!(first, second);
        // Assert on this message's own entry rather than the absolute map size:
        // the global cache is also populated by the production compaction path,
        // so a concurrent test exercising it could otherwise inflate `cache_len`.
        assert!(
            cache_contains(&msg),
            "equal content must be memoized under one entry"
        );
    }

    #[tokio::test]
    async fn distinct_messages_get_distinct_entries() {
        let _guard = super::lock_cache_tests();
        clear();

        let counter = counter().await;
        let a = Message::user().with_text("first distinct message");
        let b = Message::user().with_text("second distinct message");

        assert_ne!(
            content_hash(&a),
            content_hash(&b),
            "distinct content must hash to distinct keys"
        );

        count_cached(&counter, &a);
        count_cached(&counter, &b);

        // Each distinct message must get its own entry. Assert on per-message
        // presence rather than the absolute map size, which other (production)
        // callers in the process may also grow.
        assert!(cache_contains(&a), "first message must be cached");
        assert!(cache_contains(&b), "second message must be cached");
    }

    #[tokio::test]
    async fn clear_empties_cache() {
        let _guard = super::lock_cache_tests();
        clear();

        let counter = counter().await;
        let msg = Message::user().with_text("anything");
        count_cached(&counter, &msg);
        assert!(cache_contains(&msg), "entry present before clear");

        clear();
        assert!(!cache_contains(&msg), "clear must drop the entry");
    }

    #[tokio::test]
    async fn eviction_caps_the_map() {
        let _guard = super::lock_cache_tests();
        clear();

        let counter = counter().await;
        // Drive insertions past the cap; the wholesale drop keeps the map bounded.
        for i in 0..(MAX_CACHE_ENTRIES + 5) {
            count_cached(&counter, &Message::user().with_text(format!("evict-{i}")));
        }
        assert!(
            cache_len() <= MAX_CACHE_ENTRIES,
            "cache must stay within the configured cap"
        );
    }

    #[tokio::test]
    async fn cached_sum_equals_direct_sum() {
        let _guard = super::lock_cache_tests();
        clear();

        let counter = counter().await;
        let messages = vec![
            Message::user().with_text("alpha message content"),
            Message::assistant().with_text("beta reply with more words here"),
            Message::user().with_text("alpha message content"), // duplicate, exercises a hit
            Message::assistant().with_text("gamma final response"),
        ];

        let cached_sum: usize = messages.iter().map(|m| count_cached(&counter, m)).sum();
        let direct_sum: usize = messages
            .iter()
            .map(|m| counter.count_chat_tokens("", std::slice::from_ref(m), &[]))
            .sum();

        assert_eq!(
            cached_sum, direct_sum,
            "cached totals must match the uncached path exactly"
        );
    }
}
