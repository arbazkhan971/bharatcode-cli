use super::TunnelInfo;
use anyhow::{Context, Result};
use bharatcode_core::acp::transport::auth::token_matches;
use futures::{SinkExt, Stream, StreamExt};
use reqwest;
use serde::{Deserialize, Serialize};
use socket2::{SockRef, TcpKeepalive};
use std::collections::HashMap;
use std::fmt::Display;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, OwnedSemaphorePermit, RwLock, Semaphore};
use tokio::task::JoinHandle;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info, warn};
use url::Url;

/// Shared state for proxying tunnel requests to the local goosed server.
#[derive(Clone)]
struct ProxyContext {
    port: u16,
    tunnel_secret: String,
    server_secret: String,
    http_client: reqwest::Client,
}

const WORKER_URL: &str = "https://cloudflare-tunnel-proxy.michael-neale.workers.dev";
const IDLE_TIMEOUT_SECS: u64 = 300;
const CONNECTION_TIMEOUT_SECS: u64 = 30;
const MAX_WS_SIZE: usize = 900_000;

/// Requests the tunnel will proxy at once for a single websocket connection. A streaming
/// response holds its slot for the lifetime of the stream, so this also bounds concurrent
/// SSE subscriptions.
const MAX_CONCURRENT_REQUESTS: usize = 16;

/// Upper bound on a buffered (non-streaming) response body. Streaming responses are
/// forwarded chunk by chunk and are never subject to this cap.
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

const STATUS_PAYLOAD_TOO_LARGE: u16 = 413;
const STATUS_SERVICE_UNAVAILABLE: u16 = 503;

fn get_worker_url() -> String {
    std::env::var("BHARATCODE_TUNNEL_WORKER_URL")
        .ok()
        .unwrap_or_else(|| WORKER_URL.to_string())
}

type WebSocketSender = Arc<
    RwLock<
        Option<
            futures::stream::SplitSink<
                tokio_tungstenite::WebSocketStream<
                    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
                >,
                Message,
            >,
        >,
    >,
>;

#[derive(Debug, Serialize, Deserialize)]
struct TunnelMessage {
    #[serde(rename = "requestId")]
    request_id: String,
    method: String,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    headers: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<String>,
}

#[derive(Debug, Serialize)]
struct TunnelResponse {
    #[serde(rename = "requestId")]
    request_id: String,
    status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    headers: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "chunkIndex")]
    chunk_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "totalChunks")]
    total_chunks: Option<usize>,
    #[serde(rename = "isChunked")]
    is_chunked: bool,
    #[serde(rename = "isStreaming")]
    is_streaming: bool,
    #[serde(rename = "isFirstChunk")]
    is_first_chunk: bool,
    #[serde(rename = "isLastChunk")]
    is_last_chunk: bool,
}

fn error_response(request_id: String, status: u16, error: String) -> TunnelResponse {
    TunnelResponse {
        request_id,
        status,
        headers: None,
        body: None,
        error: Some(error),
        chunk_index: None,
        total_chunks: None,
        is_chunked: false,
        is_streaming: false,
        is_first_chunk: false,
        is_last_chunk: false,
    }
}

#[derive(Debug, PartialEq)]
enum BodyOutcome {
    Complete(String),
    TooLarge,
}

/// Buffers a response body, giving up as soon as it would exceed `limit` rather than
/// growing an unbounded allocation on behalf of a remote caller.
async fn collect_body_capped<S, B, E>(
    stream: S,
    content_length: Option<u64>,
    limit: usize,
) -> Result<BodyOutcome>
where
    S: Stream<Item = std::result::Result<B, E>>,
    B: AsRef<[u8]>,
    E: Display,
{
    if content_length.is_some_and(|len| len > limit as u64) {
        return Ok(BodyOutcome::TooLarge);
    }

    let mut buffer: Vec<u8> = Vec::new();
    let mut stream = std::pin::pin!(stream);

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| anyhow::anyhow!("Failed to read response body: {e}"))?;
        let chunk = chunk.as_ref();

        if buffer.len() + chunk.len() > limit {
            return Ok(BodyOutcome::TooLarge);
        }
        buffer.extend_from_slice(chunk);
    }

    Ok(BodyOutcome::Complete(
        String::from_utf8_lossy(&buffer).into_owned(),
    ))
}

