//! Cancellation + stall-deadline guard for the provider streaming path.
//!
//! Two things can wedge a turn forever, and neither is covered by the HTTP
//! client timeout of an individual provider:
//!
//! * `Provider::stream()` never returns (handshake / subprocess / proxy hang), and
//! * the returned stream never yields another item (half-open connection, a
//!   provider that stops writing mid-body, an ACP subprocess that goes quiet).
//!
//! In both cases the agent loop is parked inside an `await` that does not
//! observe the turn's [`CancellationToken`], so pressing Esc/Ctrl-C does not
//! end the turn: the cancellation checks in the agent loop only run *between*
//! stream items, which never arrive.
//!
//! [`StreamGuard`] closes that hole. It races the stream-creation future and
//! **every** `next()` poll against the turn's cancellation token and a
//! wall-clock stall budget, so an await can only park for as long as the
//! provider is making progress. Dropping the underlying stream on abort also
//! drops the response body, which is what actually releases the connection for
//! the reqwest-backed providers.
//!
//! ## Semantics
//!
//! The budget is a **stall** (idle) budget, not a turn budget: it is armed
//! fresh for each await, so a long-but-progressing stream is never cut off,
//! while a stream that goes silent for the budget is aborted. It defaults to
//! [`crate::providers::base::DEFAULT_PROVIDER_TIMEOUT_SECS`] (the same
//! wall-clock allowance providers give a single HTTP call) and is configurable
//! with `BHARATCODE_PROVIDER_DEADLINE_SECS`; `0` disables it, leaving
//! cancellation as the only abort source.
//!
//! ## Abort errors are not retryable
//!
//! Cancellation and stall aborts are terminal for the turn: retrying or
//! falling back to another model would just resend a request the user cancelled,
//! or re-hang against a provider that has already proven unresponsive. Ideally
//! these would be their own `ProviderError` variants, but `ProviderError` is
//! public API of the `bharatcode-providers` crate and adding variants is a
//! breaking change for every downstream matcher. Instead they are tagged
//! `RequestFailed` messages carrying a stable marker prefix, and are recognised
//! with [`is_cancelled`] / [`is_deadline`] / [`is_abort`] rather than by string
//! matching at call sites.
//!
//! ## Residual limitation
//!
//! Aborting stops *us* from waiting; it cannot force every provider to abort
//! its work server-side. Dropping a reqwest response body closes the
//! connection, but a subprocess-backed provider (ACP/CLI) may keep generating
//! until its own process exits, and any tokens already produced upstream may
//! still be billed. Guaranteeing per-request HTTP abort would require threading
//! a cancellation token into the `Provider` trait itself.

use std::future::Future;
use std::time::Duration;

use bharatcode_providers::errors::ProviderError;
use futures::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::conversation::message::{Message, MessageContent};
use crate::providers::base::{MessageStream, ProviderUsage, DEFAULT_PROVIDER_TIMEOUT_SECS};

/// Environment variable holding the per-await stall budget, in whole seconds.
///
/// Unset / blank / non-numeric falls back to [`DEFAULT_PROVIDER_TIMEOUT_SECS`];
/// `0` disables the budget.
pub const ENV_VAR: &str = "BHARATCODE_PROVIDER_DEADLINE_SECS";

/// Largest budget we honour, in seconds (24h), so a fat-fingered value cannot
/// pin a request open effectively forever.
const MAX_DEADLINE_SECS: u64 = 24 * 60 * 60;

/// Marker prefix identifying an abort caused by the turn's cancellation token.
const CANCELLED_MARKER: &str = "bharatcode.abort.cancelled";

/// Marker prefix identifying an abort caused by the stall budget.
const DEADLINE_MARKER: &str = "bharatcode.abort.deadline";

/// Resolve the configured stall budget. `None` means "no budget" (explicitly
/// disabled with `0`).
pub fn request_deadline() -> Option<Duration> {
    match std::env::var(ENV_VAR) {
        Ok(raw) => parse_deadline(&raw),
        Err(_) => Some(Duration::from_secs(DEFAULT_PROVIDER_TIMEOUT_SECS)),
    }
}

/// Pure parser over an explicit value (testable without touching the env).
///
/// `0` disables the budget; blank / non-numeric values fall back to the default
/// rather than silently disabling the guard. Positive values clamp to
/// [`MAX_DEADLINE_SECS`].
pub fn parse_deadline(raw: &str) -> Option<Duration> {
    let raw = raw.trim();
    let Ok(secs) = raw.parse::<u64>() else {
        return Some(Duration::from_secs(DEFAULT_PROVIDER_TIMEOUT_SECS));
    };
    if secs == 0 {
        return None;
    }
    Some(Duration::from_secs(secs.min(MAX_DEADLINE_SECS)))
}

