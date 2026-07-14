use anyhow::Result;
use bharatcode_core::providers::utils::init_goose_request_log;
use std::sync::OnceLock;

// Used to ensure we only set up tracing once
static INIT: OnceLock<Result<()>> = OnceLock::new();

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

/// Sets up the logging infrastructure for the CLI.
/// Logs go to a JSON file only (no console output).
pub fn setup_logging(name: Option<&str>) -> &'static Result<()> {
    INIT.get_or_init(|| {
        use tracing_subscriber::util::SubscriberInitExt;

        if request_logging_enabled() {
            init_goose_request_log()?;
        }
        let config = bharatcode_core::logging::LoggingConfig {
            component: "cli",
            name,
            extra_directives: &["bharatcode_cli=info"],
            console: false,
            json: true,
        };
        let subscriber = bharatcode_core::logging::build_logging_subscriber(&config)?;

        subscriber
            .try_init()
            .map_err(|e| anyhow::anyhow!("Failed to set global subscriber: {}", e))?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::{is_truthy, request_logging_enabled, REQUEST_LOG_ENV};
    use bharatcode_core::tracing::langfuse_layer;
    use std::env;
    use tempfile::TempDir;

    fn setup_temp_home() -> TempDir {
        let temp_dir = TempDir::new().unwrap();
        if cfg!(windows) {
            env::set_var("USERPROFILE", temp_dir.path());
        } else {
            env::set_var("HOME", temp_dir.path());
        }
        temp_dir
    }

    #[test]
    fn test_log_directory_creation() {
        let _temp_dir = setup_temp_home();
        let log_dir = bharatcode_core::logging::prepare_log_directory("cli", true).unwrap();
        assert!(log_dir.exists());
        assert!(log_dir.is_dir());

        let path_components: Vec<_> = log_dir.components().collect();
        assert!(path_components
            .iter()
            .any(|c| c.as_os_str() == "bharatcode"));
        assert!(path_components.iter().any(|c| c.as_os_str() == "logs"));
        assert!(path_components.iter().any(|c| c.as_os_str() == "cli"));
    }

    #[tokio::test]
    async fn test_langfuse_layer_creation() {
        let _temp_dir = setup_temp_home();

        let original_vars = [
            ("LANGFUSE_PUBLIC_KEY", env::var("LANGFUSE_PUBLIC_KEY").ok()),
            ("LANGFUSE_SECRET_KEY", env::var("LANGFUSE_SECRET_KEY").ok()),
            ("LANGFUSE_URL", env::var("LANGFUSE_URL").ok()),
            (
                "LANGFUSE_INIT_PROJECT_PUBLIC_KEY",
                env::var("LANGFUSE_INIT_PROJECT_PUBLIC_KEY").ok(),
            ),
            (
                "LANGFUSE_INIT_PROJECT_SECRET_KEY",
                env::var("LANGFUSE_INIT_PROJECT_SECRET_KEY").ok(),
            ),
        ];

        for (var, _) in &original_vars {
            env::remove_var(var);
        }

        assert!(langfuse_layer::create_langfuse_observer().is_none());

        env::set_var("LANGFUSE_PUBLIC_KEY", "test_public_key");
        env::set_var("LANGFUSE_SECRET_KEY", "test_secret_key");
        assert!(langfuse_layer::create_langfuse_observer().is_some());

        env::remove_var("LANGFUSE_PUBLIC_KEY");
        env::remove_var("LANGFUSE_SECRET_KEY");
        env::set_var("LANGFUSE_INIT_PROJECT_PUBLIC_KEY", "test_public_key");
        env::set_var("LANGFUSE_INIT_PROJECT_SECRET_KEY", "test_secret_key");
        assert!(langfuse_layer::create_langfuse_observer().is_some());

        env::remove_var("LANGFUSE_INIT_PROJECT_PUBLIC_KEY");
        assert!(langfuse_layer::create_langfuse_observer().is_none());

        for (var, value) in original_vars {
            match value {
                Some(val) => env::set_var(var, val),
                None => env::remove_var(var),
            }
        }
    }

    #[tokio::test]
    async fn test_default_filter_avoids_debug_by_default() {
        // The shared helper honours RUST_LOG; without it the defaults apply.
        // We just smoke-check that building the subscriber doesn't panic.
        let _temp_dir = setup_temp_home();
        let config = bharatcode_core::logging::LoggingConfig {
            component: "cli-test",
            name: None,
            extra_directives: &["bharatcode_cli=info"],
            console: false,
            json: true,
        };
        assert!(bharatcode_core::logging::build_logging_subscriber(&config).is_ok());
    }

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
