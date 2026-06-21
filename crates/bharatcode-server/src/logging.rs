use anyhow::Result;
use bharatcode_core::providers::utils::init_goose_request_log;
use tracing_subscriber::util::SubscriberInitExt;

/// Sets up the logging infrastructure for the server.
/// Logs go to a JSON file and a pretty console layer on stderr.
pub fn setup_logging(name: Option<&str>) -> Result<()> {
    init_goose_request_log()?;
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
