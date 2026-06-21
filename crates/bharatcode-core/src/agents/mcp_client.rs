use crate::action_required_manager::{ActionRequiredManager, ElicitationOutcome};
use crate::agents::tool_execution::ToolCallContext;
use crate::agents::types::SharedProvider;
use crate::session_context::{SESSION_ID_HEADER, WORKING_DIR_HEADER};
use rmcp::model::{
    CreateElicitationRequestParams, CreateElicitationResult, ElicitationAction, ErrorCode,
    ExtensionCapabilities, Extensions, JsonObject, ListRootsResult, LoggingMessageNotification,
    Meta, Root, SamplingMessageContent,
};
/// MCP client implementation for BharatCode
use rmcp::{
    model::{
        CallToolRequestParams, CallToolResult, CancelledNotificationParam, ClientCapabilities,
        ClientInfo, ClientRequest, CreateMessageRequestParams, CreateMessageResult,
        GetPromptRequestParams, GetPromptResult, Implementation, InitializeRequestParams,
        InitializeResult, ListPromptsResult, ListResourcesResult, ListToolsResult, Notification,
        PaginatedRequestParams, ProtocolVersion, ReadResourceRequestParams, ReadResourceResult,
        Request, RequestId, RequestOptionalParam, Role, SamplingMessage, ServerNotification,
        ServerResult,
    },
    service::{
        ClientInitializeError, PeerRequestOptions, RequestContext, RequestHandle, RunningService,
        ServiceRole,
    },
    transport::IntoTransport,
    ClientHandler, ErrorData, Peer, RoleClient, ServiceError, ServiceExt,
};
use serde_json::Value;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{
    mpsc::{self, Sender},
    Mutex,
};
use tokio_util::sync::CancellationToken;

pub type BoxError = Box<dyn std::error::Error + Sync + Send>;

pub type Error = rmcp::ServiceError;

const MCP_APPS_UI_EXTENSION_ID: &str = "io.modelcontextprotocol/ui";
const MCP_APPS_UI_MIME_TYPE: &str = "text/html;profile=mcp-app";

fn default_mcp_apps_ui_extensions() -> ExtensionCapabilities {
    let mut extensions = ExtensionCapabilities::new();
    let mut ui_extension_settings = JsonObject::new();
    ui_extension_settings.insert(
        "mimeTypes".to_string(),
        serde_json::json!([MCP_APPS_UI_MIME_TYPE]),
    );
    extensions.insert(MCP_APPS_UI_EXTENSION_ID.to_string(), ui_extension_settings);
    extensions
}

#[derive(Debug, Clone, Default)]
pub struct GooseMcpHostInfo {
    pub explicit_extensions: bool,
    pub extensions: ExtensionCapabilities,
    pub client_name: Option<String>,
    pub client_version: Option<String>,
}

impl GooseMcpHostInfo {
    pub fn mcpui_enabled(&self) -> bool {
        self.extensions.contains_key(MCP_APPS_UI_EXTENSION_ID)
    }
}

#[async_trait::async_trait]
pub trait McpClientTrait: Send + Sync {
    async fn list_tools(
        &self,
        session_id: &str,
        next_cursor: Option<String>,
        cancel_token: CancellationToken,
    ) -> Result<ListToolsResult, Error>;

    async fn call_tool(
        &self,
        ctx: &ToolCallContext,
        name: &str,
        arguments: Option<JsonObject>,
        cancel_token: CancellationToken,
    ) -> Result<CallToolResult, Error>;

    fn get_info(&self) -> Option<&InitializeResult>;

    /// Return the extension's current instructions. The default reads from
    /// `get_info()`, but platform extensions can override this to provide
    /// dynamically computed instructions (e.g. freshly discovered skills).
    fn get_instructions(&self) -> Option<String> {
        self.get_info().and_then(|info| info.instructions.clone())
    }

    /// User-facing connection status for this client, derived from the MCP
    /// initialization handshake. Built-in extensions that do not perform a
    /// handshake report [`McpConnectionStatus::NotInitialized`].
    fn connection_status(&self) -> McpConnectionStatus {
        match self.get_info() {
            Some(info) => McpConnectionStatus::from_info(info),
            None => McpConnectionStatus::NotInitialized,
        }
    }

    async fn list_resources(
        &self,
        _session_id: &str,
        _next_cursor: Option<String>,
        _cancel_token: CancellationToken,
    ) -> Result<ListResourcesResult, Error> {
        Err(Error::TransportClosed)
    }

    async fn read_resource(
        &self,
        _session_id: &str,
        _uri: &str,
        _cancel_token: CancellationToken,
    ) -> Result<ReadResourceResult, Error> {
        Err(Error::TransportClosed)
    }

    async fn list_prompts(
        &self,
        _session_id: &str,
        _next_cursor: Option<String>,
        _cancel_token: CancellationToken,
    ) -> Result<ListPromptsResult, Error> {
        Err(Error::TransportClosed)
    }

    async fn get_prompt(
        &self,
        _session_id: &str,
        _name: &str,
        _arguments: Value,
        _cancel_token: CancellationToken,
    ) -> Result<GetPromptResult, Error> {
        Err(Error::TransportClosed)
    }

    async fn subscribe(&self) -> mpsc::Receiver<ServerNotification> {
        mpsc::channel(1).1
    }

    async fn get_moim(&self, _session_id: &str) -> Option<String> {
        None
    }

    async fn update_working_dir(&self, _new_dir: PathBuf) -> Result<(), Error> {
        Ok(())
    }
}

pub struct GooseClient {
    notification_handlers: Arc<Mutex<Vec<Sender<ServerNotification>>>>,
    provider: SharedProvider,
    session_id: Mutex<Option<String>>,
    client_name: String,
    capabilities: GooseMcpClientCapabilities,
    working_dir: Arc<tokio::sync::RwLock<PathBuf>>,
}

