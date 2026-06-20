//! Transient tool-failure auto-retry on dispatch.
//!
//! When the agent dispatches an approved tool call it gets back a
//! `Result<ToolCallResult, ErrorData>`. Most of the time a failure is
//! *terminal* — the arguments were malformed, or a policy hook denied the
//! call — and retrying is pointless (or actively wrong). But a meaningful
//! slice of failures from flaky MCP servers and extension tools are
//! *transient*: an internal error, a dropped transport, a momentary hiccup
//! that succeeds on a second attempt. This module wraps that single dispatch
//! so a transient failure is retried with bounded exponential backoff before
//! it is surfaced, without touching `dispatch_tool_call` itself.
//!
//! The wrapper is **off by default**. With `BHARATCODE_TOOL_RETRY` unset the
//! attempt budget is `1`, so the closure is invoked exactly once and behavior
//! is byte-identical to the un-wrapped path. Setting the variable (e.g.
//! `2:200ms`) opts in to `attempts` total tries with `base` initial backoff,
//! doubled each retry and clamped to sane bounds.
//!
//! Retryability is decided purely from the returned [`ErrorData`]:
//!   * [`ErrorCode::INTERNAL_ERROR`] (and other transport/server-side codes)
//!     are treated as transient and eligible for retry — **unless** the
//!     message is the explicit policy-denial sentinel, which is always
//!     terminal regardless of its code; and
//!   * [`ErrorCode::INVALID_PARAMS`] (and the other request-shape codes:
//!     parse, invalid-request, method-not-found) are terminal — the same
//!     request will fail the same way, so there is nothing to gain.
//!
//! This is deliberately distinct from the tool *governor* (concurrency cap +
//! per-tool wall-clock timeout), which bounds runaway or hung tools while they
//! are still executing. This module never interrupts a running tool; it only
//! reacts to an *already-completed* failed dispatch and decides whether to
//! try again.
//!
//! This module is original work; nothing here is ported from third-party
//! sources.

use std::future::Future;
use std::time::Duration;

use rmcp::model::{ErrorCode, ErrorData};
use tokio_util::sync::CancellationToken;

/// Environment knob: `"<attempts>:<base>"`, e.g. `"2:200ms"` or `"3:100ms"`.
/// Unset (or unparseable) means a single attempt — i.e. the feature is off and
/// the wrapped closure runs exactly once.
const RETRY_KEY: &str = "BHARATCODE_TOOL_RETRY";

/// Substring that uniquely identifies a policy-hook denial. The dispatch path
/// emits this with [`ErrorCode::INTERNAL_ERROR`], so we must special-case it:
/// a denial must never be retried even though its code is otherwise transient.
const POLICY_DENY_MARKER: &str = "denied by policy hook";

/// Bounds for the attempt budget. At least one attempt always runs; the upper
/// bound keeps a fat-fingered env value from hammering a flaky tool forever.
const MIN_ATTEMPTS: u32 = 1;
const MAX_ATTEMPTS: u32 = 8;

/// Bounds for the base backoff delay. A zero base would busy-loop; the ceiling
/// keeps a single retry from stalling the whole turn.
const MIN_BASE_DELAY: Duration = Duration::from_millis(1);
const MAX_BASE_DELAY: Duration = Duration::from_secs(30);

/// Default base delay used when `attempts` is given without a parseable delay.
const DEFAULT_BASE_DELAY: Duration = Duration::from_millis(200);

/// How a failed dispatch should be handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Disposition {
    /// The failure looks transient — try again if budget remains.
    Retryable,
    /// The failure is terminal — surface it immediately.
    Terminal,
}

/// Classify an [`ErrorData`] as retryable (transient) or terminal.
///
/// The policy-denial sentinel is terminal regardless of its error code.
/// Request-shape codes (invalid params/request, parse, method-not-found) are
/// terminal. Everything else — notably `INTERNAL_ERROR` and any unknown /
/// transport code — is treated as transient and eligible for retry.
fn classify(err: &ErrorData) -> Disposition {
    if err.message.contains(POLICY_DENY_MARKER) {
        return Disposition::Terminal;
    }

    match err.code {
        ErrorCode::INVALID_PARAMS
        | ErrorCode::INVALID_REQUEST
        | ErrorCode::PARSE_ERROR
        | ErrorCode::METHOD_NOT_FOUND => Disposition::Terminal,
        // INTERNAL_ERROR, transport/server-side, and any code we don't
        // explicitly recognize: assume transient and worth one more shot.
        _ => Disposition::Retryable,
    }
}