fn validate_and_build_request(
    client: &reqwest::Client,
    url: &str,
    message: &TunnelMessage,
    tunnel_secret: &str,
    server_secret: &str,
) -> Result<reqwest::RequestBuilder> {
    if tunnel_secret.trim().is_empty() {
        anyhow::bail!("Tunnel secret is not configured");
    }

    let incoming_secret = message
        .headers
        .as_ref()
        .and_then(|h| {
            h.iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("x-secret-key"))
                .map(|(_, v)| v)
        })
        .ok_or_else(|| anyhow::anyhow!("Missing tunnel secret header"))?;

    if !token_matches(Some(incoming_secret.as_str()), tunnel_secret) {
        anyhow::bail!("Invalid tunnel secret");
    }

    let mut request_builder = match message.method.as_str() {
        "GET" => client.get(url),
        "POST" => client.post(url),
        "PUT" => client.put(url),
        "DELETE" => client.delete(url),
        "PATCH" => client.patch(url),
        _ => client.get(url),
    };

    if let Some(headers) = &message.headers {
        for (key, value) in headers {
            if key.eq_ignore_ascii_case("x-secret-key") {
                continue;
            }
            request_builder = request_builder.header(key, value);
        }
    }

    request_builder = request_builder.header("X-Secret-Key", server_secret);

    if let Some(body) = &message.body {
        if message.method != "GET" && message.method != "HEAD" {
            request_builder = request_builder.body(body.clone());
        }
    }

    Ok(request_builder)
}

async fn handle_streaming_response(
    response: reqwest::Response,
    status: u16,
    headers_map: HashMap<String, String>,
    request_id: String,
    message_path: String,
    ws_tx: WebSocketSender,
) -> Result<()> {
    info!("← {} {} [{}] (streaming)", status, message_path, request_id);

    let mut stream = response.bytes_stream();
    let mut chunk_index = 0;
    let mut is_first_chunk = true;

    while let Some(chunk_result) = stream.next().await {
        match chunk_result {
            Ok(chunk) => {
                let chunk_str = String::from_utf8_lossy(&chunk).to_string();
                let tunnel_response = TunnelResponse {
                    request_id: request_id.clone(),
                    status,
                    headers: if is_first_chunk {
                        Some(headers_map.clone())
                    } else {
                        None
                    },
                    body: Some(chunk_str),
                    error: None,
                    chunk_index: Some(chunk_index),
                    total_chunks: None,
                    is_chunked: false,
                    is_streaming: true,
                    is_first_chunk,
                    is_last_chunk: false,
                };
                send_response(ws_tx.clone(), tunnel_response).await?;
                chunk_index += 1;
                is_first_chunk = false;
            }
            Err(e) => {
                error!("Error reading stream chunk: {}", e);
                break;
            }
        }
    }

    let tunnel_response = TunnelResponse {
        request_id: request_id.clone(),
        status,
        headers: None,
        body: Some(String::new()),
        error: None,
        chunk_index: Some(chunk_index),
        total_chunks: None,
        is_chunked: false,
        is_streaming: true,
        is_first_chunk: false,
        is_last_chunk: true,
    };
    send_response(ws_tx, tunnel_response).await?;
    info!(
        "← {} {} [{}] (complete, {} chunks)",
        status, message_path, request_id, chunk_index
    );
    Ok(())
}