impl GooseClient {
    pub fn new(
        handlers: Arc<Mutex<Vec<Sender<ServerNotification>>>>,
        provider: SharedProvider,
        client_name: String,
        capabilities: GooseMcpClientCapabilities,
        working_dir: PathBuf,
    ) -> Self {
        GooseClient {
            notification_handlers: handlers,
            provider,
            session_id: Mutex::new(None),
            client_name,
            capabilities,
            working_dir: Arc::new(tokio::sync::RwLock::new(working_dir)),
        }
    }

    pub fn shared_working_dir(&self) -> Arc<tokio::sync::RwLock<PathBuf>> {
        self.working_dir.clone()
    }

    async fn set_session_id(&self, session_id: &str) {
        let mut slot = self.session_id.lock().await;
        assert!(
            slot.as_deref().is_none_or(|s| s == session_id),
            "McpClient received requests from different sessions"
        );
        *slot = Some(session_id.to_string());
    }

    async fn current_session_id(&self) -> Option<String> {
        self.session_id.lock().await.clone()
    }

    async fn resolve_session_id(&self, extensions: &Extensions) -> Option<String> {
        // Prefer explicit MCP metadata, then the active request scope.
        let current_session_id = self.current_session_id().await;
        Self::session_id_from_extensions(extensions).or(current_session_id)
    }

    fn session_id_from_extensions(extensions: &Extensions) -> Option<String> {
        let meta = extensions.get::<Meta>()?;
        meta.0
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(SESSION_ID_HEADER))
            .and_then(|(_, value)| value.as_str())
            .map(|value| value.to_string())
    }

    fn resolved_extensions(&self) -> ExtensionCapabilities {
        if let Some(host_info) = &self.capabilities.host_info {
            if host_info.explicit_extensions {
                return host_info.extensions.clone();
            }
        }

        if self.capabilities.mcpui {
            return default_mcp_apps_ui_extensions();
        }

        ExtensionCapabilities::new()
    }

    fn resolved_client_info(&self) -> Implementation {
        let name = self
            .capabilities
            .host_info
            .as_ref()
            .and_then(|host_info| host_info.client_name.clone())
            .unwrap_or_else(|| self.client_name.clone());
        let version = self
            .capabilities
            .host_info
            .as_ref()
            .and_then(|host_info| host_info.client_version.clone())
            .unwrap_or_else(|| {
                std::env::var("BHARATCODE_MCP_CLIENT_VERSION")
                    .unwrap_or(env!("CARGO_PKG_VERSION").to_owned())
            });

        Implementation::new(name, version)
    }
}

fn working_dir_roots(dir: &std::path::Path) -> ListRootsResult {
    let uri = url::Url::from_file_path(dir)
        .map(|u| u.to_string())
        .unwrap_or_else(|()| format!("file://{}", dir.display()));
    ListRootsResult::new(vec![Root::new(uri).with_name("working_directory")])
}

impl ClientHandler for GooseClient {
    async fn list_roots(
        &self,
        _context: RequestContext<RoleClient>,
    ) -> Result<ListRootsResult, ErrorData> {
        Ok(working_dir_roots(&self.working_dir.read().await))
    }

    async fn on_progress(
        &self,
        params: rmcp::model::ProgressNotificationParam,
        context: rmcp::service::NotificationContext<rmcp::RoleClient>,
    ) {
        self.notification_handlers
            .lock()
            .await
            .iter()
            .for_each(|handler| {
                let mut not = Notification::new(params.clone());
                not.extensions = context.extensions.clone();
                let _ = handler.try_send(ServerNotification::ProgressNotification(not));
            });
    }

    async fn on_logging_message(
        &self,
        params: rmcp::model::LoggingMessageNotificationParam,
        context: rmcp::service::NotificationContext<rmcp::RoleClient>,
    ) {
        self.notification_handlers
            .lock()
            .await
            .iter()
            .for_each(|handler| {
                let mut notification = LoggingMessageNotification::new(params.clone());
                notification.extensions = context.extensions.clone();
                let _ =
                    handler.try_send(ServerNotification::LoggingMessageNotification(notification));
            });
    }

    async fn create_message(
        &self,
        params: CreateMessageRequestParams,
        context: RequestContext<RoleClient>,
    ) -> Result<CreateMessageResult, ErrorData> {
        let provider = self
            .provider
            .lock()
            .await
            .as_ref()
            .ok_or(ErrorData::new(
                ErrorCode::INTERNAL_ERROR,
                "Could not use provider",
                None,
            ))?
            .clone();

        // Prefer explicit MCP metadata, then the active request scope.
        let session_id = self.resolve_session_id(&context.extensions).await;

        let provider_ready_messages: Vec<crate::conversation::message::Message> = params
            .messages
            .iter()
            .map(|msg| {
                let base = match msg.role {
                    Role::User => crate::conversation::message::Message::user(),
                    Role::Assistant => crate::conversation::message::Message::assistant(),
                };

                match msg.content.first().and_then(|c| c.as_text()) {
                    Some(text) => base.with_text(&text.text),
                    None => base,
                }
            })
            .collect();

        let system_prompt = params
            .system_prompt
            .as_deref()
            .unwrap_or("You are a general-purpose AI agent called bharatcode");

        let model_config = provider.get_model_config();
        let (response, usage) = provider
            .complete(
                &model_config,
                session_id.as_deref().unwrap_or(""),
                system_prompt,
                &provider_ready_messages,
                &[],
            )
            .await
            .map_err(|e| {
                ErrorData::new(
                    ErrorCode::INTERNAL_ERROR,
                    "Unexpected error while completing the prompt",
                    Some(Value::from(e.to_string())),
                )
            })?;

        Ok(CreateMessageResult::new(
            SamplingMessage::new(
                Role::Assistant,
                if let Some(content) = response.content.first() {
                    match content {
                        crate::conversation::message::MessageContent::Text(text) => {
                            SamplingMessageContent::text(&text.text)
                        }
                        crate::conversation::message::MessageContent::Image(img) => {
                            SamplingMessageContent::Image(rmcp::model::RawImageContent {
                                data: img.data.clone(),
                                mime_type: img.mime_type.clone(),
                                meta: None,
                            })
                        }
                        _ => SamplingMessageContent::text(""),
                    }
                } else {
                    SamplingMessageContent::text("")
                },
            ),
            usage.model,
        )
        .with_stop_reason(CreateMessageResult::STOP_REASON_END_TURN))
    }

