//! Opt-in headless multi-session registry for `bharatcode serve`.
//!
//! By default `bharatcode serve` hosts a single ACP session and its behaviour is
//! byte-identical to upstream. When `BHARATCODE_MULTI_SESSION` is set to a truthy
//! value, the serve path instead stands up a [`MultiSessionRegistry`]: a bounded,
//! in-process map of named agent sessions keyed by an opaque `session_key`, so a
//! single headless process can host several concurrently-named sessions isolated
//! from one another.
//!
//! The registry is intentionally pure (`std` + `chrono`) and carries no
//! dependency on the server crate, so the portable-default build keeps compiling
//! even where the headless server is not wired in. Each slot records a creation
//! instant and a monotonically-increasing access counter; on overflow the
//! least-recently-used (LRU) slot is evicted to keep the map within
//! [`MultiSessionRegistry::max_sessions`].
//!
//! Tuning:
//!   * `BHARATCODE_MULTI_SESSION`  truthy (`1`, `true`, `yes`, `on`) => enabled.
//!   * `BHARATCODE_MAX_SESSIONS`   integer, clamped to `1..=64` (default `8`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

use chrono::{DateTime, FixedOffset, Utc};

/// Environment key that turns the multi-session registry on. Absent / falsey =>
/// fully disabled (the single-session serve path is unchanged).
pub const MULTI_SESSION_ENABLED_KEY: &str = "BHARATCODE_MULTI_SESSION";

/// Environment key that caps how many named sessions a single headless process
/// will host concurrently.
pub const MAX_SESSIONS_KEY: &str = "BHARATCODE_MAX_SESSIONS";

/// Lower bound for the session cap (a registry must hold at least one session).
const MIN_SESSIONS: usize = 1;
/// Upper bound for the session cap (guards against absurd / hostile values).
const MAX_SESSIONS_CEILING: usize = 64;
/// Default cap when `BHARATCODE_MAX_SESSIONS` is unset or unparseable.
const DEFAULT_MAX_SESSIONS: usize = 8;

/// Whether the multi-session registry is enabled for this process.
///
/// Reads `BHARATCODE_MULTI_SESSION` straight from the environment and accepts the
/// usual truthy spellings (`1`, `true`, `yes`, `on`); anything else — including
/// absence — is OFF. Reading the raw env var (rather than a coerced config value)
/// mirrors `goose::memory_store::is_enabled`, so a bare `1` survives without being
/// mangled by config-number coercion.
pub fn is_enabled() -> bool {
    match std::env::var(MULTI_SESSION_ENABLED_KEY) {
        Ok(raw) => is_truthy(&raw),
        Err(_) => false,
    }
}

fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// The configured maximum number of concurrent named sessions.
///
/// Read from `BHARATCODE_MAX_SESSIONS` and clamped into `1..=64`; an unset,
/// empty, or unparseable value falls back to the default of `8`. The clamp means
/// `0` becomes `1` and `999` becomes `64`.
pub fn max_sessions() -> usize {
    match std::env::var(MAX_SESSIONS_KEY) {
        Ok(raw) => match raw.trim().parse::<usize>() {
            Ok(n) => n.clamp(MIN_SESSIONS, MAX_SESSIONS_CEILING),
            Err(_) => DEFAULT_MAX_SESSIONS,
        },
        Err(_) => DEFAULT_MAX_SESSIONS,
    }
}

/// India Standard Time (UTC+05:30), the wall clock surfaced to operators in
/// [`MultiSessionRegistry::list`].
fn ist_offset() -> FixedOffset {
    FixedOffset::east_opt(5 * 3600 + 30 * 60).expect("IST (+05:30) is a valid fixed offset")
}

/// A startup banner describing the multi-session mode the serve path is entering.
///
/// Kept here (next to the registry it describes) so the wired call site in
/// `cli.rs` stays a one-liner.
pub fn banner(max: usize) -> String {
    format!("multi-session serve enabled (max {max} concurrent named sessions)")
}

/// One hosted session slot. Opaque to the registry: the registry only tracks the
/// bookkeeping needed to isolate, list, and evict slots. The real per-session
/// agent wiring is attached by the serve path that owns the registry.
#[derive(Debug, Clone)]
pub struct SessionHandle {
    /// The `session_key` this slot is registered under.
    pub key: String,
    /// When the slot was first created (UTC; rendered to IST on `list`).
    pub created_at: DateTime<Utc>,
    /// Monotonic access tick, bumped on every `get_or_create` touch. Drives LRU
    /// eviction: the slot with the smallest tick is the least-recently-used.
    last_access: u64,
}

