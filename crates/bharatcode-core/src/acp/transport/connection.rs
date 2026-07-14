//! Per-connection state. Server→client messages fan out to a connection-scoped
//! stream, a per-session stream for each active `sessionId`, and an
//! all-outbound stream consumed by WebSocket.
//!
//! Every resource an unauthenticated peer can cause the server to allocate is
//! bounded: the number of live connections (registry slots), the queue toward
//! the agent, the outbound fan-out buffers, the per-connection session streams,
//! and the pending response-route table. A connection that never completes
//! initialize is reclaimed by the initialize deadline; a connection whose client
//! vanishes is reclaimed by `ConnectionGuard`.

use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::Duration,
};

use serde_json::Value;
use tokio::sync::{broadcast, mpsc, Mutex, OwnedSemaphorePermit, RwLock, Semaphore};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{error, info, trace, warn};

use crate::acp::adapters::{ReceiverToAsyncRead, SenderToAsyncWrite};
use crate::acp::server_factory::AcpServer;

/// Global ceiling on live connections. Each connection owns an agent task, so
/// this is the primary guard against an unauthenticated peer exhausting memory
/// by opening connections it never uses.
const MAX_CONNECTIONS: usize = 64;

const TO_AGENT_QUEUE_CAPACITY: usize = 256;

const OUTBOUND_BROADCAST_CAPACITY: usize = 1024;

/// Buffers messages emitted before a subscriber attaches (e.g. session
/// notifications that land before the client opens the session GET stream).
const PRE_SUBSCRIBE_BUFFER_CAPACITY: usize = 1024;

/// Session streams are created from the client-supplied `Acp-Session-Id`, so
/// they are capped per connection. Each stream preallocates an
/// `OUTBOUND_BROADCAST_CAPACITY` ring, so this cap is what bounds a connection's
/// worst-case footprint, not just its entry count.
const MAX_SESSION_STREAMS: usize = 64;

/// Pending routes are keyed by client-supplied JSON-RPC ids and are only removed
/// when the agent answers. A client that fires requests the agent never answers
/// would otherwise grow this table without limit.
const MAX_PENDING_ROUTES: usize = 1024;

/// How long a client may hold a connection slot while waiting for the agent's
/// initialize response.
pub(crate) const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, thiserror::Error)]
pub(crate) enum CreateConnectionError {
    #[error("connection limit reached")]
    AtCapacity,
    #[error(transparent)]
    Agent(#[from] anyhow::Error),
}

#[derive(Debug)]
pub(crate) enum InitializeOutcome {
    Response(String),
    /// The agent dropped its writer before answering initialize.
    AgentClosed,
    /// The agent did not answer within [`INITIALIZE_TIMEOUT`].
    TimedOut,
    /// The router already owns the receiver; initialize was sent twice.
    ReceiverConsumed,
}

#[derive(Clone, Debug)]
pub(crate) enum ResponseRoute {
    Connection,
    Session(String),
}

struct OutboundStream {
    tx: broadcast::Sender<String>,
    pre_subscribe_buffer: Mutex<Option<VecDeque<String>>>,
}

impl OutboundStream {
    fn new() -> Self {
        let (tx, _) = broadcast::channel(OUTBOUND_BROADCAST_CAPACITY);
        Self {
            tx,
            pre_subscribe_buffer: Mutex::new(Some(VecDeque::new())),
        }
    }

    async fn push(&self, msg: String) {
        let mut guard = self.pre_subscribe_buffer.lock().await;
        match guard.as_mut() {
            Some(buf) => {
                if buf.len() >= PRE_SUBSCRIBE_BUFFER_CAPACITY {
                    warn!(
                        "Pre-subscribe buffer full ({} messages); dropping oldest",
                        PRE_SUBSCRIBE_BUFFER_CAPACITY
                    );
                    buf.pop_front();
                }
                buf.push_back(msg);
            }
            None => {
                drop(guard);
                let _ = self.tx.send(msg);
            }
        }
    }

