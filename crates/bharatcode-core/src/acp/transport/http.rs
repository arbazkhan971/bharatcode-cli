use std::{convert::Infallible, sync::Arc, time::Duration};

use axum::{
    body::Body,
    extract::State,
    http::{HeaderValue, Request, StatusCode},
    response::{IntoResponse, Response, Sse},
};
use http_body_util::BodyExt;
use serde_json::Value;
use tokio::sync::broadcast;
use tracing::{debug, error, info, trace, warn};

use super::connection::{
    ConnectionGuard, ConnectionRegistry, CreateConnectionError, InitializeOutcome, ResponseRoute,
    INITIALIZE_TIMEOUT,
};
use super::*;

pub(crate) async fn handle_post(
    State(registry): State<Arc<ConnectionRegistry>>,
    request: Request<Body>,
) -> Response {
    if !content_type_is_json(&request) {
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Unsupported Media Type: Content-Type must be application/json",
        )
            .into_response();
    }

    let connection_id = header_value(&request, HEADER_CONNECTION_ID);
    let session_id = header_value(&request, HEADER_SESSION_ID);

    let body_bytes = match request.into_body().collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            error!("Failed to read request body: {}", e);
            return (StatusCode::BAD_REQUEST, "Failed to read request body").into_response();
        }
    };

    let json_message: Value = match serde_json::from_slice(&body_bytes) {
        Ok(v) => v,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("Invalid JSON: {}", e)).into_response();
        }
    };

    if json_message.is_array() {
        return (
            StatusCode::NOT_IMPLEMENTED,
            "Batch requests are not supported",
        )
            .into_response();
    }

    if is_initialize_request(&json_message) {
        return handle_initialize(registry, json_message).await;
    }

    let Some(connection_id) = connection_id else {
        return (
            StatusCode::BAD_REQUEST,
            "Bad Request: Acp-Connection-Id header required",
        )
            .into_response();
    };

    let Some(connection) = registry.get(&connection_id).await else {
        return (StatusCode::NOT_FOUND, "Unknown Acp-Connection-Id").into_response();
    };

    if let Some(method) = json_message.get("method").and_then(|m| m.as_str()) {
        if method_requires_session_header(method) && session_id.is_none() {
            return (
                StatusCode::BAD_REQUEST,
                "Bad Request: Acp-Session-Id header required for session-scoped methods",
            )
                .into_response();
        }
    }

    if !is_jsonrpc_request_with_id(&json_message)
        && !is_jsonrpc_notification(&json_message)
        && !is_jsonrpc_response(&json_message)
    {
        return (StatusCode::BAD_REQUEST, "Invalid JSON-RPC message").into_response();
    }

    if let Some(sid) = session_id.as_deref() {
        if !connection.ensure_session(sid).await {
            return session_limit_response(&connection_id);
        }
    }
    if is_jsonrpc_request_with_id(&json_message) {
        if let Some(id) = json_message.get("id") {
            let route = match session_id.as_deref() {
                Some(sid) => ResponseRoute::Session(sid.to_string()),
                None => ResponseRoute::Connection,
            };
            connection.record_pending_route(id.clone(), route).await;
        }
    }

    let message_str = serde_json::to_string(&json_message).unwrap();
    trace!(connection_id = %connection_id, payload = %message_str, "POST → agent");
    if connection.to_agent_tx.send(message_str).await.is_err() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to forward message to agent",
        )
            .into_response();
    }

    StatusCode::ACCEPTED.into_response()
}

/// Creates the connection, then holds it under a [`ConnectionGuard`] until the
/// agent has answered initialize. Every exit before the hand-off — send failure,
/// agent death, deadline, or the client hanging up mid-request — reclaims the
/// connection instead of leaving it in the registry.
async fn handle_initialize(registry: Arc<ConnectionRegistry>, json_message: Value) -> Response {
    let (connection_id, connection) = match registry.create_connection().await {
        Ok(pair) => pair,
        Err(CreateConnectionError::AtCapacity) => {
            let live_connections = registry.active_connections().await;
            warn!(
                live_connections,
                "Rejecting initialize: connection limit reached"
            );
            return capacity_response();
        }
        Err(CreateConnectionError::Agent(e)) => {
            error!("Failed to create connection: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to create connection",
            )
                .into_response();
        }
    };

    let guard = ConnectionGuard::new(registry, connection_id.clone());

    let message_str = serde_json::to_string(&json_message).unwrap();
    trace!(connection_id = %connection_id, payload = %message_str, "initialize → agent");
    if connection.to_agent_tx.send(message_str).await.is_err() {
        guard.close().await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to forward initialize to agent",
        )
            .into_response();
    }

    let init_response = match connection.await_initialize(INITIALIZE_TIMEOUT).await {
        InitializeOutcome::Response(msg) => msg,
        InitializeOutcome::TimedOut => {
            warn!(connection_id = %connection_id, "Agent did not respond to initialize within {:?}", INITIALIZE_TIMEOUT);
            guard.close().await;
            return (
                StatusCode::GATEWAY_TIMEOUT,
                "Agent did not respond to initialize",
            )
                .into_response();
        }
        InitializeOutcome::AgentClosed => {
            guard.close().await;
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Agent closed before initialize response",
            )
                .into_response();
        }
        InitializeOutcome::ReceiverConsumed => {
            guard.close().await;
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Initialize receiver already consumed",
            )
                .into_response();
        }
    };

    connection.start_router().await;
    guard.disarm();

    let mut response = (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, JSON_MIME_TYPE)],
        init_response,
    )
        .into_response();
    if let Ok(v) = HeaderValue::from_str(&connection_id) {
        response.headers_mut().insert(HEADER_CONNECTION_ID, v);
    }
    info!(connection_id = %connection_id, "Initialize complete");
    response
}