async fn handle_chunked_response(
    body: String,
    status: u16,
    headers_map: HashMap<String, String>,
    request_id: String,
    message_path: String,
    ws_tx: WebSocketSender,
) -> Result<()> {
    let total_chunks = body.len().div_ceil(MAX_WS_SIZE);
    info!(
        "← {} {} [{}] ({} bytes, {} chunks)",
        status,
        message_path,
        request_id,
        body.len(),
        total_chunks
    );

    for (i, chunk) in body.as_bytes().chunks(MAX_WS_SIZE).enumerate() {
        let chunk_str = String::from_utf8_lossy(chunk).to_string();
        let tunnel_response = TunnelResponse {
            request_id: request_id.clone(),
            status,
            headers: if i == 0 {
                Some(headers_map.clone())
            } else {
                None
            },
            body: Some(chunk_str),
            error: None,
            chunk_index: Some(i),
            total_chunks: Some(total_chunks),
            is_chunked: true,
            is_streaming: false,
            is_first_chunk: false,
            is_last_chunk: false,
        };
        send_response(ws_tx.clone(), tunnel_response).await?;
    }
    Ok(())
}

async fn handle_request(
    message: TunnelMessage,
    ctx: ProxyContext,
    ws_tx: WebSocketSender,
    scheme: &str,
) -> Result<()> {
    let request_id = message.request_id.clone();
    let client = &ctx.http_client;

    let url = format!("{}://127.0.0.1:{}{}", scheme, ctx.port, message.path);

    let request_builder = match validate_and_build_request(
        client,
        &url,
        &message,
        &ctx.tunnel_secret,
        &ctx.server_secret,
    ) {
        Ok(builder) => builder,
        Err(e) => {
            error!("✗ Authentication error [{}]: {}", request_id, e);
            send_response(ws_tx, error_response(request_id, 401, e.to_string())).await?;
            return Ok(());
        }
    };

    let response = match request_builder.send().await {
        Ok(resp) => resp,
        Err(e) => {
            error!("✗ Request error [{}]: {}", request_id, e);
            send_response(ws_tx, error_response(request_id, 500, e.to_string())).await?;
            return Ok(());
        }
    };

    let status = response.status().as_u16();
    // Normalize header names to lowercase per RFC 7230 (HTTP headers are case-insensitive)
    let headers_map: HashMap<String, String> = response
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_lowercase(),
                v.to_str().unwrap_or("").to_string(),
            )
        })
        .collect();

    let is_streaming = headers_map
        .get("content-type")
        .map(|ct| ct.contains("text/event-stream"))
        .unwrap_or(false);

    if is_streaming {
        handle_streaming_response(
            response,
            status,
            headers_map,
            request_id,
            message.path,
            ws_tx,
        )
        .await?;
    } else {
        let content_length = response.content_length();
        let body =
            match collect_body_capped(response.bytes_stream(), content_length, MAX_RESPONSE_BYTES)
                .await
            {
                Ok(BodyOutcome::Complete(body)) => body,
                Ok(BodyOutcome::TooLarge) => {
                    warn!(
                        "✗ Response body exceeds {} byte cap [{}] {}",
                        MAX_RESPONSE_BYTES, request_id, message.path
                    );
                    send_response(
                        ws_tx,
                        error_response(
                            request_id,
                            STATUS_PAYLOAD_TOO_LARGE,
                            format!(
                                "Response body exceeds the {MAX_RESPONSE_BYTES} byte tunnel cap"
                            ),
                        ),
                    )
                    .await?;
                    return Ok(());
                }
                Err(e) => {
                    error!("✗ Response body error [{}]: {}", request_id, e);
                    send_response(ws_tx, error_response(request_id, 500, e.to_string())).await?;
                    return Ok(());
                }
            };

        if body.len() > MAX_WS_SIZE {
            handle_chunked_response(body, status, headers_map, request_id, message.path, ws_tx)
                .await?;
        } else {
            let tunnel_response = TunnelResponse {
                request_id: request_id.clone(),
                status,
                headers: Some(headers_map),
                body: Some(body),
                error: None,
                chunk_index: None,
                total_chunks: None,
                is_chunked: false,
                is_streaming: false,
                is_first_chunk: false,
                is_last_chunk: false,
            };
            send_response(ws_tx, tunnel_response).await?;
        }
    }

    Ok(())
}

