use std::sync::Arc;

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use futures::{SinkExt, StreamExt};
use tracing::{debug, error, info, trace, warn};

use super::connection::{Connection, ConnectionGuard, ConnectionRegistry, CreateConnectionError};
use super::HEADER_CONNECTION_ID;

pub(crate) async fn handle_ws_upgrade(
    registry: Arc<ConnectionRegistry>,
    ws: WebSocketUpgrade,
) -> Response {
    let (connection_id, connection) = match registry.create_connection().await {
        Ok(pair) => pair,
        Err(CreateConnectionError::AtCapacity) => {
            let live_connections = registry.active_connections().await;
            warn!(
                live_connections,
                "Rejecting WebSocket upgrade: connection limit reached"
            );
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "Service Unavailable: connection limit reached",
            )
                .into_response();
        }
        Err(CreateConnectionError::Agent(e)) => {
            error!("Failed to create WebSocket connection: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to create WebSocket connection",
            )
                .into_response();
        }
    };

    connection.start_router().await;

    // The guard rides into the upgrade callback: if the upgrade never completes,
    // the callback is dropped and the connection is reclaimed rather than
    // stranded in the registry with a live agent task.
    let guard = ConnectionGuard::new(registry, connection_id.clone());
    let mut response = ws.on_upgrade(move |socket| run_ws(socket, guard, connection));

    if let Ok(v) = HeaderValue::from_str(&connection_id) {
        response.headers_mut().insert(HEADER_CONNECTION_ID, v);
    }
    info!(connection_id = %connection_id, "WebSocket connection created");
    response
}

async fn run_ws(socket: WebSocket, guard: ConnectionGuard, connection: Arc<Connection>) {
    let connection_id = guard.connection_id().to_string();
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (replay, mut outbound_rx) = connection.subscribe_all_outbound().await;

    debug!(connection_id = %connection_id, "Starting WebSocket message loop");

    for text in replay {
        trace!(connection_id = %connection_id, payload = %text, "Agent → Client (replay): {} bytes", text.len());
        if ws_tx.send(Message::Text(text.into())).await.is_err() {
            error!(connection_id = %connection_id, "WebSocket send failed during replay");
            guard.close().await;
            return;
        }
    }

    loop {
        tokio::select! {
            msg_result = ws_rx.next() => {
                match msg_result {
                    Some(Ok(Message::Text(text))) => {
                        let text_str = text.to_string();
                        trace!(connection_id = %connection_id, payload = %text_str, "Client → Agent: {} bytes", text_str.len());
                        if connection.to_agent_tx.send(text_str).await.is_err() {
                            error!(connection_id = %connection_id, "Agent channel closed");
                            break;
                        }
                    }
                    Some(Ok(Message::Close(frame))) => {
                        debug!(connection_id = %connection_id, "Client closed connection: {:?}", frame);
                        break;
                    }
                    Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => continue,
                    Some(Ok(Message::Binary(_))) => {
                        warn!(connection_id = %connection_id, "Ignoring binary message (ACP uses text)");
                        continue;
                    }
                    Some(Err(e)) => {
                        error!(connection_id = %connection_id, "WebSocket error: {}", e);
                        break;
                    }
                    None => break,
                }
            }

            recv = outbound_rx.recv() => {
                match recv {
                    Ok(text) => {
                        trace!(connection_id = %connection_id, payload = %text, "Agent → Client: {} bytes", text.len());
                        if ws_tx.send(Message::Text(text.into())).await.is_err() {
                            error!(connection_id = %connection_id, "WebSocket send failed");
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(connection_id = %connection_id, "WebSocket lagged {} messages", n);
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }

    debug!(connection_id = %connection_id, "Cleaning up WebSocket connection");
    guard.close().await;
}
