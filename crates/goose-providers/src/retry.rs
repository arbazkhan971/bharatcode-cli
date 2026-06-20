use crate::base::Provider;
use crate::errors::ProviderError;
use async_trait::async_trait;
use std::future::Future;
use std::time::Duration;
use tokio::time::sleep;

pub const DEFAULT_MAX_RETRIES: usize = 3;
pub const DEFAULT_INITIAL_RETRY_INTERVAL_MS: u64 = 1000;
pub const DEFAULT_BACKOFF_MULTIPLIER: f64 = 2.0;
pub const DEFAULT_MAX_RETRY_INTERVAL_MS: u64 = 30_000;

/// Environment variable for the maximum number of attempts, *including* the
/// first one (so `BHARATCODE_RETRY_MAX=5` permits the initial call plus 4
/// retries). Read centrally by [`RetryConfig::with_env_overrides`].
pub const ENV_RETRY_MAX: &str = "BHARATCODE_RETRY_MAX";
/// Environment variable for the base/initial retry interval in milliseconds.
pub const ENV_RETRY_BASE_MS: &str = "BHARATCODE_RETRY_BASE_MS";
/// Environment variable for the ceiling on any single retry interval (ms).
pub const ENV_RETRY_MAX_MS: &str = "BHARATCODE_RETRY_MAX_MS";

/// Hard cap on attempts to keep a misconfigured environment safe.
const ENV_MAX_ATTEMPTS_CAP: usize = 10;

fn parse_retry_env<T: std::str::FromStr>(key: &str) -> Option<T> {
    std::env::var(key).ok()?.trim().parse().ok()
}

#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_retries: usize,
    /// Initial interval between retries in milliseconds
    pub initial_interval_ms: u64,
    /// Multiplier for backoff (exponential)
    pub backoff_multiplier: f64,
    /// Maximum interval between retries in milliseconds
    pub max_interval_ms: u64,
    /// When true, only retry on transient errors (ServerError, NetworkError,
    /// RateLimitExceeded). RequestFailed (4xx client errors) will not be retried.
    pub transient_only: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: DEFAULT_MAX_RETRIES,
            initial_interval_ms: DEFAULT_INITIAL_RETRY_INTERVAL_MS,
            backoff_multiplier: DEFAULT_BACKOFF_MULTIPLIER,
            max_interval_ms: DEFAULT_MAX_RETRY_INTERVAL_MS,
            transient_only: false,
        }
    }
}

impl RetryConfig {
    pub fn new(
        max_retries: usize,
        initial_interval_ms: u64,
        backoff_multiplier: f64,
        max_interval_ms: u64,
    ) -> Self {
        Self {
            max_retries,
            initial_interval_ms,
            backoff_multiplier,
            max_interval_ms,
            transient_only: false,
        }
    }

    pub fn transient_only(mut self) -> Self {
        self.transient_only = true;
        self
    }

    /// Apply `BHARATCODE_RETRY_*` overrides on top of this config.
    ///
    /// Only fields whose environment variable is explicitly set are overridden,
    /// so a provider's own [`RetryConfig`] (e.g. Ollama's transient-only policy
    /// or Databricks' tuned intervals) is preserved when the operator has not
    /// asked for a global override. This is the single, central place every
    /// provider honours the `BHARATCODE_RETRY_*` knobs (see
    /// [`ProviderRetry::with_retry`]).
    pub fn with_env_overrides(mut self) -> Self {
        if let Some(max_attempts) = parse_retry_env::<usize>(ENV_RETRY_MAX) {
            // `BHARATCODE_RETRY_MAX` is the total attempt count including the
            // first; `max_retries` counts only the retries after it.
            let max_attempts = max_attempts.clamp(1, ENV_MAX_ATTEMPTS_CAP);
            self.max_retries = max_attempts - 1;
        }
        if let Some(base_ms) = parse_retry_env::<u64>(ENV_RETRY_BASE_MS) {
            self.initial_interval_ms = base_ms.max(1);
        }
        if let Some(max_ms) = parse_retry_env::<u64>(ENV_RETRY_MAX_MS) {
            self.max_interval_ms = max_ms;
        }
        // Keep the ceiling consistent with the (possibly overridden) base.
        self.max_interval_ms = self.max_interval_ms.max(self.initial_interval_ms);
        self
    }

    pub fn max_retries(&self) -> usize {
        self.max_retries
    }

    pub fn delay_for_attempt(&self, attempt: usize) -> Duration {
        if attempt == 0 {
            return Duration::from_millis(0);
        }

        let exponent = (attempt - 1) as u32;
        let base_delay_ms = (self.initial_interval_ms as f64
            * self.backoff_multiplier.powi(exponent as i32)) as u64;

        let capped_delay_ms = std::cmp::min(base_delay_ms, self.max_interval_ms);

        let jitter_factor_to_avoid_thundering_herd = 0.8 + (rand::random::<f64>() * 0.4);
        let jitter_delay_ms =
            (capped_delay_ms as f64 * jitter_factor_to_avoid_thundering_herd) as u64;

        Duration::from_millis(jitter_delay_ms)
    }
}

pub fn should_retry(error: &ProviderError, config: &RetryConfig) -> bool {
    match error {
        ProviderError::RateLimitExceeded { .. }
        | ProviderError::ServerError(_)
        | ProviderError::NetworkError(_) => true,
        ProviderError::RequestFailed(_) => !config.transient_only,
        _ => false,
    }
}