async fn send_response(ws_tx: WebSocketSender, response: TunnelResponse) -> Result<()> {
    let json = serde_json::to_string(&response)?;
    if let Some(tx) = ws_tx.write().await.as_mut() {
        tx.send(Message::Text(json.into()))
            .await
            .context("Failed to send response")?;
    }
    Ok(())
}

fn configure_tcp_keepalive(
    stream: &tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) {
    let tcp_stream = stream.get_ref().get_ref();
    let socket_ref = SockRef::from(tcp_stream);

    let keepalive = TcpKeepalive::new()
        .with_time(Duration::from_secs(30))
        .with_interval(Duration::from_secs(30));

    if let Err(e) = socket_ref.set_tcp_keepalive(&keepalive) {
        warn!("Failed to set TCP keep-alive: {}", e);
    } else {
        info!("✓ TCP keep-alive enabled (30s interval)");
    }
}

/// Claims one of the connection's in-flight request slots, or `None` when saturated. The
/// permit lives as long as the spawned request task, so aborting that task frees the slot.
fn acquire_request_slot(limiter: &Arc<Semaphore>) -> Option<OwnedSemaphorePermit> {
    limiter.clone().try_acquire_owned().ok()
}

async fn handle_websocket_messages(
    mut read: futures::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
    ws_tx: WebSocketSender,
    ctx: ProxyContext,
    last_activity: Arc<RwLock<Instant>>,
    active_tasks: Arc<RwLock<Vec<JoinHandle<()>>>>,
    limiter: Arc<Semaphore>,
    scheme: String,
) {
    while let Some(msg) = read.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                *last_activity.write().await = Instant::now();

                match serde_json::from_str::<TunnelMessage>(&text) {
                    Ok(tunnel_msg) => {
                        let Some(permit) = acquire_request_slot(&limiter) else {
                            warn!(
                                "✗ Overloaded, rejecting request [{}] {} ({} in flight)",
                                tunnel_msg.request_id, tunnel_msg.path, MAX_CONCURRENT_REQUESTS
                            );
                            // Answered inline: the read loop stalls until the rejection is
                            // written, which is the backpressure signal to the worker.
                            if let Err(e) = send_response(
                                ws_tx.clone(),
                                error_response(
                                    tunnel_msg.request_id,
                                    STATUS_SERVICE_UNAVAILABLE,
                                    format!(
                                        "Tunnel is at its limit of {MAX_CONCURRENT_REQUESTS} concurrent requests"
                                    ),
                                ),
                            )
                            .await
                            {
                                error!("Error sending overload response: {}", e);
                                break;
                            }
                            continue;
                        };

                        let ws_tx_clone = ws_tx.clone();
                        let ctx_clone = ctx.clone();
                        let scheme_clone = scheme.clone();
                        let task = tokio::spawn(async move {
                            let _permit = permit;
                            if let Err(e) =
                                handle_request(tunnel_msg, ctx_clone, ws_tx_clone, &scheme_clone)
                                    .await
                            {
                                error!("Error handling request: {}", e);
                            }
                        });
                        {
                            let mut tasks = active_tasks.write().await;
                            tasks.retain(|t| !t.is_finished());
                            tasks.push(task);
                        }
                    }
                    Err(e) => {
                        error!("Error parsing tunnel message: {}", e);
                    }
                }
            }
            Ok(Message::Close(_)) => {
                info!("✗ Connection closed by server");
                break;
            }
            Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {
                *last_activity.write().await = Instant::now();
            }
            Err(e) => {
                error!("✗ WebSocket error: {}", e);
                break;
            }
            _ => {}
        }
    }
}

async fn cleanup_connection(
    ws_tx: WebSocketSender,
    active_tasks: Arc<RwLock<Vec<JoinHandle<()>>>>,
) {
    if let Some(mut tx) = ws_tx.write().await.take() {
        let _ = tx.close().await;
    }

    let tasks = active_tasks.write().await.drain(..).collect::<Vec<_>>();
    info!("Aborting {} active request tasks", tasks.len());
    for task in tasks {
        task.abort();
    }
}

