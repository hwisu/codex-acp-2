use agent_client_protocol::schema::{ToolCallContent, ToolCallStatus};
use codex_protocol::{
    ThreadId,
    openai_models::ReasoningEffort,
    protocol::{
        AgentStatus, CollabAgentInteractionEndEvent, CollabAgentSpawnEndEvent, CollabCloseEndEvent,
        CollabResumeBeginEvent, CollabResumeEndEvent, CollabWaitingEndEvent,
    },
};

use crate::display::{
    format_agent_label, format_agent_status, format_collab_prompt, format_collab_waiting_statuses,
    tool_call_text_content,
};

pub(super) fn spawn_begin_content(
    model: &str,
    reasoning_effort: ReasoningEffort,
    prompt: &str,
) -> Vec<ToolCallContent> {
    let mut content = vec![tool_call_text_content(format!(
        "Model: {model} ({reasoning_effort})"
    ))];
    content.extend(prompt_content(prompt));
    content
}

pub(super) fn spawn_end_title(event: &CollabAgentSpawnEndEvent) -> String {
    event
        .new_thread_id
        .as_ref()
        .map(|thread_id| {
            format!(
                "Spawned {}",
                agent_label(
                    event.new_agent_nickname.as_deref(),
                    event.new_agent_role.as_deref(),
                    thread_id,
                )
            )
        })
        .unwrap_or_else(|| "Spawn subagent".to_string())
}

pub(super) fn spawn_end_status(event: &CollabAgentSpawnEndEvent) -> ToolCallStatus {
    if event.new_thread_id.is_some() {
        ToolCallStatus::Completed
    } else {
        ToolCallStatus::Failed
    }
}

pub(super) fn spawn_end_content(event: &CollabAgentSpawnEndEvent) -> Vec<ToolCallContent> {
    status_content(&event.status, Some(event.prompt.as_str()))
}

pub(super) fn interaction_end_title(event: &CollabAgentInteractionEndEvent) -> String {
    format!(
        "Sent input to {}",
        agent_label(
            event.receiver_agent_nickname.as_deref(),
            event.receiver_agent_role.as_deref(),
            &event.receiver_thread_id,
        )
    )
}

pub(super) fn interaction_end_content(
    event: &CollabAgentInteractionEndEvent,
) -> Vec<ToolCallContent> {
    status_content(&event.status, Some(event.prompt.as_str()))
}

pub(super) fn waiting_end_content(event: &CollabWaitingEndEvent) -> Vec<ToolCallContent> {
    format_collab_waiting_statuses(&event.agent_statuses)
        .map(tool_call_text_content)
        .into_iter()
        .collect()
}

pub(super) fn close_end_title(event: &CollabCloseEndEvent) -> String {
    format!(
        "Closed {}",
        agent_label(
            event.receiver_agent_nickname.as_deref(),
            event.receiver_agent_role.as_deref(),
            &event.receiver_thread_id,
        )
    )
}

pub(super) fn close_end_content(event: &CollabCloseEndEvent) -> Vec<ToolCallContent> {
    status_content(&event.status, None)
}

pub(super) fn resume_begin_title(event: &CollabResumeBeginEvent) -> String {
    format!(
        "Resume {}",
        agent_label(
            event.receiver_agent_nickname.as_deref(),
            event.receiver_agent_role.as_deref(),
            &event.receiver_thread_id,
        )
    )
}

pub(super) fn resume_end_title(event: &CollabResumeEndEvent) -> String {
    format!(
        "Resumed {}",
        agent_label(
            event.receiver_agent_nickname.as_deref(),
            event.receiver_agent_role.as_deref(),
            &event.receiver_thread_id,
        )
    )
}

pub(super) fn resume_end_content(event: &CollabResumeEndEvent) -> Vec<ToolCallContent> {
    status_content(&event.status, None)
}

fn status_content(status: &AgentStatus, prompt: Option<&str>) -> Vec<ToolCallContent> {
    let mut content = vec![tool_call_text_content(format!(
        "Status: {}",
        format_agent_status(status)
    ))];
    if let Some(prompt) = prompt {
        content.extend(prompt_content(prompt));
    }
    content
}

pub(super) fn prompt_content(prompt: &str) -> Vec<ToolCallContent> {
    format_collab_prompt(prompt)
        .map(tool_call_text_content)
        .into_iter()
        .collect()
}

fn agent_label(nickname: Option<&str>, role: Option<&str>, thread_id: &ThreadId) -> String {
    format_agent_label(nickname, role, Some(thread_id))
}
