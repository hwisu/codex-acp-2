use agent_client_protocol::schema::{
    Content, ContentBlock, ToolCall, ToolCallContent, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields,
};
use codex_protocol::{mcp::CallToolResult, protocol::McpInvocation};

use crate::boundary::{effect::BridgeEffect, raw};

pub(crate) fn mcp_tool_call_begin(call_id: String, invocation: &McpInvocation) -> ToolCall {
    let title = format!("Tool: {}/{}", invocation.server, invocation.tool);
    ToolCall::new(call_id, title)
        .status(ToolCallStatus::InProgress)
        .raw_input(raw::mcp_invocation(invocation))
}

pub(crate) fn mcp_tool_call_begin_effect(
    call_id: String,
    invocation: &McpInvocation,
) -> BridgeEffect {
    BridgeEffect::tool_call(mcp_tool_call_begin(call_id, invocation))
}

pub(crate) fn mcp_tool_call_end(
    call_id: String,
    result: Result<CallToolResult, String>,
) -> ToolCallUpdate {
    let is_error = match result.as_ref() {
        Ok(result) => result.is_error.unwrap_or_default(),
        Err(_) => true,
    };
    let raw_output = raw::mcp_tool_call_output(result.as_ref());

    ToolCallUpdate::new(
        call_id,
        ToolCallUpdateFields::new()
            .status(if is_error {
                ToolCallStatus::Failed
            } else {
                ToolCallStatus::Completed
            })
            .raw_output(raw_output)
            .content(mcp_tool_call_content(result)),
    )
}

pub(crate) fn mcp_tool_call_end_effect(
    call_id: String,
    result: Result<CallToolResult, String>,
) -> BridgeEffect {
    BridgeEffect::tool_call_update(mcp_tool_call_end(call_id, result))
}

fn mcp_tool_call_content(result: Result<CallToolResult, String>) -> Option<Vec<ToolCallContent>> {
    result
        .ok()
        .filter(|result| !result.content.is_empty())
        .map(|result| {
            result
                .content
                .into_iter()
                .filter_map(|content| serde_json::from_value::<ContentBlock>(content).ok())
                .map(|content| ToolCallContent::Content(Content::new(content)))
                .collect()
        })
}

#[cfg(test)]
mod tests {
    use agent_client_protocol::schema::ToolCallStatus;

    use super::mcp_tool_call_end;

    #[test]
    fn mcp_tool_call_end_marks_error_results_failed() {
        let update = mcp_tool_call_end("mcp-call".to_string(), Err("failed".to_string()));

        assert_eq!(update.tool_call_id.0.as_ref(), "mcp-call");
        assert_eq!(update.fields.status, Some(ToolCallStatus::Failed));
    }
}