impl SessionHandle {
    /// IST creation timestamp rendered as `YYYY-MM-DD HH:MM:SS`.
    pub fn created_at_ist(&self) -> String {
        self.created_at
            .with_timezone(&ist_offset())
            .format("%Y-%m-%d %H:%M:%S")
            .to_string()
    }
}

/// A bounded, thread-safe registry of named agent sessions.
///
/// Hosting is opt-in (see [`is_enabled`]); when active, the serve path builds one
/// registry with [`MultiSessionRegistry::new`] and shares it across connections.
#[derive(Debug)]
pub struct MultiSessionRegistry {
    sessions: RwLock<HashMap<String, SessionHandle>>,
    max_sessions: usize,
    access_clock: AtomicU64,
}

impl MultiSessionRegistry {
    /// Build a registry with an explicit cap. The cap is clamped into `1..=64`
    /// so a caller cannot construct a zero-capacity (always-evicting) registry.
    pub fn new(max_sessions: usize) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            max_sessions: max_sessions.clamp(MIN_SESSIONS, MAX_SESSIONS_CEILING),
            access_clock: AtomicU64::new(0),
        }
    }

    /// Build a registry sized from the environment (`BHARATCODE_MAX_SESSIONS`).
    pub fn from_env() -> Self {
        Self::new(max_sessions())
    }

    /// The cap this registry was built with (already clamped to `1..=64`).
    pub fn max_sessions(&self) -> usize {
        self.max_sessions
    }

    /// Number of currently-hosted sessions.
    pub fn len(&self) -> usize {
        self.sessions.read().expect("registry lock poisoned").len()
    }

    /// Whether the registry currently hosts no sessions.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn next_tick(&self) -> u64 {
        self.access_clock.fetch_add(1, Ordering::Relaxed)
    }

    /// Evict the least-recently-used session iff the map is at capacity.
    ///
    /// Returns the evicted key, if any. Called from [`get_or_create`] before a
    /// brand-new key is inserted so the map never exceeds `max_sessions`.
    pub fn evict_oldest_if_full(&self) -> Option<String> {
        let mut guard = self.sessions.write().expect("registry lock poisoned");
        if guard.len() < self.max_sessions {
            return None;
        }
        let victim = guard
            .values()
            .min_by_key(|h| h.last_access)
            .map(|h| h.key.clone())?;
        guard.remove(&victim);
        Some(victim)
    }

    /// Return the handle for `key`, creating it if absent.
    ///
    /// Idempotent for a given key: a repeat call returns a clone of the same slot
    /// (with its access tick refreshed for LRU). Inserting a brand-new key when
    /// the registry is full first evicts the least-recently-used slot.
    pub fn get_or_create(&self, key: &str) -> SessionHandle {
        {
            let mut guard = self.sessions.write().expect("registry lock poisoned");
            if let Some(handle) = guard.get_mut(key) {
                handle.last_access = self.next_tick();
                return handle.clone();
            }
        }

        // New key: make room (LRU) before inserting so we never exceed the cap.
        self.evict_oldest_if_full();

        let mut guard = self.sessions.write().expect("registry lock poisoned");
        // Another writer may have created it between the two locks; honour that.
        if let Some(handle) = guard.get_mut(key) {
            handle.last_access = self.next_tick();
            return handle.clone();
        }
        let handle = SessionHandle {
            key: key.to_string(),
            created_at: Utc::now(),
            last_access: self.next_tick(),
        };
        guard.insert(key.to_string(), handle.clone());
        handle
    }

    /// Drop a session by key, returning the removed handle if it existed.
    pub fn remove(&self, key: &str) -> Option<SessionHandle> {
        self.sessions
            .write()
            .expect("registry lock poisoned")
            .remove(key)
    }

    /// Snapshot of hosted sessions as `(key, created_at_ist)` tuples, ordered by
    /// creation time (oldest first) for a stable operator-facing listing.
    pub fn list(&self) -> Vec<(String, String)> {
        let guard = self.sessions.read().expect("registry lock poisoned");
        let mut handles: Vec<&SessionHandle> = guard.values().collect();
        handles.sort_by_key(|h| h.created_at);
        handles
            .into_iter()
            .map(|h| (h.key.clone(), h.created_at_ist()))
            .collect()
    }
}

impl Default for MultiSessionRegistry {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_SESSIONS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serialises the env-var-touching tests; `set_var`/`remove_var` mutate
    /// process-global state and would otherwise race under the test runner.
    static ENV_GUARD: Mutex<()> = Mutex::new(());

    #[test]
    fn is_enabled_false_when_env_unset() {
        let _guard = ENV_GUARD.lock().unwrap();
        std::env::remove_var(MULTI_SESSION_ENABLED_KEY);
        assert!(!is_enabled());
    }

