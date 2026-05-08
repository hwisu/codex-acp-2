use agent_client_protocol::schema::{
    ToolCall, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use codex_protocol::protocol::{
    CollabAgentInteractionBeginEvent, CollabAgentInteractionEndEvent, CollabAgentSpawnBeginEvent,
    CollabAgentSpawnEndEvent, CollabCloseBeginEvent, CollabCloseEndEvent, CollabResumeBeginEvent,
    CollabResumeEndEvent, CollabWaitingBeginEvent, CollabWaitingEndEvent,
};
use itertools::Itertools;

use crate::display::{format_collab_agent_ref, tool_call_text_content};

use super::{
    client::SessionClient,
    collab_render::{
        close_end_content, close_end_title, interaction_end_content, interaction_end_title,
        prompt_content, resume_begin_title, resume_end_content, resume_end_title,
        spawn_begin_content, spawn_end_content, spawn_end_status, spawn_end_title,
        waiting_end_content,
    },
    submission::PromptState,
};

impl PromptState {
    pub(super) fn collab_spawn_begin(client: &SessionClient, event: &CollabAgentSpawnBeginEvent) {
        let raw_input = serde_json::json!(event);
        let call_id = event.call_id.clone();
        let content = spawn_begin_content(&event.model, event.reasoning_effort, &event.prompt);

        client.send_tool_call(
            ToolCall::new(call_id, "Spawn subagent")
                .kind(ToolKind::Other)
                .status(ToolCallStatus::InProgress)
                .raw_input(raw_input)
                .content(content),
        );
    }

    pub(super) fn collab_spawn_end(client: &SessionClient, event: &CollabAgentSpawnEndEvent) {
        let raw_output = serde_json::json!(event);
        let call_id = event.call_id.clone();
        let title = spawn_end_title(event);
        let status = spawn_end_status(event);
        let content = spawn_end_content(event);

        client.send_tool_call_update(ToolCallUpdate::new(
            call_id,
            ToolCallUpdateFields::new()
                .status(status)
                .title(Some(title))
                .content(Some(content))
                .raw_output(raw_output),
        ));
    }

    pub(super) fn collab_interaction_begin(
        client: &SessionClient,
        event: &CollabAgentInteractionBeginEvent,
    ) {
        let raw_input = serde_json::json!(event);
        let call_id = event.call_id.clone();
        let content = prompt_content(&event.prompt);

        client.send_tool_call(
            ToolCall::new(call_id, "Send input to subagent")
                .kind(ToolKind::Other)
                .status(ToolCallStatus::InProgress)
                .raw_input(raw_input)
                .content(content),
        );
    }

    pub(super) fn collab_interaction_end(
        client: &SessionClient,
        event: &CollabAgentInteractionEndEvent,
    ) {
        let raw_output = serde_json::json!(event);
        let call_id = event.call_id.clone();
        let title = interaction_end_title(event);
        let content = interaction_end_content(event);

        client.send_tool_call_update(ToolCallUpdate::new(
            call_id,
            ToolCallUpdateFields::new()
                .status(ToolCallStatus::Completed)
                .title(Some(title))
                .content(Some(content))
                .raw_output(raw_output),
        ));
    }

    pub(super) fn collab_waiting_begin(client: &SessionClient, event: &CollabWaitingBeginEvent) {
        let raw_input = serde_json::json!(event);
        let call_id = event.call_id.clone();
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

        client.send_tool_call(
            ToolCall::new(call_id, title)
                .kind(ToolKind::Other)
                .status(ToolCallStatus::InProgress)
                .raw_input(raw_input)
                .content(content.unwrap_or_default()),
        );
    }

    pub(super) fn collab_waiting_end(client: &SessionClient, event: &CollabWaitingEndEvent) {
        let raw_output = serde_json::json!(event);
        let call_id = event.call_id.clone();
        let content = waiting_end_content(event);

        client.send_tool_call_update(ToolCallUpdate::new(
            call_id,
            ToolCallUpdateFields::new()
                .status(ToolCallStatus::Completed)
                .title(Some("Subagent wait complete".to_string()))
                .content((!content.is_empty()).then_some(content))
                .raw_output(raw_output),
        ));
    }

    pub(super) fn collab_close_begin(client: &SessionClient, event: &CollabCloseBeginEvent) {
        let raw_input = serde_json::json!(event);
        let call_id = event.call_id.clone();
        client.send_tool_call(
            ToolCall::new(call_id, "Close subagent")
                .kind(ToolKind::Other)
                .status(ToolCallStatus::InProgress)
                .raw_input(raw_input),
        );
    }

    pub(super) fn collab_close_end(client: &SessionClient, event: &CollabCloseEndEvent) {
        let raw_output = serde_json::json!(event);
        let call_id = event.call_id.clone();
        let title = close_end_title(event);
        let content = close_end_content(event);

        client.send_tool_call_update(ToolCallUpdate::new(
            call_id,
            ToolCallUpdateFields::new()
                .status(ToolCallStatus::Completed)
                .title(Some(title))
                .content(Some(content))
                .raw_output(raw_output),
        ));
    }

    pub(super) fn collab_resume_begin(client: &SessionClient, event: &CollabResumeBeginEvent) {
        let raw_input = serde_json::json!(event);
        let call_id = event.call_id.clone();
        let title = resume_begin_title(event);
        client.send_tool_call(
            ToolCall::new(call_id, title)
                .kind(ToolKind::Other)
                .status(ToolCallStatus::InProgress)
                .raw_input(raw_input),
        );
    }

    pub(super) fn collab_resume_end(client: &SessionClient, event: &CollabResumeEndEvent) {
        let raw_output = serde_json::json!(event);
        let call_id = event.call_id.clone();
        let title = resume_end_title(event);
        let content = resume_end_content(event);

        client.send_tool_call_update(ToolCallUpdate::new(
            call_id,
            ToolCallUpdateFields::new()
                .status(ToolCallStatus::Completed)
                .title(Some(title))
                .content(Some(content))
                .raw_output(raw_output),
        ));
    }
}