    async fn subscribe_with_replay(&self) -> (Vec<String>, broadcast::Receiver<String>) {
        let mut guard = self.pre_subscribe_buffer.lock().await;
        let receiver = self.tx.subscribe();
        let replay = guard.take().map(Vec::from).unwrap_or_default();
        (replay, receiver)
    }
}

/// Response-route table with a bound. Insertion order is tracked separately so
/// that overflow evicts the oldest outstanding request rather than a random one.
#[derive(Default)]
struct PendingRoutes {
    routes: HashMap<Value, ResponseRoute>,
    order: VecDeque<Value>,
}

impl PendingRoutes {
    fn insert(&mut self, id: Value, route: ResponseRoute) {
        if self.routes.insert(id.clone(), route).is_none() {
            self.order.push_back(id);
        }

        while self.routes.len() > MAX_PENDING_ROUTES {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if self.routes.remove(&oldest).is_some() {
                warn!(
                    "Pending route table full ({} entries); dropping oldest request route",
                    MAX_PENDING_ROUTES
                );
            }
        }

        if self.order.len() > MAX_PENDING_ROUTES * 2 {
            let routes = &self.routes;
            self.order.retain(|id| routes.contains_key(id));
        }
    }

    fn remove(&mut self, id: &Value) -> Option<ResponseRoute> {
        self.routes.remove(id)
    }

    fn clear(&mut self) {
        self.routes.clear();
        self.order.clear();
    }
}

pub(crate) struct Connection {
    pub to_agent_tx: mpsc::Sender<String>,
    /// Consumed once by the initialize handler to read the synchronous
    /// initialize response, then by the router task.
    init_receiver: Mutex<Option<mpsc::UnboundedReceiver<String>>>,
    router_started: Mutex<bool>,
    agent_handle: tokio::task::JoinHandle<()>,
    router_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// Held for the connection's lifetime; released by [`Connection::shutdown`]
    /// so a reclaimed slot is immediately available to the next client.
    slot: Mutex<Option<OwnedSemaphorePermit>>,

    connection_stream: Arc<OutboundStream>,
    session_streams: Arc<RwLock<HashMap<String, Arc<OutboundStream>>>>,
    all_outbound: Arc<OutboundStream>,
    pending_routes: Arc<Mutex<PendingRoutes>>,
}

pub(crate) struct ConnectionRegistry {
    server: Arc<AcpServer>,
    connections: RwLock<HashMap<String, Arc<Connection>>>,
    slots: Arc<Semaphore>,
}

impl ConnectionRegistry {
    pub fn new(server: Arc<AcpServer>) -> Self {
        Self::with_capacity(server, MAX_CONNECTIONS)
    }

    fn with_capacity(server: Arc<AcpServer>, max_connections: usize) -> Self {
        Self {
            server,
            connections: RwLock::new(HashMap::new()),
            slots: Arc::new(Semaphore::new(max_connections)),
        }
    }

    /// Reserves a connection slot before any agent is spawned, so a burst of
    /// concurrent initializes cannot overshoot the cap while agents are being
    /// built.
    fn acquire_slot(&self) -> Result<OwnedSemaphorePermit, CreateConnectionError> {
        Arc::clone(&self.slots)
            .try_acquire_owned()
            .map_err(|_| CreateConnectionError::AtCapacity)
    }

    pub async fn create_connection(
        &self,
    ) -> Result<(String, Arc<Connection>), CreateConnectionError> {
        let slot = self.acquire_slot()?;

        let agent = self.server.create_agent().await?;
        let connection_id = uuid::Uuid::new_v4().to_string();

        let (to_agent_tx, to_agent_rx) = mpsc::channel::<String>(TO_AGENT_QUEUE_CAPACITY);
        let (from_agent_tx, from_agent_rx) = mpsc::unbounded_channel::<String>();

        let read_stream = ReceiverToAsyncRead::new(to_agent_rx);
        let write_stream = SenderToAsyncWrite::new(from_agent_tx);
        let fut =
            crate::acp::server::serve(agent, read_stream.compat(), write_stream.compat_write());

        let conn_id_for_task = connection_id.clone();
        let agent_handle = tokio::spawn(async move {
            if let Err(e) = fut.await {
                error!(connection_id = %conn_id_for_task, "ACP agent task error: {}", e);
            }
        });

        let connection = self
            .register(
                connection_id.clone(),
                slot,
                to_agent_tx,
                from_agent_rx,
                agent_handle,
            )
            .await;

        info!(connection_id = %connection_id, "Connection created");
        Ok((connection_id, connection))
    }