pub async fn retry_operation<F, Fut, T>(
    config: &RetryConfig,
    operation: F,
) -> Result<T, ProviderError>
where
    F: Fn() -> Fut + Send,
    Fut: Future<Output = Result<T, ProviderError>> + Send,
    T: Send,
{
    let mut attempts = 0;

    loop {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(error) => {
                if should_retry(&error, config) && attempts < config.max_retries {
                    attempts += 1;
                    tracing::warn!(
                        "Request failed, retrying ({}/{}): {:?}",
                        attempts,
                        config.max_retries,
                        error
                    );

                    let delay = match &error {
                        ProviderError::RateLimitExceeded {
                            retry_delay: Some(d),
                            ..
                        } => *d,
                        _ => config.delay_for_attempt(attempts),
                    };

                    sleep(delay).await;
                    continue;
                }
                return Err(error);
            }
        }
    }
}

/// Trait for retry functionality to keep Provider dyn-compatible.
///
/// All `Provider` implementors get this via the blanket impl below.
#[async_trait]
pub trait ProviderRetry {
    fn retry_config(&self) -> RetryConfig {
        RetryConfig::default()
    }

    async fn with_retry<F, Fut, T>(&self, operation: F) -> Result<T, ProviderError>
    where
        F: Fn() -> Fut + Send,
        Fut: Future<Output = Result<T, ProviderError>> + Send,
        T: Send,
    {
        // Apply the central `BHARATCODE_RETRY_*` overrides so every provider that
        // retries through this path honours the operator's configured policy.
        self.with_retry_config(operation, self.retry_config().with_env_overrides())
            .await
    }

    async fn with_retry_config<F, Fut, T>(
        &self,
        operation: F,
        config: RetryConfig,
    ) -> Result<T, ProviderError>
    where
        F: Fn() -> Fut + Send,
        Fut: Future<Output = Result<T, ProviderError>> + Send,
        T: Send;
}

#[async_trait]
impl<P: Provider> ProviderRetry for P {
    fn retry_config(&self) -> RetryConfig {
        Provider::retry_config(self)
    }

    async fn with_retry_config<F, Fut, T>(
        &self,
        operation: F,
        config: RetryConfig,
    ) -> Result<T, ProviderError>
    where
        F: Fn() -> Fut + Send,
        Fut: Future<Output = Result<T, ProviderError>> + Send,
        T: Send,
    {
        let mut attempts = 0;
        let mut auth_retried = false;

        loop {
            return match operation().await {
                Ok(result) => Ok(result),
                Err(error) => {
                    // Auth retry is separate from transient-error retries: we get
                    // at most 1 credential refresh, independent of max_retries.
                    if matches!(error, ProviderError::Authentication(_)) && !auth_retried {
                        auth_retried = true;
                        match self.refresh_credentials().await {
                            Ok(()) => {
                                tracing::warn!(
                                    "Credentials refreshed after auth error, retrying: {:?}",
                                    error
                                );
                                continue;
                            }
                            Err(refresh_err) => {
                                tracing::warn!(
                                    "Credential refresh failed, returning original auth error: {:?}",
                                    refresh_err
                                );
                            }
                        }
                    }

                    if should_retry(&error, &config) && attempts < config.max_retries {
                        attempts += 1;
                        tracing::warn!(
                            "Request failed, retrying ({}/{}): {:?}",
                            attempts,
                            config.max_retries,
                            error
                        );

                        let delay = match &error {
                            ProviderError::RateLimitExceeded {
                                retry_delay: Some(provider_delay),
                                ..
                            } => *provider_delay,
                            _ => config.delay_for_attempt(attempts),
                        };

                        let skip_backoff = std::env::var("BHARATCODE_PROVIDER_SKIP_BACKOFF")
                            .unwrap_or_default()
                            .parse::<bool>()
                            .unwrap_or(false);

                        if skip_backoff {
                            tracing::info!(
                                "Skipping backoff due to BHARATCODE_PROVIDER_SKIP_BACKOFF"
                            );
                        } else {
                            tracing::info!("Backing off for {:?} before retry", delay);
                            sleep(delay).await;
                        }
                        continue;
                    }

                    Err(error)
                }
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_retries_request_failed() {
        let config = RetryConfig::default();
        let error = ProviderError::RequestFailed("Bad request (400): model not found".into());
        assert!(should_retry(&error, &config));
    }

    #[test]
    fn transient_only_skips_request_failed() {
        let config = RetryConfig::default().transient_only();
        let error = ProviderError::RequestFailed("Bad request (400): model not found".into());
        assert!(!should_retry(&error, &config));
    }

    #[test]
    fn transient_only_still_retries_server_error() {
        let config = RetryConfig::default().transient_only();
        assert!(should_retry(
            &ProviderError::ServerError("500 internal".into()),
            &config
        ));
    }

    #[test]
    fn transient_only_still_retries_network_error() {
        let config = RetryConfig::default().transient_only();
        assert!(should_retry(
            &ProviderError::NetworkError("connection refused".into()),
            &config
        ));
    }

    #[test]
    fn transient_only_still_retries_rate_limit() {
        let config = RetryConfig::default().transient_only();
        assert!(should_retry(
            &ProviderError::RateLimitExceeded {
                details: "too many requests".into(),
                retry_delay: None,
            },
            &config
        ));
    }

    #[test]
    fn never_retries_auth_errors() {
        let config = RetryConfig::default();
        assert!(!should_retry(
            &ProviderError::Authentication("invalid key".into()),
            &config
        ));
    }
}
