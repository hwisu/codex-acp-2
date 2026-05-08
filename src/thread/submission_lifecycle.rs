use agent_client_protocol::{
    Error,
    schema::{
        Content, ContentBlock, Meta, ResourceLink, SessionUpdate, StopReason, ToolCall,
        ToolCallContent, ToolCallLocation, ToolCallStatus, ToolKind, UsageUpdate,
    },
};
use codex_protocol::{
    models::MessagePhase,
    plan_tool::UpdatePlanArgs,
    protocol::{
        AgentMessageContentDeltaEvent, AgentMessageEvent, AgentReasoningEvent, ErrorEvent,
        ItemCompletedEvent, ItemStartedEvent, StreamErrorEvent, ThreadGoalUpdatedEvent,
        ThreadRolledBackEvent, TokenCountEvent, TurnAbortedEvent, TurnCompleteEvent,
        TurnStartedEvent, UserMessageEvent, ViewImageToolCallEvent, WarningEvent,
    },
};
use serde_json::json;
use tracing::{error, info, warn};

use crate::display::format_thread_goal_update;

use super::{client::SessionClient, submission::PromptState};

const TOKEN_USAGE_META_KEY: &str = "codex_token_usage";

impl PromptState {
    pub(super) fn turn_started(
        TurnStartedEvent {
            model_context_window,
            collaboration_mode_kind,
            turn_id,
            started_at: _,
        }: TurnStartedEvent,
    ) {
        info!(
            "Task started with context window of {turn_id} {model_context_window:?} {collaboration_mode_kind:?}"
        );
    }

    pub(super) fn token_count(
        client: &SessionClient,
        TokenCountEvent {
            info,
            rate_limits: _,
        }: TokenCountEvent,
    ) {
        if let Some(info) = info.as_ref()
            && let Some(size) = info.model_context_window
        {
            let used =
                u64::try_from(info.last_token_usage.tokens_in_context_window()).unwrap_or_default();
            let Ok(size) = u64::try_from(size) else {
                return;
            };
            let usage_update = UsageUpdate::new(used, size).meta(Meta::from_iter([(
                TOKEN_USAGE_META_KEY.to_string(),
                json!({
                    "total": &info.total_token_usage,
                    "last": &info.last_token_usage,
                }),
            )]));
            client.send_notification(SessionUpdate::UsageUpdate(usage_update));
        }
    }

    pub(super) fn agent_message_content_delta(
        &mut self,
        client: &SessionClient,
        AgentMessageContentDeltaEvent {
            thread_id,
            turn_id,
            item_id,
            delta,
        }: AgentMessageContentDeltaEvent,
    ) {
        info!(
            "Agent message content delta received: thread_id: {thread_id}, turn_id: {turn_id}, item_id: {item_id}, delta: {delta:?}"
        );
        self.record_message_delta(&delta);
        client.send_agent_text(delta);
    }

    pub(super) fn item_started(
        ItemStartedEvent {
            thread_id,
            turn_id,
            item,
            ..
        }: ItemStartedEvent,
    ) {
        info!("Item started with thread_id: {thread_id}, turn_id: {turn_id}, item: {item:?}");
    }

    pub(super) fn user_message(
        UserMessageEvent {
            message,
            images: _,
            text_elements: _,
            local_images: _,
        }: UserMessageEvent,
    ) {
        info!("User message: {message:?}");
    }

    pub(super) fn reasoning_content_delta(
        &mut self,
        client: &SessionClient,
        thread_id: String,
        turn_id: String,
        item_id: String,
        index: i64,
        delta: String,
    ) {
        info!(
            "Agent reasoning content delta received: thread_id: {thread_id}, turn_id: {turn_id}, item_id: {item_id}, index: {index}, delta: {delta:?}"
        );
        self.mark_reasoning_delta_seen();
        client.send_agent_thought(delta);
    }

    pub(super) fn reasoning_section_break(&mut self, client: &SessionClient) {
        self.mark_reasoning_delta_seen();
        client.send_agent_thought("\n\n");
    }

    pub(super) fn agent_message(
        &mut self,
        client: &SessionClient,
        AgentMessageEvent {
            message,
            phase,
            memory_citation: _,
        }: AgentMessageEvent,
    ) {
        info!("Agent message (non-delta) received: {message:?}");
        let streamed_message = self.take_message_delta_text();
        let should_send = match (phase, streamed_message.as_deref()) {
            (_, None) => true,
            (Some(MessagePhase::FinalAnswer), Some(streamed)) => streamed != message,
            _ => false,
        };
        if should_send {
            client.send_agent_text(message);
        }
    }