/// True when `error` is an abort raised because the turn was cancelled.
pub fn is_cancelled(error: &ProviderError) -> bool {
    marked_with(error, CANCELLED_MARKER)
}

/// True when `error` is an abort raised because the provider stalled past the
/// wall-clock budget.
pub fn is_deadline(error: &ProviderError) -> bool {
    marked_with(error, DEADLINE_MARKER)
}

/// True for any guard-raised abort. Such errors must never be retried, fallen
/// back from, or masked by a later provider error.
pub fn is_abort(error: &ProviderError) -> bool {
    is_cancelled(error) || is_deadline(error)
}

fn marked_with(error: &ProviderError, marker: &str) -> bool {
    matches!(error, ProviderError::RequestFailed(msg) if msg.starts_with(marker))
}

fn cancelled_error() -> ProviderError {
    ProviderError::RequestFailed(format!("{CANCELLED_MARKER}: request cancelled"))
}

fn deadline_error(budget: Duration) -> ProviderError {
    ProviderError::RequestFailed(format!(
        "{DEADLINE_MARKER}: provider made no progress for {}s (adjust or disable with {ENV_VAR})",
        budget.as_secs()
    ))
}

/// Races provider awaits against cancellation and a stall budget.
///
/// Cloneable and cheap: a `StreamGuard` is just the budget plus an optional
/// token handle, so it can be handed to the streaming path and moved into the
/// stream it wraps.
#[derive(Clone)]
pub struct StreamGuard {
    deadline: Option<Duration>,
    cancel: Option<CancellationToken>,
}

impl StreamGuard {
    /// Guard with the env-configured budget (see [`ENV_VAR`]) and the turn's
    /// cancellation token. This is what the agent loop uses.
    pub fn from_env(cancel: Option<CancellationToken>) -> Self {
        Self {
            deadline: request_deadline(),
            cancel,
        }
    }

    /// Guard with an explicit budget, bypassing the environment.
    pub fn new(deadline: Option<Duration>, cancel: Option<CancellationToken>) -> Self {
        Self { deadline, cancel }
    }

    fn is_inert(&self) -> bool {
        self.deadline.is_none() && self.cancel.is_none()
    }

    /// Abort immediately if the turn is already cancelled.
    ///
    /// Used to bail out of multi-step work (e.g. walking a fallback chain)
    /// between awaits, where there is no single future to race.
    pub fn check(&self) -> Result<(), ProviderError> {
        match &self.cancel {
            Some(token) if token.is_cancelled() => Err(cancelled_error()),
            _ => Ok(()),
        }
    }

    /// Await a provider request, racing it against cancellation and the stall
    /// budget. Whichever fires first wins; the future's own result is returned
    /// untouched when it completes.
    pub async fn guard<F, T>(&self, fut: F) -> Result<T, ProviderError>
    where
        F: Future<Output = Result<T, ProviderError>>,
    {
        match self.race(fut).await {
            Ok(result) => result,
            Err(abort) => Err(abort),
        }
    }

    /// Wrap a provider stream so every `next()` poll is raced against
    /// cancellation and the stall budget.
    ///
    /// On abort the wrapper yields the abort error and ends the stream, and
    /// drops the inner stream — releasing the underlying connection.
    pub fn guard_stream(&self, inner: MessageStream) -> MessageStream {
        if self.is_inert() {
            return inner;
        }

        let guard = self.clone();
        Box::pin(async_stream::stream! {
            let mut inner = inner;
            let mut waiting_for_user = false;
            loop {
                let next = if waiting_for_user {
                    guard.race_cancellation(inner.next()).await
                } else {
                    guard.race(inner.next()).await
                };
                match next {
                    Ok(Some(item)) => {
                        waiting_for_user = item_requests_user_action(&item);
                        yield item;
                    }
                    Ok(None) => break,
                    Err(abort) => {
                        yield Err(abort);
                        break;
                    }
                }
            }
        })
    }

