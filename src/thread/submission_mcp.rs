use agent_client_protocol::{
    Error,
    schema::{
        Content, ContentBlock, ToolCall, ToolCallContent, ToolCallStatus, ToolCallUpdate,
        ToolCallUpdateFields,
    },
};
use codex_protocol::{
    approvals::{ElicitationRequest, ElicitationRequestEvent},
    mcp::CallToolResult,
    protocol::{ElicitationAction, McpInvocation, Op},
};
use tracing::info;

use super::{
    approvals::{PendingPermissionRequest, build_supported_mcp_elicitation_permission_request},
    client::SessionClient,
    submission::{PermissionInteractionRequest, PromptState},
};

impl PromptState {
    pub(super) async fn mcp_elicitation(
        &mut self,
        client: &SessionClient,
        event: ElicitationRequestEvent,
    ) -> Result<(), Error> {
        let raw_input = serde_json::json!(&event);
        let ElicitationRequestEvent {
            server_name,
            id,
            request,
            turn_id: _,
        } = event;
        if let Some(supported_request) = build_supported_mcp_elicitation_permission_request(
            &server_name,
            &id,
            &request,
            raw_input,
        ) {
            info!(
                "Routing MCP tool approval elicitation through ACP permission request: server={}, id={:?}",
                server_name, id
            );
            self.spawn_permission_request(
                client,
                PermissionInteractionRequest {
                    request_key: supported_request.request_key,
                    pending_request: PendingPermissionRequest::McpElicitation {
                        server_name,
                        request_id: id,
                        option_map: supported_request.option_map,
                    },
                    tool_call: supported_request.tool_call,
                    options: supported_request.options,
                },
            );
            return Ok(());
        }

        let request_kind = match &request {
            ElicitationRequest::Form { .. } => "form",
            ElicitationRequest::Url { .. } => "url",
        };

        info!(
            "Auto-declining unsupported MCP elicitation: server={}, id={:?}, kind={request_kind}",
            server_name, id
        );

        self.thread
            .submit_ok(Op::ResolveElicitation {
                server_name,
                request_id: id,
                decision: ElicitationAction::Decline,
                content: None,
                meta: None,
            })
            .await?;

        Ok(())
    }
    pub(super) fn start_mcp_tool_call(
        client: &SessionClient,
        call_id: String,
        invocation: &McpInvocation,
    ) {
        let title = format!("Tool: {}/{}", invocation.server, invocation.tool);
        client.send_tool_call(
            ToolCall::new(call_id, title)
                .status(ToolCallStatus::InProgress)
                .raw_input(serde_json::json!(invocation)),
        );
    }
    pub(super) fn end_mcp_tool_call(
        client: &SessionClient,
        call_id: String,
        result: Result<CallToolResult, String>,
    ) {
        let is_error = match result.as_ref() {
            Ok(result) => result.is_error.unwrap_or_default(),
            Err(_) => true,
        };
        let raw_output = match result.as_ref() {
            Ok(result) => serde_json::json!(result),
            Err(err) => serde_json::json!(err),
        };

        client.send_tool_call_update(ToolCallUpdate::new(
            call_id,
            ToolCallUpdateFields::new()
                .status(if is_error {
                    ToolCallStatus::Failed
                } else {
                    ToolCallStatus::Completed
                })
                .raw_output(raw_output)
                .content(
                    result
                        .ok()
                        .filter(|result| !result.content.is_empty())
                        .map(|result| {
                            result
                                .content
                                .into_iter()
                                .filter_map(|content| {
                                    serde_json::from_value::<ContentBlock>(content).ok()
                                })
                                .map(|content| ToolCallContent::Content(Content::new(content)))
                                .collect()
                        }),
                ),
        ));
    }
}
