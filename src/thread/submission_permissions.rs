use codex_protocol::request_permissions::RequestPermissionsEvent;

use crate::boundary::permission;

use super::{
    approvals::PendingPermissionRequest,
    client::SessionClient,
    submission::{PermissionInteractionRequest, PromptState},
};

impl PromptState {
    pub(super) fn request_permissions(
        &mut self,
        client: &SessionClient,
        event: RequestPermissionsEvent,
    ) {
        let request = permission::request_permissions_interaction(event);
        let permission::RequestPermissionsInteraction {
            request_key,
            call_id,
            permissions,
            permission_request,
        } = request;

        self.spawn_permission_request(
            client,
            PermissionInteractionRequest {
                request_key,
                pending_request: PendingPermissionRequest::RequestPermissions {
                    call_id,
                    permissions,
                },
                request_effect: client.request_permission_effect(permission_request),
            },
        );
    }
}
