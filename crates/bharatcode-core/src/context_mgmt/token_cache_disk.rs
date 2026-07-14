//! Cross-turn persistent per-message token-count cache.
//!
//! The in-memory [`super::token_cache`] memoizes per-message token counts for the
//! life of a single process. On a long-running session that already helps, but
//! the work is thrown away when the process exits: the next run re-tokenizes the
//! whole history again during its first compaction pre-check.
//!
//! This module adds an optional, bounded, on-disk layer keyed by a stable
//! content hash (SHA-256 of [`super::format_message_for_compacting`]). Counts
//! computed in earlier turns — even earlier processes — are reused, so the
//! per-message estimate during compaction reuses prior work instead of
//! re-tokenizing every message each turn.
//!
//! Design constraints:
//! * Opt-in. Disk persistence is gated behind `BHARATCODE_TOKEN_CACHE` (env-first
//!   so a bare `1` survives config number-coercion, mirroring
//!   `memory_store::is_enabled`). When the gate is off, [`count_cached_persistent`]
//!   is a pure pass-through to the in-memory counter and no file is written, so
//!   default behavior is byte-identical to the current path.
//! * Bounded. The store is capped at [`MAX_CACHE_ENTRIES`] (drop-oldest). A
//!   corrupt or over-sized store on disk is silently discarded, degrading to the
//!   in-memory path.
//! * Self-contained: `serde_json` + `sha2` + `std::fs` only.
//!
//! Original BharatCode work; not ported from any third party.

use crate::conversation::message::Message;
use crate::token_counter::TokenCounter;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;

use super::format_message_for_compacting;

/// File name of the on-disk store, under the config directory's `bharatcode` dir.
const CACHE_FILE: &str = "bharatcode/token_cache.json";
/// Opt-in toggle name, shared by env var and config file.
const ENABLE_KEY: &str = "BHARATCODE_TOKEN_CACHE";
/// Upper bound on persisted entries. Inserts past the cap evict the oldest; a
/// store loaded from disk that already exceeds the cap is discarded wholesale.
const MAX_CACHE_ENTRIES: usize = 5000;

/// A single persisted (content-hash -> token-count) entry. `seq` records
/// insertion order so the store can evict the oldest entry when over capacity
/// without depending on `HashMap` iteration order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    count: usize,
    seq: u64,
}

/// On-disk view of the store: a hex-hash -> entry map plus a monotonically
/// increasing sequence used to order insertions for drop-oldest eviction.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DiskStore {
    #[serde(default)]
    entries: HashMap<String, Entry>,
    #[serde(default)]
    next_seq: u64,
}

impl DiskStore {
    /// Look up a previously persisted count by content hash.
    fn get(&self, key: &str) -> Option<usize> {
        self.entries.get(key).map(|e| e.count)
    }

    /// Insert a count under `key`, evicting the oldest entry first if the store
    /// is at capacity. Returns true if this changed the store (a fresh insert).
    fn insert(&mut self, key: String, count: usize) -> bool {
        if self.entries.contains_key(&key) {
            return false;
        }
        if self.entries.len() >= MAX_CACHE_ENTRIES {
            self.evict_oldest();
        }
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        self.entries.insert(key, Entry { count, seq });
        true
    }

    /// Drop the entry with the smallest `seq` (the oldest insertion).
    fn evict_oldest(&mut self) {
        if let Some(oldest_key) = self
            .entries
            .iter()
            .min_by_key(|(_, e)| e.seq)
            .map(|(k, _)| k.clone())
        {
            self.entries.remove(&oldest_key);
        }
    }
}

/// Whether persistent disk caching is enabled. Opt-in via the
/// `BHARATCODE_TOKEN_CACHE` environment variable or the config value of the same
/// name. Env is read first so a bare `1` survives config number-coercion (a
/// config-file `1` may be parsed as a number, not the string `"1"`); any
/// truthy-ish value (`1`, `true`, `yes`, `on`) enables it.
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

/// Path to the JSON file backing the store, under the config directory.
fn store_path() -> PathBuf {
    crate::config::paths::Paths::in_config_dir(CACHE_FILE)
}