    async fn create_elicitation(
        &self,
        request: CreateElicitationRequestParams,
        context: RequestContext<RoleClient>,
    ) -> Result<CreateElicitationResult, ErrorData> {
        let session_id = self
            .resolve_session_id(&context.extensions)
            .await
            .ok_or_else(|| {
                ErrorData::new(
                    ErrorCode::INTERNAL_ERROR,
                    "Could not resolve session id for elicitation request",
                    None,
                )
            })?;

        let (message, schema_value) = match &request {
            CreateElicitationRequestParams::FormElicitationParams {
                message,
                requested_schema,
                ..
            } => {
                let schema_value = serde_json::to_value(requested_schema).map_err(|e| {
                    ErrorData::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Failed to serialize elicitation schema: {}", e),
                        None,
                    )
                })?;
                (message.clone(), schema_value)
            }
            CreateElicitationRequestParams::UrlElicitationParams { message, url, .. } => {
                (message.clone(), serde_json::json!({ "url": url }))
            }
        };

        ActionRequiredManager::global()
            .request_and_wait(session_id, message, schema_value, Duration::from_secs(300))
            .await
            .map(|response| match response {
                ElicitationOutcome::Accept(user_data) => {
                    CreateElicitationResult::new(ElicitationAction::Accept).with_content(user_data)
                }
                ElicitationOutcome::Decline => {
                    CreateElicitationResult::new(ElicitationAction::Decline)
                }
                ElicitationOutcome::Cancel => {
                    CreateElicitationResult::new(ElicitationAction::Cancel)
                }
            })
            .map_err(|e| {
                ErrorData::new(
                    ErrorCode::INTERNAL_ERROR,
                    format!("Elicitation request timed out or failed: {}", e),
                    None,
                )
            })
    }

    fn get_info(&self) -> ClientInfo {
        let extensions = self.resolved_extensions();

        InitializeRequestParams::new(
            ClientCapabilities::builder()
                .enable_roots()
                .enable_extensions_with(extensions)
                .enable_sampling()
                .enable_elicitation()
                .build(),
            self.resolved_client_info(),
        )
        .with_protocol_version(ProtocolVersion::V_2025_03_26)
    }
}

#[derive(Debug, Clone)]
pub struct GooseMcpClientCapabilities {
    pub mcpui: bool,
    pub host_info: Option<GooseMcpHostInfo>,
}

/// The MCP client is the interface for MCP operations.
pub struct McpClient {
    client: Mutex<RunningService<RoleClient, GooseClient>>,
    notification_subscribers: Arc<Mutex<Vec<mpsc::Sender<ServerNotification>>>>,
    server_info: Option<InitializeResult>,
    timeout: std::time::Duration,
    docker_container: Option<String>,
}

impl McpClient {
    pub async fn connect<T, E, A>(
        transport: T,
        timeout: std::time::Duration,
        provider: SharedProvider,
        client_name: String,
        capabilities: GooseMcpClientCapabilities,
        working_dir: PathBuf,
    ) -> Result<Self, ClientInitializeError>
    where
        T: IntoTransport<RoleClient, E, A>,
        E: std::error::Error + From<std::io::Error> + Send + Sync + 'static,
    {
        Self::connect_with_container(
            transport,
            timeout,
            provider,
            None,
            client_name,
            capabilities,
            working_dir,
        )
        .await
    }

    pub async fn connect_with_container<T, E, A>(
        transport: T,
        timeout: std::time::Duration,
        provider: SharedProvider,
        docker_container: Option<String>,
        client_name: String,
        capabilities: GooseMcpClientCapabilities,
        working_dir: PathBuf,
    ) -> Result<Self, ClientInitializeError>
    where
        T: IntoTransport<RoleClient, E, A>,
        E: std::error::Error + From<std::io::Error> + Send + Sync + 'static,
    {
        let notification_subscribers =
            Arc::new(Mutex::new(Vec::<mpsc::Sender<ServerNotification>>::new()));

        let client = GooseClient::new(
            notification_subscribers.clone(),
            provider,
            client_name.clone(),
            capabilities.clone(),
            working_dir,
        );
        let client: rmcp::service::RunningService<rmcp::RoleClient, GooseClient> =
            client.serve(transport).await?;
        let server_info = client.peer_info().cloned();

        Ok(Self {
            client: Mutex::new(client),
            notification_subscribers,
            server_info,
            timeout,
            docker_container,
        })
    }

    pub fn docker_container(&self) -> Option<&str> {
        self.docker_container.as_deref()
    }

    async fn do_update_working_dir(&self, new_dir: PathBuf) -> Result<(), Error> {
        let client = self.client.lock().await;
        let shared = client.service().shared_working_dir();
        *shared.write().await = new_dir;
        client.peer().notify_roots_list_changed().await?;
        Ok(())
    }