    async fn register(
        &self,
        connection_id: String,
        slot: OwnedSemaphorePermit,
        to_agent_tx: mpsc::Sender<String>,
        init_receiver: mpsc::UnboundedReceiver<String>,
        agent_handle: tokio::task::JoinHandle<()>,
    ) -> Arc<Connection> {
        let connection = Arc::new(Connection {
            to_agent_tx,
            init_receiver: Mutex::new(Some(init_receiver)),
            router_started: Mutex::new(false),
            agent_handle,
            router_handle: Mutex::new(None),
            slot: Mutex::new(Some(slot)),
            connection_stream: Arc::new(OutboundStream::new()),
            session_streams: Arc::new(RwLock::new(HashMap::new())),
            all_outbound: Arc::new(OutboundStream::new()),
            pending_routes: Arc::new(Mutex::new(PendingRoutes::default())),
        });

        self.connections
            .write()
            .await
            .insert(connection_id, connection.clone());

        connection
    }

    pub async fn get(&self, connection_id: &str) -> Option<Arc<Connection>> {
        self.connections.read().await.get(connection_id).cloned()
    }

    /// The only teardown path: removes the connection from the registry and
    /// shuts it down, so the registry entry and the slot are always released
    /// together. Returns false if the connection was already reclaimed.
    pub async fn close(&self, connection_id: &str) -> bool {
        let removed = self.connections.write().await.remove(connection_id);
        match removed {
            Some(connection) => {
                connection.shutdown().await;
                true
            }
            None => false,
        }
    }

    pub async fn active_connections(&self) -> usize {
        self.connections.read().await.len()
    }
}

/// Reclaims a connection if the task that owns it goes away before handing the
/// connection off — a client that disconnects mid-initialize, or a WebSocket
/// upgrade that never completes, would otherwise leave the registry entry, the
/// agent task, and the connection slot behind forever.
pub(crate) struct ConnectionGuard {
    registry: Arc<ConnectionRegistry>,
    connection_id: String,
    armed: bool,
}

impl ConnectionGuard {
    pub fn new(registry: Arc<ConnectionRegistry>, connection_id: String) -> Self {
        Self {
            registry,
            connection_id,
            armed: true,
        }
    }

    pub fn connection_id(&self) -> &str {
        &self.connection_id
    }

    pub async fn close(mut self) {
        self.armed = false;
        self.registry.close(&self.connection_id).await;
    }

    /// Hands the connection off to a client that will close it later.
    pub fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let registry = self.registry.clone();
        let connection_id = std::mem::take(&mut self.connection_id);
        runtime.spawn(async move {
            if registry.close(&connection_id).await {
                warn!(connection_id = %connection_id, "Reclaimed connection abandoned by client");
            }
        });
    }
}

impl Connection {
    /// Waits for the agent's initialize response under a deadline, so a wedged
    /// agent cannot pin a connection slot indefinitely.
    pub async fn await_initialize(&self, deadline: Duration) -> InitializeOutcome {
        let mut guard = self.init_receiver.lock().await;
        let Some(rx) = guard.as_mut() else {
            return InitializeOutcome::ReceiverConsumed;
        };

        match tokio::time::timeout(deadline, rx.recv()).await {
            Ok(Some(msg)) => InitializeOutcome::Response(msg),
            Ok(None) => InitializeOutcome::AgentClosed,
            Err(_) => InitializeOutcome::TimedOut,
        }
    }

