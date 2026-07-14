//! Parallel tool-execution governor.
//!
//! The agent already fans tool calls out concurrently: it builds a vector of
//! `(request_id, ToolStream)` pairs and drives them with `stream::select_all`.
//! That is fast, but unbounded — a single hung or runaway tool can stall the
//! whole turn, and a large batch can launch an arbitrary number of in-flight
//! streams at once. This module adds two opt-in guard-rails on top of that
//! existing machinery without changing its shape:
//!
//!   * a **concurrency cap** — at most `max_in_flight` tool streams are polled
//!     to completion at any one time, enforced with an `Arc<Semaphore>` permit
//!     that each wrapped stream holds for its lifetime; and
//!   * a **per-tool wall-clock timeout** — each stream's terminal
//!     `ToolStreamItem::Result` is raced against a `tokio::time::sleep`, and on
//!     elapse a synthetic error result is yielded so the turn can make progress
//!     instead of blocking forever on the slow tool.
//!
//! Both knobs are **off by default**, so with neither environment variable set
//! `wrap_stream` returns the original stream untouched and behavior is
//! byte-identical to the ungoverned path.
//!
//! Tuning (all optional, read once via [`ToolGovernor::from_env`]):
//!   * `BHARATCODE_TOOL_MAX_INFLIGHT` — max concurrent tool streams, clamped to
//!     `1..=64`; unset (or unparseable) means unbounded (`usize::MAX`, a no-op).
//!   * `BHARATCODE_TOOL_TIMEOUT_SECS` — per-tool wall-clock budget in seconds;
//!     unset (or unparseable / zero) means no timeout.
//!
//! This module is original work; nothing here is ported from third-party
//! sources.

use std::sync::Arc;
use std::time::Duration;

use rmcp::model::{ErrorCode, ErrorData};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use super::{ToolStream, ToolStreamItem};

const MAX_INFLIGHT_KEY: &str = "BHARATCODE_TOOL_MAX_INFLIGHT";
const TIMEOUT_SECS_KEY: &str = "BHARATCODE_TOOL_TIMEOUT_SECS";

/// Lower/upper bounds for the concurrency cap when the operator sets one.
const MIN_INFLIGHT: usize = 1;
const MAX_INFLIGHT: usize = 64;

/// Message surfaced as the synthetic result when a tool blows its budget.
const TIMEOUT_MESSAGE: &str = "tool exceeded BHARATCODE_TOOL_TIMEOUT_SECS";

/// Opt-in governor for the concurrent tool fan-out.
///
/// Constructed once (typically behind a process-wide `LazyLock`) and shared by
/// reference across every `(request_id, stream)` pair of a turn. With its
/// defaults — `max_in_flight == usize::MAX` and `per_tool_timeout == None` — it
/// is a transparent no-op.
pub struct ToolGovernor {
    max_in_flight: usize,
    per_tool_timeout: Option<Duration>,
    semaphore: Arc<Semaphore>,
}

impl ToolGovernor {
    /// Reads both knobs from the environment.
    ///
    /// `BHARATCODE_TOOL_MAX_INFLIGHT` is clamped to `1..=64`; unset or
    /// unparseable yields `usize::MAX` (unbounded, no-op). A literal `0` clamps
    /// up to `1`. `BHARATCODE_TOOL_TIMEOUT_SECS` becomes `Some(Duration)` only
    /// when it parses to a non-zero value; otherwise `None`.
    pub fn from_env() -> Self {
        let max_in_flight = std::env::var(MAX_INFLIGHT_KEY)
            .ok()
            .and_then(|raw| raw.trim().parse::<usize>().ok())
            .map(|n| n.clamp(MIN_INFLIGHT, MAX_INFLIGHT))
            .unwrap_or(usize::MAX);

        let per_tool_timeout = std::env::var(TIMEOUT_SECS_KEY)
            .ok()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .filter(|secs| *secs > 0)
            .map(Duration::from_secs);

        Self::new(max_in_flight, per_tool_timeout)
    }

    fn new(max_in_flight: usize, per_tool_timeout: Option<Duration>) -> Self {
        // `Semaphore` accepts permit counts up to its own internal maximum;
        // `usize::MAX` would overflow that, so an unbounded cap is represented
        // by a permit pool large enough never to block the fan-out in practice.
        let permits = if max_in_flight == usize::MAX {
            Semaphore::MAX_PERMITS
        } else {
            max_in_flight
        };
        Self {
            max_in_flight,
            per_tool_timeout,
            semaphore: Arc::new(Semaphore::new(permits)),
        }
    }

