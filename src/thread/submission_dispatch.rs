use codex_protocol::protocol::EventMsg;
use tracing::info;

use crate::boundary::{
    effect::IgnoredCodexEventReason,
    mapper::{self, LiveEventRoute, LiveExecEvent, LiveForwardEvent, LivePermissionEvent},
};

use super::{client::SessionClient, submission::PromptState};

impl PromptState {
    pub(super) async fn handle_event(&mut self, client: &SessionClient, event: EventMsg) {
        self.increment_event_count();
        if mapper::completes_active_web_search_before(&event)
            && let Some(effect) = self.complete_web_search()
        {
            self.execute_or_fail(client, effect).await;
        }

        match mapper::route_live_event(event) {
            LiveEventRoute::Forward(event) => self.handle_forward_event(client, event).await,
            LiveEventRoute::RequestPermission(event) => {
                self.handle_permission_event(client, event).await;
            }
            LiveEventRoute::Ignore { event, reason } => {
                log_ignored_codex_event(&event, reason);
            }
        }
    }

    async fn handle_forward_event(&mut self, client: &SessionClient, event: LiveForwardEvent) {
        match event {
            LiveForwardEvent::Effect(effect) => {
                self.execute_or_fail(client, *effect).await;
            }
            LiveForwardEvent::AgentMessageContentDelta(event) => {
                let effect = self.agent_message_content_delta(event);
                self.execute_or_fail(client, effect).await;
            }
            LiveForwardEvent::ReasoningContentDelta(event) => {
                let effect = self.reasoning_content_delta(
                    event.thread_id,
                    event.turn_id,
                    event.item_id,
                    event.index,
                    event.delta,
                );
                self.execute_or_fail(client, effect).await;
            }
            LiveForwardEvent::AgentReasoningSectionBreak(event) => {
                info!(
                    "Agent reasoning section break received:  item_id: {}, index: {}",
                    event.item_id, event.summary_index
                );
                let effect = self.reasoning_section_break();
                self.execute_or_fail(client, effect).await;
            }
            LiveForwardEvent::AgentMessage(event) => {
                if let Some(effect) = self.agent_message(event) {
                    self.execute_or_fail(client, effect).await;
                }
            }
            LiveForwardEvent::AgentReasoning(event) => {
                if let Some(effect) = self.agent_reasoning(event) {
                    self.execute_or_fail(client, effect).await;
                }
            }
            LiveForwardEvent::WebSearchBegin(event) => {
                let call_id = event.call_id;
                info!("Web search started: call_id={call_id}");
                let effect = self.start_web_search(call_id);
                self.execute_or_fail(client, effect).await;
            }
            LiveForwardEvent::WebSearchEnd(event) => {
                let call_id = event.call_id;
                let query = event.query;
                let action = event.action;
                info!("Web search query received: call_id={call_id}, query={query}");
                let effect = Self::update_web_search_query(call_id, query, action);
                self.execute_or_fail(client, effect).await;
            }
            LiveForwardEvent::Exec(event) => {
                self.handle_exec_event(client, event).await;
            }
            LiveForwardEvent::TurnComplete(event) => {
                self.turn_complete(event);
            }
            LiveForwardEvent::Error(event) => {
                self.error(event);
            }
            LiveForwardEvent::TurnAborted(event) => {
                self.turn_aborted(event);
            }
            LiveForwardEvent::ShutdownComplete => {
                self.shutdown_complete();
            }
            LiveForwardEvent::GuardianAssessment(event) => {
                info!(
                    "Guardian assessment: id={}, status={:?}, turn_id={}",
                    event.id, event.status, event.turn_id
                );
                let effect = self.guardian_assessment(event);
                self.execute_or_fail(client, effect).await;
            }
        }
    }

    async fn handle_permission_event(
        &mut self,
        client: &SessionClient,
        event: LivePermissionEvent,
    ) {
        match event {
            LivePermissionEvent::ExecApprovalRequest(event) => {
                info!(
                    "Command execution started: call_id={}, command={:?}",
                    event.call_id, event.command
                );
                if let Err(err) = self.exec_approval(client, event) {
                    self.send_result(Err(err));
                }
            }
            LivePermissionEvent::RequestPermissions(event) => {
                info!("Request permissions: {} {}", event.call_id, event.turn_id);
                self.request_permissions(client, event);
            }
            LivePermissionEvent::ElicitationRequest(event) => {
                info!(
                    "Elicitation request: server={}, id={:?}",
                    event.server_name, event.id
                );
                if let Err(err) = self.mcp_elicitation(client, event).await {
                    self.send_result(Err(err));
                }
            }
            LivePermissionEvent::ApplyPatchApprovalRequest(event) => {
                info!(
                    "Apply patch approval request: call_id={}, reason={:?}",
                    event.call_id, event.reason
                );
                self.patch_approval(client, event);
            }
        }
    }

    async fn handle_exec_event(&mut self, client: &SessionClient, event: LiveExecEvent) {
        match event {
            LiveExecEvent::CommandBegin(event) => {
                info!(
                    "Command execution started: call_id={}, command={:?}",
                    event.call_id, event.command
                );
                let effect = self.exec_command_begin(client, event);
                self.execute_or_fail(client, effect).await;
            }
            LiveExecEvent::OutputDelta(event) => {
                if let Some(effect) = self.exec_command_output_delta(client, event) {
                    self.execute_or_fail(client, effect).await;
                }
            }
            LiveExecEvent::CommandEnd(event) => {
                info!(
                    "Command execution ended: call_id={}, exit_code={}",
                    event.call_id, event.exit_code
                );
                for effect in self.exec_command_end(client, event) {
                    self.execute_or_fail(client, effect).await;
                }
            }
            LiveExecEvent::TerminalInteraction(event) => {
                info!(
                    "Terminal interaction: call_id={}, process_id={}, stdin={}",
                    event.call_id, event.process_id, event.stdin
                );
                if let Some(effect) = self.terminal_interaction(client, event) {
                    self.execute_or_fail(client, effect).await;
                }
            }
        }
    }
}

impl PromptState {
    async fn execute_or_fail(
        &mut self,
        client: &SessionClient,
        effect: crate::boundary::effect::BridgeEffect,
    ) {
        if let Err(err) = self.execute_bridge_effect(client, effect).await {
            self.send_result(Err(err));
        }
    }
}

fn log_ignored_codex_event(event: &EventMsg, reason: IgnoredCodexEventReason) {
    info!("Ignoring Codex event {event}: {reason:?}");
}