    pub async fn start_router(self: &Arc<Self>) {
        let mut started = self.router_started.lock().await;
        if *started {
            return;
        }
        let Some(mut rx) = self.init_receiver.lock().await.take() else {
            return;
        };

        let me = self.clone();
        let handle = tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                me.route_outbound(msg).await;
            }
        });
        *self.router_handle.lock().await = Some(handle);
        *started = true;
    }

    async fn route_outbound(self: &Arc<Self>, msg: String) {
        self.all_outbound.push(msg.clone()).await;

        let parsed: Option<Value> = serde_json::from_str(&msg).ok();
        let target = match parsed.as_ref() {
            Some(v) => self.classify(v).await,
            None => Target::Connection,
        };

        match target {
            Target::Connection => {
                trace!(target = "connection", "→ connection-scoped stream");
                self.connection_stream.push(msg).await;
            }
            Target::Session(sid) => {
                trace!(target = %sid, "→ session-scoped stream");
                match self.get_or_create_session_stream(&sid).await {
                    Some(stream) => stream.push(msg).await,
                    None => self.connection_stream.push(msg).await,
                }
            }
        }
    }

    async fn classify(self: &Arc<Self>, v: &Value) -> Target {
        let has_method = v.get("method").is_some();
        let has_id = v.get("id").is_some();
        let has_result_or_error = v.get("result").is_some() || v.get("error").is_some();

        if has_method {
            if let Some(sid) = extract_session_id_from_params(v) {
                return Target::Session(sid);
            }
            return Target::Connection;
        }

        if has_id && has_result_or_error {
            let id = v.get("id").cloned().unwrap_or(Value::Null);
            let route = self.pending_routes.lock().await.remove(&id);
            return match route {
                Some(ResponseRoute::Session(sid)) => Target::Session(sid),
                Some(ResponseRoute::Connection) | None => Target::Connection,
            };
        }

        Target::Connection
    }

    pub async fn record_pending_route(&self, id: Value, route: ResponseRoute) {
        if id.is_null() {
            return;
        }
        self.pending_routes.lock().await.insert(id, route);
    }

    pub async fn subscribe_connection_stream(&self) -> (Vec<String>, broadcast::Receiver<String>) {
        self.connection_stream.subscribe_with_replay().await
    }

    pub async fn subscribe_session_stream(
        &self,
        session_id: &str,
    ) -> Option<(Vec<String>, broadcast::Receiver<String>)> {
        let stream = self.session_streams.read().await.get(session_id).cloned()?;
        Some(stream.subscribe_with_replay().await)
    }

    /// Returns false when the connection already holds [`MAX_SESSION_STREAMS`]
    /// distinct sessions and `session_id` is a new one.
    pub async fn ensure_session(&self, session_id: &str) -> bool {
        self.get_or_create_session_stream(session_id)
            .await
            .is_some()
    }

    async fn get_or_create_session_stream(&self, session_id: &str) -> Option<Arc<OutboundStream>> {
        if let Some(s) = self.session_streams.read().await.get(session_id) {
            return Some(s.clone());
        }

        let mut streams = self.session_streams.write().await;
        if let Some(s) = streams.get(session_id) {
            return Some(s.clone());
        }
        if streams.len() >= MAX_SESSION_STREAMS {
            warn!(
                session_id = %session_id,
                "Refusing new session stream; connection is at the {} session limit",
                MAX_SESSION_STREAMS
            );
            return None;
        }

        Some(
            streams
                .entry(session_id.to_string())
                .or_insert_with(|| Arc::new(OutboundStream::new()))
                .clone(),
        )
    }

    pub async fn subscribe_all_outbound(&self) -> (Vec<String>, broadcast::Receiver<String>) {
        self.all_outbound.subscribe_with_replay().await
    }

    /// Idempotent. Dropping the initialize receiver breaks the agent's writer,
    /// so an agent that outlives its abort cannot keep queueing output.
    pub async fn shutdown(&self) {
        self.agent_handle.abort();
        if let Some(h) = self.router_handle.lock().await.take() {
            h.abort();
        }
        self.init_receiver.lock().await.take();
        self.session_streams.write().await.clear();
        self.pending_routes.lock().await.clear();
        self.slot.lock().await.take();
    }
}

#[derive(Debug)]
enum Target {
    Connection,
    Session(String),
}

