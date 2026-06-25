use std::collections::HashMap;

use agent_client_protocol::schema::v1::{RequestPermissionOutcome, SelectedPermissionOutcome};
use codex_protocol::{
    approvals::ElicitationAction,
    mcp::RequestId,
    protocol::{Op, ReviewDecision},
    request_permissions::{
        PermissionGrantScope, RequestPermissionProfile, RequestPermissionsResponse,
    },
};

use crate::boundary::constants::{mcp_approval, permission_option};

#[derive(Clone)]
pub(crate) struct McpElicitationResolution {
    pub(crate) action: ElicitationAction,
    pub(crate) content: Option<serde_json::Value>,
    pub(crate) meta: Option<serde_json::Value>,
}

impl McpElicitationResolution {
    pub(crate) fn accept() -> Self {
        Self {
            action: ElicitationAction::Accept,
            content: None,
            meta: None,
        }
    }

    pub(crate) fn accept_with_persist(persist: &'static str) -> Self {
        Self {
            action: ElicitationAction::Accept,
            content: None,
            meta: Some(serde_json::json!({ mcp_approval::PERSIST_KEY: persist })),
        }
    }

    pub(crate) fn cancel() -> Self {
        Self {
            action: ElicitationAction::Cancel,
            content: None,
            meta: None,
        }
    }
}

pub(crate) fn exec_approval_op(
    approval_id: String,
    turn_id: String,
    outcome: RequestPermissionOutcome,
    option_map: &HashMap<String, ReviewDecision>,
) -> Op {
    Op::ExecApproval {
        id: approval_id,
        turn_id: Some(turn_id),
        decision: review_decision(outcome, option_map),
    }
}

pub(crate) fn patch_approval_op(
    call_id: String,
    outcome: RequestPermissionOutcome,
    option_map: &HashMap<String, ReviewDecision>,
) -> Op {
    Op::PatchApproval {
        id: call_id,
        decision: review_decision(outcome, option_map),
    }
}

pub(crate) fn request_permissions_op(
    call_id: String,
    outcome: RequestPermissionOutcome,
    permissions: &RequestPermissionProfile,
) -> Op {
    Op::RequestPermissionsResponse {
        id: call_id,
        response: request_permissions_response(outcome, permissions),
    }
}

pub(crate) fn resolve_mcp_elicitation_op(
    server_name: String,
    request_id: RequestId,
    outcome: RequestPermissionOutcome,
    option_map: &HashMap<String, McpElicitationResolution>,
) -> Op {
    let response = mcp_elicitation_resolution(outcome, option_map);
    Op::ResolveElicitation {
        server_name,
        request_id,
        decision: response.action,
        content: response.content,
        meta: response.meta,
    }
}

fn review_decision(
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
        RequestPermissionOutcome::Cancelled => ReviewDecision::Abort,
        _unknown_outcome => ReviewDecision::Abort,
    }
}

fn request_permissions_response(
    outcome: RequestPermissionOutcome,
    permissions: &RequestPermissionProfile,
) -> RequestPermissionsResponse {
    match outcome {
        RequestPermissionOutcome::Selected(SelectedPermissionOutcome { option_id, .. }) => {
            match option_id.0.as_ref() {
                permission_option::APPROVED_FOR_SESSION => RequestPermissionsResponse {
                    permissions: permissions.clone(),
                    scope: PermissionGrantScope::Session,
                    strict_auto_review: false,
                },
                permission_option::APPROVED => RequestPermissionsResponse {
                    permissions: permissions.clone(),
                    scope: PermissionGrantScope::Turn,
                    strict_auto_review: false,
                },
                _unknown_option_id => denied_request_permissions_response(),
            }
        }
        RequestPermissionOutcome::Cancelled => denied_request_permissions_response(),
        _unknown_outcome => denied_request_permissions_response(),
    }
}

fn denied_request_permissions_response() -> RequestPermissionsResponse {
    RequestPermissionsResponse {
        permissions: RequestPermissionProfile::default(),
        scope: PermissionGrantScope::Turn,
        strict_auto_review: true,
    }
}

