use codex_protocol::protocol::ApplyPatchApprovalRequestEvent;

use crate::boundary::permission;

use super::{
    approvals::PendingPermissionRequest,
    client::SessionClient,
    submission::{PermissionInteractionRequest, PromptState},
};

impl PromptState {
    pub(super) fn patch_approval(
        &mut self,
        client: &SessionClient,
        event: ApplyPatchApprovalRequestEvent,
    ) {
        let request = permission::patch_approval_interaction(event);
        let permission::PatchApprovalInteraction {
            request_key,
            call_id,
            option_map,
            permission_request,
        } = request;
        self.spawn_permission_request(
            client,
            PermissionInteractionRequest {
                request_key,
                pending_request: PendingPermissionRequest::Patch {
                    call_id,
                    option_map,
                },
                request_effect: client.request_permission_effect(permission_request),
            },
        );
    }
}