fn extract_session_id_from_params(v: &Value) -> Option<String> {
    v.get("params")
        .and_then(|p| p.get("sessionId"))
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::server_factory::AcpServerFactoryConfig;
    use crate::agents::GoosePlatform;
    use std::path::PathBuf;
    use tokio::time::timeout;

    /// The registry only touches `AcpServer` inside `create_connection`, which
    /// these tests bypass via `register` — so a config-only server is enough.
    fn test_registry(max_connections: usize) -> Arc<ConnectionRegistry> {
        let server = Arc::new(AcpServer::new(AcpServerFactoryConfig {
            builtins: Vec::new(),
            data_dir: PathBuf::from("."),
            config_dir: PathBuf::from("."),
            goose_platform: GoosePlatform::GooseCli,
            additional_source_roots: Vec::new(),
        }));
        Arc::new(ConnectionRegistry::with_capacity(server, max_connections))
    }

    /// Registers a connection whose "agent" never reads its input and never
    /// writes unless the test sends on the returned handle.
    async fn register_fake(
        registry: &Arc<ConnectionRegistry>,
        connection_id: &str,
    ) -> Result<(Arc<Connection>, mpsc::UnboundedSender<String>), CreateConnectionError> {
        let slot = registry.acquire_slot()?;

        let (to_agent_tx, to_agent_rx) = mpsc::channel::<String>(TO_AGENT_QUEUE_CAPACITY);
        let (from_agent_tx, from_agent_rx) = mpsc::unbounded_channel::<String>();

        let agent_handle = tokio::spawn(async move {
            let _hold_input_open = to_agent_rx;
            std::future::pending::<()>().await;
        });

        let connection = registry
            .register(
                connection_id.to_string(),
                slot,
                to_agent_tx,
                from_agent_rx,
                agent_handle,
            )
            .await;

        Ok((connection, from_agent_tx))
    }

    async fn fake_connection() -> (Arc<Connection>, mpsc::UnboundedSender<String>) {
        let registry = test_registry(MAX_CONNECTIONS);
        register_fake(&registry, "conn").await.unwrap()
    }

    /// Waits for the registry to drain without depending on when a `Drop`-spawned
    /// cleanup task happens to be polled.
    async fn wait_for_active_connections(registry: &Arc<ConnectionRegistry>, expected: usize) {
        let settled = timeout(Duration::from_secs(5), async {
            while registry.active_connections().await != expected {
                tokio::task::yield_now().await;
            }
        })
        .await;
        assert!(
            settled.is_ok(),
            "registry did not settle at {} connections (still {})",
            expected,
            registry.active_connections().await
        );
    }

    #[tokio::test]
    async fn buffers_connection_scoped_messages_before_first_subscribe() {
        let (conn, agent_tx) = fake_connection().await;
        conn.start_router().await;

        agent_tx
            .send(r#"{"id":1,"result":{"capabilities":{}}}"#.to_string())
            .unwrap();

        tokio::time::sleep(Duration::from_millis(20)).await;

        let (replay, _rx) = conn.subscribe_connection_stream().await;
        assert_eq!(replay.len(), 1);
        assert!(replay[0].contains("\"capabilities\""));

        conn.shutdown().await;
    }

    #[tokio::test]
    async fn routes_session_scoped_notification_to_session_stream() {
        let (conn, agent_tx) = fake_connection().await;
        conn.start_router().await;

        assert!(conn.ensure_session("sess_abc").await);

        let (_, mut rx) = conn.subscribe_session_stream("sess_abc").await.unwrap();
        agent_tx
            .send(
                r#"{"method":"session/update","params":{"sessionId":"sess_abc","update":{}}}"#
                    .to_string(),
            )
            .unwrap();

        let got = timeout(Duration::from_secs(1), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(got.contains("session/update"));

        let (replay, _) = conn.subscribe_connection_stream().await;
        assert!(
            replay.is_empty(),
            "connection stream should not have session-scoped messages"
        );

        conn.shutdown().await;
    }

    #[tokio::test]
    async fn routes_response_using_pending_route_table() {
        let (conn, agent_tx) = fake_connection().await;
        conn.start_router().await;

        assert!(conn.ensure_session("sess_xyz").await);
        conn.record_pending_route(
            Value::from(42),
            ResponseRoute::Session("sess_xyz".to_string()),
        )
        .await;

        let (_, mut rx) = conn.subscribe_session_stream("sess_xyz").await.unwrap();
        agent_tx
            .send(r#"{"id":42,"result":{"stopReason":"end_turn"}}"#.to_string())
            .unwrap();

        let got = timeout(Duration::from_secs(1), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(got.contains("\"stopReason\""));

        conn.shutdown().await;
    }

    #[tokio::test]
    async fn websocket_all_outbound_sees_everything() {
        let (conn, agent_tx) = fake_connection().await;
        conn.start_router().await;

        agent_tx
            .send(r#"{"id":1,"result":{}}"#.to_string())
            .unwrap();
        agent_tx
            .send(r#"{"method":"session/update","params":{"sessionId":"s1"}}"#.to_string())
            .unwrap();

        tokio::time::sleep(Duration::from_millis(20)).await;

        let (replay, _all_rx) = conn.subscribe_all_outbound().await;
        assert_eq!(replay.len(), 2);
        assert!(replay[0].contains("\"id\":1"));
        assert!(replay[1].contains("session/update"));

        conn.shutdown().await;
    }

    #[tokio::test]
    async fn unknown_session_subscribe_returns_none() {
        let (conn, _agent_tx) = fake_connection().await;
        conn.start_router().await;

        assert!(conn.subscribe_session_stream("nope").await.is_none());

        conn.shutdown().await;
    }

    #[tokio::test]
    async fn pre_subscribe_buffer_is_bounded() {
        let (conn, agent_tx) = fake_connection().await;
        conn.start_router().await;

        for i in 0..(PRE_SUBSCRIBE_BUFFER_CAPACITY + 50) {
            agent_tx
                .send(format!(r#"{{"id":{},"result":{{}}}}"#, i))
                .unwrap();
        }

        tokio::time::sleep(Duration::from_millis(50)).await;

        let (replay, _rx) = conn.subscribe_connection_stream().await;
        assert_eq!(replay.len(), PRE_SUBSCRIBE_BUFFER_CAPACITY);

        conn.shutdown().await;
    }

    #[tokio::test(start_paused = true)]
    async fn initialize_times_out_when_agent_never_responds() {
        let registry = test_registry(4);
        let (conn, _agent_tx) = register_fake(&registry, "wedged").await.unwrap();

        let outcome = conn.await_initialize(INITIALIZE_TIMEOUT).await;
        assert!(
            matches!(outcome, InitializeOutcome::TimedOut),
            "expected TimedOut, got {:?}",
            outcome
        );

        assert!(registry.close("wedged").await);
        assert_eq!(registry.active_connections().await, 0);
        assert_eq!(registry.slots.available_permits(), 4);
    }

    #[tokio::test]
    async fn initialize_reports_agent_closed_when_agent_disconnects_first() {
        let registry = test_registry(4);
        let (conn, agent_tx) = register_fake(&registry, "gone").await.unwrap();

        drop(agent_tx);

        let outcome = conn.await_initialize(INITIALIZE_TIMEOUT).await;
        assert!(
            matches!(outcome, InitializeOutcome::AgentClosed),
            "expected AgentClosed, got {:?}",
            outcome
        );

        assert!(registry.close("gone").await);
        assert_eq!(registry.active_connections().await, 0);
        assert_eq!(registry.slots.available_permits(), 4);
    }

    #[tokio::test]
    async fn client_disconnect_before_initialize_reclaims_the_connection() {
        let registry = test_registry(2);
        let (_conn, _agent_tx) = register_fake(&registry, "abandoned").await.unwrap();
        assert_eq!(registry.active_connections().await, 1);

        drop(ConnectionGuard::new(
            registry.clone(),
            "abandoned".to_string(),
        ));

        wait_for_active_connections(&registry, 0).await;
        assert_eq!(registry.slots.available_permits(), 2);
    }

    #[tokio::test]
    async fn handed_off_connection_survives_its_guard() {
        let registry = test_registry(2);
        let (_conn, _agent_tx) = register_fake(&registry, "live").await.unwrap();

        ConnectionGuard::new(registry.clone(), "live".to_string()).disarm();

        tokio::task::yield_now().await;
        assert_eq!(registry.active_connections().await, 1);
        assert!(registry.get("live").await.is_some());
    }

    #[tokio::test]
    async fn connection_cap_is_enforced_and_slots_return_to_baseline() {
        let registry = test_registry(3);

        for i in 0..3 {
            register_fake(&registry, &format!("conn-{}", i))
                .await
                .unwrap();
        }
        assert_eq!(registry.active_connections().await, 3);
        assert_eq!(registry.slots.available_permits(), 0);

        let rejected = register_fake(&registry, "conn-overflow").await;
        assert!(
            matches!(rejected, Err(CreateConnectionError::AtCapacity)),
            "expected AtCapacity once the cap is reached"
        );
        assert_eq!(
            registry.active_connections().await,
            3,
            "a rejected connection must not be registered"
        );

        assert!(registry.close("conn-0").await);
        assert_eq!(registry.slots.available_permits(), 1);

        register_fake(&registry, "conn-reused").await.unwrap();
        assert_eq!(registry.active_connections().await, 3);

        for id in ["conn-1", "conn-2", "conn-reused"] {
            assert!(registry.close(id).await);
        }
        assert_eq!(registry.active_connections().await, 0);
        assert_eq!(registry.slots.available_permits(), 3);
    }

    #[tokio::test]
    async fn closing_an_unknown_connection_is_not_an_error() {
        let registry = test_registry(1);
        assert!(!registry.close("never-existed").await);
        assert_eq!(registry.slots.available_permits(), 1);
    }

    #[tokio::test]
    async fn to_agent_queue_applies_backpressure_when_the_agent_stalls() {
        let (conn, _agent_tx) = fake_connection().await;

        for i in 0..TO_AGENT_QUEUE_CAPACITY {
            conn.to_agent_tx
                .try_send(format!(r#"{{"id":{}}}"#, i))
                .expect("queue accepts messages up to its capacity");
        }

        assert!(
            matches!(
                conn.to_agent_tx.try_send("overflow".to_string()),
                Err(mpsc::error::TrySendError::Full(_))
            ),
            "a stalled agent must apply backpressure, not buffer without limit"
        );

        assert!(
            timeout(
                Duration::from_millis(50),
                conn.to_agent_tx.send("blocked".to_string())
            )
            .await
            .is_err(),
            "a blocking send must not succeed while the queue is full"
        );

        conn.shutdown().await;
    }

    #[tokio::test]
    async fn outbound_stream_drops_the_slowest_subscriber_rather_than_growing() {
        let stream = OutboundStream::new();
        let (_replay, mut rx) = stream.subscribe_with_replay().await;

        let overflow = 10;
        for i in 0..(OUTBOUND_BROADCAST_CAPACITY + overflow) {
            stream.push(format!(r#"{{"seq":{}}}"#, i)).await;
        }

        assert!(
            matches!(
                rx.try_recv(),
                Err(broadcast::error::TryRecvError::Lagged(n)) if n == overflow as u64
            ),
            "a subscriber that never reads must lag, not force unbounded buffering"
        );
    }

    #[tokio::test]
    async fn session_streams_are_capped_per_connection() {
        let (conn, _agent_tx) = fake_connection().await;

        for i in 0..MAX_SESSION_STREAMS {
            assert!(conn.ensure_session(&format!("sess-{}", i)).await);
        }

        assert!(
            !conn.ensure_session("sess-overflow").await,
            "a new session past the cap must be refused"
        );
        assert!(
            conn.ensure_session("sess-0").await,
            "an existing session must still resolve at the cap"
        );

        conn.shutdown().await;
    }

    #[tokio::test]
    async fn pending_routes_evict_the_oldest_unanswered_request() {
        let (conn, _agent_tx) = fake_connection().await;

        for i in 0..(MAX_PENDING_ROUTES + 5) {
            conn.record_pending_route(Value::from(i as i64), ResponseRoute::Connection)
                .await;
        }

        let mut routes = conn.pending_routes.lock().await;
        assert_eq!(routes.routes.len(), MAX_PENDING_ROUTES);
        assert!(
            routes.remove(&Value::from(0i64)).is_none(),
            "the oldest route should have been evicted"
        );
        assert!(
            routes
                .remove(&Value::from((MAX_PENDING_ROUTES + 4) as i64))
                .is_some(),
            "the newest route should be retained"
        );
        drop(routes);

        conn.shutdown().await;
    }

    #[tokio::test]
    async fn shutdown_releases_the_slot_even_if_the_connection_arc_outlives_it() {
        let registry = test_registry(1);
        let (conn, _agent_tx) = register_fake(&registry, "lingering").await.unwrap();
        conn.start_router().await;

        assert!(registry.close("lingering").await);

        assert_eq!(
            registry.slots.available_permits(),
            1,
            "the slot must be freed on shutdown, not on the last Arc drop"
        );
        register_fake(&registry, "next").await.unwrap();

        drop(conn);
    }
}