    async fn send_request_with_context(
        &self,
        session_id: &str,
        working_dir: Option<&str>,
        request: ClientRequest,
        cancel_token: CancellationToken,
    ) -> Result<ServerResult, Error> {
        let request = inject_session_context_into_request(request, Some(session_id), working_dir);
        // The inner mutex is held only for the send; the actual response wait
        // happens outside the lock so concurrent calls can overlap.
        let handle = {
            let client = self.client.lock().await;
            client.service().set_session_id(session_id).await;
            client
                .send_cancellable_request(request, PeerRequestOptions::no_options())
                .await
        }?;

        await_response(handle, self.timeout, &cancel_token).await
    }
}

async fn await_response(
    handle: RequestHandle<RoleClient>,
    timeout: Duration,
    cancel_token: &CancellationToken,
) -> Result<<RoleClient as ServiceRole>::PeerResp, ServiceError> {
    let receiver = handle.rx;
    let peer = handle.peer;
    let request_id = handle.id;
    tokio::select! {
        result = receiver => {
            result.map_err(|_e| ServiceError::TransportClosed)?
        }
        _ = tokio::time::sleep(timeout) => {
            send_cancel_message(&peer, request_id, Some("timed out".to_owned())).await?;
            Err(ServiceError::Timeout{timeout})
        }
        _ = cancel_token.cancelled() => {
            send_cancel_message(&peer, request_id, Some("operation cancelled".to_owned())).await?;
            Err(ServiceError::Cancelled { reason: None })
        }
    }
}

async fn send_cancel_message(
    peer: &Peer<RoleClient>,
    request_id: RequestId,
    reason: Option<String>,
) -> Result<(), ServiceError> {
    peer.send_notification(
        Notification::new(CancelledNotificationParam { request_id, reason }).into(),
    )
    .await
}

#[async_trait::async_trait]
impl McpClientTrait for McpClient {
    fn get_info(&self) -> Option<&InitializeResult> {
        self.server_info.as_ref()
    }

    async fn list_resources(
        &self,
        session_id: &str,
        cursor: Option<String>,
        cancel_token: CancellationToken,
    ) -> Result<ListResourcesResult, Error> {
        let res = self
            .send_request_with_context(
                session_id,
                None,
                ClientRequest::ListResourcesRequest(RequestOptionalParam::with_param(
                    PaginatedRequestParams::default().with_cursor(cursor),
                )),
                cancel_token,
            )
            .await?;

        match res {
            ServerResult::ListResourcesResult(result) => Ok(result),
            _ => Err(ServiceError::UnexpectedResponse),
        }
    }

    async fn read_resource(
        &self,
        session_id: &str,
        uri: &str,
        cancel_token: CancellationToken,
    ) -> Result<ReadResourceResult, Error> {
        let res = self
            .send_request_with_context(
                session_id,
                None,
                ClientRequest::ReadResourceRequest(Request::new(ReadResourceRequestParams::new(
                    uri.to_string(),
                ))),
                cancel_token,
            )
            .await?;

        match res {
            ServerResult::ReadResourceResult(result) => Ok(result),
            _ => Err(ServiceError::UnexpectedResponse),
        }
    }

    async fn list_tools(
        &self,
        session_id: &str,
        cursor: Option<String>,
        cancel_token: CancellationToken,
    ) -> Result<ListToolsResult, Error> {
        let res = self
            .send_request_with_context(
                session_id,
                None,
                ClientRequest::ListToolsRequest(RequestOptionalParam::with_param(
                    PaginatedRequestParams::default().with_cursor(cursor),
                )),
                cancel_token,
            )
            .await?;

        match res {
            ServerResult::ListToolsResult(result) => Ok(result),
            _ => Err(ServiceError::UnexpectedResponse),
        }
    }

    async fn call_tool(
        &self,
        ctx: &ToolCallContext,
        name: &str,
        arguments: Option<JsonObject>,
        cancel_token: CancellationToken,
    ) -> Result<CallToolResult, Error> {
        let mut params = CallToolRequestParams::new(name.to_string());
        if let Some(args) = arguments {
            params = params.with_arguments(args);
        }
        let request = ClientRequest::CallToolRequest(Request::new(params));

        let result = self
            .send_request_with_context(
                &ctx.session_id,
                ctx.working_dir_str(),
                request,
                cancel_token,
            )
            .await;

        match result? {
            ServerResult::CallToolResult(result) => Ok(result),
            _ => Err(ServiceError::UnexpectedResponse),
        }
    }

    async fn list_prompts(
        &self,
        session_id: &str,
        cursor: Option<String>,
        cancel_token: CancellationToken,
    ) -> Result<ListPromptsResult, Error> {
        let res = self
            .send_request_with_context(
                session_id,
                None,
                ClientRequest::ListPromptsRequest(RequestOptionalParam::with_param(
                    PaginatedRequestParams::default().with_cursor(cursor),
                )),
                cancel_token,
            )
            .await?;

        match res {
            ServerResult::ListPromptsResult(result) => Ok(result),
            _ => Err(ServiceError::UnexpectedResponse),
        }
    }

    async fn get_prompt(
        &self,
        session_id: &str,
        name: &str,
        arguments: Value,
        cancel_token: CancellationToken,
    ) -> Result<GetPromptResult, Error> {
        let arguments = match arguments {
            Value::Object(map) => Some(map),
            _ => None,
        };
        let mut params = GetPromptRequestParams::new(name.to_string());
        if let Some(args) = arguments {
            params = params.with_arguments(args);
        }
        let res = self
            .send_request_with_context(
                session_id,
                None,
                ClientRequest::GetPromptRequest(Request::new(params)),
                cancel_token,
            )
            .await?;

        match res {
            ServerResult::GetPromptResult(result) => Ok(result),
            _ => Err(ServiceError::UnexpectedResponse),
        }
    }

    async fn subscribe(&self) -> mpsc::Receiver<ServerNotification> {
        let (tx, rx) = mpsc::channel(16);
        self.notification_subscribers.lock().await.push(tx);
        rx
    }

