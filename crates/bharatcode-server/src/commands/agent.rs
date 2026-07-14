use crate::configuration;
use crate::state;
use anyhow::Result;
use axum::middleware;
use axum_server::Handle;
use bharatcode_core::acp::server_factory::{AcpServer, AcpServerFactoryConfig};
use bharatcode_core::acp::transport::create_acp_router;
use bharatcode_core::agents::GoosePlatform;
use bharatcode_core::config::paths::Paths;
use bharatcode_server::auth::{check_acp_token, check_token};
#[cfg(any(feature = "rustls-tls", feature = "native-tls"))]
use bharatcode_server::tls::setup_tls;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

const SECRET_KEY_ENV: &str = "BHARATCODE_SERVER__SECRET_KEY";

fn boot_marker(message: &str) {
    eprintln!("GOOSED_BOOT: {message}");
}

fn generate_secret_key() -> String {
    hex::encode(rand::random::<[u8; 32]>())
}

/// A blank secret authenticates any caller that sends an equally blank
/// `X-Secret-Key`, so refuse to start rather than falling back to a generated
/// secret the operator's clients would not know.
fn validate_configured_secret(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!(
            "{SECRET_KEY_ENV} is set but blank; unset it to use a generated secret, or set a non-empty value"
        );
    }
    Ok(trimmed.to_string())
}

fn resolve_secret_key() -> Result<String> {
    match std::env::var(SECRET_KEY_ENV) {
        Ok(configured) => validate_configured_secret(&configured),
        Err(std::env::VarError::NotPresent) => Ok(generate_secret_key()),
        Err(std::env::VarError::NotUnicode(_)) => {
            anyhow::bail!("{SECRET_KEY_ENV} is set to a value that is not valid unicode")
        }
    }
}

#[cfg(unix)]
async fn shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};

    let mut sigint = signal(SignalKind::interrupt()).expect("failed to install SIGINT handler");
    let mut sigterm = signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");

    tokio::select! {
        _ = sigint.recv() => {},
        _ = sigterm.recv() => {},
    }
}

#[cfg(not(unix))]
async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

pub async fn run() -> Result<()> {
    // Install the rustls crypto provider early, before any spawned tasks (tunnel,
    // gateways, etc.) try to open TLS connections. Both `ring` and `aws-lc-rs`
    // features are enabled on rustls (via different transitive deps), so rustls
    // cannot auto-detect a provider — we must pick one explicitly.
    #[cfg(feature = "rustls-tls")]
    let _ = rustls::crypto::ring::default_provider().install_default();

    boot_marker("main entered");
    crate::logging::setup_logging(Some("goosed"))?;

    let settings = configuration::Settings::new()?;

    let secret_key = resolve_secret_key()?;

    boot_marker("appstate init start");
    let app_state = state::AppState::new(settings.tls).await?;

    // Share the server secret with the tunnel manager so it uses the same
    // key for forwarded requests, without mutating the process environment.
    app_state
        .tunnel_manager
        .set_server_secret(secret_key.clone())
        .await;

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // TODO(acp-migration): When ui/desktop launches `goose serve` directly,
    // move any goosed-only ACP setup into the goose serve path before deleting
    // this bridge. In particular, verify everything ACP currently gets from
    // goosed startup/AppState initialization, including builtin extension
    // registration and the desktop platform identity.
    let acp_server = Arc::new(AcpServer::new(AcpServerFactoryConfig {
        builtins: vec!["developer".to_string()],
        data_dir: Paths::data_dir(),
        config_dir: Paths::config_dir(),
        goose_platform: GoosePlatform::GooseDesktop,
        additional_source_roots: Vec::new(),
    }));

    let rest_router = crate::routes::configure(app_state.clone(), secret_key.clone()).layer(
        middleware::from_fn_with_state(secret_key.clone(), check_token),
    );
    let acp_router = create_acp_router(acp_server).layer(middleware::from_fn_with_state(
        secret_key.clone(),
        check_acp_token,
    ));

    let app = rest_router.merge(acp_router).layer(cors);

    let addr = settings.socket_addr();

    let tunnel_manager = app_state.tunnel_manager.clone();
    tokio::spawn(async move {
        tunnel_manager.check_auto_start().await;
    });

    let gateway_manager = app_state.gateway_manager.clone();
    tokio::spawn(async move {
        gateway_manager.check_auto_start().await;
    });

    if settings.tls {
        #[cfg(any(feature = "rustls-tls", feature = "native-tls"))]
        {
            boot_marker("tls setup start");
            let tls_setup = setup_tls(
                settings.tls_cert_path.as_deref(),
                settings.tls_key_path.as_deref(),
            )
            .await?;

            let handle = Handle::new();
            let shutdown_handle = handle.clone();
            tokio::spawn(async move {
                shutdown_signal().await;
                shutdown_handle.graceful_shutdown(None);
            });

            info!("listening on https://{}", addr);
            boot_marker("listening");

            #[cfg(feature = "rustls-tls")]
            axum_server::bind_rustls(addr, tls_setup.config)
                .handle(handle)
                .serve(app.into_make_service())
                .await?;

            #[cfg(feature = "native-tls")]
            axum_server::bind_openssl(addr, tls_setup.config)
                .handle(handle)
                .serve(app.into_make_service())
                .await?;
        }

        #[cfg(not(any(feature = "rustls-tls", feature = "native-tls")))]
        {
            anyhow::bail!(
                "TLS was requested but no TLS backend is enabled. \
                 Enable the `rustls-tls` or `native-tls` feature."
            );
        }
    } else {
        boot_marker("tcp bind start");
        let listener = tokio::net::TcpListener::bind(addr).await?;

        info!("listening on http://{}", addr);
        boot_marker("listening");

        axum::serve(listener, app)
            .with_graceful_shutdown(async { shutdown_signal().await })
            .await?;
    }

    #[cfg(feature = "otel")]
    if bharatcode_core::otel::otlp::is_otlp_initialized() {
        bharatcode_core::otel::otlp::shutdown_otlp();
    }

    info!("server shutdown complete");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{generate_secret_key, validate_configured_secret};

    #[test]
    fn rejects_blank_configured_secret() {
        for raw in ["", " ", "\t", "\n", "   \r\n "] {
            assert!(
                validate_configured_secret(raw).is_err(),
                "expected {raw:?} to be rejected at startup"
            );
        }
    }

    #[test]
    fn accepts_and_trims_a_real_configured_secret() {
        assert_eq!(validate_configured_secret("s3cret").unwrap(), "s3cret");
        assert_eq!(validate_configured_secret("  s3cret\n").unwrap(), "s3cret");
    }

    #[test]
    fn generated_secret_stays_valid() {
        let generated = generate_secret_key();
        assert_eq!(generated.len(), 64);
        assert!(generated.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(
            validate_configured_secret(&generated).unwrap(),
            generated,
            "the generated-secret path must survive the new validation unchanged"
        );
    }
}
