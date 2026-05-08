use agent_client_protocol::schema::{ToolCall, ToolCallLocation, ToolCallStatus, ToolKind};
use codex_protocol::{
    plan_tool::UpdatePlanArgs,
    protocol::{
        AgentMessageEvent, AgentReasoningEvent, AgentReasoningRawContentEvent, EventMsg,
        RolloutItem, TurnAbortedEvent, TurnCompleteEvent, UserMessageEvent,
    },
};

use crate::display::{format_image_generation_content, format_thread_goal_update};

use super::{
    actor::ThreadActor,
    collab_render::{
        close_end_content, close_end_title, interaction_end_content, interaction_end_title,
        resume_end_content, resume_end_title, spawn_end_content, spawn_end_status, spawn_end_title,
        waiting_end_content,
    },
    deps::Auth,
};

impl<A: Auth> ThreadActor<A> {
    /// Replay conversation history to the client via session/update notifications.
    /// This is called when loading a session to stream all prior messages.
    ///
    /// We process both `EventMsg` and `ResponseItem`:
    /// - `EventMsg` for user/agent messages and reasoning (like the TUI does)
    /// - `ResponseItem` for tool calls only (not persisted as `EventMsg`)
    pub(super) fn handle_replay_history(&mut self, history: Vec<RolloutItem>) {
        for item in history {
            match item {
                RolloutItem::EventMsg(event_msg) => {
                    self.replay_event_msg(&event_msg);
                }
                RolloutItem::ResponseItem(response_item) => {
                    self.replay_response_item(&response_item);
                }
                // Skip SessionMeta, TurnContext, Compacted
                _ => {}
            }
        }
    }

    /// Convert and send an `EventMsg` as ACP notification(s) during replay.
    /// Replays enough state to keep loaded sessions useful in external-agent clients.
    fn replay_event_msg(&mut self, msg: &EventMsg) {
        self.state.update_from_event(msg);

        match msg {
            EventMsg::UserMessage(UserMessageEvent { message, .. }) => {
                self.client.send_user_message(message.clone());
            }
            EventMsg::AgentMessage(AgentMessageEvent {
                message,
                phase: _,
                memory_citation: _,
            }) => {
                self.client.send_agent_text(message.clone());
            }
            EventMsg::AgentReasoning(AgentReasoningEvent { text })
            | EventMsg::AgentReasoningRawContent(AgentReasoningRawContentEvent { text }) => {
                self.client.send_agent_thought(text.clone());
            }
            EventMsg::ThreadGoalUpdated(event) => {
                self.client
                    .send_agent_text(format_thread_goal_update(event));
            }
            EventMsg::PlanUpdate(UpdatePlanArgs { plan, .. }) => {
                self.client.update_plan(plan.clone());
            }
            EventMsg::ImageGenerationEnd(event) => {
                let status = if event.status == "completed" {
                    ToolCallStatus::Completed
                } else {
                    ToolCallStatus::Failed
                };
                let locations = event
                    .saved_path
                    .clone()
                    .map(|path| vec![ToolCallLocation::new(path)]);
                let content = format_image_generation_content(event);
                let mut tool_call = ToolCall::new(event.call_id.clone(), "Generate image")
                    .kind(ToolKind::Other)
                    .status(status)
                    .content(content);
                if let Some(locations) = locations {
                    tool_call = tool_call.locations(locations);
                }

                self.client.send_tool_call(tool_call);
            }
            EventMsg::CollabAgentSpawnEnd(event) => {
                let title = spawn_end_title(event);
                let status = spawn_end_status(event);
                let content = spawn_end_content(event);

                self.client.send_tool_call(
                    ToolCall::new(event.call_id.clone(), title)
                        .kind(ToolKind::Other)
                        .status(status)
                        .content(content),
                );
            }
            EventMsg::CollabAgentInteractionEnd(event) => {
                let title = interaction_end_title(event);
                let content = interaction_end_content(event);

                self.client.send_tool_call(
                    ToolCall::new(event.call_id.clone(), title)
                        .kind(ToolKind::Other)
                        .status(ToolCallStatus::Completed)
                        .content(content),
                );
            }
            EventMsg::CollabWaitingEnd(event) => {
                let content = waiting_end_content(event);

                self.client.send_tool_call(
                    ToolCall::new(event.call_id.clone(), "Subagent wait complete")
                        .kind(ToolKind::Other)
                        .status(ToolCallStatus::Completed)
                        .content(content),
                );
            }
            EventMsg::CollabCloseEnd(event) => {
                let title = close_end_title(event);
                let content = close_end_content(event);

                self.client.send_tool_call(
                    ToolCall::new(event.call_id.clone(), title)
                        .kind(ToolKind::Other)
                        .status(ToolCallStatus::Completed)
                        .content(content),
                );
            }
            EventMsg::CollabResumeEnd(event) => {
                let title = resume_end_title(event);
                let content = resume_end_content(event);

                self.client.send_tool_call(
                    ToolCall::new(event.call_id.clone(), title)
                        .kind(ToolKind::Other)
                        .status(ToolCallStatus::Completed)
                        .content(content),
                );
            }
            EventMsg::SkillsUpdateAvailable => {
                self.client
                    .send_agent_text("Skills changed. Run /skills to refresh the list.");
            }
            EventMsg::RequestUserInput(event) => {
                self.register_pending_user_input(event.turn_id.clone(), event.clone());
            }
            EventMsg::TurnComplete(TurnCompleteEvent { turn_id, .. })
            | EventMsg::TurnAborted(TurnAbortedEvent {
                turn_id: Some(turn_id),
                ..
            }) => {
                self.state.clear_pending_user_input_for_submission(turn_id);
            }
            EventMsg::ShutdownComplete | EventMsg::Error(..) => {
                self.state.clear_pending_user_input();
            }
            // Skip other event types during replay - they either:
            // - Are transient (deltas, turn lifecycle)
            // - Don't have direct ACP equivalents
            // - Are handled via ResponseItem instead
            _ => {}
        }
    }
}
