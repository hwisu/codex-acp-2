use std::collections::HashMap;

use agent_client_protocol::schema::{
    Content, ContentBlock, PermissionOption, PermissionOptionKind, TextContent, ToolCallContent,
    ToolCallId, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields,
};
use codex_protocol::{
    approvals::{ElicitationRequest, ElicitationRequestEvent},
    mcp::RequestId,
    protocol::{ElicitationAction, Op},
};

use crate::boundary::{
    approval::McpElicitationResolution, constants::mcp_approval, effect::PermissionRequestSeed, raw,
};

pub(crate) struct SupportedMcpElicitationPermissionRequest {
    pub(crate) request_key: String,
    pub(crate) permission_request: PermissionRequestSeed,
    pub(crate) option_map: HashMap<String, McpElicitationResolution>,
}

pub(crate) struct UnsupportedMcpElicitationDecline {
    pub(crate) request_kind: &'static str,
    pub(crate) op: Op,
}

pub(crate) fn build_supported_mcp_elicitation_permission_request(
    event: &ElicitationRequestEvent,
) -> Option<SupportedMcpElicitationPermissionRequest> {
    let raw_input = raw::mcp_elicitation(event);
    let ElicitationRequestEvent {
        server_name,
        id,
        request,
        turn_id: _,
    } = event;
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
        .get(mcp_approval::KIND_KEY)
        .and_then(serde_json::Value::as_str)
        != Some(mcp_approval::KIND_MCP_TOOL_CALL)
    {
        return None;
    }

    let (allow_session_remember, allow_persistent_approval) = mcp_tool_approval_persist_modes(meta);
    let mut options = vec![PermissionOption::new(
        mcp_approval::ALLOW_OPTION_ID,
        "Allow",
        PermissionOptionKind::AllowOnce,
    )];
    let mut option_map = HashMap::from([(
        mcp_approval::ALLOW_OPTION_ID.to_string(),
        McpElicitationResolution::accept(),
    )]);

    if allow_session_remember {
        options.push(PermissionOption::new(
            mcp_approval::ALLOW_SESSION_OPTION_ID,
            "Allow for this session",
            PermissionOptionKind::AllowAlways,
        ));
        option_map.insert(
            mcp_approval::ALLOW_SESSION_OPTION_ID.to_string(),
            McpElicitationResolution::accept_with_persist(mcp_approval::PERSIST_SESSION),
        );
    }

    if allow_persistent_approval {
        options.push(PermissionOption::new(
            mcp_approval::ALLOW_ALWAYS_OPTION_ID,
            "Allow and don't ask again",
            PermissionOptionKind::AllowAlways,
        ));
        option_map.insert(
            mcp_approval::ALLOW_ALWAYS_OPTION_ID.to_string(),
            McpElicitationResolution::accept_with_persist(mcp_approval::PERSIST_ALWAYS),
        );
    }

    options.push(PermissionOption::new(
        mcp_approval::CANCEL_OPTION_ID,
        "Cancel",
        PermissionOptionKind::RejectOnce,
    ));
    option_map.insert(
        mcp_approval::CANCEL_OPTION_ID.to_string(),
        McpElicitationResolution::cancel(),
    );

    let tool_call_id =
        mcp_tool_approval_call_id(id).unwrap_or_else(|| format!("mcp-elicitation:{id}"));
    let title = meta
        .get(mcp_approval::TOOL_TITLE_KEY)
        .and_then(serde_json::Value::as_str)
        .filter(|title| !title.trim().is_empty())
        .map(|title| format!("Approve {title}"))
        .unwrap_or_else(|| "Approve MCP tool call".to_string());
    let content = format_mcp_tool_approval_content(server_name, message, meta);

    Some(SupportedMcpElicitationPermissionRequest {
        request_key: mcp_elicitation_request_key(server_name, id),
        permission_request: PermissionRequestSeed::new(
            ToolCallUpdate::new(
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
        ),
        option_map,
    })
}

pub(crate) fn unsupported_mcp_elicitation_decline(
    server_name: String,
    request_id: RequestId,
    request: &ElicitationRequest,
) -> UnsupportedMcpElicitationDecline {
    UnsupportedMcpElicitationDecline {
        request_kind: mcp_elicitation_request_kind(request),
        op: Op::ResolveElicitation {
            server_name,
            request_id,
            decision: ElicitationAction::Decline,
            content: None,
            meta: None,
        },
    }
}

fn mcp_elicitation_request_key(server_name: &str, request_id: &RequestId) -> String {
    format!("mcp-elicitation:{server_name}:{request_id}")
}

fn mcp_elicitation_request_kind(request: &ElicitationRequest) -> &'static str {
    match request {
        ElicitationRequest::Form { .. } => "form",
        ElicitationRequest::Url { .. } => "url",
    }
}

fn mcp_tool_approval_call_id(request_id: &RequestId) -> Option<String> {
    match request_id {
        RequestId::String(value) => value
            .strip_prefix(mcp_approval::REQUEST_ID_PREFIX)
            .map(ToString::to_string),
        RequestId::Integer(_) => None,
    }
}