fn mcp_elicitation_resolution(
    outcome: RequestPermissionOutcome,
    option_map: &HashMap<String, McpElicitationResolution>,
) -> McpElicitationResolution {
    match outcome {
        RequestPermissionOutcome::Selected(SelectedPermissionOutcome { option_id, .. }) => {
            option_map
                .get(option_id.0.as_ref())
                .cloned()
                .unwrap_or_else(McpElicitationResolution::cancel)
        }
        RequestPermissionOutcome::Cancelled => McpElicitationResolution::cancel(),
        _unknown_outcome => McpElicitationResolution::cancel(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use agent_client_protocol::schema::v1::RequestPermissionOutcome;
    use codex_protocol::request_permissions::{PermissionGrantScope, RequestPermissionProfile};

    fn selected(option_id: impl Into<String>) -> RequestPermissionOutcome {
        RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option_id.into()))
    }

    #[test]
    fn exec_approval_op_uses_selected_review_decision() {
        let op = exec_approval_op(
            "approval-id".to_string(),
            "turn-id".to_string(),
            selected(permission_option::DENIED),
            &HashMap::from([(
                permission_option::DENIED.to_string(),
                ReviewDecision::Denied,
            )]),
        );

        assert!(matches!(
            op,
            Op::ExecApproval {
                id,
                turn_id,
                decision: ReviewDecision::Denied,
            } if id == "approval-id" && turn_id.as_deref() == Some("turn-id")
        ));
    }

    #[test]
    fn unknown_review_option_aborts() {
        let op = patch_approval_op("patch-id".to_string(), selected("unknown"), &HashMap::new());

        assert!(matches!(
            op,
            Op::PatchApproval {
                id,
                decision: ReviewDecision::Abort,
            } if id == "patch-id"
        ));
    }

    #[test]
    fn request_permissions_session_option_grants_session_scope() {
        let op = request_permissions_op(
            "permissions-id".to_string(),
            selected(permission_option::APPROVED_FOR_SESSION),
            &RequestPermissionProfile::default(),
        );

        assert!(matches!(
            op,
            Op::RequestPermissionsResponse {
                id,
                response: RequestPermissionsResponse {
                    scope: PermissionGrantScope::Session,
                    strict_auto_review: false,
                    ..
                },
            } if id == "permissions-id"
        ));
    }

    #[test]
    fn cancelled_request_permissions_denies_strict_auto_review() {
        let op = request_permissions_op(
            "permissions-id".to_string(),
            RequestPermissionOutcome::Cancelled,
            &RequestPermissionProfile::default(),
        );

        assert!(matches!(
            op,
            Op::RequestPermissionsResponse {
                response: RequestPermissionsResponse {
                    scope: PermissionGrantScope::Turn,
                    strict_auto_review: true,
                    ..
                },
                ..
            }
        ));
    }

    #[test]
    fn mcp_elicitation_selection_preserves_persist_meta() {
        let op = resolve_mcp_elicitation_op(
            "server".to_string(),
            RequestId::String("request-id".to_string()),
            selected(mcp_approval::ALLOW_ALWAYS_OPTION_ID),
            &HashMap::from([(
                mcp_approval::ALLOW_ALWAYS_OPTION_ID.to_string(),
                McpElicitationResolution::accept_with_persist(mcp_approval::PERSIST_ALWAYS),
            )]),
        );

        match op {
            Op::ResolveElicitation {
                decision,
                content,
                meta,
                ..
            } => {
                assert_eq!(decision, ElicitationAction::Accept);
                assert!(content.is_none());
                assert_eq!(
                    meta.as_ref()
                        .and_then(|value| value.get(mcp_approval::PERSIST_KEY))
                        .and_then(serde_json::Value::as_str),
                    Some(mcp_approval::PERSIST_ALWAYS)
                );
            }
            other => panic!("unexpected op: {other:?}"),
        }
    }
}
