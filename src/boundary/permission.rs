use std::{collections::HashMap, path::PathBuf};

use agent_client_protocol::Error;
use agent_client_protocol::schema::{
    Meta, PermissionOption, PermissionOptionKind, ToolCallContent, ToolCallId, ToolCallLocation,
    ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use codex_protocol::{
    models::AdditionalPermissionProfile,
    permissions::FileSystemAccessMode,
    protocol::{
        ApplyPatchApprovalRequestEvent, ExecApprovalRequestEvent, NetworkApprovalContext,
        ReviewDecision,
    },
    request_permissions::{RequestPermissionProfile, RequestPermissionsEvent},
};
use itertools::Itertools;

use crate::{
    boundary::{
        compat,
        constants::permission_option,
        effect::PermissionRequestSeed,
        file_changes::{FileChangeRenderContext, extract_tool_call_content_from_changes},
        raw,
        tool_call::{ParseCommandToolCall, parse_command_tool_call},
    },
    guardian::format_file_system_entries,
};

const MAX_DEFAULT_OPEN_EDIT_FILES: usize = 3;

#[derive(Clone)]
pub(crate) struct ExecPermissionOption {
    pub(crate) option_id: &'static str,
    pub(crate) permission_option: PermissionOption,
    pub(crate) decision: ReviewDecision,
}

pub(crate) fn build_exec_permission_options(
    available_decisions: &[ReviewDecision],
    network_approval_context: Option<&NetworkApprovalContext>,
    additional_permissions: Option<&AdditionalPermissionProfile>,
) -> Vec<ExecPermissionOption> {
    available_decisions
        .iter()
        .map(|decision| match decision {
            ReviewDecision::Approved => ExecPermissionOption {
                option_id: permission_option::APPROVED,
                permission_option: PermissionOption::new(
                    permission_option::APPROVED,
                    if network_approval_context.is_some() {
                        "Yes, just this once"
                    } else {
                        "Yes, proceed"
                    },
                    PermissionOptionKind::AllowOnce,
                ),
                decision: ReviewDecision::Approved,
            },
            ReviewDecision::ApprovedExecpolicyAmendment {
                proposed_execpolicy_amendment,
            } => {
                let command_prefix = proposed_execpolicy_amendment.command().join(" ");
                let label = if command_prefix.contains('\n')
                    || command_prefix.contains('\r')
                    || command_prefix.is_empty()
                {
                    "Yes, and remember this command pattern".to_string()
                } else {
                    format!(
                        "Yes, and don't ask again for commands that start with `{command_prefix}`"
                    )
                };
                ExecPermissionOption {
                    option_id: permission_option::APPROVED_EXECPOLICY_AMENDMENT,
                    permission_option: PermissionOption::new(
                        permission_option::APPROVED_EXECPOLICY_AMENDMENT,
                        label,
                        PermissionOptionKind::AllowAlways,
                    ),
                    decision: ReviewDecision::ApprovedExecpolicyAmendment {
                        proposed_execpolicy_amendment: proposed_execpolicy_amendment.clone(),
                    },
                }
            }
            ReviewDecision::ApprovedForSession => ExecPermissionOption {
                option_id: permission_option::APPROVED_FOR_SESSION,
                permission_option: PermissionOption::new(
                    permission_option::APPROVED_FOR_SESSION,
                    if network_approval_context.is_some() {
                        "Yes, and allow this host for this session"
                    } else if additional_permissions.is_some() {
                        "Yes, and allow these permissions for this session"
                    } else {
                        "Yes, and don't ask again for this command in this session"
                    },
                    PermissionOptionKind::AllowAlways,
                ),
                decision: ReviewDecision::ApprovedForSession,
            },
            ReviewDecision::NetworkPolicyAmendment {
                network_policy_amendment,
            } => {
                let (option_id, label, kind) = match network_policy_amendment.action {
                    codex_protocol::protocol::NetworkPolicyRuleAction::Allow => (
                        permission_option::NETWORK_POLICY_AMENDMENT_ALLOW,
                        "Yes, and allow this host in the future",
                        PermissionOptionKind::AllowAlways,
                    ),
                    codex_protocol::protocol::NetworkPolicyRuleAction::Deny => (
                        permission_option::NETWORK_POLICY_AMENDMENT_DENY,
                        "No, and block this host in the future",
                        PermissionOptionKind::RejectAlways,
                    ),
                };
                ExecPermissionOption {
                    option_id,
                    permission_option: PermissionOption::new(option_id, label, kind),
                    decision: ReviewDecision::NetworkPolicyAmendment {
                        network_policy_amendment: network_policy_amendment.clone(),
                    },
                }
            }
            ReviewDecision::Denied => ExecPermissionOption {
                option_id: permission_option::DENIED,
                permission_option: PermissionOption::new(
                    permission_option::DENIED,
                    "No, continue without running it",
                    PermissionOptionKind::RejectOnce,
                ),
                decision: ReviewDecision::Denied,
            },
            ReviewDecision::Abort => ExecPermissionOption {
                option_id: permission_option::ABORT,
                permission_option: PermissionOption::new(
                    permission_option::ABORT,
                    "No, and tell Codex what to do differently",
                    PermissionOptionKind::RejectOnce,
                ),
                decision: ReviewDecision::Abort,
            },
            ReviewDecision::TimedOut => ExecPermissionOption {
                option_id: permission_option::TIMED_OUT,
                permission_option: PermissionOption::new(
                    permission_option::TIMED_OUT,
                    "Time out, tell Codex what to do differently",
                    PermissionOptionKind::RejectOnce,
                ),
                decision: ReviewDecision::TimedOut,
            },
        })
        .collect()
}

pub(crate) struct ExecApprovalInteraction {
    pub(crate) request_key: String,
    pub(crate) approval_id: String,
    pub(crate) turn_id: String,
    pub(crate) option_map: HashMap<String, ReviewDecision>,
    pub(crate) active_command: ActiveCommandSeed,
    pub(crate) permission_request: PermissionRequestSeed,
}

pub(crate) struct ActiveCommandSeed {
    pub(crate) call_id: String,
    pub(crate) tool_call_id: ToolCallId,
    pub(crate) title: String,
    pub(crate) kind: ToolKind,
    pub(crate) terminal_output: bool,
    pub(crate) file_extension: Option<String>,
    pub(crate) cwd: PathBuf,
}

pub(crate) struct PatchApprovalInteraction {
    pub(crate) request_key: String,
    pub(crate) call_id: String,
    pub(crate) option_map: HashMap<String, ReviewDecision>,
    pub(crate) permission_request: PermissionRequestSeed,
}

pub(crate) struct RequestPermissionsInteraction {
    pub(crate) request_key: String,
    pub(crate) call_id: String,
    pub(crate) permissions: RequestPermissionProfile,
    pub(crate) permission_request: PermissionRequestSeed,
}

pub(crate) fn exec_approval_interaction(
    event: ExecApprovalRequestEvent,
) -> Result<ExecApprovalInteraction, Error> {
    let available_decisions = event.effective_available_decisions();
    let raw_input = raw::exec_approval_request(&event);
    let content = exec_approval_content(&event, &available_decisions)?;
    let ExecApprovalRequestEvent {
        call_id,
        command: _,
        turn_id,
        cwd,
        reason: _,
        parsed_cmd,
        proposed_execpolicy_amendment: _,
        approval_id,
        network_approval_context,
        additional_permissions,
        available_decisions: _,
        proposed_network_policy_amendments: _,
    } = event;

    let tool_call_id = ToolCallId::new(call_id.clone());
    let ParseCommandToolCall {
        title,
        terminal_output,
        file_extension,
        locations,
        kind,
    } = parse_command_tool_call(parsed_cmd, &cwd);

    let permission_options = build_exec_permission_options(
        &available_decisions,
        network_approval_context.as_ref(),
        additional_permissions.as_ref(),
    );
    let option_map = permission_options
        .iter()
        .map(|option| (option.option_id.to_string(), option.decision.clone()))
        .collect();

    Ok(ExecApprovalInteraction {
        request_key: exec_approval_request_key(&call_id),
        approval_id: approval_id.unwrap_or_else(|| call_id.clone()),
        turn_id,
        option_map,
        active_command: ActiveCommandSeed {
            call_id: call_id.clone(),
            tool_call_id: tool_call_id.clone(),
            title: title.clone(),
            kind,
            terminal_output,
            file_extension,
            cwd: cwd.to_path_buf(),
        },
        permission_request: PermissionRequestSeed::new(
            ToolCallUpdate::new(
                tool_call_id,
                ToolCallUpdateFields::new()
                    .kind(kind)
                    .status(ToolCallStatus::Pending)
                    .title(title)
                    .raw_input(raw_input)
                    .content(content)
                    .locations(non_empty_locations(locations)),
            ),
            permission_options
                .into_iter()
                .map(|option| option.permission_option)
                .collect(),
        ),
    })
}

pub(crate) fn patch_approval_interaction(
    event: ApplyPatchApprovalRequestEvent,
) -> PatchApprovalInteraction {
    let raw_input = raw::patch_approval_request(&event);
    let ApplyPatchApprovalRequestEvent {
        call_id,
        changes,
        reason,
        grant_root: _,
        turn_id: _,
    } = event;
    let (title, locations, content) =
        extract_tool_call_content_from_changes(changes, FileChangeRenderContext::BeforeApply);
    let mut content = content.collect::<Vec<ToolCallContent>>();
    let meta = patch_edit_output_display_meta(locations.len());
    if let Some(reason) = reason {
        content.push(reason.into());
    }

    PatchApprovalInteraction {
        request_key: patch_approval_request_key(&call_id),
        call_id: call_id.clone(),
        option_map: HashMap::from([
            (
                permission_option::APPROVED.to_string(),
                ReviewDecision::Approved,
            ),
            (
                permission_option::DENIED.to_string(),
                ReviewDecision::Denied,
            ),
        ]),
        permission_request: PermissionRequestSeed::new(
            ToolCallUpdate::new(
                call_id,
                ToolCallUpdateFields::new()
                    .kind(ToolKind::Edit)
                    .status(ToolCallStatus::Pending)
                    .title(title)
                    .locations(locations)
                    .content(content)
                    .raw_input(raw_input),
            )
            .meta(meta),
            vec![
                PermissionOption::new(
                    permission_option::APPROVED,
                    "Yes",
                    PermissionOptionKind::AllowOnce,
                ),
                PermissionOption::new(
                    permission_option::DENIED,
                    "No, continue without these edits",
                    PermissionOptionKind::RejectOnce,
                ),
            ],
        ),
    }
}

pub(crate) fn request_permissions_interaction(
    event: RequestPermissionsEvent,
) -> RequestPermissionsInteraction {
    let raw_input = raw::request_permissions(&event);
    let RequestPermissionsEvent {
        call_id,
        turn_id: _,
        reason,
        permissions,
        cwd: _,
    } = event;

    let tool_call_id = ToolCallId::new(call_id.clone());
    let content = request_permissions_content(reason.as_ref(), &permissions);
    let title = reason.unwrap_or_else(|| "Permissions Request".to_string());

    RequestPermissionsInteraction {
        request_key: request_permissions_request_key(&call_id),
        call_id,
        permissions,
        permission_request: PermissionRequestSeed::new(
            ToolCallUpdate::new(
                tool_call_id,
                ToolCallUpdateFields::new()
                    .status(ToolCallStatus::Pending)
                    .title(title)
                    .raw_input(raw_input)
                    .content(content),
            ),
            vec![
                PermissionOption::new(
                    permission_option::APPROVED_FOR_SESSION,
                    "Yes, for session",
                    PermissionOptionKind::AllowAlways,
                ),
                PermissionOption::new(
                    permission_option::APPROVED,
                    "Yes",
                    PermissionOptionKind::AllowOnce,
                ),
                PermissionOption::new(
                    permission_option::ABORT,
                    "No",
                    PermissionOptionKind::RejectOnce,
                ),
            ],
        ),
    }
}

fn exec_approval_request_key(call_id: &str) -> String {
    format!("exec:{call_id}")
}

fn patch_approval_request_key(call_id: &str) -> String {
    format!("patch:{call_id}")
}

fn request_permissions_request_key(call_id: &str) -> String {
    format!("permissions:{call_id}")
}

pub(crate) fn patch_edit_output_display_meta(file_count: usize) -> Meta {
    let default_open = file_count <= MAX_DEFAULT_OPEN_EDIT_FILES;
    compat::tool_call_output_display_meta(
        default_open,
        if default_open {
            "smallFileEdit"
        } else {
            "manyFileEdits"
        },
    )
}

fn exec_approval_content(
    event: &ExecApprovalRequestEvent,
    available_decisions: &[ReviewDecision],
) -> Result<Option<Vec<ToolCallContent>>, Error> {
    let mut content = vec![];

    if let Some(reason) = event.reason.as_ref() {
        content.push(reason.clone());
    }
    if let Some(amendment) = event.proposed_execpolicy_amendment.as_ref() {
        content.push(format!(
            "Proposed Amendment: {}",
            amendment.command().join("\n")
        ));
    }
    if let Some(policy) = event.network_approval_context.as_ref() {
        let NetworkApprovalContext { host, protocol } = policy;
        content.push(format!("Network Approval Context: {:?} {}", protocol, host));
    }
    if let Some(permissions) = event.additional_permissions.as_ref() {
        content.push(format!(
            "Additional Permissions: {}",
            serde_json::to_string_pretty(&permissions)?
        ));
    }
    content.push(format!(
        "Available Decisions: {}",
        available_decisions
            .iter()
            .map(ToString::to_string)
            .join("\n")
    ));
    if let Some(amendments) = event.proposed_network_policy_amendments.as_ref() {
        content.push(format!(
            "Proposed Network Policy Amendments: {}",
            amendments
                .iter()
                .map(|amendment| format!("{:?} {:?}", amendment.action, amendment.host))
                .join("\n")
        ));
    }

    Ok((!content.is_empty()).then(|| vec![content.join("\n").into()]))
}

fn non_empty_locations(locations: Vec<ToolCallLocation>) -> Option<Vec<ToolCallLocation>> {
    (!locations.is_empty()).then_some(locations)
}

fn request_permissions_content(
    reason: Option<&String>,
    permissions: &RequestPermissionProfile,
) -> Option<Vec<ToolCallContent>> {
    let mut content = vec![];

    if let Some(reason) = reason {
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

    if content.is_empty() {
        None
    } else {
        Some(vec![content.join("\n").into()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use codex_protocol::{
        parse_command::ParsedCommand,
        protocol::{ExecApprovalRequestEvent, ReviewDecision},
    };

    #[test]
    fn exec_approval_interaction_builds_active_command_and_request() -> anyhow::Result<()> {
        let interaction = exec_approval_interaction(ExecApprovalRequestEvent {
            call_id: "exec-call".to_string(),
            approval_id: Some("approval-id".to_string()),
            turn_id: "turn-id".to_string(),
            command: vec!["echo".to_string(), "hi".to_string()],
            cwd: std::env::current_dir()?.try_into()?,
            reason: Some("Need to run command".to_string()),
            network_approval_context: None,
            proposed_execpolicy_amendment: None,
            proposed_network_policy_amendments: None,
            additional_permissions: None,
            available_decisions: Some(vec![ReviewDecision::Approved, ReviewDecision::Denied]),
            parsed_cmd: vec![ParsedCommand::Unknown {
                cmd: "echo hi".to_string(),
            }],
        })?;

        assert_eq!(interaction.request_key, "exec:exec-call");
        assert_eq!(interaction.approval_id, "approval-id");
        assert_eq!(interaction.turn_id, "turn-id");
        assert_eq!(interaction.active_command.call_id, "exec-call");
        assert_eq!(interaction.active_command.title, "echo hi");
        assert_eq!(
            interaction.permission_request.tool_call_id().0.as_ref(),
            "exec-call"
        );
        assert_eq!(
            interaction.permission_request.option_ids(),
            vec![
                permission_option::APPROVED.to_string(),
                permission_option::DENIED.to_string(),
            ]
        );

        Ok(())
    }

    #[test]
    fn patch_approval_interaction_uses_stable_options() {
        let interaction = patch_approval_interaction(ApplyPatchApprovalRequestEvent {
            call_id: "patch-call".to_string(),
            turn_id: "turn-id".to_string(),
            changes: HashMap::new(),
            reason: Some("Need to edit files".to_string()),
            grant_root: None,
        });

        assert_eq!(interaction.request_key, "patch:patch-call");
        assert_eq!(interaction.call_id, "patch-call");
        assert_eq!(
            interaction.permission_request.tool_call_id().0.as_ref(),
            "patch-call"
        );
        assert_eq!(
            interaction.permission_request.option_ids(),
            vec![
                permission_option::APPROVED.to_string(),
                permission_option::DENIED.to_string(),
            ]
        );
        assert_eq!(
            interaction
                .option_map
                .get(permission_option::APPROVED)
                .cloned(),
            Some(ReviewDecision::Approved)
        );
    }

    #[test]
    fn patch_edit_output_display_meta_opens_small_file_edits() {
        let meta = patch_edit_output_display_meta(2);

        assert_eq!(patch_default_open(&meta), Some(true));
    }

    #[test]
    fn patch_edit_output_display_meta_folds_many_file_edits() {
        let meta = patch_edit_output_display_meta(MAX_DEFAULT_OPEN_EDIT_FILES + 1);

        assert_eq!(patch_default_open(&meta), Some(false));
    }

    fn patch_default_open(metadata: &Meta) -> Option<bool> {
        use crate::boundary::constants::meta;

        metadata
            .get(meta::CODEX_ACP)
            .and_then(|value| value.get(meta::TOOL_CALL_OUTPUT))
            .and_then(|value| value.get(meta::TOOL_CALL_OUTPUT_DEFAULT_OPEN))
            .and_then(serde_json::Value::as_bool)
    }

    #[test]
    fn request_permissions_interaction_uses_stable_options() {
        let interaction = request_permissions_interaction(RequestPermissionsEvent {
            call_id: "permissions-call".to_string(),
            turn_id: "turn-id".to_string(),
            reason: Some("Need broader access".to_string()),
            permissions: RequestPermissionProfile::default(),
            cwd: None,
        });

        assert_eq!(interaction.request_key, "permissions:permissions-call");
        assert_eq!(interaction.call_id, "permissions-call");
        assert_eq!(
            interaction.permission_request.tool_call_id().0.as_ref(),
            "permissions-call"
        );
        assert_eq!(
            interaction.permission_request.option_ids(),
            vec![
                permission_option::APPROVED_FOR_SESSION.to_string(),
                permission_option::APPROVED.to_string(),
                permission_option::ABORT.to_string(),
            ]
        );
    }
}