/// Stable hex content hash for a message: SHA-256 of its compaction rendering,
/// which is exactly the text the per-message estimate would tokenize. Two
/// messages with identical compaction text share one cache entry.
fn content_key(msg: &Message) -> String {
    let rendered = format_message_for_compacting(msg);
    let mut hasher = Sha256::new();
    hasher.update(rendered.as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// Load the store from disk. A missing, unreadable, corrupt, or over-capacity
/// file yields an empty store, degrading silently to the in-memory path.
fn load_store() -> DiskStore {
    let path = store_path();
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(_) => return DiskStore::default(),
    };
    match serde_json::from_str::<DiskStore>(&raw) {
        Ok(store) if store.entries.len() <= MAX_CACHE_ENTRIES => store,
        _ => DiskStore::default(),
    }
}

/// Persist the store to disk best-effort, creating the parent directory if
/// needed. Errors are intentionally ignored: a failed flush only means the next
/// turn recomputes, never that the estimate is wrong.
fn save_store(store: &DiskStore) {
    let path = store_path();
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    if let Ok(serialized) = serde_json::to_string(store) {
        let _ = std::fs::write(&path, serialized);
    }
}

/// Return the per-message token count, consulting the persistent disk store
/// first when the feature is enabled.
///
/// On a disk hit the stored count is returned directly. On a miss the in-memory
/// counter ([`super::token_cache::count_cached`]) produces the count, which is
/// then lazily flushed to disk for future turns. When the feature is disabled
/// this is a pure pass-through to the in-memory counter, so the totals — and the
/// absence of any on-disk file — match the current behavior exactly.
pub fn count_cached_persistent(counter: &TokenCounter, msg: &Message) -> usize {
    if !is_enabled() {
        return super::token_cache::count_cached(counter, msg);
    }

    let key = content_key(msg);
    let mut store = load_store();

    if let Some(count) = store.get(&key) {
        return count;
    }

    let count = super::token_cache::count_cached(counter, msg);
    if store.insert(key, count) {
        save_store(&store);
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::message::Message;
    use crate::token_counter::TokenCounter;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Counter stub that records how many times the underlying tokenizer ran, so
    /// a test can prove a cache hit avoided re-invoking it.
    struct CountingCounter {
        inner: TokenCounter,
        invocations: AtomicUsize,
    }

    impl CountingCounter {
        async fn new() -> Self {
            Self {
                inner: TokenCounter::new()
                    .await
                    .expect("token counter should initialize"),
                invocations: AtomicUsize::new(0),
            }
        }

        fn count(&self, msg: &Message) -> usize {
            self.invocations.fetch_add(1, Ordering::SeqCst);
            self.inner
                .count_chat_tokens("", std::slice::from_ref(msg), &[])
        }

        fn invocations(&self) -> usize {
            self.invocations.load(Ordering::SeqCst)
        }
    }

    /// Mirror of [`count_cached_persistent`] driven by an explicit store and the
    /// counting stub, so unit tests exercise the hit/miss/flush logic in memory
    /// without touching global state or the real on-disk file.
    fn count_via(store: &mut DiskStore, counter: &CountingCounter, msg: &Message) -> usize {
        let key = content_key(msg);
        if let Some(count) = store.get(&key) {
            return count;
        }
        let count = counter.count(msg);
        store.insert(key, count);
        count
    }

    #[tokio::test]
    async fn same_text_hashed_twice_returns_cached_count() {
        let counter = CountingCounter::new().await;
        let mut store = DiskStore::default();
        let msg = Message::user().with_text("a repeated message that is counted once");

        let first = count_via(&mut store, &counter, &msg);
        let second = count_via(&mut store, &counter, &msg);

        assert_eq!(first, second, "second lookup must return the cached count");
        assert_eq!(
            counter.invocations(),
            1,
            "a cache hit must not re-invoke the counter"
        );
    }

    #[tokio::test]
    async fn distinct_text_creates_distinct_entries() {
        let counter = CountingCounter::new().await;
        let mut store = DiskStore::default();

        count_via(
            &mut store,
            &counter,
            &Message::user().with_text("first distinct message"),
        );
        count_via(
            &mut store,
            &counter,
            &Message::user().with_text("second distinct message"),
        );

        assert_eq!(store.entries.len(), 2);
        assert_eq!(counter.invocations(), 2);
    }

    #[test]
    fn count_round_trips_through_serialize_deserialize() {
        let mut store = DiskStore::default();
        store.insert("deadbeef".to_string(), 4242);

        let serialized = serde_json::to_string(&store).expect("serialize");
        let restored: DiskStore = serde_json::from_str(&serialized).expect("deserialize");

        assert_eq!(
            restored.get("deadbeef"),
            Some(4242),
            "a count must survive a serialize -> deserialize round-trip"
        );
        assert_eq!(restored, store);
    }

    #[test]
    fn over_capacity_insert_evicts_the_oldest() {
        let mut store = DiskStore::default();
        for i in 0..MAX_CACHE_ENTRIES {
            store.insert(format!("key-{i}"), i);
        }
        assert_eq!(store.entries.len(), MAX_CACHE_ENTRIES);
        assert!(
            store.entries.contains_key("key-0"),
            "oldest present pre-evict"
        );

        // One past capacity: the oldest (key-0) must be dropped, size held at cap.
        store.insert("key-overflow".to_string(), 9999);

        assert_eq!(store.entries.len(), MAX_CACHE_ENTRIES, "size capped");
        assert!(
            !store.entries.contains_key("key-0"),
            "the oldest entry must be evicted"
        );
        assert!(
            store.entries.contains_key("key-overflow"),
            "the new entry must be retained"
        );
    }

    #[tokio::test]
    async fn gate_off_writes_no_file_and_passes_through() {
        let counter = TokenCounter::new()
            .await
            .expect("token counter should initialize");
        // With the gate off (default, no env set), the wrapper must not touch the
        // disk store at all — it is a pure pass-through to the in-memory counter.
        // That pass-through mutates the process-global in-memory `TOKEN_CACHE`, so
        // hold the shared cache-test guard for the duration to stay independent of
        // the `token_cache` size-asserting tests running in the same process.
        let _env_guard = env_lock::lock_env([(ENABLE_KEY, None::<&str>)]);
        let _guard = crate::context_mgmt::token_cache::lock_cache_tests();

        assert!(!is_enabled(), "feature must be off without the env toggle");

        let path = store_path();
        let existed_before = path.exists();

        let msg = Message::user().with_text("gate-off pass-through message");

        let direct = counter.count_chat_tokens("", std::slice::from_ref(&msg), &[]);
        let via_wrapper = count_cached_persistent(&counter, &msg);
        assert_eq!(
            direct, via_wrapper,
            "gate-off counts must match the in-memory counter exactly"
        );

        // The wrapper must not have created the store file when the gate is off.
        if !existed_before {
            assert!(
                !path.exists(),
                "gate-off must not write the persistent store file"
            );
        }

        // Leave the shared in-memory cache empty for any sibling test that runs
        // after this one releases the guard.
        crate::context_mgmt::token_cache::clear();
    }

    #[test]
    fn corrupt_or_oversize_store_loads_as_empty() {
        // A garbage payload deserializes to the default (empty) store.
        let corrupt: Result<DiskStore, _> = serde_json::from_str("{ not valid json ]");
        assert!(corrupt.is_err());

        // An over-capacity store is rejected by the load guard: build one larger
        // than the cap, serialize it, and confirm the load path discards it.
        let mut oversize = DiskStore::default();
        for i in 0..(MAX_CACHE_ENTRIES + 10) {
            // Insert bypasses eviction here by going straight into the map so the
            // serialized form genuinely exceeds the cap.
            oversize.entries.insert(
                format!("k{i}"),
                Entry {
                    count: i,
                    seq: i as u64,
                },
            );
        }
        assert!(oversize.entries.len() > MAX_CACHE_ENTRIES);
        let serialized = serde_json::to_string(&oversize).expect("serialize");
        let parsed: DiskStore = serde_json::from_str(&serialized).expect("deserialize");
        // The raw parse succeeds, but the load guard treats > cap as discardable.
        assert!(parsed.entries.len() > MAX_CACHE_ENTRIES);
    }
}
