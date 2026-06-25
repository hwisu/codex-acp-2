use std::collections::HashMap;

use agent_client_protocol::{Error, schema::v1::RequestPermissionResponse};
use tracing::warn;

use crate::{
    boundary::{approval, effect::BridgeEffect},
    thread::{
        ThreadMessage,
        approvals::{PendingPermissionInteraction, PendingPermissionRequest},
        client::SessionClient,
        submission::{PermissionInteractionRequest, PromptState},
    },
};

#[derive(Default)]
pub(super) struct PermissionInteractionState {
    interactions: HashMap<String, PendingPermissionInteraction>,
}

impl PermissionInteractionState {
    fn abort_all(&mut self) {
        for (_, interaction) in self.interactions.drain() {
            interaction.task.abort();
        }
    }

    fn spawn(
        &mut self,
        client: &SessionClient,
        resolution_tx: tokio::sync::mpsc::UnboundedSender<ThreadMessage>,
        submission_id: String,
        request: PermissionInteractionRequest,
    ) {
        let PermissionInteractionRequest {
            request_key,
            pending_request,
            request_effect,
        } = request;
        let client = client.clone();
        let resolved_request_key = request_key.clone();
        let handle = tokio::spawn(async move {
            let response = match request_effect {
                BridgeEffect::RequestPermission(request) => {
                    client.request_permission_request(request).await
                }
                _other_effect => Err(Error::internal_error()
                    .data("permission interaction received non-permission bridge effect")),
            };
            drop(
                resolution_tx.send(ThreadMessage::PermissionRequestResolved {
                    submission_id,
                    request_key: resolved_request_key,
                    response,
                }),
            );
        });

        if let Some(interaction) = self.interactions.insert(
            request_key,
            PendingPermissionInteraction {
                request: pending_request,
                task: handle,
            },
        ) {
            interaction.task.abort();
        }
    }

    async fn resolve(
        &mut self,
        request_key: String,
        response: Result<RequestPermissionResponse, Error>,
    ) -> Result<Option<BridgeEffect>, Error> {
        let Some(interaction) = self.interactions.remove(&request_key) else {
            warn!("Ignoring permission response for unknown request key: {request_key}");
            return Ok(None);
        };
        let pending_request = interaction.request;
        let response = response?;

        let op = match pending_request {
            PendingPermissionRequest::Exec {
                approval_id,
                turn_id,
                option_map,
            } => approval::exec_approval_op(approval_id, turn_id, response.outcome, &option_map),
            PendingPermissionRequest::Patch {
                call_id,
                option_map,
            } => approval::patch_approval_op(call_id, response.outcome, &option_map),
            PendingPermissionRequest::RequestPermissions {
                call_id,
                permissions,
            } => approval::request_permissions_op(call_id, response.outcome, &permissions),
            PendingPermissionRequest::McpElicitation {
                server_name,
                request_id,
                option_map,
            } => approval::resolve_mcp_elicitation_op(
                server_name,
                request_id,
                response.outcome,
                &option_map,
            ),
        };

        Ok(Some(BridgeEffect::SubmitOp(op)))
    }
}

impl PromptState {
    pub(in crate::thread) fn abort_pending_interactions(&mut self) {
        self.permission_interactions.abort_all();
    }

    pub(in crate::thread) fn spawn_permission_request(
        &mut self,
        client: &SessionClient,
        request: PermissionInteractionRequest,
    ) {
        self.permission_interactions.spawn(
            client,
            self.resolution_tx.clone(),
            self.submission_id.clone(),
            request,
        );
    }

    pub(in crate::thread) async fn handle_permission_request_resolved(
        &mut self,
        _client: &SessionClient,
        request_key: String,
        response: Result<RequestPermissionResponse, Error>,
    ) -> Result<(), Error> {
        if let Some(effect) = self
            .permission_interactions
            .resolve(request_key, response)
            .await?
        {
            self.execute_bridge_effect(_client, effect).await?;
        }

        Ok(())
    }
}
