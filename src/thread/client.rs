use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex, MutexGuard},
};

use agent_client_protocol::{
    Client, ConnectionTo, Error,
    schema::{
        ClientCapabilities, ContentChunk, Implementation, Meta, PermissionOption, Plan, PlanEntry,
        PlanEntryPriority, PlanEntryStatus, RequestPermissionRequest, RequestPermissionResponse,
        SessionId, SessionNotification, SessionUpdate, ToolCall, ToolCallId, ToolCallStatus,
        ToolCallUpdate, ToolCallUpdateFields, ToolKind,
    },
};
use codex_protocol::plan_tool::{PlanItemArg, StepStatus};
use tracing::error;

use super::{
    DISABLE_TERMINAL_OUTPUT, ENABLE_EXPERIMENTAL_TERMINAL_OUTPUT, tool_calls::ActiveCommand,
};

const CODEX_ACP_META_KEY: &str = "codex_acp";
const WARNING_META_KIND: &str = "warning";

/// Abstraction over the ACP connection for sending notifications and requests
/// back to the client. This replaces the old `Client` trait usage.
pub(super) trait ClientSender: Send + Sync + 'static {
    fn send_session_notification(&self, notif: SessionNotification) -> Result<(), Error>;
    fn request_permission(
        &self,
        req: RequestPermissionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<RequestPermissionResponse, Error>> + Send + '_>>;
}

/// Production implementation that wraps a `ConnectionTo<Client>`.
struct AcpConnection(ConnectionTo<Client>);

impl ClientSender for AcpConnection {
    fn send_session_notification(&self, notif: SessionNotification) -> Result<(), Error> {
        self.0.send_notification(notif)
    }

    fn request_permission(
        &self,
        req: RequestPermissionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<RequestPermissionResponse, Error>> + Send + '_>> {
        Box::pin(async move { self.0.send_request(req).block_task().await })
    }
}

#[derive(Clone)]
pub(super) struct SessionClient {
    session_id: SessionId,
    client: Arc<dyn ClientSender>,
    client_capabilities: Arc<Mutex<ClientCapabilities>>,
    client_info: Arc<Mutex<Option<Implementation>>>,
}

impl SessionClient {
    pub(super) fn new(
        session_id: SessionId,
        cx: ConnectionTo<Client>,
        client_capabilities: Arc<Mutex<ClientCapabilities>>,
        client_info: Arc<Mutex<Option<Implementation>>>,
    ) -> Self {
        Self {
            session_id,
            client: Arc::new(AcpConnection(cx)),
            client_capabilities,
            client_info,
        }
    }

    #[cfg(test)]
    pub(super) fn with_client(
        session_id: SessionId,
        client: Arc<dyn ClientSender>,
        client_capabilities: Arc<Mutex<ClientCapabilities>>,
        client_info: Arc<Mutex<Option<Implementation>>>,
    ) -> Self {
        Self {
            session_id,
            client,
            client_capabilities,
            client_info,
        }
    }

    fn is_zed_client(&self) -> bool {
        {
            let client_info = lock_client_state(&self.client_info, "client info");
            let Some(client_info) = client_info.as_ref() else {
                return false;
            };
            let title_is_zed = client_info
                .title
                .as_deref()
                .is_some_and(|title| title.to_ascii_lowercase().contains("zed"));

            client_info.name.eq_ignore_ascii_case("zed") || title_is_zed
        }
    }

    pub(super) fn supports_terminal_output(&self, active_command: &ActiveCommand) -> bool {
        if *DISABLE_TERMINAL_OUTPUT || !active_command.terminal_output {
            return false;
        }

        let client_supports_terminal_output = {
            let client_capabilities =
                lock_client_state(&self.client_capabilities, "client capabilities");
            client_capabilities.meta.as_ref().is_some_and(|v| {
                v.get("terminal_output")
                    .is_some_and(|v| v.as_bool().unwrap_or_default())
            })
        };

        if !client_supports_terminal_output {
            return false;
        }

        // Zed currently renders command output via the display-only terminal bridge in ACP
        // `_meta`; other clients can opt in until we switch to terminal/create.
        self.is_zed_client() || *ENABLE_EXPERIMENTAL_TERMINAL_OUTPUT
    }

