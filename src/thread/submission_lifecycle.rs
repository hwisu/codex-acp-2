use agent_client_protocol::{Error, schema::v1::StopReason};
use codex_protocol::{
    models::MessagePhase,
    protocol::{
        AgentMessageContentDeltaEvent, AgentMessageEvent, AgentReasoningEvent, ErrorEvent,
        TurnAbortedEvent, TurnCompleteEvent,
    },
};
use serde_json::json;
use tracing::{error, info};

use crate::boundary::{effect::BridgeEffect, session_update};

use super::submission::PromptState;

impl PromptState {
    pub(super) fn agent_message_content_delta(
        &mut self,
        AgentMessageContentDeltaEvent {
            thread_id,
            turn_id,
            item_id,
            delta,
        }: AgentMessageContentDeltaEvent,
    ) -> BridgeEffect {
        info!(
            "Agent message content delta received: thread_id: {thread_id}, turn_id: {turn_id}, item_id: {item_id}, delta: {delta:?}"
        );
        self.record_message_delta(&delta);
        session_update::agent_text_effect(delta)
    }

    pub(super) fn reasoning_content_delta(
        &mut self,
        thread_id: String,
        turn_id: String,
        item_id: String,
        index: i64,
        delta: String,
    ) -> BridgeEffect {
        info!(
            "Agent reasoning content delta received: thread_id: {thread_id}, turn_id: {turn_id}, item_id: {item_id}, index: {index}, delta: {delta:?}"
        );
        self.mark_reasoning_delta_seen();
        session_update::agent_thought_effect(delta)
    }

    pub(super) fn reasoning_section_break(&mut self) -> BridgeEffect {
        self.mark_reasoning_delta_seen();
        session_update::agent_thought_effect("\n\n")
    }

    pub(super) fn agent_message(
        &mut self,
        AgentMessageEvent {
            message,
            phase,
            memory_citation: _,
        }: AgentMessageEvent,
    ) -> Option<BridgeEffect> {
        info!("Agent message (non-delta) received: {message:?}");
        let streamed_message = self.take_message_delta_text();
        let should_send = match (phase, streamed_message.as_deref()) {
            (_, None) => true,
            (Some(MessagePhase::FinalAnswer), Some(streamed)) => streamed != message,
            _ => false,
        };
        should_send.then(|| session_update::agent_text_effect(message))
    }

    pub(super) fn agent_reasoning(
        &mut self,
        AgentReasoningEvent { text }: AgentReasoningEvent,
    ) -> Option<BridgeEffect> {
        info!("Agent reasoning (non-delta) received: {text:?}");
        // We didn't receive this message via streaming.
        (!self.take_seen_reasoning_deltas()).then(|| session_update::agent_thought_effect(text))
    }

    pub(super) fn shutdown_complete(&mut self) {
        info!("Agent shutting down");
        self.abort_pending_interactions();
        self.send_result(Ok(StopReason::Cancelled));
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
}