async fn run_single_connection(
    port: u16,
    agent_id: String,
    tunnel_secret: String,
    server_secret: String,
    scheme: String,
    restart_tx: mpsc::Sender<()>,
) {
    #[cfg(feature = "rustls-tls")]
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let worker_url = get_worker_url();
    let ws_url = worker_url
        .replace("https://", "wss://")
        .replace("http://", "ws://");

    let url = format!("{}/connect?agent_id={}", ws_url, agent_id);

    info!("Connecting to {}...", url);

    let ws_stream = match tokio::time::timeout(
        Duration::from_secs(CONNECTION_TIMEOUT_SECS),
        connect_async(url.clone()),
    )
    .await
    {
        Ok(Ok((stream, _))) => {
            configure_tcp_keepalive(&stream);
            stream
        }
        Ok(Err(e)) => {
            error!("✗ WebSocket connection error: {}", e);
            let _ = restart_tx.send(()).await;
            return;
        }
        Err(_) => {
            error!(
                "✗ WebSocket connection timeout after {}s",
                CONNECTION_TIMEOUT_SECS
            );
            let _ = restart_tx.send(()).await;
            return;
        }
    };

    info!("✓ Connected as agent: {}", agent_id);
    info!("✓ Proxying to: {}://127.0.0.1:{}", scheme, port);
    let public_url = format!("{}/tunnel/{}", worker_url, agent_id);
    info!("✓ Public URL: {}", public_url);

    let mut client_builder = reqwest::Client::builder();
    if scheme == "https" {
        client_builder = client_builder.danger_accept_invalid_certs(true);
    }
    let http_client = client_builder
        .build()
        .expect("failed to build reqwest client");

    let ctx = ProxyContext {
        port,
        tunnel_secret,
        server_secret,
        http_client,
    };

    let (write, read) = ws_stream.split();
    let ws_tx: WebSocketSender = Arc::new(RwLock::new(Some(write)));
    let last_activity = Arc::new(RwLock::new(Instant::now()));
    let active_tasks: Arc<RwLock<Vec<JoinHandle<()>>>> = Arc::new(RwLock::new(Vec::new()));
    let limiter = Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS));

    let last_activity_clone = last_activity.clone();
    let idle_task = async move {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            let elapsed = last_activity_clone.read().await.elapsed();
            if elapsed > Duration::from_secs(IDLE_TIMEOUT_SECS) {
                warn!(
                    "No activity for {} minutes, forcing reconnect",
                    IDLE_TIMEOUT_SECS / 60
                );
                break;
            }
        }
    };

    tokio::select! {
        _ = idle_task => {
            info!("✗ Idle timeout triggered");
        }
        _ = handle_websocket_messages(
            read,
            ws_tx.clone(),
            ctx,
            last_activity,
            active_tasks.clone(),
            limiter,
            scheme,
        ) => {
            info!("✗ Connection ended");
        }
    }

    cleanup_connection(ws_tx, active_tasks).await;

    let _ = restart_tx.send(()).await;
}

pub async fn start(
    port: u16,
    tunnel_secret: String,
    server_secret: String,
    agent_id: String,
    scheme: &str,
    handle: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
    restart_tx: mpsc::Sender<()>,
) -> Result<TunnelInfo> {
    let worker_url = get_worker_url();

    let agent_id_clone = agent_id.clone();
    let tunnel_secret_clone = tunnel_secret.clone();
    let server_secret_clone = server_secret;
    let scheme = scheme.to_string();

    let task = tokio::spawn(async move {
        run_single_connection(
            port,
            agent_id_clone,
            tunnel_secret_clone,
            server_secret_clone,
            scheme,
            restart_tx,
        )
        .await;
    });

    *handle.write().await = Some(task);

    let public_url = format!("{}/tunnel/{}", worker_url, agent_id);
    let hostname = Url::parse(&worker_url)?
        .host_str()
        .unwrap_or("")
        .to_string();

    Ok(TunnelInfo {
        state: super::TunnelState::Running,
        url: public_url,
        hostname,
        secret: tunnel_secret,
    })
}

