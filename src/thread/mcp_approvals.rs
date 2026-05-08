use std::collections::HashMap;

use agent_client_protocol::schema::{
    Content, ContentBlock, PermissionOption, PermissionOptionKind, TextContent, ToolCallContent,
    ToolCallId, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields,
};
use codex_protocol::{approvals::ElicitationRequest, mcp::RequestId};

use super::approvals::{ResolvedMcpElicitation, mcp_elicitation_request_key};

pub(super) const MCP_TOOL_APPROVAL_KIND_KEY: &str = "codex_approval_kind";
pub(super) const MCP_TOOL_APPROVAL_KIND_MCP_TOOL_CALL: &str = "mcp_tool_call";
pub(super) const MCP_TOOL_APPROVAL_PERSIST_KEY: &str = "persist";
pub(super) const MCP_TOOL_APPROVAL_PERSIST_SESSION: &str = "session";
pub(super) const MCP_TOOL_APPROVAL_PERSIST_ALWAYS: &str = "always";
pub(super) const MCP_TOOL_APPROVAL_TOOL_TITLE_KEY: &str = "tool_title";
pub(super) const MCP_TOOL_APPROVAL_TOOL_DESCRIPTION_KEY: &str = "tool_description";
pub(super) const MCP_TOOL_APPROVAL_CONNECTOR_NAME_KEY: &str = "connector_name";
pub(super) const MCP_TOOL_APPROVAL_CONNECTOR_DESCRIPTION_KEY: &str = "connector_description";
pub(super) const MCP_TOOL_APPROVAL_TOOL_PARAMS_KEY: &str = "tool_params";
pub(super) const MCP_TOOL_APPROVAL_TOOL_PARAMS_DISPLAY_KEY: &str = "tool_params_display";
pub(super) const MCP_TOOL_APPROVAL_REQUEST_ID_PREFIX: &str = "mcp_tool_call_approval_";
pub(super) const MCP_TOOL_APPROVAL_ALLOW_OPTION_ID: &str = "approved";
pub(super) const MCP_TOOL_APPROVAL_ALLOW_SESSION_OPTION_ID: &str = "approved-for-session";
pub(super) const MCP_TOOL_APPROVAL_ALLOW_ALWAYS_OPTION_ID: &str = "approved-always";
pub(super) const MCP_TOOL_APPROVAL_CANCEL_OPTION_ID: &str = "cancel";

pub(super) struct SupportedMcpElicitationPermissionRequest {
    pub(super) request_key: String,
    pub(super) tool_call: ToolCallUpdate,
    pub(super) options: Vec<PermissionOption>,
    pub(super) option_map: HashMap<String, ResolvedMcpElicitation>,
}

pub(super) fn build_supported_mcp_elicitation_permission_request(
    server_name: &str,
    request_id: &RequestId,
    request: &ElicitationRequest,
    raw_input: serde_json::Value,
) -> Option<SupportedMcpElicitationPermissionRequest> {
    let ElicitationRequest::Form {
        meta: Some(meta),
        message,
        requested_schema: _,
    } = request
    else {
        return None;
    };
    let meta = meta.as_object()?;
    if meta
        .get(MCP_TOOL_APPROVAL_KIND_KEY)
        .and_then(serde_json::Value::as_str)
        != Some(MCP_TOOL_APPROVAL_KIND_MCP_TOOL_CALL)
    {
        return None;
    }

    let (allow_session_remember, allow_persistent_approval) = mcp_tool_approval_persist_modes(meta);
    let mut options = vec![PermissionOption::new(
        MCP_TOOL_APPROVAL_ALLOW_OPTION_ID,
        "Allow",
        PermissionOptionKind::AllowOnce,
    )];
    let mut option_map = HashMap::from([(
        MCP_TOOL_APPROVAL_ALLOW_OPTION_ID.to_string(),
        ResolvedMcpElicitation::accept(),
    )]);

    if allow_session_remember {
        options.push(PermissionOption::new(
            MCP_TOOL_APPROVAL_ALLOW_SESSION_OPTION_ID,
            "Allow for this session",
            PermissionOptionKind::AllowAlways,
        ));
        option_map.insert(
            MCP_TOOL_APPROVAL_ALLOW_SESSION_OPTION_ID.to_string(),
            ResolvedMcpElicitation::accept_with_persist(MCP_TOOL_APPROVAL_PERSIST_SESSION),
        );
    }

    if allow_persistent_approval {
        options.push(PermissionOption::new(
            MCP_TOOL_APPROVAL_ALLOW_ALWAYS_OPTION_ID,
            "Allow and don't ask again",
            PermissionOptionKind::AllowAlways,
        ));
        option_map.insert(
            MCP_TOOL_APPROVAL_ALLOW_ALWAYS_OPTION_ID.to_string(),
            ResolvedMcpElicitation::accept_with_persist(MCP_TOOL_APPROVAL_PERSIST_ALWAYS),
        );
    }

    options.push(PermissionOption::new(
        MCP_TOOL_APPROVAL_CANCEL_OPTION_ID,
        "Cancel",
        PermissionOptionKind::RejectOnce,
    ));
    option_map.insert(
        MCP_TOOL_APPROVAL_CANCEL_OPTION_ID.to_string(),
        ResolvedMcpElicitation::cancel(),
    );

    let tool_call_id = mcp_tool_approval_call_id(request_id)
        .unwrap_or_else(|| format!("mcp-elicitation:{request_id}"));
    let title = meta
        .get(MCP_TOOL_APPROVAL_TOOL_TITLE_KEY)
        .and_then(serde_json::Value::as_str)
        .filter(|title| !title.trim().is_empty())
        .map(|title| format!("Approve {title}"))
        .unwrap_or_else(|| "Approve MCP tool call".to_string());
    let content = format_mcp_tool_approval_content(server_name, message, meta);

    Some(SupportedMcpElicitationPermissionRequest {
        request_key: mcp_elicitation_request_key(server_name, request_id),
        tool_call: ToolCallUpdate::new(
            ToolCallId::new(tool_call_id),
            ToolCallUpdateFields::new()
                .status(ToolCallStatus::Pending)
                .title(title)
                .content(vec![ToolCallContent::Content(Content::new(
                    ContentBlock::Text(TextContent::new(content)),
                ))])
                .raw_input(raw_input),
        ),
        options,
        option_map,
    })
}

