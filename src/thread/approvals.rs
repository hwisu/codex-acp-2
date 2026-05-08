use std::collections::HashMap;

use agent_client_protocol::schema::{RequestPermissionOutcome, SelectedPermissionOutcome};
use codex_protocol::{
    approvals::ElicitationAction, mcp::RequestId, protocol::ReviewDecision,
    request_permissions::RequestPermissionProfile,
};
use tokio::task::JoinHandle;

pub(super) use super::mcp_approvals::build_supported_mcp_elicitation_permission_request;
#[cfg(test)]
pub(super) use super::mcp_approvals::{
    MCP_TOOL_APPROVAL_ALLOW_ALWAYS_OPTION_ID, MCP_TOOL_APPROVAL_ALLOW_OPTION_ID,
    MCP_TOOL_APPROVAL_ALLOW_SESSION_OPTION_ID, MCP_TOOL_APPROVAL_CANCEL_OPTION_ID,
    MCP_TOOL_APPROVAL_PERSIST_SESSION, MCP_TOOL_APPROVAL_REQUEST_ID_PREFIX,
};

pub(super) fn resolve_review_decision(
    outcome: RequestPermissionOutcome,
    option_map: &HashMap<String, ReviewDecision>,
) -> ReviewDecision {
    match outcome {
        RequestPermissionOutcome::Selected(SelectedPermissionOutcome { option_id, .. }) => {
            option_map
                .get(option_id.0.as_ref())
                .cloned()
                .unwrap_or(ReviewDecision::Abort)
        }
        _ => ReviewDecision::Abort,
    }
}

pub(super) enum PendingPermissionRequest {
    Exec {
        approval_id: String,
        turn_id: String,
        option_map: HashMap<String, ReviewDecision>,
    },
    Patch {
        call_id: String,
        option_map: HashMap<String, ReviewDecision>,
    },
    RequestPermissions {
        call_id: String,
        permissions: RequestPermissionProfile,
    },
    McpElicitation {
        server_name: String,
        request_id: RequestId,
        option_map: HashMap<String, ResolvedMcpElicitation>,
    },
}

pub(super) struct PendingPermissionInteraction {
    pub(super) request: PendingPermissionRequest,
    pub(super) task: JoinHandle<()>,
}

#[derive(Clone)]
pub(super) struct ResolvedMcpElicitation {
    pub(super) action: ElicitationAction,
    pub(super) content: Option<serde_json::Value>,
    pub(super) meta: Option<serde_json::Value>,
}

impl ResolvedMcpElicitation {
    pub(super) fn accept() -> Self {
        Self {
            action: ElicitationAction::Accept,
            content: None,
            meta: None,
        }
    }

    pub(super) fn accept_with_persist(persist: &'static str) -> Self {
        Self {
            action: ElicitationAction::Accept,
            content: None,
            meta: Some(serde_json::json!({ "persist": persist })),
        }
    }

    pub(super) fn cancel() -> Self {
        Self {
            action: ElicitationAction::Cancel,
            content: None,
            meta: None,
        }
    }
}

pub(super) fn exec_request_key(call_id: &str) -> String {
    format!("exec:{call_id}")
}

pub(super) fn patch_request_key(call_id: &str) -> String {
    format!("patch:{call_id}")
}

pub(super) fn permissions_request_key(call_id: &str) -> String {
    format!("permissions:{call_id}")
}

pub(super) fn mcp_elicitation_request_key(server_name: &str, request_id: &RequestId) -> String {
    format!("mcp-elicitation:{server_name}:{request_id}")
}