    async fn update_working_dir(&self, new_dir: PathBuf) -> Result<(), Error> {
        self.do_update_working_dir(new_dir).await
    }
}

/// Injects the given session_id and working_dir into Extensions._meta.
/// None (or empty) removes any existing values.
fn inject_session_context_into_extensions(
    mut extensions: Extensions,
    session_id: Option<&str>,
    working_dir: Option<&str>,
) -> Extensions {
    let session_id = session_id.filter(|id| !id.is_empty());
    let working_dir = working_dir.filter(|dir| !dir.is_empty());
    let mut meta_map = extensions
        .get::<Meta>()
        .map(|meta| meta.0.clone())
        .unwrap_or_default();

    // JsonObject is case-sensitive, so we use retain for case-insensitive removal
    meta_map.retain(|k, _| {
        !k.eq_ignore_ascii_case(SESSION_ID_HEADER) && !k.eq_ignore_ascii_case(WORKING_DIR_HEADER)
    });

    if let Some(session_id) = session_id {
        meta_map.insert(
            SESSION_ID_HEADER.to_string(),
            Value::String(session_id.to_string()),
        );
    }

    if let Some(working_dir) = working_dir {
        meta_map.insert(
            WORKING_DIR_HEADER.to_string(),
            Value::String(working_dir.to_string()),
        );
    }

    extensions.insert(Meta(meta_map));
    extensions
}

fn inject_session_context_into_request(
    request: ClientRequest,
    session_id: Option<&str>,
    working_dir: Option<&str>,
) -> ClientRequest {
    match request {
        ClientRequest::ListResourcesRequest(mut req) => {
            req.extensions =
                inject_session_context_into_extensions(req.extensions, session_id, working_dir);
            ClientRequest::ListResourcesRequest(req)
        }
        ClientRequest::ReadResourceRequest(mut req) => {
            req.extensions =
                inject_session_context_into_extensions(req.extensions, session_id, working_dir);
            ClientRequest::ReadResourceRequest(req)
        }
        ClientRequest::ListToolsRequest(mut req) => {
            req.extensions =
                inject_session_context_into_extensions(req.extensions, session_id, working_dir);
            ClientRequest::ListToolsRequest(req)
        }
        ClientRequest::CallToolRequest(mut req) => {
            req.extensions =
                inject_session_context_into_extensions(req.extensions, session_id, working_dir);
            ClientRequest::CallToolRequest(req)
        }
        ClientRequest::ListPromptsRequest(mut req) => {
            req.extensions =
                inject_session_context_into_extensions(req.extensions, session_id, working_dir);
            ClientRequest::ListPromptsRequest(req)
        }
        ClientRequest::GetPromptRequest(mut req) => {
            req.extensions =
                inject_session_context_into_extensions(req.extensions, session_id, working_dir);
            ClientRequest::GetPromptRequest(req)
        }
        other => other,
    }
}

// ============================================================================
// v19: MCP UX polish — additive, user-facing helpers for clearer errors,
// connection status, and listing output. None of these change protocol
// behavior; they only translate already-available data into readable text for
// CLI/desktop surfaces. User-facing strings are kept neutral (no host-product
// branding) so they remain safe to display verbatim.
// ============================================================================

/// User-facing connection status for an MCP server, derived from the handshake
/// `InitializeResult` captured when the client connected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpConnectionStatus {
    /// The server completed initialization and reported its identity.
    Connected {
        server_name: String,
        server_version: String,
        protocol_version: String,
    },
    /// No initialization result is available — either the client has not
    /// connected yet, or it is a built-in extension that performs no handshake.
    NotInitialized,
}

impl McpConnectionStatus {
    fn from_info(info: &InitializeResult) -> Self {
        McpConnectionStatus::Connected {
            server_name: info.server_info.name.clone(),
            server_version: info.server_info.version.clone(),
            protocol_version: info.protocol_version.to_string(),
        }
    }

    pub fn is_connected(&self) -> bool {
        matches!(self, McpConnectionStatus::Connected { .. })
    }

    /// One-line, user-facing status suitable for CLI/desktop display.
    pub fn status_line(&self) -> String {
        match self {
            McpConnectionStatus::Connected {
                server_name,
                server_version,
                protocol_version,
            } => format!("connected to {server_name} v{server_version} (MCP {protocol_version})"),
            McpConnectionStatus::NotInitialized => "not connected".to_string(),
        }
    }
}

/// Human-readable, user-facing description of an MCP client/transport error.
///
/// `ServiceError`'s own `Display` is terse and protocol-oriented (e.g.
/// "Transport closed"); this expands each case into actionable guidance without
/// leaking internal type names into user-facing output.
pub fn describe_service_error(err: &ServiceError) -> String {
    match err {
        ServiceError::McpError(data) => describe_mcp_error_data(data),
        ServiceError::TransportSend(send_err) => format!(
            "Could not send the request to the MCP server (the connection may have dropped): {send_err}"
        ),
        ServiceError::TransportClosed => {
            "The connection to the MCP server was closed. The server may have exited or crashed; \
             try reconnecting the extension."
                .to_string()
        }
        ServiceError::UnexpectedResponse => {
            "The MCP server returned an unexpected response that did not match the request. \
             This usually indicates a protocol mismatch between the client and server."
                .to_string()
        }
        ServiceError::Cancelled { reason } => match reason {
            Some(reason) if !reason.trim().is_empty() => {
                format!("The MCP request was cancelled: {reason}.")
            }
            _ => "The MCP request was cancelled.".to_string(),
        },
        ServiceError::Timeout { timeout } => format!(
            "The MCP server did not respond within {}s. It may be slow to start, overloaded, or \
             stuck; try again or increase the extension timeout.",
            timeout.as_secs()
        ),
        // `ServiceError` is #[non_exhaustive]; keep a stable readable fallback.
        other => format!("The MCP request failed: {other}"),
    }
}

