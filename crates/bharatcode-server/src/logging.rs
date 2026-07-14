use anyhow::Result;
use bharatcode_core::providers::utils::init_goose_request_log;
use tracing_subscriber::util::SubscriberInitExt;

/// Environment variable that opts in to the full LLM request log.
pub const REQUEST_LOG_ENV: &str = "BHARATCODE_REQUEST_LOG";

/// Returns true when full request-body logging is explicitly enabled via
/// `BHARATCODE_REQUEST_LOG`.
///
/// Accepted truthy values (case-insensitive): `1`, `true`, `yes`, `on`.
///
/// The request log persists whole conversations to disk — system prompts, user
/// messages, tool arguments and provider responses — so it stays off unless an
/// operator asks for it. With no logger installed, `start_log` in the provider
/// layer returns `None` and every provider skips request logging.
pub fn request_logging_enabled() -> bool {
    is_truthy(std::env::var(REQUEST_LOG_ENV).ok().as_deref())
}

fn is_truthy(value: Option<&str>) -> bool {
    matches!(
        value.map(str::trim).map(str::to_ascii_lowercase).as_deref(),
        Some("1") | Some("true") | Some("yes") | Some("on")
    )
}

/// Sets up the logging infrastructure for the server.
/// Logs go to a JSON file and a pretty console layer on stderr.
pub fn setup_logging(name: Option<&str>) -> Result<()> {
    if request_logging_enabled() {
        init_goose_request_log()?;
    }
    let config = bharatcode_core::logging::LoggingConfig {
        component: "server",
        name,
        extra_directives: &["bharatcode_server=info", "tower_http=info"],
        console: true,
        json: false,
    };
    let subscriber = bharatcode_core::logging::build_logging_subscriber(&config)?;
    subscriber.try_init()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{is_truthy, request_logging_enabled, REQUEST_LOG_ENV};
    use std::env;

    #[test]
    fn request_logging_is_off_unless_opted_in() {
        env::remove_var(REQUEST_LOG_ENV);
        assert!(!request_logging_enabled());

        env::set_var(REQUEST_LOG_ENV, "1");
        assert!(request_logging_enabled());

        env::remove_var(REQUEST_LOG_ENV);
    }

    #[test]
    fn request_log_gate_accepts_only_truthy_values() {
        for value in ["1", "true", "TRUE", " yes ", "on"] {
            assert!(is_truthy(Some(value)), "expected {value:?} to enable");
        }
        for value in ["", "0", "false", "off", "no", "maybe"] {
            assert!(!is_truthy(Some(value)), "expected {value:?} to stay off");
        }
        assert!(!is_truthy(None));
    }
}
