//! In-flight provider-request coalescer (single-flight).
//!
//! Concurrent identical provider calls — same `(provider, model, system,
//! messages, tools)` — are common when several subagents or parallel tools
//! issue the same completion or embedding request at once. Without coalescing,
//! every one of those calls hits the provider independently, producing a
//! duplicate-request stampede that wastes tokens, money and rate-limit budget
//! for a single logical result.
//!
//! [`RequestCoalescer`] solves this with single-flight semantics: the first
//! caller for a given request key registers a shared in-flight future; any
//! later caller that arrives while that future is still pending clones and
//! awaits the *same* future instead of starting its own. When the future
//! resolves, every waiter receives an identical copy of the result and the
//! in-flight entry is dropped, so the next request for the same key starts
//! fresh.
//!
//! This is **distinct from the on-disk prompt cache** (`crate::prompt_cache`,
//! a cross-process cache of *completed* results): the coalescer never stores
//! anything past the lifetime of an in-flight call. It only deduplicates calls
//! that overlap in time. The two compose cleanly — the cache short-circuits
//! repeats that have already finished, the coalescer collapses repeats that are
//! still running.
//!
//! ## Default OFF
//!
//! Coalescing is opt-in behind the `BHARATCODE_COALESCE` environment variable
//! (any truthy value: `1`, `true`, `yes`, `on`). When it is unset — the default
//! — [`RequestCoalescer::coalesce`] simply awaits the supplied future directly:
//! no map insertion, no hashing of futures, behaviour byte-for-byte identical to
//! a direct call. A unique request, or one with no concurrent duplicate in
//! flight, likewise behaves exactly like a direct call even when the feature is
//! enabled.

use std::any::Any;
use std::collections::HashMap;
use std::future::Future;
use std::hash::Hasher;
use std::sync::{LazyLock, Mutex, Weak};

use futures::future::{FutureExt, Shared};
use serde::Serialize;

use crate::conversation::message::Message;
use rmcp::model::Tool;

/// Environment variable that opts coalescing in. Default (unset) is OFF.
const ENV_VAR: &str = "BHARATCODE_COALESCE";

/// Whether single-flight coalescing has been opted in via `BHARATCODE_COALESCE`.
///
/// Truthy values are `1`, `true`, `yes`, `on` (case-insensitive, surrounding
/// whitespace ignored). Anything else, including unset, leaves coalescing off.
pub fn is_enabled() -> bool {
    std::env::var(ENV_VAR)
        .map(|raw| is_truthy(&raw))
        .unwrap_or(false)
}

fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Boxed, type-erased clone of the in-flight result type produced by a shared
/// future. Stored behind a [`Weak`] so a completed/dropped call frees its slot
/// automatically without an explicit removal race.
type ErasedShared = dyn Any + Send + Sync;

/// Process-wide table of in-flight shared futures, keyed by request hash.
///
/// Each value is a [`Weak`] handle to the type-erased [`Shared`] future. A
/// `Weak` is used deliberately: once every waiter has finished awaiting and
/// dropped its clone, the strong count hits zero, the entry's `upgrade()`
/// returns `None`, and the slot is reclaimed lazily on the next access for that
/// key — so the map never retains stale, completed calls.
static IN_FLIGHT: LazyLock<Mutex<HashMap<u64, Weak<ErasedShared>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Serialize)]
struct KeyInput<'a> {
    provider: &'a str,
    model: &'a str,
    system: &'a str,
    messages: &'a [Message],
    tools: &'a [Tool],
}

/// Compute a stable `u64` request key for single-flight coalescing.
///
/// The key folds `(provider, model, system, messages, tools)` into a digest the
/// same way [`crate::prompt_cache::cache_key`] does (canonical JSON of the same
/// fields), so two callers issuing the same logical request derive the same key
/// and share one in-flight call. The wider SHA-256 hex of the disk cache is
/// narrowed to a `u64` here because this key only has to be unique among the
/// handful of requests in flight at one instant, not durable across processes.
pub fn request_key(
    provider: &str,
    model: &str,
    system: &str,
    messages: &[Message],
    tools: &[Tool],
) -> u64 {
    let input = KeyInput {
        provider,
        model,
        system,
        messages,
        tools,
    };
    let serialized = serde_json::to_vec(&input).unwrap_or_default();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hasher.write(&serialized);
    hasher.finish()
}

/// Single-flight wrapper around an async provider request.
///
/// A unit struct rather than free functions so the coalescer reads as a small,
/// nameable component at the call site (`RequestCoalescer::coalesce(..)`) and so
/// the streaming / embeddings paths can hold one in a field if they prefer.
pub struct RequestCoalescer;

