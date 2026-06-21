//! Opt-in provider request deadline + graceful cancellation wrapper.
//!
//! Some providers (or flaky networks behind them) can leave a request future
//! hanging indefinitely with no error ever surfacing. This module offers a
//! small, reusable guard that races *any* provider request future against two
//! things:
//!
//! * a wall-clock budget (`BHARATCODE_PROVIDER_DEADLINE_SECS`), and
//! * an optional [`CancellationToken`].
//!
//! Whichever fires first wins, and the caller gets a clean
//! [`ProviderError::RequestFailed`] instead of a stuck task. This keeps the
//! streaming / fallback path and any other provider call site responsive.
//!
//! The feature is **default-OFF**: when `BHARATCODE_PROVIDER_DEADLINE_SECS` is
//! unset (and no cancellation token is supplied) the wrapper simply awaits the
//! underlying future unchanged, so there is zero added overhead and default
//! behaviour is identical to today.
//!
//! Original BharatCode work; not ported from any third party.

use std::future::Future;
use std::time::Duration;

use bharatcode_providers::errors::ProviderError;
use tokio_util::sync::CancellationToken;

/// Environment variable holding the per-request deadline, in whole seconds.
///
/// Unset / blank / non-numeric / `0` leaves the deadline disabled (no budget).
pub const ENV_VAR: &str = "BHARATCODE_PROVIDER_DEADLINE_SECS";

/// Largest deadline we honour, in seconds (24h). Absurd values clamp here so a
/// fat-fingered configuration cannot overflow [`Duration`] arithmetic or pin a
/// request open effectively forever.
const MAX_DEADLINE_SECS: u64 = 24 * 60 * 60;

/// Resolve the configured per-request deadline, if any.
///
/// Reads [`ENV_VAR`] (env-first). Returns `None` when the variable is unset,
/// blank, non-numeric, or `0` — keeping the feature inert by default. Positive
/// values are clamped to [`MAX_DEADLINE_SECS`] so absurd inputs stay safe.
pub fn request_deadline() -> Option<Duration> {
    let raw = std::env::var(ENV_VAR).ok()?;
    parse_deadline(&raw)
}

/// Pure parser over an explicit value (testable without touching the env).
///
/// Mirrors [`request_deadline`]: blank / non-numeric / `0` => `None`; positive
/// seconds are clamped to [`MAX_DEADLINE_SECS`].
pub fn parse_deadline(raw: &str) -> Option<Duration> {
    let secs: u64 = raw.trim().parse().ok()?;
    if secs == 0 {
        return None;
    }
    Some(Duration::from_secs(secs.min(MAX_DEADLINE_SECS)))
}

/// Wrap a provider request future with the optional deadline and an optional
/// cancellation token.
///
/// Behaviour:
///
/// * **No deadline configured and no token** — the future is awaited unchanged
///   (zero overhead, default-OFF behaviour).
/// * **A token is supplied but no deadline** — the future races only the
///   token; cancellation yields
///   `ProviderError::RequestFailed("request cancelled")`.
/// * **A deadline is configured** — the future races the wall-clock budget and
///   (if present) the token. Exceeding the budget yields
///   `ProviderError::RequestFailed("provider request exceeded \
///   BHARATCODE_PROVIDER_DEADLINE_SECS")`; cancellation yields the cancelled
///   error above.
///
/// In every case a future that completes first returns its own
/// `Result<T, ProviderError>` untouched.
pub async fn with_deadline<F, T>(
    fut: F,
    cancel: Option<&CancellationToken>,
) -> Result<T, ProviderError>
where
    F: Future<Output = Result<T, ProviderError>>,
{
    with_deadline_for(fut, request_deadline(), cancel).await
}

/// Race a future against an explicit `deadline` and optional `cancel` token.
///
/// This is the engine behind [`with_deadline`], split out so the deadline can
/// be supplied directly (instead of via the environment). [`with_deadline`]
/// resolves the deadline from [`request_deadline`] and delegates here.
async fn with_deadline_for<F, T>(
    fut: F,
    deadline: Option<Duration>,
    cancel: Option<&CancellationToken>,
) -> Result<T, ProviderError>
where
    F: Future<Output = Result<T, ProviderError>>,
{
    // Fast path: nothing to race against — await unchanged, zero overhead.
    if deadline.is_none() && cancel.is_none() {
        return fut.await;
    }

    tokio::pin!(fut);

    match (deadline, cancel) {
        (Some(budget), Some(token)) => {
            tokio::select! {
                biased;
                () = token.cancelled() => Err(cancelled_error()),
                () = tokio::time::sleep(budget) => Err(deadline_error()),
                res = &mut fut => res,
            }
        }
        (Some(budget), None) => {
            tokio::select! {
                biased;
                () = tokio::time::sleep(budget) => Err(deadline_error()),
                res = &mut fut => res,
            }
        }
        (None, Some(token)) => {
            tokio::select! {
                biased;
                () = token.cancelled() => Err(cancelled_error()),
                res = &mut fut => res,
            }
        }
        // Covered by the fast path above; kept for exhaustiveness.
        (None, None) => fut.await,
    }
}