    /// Race an arbitrary future against cancellation and the budget.
    ///
    /// `Ok(output)` when the future wins, `Err(abort)` when it is pre-empted.
    async fn race<F, T>(&self, fut: F) -> Result<T, ProviderError>
    where
        F: Future<Output = T>,
    {
        if self.is_inert() {
            return Ok(fut.await);
        }

        tokio::pin!(fut);

        match (self.deadline, &self.cancel) {
            (Some(budget), Some(token)) => {
                tokio::select! {
                    biased;
                    () = token.cancelled() => Err(cancelled_error()),
                    out = &mut fut => Ok(out),
                    () = tokio::time::sleep(budget) => Err(deadline_error(budget)),
                }
            }
            (Some(budget), None) => {
                tokio::select! {
                    biased;
                    out = &mut fut => Ok(out),
                    () = tokio::time::sleep(budget) => Err(deadline_error(budget)),
                }
            }
            (None, Some(token)) => {
                tokio::select! {
                    biased;
                    () = token.cancelled() => Err(cancelled_error()),
                    out = &mut fut => Ok(out),
                }
            }
            (None, None) => Ok(fut.await),
        }
    }

    async fn race_cancellation<F, T>(&self, fut: F) -> Result<T, ProviderError>
    where
        F: Future<Output = T>,
    {
        match &self.cancel {
            Some(token) => {
                tokio::select! {
                    biased;
                    () = token.cancelled() => Err(cancelled_error()),
                    out = fut => Ok(out),
                }
            }
            None => Ok(fut.await),
        }
    }
}

