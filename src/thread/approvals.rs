use std::collections::HashMap;

use codex_protocol::{
    mcp::RequestId, protocol::ReviewDecision, request_permissions::RequestPermissionProfile,
};
use tokio::task::JoinHandle;

use crate::boundary::approval::McpElicitationResolution;
#[cfg(test)]
pub(super) use crate::boundary::constants::mcp_approval::{
    ALLOW_ALWAYS_OPTION_ID as MCP_TOOL_APPROVAL_ALLOW_ALWAYS_OPTION_ID,
    ALLOW_OPTION_ID as MCP_TOOL_APPROVAL_ALLOW_OPTION_ID,
    ALLOW_SESSION_OPTION_ID as MCP_TOOL_APPROVAL_ALLOW_SESSION_OPTION_ID,
    CANCEL_OPTION_ID as MCP_TOOL_APPROVAL_CANCEL_OPTION_ID,
    PERSIST_SESSION as MCP_TOOL_APPROVAL_PERSIST_SESSION,
    REQUEST_ID_PREFIX as MCP_TOOL_APPROVAL_REQUEST_ID_PREFIX,
};

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
        option_map: HashMap<String, McpElicitationResolution>,
    },
}

pub(super) struct PendingPermissionInteraction {
    pub(super) request: PendingPermissionRequest,
    pub(super) task: JoinHandle<()>,
}
