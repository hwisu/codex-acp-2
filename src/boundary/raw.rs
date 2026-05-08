use codex_protocol::{
    approvals::ElicitationRequestEvent,
    mcp::CallToolResult,
    models::{FunctionCallOutputPayload, WebSearchAction},
    protocol::{
        ApplyPatchApprovalRequestEvent, CollabAgentInteractionBeginEvent,
        CollabAgentInteractionEndEvent, CollabAgentSpawnBeginEvent, CollabAgentSpawnEndEvent,
        CollabCloseBeginEvent, CollabCloseEndEvent, CollabResumeBeginEvent, CollabResumeEndEvent,
        CollabWaitingBeginEvent, CollabWaitingEndEvent, DynamicToolCallResponseEvent,
        ExecApprovalRequestEvent, ExecCommandBeginEvent, ExecCommandEndEvent,
        GuardianAssessmentEvent, ImageGenerationEndEvent, McpInvocation, PatchApplyBeginEvent,
        PatchApplyEndEvent, PatchApplyUpdatedEvent,
    },
    request_permissions::RequestPermissionsEvent,
};

pub(crate) fn exec_approval_request(event: &ExecApprovalRequestEvent) -> serde_json::Value {
    serde_json::json!(event)
}

pub(crate) fn exec_command_begin(event: &ExecCommandBeginEvent) -> serde_json::Value {
    serde_json::json!(event)
}

pub(crate) fn exec_command_end(event: &ExecCommandEndEvent) -> serde_json::Value {
    serde_json::json!({
        "call_id": &event.call_id,
        "process_id": &event.process_id,
        "turn_id": &event.turn_id,
        "command": &event.command,
        "cwd": &event.cwd,
        "parsed_cmd": &event.parsed_cmd,
        "source": &event.source,
        "interaction_input": &event.interaction_input,
        "exit_code": event.exit_code,
        "duration": event.duration,
        "status": &event.status,
        "stdout_bytes": event.stdout.len(),
        "stderr_bytes": event.stderr.len(),
        "aggregated_output_bytes": event.aggregated_output.len(),
        "formatted_output_bytes": event.formatted_output.len(),
        "output_omitted": true,
    })
}

pub(crate) fn patch_approval_request(event: &ApplyPatchApprovalRequestEvent) -> serde_json::Value {
    serde_json::json!(event)
}

pub(crate) fn patch_apply_begin(event: &PatchApplyBeginEvent) -> serde_json::Value {
    serde_json::json!(event)
}

pub(crate) fn patch_apply_updated(event: &PatchApplyUpdatedEvent) -> serde_json::Value {
    serde_json::json!(event)
}

pub(crate) fn patch_apply_end(event: &PatchApplyEndEvent) -> serde_json::Value {
    serde_json::json!(event)
}

pub(crate) fn guardian_assessment(event: &GuardianAssessmentEvent) -> serde_json::Value {
    serde_json::json!(event)
}

pub(crate) fn request_permissions(event: &RequestPermissionsEvent) -> serde_json::Value {
    serde_json::json!(event)
}

pub(crate) fn mcp_elicitation(event: &ElicitationRequestEvent) -> serde_json::Value {
    serde_json::json!(event)
}

pub(crate) fn mcp_invocation(invocation: &McpInvocation) -> serde_json::Value {
    serde_json::json!(invocation)
}

pub(crate) fn mcp_tool_call_output(result: Result<&CallToolResult, &String>) -> serde_json::Value {
    match result {
        Ok(result) => serde_json::json!(result),
        Err(err) => serde_json::json!(err),
    }
}

pub(crate) fn dynamic_tool_arguments(arguments: &serde_json::Value) -> serde_json::Value {
    arguments.clone()
}

pub(crate) fn dynamic_tool_response(event: &DynamicToolCallResponseEvent) -> serde_json::Value {
    serde_json::json!(event)
}

pub(crate) fn web_search_update(query: &str, action: &WebSearchAction) -> serde_json::Value {
    serde_json::json!({
        "query": query,
        "action": action,
    })
}

pub(crate) fn image_generation_end(event: &ImageGenerationEndEvent) -> serde_json::Value {
    serde_json::json!(event)
}

pub(crate) fn collab_spawn_begin(event: &CollabAgentSpawnBeginEvent) -> serde_json::Value {
    serde_json::json!(event)
}

pub(crate) fn collab_spawn_end(event: &CollabAgentSpawnEndEvent) -> serde_json::Value {
    serde_json::json!(event)
}

pub(crate) fn collab_interaction_begin(
    event: &CollabAgentInteractionBeginEvent,
) -> serde_json::Value {
    serde_json::json!(event)
}

pub(crate) fn collab_interaction_end(event: &CollabAgentInteractionEndEvent) -> serde_json::Value {
    serde_json::json!(event)
}

pub(crate) fn collab_waiting_begin(event: &CollabWaitingBeginEvent) -> serde_json::Value {
    serde_json::json!(event)
}

pub(crate) fn collab_waiting_end(event: &CollabWaitingEndEvent) -> serde_json::Value {
    serde_json::json!(event)
}

pub(crate) fn collab_close_begin(event: &CollabCloseBeginEvent) -> serde_json::Value {
    serde_json::json!(event)
}

pub(crate) fn collab_close_end(event: &CollabCloseEndEvent) -> serde_json::Value {
    serde_json::json!(event)
}

pub(crate) fn collab_resume_begin(event: &CollabResumeBeginEvent) -> serde_json::Value {
    serde_json::json!(event)
}

pub(crate) fn collab_resume_end(event: &CollabResumeEndEvent) -> serde_json::Value {
    serde_json::json!(event)
}

pub(crate) fn response_item_function_call_output(
    output: &FunctionCallOutputPayload,
) -> Option<serde_json::Value> {
    serde_json::to_value(output).ok()
}

pub(crate) fn response_item_custom_tool_call_output(
    output: &FunctionCallOutputPayload,
) -> serde_json::Value {
    serde_json::json!(output)
}