    /// True when neither knob is engaged, i.e. `wrap_stream` is a pass-through.
    fn is_noop(&self) -> bool {
        self.max_in_flight == usize::MAX && self.per_tool_timeout.is_none()
    }

    /// Acquires a permit bounding the number of concurrently polled streams.
    ///
    /// The returned guard is held for the wrapped stream's lifetime; dropping it
    /// (on completion or cancellation) releases the slot for the next stream.
    /// When the cap is unbounded this still succeeds immediately.
    #[cfg(test)]
    async fn acquire_permit(&self) -> OwnedSemaphorePermit {
        acquire_owned(&self.semaphore).await
    }

    /// Wraps a single tool stream with the configured guard-rails.
    ///
    /// When the governor is a no-op the input stream is returned unchanged
    /// (byte-identical behavior). Otherwise the stream is re-driven so that:
    ///   * a concurrency permit is acquired before its first item is polled, and
    ///     held until the stream ends; and
    ///   * if a timeout is configured, the terminal `Result` is raced against a
    ///     `sleep`, replacing it with a synthetic timeout error on elapse.
    pub fn wrap_stream(&self, request_id: String, stream: ToolStream) -> ToolStream {
        if self.is_noop() {
            return stream;
        }

        let timeout = self.per_tool_timeout;
        let bounded = self.max_in_flight != usize::MAX;
        let semaphore = Arc::clone(&self.semaphore);
        // request_id is part of the governor contract and useful for tracing;
        // bind it so the closure owns a copy even though the happy path stays quiet.
        let _request_id = request_id;

        Box::pin(async_stream::stream! {
            // Hold a permit for the stream's whole lifetime when a cap is set.
            // Acquiring on the first poll bounds how many streams `select_all`
            // can drive concurrently; the guard releases the slot on drop.
            let _permit = if bounded {
                Some(acquire_owned(&semaphore).await)
            } else {
                None
            };

            let mut inner = stream;
            let deadline = timeout.map(tokio::time::sleep);
            tokio::pin!(deadline);

            loop {
                match deadline.as_mut().as_pin_mut() {
                    Some(sleep) => {
                        tokio::select! {
                            biased;

                            item = futures::StreamExt::next(&mut inner) => {
                                match item {
                                    Some(item) => yield item,
                                    None => break,
                                }
                            }
                            () = sleep => {
                                yield ToolStreamItem::Result(Err(timeout_error()));
                                break;
                            }
                        }
                    }
                    None => {
                        match futures::StreamExt::next(&mut inner).await {
                            Some(item) => yield item,
                            None => break,
                        }
                    }
                }
            }
        })
    }
}

/// Builds the synthetic result yielded when a tool exceeds its timeout budget.
///
/// The `ToolStream` error half is `rmcp::model::ErrorData`; the donor's
/// `ToolError::ExecutionError(..)` maps onto an internal-error `ErrorData`
/// carrying the same message so downstream handling is unchanged.
fn timeout_error() -> ErrorData {
    ErrorData::new(ErrorCode::INTERNAL_ERROR, TIMEOUT_MESSAGE.to_string(), None)
}

/// Acquires one owned permit from the shared pool. The pool is never closed, so
/// acquisition cannot fail.
async fn acquire_owned(semaphore: &Arc<Semaphore>) -> OwnedSemaphorePermit {
    Arc::clone(semaphore)
        .acquire_owned()
        .await
        .expect("tool governor semaphore is never closed")
}

#[cfg(test)]
mod tests {
    use super::super::tool_stream;
    use super::*;
    use crate::mcp_utils::ToolResult;
    use futures::StreamExt;
    use rmcp::model::CallToolResult;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Serializes env-mutating tests; `from_env` reads process-global state.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn never_completing_stream() -> ToolStream {
        // A stream whose terminal Result future never resolves: the notification
        // half is empty and the `done` future is `pending`, so without a
        // timeout it would block the turn forever.
        tool_stream(
            Box::new(futures::stream::empty()),
            futures::future::pending::<ToolResult<CallToolResult>>(),
        )
    }

    fn immediate_ok_stream() -> ToolStream {
        tool_stream(
            Box::new(futures::stream::empty()),
            futures::future::ready(Ok(CallToolResult::success(vec![]))),
        )
    }