pub(crate) async fn handle_get(
    registry: Arc<ConnectionRegistry>,
    request: Request<Body>,
) -> Response {
    if !accepts_mime_type(&request, EVENT_STREAM_MIME_TYPE) {
        return (
            StatusCode::NOT_ACCEPTABLE,
            "Not Acceptable: Client must accept text/event-stream",
        )
            .into_response();
    }

    let Some(connection_id) = header_value(&request, HEADER_CONNECTION_ID) else {
        return (
            StatusCode::BAD_REQUEST,
            "Bad Request: Acp-Connection-Id header required",
        )
            .into_response();
    };

    let Some(connection) = registry.get(&connection_id).await else {
        return (StatusCode::NOT_FOUND, "Unknown Acp-Connection-Id").into_response();
    };

    let session_id = header_value(&request, HEADER_SESSION_ID);

    let subscription = match session_id.as_deref() {
        Some(sid) => {
            if !connection.ensure_session(sid).await {
                return session_limit_response(&connection_id);
            }
            connection.subscribe_session_stream(sid).await
        }
        None => Some(connection.subscribe_connection_stream().await),
    };

    let Some((replay, receiver)) = subscription else {
        return (StatusCode::NOT_FOUND, "Unknown Acp-Session-Id").into_response();
    };

    let sse = build_sse_stream(replay, receiver);

    let mut response = sse.into_response();
    if let Ok(v) = HeaderValue::from_str(&connection_id) {
        response.headers_mut().insert(HEADER_CONNECTION_ID, v);
    }
    if let Some(sid) = session_id {
        if let Ok(v) = HeaderValue::from_str(&sid) {
            response.headers_mut().insert(HEADER_SESSION_ID, v);
        }
    }
    response
}

fn capacity_response() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        "Service Unavailable: connection limit reached",
    )
        .into_response()
}

fn session_limit_response(connection_id: &str) -> Response {
    warn!(connection_id = %connection_id, "Rejecting request: session limit reached for connection");
    (
        StatusCode::SERVICE_UNAVAILABLE,
        "Service Unavailable: session limit reached for this connection",
    )
        .into_response()
}

fn build_sse_stream(
    replay: Vec<String>,
    mut receiver: broadcast::Receiver<String>,
) -> Sse<impl futures::Stream<Item = Result<axum::response::sse::Event, Infallible>>> {
    let stream = async_stream::stream! {
        for msg in replay {
            trace!(payload = %msg, "SSE → client (replay)");
            yield Ok::<_, Infallible>(axum::response::sse::Event::default().data(msg));
        }
        loop {
            match receiver.recv().await {
                Ok(msg) => {
                    trace!(payload = %msg, "SSE → client");
                    yield Ok::<_, Infallible>(axum::response::sse::Event::default().data(msg));
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    debug!("SSE subscriber lagged {} messages", n);
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text(""),
    )
}

pub(crate) async fn handle_delete(
    State(registry): State<Arc<ConnectionRegistry>>,
    request: Request<Body>,
) -> Response {
    let Some(connection_id) = header_value(&request, HEADER_CONNECTION_ID) else {
        return (
            StatusCode::BAD_REQUEST,
            "Bad Request: Acp-Connection-Id header required",
        )
            .into_response();
    };

    if !registry.close(&connection_id).await {
        return (StatusCode::NOT_FOUND, "Unknown Acp-Connection-Id").into_response();
    }
    info!(connection_id = %connection_id, "Connection terminated via DELETE");
    StatusCode::ACCEPTED.into_response()
}