    pub(super) fn agent_reasoning(
        &mut self,
        client: &SessionClient,
        AgentReasoningEvent { text }: AgentReasoningEvent,
    ) {
        info!("Agent reasoning (non-delta) received: {text:?}");
        // We didn't receive this message via streaming.
        if !self.take_seen_reasoning_deltas() {
            client.send_agent_thought(text);
        }
    }

    pub(super) fn thread_goal_updated(client: &SessionClient, event: ThreadGoalUpdatedEvent) {
        info!("Thread goal updated: {:?}", event.goal.objective);
        client.send_agent_text(format_thread_goal_update(&event));
    }

    pub(super) fn plan_update(
        client: &SessionClient,
        UpdatePlanArgs { explanation, plan }: UpdatePlanArgs,
    ) {
        info!("Agent plan updated. Explanation: {:?}", explanation);
        client.update_plan(plan);
    }

    pub(super) fn item_completed(
        ItemCompletedEvent {
            thread_id,
            turn_id,
            item,
            ..
        }: ItemCompletedEvent,
    ) {
        info!("Item completed: thread_id={thread_id}, turn_id={turn_id}, item={item:?}");
    }

    pub(super) fn thread_rolled_back(
        client: &SessionClient,
        ThreadRolledBackEvent { num_turns }: ThreadRolledBackEvent,
    ) {
        if num_turns == 1 {
            client.send_agent_text("Undo completed.");
        } else {
            client.send_agent_text(format!("Rolled back {num_turns} turns."));
        }
    }

    pub(super) fn stream_error(
        StreamErrorEvent {
            message,
            codex_error_info,
            additional_details,
        }: StreamErrorEvent,
    ) {
        error!("Handled error during turn: {message} {codex_error_info:?} {additional_details:?}");
    }

    pub(super) fn shutdown_complete(&mut self) {
        info!("Agent shutting down");
        self.abort_pending_interactions();
        self.send_result(Ok(StopReason::Cancelled));
    }

    pub(super) fn warning(client: &SessionClient, WarningEvent { message }: WarningEvent) {
        warn!("Warning: {message}");
        client.send_agent_warning(message);
    }

    pub(super) fn context_compacted(client: &SessionClient) {
        info!("Context compacted");
        client.send_agent_text("Context compacted\n".to_string());
    }

    pub(super) fn skills_update_available(client: &SessionClient) {
        info!("Skills update available");
        client.send_agent_text("Skills changed. Run /skills to refresh the list.".to_string());
    }

    pub(super) fn turn_complete(
        &mut self,
        TurnCompleteEvent {
            last_agent_message,
            turn_id,
            completed_at: _,
            duration_ms: _,
            time_to_first_token_ms: _,
        }: TurnCompleteEvent,
    ) {
        info!(
            "Task {turn_id} completed successfully after {} events. Last agent message: {last_agent_message:?}",
            self.event_count()
        );
        self.abort_pending_interactions();
        self.send_result(Ok(StopReason::EndTurn));
    }

    pub(super) fn error(
        &mut self,
        ErrorEvent {
            message,
            codex_error_info,
        }: ErrorEvent,
    ) {
        error!("Unhandled error during turn: {message} {codex_error_info:?}");
        self.abort_pending_interactions();
        self.send_result(Err(Error::internal_error().data(
            json!({ "message": message, "codex_error_info": codex_error_info }),
        )));
    }

    pub(super) fn turn_aborted(
        &mut self,
        TurnAbortedEvent {
            reason,
            turn_id,
            completed_at: _,
            duration_ms: _,
        }: TurnAbortedEvent,
    ) {
        info!("Turn {turn_id:?} aborted: {reason:?}");
        self.abort_pending_interactions();
        self.send_result(Ok(StopReason::Cancelled));
    }

    pub(super) fn view_image_tool_call(
        client: &SessionClient,
        ViewImageToolCallEvent { call_id, path }: ViewImageToolCallEvent,
    ) {
        info!("ViewImageToolCallEvent received");
        let display_path = path.display().to_string();
        client.send_notification(SessionUpdate::ToolCall(
            ToolCall::new(call_id, format!("View Image {display_path}"))
                .kind(ToolKind::Read)
                .status(ToolCallStatus::Completed)
                .content(vec![ToolCallContent::Content(Content::new(
                    ContentBlock::ResourceLink(ResourceLink::new(
                        display_path.clone(),
                        display_path,
                    )),
                ))])
                .locations(vec![ToolCallLocation::new(path)]),
        ));
    }
}