    #[test]
    fn is_enabled_truthy_spellings() {
        let _guard = ENV_GUARD.lock().unwrap();
        for raw in ["1", "true", "TRUE", " yes ", "on"] {
            std::env::set_var(MULTI_SESSION_ENABLED_KEY, raw);
            assert!(is_enabled(), "{raw:?} should enable");
        }
        for raw in ["0", "false", "no", "off", ""] {
            std::env::set_var(MULTI_SESSION_ENABLED_KEY, raw);
            assert!(!is_enabled(), "{raw:?} should not enable");
        }
        std::env::remove_var(MULTI_SESSION_ENABLED_KEY);
    }

    #[test]
    fn max_sessions_clamps_and_defaults() {
        let _guard = ENV_GUARD.lock().unwrap();

        std::env::remove_var(MAX_SESSIONS_KEY);
        assert_eq!(max_sessions(), DEFAULT_MAX_SESSIONS);

        std::env::set_var(MAX_SESSIONS_KEY, "0");
        assert_eq!(max_sessions(), 1, "0 clamps up to 1");

        std::env::set_var(MAX_SESSIONS_KEY, "999");
        assert_eq!(max_sessions(), 64, "999 clamps down to 64");

        std::env::set_var(MAX_SESSIONS_KEY, "16");
        assert_eq!(max_sessions(), 16, "in-range value passes through");

        std::env::set_var(MAX_SESSIONS_KEY, "not-a-number");
        assert_eq!(max_sessions(), DEFAULT_MAX_SESSIONS, "garbage falls back");

        std::env::remove_var(MAX_SESSIONS_KEY);
    }

    #[test]
    fn new_clamps_explicit_cap() {
        assert_eq!(MultiSessionRegistry::new(0).max_sessions(), 1);
        assert_eq!(MultiSessionRegistry::new(999).max_sessions(), 64);
        assert_eq!(MultiSessionRegistry::new(4).max_sessions(), 4);
    }

    #[test]
    fn get_or_create_is_idempotent_for_same_key() {
        let reg = MultiSessionRegistry::new(4);
        let first = reg.get_or_create("alpha");
        let second = reg.get_or_create("alpha");
        assert_eq!(first.key, second.key);
        assert_eq!(
            first.created_at, second.created_at,
            "same key must reuse the same slot, not recreate it"
        );
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn caps_at_max_and_evicts_oldest_lru() {
        let reg = MultiSessionRegistry::new(3);
        reg.get_or_create("a");
        reg.get_or_create("b");
        reg.get_or_create("c");
        assert_eq!(reg.len(), 3);

        // Touch "a" so it is now more-recently-used than "b": "b" becomes the LRU.
        reg.get_or_create("a");

        // Inserting a 4th distinct key must evict the LRU ("b") and stay at cap.
        reg.get_or_create("d");
        assert_eq!(reg.len(), 3, "registry must never exceed its cap");

        let keys: Vec<String> = reg.list().into_iter().map(|(k, _)| k).collect();
        assert!(keys.contains(&"a".to_string()), "recently-touched survives");
        assert!(keys.contains(&"c".to_string()));
        assert!(keys.contains(&"d".to_string()), "newest survives");
        assert!(
            !keys.contains(&"b".to_string()),
            "least-recently-used must be evicted"
        );
    }

    #[test]
    fn evict_oldest_if_full_noop_below_cap() {
        let reg = MultiSessionRegistry::new(3);
        reg.get_or_create("only");
        assert_eq!(reg.evict_oldest_if_full(), None, "below cap => no eviction");
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn remove_drops_session() {
        let reg = MultiSessionRegistry::new(2);
        reg.get_or_create("x");
        assert!(reg.remove("x").is_some());
        assert!(reg.remove("x").is_none());
        assert!(reg.is_empty());
    }

    #[test]
    fn list_returns_key_and_ist_timestamp_tuples() {
        let reg = MultiSessionRegistry::new(2);
        reg.get_or_create("s1");
        let listing = reg.list();
        assert_eq!(listing.len(), 1);
        let (key, ist) = &listing[0];
        assert_eq!(key, "s1");
        // IST stamp rendered as `YYYY-MM-DD HH:MM:SS`.
        assert_eq!(ist.len(), "0000-00-00 00:00:00".len());
        assert!(ist.contains('-') && ist.contains(':'));
    }

    #[test]
    fn banner_mentions_cap_and_no_vendor_leak() {
        let b = banner(8);
        assert!(b.contains('8'));
        let lower = b.to_ascii_lowercase();
        assert!(!lower.contains("goose"));
        assert!(!lower.contains("block"));
    }
}