fn mcp_tool_approval_call_id(request_id: &RequestId) -> Option<String> {
    match request_id {
        RequestId::String(value) => value
            .strip_prefix(MCP_TOOL_APPROVAL_REQUEST_ID_PREFIX)
            .map(ToString::to_string),
        RequestId::Integer(_) => None,
    }
}

fn mcp_tool_approval_persist_modes(
    meta: &serde_json::Map<String, serde_json::Value>,
) -> (bool, bool) {
    match meta.get(MCP_TOOL_APPROVAL_PERSIST_KEY) {
        Some(serde_json::Value::String(persist)) => (
            persist == MCP_TOOL_APPROVAL_PERSIST_SESSION,
            persist == MCP_TOOL_APPROVAL_PERSIST_ALWAYS,
        ),
        Some(serde_json::Value::Array(values)) => (
            values
                .iter()
                .any(|value| value.as_str() == Some(MCP_TOOL_APPROVAL_PERSIST_SESSION)),
            values
                .iter()
                .any(|value| value.as_str() == Some(MCP_TOOL_APPROVAL_PERSIST_ALWAYS)),
        ),
        _ => (false, false),
    }
}

fn format_mcp_tool_approval_content(
    server_name: &str,
    message: &str,
    meta: &serde_json::Map<String, serde_json::Value>,
) -> String {
    let mut sections = vec![message.trim().to_string()];

    let source = meta
        .get(MCP_TOOL_APPROVAL_CONNECTOR_NAME_KEY)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(|value| format!("Source: {value}"))
        .unwrap_or_else(|| format!("Server: {server_name}"));
    sections.push(source);

    if let Some(description) = meta
        .get(MCP_TOOL_APPROVAL_CONNECTOR_DESCRIPTION_KEY)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
    {
        sections.push(description.to_string());
    }

    if let Some(description) = meta
        .get(MCP_TOOL_APPROVAL_TOOL_DESCRIPTION_KEY)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
    {
        sections.push(description.to_string());
    }

    if let Some(params) = format_mcp_tool_approval_params(meta) {
        sections.push(format!("Arguments:\n{params}"));
    }

    sections.join("\n\n")
}

fn format_mcp_tool_approval_params(
    meta: &serde_json::Map<String, serde_json::Value>,
) -> Option<String> {
    if let Some(serde_json::Value::Array(params)) =
        meta.get(MCP_TOOL_APPROVAL_TOOL_PARAMS_DISPLAY_KEY)
    {
        let params = params
            .iter()
            .filter_map(|param| {
                let object = param.as_object()?;
                let name = object
                    .get("display_name")
                    .and_then(serde_json::Value::as_str)
                    .or_else(|| object.get("name").and_then(serde_json::Value::as_str))?;
                let value = object.get("value")?;
                Some(format!(
                    "- {name}: {}",
                    format_mcp_tool_approval_value(value)
                ))
            })
            .collect::<Vec<_>>();
        if !params.is_empty() {
            return Some(params.join("\n"));
        }
    }

    meta.get(MCP_TOOL_APPROVAL_TOOL_PARAMS_KEY).map(|params| {
        serde_json::to_string_pretty(params)
            .unwrap_or_else(|_| format_mcp_tool_approval_value(params))
    })
}

fn format_mcp_tool_approval_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.clone(),
        _ => serde_json::to_string(value).unwrap_or_else(|_| value.to_string()),
    }
}
