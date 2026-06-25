//! Builder helpers for Codex protocol events used in tests.
//!
//! These factories absorb upstream field additions (e.g. new timestamp /
//! audit fields) into one place so test code does not have to be updated
//! at every event-construction site on each codex bump.

use std::{collections::HashMap, path::PathBuf, time::Duration};

use codex_protocol::{
    ThreadId,
    openai_models::ReasoningEffort,
    parse_command::ParsedCommand,
    protocol::{
        AgentStatus, ApplyPatchApprovalRequestEvent, CollabAgentSpawnBeginEvent,
        CollabAgentSpawnEndEvent, ExecApprovalRequestEvent, ExecCommandBeginEvent,
        ExecCommandEndEvent, ExecCommandSource, ExecCommandStatus, FileChange, ReviewDecision,
    },
    request_permissions::{RequestPermissionProfile, RequestPermissionsEvent},
};
use codex_utils_absolute_path::AbsolutePathBuf;

pub(crate) fn exec_command_begin(
    call_id: impl Into<String>,
    turn_id: impl Into<String>,
    cwd: AbsolutePathBuf,
    command: Vec<String>,
    parsed_cmd: Vec<ParsedCommand>,
) -> ExecCommandBeginEvent {
    ExecCommandBeginEvent {
        call_id: call_id.into(),
        process_id: None,
        turn_id: turn_id.into(),
        started_at_ms: 0,
        command,
        cwd: cwd.into(),
        parsed_cmd,
        source: ExecCommandSource::default(),
        interaction_input: None,
    }
}

pub(crate) fn exec_command_end(
    call_id: impl Into<String>,
    turn_id: impl Into<String>,
    cwd: AbsolutePathBuf,
    command: Vec<String>,
    stdout: impl Into<String>,
) -> ExecCommandEndEvent {
    let stdout = stdout.into();
    ExecCommandEndEvent {
        call_id: call_id.into(),
        process_id: None,
        turn_id: turn_id.into(),
        completed_at_ms: 0,
        command,
        cwd: cwd.into(),
        parsed_cmd: vec![],
        source: ExecCommandSource::default(),
        interaction_input: None,
        stdout: stdout.clone(),
        stderr: String::new(),
        aggregated_output: stdout.clone(),
        exit_code: 0,
        duration: Duration::from_millis(1),
        formatted_output: stdout,
        status: ExecCommandStatus::Completed,
    }
}

pub(crate) fn exec_approval_request(
    call_id: impl Into<String>,
    turn_id: impl Into<String>,
    cwd: AbsolutePathBuf,
    command: Vec<String>,
    parsed_cmd: Vec<ParsedCommand>,
    available_decisions: Option<Vec<ReviewDecision>>,
) -> ExecApprovalRequestEvent {
    ExecApprovalRequestEvent {
        call_id: call_id.into(),
        approval_id: Some("approval-id".to_string()),
        turn_id: turn_id.into(),
        environment_id: None,
        started_at_ms: 0,
        command,
        cwd,
        reason: None,
        network_approval_context: None,
        proposed_execpolicy_amendment: None,
        proposed_network_policy_amendments: None,
        additional_permissions: None,
        available_decisions,
        parsed_cmd,
    }
}

pub(crate) fn apply_patch_approval_request(
    call_id: impl Into<String>,
    turn_id: impl Into<String>,
    changes: HashMap<PathBuf, FileChange>,
    reason: Option<String>,
) -> ApplyPatchApprovalRequestEvent {
    ApplyPatchApprovalRequestEvent {
        call_id: call_id.into(),
        turn_id: turn_id.into(),
        started_at_ms: 0,
        changes,
        reason,
        grant_root: None,
    }
}

pub(crate) fn request_permissions(
    call_id: impl Into<String>,
    turn_id: impl Into<String>,
    reason: Option<String>,
    permissions: RequestPermissionProfile,
) -> RequestPermissionsEvent {
    RequestPermissionsEvent {
        call_id: call_id.into(),
        turn_id: turn_id.into(),
        started_at_ms: 0,
        reason,
        permissions,
        cwd: None,
        environment_id: None,
    }
}

pub(crate) fn collab_spawn_begin(
    call_id: impl Into<String>,
    sender_thread_id: ThreadId,
    prompt: impl Into<String>,
    model: impl Into<String>,
    reasoning_effort: ReasoningEffort,
) -> CollabAgentSpawnBeginEvent {
    CollabAgentSpawnBeginEvent {
        call_id: call_id.into(),
        started_at_ms: 0,
        sender_thread_id,
        prompt: prompt.into(),
        model: model.into(),
        reasoning_effort,
    }
}

pub(crate) fn collab_spawn_end(
    call_id: impl Into<String>,
    sender_thread_id: ThreadId,
    new_thread_id: Option<ThreadId>,
    prompt: impl Into<String>,
    model: impl Into<String>,
    reasoning_effort: ReasoningEffort,
) -> CollabAgentSpawnEndEvent {
    CollabAgentSpawnEndEvent {
        call_id: call_id.into(),
        completed_at_ms: 0,
        sender_thread_id,
        new_thread_id,
        new_agent_nickname: None,
        new_agent_role: None,
        prompt: prompt.into(),
        model: model.into(),
        reasoning_effort,
        status: AgentStatus::Running,
    }
}