/// Translate a JSON-RPC error payload from a server into user-facing text.
fn describe_mcp_error_data(data: &ErrorData) -> String {
    let detail = data.message.trim();
    let prefix = match data.code.0 {
        -32700 => "The MCP server could not parse the request",
        -32600 => "The MCP server rejected the request as invalid",
        -32601 => "The MCP server does not support that method or tool",
        -32602 => "The MCP request had invalid parameters",
        -32603 => "The MCP server hit an internal error",
        -32002 => "The requested MCP resource was not found",
        _ => "The MCP server returned an error",
    };
    if detail.is_empty() {
        format!("{prefix} (code {}).", data.code.0)
    } else {
        format!("{prefix}: {detail}")
    }
}

/// Maximum number of item names rendered inline before collapsing to a count.
const LISTING_PREVIEW_LIMIT: usize = 8;

/// Compact, user-facing summary of the tools an MCP server exposes.
pub fn summarize_tools(result: &ListToolsResult) -> String {
    let names: Vec<&str> = result.tools.iter().map(|tool| tool.name.as_ref()).collect();
    summarize_listing("tool", &names, result.next_cursor.is_some())
}

/// Compact, user-facing summary of the resources an MCP server exposes.
pub fn summarize_resources(result: &ListResourcesResult) -> String {
    let names: Vec<&str> = result
        .resources
        .iter()
        .map(|resource| {
            if resource.name.is_empty() {
                resource.uri.as_str()
            } else {
                resource.name.as_str()
            }
        })
        .collect();
    summarize_listing("resource", &names, result.next_cursor.is_some())
}

/// Compact, user-facing summary of the prompts an MCP server exposes.
pub fn summarize_prompts(result: &ListPromptsResult) -> String {
    let names: Vec<&str> = result
        .prompts
        .iter()
        .map(|prompt| prompt.name.as_str())
        .collect();
    summarize_listing("prompt", &names, result.next_cursor.is_some())
}

fn summarize_listing(kind: &str, names: &[&str], has_more_pages: bool) -> String {
    if names.is_empty() {
        return format!("No {kind}s available.");
    }

    let count = names.len();
    let shown: Vec<&str> = names.iter().take(LISTING_PREVIEW_LIMIT).copied().collect();
    let mut summary = format!("{count} {}: {}", pluralize(kind, count), shown.join(", "));

    let hidden = count.saturating_sub(shown.len());
    if hidden > 0 {
        summary.push_str(&format!(", and {hidden} more"));
    }
    if has_more_pages {
        summary.push_str(" (more available on the next page)");
    }
    summary
}

