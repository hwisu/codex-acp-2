use agent_client_protocol::schema::v1::{
    ToolCall, ToolCallContent, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use codex_protocol::{
    ThreadId,
    openai_models::ReasoningEffort,
    protocol::{
        AgentStatus, CollabAgentInteractionBeginEvent, CollabAgentInteractionEndEvent,
        CollabAgentSpawnBeginEvent, CollabAgentSpawnEndEvent, CollabCloseBeginEvent,
        CollabCloseEndEvent, CollabResumeBeginEvent, CollabResumeEndEvent, CollabWaitingBeginEvent,
        CollabWaitingEndEvent,
    },
};
use itertools::Itertools;

use crate::{
    boundary::{effect::BridgeEffect, raw},
    display::{
        format_agent_label, format_agent_status, format_collab_agent_ref, format_collab_prompt,
        format_collab_waiting_statuses, tool_call_text_content,
    },
};

pub(crate) fn collab_spawn_begin_tool_call(event: &CollabAgentSpawnBeginEvent) -> ToolCall {
    ToolCall::new(event.call_id.clone(), "Spawn subagent")
        .kind(ToolKind::Other)
        .status(ToolCallStatus::InProgress)
        .raw_input(raw::collab_spawn_begin(event))
        .content(collab_spawn_begin_content(
            &event.model,
            event.reasoning_effort.clone(),
            &event.prompt,
        ))
}

pub(crate) fn collab_spawn_begin_effect(event: &CollabAgentSpawnBeginEvent) -> BridgeEffect {
    BridgeEffect::tool_call(collab_spawn_begin_tool_call(event))
}

pub(crate) fn collab_spawn_end_tool_call_update(
    event: &CollabAgentSpawnEndEvent,
) -> ToolCallUpdate {
    ToolCallUpdate::new(
        event.call_id.clone(),
        ToolCallUpdateFields::new()
            .status(collab_spawn_end_status(event))
            .title(Some(collab_spawn_end_title(event)))
            .content(Some(collab_spawn_end_content(event)))
            .raw_output(raw::collab_spawn_end(event)),
    )
}

pub(crate) fn collab_spawn_end_effect(event: &CollabAgentSpawnEndEvent) -> BridgeEffect {
    BridgeEffect::tool_call_update(collab_spawn_end_tool_call_update(event))
}

pub(crate) fn collab_spawn_replay_tool_call(event: &CollabAgentSpawnEndEvent) -> ToolCall {
    ToolCall::new(event.call_id.clone(), collab_spawn_end_title(event))
        .kind(ToolKind::Other)
        .status(collab_spawn_end_status(event))
        .content(collab_spawn_end_content(event))
}

pub(crate) fn collab_spawn_replay_effect(event: &CollabAgentSpawnEndEvent) -> BridgeEffect {
    BridgeEffect::tool_call(collab_spawn_replay_tool_call(event))
}

pub(crate) fn collab_interaction_begin_tool_call(
    event: &CollabAgentInteractionBeginEvent,
) -> ToolCall {
    ToolCall::new(event.call_id.clone(), "Send input to subagent")
        .kind(ToolKind::Other)
        .status(ToolCallStatus::InProgress)
        .raw_input(raw::collab_interaction_begin(event))
        .content(collab_prompt_content(&event.prompt))
}

pub(crate) fn collab_interaction_begin_effect(
    event: &CollabAgentInteractionBeginEvent,
) -> BridgeEffect {
    BridgeEffect::tool_call(collab_interaction_begin_tool_call(event))
}

pub(crate) fn collab_interaction_end_tool_call_update(
    event: &CollabAgentInteractionEndEvent,
) -> ToolCallUpdate {
    ToolCallUpdate::new(
        event.call_id.clone(),
        ToolCallUpdateFields::new()
            .status(ToolCallStatus::Completed)
            .title(Some(collab_interaction_end_title(event)))
            .content(Some(collab_interaction_end_content(event)))
            .raw_output(raw::collab_interaction_end(event)),
    )
}

pub(crate) fn collab_interaction_end_effect(
    event: &CollabAgentInteractionEndEvent,
) -> BridgeEffect {
    BridgeEffect::tool_call_update(collab_interaction_end_tool_call_update(event))
}

pub(crate) fn collab_interaction_replay_tool_call(
    event: &CollabAgentInteractionEndEvent,
) -> ToolCall {
    ToolCall::new(event.call_id.clone(), collab_interaction_end_title(event))
        .kind(ToolKind::Other)
        .status(ToolCallStatus::Completed)
        .content(collab_interaction_end_content(event))
}

pub(crate) fn collab_interaction_replay_effect(
    event: &CollabAgentInteractionEndEvent,
) -> BridgeEffect {
    BridgeEffect::tool_call(collab_interaction_replay_tool_call(event))
}

pub(crate) fn collab_waiting_begin_tool_call(event: &CollabWaitingBeginEvent) -> ToolCall {
    let title = match event.receiver_agents.as_slice() {
        [receiver] => format!("Waiting for {}", format_collab_agent_ref(receiver)),
        _ => format!("Waiting for {} agent(s)", event.receiver_thread_ids.len()),
    };
    let content = (!event.receiver_agents.is_empty()).then(|| {
        vec![tool_call_text_content(
            event
                .receiver_agents
                .iter()
                .map(format_collab_agent_ref)
                .map(|label| format!("- {label}"))
                .join("\n"),
        )]
    });

    ToolCall::new(event.call_id.clone(), title)
        .kind(ToolKind::Other)
        .status(ToolCallStatus::InProgress)
        .raw_input(raw::collab_waiting_begin(event))
        .content(content.unwrap_or_default())
}

pub(crate) fn collab_waiting_begin_effect(event: &CollabWaitingBeginEvent) -> BridgeEffect {
    BridgeEffect::tool_call(collab_waiting_begin_tool_call(event))
}

pub(crate) fn collab_waiting_end_tool_call_update(event: &CollabWaitingEndEvent) -> ToolCallUpdate {
    let content = collab_waiting_end_content(event);
    ToolCallUpdate::new(
        event.call_id.clone(),
        ToolCallUpdateFields::new()
            .status(ToolCallStatus::Completed)
            .title(Some("Subagent wait complete".to_string()))
            .content((!content.is_empty()).then_some(content))
            .raw_output(raw::collab_waiting_end(event)),
    )
}

pub(crate) fn collab_waiting_end_effect(event: &CollabWaitingEndEvent) -> BridgeEffect {
    BridgeEffect::tool_call_update(collab_waiting_end_tool_call_update(event))
}

pub(crate) fn collab_waiting_replay_tool_call(event: &CollabWaitingEndEvent) -> ToolCall {
    ToolCall::new(event.call_id.clone(), "Subagent wait complete")
        .kind(ToolKind::Other)
        .status(ToolCallStatus::Completed)
        .content(collab_waiting_end_content(event))
}

pub(crate) fn collab_waiting_replay_effect(event: &CollabWaitingEndEvent) -> BridgeEffect {
    BridgeEffect::tool_call(collab_waiting_replay_tool_call(event))
}

pub(crate) fn collab_close_begin_tool_call(event: &CollabCloseBeginEvent) -> ToolCall {
    ToolCall::new(event.call_id.clone(), "Close subagent")
        .kind(ToolKind::Other)
        .status(ToolCallStatus::InProgress)
        .raw_input(raw::collab_close_begin(event))
}

pub(crate) fn collab_close_begin_effect(event: &CollabCloseBeginEvent) -> BridgeEffect {
    BridgeEffect::tool_call(collab_close_begin_tool_call(event))
}

pub(crate) fn collab_close_end_tool_call_update(event: &CollabCloseEndEvent) -> ToolCallUpdate {
    ToolCallUpdate::new(
        event.call_id.clone(),
        ToolCallUpdateFields::new()
            .status(ToolCallStatus::Completed)
            .title(Some(collab_close_end_title(event)))
            .content(Some(collab_close_end_content(event)))
            .raw_output(raw::collab_close_end(event)),
    )
}

pub(crate) fn collab_close_end_effect(event: &CollabCloseEndEvent) -> BridgeEffect {
    BridgeEffect::tool_call_update(collab_close_end_tool_call_update(event))
}

pub(crate) fn collab_close_replay_tool_call(event: &CollabCloseEndEvent) -> ToolCall {
    ToolCall::new(event.call_id.clone(), collab_close_end_title(event))
        .kind(ToolKind::Other)
        .status(ToolCallStatus::Completed)
        .content(collab_close_end_content(event))
}

pub(crate) fn collab_close_replay_effect(event: &CollabCloseEndEvent) -> BridgeEffect {
    BridgeEffect::tool_call(collab_close_replay_tool_call(event))
}

pub(crate) fn collab_resume_begin_tool_call(event: &CollabResumeBeginEvent) -> ToolCall {
    ToolCall::new(event.call_id.clone(), collab_resume_begin_title(event))
        .kind(ToolKind::Other)
        .status(ToolCallStatus::InProgress)
        .raw_input(raw::collab_resume_begin(event))
}

pub(crate) fn collab_resume_begin_effect(event: &CollabResumeBeginEvent) -> BridgeEffect {
    BridgeEffect::tool_call(collab_resume_begin_tool_call(event))
}

pub(crate) fn collab_resume_end_tool_call_update(event: &CollabResumeEndEvent) -> ToolCallUpdate {
    ToolCallUpdate::new(
        event.call_id.clone(),
        ToolCallUpdateFields::new()
            .status(ToolCallStatus::Completed)
            .title(Some(collab_resume_end_title(event)))
            .content(Some(collab_resume_end_content(event)))
            .raw_output(raw::collab_resume_end(event)),
    )
}

pub(crate) fn collab_resume_end_effect(event: &CollabResumeEndEvent) -> BridgeEffect {
    BridgeEffect::tool_call_update(collab_resume_end_tool_call_update(event))
}

pub(crate) fn collab_resume_replay_tool_call(event: &CollabResumeEndEvent) -> ToolCall {
    ToolCall::new(event.call_id.clone(), collab_resume_end_title(event))
        .kind(ToolKind::Other)
        .status(ToolCallStatus::Completed)
        .content(collab_resume_end_content(event))
}

pub(crate) fn collab_resume_replay_effect(event: &CollabResumeEndEvent) -> BridgeEffect {
    BridgeEffect::tool_call(collab_resume_replay_tool_call(event))
}

fn collab_spawn_begin_content(
    model: &str,
    reasoning_effort: ReasoningEffort,
    prompt: &str,
) -> Vec<ToolCallContent> {
    let mut content = vec![tool_call_text_content(format!(
        "Model: {model} ({reasoning_effort})"
    ))];
    content.extend(collab_prompt_content(prompt));
    content
}

fn collab_spawn_end_title(event: &CollabAgentSpawnEndEvent) -> String {
    event
        .new_thread_id
        .as_ref()
        .map(|thread_id| {
            format!(
                "Spawned {}",
                collab_agent_label(
                    event.new_agent_nickname.as_deref(),
                    event.new_agent_role.as_deref(),
                    thread_id,
                )
            )
        })
        .unwrap_or_else(|| "Spawn subagent".to_string())
}

fn collab_spawn_end_status(event: &CollabAgentSpawnEndEvent) -> ToolCallStatus {
    if event.new_thread_id.is_some() {
        ToolCallStatus::Completed
    } else {
        ToolCallStatus::Failed
    }
}

fn collab_spawn_end_content(event: &CollabAgentSpawnEndEvent) -> Vec<ToolCallContent> {
    collab_status_content(&event.status, Some(event.prompt.as_str()))
}

fn collab_interaction_end_title(event: &CollabAgentInteractionEndEvent) -> String {
    format!(
        "Sent input to {}",
        collab_agent_label(
            event.receiver_agent_nickname.as_deref(),
            event.receiver_agent_role.as_deref(),
            &event.receiver_thread_id,
        )
    )
}

fn collab_interaction_end_content(event: &CollabAgentInteractionEndEvent) -> Vec<ToolCallContent> {
    collab_status_content(&event.status, Some(event.prompt.as_str()))
}

fn collab_waiting_end_content(event: &CollabWaitingEndEvent) -> Vec<ToolCallContent> {
    format_collab_waiting_statuses(&event.agent_statuses)
        .map(tool_call_text_content)
        .into_iter()
        .collect()
}

fn collab_close_end_title(event: &CollabCloseEndEvent) -> String {
    format!(
        "Closed {}",
        collab_agent_label(
            event.receiver_agent_nickname.as_deref(),
            event.receiver_agent_role.as_deref(),
            &event.receiver_thread_id,
        )
    )
}

fn collab_close_end_content(event: &CollabCloseEndEvent) -> Vec<ToolCallContent> {
    collab_status_content(&event.status, None)
}

fn collab_resume_begin_title(event: &CollabResumeBeginEvent) -> String {
    format!(
        "Resume {}",
        collab_agent_label(
            event.receiver_agent_nickname.as_deref(),
            event.receiver_agent_role.as_deref(),
            &event.receiver_thread_id,
        )
    )
}

fn collab_resume_end_title(event: &CollabResumeEndEvent) -> String {
    format!(
        "Resumed {}",
        collab_agent_label(
            event.receiver_agent_nickname.as_deref(),
            event.receiver_agent_role.as_deref(),
            &event.receiver_thread_id,
        )
    )
}

fn collab_resume_end_content(event: &CollabResumeEndEvent) -> Vec<ToolCallContent> {
    collab_status_content(&event.status, None)
}

fn collab_status_content(status: &AgentStatus, prompt: Option<&str>) -> Vec<ToolCallContent> {
    let mut content = vec![tool_call_text_content(format!(
        "Status: {}",
        format_agent_status(status)
    ))];
    if let Some(prompt) = prompt {
        content.extend(collab_prompt_content(prompt));
    }
    content
}

fn collab_prompt_content(prompt: &str) -> Vec<ToolCallContent> {
    format_collab_prompt(prompt)
        .map(tool_call_text_content)
        .into_iter()
        .collect()
}

fn collab_agent_label(nickname: Option<&str>, role: Option<&str>, thread_id: &ThreadId) -> String {
    format_agent_label(nickname, role, Some(thread_id))
}