impl RequestCoalescer {
    /// Run `producer` under single-flight semantics for the given request `key`.
    ///
    /// When coalescing is **disabled** (the default) this awaits `producer`
    /// directly: no hashing, no map insertion, identical behaviour to calling
    /// the future yourself.
    ///
    /// When **enabled**, the first caller for `key` registers a shared future
    /// and drives it; any caller that arrives while that future is still pending
    /// clones and awaits the same future, so the underlying `producer` runs
    /// exactly once and every waiter receives an identical `T`. Once all waiters
    /// have finished, the in-flight slot is released and the next call for `key`
    /// starts a fresh producer.
    ///
    /// `T` must be `Clone` because [`Shared`] hands every waiter its own copy of
    /// the resolved value. For fallible provider calls, set `T` to a
    /// `Result<_, E>` whose error type is `Clone` (e.g. an `Arc`-wrapped error).
    pub async fn coalesce<T, F>(key: u64, producer: F) -> T
    where
        T: Clone + Send + Sync + 'static,
        F: Future<Output = T> + Send + 'static,
    {
        if !is_enabled() {
            return producer.await;
        }

        Self::shared_for(key, producer).await
    }

    /// Return the shared future for `key`, registering `producer` as the
    /// in-flight call if no live shared future already exists.
    ///
    /// The map lock is held only for the brief lookup/insert; the returned
    /// `Shared` is awaited *after* the lock is dropped, so concurrent callers
    /// for different keys never serialise on the producer itself.
    ///
    /// The map stores a [`Weak`] to a type-erased keep-alive `Arc`; that same
    /// `Arc` is captured inside the returned future, so the `Weak` stays
    /// upgradeable for exactly as long as at least one waiter is still holding
    /// (and awaiting) a clone of the shared future. Once the last waiter drops,
    /// the `Arc`'s strong count hits zero and the entry's slot is reclaimed on
    /// the next access — no explicit removal, no removal race.
    fn shared_for<T, F>(key: u64, producer: F) -> SharedWithKeepAlive<T>
    where
        T: Clone + Send + Sync + 'static,
        F: Future<Output = T> + Send + 'static,
    {
        let mut map = IN_FLIGHT.lock().unwrap_or_else(|e| e.into_inner());

        if let Some(weak) = map.get(&key) {
            if let Some(strong) = weak.upgrade() {
                match strong.downcast::<Shared<BoxFuture<'static, T>>>() {
                    Ok(typed) => {
                        // A live in-flight call for this key already exists: join
                        // it. We hold the same keep-alive `Arc` (re-erased) so the
                        // map's `Weak` stays upgradeable until both waiters drop.
                        let shared = (*typed).clone();
                        let keep_alive: std::sync::Arc<ErasedShared> = typed;
                        return SharedWithKeepAlive {
                            shared,
                            _keep_alive: keep_alive,
                        };
                    }
                    Err(_) => {
                        // A live entry exists but for a different `T`. A 64-bit
                        // hash collision across two distinct request shapes is
                        // astronomically unlikely; if it ever happens we simply
                        // decline to coalesce and run this caller's producer on
                        // its own, which is always correct.
                        return SharedWithKeepAlive::standalone(producer);
                    }
                }
            }
        }

        let shared: Shared<BoxFuture<'static, T>> = producer.boxed().shared();
        let keep_alive: std::sync::Arc<ErasedShared> = std::sync::Arc::new(shared.clone());
        map.insert(key, std::sync::Arc::downgrade(&keep_alive));
        SharedWithKeepAlive {
            shared,
            _keep_alive: keep_alive,
        }
    }
}

/// Convenience alias: the heap-allocated, `Send` future a producer is boxed
/// into before being shared.
type BoxFuture<'a, T> = futures::future::BoxFuture<'a, T>;

/// A shared future bundled with the type-erased `Arc` keep-alive that the
/// in-flight map's `Weak` points at. Awaiting it yields the producer's result;
/// holding it keeps the map entry upgradeable for concurrent callers.
struct SharedWithKeepAlive<T: Clone> {
    shared: Shared<BoxFuture<'static, T>>,
    _keep_alive: std::sync::Arc<ErasedShared>,
}

impl<T> SharedWithKeepAlive<T>
where
    T: Clone + Send + Sync + 'static,
{
    /// Wrap a producer that is *not* registered in the in-flight map (used for
    /// the rare hash-collision fallback). The keep-alive is an empty `Arc` since
    /// nothing in the map refers to it.
    fn standalone<F>(producer: F) -> Self
    where
        F: Future<Output = T> + Send + 'static,
    {
        Self {
            shared: producer.boxed().shared(),
            _keep_alive: std::sync::Arc::new(()) as std::sync::Arc<ErasedShared>,
        }
    }
}