fn pluralize(word: &str, count: usize) -> String {
    if count == 1 {
        word.to_string()
    } else {
        format!("{word}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::GoosePlatform;
    use serde_json::json;
    use test_case::test_case;

    fn new_client(platform: GoosePlatform) -> GooseClient {
        let capabilities = match platform {
            GoosePlatform::GooseDesktop => GooseMcpClientCapabilities {
                mcpui: true,
                host_info: None,
            },
            GoosePlatform::GooseCli => GooseMcpClientCapabilities {
                mcpui: false,
                host_info: None,
            },
        };

        GooseClient::new(
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(Mutex::new(None)),
            platform.to_string(),
            capabilities,
            std::env::current_dir().unwrap_or_default(),
        )
    }

    fn request_extensions(request: &ClientRequest) -> Option<&Extensions> {
        match request {
            ClientRequest::ListResourcesRequest(req) => Some(&req.extensions),
            ClientRequest::ReadResourceRequest(req) => Some(&req.extensions),
            ClientRequest::ListToolsRequest(req) => Some(&req.extensions),
            ClientRequest::CallToolRequest(req) => Some(&req.extensions),
            ClientRequest::ListPromptsRequest(req) => Some(&req.extensions),
            ClientRequest::GetPromptRequest(req) => Some(&req.extensions),
            _ => None,
        }
    }

    fn list_resources_request(extensions: Extensions) -> ClientRequest {
        let mut req = RequestOptionalParam::with_param(PaginatedRequestParams::default());
        req.extensions = extensions;
        ClientRequest::ListResourcesRequest(req)
    }

    fn read_resource_request(extensions: Extensions) -> ClientRequest {
        let mut req = Request::new(ReadResourceRequestParams::new(
            "test://resource".to_string(),
        ));
        req.extensions = extensions;
        ClientRequest::ReadResourceRequest(req)
    }

    fn list_tools_request(extensions: Extensions) -> ClientRequest {
        let mut req = RequestOptionalParam::with_param(PaginatedRequestParams::default());
        req.extensions = extensions;
        ClientRequest::ListToolsRequest(req)
    }

    fn call_tool_request(extensions: Extensions) -> ClientRequest {
        let mut req = Request::new(CallToolRequestParams::new("tool".to_string()));
        req.extensions = extensions;
        ClientRequest::CallToolRequest(req)
    }

    fn list_prompts_request(extensions: Extensions) -> ClientRequest {
        let mut req = RequestOptionalParam::with_param(PaginatedRequestParams::default());
        req.extensions = extensions;
        ClientRequest::ListPromptsRequest(req)
    }

    fn get_prompt_request(extensions: Extensions) -> ClientRequest {
        let mut req = Request::new(GetPromptRequestParams::new("prompt".to_string()));
        req.extensions = extensions;
        ClientRequest::GetPromptRequest(req)
    }

    #[test_case(
        Some("ext-session"),
        Some("current-session"),
        Some("ext-session");
        "extensions win"
    )]
    #[test_case(
        None,
        Some("current-session"),
        Some("current-session");
        "current when no extensions"
    )]
    #[test_case(
        None,
        None,
        None;
        "no session when no extensions or current"
    )]
    fn test_resolve_session_id(
        ext_session: Option<&str>,
        current_session: Option<&str>,
        expected: Option<&str>,
    ) {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let client = new_client(GoosePlatform::GooseCli);
            if let Some(session_id) = current_session {
                client.set_session_id(session_id).await;
            }

            let extensions =
                inject_session_context_into_extensions(Extensions::new(), ext_session, None);

            let resolved = client.resolve_session_id(&extensions).await;

            let expected = expected.map(str::to_string);
            assert_eq!(resolved, expected);
        });
    }

    #[test_case(list_resources_request; "list_resources")]
    #[test_case(read_resource_request; "read_resource")]
    #[test_case(list_tools_request; "list_tools")]
    #[test_case(call_tool_request; "call_tool")]
    #[test_case(list_prompts_request; "list_prompts")]
    #[test_case(get_prompt_request; "get_prompt")]
    fn test_request_injects_session(request_builder: fn(Extensions) -> ClientRequest) {
        let session_id = "test-session-id";
        let mut extensions = Extensions::new();
        extensions.insert(
            serde_json::from_value::<Meta>(json!({
                "BharatCode-Session-Id": "old-session-id",
                "other-key": "preserve-me"
            }))
            .unwrap(),
        );

        let request = request_builder(extensions);
        let request = inject_session_context_into_request(request, Some(session_id), None);
        let extensions = request_extensions(&request).expect("request should have extensions");
        let meta = extensions
            .get::<Meta>()
            .expect("extensions should contain meta");

        assert_eq!(
            meta.0.get(SESSION_ID_HEADER),
            Some(&Value::String(session_id.to_string()))
        );
        assert_eq!(
            meta.0.get("other-key"),
            Some(&Value::String("preserve-me".to_string()))
        );
    }

    #[test]
    fn test_session_id_in_mcp_meta() {
        let session_id = "test-session-789";
        let extensions =
            inject_session_context_into_extensions(Default::default(), Some(session_id), None);
        let mcp_meta = extensions.get::<Meta>().unwrap();

        assert_eq!(
            &mcp_meta.0,
            json!({
                SESSION_ID_HEADER: session_id
            })
            .as_object()
            .unwrap()
        );
    }

    #[test_case(
        Some("new-session-id"),
        json!({
            SESSION_ID_HEADER: "new-session-id",
            "other-key": "preserve-me"
        });
        "replace"
    )]
    #[test_case(
        None,
        json!({
            "other-key": "preserve-me"
        });
        "remove"
    )]
    #[test_case(
        Some(""),
        json!({
            "other-key": "preserve-me"
        });
        "empty removes"
    )]
    fn test_session_id_case_insensitive_replacement(
        session_id: Option<&str>,
        expected_meta: serde_json::Value,
    ) {
        use rmcp::model::Extensions;
        use serde_json::from_value;

        let mut extensions = Extensions::new();
        extensions.insert(
            from_value::<Meta>(json!({
                SESSION_ID_HEADER: "old-session-1",
                "Agent-Session-Id": "old-session-2",
                "other-key": "preserve-me"
            }))
            .unwrap(),
        );

        let extensions = inject_session_context_into_extensions(extensions, session_id, None);
        let mcp_meta = extensions.get::<Meta>().unwrap();

        assert_eq!(&mcp_meta.0, expected_meta.as_object().unwrap());
    }

    #[test]
    fn test_client_info_advertises_mcp_apps_ui_extension() {
        let client = new_client(GoosePlatform::GooseDesktop);
        let info = ClientHandler::get_info(&client);

        // Verify the client advertises the MCP Apps UI extension capability
        let extensions = info
            .capabilities
            .extensions
            .expect("capabilities should have extensions");

        let ui_ext = extensions
            .get("io.modelcontextprotocol/ui")
            .expect("should have io.modelcontextprotocol/ui extension");

        let mime_types = ui_ext
            .get("mimeTypes")
            .expect("ui extension should have mimeTypes");

        assert_eq!(mime_types, &json!(["text/html;profile=mcp-app"]));
    }

    #[test]
    fn test_client_capabilities_advertise_roots() {
        let client = new_client(GoosePlatform::GooseCli);
        let info = ClientHandler::get_info(&client);
        assert!(
            info.capabilities.roots.is_some(),
            "client should advertise roots capability"
        );
    }

    #[test]
    fn test_explicit_host_info_passes_through_client_identity() {
        let client = GooseClient::new(
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(Mutex::new(None)),
            GoosePlatform::GooseDesktop.to_string(),
            GooseMcpClientCapabilities {
                mcpui: true,
                host_info: Some(GooseMcpHostInfo {
                    explicit_extensions: true,
                    extensions: ExtensionCapabilities::new(),
                    client_name: Some("bharatcode2".to_string()),
                    client_version: Some("0.1.0".to_string()),
                }),
            },
            std::env::current_dir().unwrap_or_default(),
        );

        let info = ClientHandler::get_info(&client);
        assert_eq!(info.client_info.name, "bharatcode2");
        assert_eq!(info.client_info.version, "0.1.0");
        let extensions = info
            .capabilities
            .extensions
            .expect("client should still serialize an extensions object");
        assert!(
            !extensions.contains_key(MCP_APPS_UI_EXTENSION_ID),
            "explicit empty host extensions should disable platform fallback"
        );
    }

    #[test]
    fn test_explicit_host_extensions_override_platform_fallback() {
        let client = GooseClient::new(
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(Mutex::new(None)),
            GoosePlatform::GooseCli.to_string(),
            GooseMcpClientCapabilities {
                mcpui: false,
                host_info: Some(GooseMcpHostInfo {
                    explicit_extensions: true,
                    extensions: default_mcp_apps_ui_extensions(),
                    client_name: Some("bharatcode2".to_string()),
                    client_version: Some("0.1.0".to_string()),
                }),
            },
            std::env::current_dir().unwrap_or_default(),
        );

        let info = ClientHandler::get_info(&client);
        let extensions = info
            .capabilities
            .extensions
            .expect("capabilities should have explicit host extensions");

        assert!(extensions.contains_key(MCP_APPS_UI_EXTENSION_ID));
        assert_eq!(info.client_info.name, "bharatcode2");
    }

    #[test]
    fn test_host_identity_does_not_disable_platform_fallback_without_explicit_extensions() {
        let client = GooseClient::new(
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(Mutex::new(None)),
            GoosePlatform::GooseDesktop.to_string(),
            GooseMcpClientCapabilities {
                mcpui: true,
                host_info: Some(GooseMcpHostInfo {
                    explicit_extensions: false,
                    extensions: ExtensionCapabilities::new(),
                    client_name: Some("bharatcode2".to_string()),
                    client_version: Some("0.1.0".to_string()),
                }),
            },
            std::env::current_dir().unwrap_or_default(),
        );

        let info = ClientHandler::get_info(&client);
        let extensions = info
            .capabilities
            .extensions
            .expect("platform fallback should still advertise MCP Apps UI");

        assert!(extensions.contains_key(MCP_APPS_UI_EXTENSION_ID));
        assert_eq!(info.client_info.name, "bharatcode2");
    }

    #[test]
    fn test_working_dir_roots_returns_current_dir_as_root() {
        let dir = PathBuf::from("/tmp/test-project");
        let result = working_dir_roots(&dir);
        assert_eq!(result.roots.len(), 1);
        assert_eq!(result.roots[0].uri, "file:///tmp/test-project");
        assert_eq!(result.roots[0].name.as_deref(), Some("working_directory"));
    }
}