fn item_requests_user_action(
    item: &Result<(Option<Message>, Option<ProviderUsage>), ProviderError>,
) -> bool {
    item.as_ref()
        .ok()
        .and_then(|(message, _)| message.as_ref())
        .is_some_and(|message| {
            message
                .content
                .iter()
                .any(|content| matches!(content, MessageContent::ActionRequired(_)))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bharatcode_providers::conversation::token_usage::{ProviderUsage, Usage};
    use std::future::pending;

    /// Serialize tests that mutate the shared process environment.
    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// A budget small enough to keep tests fast. Every test races it against a
    /// future that can *never* complete, so the outcome does not depend on
    /// timing.
    const TINY: Duration = Duration::from_millis(20);

    fn chunk(text: &str) -> Result<(Option<Message>, Option<ProviderUsage>), ProviderError> {
        Ok((
            Some(Message::assistant().with_text(text)),
            Some(ProviderUsage::new("fake".to_string(), Usage::default())),
        ))
    }

    /// A stream that yields `text` once and then never yields again.
    fn stalling_stream(text: &str) -> MessageStream {
        let first = chunk(text);
        Box::pin(async_stream::stream! {
            yield first;
            let never: Result<(Option<Message>, Option<ProviderUsage>), ProviderError> =
                pending().await;
            yield never;
        })
    }

    fn approval_stalling_stream() -> MessageStream {
        let action = Ok((
            Some(Message::assistant().with_action_required(
                "approval-id",
                "developer__shell".to_string(),
                Default::default(),
                None,
            )),
            None,
        ));
        Box::pin(async_stream::stream! {
            yield action;
            let never: Result<(Option<Message>, Option<ProviderUsage>), ProviderError> =
                pending().await;
            yield never;
        })
    }

    #[test]
    fn deadline_defaults_on_when_env_unset() {
        let _guard = env_guard();
        std::env::remove_var(ENV_VAR);
        assert_eq!(
            request_deadline(),
            Some(Duration::from_secs(DEFAULT_PROVIDER_TIMEOUT_SECS))
        );
    }

    #[test]
    fn deadline_honors_env() {
        let _guard = env_guard();
        std::env::set_var(ENV_VAR, "30");
        assert_eq!(request_deadline(), Some(Duration::from_secs(30)));
        std::env::remove_var(ENV_VAR);
    }

    #[test]
    fn zero_disables_and_junk_falls_back_to_default() {
        let default = Some(Duration::from_secs(DEFAULT_PROVIDER_TIMEOUT_SECS));
        assert_eq!(parse_deadline("0"), None);
        assert_eq!(parse_deadline(""), default);
        assert_eq!(parse_deadline("   "), default);
        assert_eq!(parse_deadline("abc"), default);
        assert_eq!(parse_deadline("-5"), default);
    }

    #[test]
    fn parse_deadline_clamps_absurd_values() {
        assert_eq!(
            parse_deadline("999999999999"),
            Some(Duration::from_secs(MAX_DEADLINE_SECS))
        );
        assert_eq!(
            parse_deadline(&u64::MAX.to_string()),
            Some(Duration::from_secs(MAX_DEADLINE_SECS))
        );
    }

    #[test]
    fn abort_errors_are_classified_and_other_errors_are_not() {
        assert!(is_cancelled(&cancelled_error()));
        assert!(is_abort(&cancelled_error()));
        assert!(!is_deadline(&cancelled_error()));

        assert!(is_deadline(&deadline_error(TINY)));
        assert!(is_abort(&deadline_error(TINY)));
        assert!(!is_cancelled(&deadline_error(TINY)));

        // A provider's own failures must never be mistaken for an abort, even
        // when they mention cancellation or timeouts.
        assert!(!is_abort(&ProviderError::RequestFailed(
            "upstream cancelled the request and timed out".to_string()
        )));
        assert!(!is_abort(&ProviderError::ServerError("overloaded".into())));
    }

    #[tokio::test]
    async fn completed_future_passes_through_untouched() {
        let guard = StreamGuard::new(
            Some(Duration::from_secs(3600)),
            Some(CancellationToken::new()),
        );
        assert_eq!(
            guard.guard(async { Ok::<u32, ProviderError>(7) }).await,
            Ok(7)
        );

        let err = guard
            .guard(async { Err::<u32, _>(ProviderError::Authentication("nope".to_string())) })
            .await;
        assert_eq!(err, Err(ProviderError::Authentication("nope".to_string())));
    }

    #[tokio::test]
    async fn never_returning_request_hits_the_stall_budget() {
        let guard = StreamGuard::new(Some(TINY), None);
        let err = guard
            .guard(pending::<Result<u32, ProviderError>>())
            .await
            .expect_err("a never-returning request must abort");
        assert!(is_deadline(&err), "{err}");
    }

    #[tokio::test]
    async fn cancellation_beats_a_never_returning_request() {
        let token = CancellationToken::new();
        token.cancel();
        let guard = StreamGuard::new(Some(Duration::from_secs(3600)), Some(token));

        let err = guard
            .guard(pending::<Result<u32, ProviderError>>())
            .await
            .expect_err("a cancelled turn must abort");
        assert!(is_cancelled(&err), "{err}");
        assert_eq!(guard.check(), Err(cancelled_error()));
    }

    #[tokio::test]
    async fn stalled_stream_poll_aborts_after_the_budget() {
        let guard = StreamGuard::new(Some(TINY), None);
        let mut stream = guard.guard_stream(stalling_stream("hello"));

        let first = stream.next().await.expect("first chunk").expect("ok chunk");
        assert_eq!(first.0.unwrap().as_concat_text(), "hello");

        let err = stream
            .next()
            .await
            .expect("guard must yield an abort, not park forever")
            .expect_err("stalled poll must be an error");
        assert!(is_deadline(&err), "{err}");
        assert!(stream.next().await.is_none(), "stream must end after abort");
    }

    #[tokio::test]
    async fn cancelling_mid_stream_aborts_the_next_poll() {
        let token = CancellationToken::new();
        let guard = StreamGuard::new(Some(Duration::from_secs(3600)), Some(token.clone()));
        let mut stream = guard.guard_stream(stalling_stream("hello"));

        stream.next().await.expect("first chunk").expect("ok chunk");
        token.cancel();

        let err = stream
            .next()
            .await
            .expect("guard must yield an abort, not park forever")
            .expect_err("cancelled poll must be an error");
        assert!(is_cancelled(&err), "{err}");
        assert!(stream.next().await.is_none(), "stream must end after abort");
    }

    #[tokio::test]
    async fn approval_wait_pauses_deadline_but_remains_cancellable() {
        let token = CancellationToken::new();
        let guard = StreamGuard::new(Some(TINY), Some(token.clone()));
        let mut stream = guard.guard_stream(approval_stalling_stream());

        stream
            .next()
            .await
            .expect("action required")
            .expect("ok chunk");
        assert!(
            tokio::time::timeout(TINY * 3, stream.next()).await.is_err(),
            "a human approval wait must outlive the provider stall budget"
        );

        token.cancel();
        let err = stream
            .next()
            .await
            .expect("guard must yield cancellation")
            .expect_err("cancelled approval wait must be an error");
        assert!(is_cancelled(&err), "{err}");
    }

    #[tokio::test]
    async fn inert_guard_passes_the_stream_through() {
        let guard = StreamGuard::new(None, None);
        let mut stream = guard.guard_stream(Box::pin(futures::stream::iter(vec![chunk("a")])));

        let first = stream.next().await.expect("chunk").expect("ok chunk");
        assert_eq!(first.0.unwrap().as_concat_text(), "a");
        assert!(stream.next().await.is_none());
    }
}