/// Error returned when the wall-clock budget is exceeded.
fn deadline_error() -> ProviderError {
    ProviderError::RequestFailed(
        "provider request exceeded BHARATCODE_PROVIDER_DEADLINE_SECS".to_string(),
    )
}

/// Error returned when the supplied cancellation token fires.
fn cancelled_error() -> ProviderError {
    ProviderError::RequestFailed("request cancelled".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::pending;

    /// Serialize tests that mutate the shared process environment.
    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|p| p.into_inner())
    }

    #[test]
    fn request_deadline_inert_when_env_unset() {
        let _guard = env_guard();
        std::env::remove_var(ENV_VAR);
        assert_eq!(request_deadline(), None);
    }

    #[test]
    fn request_deadline_honors_env() {
        let _guard = env_guard();
        std::env::set_var(ENV_VAR, "30");
        assert_eq!(request_deadline(), Some(Duration::from_secs(30)));
        std::env::remove_var(ENV_VAR);
    }

    #[test]
    fn parse_deadline_rejects_blank_zero_and_junk() {
        assert_eq!(parse_deadline(""), None);
        assert_eq!(parse_deadline("   "), None);
        assert_eq!(parse_deadline("0"), None);
        assert_eq!(parse_deadline("abc"), None);
        assert_eq!(parse_deadline("-5"), None);
    }

    #[test]
    fn parse_deadline_clamps_absurd_values() {
        // Far larger than the cap clamps down to MAX_DEADLINE_SECS.
        assert_eq!(
            parse_deadline("999999999999"),
            Some(Duration::from_secs(MAX_DEADLINE_SECS))
        );
        // u64::MAX must not panic / overflow; it also clamps.
        assert_eq!(
            parse_deadline(&u64::MAX.to_string()),
            Some(Duration::from_secs(MAX_DEADLINE_SECS))
        );
    }

    #[tokio::test]
    async fn ready_ok_future_returns_unchanged_without_deadline() {
        let _guard = env_guard();
        std::env::remove_var(ENV_VAR);
        let out = with_deadline(async { Ok::<u32, ProviderError>(7) }, None).await;
        assert_eq!(out, Ok(7));
    }

    #[tokio::test]
    async fn ready_err_future_returns_unchanged_without_deadline() {
        let _guard = env_guard();
        std::env::remove_var(ENV_VAR);
        let out: Result<u32, ProviderError> = with_deadline(
            async { Err(ProviderError::Authentication("nope".to_string())) },
            None,
        )
        .await;
        assert_eq!(out, Err(ProviderError::Authentication("nope".to_string())));
    }

    #[tokio::test]
    async fn never_completing_future_hits_deadline() {
        // A future that never completes, raced against a tiny 1ms budget,
        // resolves to the deadline error (real time, no test-util needed).
        let fut = pending::<Result<u32, ProviderError>>();
        let out = with_deadline_for(fut, Some(Duration::from_millis(1)), None).await;
        assert_eq!(
            out,
            Err(ProviderError::RequestFailed(
                "provider request exceeded BHARATCODE_PROVIDER_DEADLINE_SECS".to_string()
            ))
        );
    }

    #[tokio::test]
    async fn with_deadline_reads_env_budget() {
        // The public entry point honours the env-configured budget end-to-end:
        // a 1s budget against a never-completing future still yields the
        // deadline error (kept short to stay fast).
        let _guard = env_guard();
        std::env::set_var(ENV_VAR, "1");
        let fut = pending::<Result<u32, ProviderError>>();
        let out = with_deadline(fut, None).await;
        std::env::remove_var(ENV_VAR);
        assert_eq!(
            out,
            Err(ProviderError::RequestFailed(
                "provider request exceeded BHARATCODE_PROVIDER_DEADLINE_SECS".to_string()
            ))
        );
    }

    #[tokio::test]
    async fn pre_cancelled_token_short_circuits() {
        let _guard = env_guard();
        std::env::remove_var(ENV_VAR);
        let token = CancellationToken::new();
        token.cancel();
        // Even a future that never completes must short-circuit to cancelled.
        let fut = pending::<Result<u32, ProviderError>>();
        let out = with_deadline(fut, Some(&token)).await;
        assert_eq!(
            out,
            Err(ProviderError::RequestFailed(
                "request cancelled".to_string()
            ))
        );
    }

    #[tokio::test]
    async fn pre_cancelled_token_wins_over_deadline() {
        let _guard = env_guard();
        std::env::set_var(ENV_VAR, "3600");
        let token = CancellationToken::new();
        token.cancel();
        let fut = pending::<Result<u32, ProviderError>>();
        let out = with_deadline(fut, Some(&token)).await;
        std::env::remove_var(ENV_VAR);
        assert_eq!(
            out,
            Err(ProviderError::RequestFailed(
                "request cancelled".to_string()
            ))
        );
    }

    #[tokio::test]
    async fn ready_future_wins_over_live_token_and_deadline() {
        let _guard = env_guard();
        std::env::set_var(ENV_VAR, "3600");
        let token = CancellationToken::new();
        let out = with_deadline(async { Ok::<u32, ProviderError>(42) }, Some(&token)).await;
        std::env::remove_var(ENV_VAR);
        assert_eq!(out, Ok(42));
    }
}