/// Resolved retry policy: how many total attempts and the initial backoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RetryPolicy {
    attempts: u32,
    base_delay: Duration,
}

impl RetryPolicy {
    /// The off-by-default policy: a single attempt, no backoff in play.
    const fn single() -> Self {
        Self {
            attempts: 1,
            base_delay: DEFAULT_BASE_DELAY,
        }
    }

    /// Parse the policy from the process environment, clamped to safe bounds.
    /// Unset or unparseable yields [`RetryPolicy::single`] (feature off).
    fn from_env() -> Self {
        match std::env::var(RETRY_KEY) {
            Ok(raw) => Self::parse(&raw),
            Err(_) => Self::single(),
        }
    }

    /// Parse a `"<attempts>[:<base>]"` spec, e.g. `"2:200ms"`, `"3"`, `"2:1s"`.
    ///
    /// A missing or unparseable attempt count disables retries (`attempts==1`).
    /// A missing or unparseable delay falls back to [`DEFAULT_BASE_DELAY`].
    /// Both fields are clamped to their respective bounds.
    fn parse(raw: &str) -> Self {
        let mut parts = raw.trim().splitn(2, ':');

        let attempts = match parts.next().and_then(|s| s.trim().parse::<u32>().ok()) {
            Some(n) => n.clamp(MIN_ATTEMPTS, MAX_ATTEMPTS),
            // No usable attempt count -> feature stays off.
            None => return Self::single(),
        };

        let base_delay = parts
            .next()
            .and_then(|s| parse_duration(s.trim()))
            .unwrap_or(DEFAULT_BASE_DELAY)
            .clamp(MIN_BASE_DELAY, MAX_BASE_DELAY);

        Self {
            attempts,
            base_delay,
        }
    }

    /// True when this policy is a transparent single attempt.
    const fn is_off(&self) -> bool {
        self.attempts <= 1
    }

    /// Backoff before the retry that follows attempt `attempt_index`
    /// (0-based). Exponential: `base * 2^attempt_index`, saturating so it can
    /// never overflow, then capped at [`MAX_BASE_DELAY`].
    fn backoff_for(&self, attempt_index: u32) -> Duration {
        let factor = 1u64.checked_shl(attempt_index).unwrap_or(u64::MAX);
        self.base_delay
            .checked_mul(factor.min(u32::MAX as u64) as u32)
            .unwrap_or(MAX_BASE_DELAY)
            .min(MAX_BASE_DELAY)
    }
}

/// Parse a small duration spec: bare digits are milliseconds; an explicit
/// `ms`/`s` suffix is honored. Returns `None` on anything unrecognized.
fn parse_duration(s: &str) -> Option<Duration> {
    if s.is_empty() {
        return None;
    }
    if let Some(num) = s.strip_suffix("ms") {
        return num.trim().parse::<u64>().ok().map(Duration::from_millis);
    }
    if let Some(num) = s.strip_suffix('s') {
        return num.trim().parse::<u64>().ok().map(Duration::from_secs);
    }
    // Bare number: interpret as milliseconds.
    s.parse::<u64>().ok().map(Duration::from_millis)
}

/// Run a tool-dispatch closure with transient-failure retry.
///
/// `dispatch` is invoked, its `Result<T, ErrorData>` inspected, and — when the
/// failure is classified as [`Disposition::Retryable`] and attempt budget
/// remains — re-invoked after an exponential backoff. The policy is read once
/// from [`RETRY_KEY`]; with it unset the closure runs exactly once and the
/// outcome is returned verbatim.
///
/// `cancel` is honored between attempts: if it is triggered, no further retry
/// is scheduled and the last error is returned immediately.
pub async fn with_tool_retry<T, F, Fut>(
    cancel: Option<CancellationToken>,
    dispatch: F,
) -> Result<T, ErrorData>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, ErrorData>>,
{
    with_tool_retry_policy(RetryPolicy::from_env(), cancel, dispatch).await
}

