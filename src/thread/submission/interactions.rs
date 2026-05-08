use std::{collections::HashMap, sync::Arc};

use agent_client_protocol::{
    Error,
    schema::{RequestPermissionOutcome, RequestPermissionResponse, SelectedPermissionOutcome},
};
use codex_protocol::{
    protocol::Op,
    request_permissions::{
        PermissionGrantScope, RequestPermissionProfile, RequestPermissionsResponse,
    },
};
use tracing::warn;

use crate::thread::{
    ThreadMessage,
    approvals::{
        PendingPermissionInteraction, PendingPermissionRequest, ResolvedMcpElicitation,
        resolve_review_decision,
    },
    client::SessionClient,
    deps::CodexThreadImpl,
    submission::{PermissionInteractionRequest, PromptState},
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
            tool_call,
            options,
        } = request;
        let client = client.clone();
        let resolved_request_key = request_key.clone();
        let handle = tokio::spawn(async move {
            let response = client.request_permission(tool_call, options).await;
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
        thread: &Arc<dyn CodexThreadImpl>,
        request_key: String,
        response: Result<RequestPermissionResponse, Error>,
    ) -> Result<(), Error> {
        let Some(interaction) = self.interactions.remove(&request_key) else {
            warn!("Ignoring permission response for unknown request key: {request_key}");
            return Ok(());
        };
        let pending_request = interaction.request;
        let response = response?;

        match pending_request {
            PendingPermissionRequest::Exec {
                approval_id,
                turn_id,
                option_map,
            } => {
                let decision = resolve_review_decision(response.outcome, &option_map);

                thread
                    .submit_ok(Op::ExecApproval {
                        id: approval_id,
                        turn_id: Some(turn_id),
                        decision,
                    })
                    .await?;
            }
            PendingPermissionRequest::Patch {
                call_id,
                option_map,
            } => {
                let decision = resolve_review_decision(response.outcome, &option_map);

                thread
                    .submit_ok(Op::PatchApproval {
                        id: call_id,
                        decision,
                    })
                    .await?;
            }
            PendingPermissionRequest::RequestPermissions {
                call_id,
                permissions,
            } => {
                let response = request_permissions_response(response.outcome, &permissions);

                thread
                    .submit_ok(Op::RequestPermissionsResponse {
                        id: call_id,
                        response,
                    })
                    .await?;
            }
            PendingPermissionRequest::McpElicitation {
                server_name,
                request_id,
                option_map,
            } => {
                let response = match response.outcome {
                    RequestPermissionOutcome::Selected(SelectedPermissionOutcome {
                        option_id,
                        ..
                    }) => option_map
                        .get(option_id.0.as_ref())
                        .cloned()
                        .unwrap_or_else(ResolvedMcpElicitation::cancel),
                    RequestPermissionOutcome::Cancelled | _ => ResolvedMcpElicitation::cancel(),
                };

                thread
                    .submit_ok(Op::ResolveElicitation {
                        server_name,
                        request_id,
                        decision: response.action,
                        content: response.content,
                        meta: response.meta,
                    })
                    .await?;
            }
        }

        Ok(())
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
        self.permission_interactions
            .resolve(&self.thread, request_key, response)
            .await
    }
}

fn request_permissions_response(
    outcome: RequestPermissionOutcome,
    permissions: &RequestPermissionProfile,
) -> RequestPermissionsResponse {
    match outcome {
        RequestPermissionOutcome::Selected(SelectedPermissionOutcome { option_id, .. }) => {
            match option_id.0.as_ref() {
                "approved-for-session" => RequestPermissionsResponse {
                    permissions: permissions.clone(),
                    scope: PermissionGrantScope::Session,
                    strict_auto_review: false,
                },
                "approved" => RequestPermissionsResponse {
                    permissions: permissions.clone(),
                    scope: PermissionGrantScope::Turn,
                    strict_auto_review: false,
                },
                _ => denied_request_permissions_response(),
            }
        }
        RequestPermissionOutcome::Cancelled | _ => denied_request_permissions_response(),
    }
}

fn denied_request_permissions_response() -> RequestPermissionsResponse {
    RequestPermissionsResponse {
        permissions: RequestPermissionProfile::default(),
        scope: PermissionGrantScope::Turn,
        strict_auto_review: true,
    }
}