    pub(super) fn send_notification(&self, update: SessionUpdate) {
        if let Err(e) = self
            .client
            .send_session_notification(SessionNotification::new(self.session_id.clone(), update))
        {
            error!("Failed to send session notification: {:?}", e);
        }
    }

    pub(super) fn send_user_message(&self, text: impl Into<String>) {
        self.send_notification(SessionUpdate::UserMessageChunk(ContentChunk::new(
            text.into().into(),
        )));
    }

    pub(super) fn send_agent_text(&self, text: impl Into<String>) {
        self.send_notification(SessionUpdate::AgentMessageChunk(ContentChunk::new(
            text.into().into(),
        )));
    }

    pub(super) fn send_agent_warning(&self, text: impl Into<String>) {
        self.send_notification(SessionUpdate::AgentMessageChunk(
            ContentChunk::new(text.into().into()).meta(Meta::from_iter([(
                CODEX_ACP_META_KEY.to_string(),
                serde_json::json!({ "kind": WARNING_META_KIND }),
            )])),
        ));
    }

    pub(super) fn send_agent_thought(&self, text: impl Into<String>) {
        self.send_notification(SessionUpdate::AgentThoughtChunk(ContentChunk::new(
            text.into().into(),
        )));
    }

    pub(super) fn send_tool_call(&self, tool_call: ToolCall) {
        self.send_notification(SessionUpdate::ToolCall(tool_call));
    }

    pub(super) fn send_tool_call_update(&self, update: ToolCallUpdate) {
        self.send_notification(SessionUpdate::ToolCallUpdate(update));
    }

    /// Send a completed tool call (used for replay and simple cases)
    pub(super) fn send_completed_tool_call(
        &self,
        call_id: impl Into<ToolCallId>,
        title: impl Into<String>,
        kind: ToolKind,
        raw_input: Option<serde_json::Value>,
    ) {
        let mut tool_call = ToolCall::new(call_id, title)
            .kind(kind)
            .status(ToolCallStatus::Completed);
        if let Some(input) = raw_input {
            tool_call = tool_call.raw_input(input);
        }
        self.send_tool_call(tool_call);
    }

    /// Send a tool call completion update (used for replay)
    pub(super) fn send_tool_call_completed(
        &self,
        call_id: impl Into<ToolCallId>,
        raw_output: Option<serde_json::Value>,
    ) {
        let mut fields = ToolCallUpdateFields::new().status(ToolCallStatus::Completed);
        if let Some(output) = raw_output {
            fields = fields.raw_output(output);
        }
        self.send_tool_call_update(ToolCallUpdate::new(call_id, fields));
    }

    pub(super) fn update_plan(&self, plan: Vec<PlanItemArg>) {
        self.send_notification(SessionUpdate::Plan(Plan::new(
            plan.into_iter()
                .map(|entry| {
                    PlanEntry::new(
                        entry.step,
                        PlanEntryPriority::Medium,
                        match entry.status {
                            StepStatus::Pending => PlanEntryStatus::Pending,
                            StepStatus::InProgress => PlanEntryStatus::InProgress,
                            StepStatus::Completed => PlanEntryStatus::Completed,
                        },
                    )
                })
                .collect(),
        )));
    }

    pub(super) async fn request_permission(
        &self,
        tool_call: ToolCallUpdate,
        options: Vec<PermissionOption>,
    ) -> Result<RequestPermissionResponse, Error> {
        self.client
            .request_permission(RequestPermissionRequest::new(
                self.session_id.clone(),
                tool_call,
                options,
            ))
            .await
    }
}

fn lock_client_state<'a, T>(mutex: &'a Mutex<T>, state_name: &str) -> MutexGuard<'a, T> {
    mutex.lock().unwrap_or_else(|err| {
        error!("{state_name} mutex was poisoned; continuing with inner state");
        err.into_inner()
    })
}
