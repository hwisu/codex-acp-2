use agent_client_protocol::Error;
use codex_protocol::approvals::ElicitationRequestEvent;
use tracing::info;

use crate::boundary::{effect::BridgeEffect, mcp_approval};

use super::{
    approvals::PendingPermissionRequest,
    client::SessionClient,
    submission::{PermissionInteractionRequest, PromptState},
};

impl PromptState {
    pub(super) async fn mcp_elicitation(
        &mut self,
        client: &SessionClient,
        event: ElicitationRequestEvent,
    ) -> Result<(), Error> {
        if let Some(supported_request) =
            mcp_approval::build_supported_mcp_elicitation_permission_request(&event)
        {
            let ElicitationRequestEvent {
                server_name,
                id,
                request: _,
                turn_id: _,
            } = event;
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
                    request_effect: client
                        .request_permission_effect(supported_request.permission_request),
                },
            );
            return Ok(());
        }

        let ElicitationRequestEvent {
            server_name,
            id,
            request,
            turn_id: _,
        } = event;
        let log_server_name = server_name.clone();
        let log_request_id = id.clone();
        let decline = mcp_approval::unsupported_mcp_elicitation_decline(server_name, id, &request);

        info!(
            "Auto-declining unsupported MCP elicitation: server={}, id={:?}, kind={}",
            log_server_name, log_request_id, decline.request_kind
        );

        self.execute_bridge_effect(client, BridgeEffect::SubmitOp(decline.op))
            .await?;

        Ok(())
    }
}
