use agent_client_protocol::schema::{
    PermissionOption, PermissionOptionKind, ToolCallLocation, ToolKind,
};
use codex_protocol::models::AdditionalPermissionProfile;
use codex_protocol::parse_command::ParsedCommand;
use codex_protocol::protocol::{NetworkApprovalContext, ReviewDecision};
use std::path::{Path, PathBuf};

use crate::boundary::constants::permission_option;

#[derive(Clone)]
pub(crate) struct ExecPermissionOption {
    pub option_id: &'static str,
    pub permission_option: PermissionOption,
    pub decision: ReviewDecision,
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

pub(crate) struct ParseCommandToolCall {
    pub title: String,
    pub file_extension: Option<String>,
    pub terminal_output: bool,
    pub locations: Vec<ToolCallLocation>,
    pub kind: ToolKind,
}

pub(crate) fn parse_command_tool_call(
    parsed_cmd: Vec<ParsedCommand>,
    cwd: &Path,
) -> ParseCommandToolCall {
    let mut titles = Vec::new();
    let mut locations = Vec::new();
    let mut file_extension = None;
    let mut terminal_output = false;
    let mut kind = ToolKind::Execute;

    for cmd in parsed_cmd {
        let mut cmd_path = None;
        match cmd {
            ParsedCommand::Read { cmd: _, name, path } => {
                titles.push(format!("Read {name}"));
                file_extension = path
                    .extension()
                    .map(|ext| ext.to_string_lossy().to_string());
                cmd_path = Some(path);
                kind = ToolKind::Read;
            }
            ParsedCommand::ListFiles { cmd: _, path } => {
                let dir = if let Some(path) = path.as_ref() {
                    &cwd.join(path)
                } else {
                    cwd
                };
                titles.push(format!("List {}", dir.display()));
                cmd_path = path.map(PathBuf::from);
                terminal_output = true;
                kind = ToolKind::Search;
            }
            ParsedCommand::Search { cmd, query, path } => {
                titles.push(match (query, path.as_ref()) {
                    (Some(query), Some(path)) => format!("Search {query} in {path}"),
                    (Some(query), None) => format!("Search {query}"),
                    _ => format!("Search {cmd}"),
                });
                terminal_output = true;
                kind = ToolKind::Search;
            }
            ParsedCommand::Unknown { cmd } => {
                titles.push(cmd);
                terminal_output = true;
            }
        }

        if let Some(path) = cmd_path {
            locations.push(ToolCallLocation::new(if path.is_relative() {
                cwd.join(&path)
            } else {
                path
            }));
        }
    }

    ParseCommandToolCall {
        title: titles.join(", "),
        file_extension,
        terminal_output,
        locations,
        kind,
    }
}