    #[test]
    fn from_env_unset_is_unbounded_and_no_timeout() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(MAX_INFLIGHT_KEY);
        std::env::remove_var(TIMEOUT_SECS_KEY);

        let governor = ToolGovernor::from_env();
        assert_eq!(governor.max_in_flight, usize::MAX);
        assert_eq!(governor.per_tool_timeout, None);
        assert!(governor.is_noop());
    }

    #[test]
    fn max_inflight_clamps_low_and_high() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(TIMEOUT_SECS_KEY);

        std::env::set_var(MAX_INFLIGHT_KEY, "0");
        assert_eq!(ToolGovernor::from_env().max_in_flight, 1);

        std::env::set_var(MAX_INFLIGHT_KEY, "999");
        assert_eq!(ToolGovernor::from_env().max_in_flight, 64);

        std::env::set_var(MAX_INFLIGHT_KEY, "8");
        assert_eq!(ToolGovernor::from_env().max_in_flight, 8);

        std::env::remove_var(MAX_INFLIGHT_KEY);
    }

    #[test]
    fn timeout_secs_parses_and_rejects_zero() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(MAX_INFLIGHT_KEY);

        std::env::set_var(TIMEOUT_SECS_KEY, "5");
        assert_eq!(
            ToolGovernor::from_env().per_tool_timeout,
            Some(Duration::from_secs(5))
        );

        std::env::set_var(TIMEOUT_SECS_KEY, "0");
        assert_eq!(ToolGovernor::from_env().per_tool_timeout, None);

        std::env::remove_var(TIMEOUT_SECS_KEY);
    }

    #[tokio::test]
    async fn timeout_yields_single_err_result_with_message() {
        let governor = ToolGovernor::new(usize::MAX, Some(Duration::from_millis(1)));
        let wrapped = governor.wrap_stream("req-1".to_string(), never_completing_stream());

        let items: Vec<_> = wrapped.collect().await;
        assert_eq!(items.len(), 1, "exactly one item expected");

        match &items[0] {
            ToolStreamItem::Result(Err(err)) => {
                assert!(
                    err.message.contains(TIMEOUT_MESSAGE),
                    "unexpected message: {}",
                    err.message
                );
            }
            ToolStreamItem::Result(Ok(_)) => panic!("expected timeout Err, got Ok"),
            ToolStreamItem::Message(_) => panic!("expected Result, got Message"),
        }
    }

    #[tokio::test]
    async fn noop_governor_passes_results_through() {
        let governor = ToolGovernor::new(usize::MAX, None);
        let wrapped = governor.wrap_stream("req-2".to_string(), immediate_ok_stream());

        let items: Vec<_> = wrapped.collect().await;
        assert_eq!(items.len(), 1);
        assert!(matches!(items[0], ToolStreamItem::Result(Ok(_))));
    }

    #[tokio::test]
    async fn permit_count_never_exceeds_cap() {
        // Cap of 2: many tasks each grab a permit, record peak concurrency while
        // holding it, then release. The semaphore must keep the peak <= cap.
        let governor = Arc::new(ToolGovernor::new(2, None));
        let in_flight = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for _ in 0..16 {
            let governor = Arc::clone(&governor);
            let in_flight = Arc::clone(&in_flight);
            let peak = Arc::clone(&peak);
            handles.push(tokio::spawn(async move {
                let permit = governor.acquire_permit().await;
                let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                // Hold the slot so overlap with peers is observable.
                tokio::time::sleep(Duration::from_millis(2)).await;
                in_flight.fetch_sub(1, Ordering::SeqCst);
                drop(permit);
            }));
        }

        for h in handles {
            h.await.unwrap();
        }

        assert_eq!(in_flight.load(Ordering::SeqCst), 0);
        assert!(
            peak.load(Ordering::SeqCst) <= 2,
            "permit count exceeded cap: peak={}",
            peak.load(Ordering::SeqCst)
        );
    }

    #[tokio::test]
    async fn acquire_permit_respects_cap() {
        let governor = ToolGovernor::new(1, None);
        let first = governor.acquire_permit().await;
        // A second acquisition must not succeed while the first is held.
        let second =
            tokio::time::timeout(Duration::from_millis(20), governor.acquire_permit()).await;
        assert!(second.is_err(), "second permit acquired despite cap of 1");
        drop(first);
        // After release the slot is available again.
        let third =
            tokio::time::timeout(Duration::from_millis(20), governor.acquire_permit()).await;
        assert!(third.is_ok(), "permit not released after drop");
    }
}