impl<T> Future for SharedWithKeepAlive<T>
where
    T: Clone + Send + 'static,
{
    type Output = T;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<T> {
        self.shared.poll_unpin(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Serialize gate-flag mutation across tests in this module so a parallel
    /// test never observes another test's `BHARATCODE_COALESCE` value.
    static GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct GateGuard<'a> {
        _lock: std::sync::MutexGuard<'a, ()>,
    }

    fn enable() -> GateGuard<'static> {
        let g = GATE.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var(ENV_VAR, "1");
        GateGuard { _lock: g }
    }

    fn disable() -> GateGuard<'static> {
        let g = GATE.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var(ENV_VAR);
        GateGuard { _lock: g }
    }

    impl Drop for GateGuard<'_> {
        fn drop(&mut self) {
            std::env::remove_var(ENV_VAR);
        }
    }

    #[test]
    fn is_truthy_recognises_common_values() {
        assert!(is_truthy("1"));
        assert!(is_truthy("TRUE"));
        assert!(is_truthy(" yes "));
        assert!(is_truthy("on"));
        assert!(!is_truthy("0"));
        assert!(!is_truthy("false"));
        assert!(!is_truthy(""));
    }

    #[test]
    fn request_key_matches_cache_key_fields() {
        let msgs = vec![Message::user().with_text("hello world")];
        let a = request_key("openai", "gpt-4o", "be helpful", &msgs, &[]);
        let b = request_key("openai", "gpt-4o", "be helpful", &msgs, &[]);
        assert_eq!(a, b, "identical requests must hash equal");

        let other = vec![Message::user().with_text("different")];
        assert_ne!(
            a,
            request_key("openai", "gpt-4o", "be helpful", &other, &[]),
            "different messages must hash differently"
        );
        assert_ne!(
            a,
            request_key("anthropic", "gpt-4o", "be helpful", &msgs, &[]),
            "different provider must hash differently"
        );
    }

    #[tokio::test]
    async fn same_key_runs_producer_once_and_fans_out() {
        let _gate = enable();
        let calls = Arc::new(AtomicUsize::new(0));
        let key = 4242;

        let c1 = calls.clone();
        let f1 = RequestCoalescer::coalesce(key, async move {
            // Yield so the second caller registers before this resolves.
            tokio::task::yield_now().await;
            let n = c1.fetch_add(1, Ordering::SeqCst) + 1;
            tokio::task::yield_now().await;
            n
        });

        let c2 = calls.clone();
        let f2 = RequestCoalescer::coalesce(key, async move {
            tokio::task::yield_now().await;
            let n = c2.fetch_add(1, Ordering::SeqCst) + 1;
            tokio::task::yield_now().await;
            n
        });

        let (r1, r2) = tokio::join!(f1, f2);

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "producer must run exactly once for two concurrent identical keys"
        );
        assert_eq!(
            r1, r2,
            "both callers must receive the same fanned-out value"
        );
        assert_eq!(r1, 1);
    }

    #[tokio::test]
    async fn different_keys_run_producer_twice() {
        let _gate = enable();
        let calls = Arc::new(AtomicUsize::new(0));

        let c1 = calls.clone();
        let f1 = RequestCoalescer::coalesce(1001_u64, async move {
            tokio::task::yield_now().await;
            c1.fetch_add(1, Ordering::SeqCst);
            "a"
        });
        let c2 = calls.clone();
        let f2 = RequestCoalescer::coalesce(2002_u64, async move {
            tokio::task::yield_now().await;
            c2.fetch_add(1, Ordering::SeqCst);
            "b"
        });

        let (r1, r2) = tokio::join!(f1, f2);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "two distinct keys must each run their own producer"
        );
        assert_eq!(r1, "a");
        assert_eq!(r2, "b");
    }

    #[tokio::test]
    async fn gate_off_is_pass_through_per_call() {
        let _gate = disable();
        let calls = Arc::new(AtomicUsize::new(0));
        let key = 7;

        for _ in 0..2 {
            let c = calls.clone();
            let value = RequestCoalescer::coalesce(key, async move {
                c.fetch_add(1, Ordering::SeqCst);
                99
            })
            .await;
            assert_eq!(value, 99);
        }

        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "with the gate off every call must run its own producer (pass-through)"
        );
    }
}