/// Core retry loop, parameterized on an explicit policy so tests can drive it
/// without touching process-wide environment state.
async fn with_tool_retry_policy<T, F, Fut>(
    policy: RetryPolicy,
    cancel: Option<CancellationToken>,
    dispatch: F,
) -> Result<T, ErrorData>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, ErrorData>>,
{
    // attempts is clamped to >= 1, so this loop always runs at least once.
    let mut attempt: u32 = 0;
    loop {
        match dispatch().await {
            Ok(value) => return Ok(value),
            Err(err) => {
                let is_last = attempt + 1 >= policy.attempts;
                // Off, out of budget, or a terminal failure -> surface as-is.
                if policy.is_off() || is_last || classify(&err) == Disposition::Terminal {
                    return Err(err);
                }

                // Do not retry once the turn has been cancelled.
                if cancel.as_ref().is_some_and(CancellationToken::is_cancelled) {
                    return Err(err);
                }

                let delay = policy.backoff_for(attempt);
                if delay > Duration::ZERO {
                    if let Some(token) = cancel.as_ref() {
                        // Sleep, but wake early (and skip the retry) on cancel.
                        tokio::select! {
                            _ = tokio::time::sleep(delay) => {}
                            _ = token.cancelled() => return Err(err),
                        }
                    } else {
                        tokio::time::sleep(delay).await;
                    }
                }

                attempt += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn err(code: ErrorCode, msg: &'static str) -> ErrorData {
        ErrorData::new(code, msg, None)
    }

    // --- classifier table -------------------------------------------------

    #[test]
    fn classifies_invalid_params_as_terminal() {
        assert_eq!(
            classify(&err(ErrorCode::INVALID_PARAMS, "bad schema")),
            Disposition::Terminal
        );
    }

    #[test]
    fn classifies_request_shape_codes_as_terminal() {
        for code in [
            ErrorCode::INVALID_REQUEST,
            ErrorCode::PARSE_ERROR,
            ErrorCode::METHOD_NOT_FOUND,
        ] {
            assert_eq!(classify(&err(code, "shape")), Disposition::Terminal);
        }
    }

    #[test]
    fn classifies_policy_denial_as_terminal_even_when_internal() {
        // The dispatch path emits the denial with INTERNAL_ERROR; the marker
        // in the message must override the otherwise-retryable code.
        let denial = err(
            ErrorCode::INTERNAL_ERROR,
            "Tool call denied by policy hook `acme`: nope. Do not retry.",
        );
        assert_eq!(classify(&denial), Disposition::Terminal);
    }

    #[test]
    fn classifies_internal_error_as_retryable() {
        assert_eq!(
            classify(&err(ErrorCode::INTERNAL_ERROR, "transport reset")),
            Disposition::Retryable
        );
    }

    #[test]
    fn classifies_unknown_code_as_retryable() {
        assert_eq!(
            classify(&err(ErrorCode(-32099), "weird server error")),
            Disposition::Retryable
        );
    }

    // --- env / spec parsing ----------------------------------------------

    #[test]
    fn unset_env_is_single_attempt() {
        assert_eq!(RetryPolicy::single().attempts, 1);
        assert!(RetryPolicy::single().is_off());
    }

    #[test]
    fn parses_attempts_and_delay() {
        let p = RetryPolicy::parse("2:200ms");
        assert_eq!(p.attempts, 2);
        assert_eq!(p.base_delay, Duration::from_millis(200));
        assert!(!p.is_off());
    }

    #[test]
    fn parses_seconds_suffix_and_bare_millis() {
        assert_eq!(
            RetryPolicy::parse("3:1s").base_delay,
            Duration::from_secs(1)
        );
        assert_eq!(
            RetryPolicy::parse("3:150").base_delay,
            Duration::from_millis(150)
        );
    }

    #[test]
    fn attempts_clamped_to_bounds() {
        assert_eq!(RetryPolicy::parse("0:10ms").attempts, MIN_ATTEMPTS);
        assert_eq!(RetryPolicy::parse("9999:10ms").attempts, MAX_ATTEMPTS);
    }

    #[test]
    fn garbage_or_missing_attempts_disables_retry() {
        assert!(RetryPolicy::parse("nope").is_off());
        assert!(RetryPolicy::parse("").is_off());
        // Missing delay -> default delay, attempts still honored.
        let p = RetryPolicy::parse("2");
        assert_eq!(p.attempts, 2);
        assert_eq!(p.base_delay, DEFAULT_BASE_DELAY);
    }

    #[test]
    fn backoff_is_exponential_and_capped() {
        let p = RetryPolicy {
            attempts: 8,
            base_delay: Duration::from_millis(100),
        };
        assert_eq!(p.backoff_for(0), Duration::from_millis(100));
        assert_eq!(p.backoff_for(1), Duration::from_millis(200));
        assert_eq!(p.backoff_for(2), Duration::from_millis(400));
        // Large shift saturates rather than overflowing, capped at the ceiling.
        assert_eq!(p.backoff_for(60), MAX_BASE_DELAY);
    }

    // --- retry loop behavior ---------------------------------------------

    /// A closure that fails `fail_times` with INTERNAL_ERROR, then succeeds,
    /// counting how many times it was invoked.
    fn flaky(
        fail_times: u32,
        calls: &Cell<u32>,
    ) -> impl Fn() -> std::future::Ready<Result<u32, ErrorData>> + '_ {
        move || {
            let n = calls.get() + 1;
            calls.set(n);
            let res = if n <= fail_times {
                Err(err(ErrorCode::INTERNAL_ERROR, "transient"))
            } else {
                Ok(n)
            };
            std::future::ready(res)
        }
    }

    #[tokio::test]
    async fn succeeds_after_n_minus_one_transient_failures() {
        // attempts >= N: a closure failing N-1 times then succeeding returns
        // Ok and is called exactly N times.
        let calls = Cell::new(0);
        let policy = RetryPolicy {
            attempts: 5,
            base_delay: Duration::from_millis(10),
        };
        let out = with_tool_retry_policy(policy, None, flaky(3, &calls)).await;
        assert_eq!(out.unwrap(), 4);
        assert_eq!(calls.get(), 4, "should have made exactly N=4 calls");
    }

    #[tokio::test]
    async fn exhausts_budget_then_surfaces_error() {
        // Always fails: called exactly `attempts` times, then surfaces Err.
        let calls = Cell::new(0);
        let policy = RetryPolicy {
            attempts: 3,
            base_delay: Duration::from_millis(10),
        };
        let out = with_tool_retry_policy(policy, None, flaky(u32::MAX, &calls)).await;
        assert!(out.is_err());
        assert_eq!(calls.get(), 3);
    }

    #[tokio::test]
    async fn single_attempt_calls_once_regardless_of_error() {
        // attempts == 1 (feature off): exactly one call even on a retryable err.
        let calls = Cell::new(0);
        let out =
            with_tool_retry_policy(RetryPolicy::single(), None, flaky(u32::MAX, &calls)).await;
        assert!(out.is_err());
        assert_eq!(calls.get(), 1);
    }

    #[tokio::test]
    async fn terminal_error_is_not_retried() {
        // A terminal (INVALID_PARAMS) failure is surfaced after one call even
        // with a generous attempt budget.
        let calls = Cell::new(0);
        let policy = RetryPolicy {
            attempts: 5,
            base_delay: Duration::from_millis(10),
        };
        let out = with_tool_retry_policy(policy, None, || {
            let n = calls.get() + 1;
            calls.set(n);
            std::future::ready(Err::<u32, _>(err(ErrorCode::INVALID_PARAMS, "bad args")))
        })
        .await;
        assert!(out.is_err());
        assert_eq!(calls.get(), 1);
    }

    #[tokio::test]
    async fn policy_denial_is_not_retried() {
        // Even with INTERNAL_ERROR code, the policy-deny marker is terminal.
        let calls = Cell::new(0);
        let policy = RetryPolicy {
            attempts: 5,
            base_delay: Duration::from_millis(10),
        };
        let out = with_tool_retry_policy(policy, None, || {
            let n = calls.get() + 1;
            calls.set(n);
            std::future::ready(Err::<u32, _>(err(
                ErrorCode::INTERNAL_ERROR,
                "Tool call denied by policy hook `acme`: nope.",
            )))
        })
        .await;
        assert!(out.is_err());
        assert_eq!(calls.get(), 1);
    }

    #[tokio::test]
    async fn cancelled_token_stops_retry() {
        // A pre-cancelled token means the first transient failure is the last.
        let calls = Cell::new(0);
        let token = CancellationToken::new();
        token.cancel();
        let policy = RetryPolicy {
            attempts: 5,
            base_delay: Duration::from_millis(10),
        };
        let out = with_tool_retry_policy(policy, Some(token), flaky(u32::MAX, &calls)).await;
        assert!(out.is_err());
        assert_eq!(calls.get(), 1);
    }

    #[tokio::test]
    async fn uncancelled_token_allows_retry() {
        // A live (un-cancelled) token does not impede normal retrying.
        let calls = Cell::new(0);
        let token = CancellationToken::new();
        let policy = RetryPolicy {
            attempts: 4,
            base_delay: Duration::from_millis(10),
        };
        let out = with_tool_retry_policy(policy, Some(token), flaky(2, &calls)).await;
        assert_eq!(out.unwrap(), 3);
        assert_eq!(calls.get(), 3);
    }
}