pub async fn stop(handle: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>) {
    if let Some(task) = handle.write().await.take() {
        task.abort();
        info!("Lapstone tunnel stopped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const TUNNEL_SECRET: &str = "tunnel-secret-abc123";
    const SERVER_SECRET: &str = "server-secret-xyz789";

    fn message_with_secret(secret: Option<&str>) -> TunnelMessage {
        let headers = secret.map(|value| {
            HashMap::from([
                ("X-Secret-Key".to_string(), value.to_string()),
                ("content-type".to_string(), "application/json".to_string()),
            ])
        });

        TunnelMessage {
            request_id: "req-1".to_string(),
            method: "GET".to_string(),
            path: "/sessions".to_string(),
            headers,
            body: None,
        }
    }

    fn validate(message: &TunnelMessage, tunnel_secret: &str) -> Result<reqwest::Request> {
        let client = reqwest::Client::new();
        validate_and_build_request(
            &client,
            "http://127.0.0.1:1/sessions",
            message,
            tunnel_secret,
            SERVER_SECRET,
        )?
        .build()
        .map_err(Into::into)
    }

    #[test]
    fn accepts_exact_tunnel_secret_and_swaps_in_server_secret() {
        let request = validate(&message_with_secret(Some(TUNNEL_SECRET)), TUNNEL_SECRET)
            .expect("exact secret should authenticate");

        let forwarded = request
            .headers()
            .get("X-Secret-Key")
            .and_then(|v| v.to_str().ok());
        assert_eq!(forwarded, Some(SERVER_SECRET));
        assert_eq!(
            request.headers().get_all("X-Secret-Key").iter().count(),
            1,
            "caller-supplied tunnel secret must not be forwarded alongside the server secret"
        );
    }

    #[test]
    fn rejects_secrets_that_are_not_byte_identical() {
        // A digest-based comparison can accept a non-identical secret on collision, and
        // truncating comparisons accept prefixes. Every one of these differs from the
        // configured secret by at least one byte, so all must be rejected.
        for candidate in [
            "tunnel-secret-abc124",
            "tunnel-secret-abc12",
            "tunnel-secret-abc1234",
            "TUNNEL-SECRET-ABC123",
            "tunnel-secret-abc123 ",
            "",
        ] {
            assert!(
                validate(&message_with_secret(Some(candidate)), TUNNEL_SECRET).is_err(),
                "expected {candidate:?} to be rejected"
            );
        }
    }

    #[test]
    fn rejects_missing_secret_header() {
        assert!(validate(&message_with_secret(None), TUNNEL_SECRET).is_err());
    }

    #[test]
    fn fails_closed_when_configured_tunnel_secret_is_blank() {
        for configured in ["", "   "] {
            assert!(
                validate(&message_with_secret(Some(configured)), configured).is_err(),
                "a blank configured secret must never authenticate a request"
            );
            assert!(validate(&message_with_secret(None), configured).is_err());
        }
    }

    /// Yields `chunks.len()` chunks of the given sizes, counting every chunk actually polled.
    fn counted_stream(
        chunks: Vec<usize>,
        polled: Arc<AtomicUsize>,
    ) -> impl Stream<Item = std::result::Result<Vec<u8>, std::io::Error>> {
        futures::stream::iter(chunks).map(
            move |size| -> std::result::Result<Vec<u8>, std::io::Error> {
                polled.fetch_add(1, Ordering::SeqCst);
                Ok(vec![b'x'; size])
            },
        )
    }

    #[test]
    fn saturated_connection_rejects_extra_requests_and_recovers_when_a_slot_frees() {
        let limiter = Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS));

        let mut held: Vec<OwnedSemaphorePermit> = (0..MAX_CONCURRENT_REQUESTS)
            .map(|i| {
                acquire_request_slot(&limiter)
                    .unwrap_or_else(|| panic!("slot {i} should be available"))
            })
            .collect();

        assert!(
            acquire_request_slot(&limiter).is_none(),
            "a saturated connection must not spawn another request task"
        );

        // The permit is owned by the request task, so completing (or aborting) it frees the slot.
        drop(held.pop().expect("a slot should be held"));
        assert!(
            acquire_request_slot(&limiter).is_some(),
            "finishing an in-flight request must return its slot to the pool"
        );
    }

    #[test]
    fn overload_response_is_an_explicit_503_with_no_body() {
        let response = error_response(
            "req-overload".to_string(),
            STATUS_SERVICE_UNAVAILABLE,
            "Tunnel is at its limit".to_string(),
        );

        assert_eq!(response.status, 503);
        assert!(response.body.is_none());
        assert!(!response.is_chunked && !response.is_streaming);
        assert!(response.error.is_some());
    }

    #[tokio::test]
    async fn buffers_bodies_up_to_the_cap() {
        let polled = Arc::new(AtomicUsize::new(0));
        let outcome = collect_body_capped(
            counted_stream(vec![4, 4], polled.clone()),
            Some(8),
            MAX_RESPONSE_BYTES,
        )
        .await
        .expect("body under the cap should be buffered");

        assert_eq!(outcome, BodyOutcome::Complete("xxxxxxxx".to_string()));
        assert_eq!(polled.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn accepts_a_body_that_is_exactly_the_cap() {
        let polled = Arc::new(AtomicUsize::new(0));
        let outcome = collect_body_capped(counted_stream(vec![4], polled), Some(4), 4)
            .await
            .expect("a body exactly at the cap is not oversize");

        assert_eq!(outcome, BodyOutcome::Complete("xxxx".to_string()));
    }

    #[tokio::test]
    async fn stops_buffering_once_a_streamed_body_grows_past_the_cap() {
        let polled = Arc::new(AtomicUsize::new(0));
        // No content-length: the cap has to be enforced while draining the stream.
        let outcome =
            collect_body_capped(counted_stream(vec![3, 3, 3, 3], polled.clone()), None, 8)
                .await
                .expect("an oversize body is reported, not an error");

        assert_eq!(outcome, BodyOutcome::TooLarge);
        assert_eq!(
            polled.load(Ordering::SeqCst),
            3,
            "buffering must stop at the chunk that breaches the cap, not drain the whole body"
        );
    }

    #[tokio::test]
    async fn rejects_an_oversize_content_length_without_reading_the_body() {
        let polled = Arc::new(AtomicUsize::new(0));
        let outcome = collect_body_capped(
            counted_stream(vec![64], polled.clone()),
            Some(MAX_RESPONSE_BYTES as u64 + 1),
            MAX_RESPONSE_BYTES,
        )
        .await
        .expect("an oversize body is reported, not an error");

        assert_eq!(outcome, BodyOutcome::TooLarge);
        assert_eq!(
            polled.load(Ordering::SeqCst),
            0,
            "a declared-oversize body must never be buffered"
        );
    }

    #[tokio::test]
    async fn surfaces_body_read_errors() {
        let stream = futures::stream::iter(vec![
            Ok(vec![b'x'; 2]),
            Err(std::io::Error::other("connection reset")),
        ]);

        let error = collect_body_capped(stream, None, MAX_RESPONSE_BYTES)
            .await
            .expect_err("a mid-body read failure must not be reported as a complete body");
        assert!(error.to_string().contains("connection reset"));
    }

    #[test]
    fn oversize_response_is_an_explicit_413_with_no_body() {
        let response = error_response(
            "req-oversize".to_string(),
            STATUS_PAYLOAD_TOO_LARGE,
            "Response body exceeds the cap".to_string(),
        );

        assert_eq!(response.status, 413);
        assert!(response.body.is_none());
        assert!(!response.is_chunked && !response.is_streaming);
    }
}