fn mcp_tool_approval_persist_modes(
    meta: &serde_json::Map<String, serde_json::Value>,
) -> (bool, bool) {
    match meta.get(mcp_approval::PERSIST_KEY) {
        Some(serde_json::Value::String(persist)) => (
            persist == mcp_approval::PERSIST_SESSION,
            persist == mcp_approval::PERSIST_ALWAYS,
        ),
        Some(serde_json::Value::Array(values)) => (
            values
                .iter()
                .any(|value| value.as_str() == Some(mcp_approval::PERSIST_SESSION)),
            values
                .iter()
                .any(|value| value.as_str() == Some(mcp_approval::PERSIST_ALWAYS)),
        ),
        _unknown_persist_shape => (false, false),
    }
}

fn format_mcp_tool_approval_content(
    server_name: &str,
    message: &str,
    meta: &serde_json::Map<String, serde_json::Value>,
) -> String {
    let mut sections = vec![message.trim().to_string()];

    let source = meta
        .get(mcp_approval::CONNECTOR_NAME_KEY)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(|value| format!("Source: {value}"))
        .unwrap_or_else(|| format!("Server: {server_name}"));
    sections.push(source);

    if let Some(description) = meta
        .get(mcp_approval::CONNECTOR_DESCRIPTION_KEY)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
    {
        sections.push(description.to_string());
    }

    if let Some(description) = meta
        .get(mcp_approval::TOOL_DESCRIPTION_KEY)
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
    if let Some(serde_json::Value::Array(params)) = meta.get(mcp_approval::TOOL_PARAMS_DISPLAY_KEY)
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

    meta.get(mcp_approval::TOOL_PARAMS_KEY).map(|params| {
        serde_json::to_string_pretty(params)
            .unwrap_or_else(|_format_error| format_mcp_tool_approval_value(params))
    })
}

fn format_mcp_tool_approval_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.clone(),
        _other_value => {
            serde_json::to_string(value).unwrap_or_else(|_format_error| value.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use codex_protocol::approvals::ElicitationRequestEvent;

    #[test]
    fn supported_mcp_tool_approval_builds_permission_request() {
        let event = ElicitationRequestEvent {
            turn_id: Some("turn-id".to_string()),
            server_name: "docs-server".to_string(),
            id: RequestId::String(format!("{}call-123", mcp_approval::REQUEST_ID_PREFIX)),
            request: ElicitationRequest::Form {
                meta: Some(serde_json::json!({
                    mcp_approval::KIND_KEY: mcp_approval::KIND_MCP_TOOL_CALL,
                    mcp_approval::PERSIST_KEY: [
                        mcp_approval::PERSIST_SESSION,
                        mcp_approval::PERSIST_ALWAYS
                    ],
                    mcp_approval::CONNECTOR_NAME_KEY: "Docs",
                    mcp_approval::TOOL_TITLE_KEY: "search_docs",
                    mcp_approval::TOOL_DESCRIPTION_KEY: "Search project documentation",
                    mcp_approval::TOOL_PARAMS_DISPLAY_KEY: [
                        {
                            "display_name": "Query",
                            "name": "query",
                            "value": "approval flow"
                        }
                    ]
                })),
                message: "Allow Docs to run tool?".to_string(),
                requested_schema: serde_json::json!({ "type": "object" }),
            },
        };

        let request =
            build_supported_mcp_elicitation_permission_request(&event).expect("supported request");

        assert_eq!(
            request.request_key,
            "mcp-elicitation:docs-server:mcp_tool_call_approval_call-123"
        );
        assert_eq!(
            request.permission_request.tool_call_id().0.as_ref(),
            "call-123"
        );
        assert_eq!(
            request.permission_request.option_ids(),
            vec![
                mcp_approval::ALLOW_OPTION_ID.to_string(),
                mcp_approval::ALLOW_SESSION_OPTION_ID.to_string(),
                mcp_approval::ALLOW_ALWAYS_OPTION_ID.to_string(),
                mcp_approval::CANCEL_OPTION_ID.to_string(),
            ]
        );
    }

    #[test]
    fn unsupported_mcp_elicitation_declines_explicitly() {
        let request = ElicitationRequest::Url {
            meta: None,
            message: "Open this URL".to_string(),
            url: "https://example.com".to_string(),
            elicitation_id: "elicitation-id".to_string(),
        };
        let decline = unsupported_mcp_elicitation_decline(
            "server".to_string(),
            RequestId::String("request-id".to_string()),
            &request,
        );

        assert_eq!(decline.request_kind, "url");
        assert!(matches!(
            decline.op,
            Op::ResolveElicitation {
                server_name,
                request_id: RequestId::String(request_id),
                decision: ElicitationAction::Decline,
                content: None,
                meta: None,
            } if server_name == "server" && request_id == "request-id"
        ));
    }
}