#[cfg(test)]
mod ux_tests {
    use super::*;
    use rmcp::model::{AnnotateAble, Prompt, RawResource, Tool};

    fn tool(name: &'static str) -> Tool {
        Tool::new(name, "desc", Arc::new(JsonObject::new()))
    }

    #[test]
    fn connection_status_status_line_formats_identity() {
        let status = McpConnectionStatus::Connected {
            server_name: "filesystem".to_string(),
            server_version: "1.2.3".to_string(),
            protocol_version: "2025-03-26".to_string(),
        };
        assert!(status.is_connected());
        assert_eq!(
            status.status_line(),
            "connected to filesystem v1.2.3 (MCP 2025-03-26)"
        );
    }

    #[test]
    fn connection_status_not_initialized_is_not_connected() {
        let status = McpConnectionStatus::NotInitialized;
        assert!(!status.is_connected());
        assert_eq!(status.status_line(), "not connected");
    }

    #[test]
    fn describe_service_error_is_user_facing() {
        let timeout = describe_service_error(&ServiceError::Timeout {
            timeout: Duration::from_secs(30),
        });
        assert!(timeout.contains("30s"));
        assert!(timeout.contains("did not respond"));

        let closed = describe_service_error(&ServiceError::TransportClosed);
        assert!(closed.contains("closed"));

        let cancelled = describe_service_error(&ServiceError::Cancelled {
            reason: Some("user aborted".to_string()),
        });
        assert!(cancelled.contains("user aborted"));
    }

    #[test]
    fn describe_service_error_maps_known_mcp_codes() {
        let err = ServiceError::McpError(ErrorData::new(
            ErrorCode::METHOD_NOT_FOUND,
            "no such tool",
            None,
        ));
        let msg = describe_service_error(&err);
        assert!(msg.contains("does not support"));
        assert!(msg.contains("no such tool"));
    }

    #[test]
    fn summarize_tools_counts_and_lists() {
        let result = ListToolsResult::with_all_items(vec![tool("read"), tool("write")]);
        assert_eq!(summarize_tools(&result), "2 tools: read, write");
    }

    #[test]
    fn summarize_tools_singular_and_empty() {
        let one = ListToolsResult::with_all_items(vec![tool("read")]);
        assert_eq!(summarize_tools(&one), "1 tool: read");

        let none = ListToolsResult::with_all_items(vec![]);
        assert_eq!(summarize_tools(&none), "No tools available.");
    }

    #[test]
    fn summarize_tools_truncates_and_flags_more_pages() {
        let names = ["t0", "t1", "t2", "t3", "t4", "t5", "t6", "t7", "t8", "t9"];
        let tools: Vec<Tool> = names.iter().map(|&name| tool(name)).collect();
        let mut result = ListToolsResult::with_all_items(tools);
        result.next_cursor = Some("cursor".to_string());

        let summary = summarize_tools(&result);
        assert!(summary.starts_with("10 tools: "));
        assert!(summary.contains("and 2 more"));
        assert!(summary.contains("more available on the next page"));
    }

    #[test]
    fn summarize_resources_falls_back_to_uri_when_name_empty() {
        let named = RawResource::new("file:///a", "Alpha").no_annotation();
        let unnamed = RawResource::new("file:///b", "").no_annotation();
        let result = ListResourcesResult::with_all_items(vec![named, unnamed]);
        assert_eq!(
            summarize_resources(&result),
            "2 resources: Alpha, file:///b"
        );
    }

    #[test]
    fn summarize_prompts_lists_names() {
        let prompts = vec![
            Prompt::new("greet", Some("Greeting"), None),
            Prompt::new("farewell", None::<String>, None),
        ];
        let result = ListPromptsResult::with_all_items(prompts);
        assert_eq!(summarize_prompts(&result), "2 prompts: greet, farewell");
    }
}
