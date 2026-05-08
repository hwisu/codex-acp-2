use agent_client_protocol::schema::{
    PermissionOption, PermissionOptionKind, ToolCallId, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields,
};
use codex_protocol::{
    permissions::FileSystemAccessMode, request_permissions::RequestPermissionsEvent,
};

use crate::guardian::format_file_system_entries;

use super::{
    approvals::{PendingPermissionRequest, permissions_request_key},
    client::SessionClient,
    submission::{PermissionInteractionRequest, PromptState},
};

impl PromptState {
    pub(super) fn request_permissions(
        &mut self,
        client: &SessionClient,
        event: RequestPermissionsEvent,
    ) {
        let raw_input = serde_json::json!(&event);
        let RequestPermissionsEvent {
            call_id,
            turn_id: _,
            reason,
            permissions,
            cwd: _,
        } = event;

        // Create a new tool call for the command execution
        let tool_call_id = ToolCallId::new(call_id.clone());

        let mut content = vec![];

        if let Some(reason) = reason.as_ref() {
            content.push(reason.clone());
        }
        if let Some(file_system) = permissions.file_system.as_ref() {
            let reads = format_file_system_entries(
                file_system
                    .entries
                    .iter()
                    .filter(|entry| entry.access == FileSystemAccessMode::Read),
            );
            if !reads.is_empty() {
                content.push(format!("File System Read Access: {reads}"));
            }
            let writes = format_file_system_entries(
                file_system
                    .entries
                    .iter()
                    .filter(|entry| entry.access == FileSystemAccessMode::Write),
            );
            if !writes.is_empty() {
                content.push(format!("File System Write Access: {writes}"));
            }
            let denies = format_file_system_entries(
                file_system
                    .entries
                    .iter()
                    .filter(|entry| entry.access == FileSystemAccessMode::None),
            );
            if !denies.is_empty() {
                content.push(format!("File System Denied Access: {denies}"));
            }
        }
        if let Some(network) = permissions.network.as_ref()
            && let Some(enabled) = network.enabled
        {
            content.push(format!("Network Access: {enabled}"));
        }

        let content = if content.is_empty() {
            None
        } else {
            Some(vec![content.join("\n").into()])
        };

        self.spawn_permission_request(
            client,
            PermissionInteractionRequest {
                request_key: permissions_request_key(&call_id),
                pending_request: PendingPermissionRequest::RequestPermissions {
                    call_id,
                    permissions,
                },
                tool_call: ToolCallUpdate::new(
                    tool_call_id,
                    ToolCallUpdateFields::new()
                        .status(ToolCallStatus::Pending)
                        .title(reason.unwrap_or_else(|| "Permissions Request".to_string()))
                        .raw_input(raw_input)
                        .content(content),
                ),
                options: vec![
                    PermissionOption::new(
                        "approved-for-session",
                        "Yes, for session",
                        PermissionOptionKind::AllowAlways,
                    ),
                    PermissionOption::new("approved", "Yes", PermissionOptionKind::AllowOnce),
                    PermissionOption::new("abort", "No", PermissionOptionKind::RejectOnce),
                ],
            },
        );
    }
}
