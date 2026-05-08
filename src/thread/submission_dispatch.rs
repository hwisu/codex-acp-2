use codex_protocol::{
    dynamic_tools::DynamicToolCallRequest,
    protocol::{
        AgentReasoningSectionBreakEvent, EventMsg, McpStartupCompleteEvent, McpStartupUpdateEvent,
        McpToolCallBeginEvent, McpToolCallEndEvent, ModelRerouteEvent, ReasoningContentDeltaEvent,
        ReasoningRawContentDeltaEvent, WebSearchBeginEvent, WebSearchEndEvent,
    },
};
use tracing::{info, warn};

use super::{client::SessionClient, submission::PromptState};

impl PromptState {
    pub(super) async fn handle_event(&mut self, client: &SessionClient, event: EventMsg) {
        self.increment_event_count();

        if should_complete_web_search_before_event(&event) {
            self.complete_web_search(client);
        }

        match event {
            EventMsg::TurnStarted(event) => {
                Self::turn_started(event);
            }
            EventMsg::TokenCount(event) => {
                Self::token_count(client, event);
            }
            EventMsg::ItemStarted(event) => {
                Self::item_started(event);
            }
            EventMsg::UserMessage(event) => {
                Self::user_message(event);
            }
            EventMsg::AgentMessageContentDelta(event) => {
                self.agent_message_content_delta(client, event);
            }
            EventMsg::ReasoningContentDelta(ReasoningContentDeltaEvent {
                thread_id,
                turn_id,
                item_id,
                delta,
                summary_index: index,
            })
            | EventMsg::ReasoningRawContentDelta(ReasoningRawContentDeltaEvent {
                thread_id,
                turn_id,
                item_id,
                delta,
                content_index: index,
            }) => {
                self.reasoning_content_delta(client, thread_id, turn_id, item_id, index, delta);
            }
            EventMsg::AgentReasoningSectionBreak(AgentReasoningSectionBreakEvent {
                item_id,
                summary_index,
            }) => {
                info!("Agent reasoning section break received:  item_id: {item_id}, index: {summary_index}");
                self.reasoning_section_break(client);
            }
            EventMsg::AgentMessage(event) => {
                self.agent_message(client, event);
            }
            EventMsg::AgentReasoning(event) => {
                self.agent_reasoning(client, event);
            }
            EventMsg::ThreadGoalUpdated(event) => {
                Self::thread_goal_updated(client, event);
            }
            EventMsg::PlanUpdate(event) => {
                Self::plan_update(client, event);
            }
            EventMsg::WebSearchBegin(WebSearchBeginEvent { call_id }) => {
                info!("Web search started: call_id={}", call_id);
                // Create a ToolCall notification for the search beginning
                self.start_web_search(client, call_id);
            }
            EventMsg::WebSearchEnd(WebSearchEndEvent {
                call_id,
                query,
                action,
            }) => {
                info!("Web search query received: call_id={call_id}, query={query}");
                // Send update that the search is in progress with the query
                // (WebSearchEnd just means we have the query, not that results are ready)
                Self::update_web_search_query(client, call_id, query, action);
                // The actual search results will come through AgentMessage events
                // We mark as completed when a new tool call begins
            }
            event @ (EventMsg::ExecApprovalRequest(..)
            | EventMsg::ExecCommandBegin(..)
            | EventMsg::ExecCommandOutputDelta(..)
            | EventMsg::ExecCommandEnd(..)
            | EventMsg::TerminalInteraction(..)) => self.handle_exec_event(client, event),
            EventMsg::DynamicToolCallRequest(DynamicToolCallRequest {
                call_id,
                turn_id,
                namespace,
                tool,
                arguments,
                ..
            }) => {
                info!("Dynamic tool call request: call_id={call_id}, turn_id={turn_id}, namespace={namespace:?}, tool={tool}");
                Self::start_dynamic_tool_call(client, call_id, &tool, &arguments);
            }
            EventMsg::DynamicToolCallResponse(event) => {
                info!(
                    "Dynamic tool call response: call_id={}, turn_id={}, tool={}",
                    event.call_id, event.turn_id, event.tool
                );
                Self::end_dynamic_tool_call(client, event);
            }
            EventMsg::McpToolCallBegin(McpToolCallBeginEvent {
                call_id,
                invocation,
                mcp_app_resource_uri: _
            }) => {
                info!(
                    "MCP tool call begin: call_id={call_id}, invocation={} {}",
                    invocation.server, invocation.tool
                );
                Self::start_mcp_tool_call(client, call_id, &invocation);
            }
            EventMsg::McpToolCallEnd(McpToolCallEndEvent {
                call_id,
                invocation,
                duration,
                result,
                mcp_app_resource_uri: _,
            }) => {
                info!(
                    "MCP tool call ended: call_id={call_id}, invocation={} {}, duration={duration:?}",
                    invocation.server, invocation.tool
                );
                Self::end_mcp_tool_call(client, call_id, result);
            }
            event @ (EventMsg::ApplyPatchApprovalRequest(..)
            | EventMsg::PatchApplyBegin(..)
            | EventMsg::PatchApplyUpdated(..)
            | EventMsg::PatchApplyEnd(..)) => self.handle_patch_event(client, event),
            EventMsg::ItemCompleted(event) => {
                Self::item_completed(event);
            }
            EventMsg::TurnComplete(event) => {
                self.turn_complete(event);
            }
            EventMsg::ThreadRolledBack(event) => {
                Self::thread_rolled_back(client, event);
            }
            EventMsg::StreamError(event) => {
                Self::stream_error(event);
            }
            EventMsg::Error(event) => {
                self.error(event);
            }
            EventMsg::TurnAborted(event) => {
                self.turn_aborted(event);
            }
            EventMsg::ShutdownComplete => {
                self.shutdown_complete();
            }
            EventMsg::ViewImageToolCall(event) => {
                Self::view_image_tool_call(client, event);
            }
            EventMsg::EnteredReviewMode(review_request) => {
                info!("Review begin: request={review_request:?}");
            }
            EventMsg::ExitedReviewMode(event) => {
                info!("Review end: output={event:?}");
                Self::review_mode_exit(client, event);
            }
            EventMsg::Warning(event) | EventMsg::GuardianWarning(event) => {
                Self::warning(client, event);
            }
            EventMsg::McpStartupUpdate(McpStartupUpdateEvent { server, status }) => {
                info!("MCP startup update: server={server}, status={status:?}");
            }
            EventMsg::McpStartupComplete(McpStartupCompleteEvent {
                ready,
                failed,
                cancelled,
            }) => {
                info!(
                    "MCP startup complete: ready={ready:?}, failed={failed:?}, cancelled={cancelled:?}"
                );
            }
            EventMsg::ElicitationRequest(event) => {
                info!("Elicitation request: server={}, id={:?}", event.server_name, event.id);
                if let Err(err) = self.mcp_elicitation(client, event).await {
                    self.send_result(Err(err));
                }
            }
            EventMsg::ModelReroute(ModelRerouteEvent { from_model, to_model, reason }) => {
                info!("Model reroute: from={from_model}, to={to_model}, reason={reason:?}");
            }
            EventMsg::ModelVerification(event) => {
                info!("Model verification requested: {event:?}");
            }

            EventMsg::ContextCompacted(..) => {
                Self::context_compacted(client);
            }
            EventMsg::RequestPermissions(event) => {
                info!("Request permissions: {} {}", event.call_id, event.turn_id);
                self.request_permissions(client, event);
            }
            EventMsg::GuardianAssessment(event) => {
                info!(
                    "Guardian assessment: id={}, status={:?}, turn_id={}",
                    event.id, event.status, event.turn_id
                );
                self.guardian_assessment(client, event);
            }
            EventMsg::ImageGenerationBegin(event) => {
                info!("Image generation started: call_id={}", event.call_id);
                Self::image_generation_begin(client, event);
            }
            EventMsg::ImageGenerationEnd(event) => {
                info!(
                    "Image generation ended: call_id={}, status={}",
                    event.call_id, event.status
                );
                Self::image_generation_end(client, event);
            }
            event @ (EventMsg::CollabAgentSpawnBegin(..)
            | EventMsg::CollabAgentSpawnEnd(..)
            | EventMsg::CollabAgentInteractionBegin(..)
            | EventMsg::CollabAgentInteractionEnd(..)
            | EventMsg::CollabWaitingBegin(..)
            | EventMsg::CollabWaitingEnd(..)
            | EventMsg::CollabCloseBegin(..)
            | EventMsg::CollabCloseEnd(..)
            | EventMsg::CollabResumeBegin(..)
            | EventMsg::CollabResumeEnd(..)) => Self::handle_collab_event(client, event),
            EventMsg::SkillsUpdateAvailable => {
                Self::skills_update_available(client);
            }

            // Ignore these events
            EventMsg::AgentReasoningRawContent(..)
            | EventMsg::HookStarted(..)
            | EventMsg::HookCompleted(..)
            // we already have a way to diff the turn, so ignore
            | EventMsg::TurnDiff(..)
            | EventMsg::RawResponseItem(..)
            | EventMsg::SessionConfigured(..)
            | EventMsg::RealtimeConversationStarted(..)
            | EventMsg::RealtimeConversationRealtime(..)
            | EventMsg::RealtimeConversationClosed(..)
            | EventMsg::RealtimeConversationSdp(..)
            | EventMsg::PlanDelta(..)=> {}
            e @ (EventMsg::RealtimeConversationListVoicesResponse(..)
            | EventMsg::DeprecationNotice(..)
            | EventMsg::RequestUserInput(..)) => {
                warn!("Unexpected event: {:?}", e);
            }
        }
    }

    fn handle_exec_event(&mut self, client: &SessionClient, event: EventMsg) {
        match event {
            EventMsg::ExecApprovalRequest(event) => {
                info!(
                    "Command execution started: call_id={}, command={:?}",
                    event.call_id, event.command
                );
                if let Err(err) = self.exec_approval(client, event) {
                    self.send_result(Err(err));
                }
            }
            EventMsg::ExecCommandBegin(event) => {
                info!(
                    "Command execution started: call_id={}, command={:?}",
                    event.call_id, event.command
                );
                self.exec_command_begin(client, event);
            }
            EventMsg::ExecCommandOutputDelta(event) => {
                self.exec_command_output_delta(client, event);
            }
            EventMsg::ExecCommandEnd(event) => {
                info!(
                    "Command execution ended: call_id={}, exit_code={}",
                    event.call_id, event.exit_code
                );
                self.exec_command_end(client, event);
            }
            EventMsg::TerminalInteraction(event) => {
                info!(
                    "Terminal interaction: call_id={}, process_id={}, stdin={}",
                    event.call_id, event.process_id, event.stdin
                );
                self.terminal_interaction(client, event);
            }
            _ => unreachable!("non-exec event routed to handle_exec_event"),
        }
    }

    fn handle_patch_event(&mut self, client: &SessionClient, event: EventMsg) {
        match event {
            EventMsg::ApplyPatchApprovalRequest(event) => {
                info!(
                    "Apply patch approval request: call_id={}, reason={:?}",
                    event.call_id, event.reason
                );
                self.patch_approval(client, event);
            }
            EventMsg::PatchApplyBegin(event) => {
                info!(
                    "Patch apply begin: call_id={}, auto_approved={}",
                    event.call_id, event.auto_approved
                );
                Self::start_patch_apply(client, event);
            }
            EventMsg::PatchApplyUpdated(event) => {
                info!(
                    "Patch apply updated: call_id={}, change_count={}",
                    event.call_id,
                    event.changes.len()
                );
                Self::update_patch_apply(client, event);
            }
            EventMsg::PatchApplyEnd(event) => {
                info!(
                    "Patch apply end: call_id={}, success={}",
                    event.call_id, event.success
                );
                Self::end_patch_apply(client, event);
            }
            _ => unreachable!("non-patch event routed to handle_patch_event"),
        }
    }

    fn handle_collab_event(client: &SessionClient, event: EventMsg) {
        match event {
            EventMsg::CollabAgentSpawnBegin(event) => {
                info!("Subagent spawn started: call_id={}", event.call_id);
                Self::collab_spawn_begin(client, &event);
            }
            EventMsg::CollabAgentSpawnEnd(event) => {
                info!("Subagent spawn ended: call_id={}", event.call_id);
                Self::collab_spawn_end(client, &event);
            }
            EventMsg::CollabAgentInteractionBegin(event) => {
                info!("Subagent interaction started: call_id={}", event.call_id);
                Self::collab_interaction_begin(client, &event);
            }
            EventMsg::CollabAgentInteractionEnd(event) => {
                info!("Subagent interaction ended: call_id={}", event.call_id);
                Self::collab_interaction_end(client, &event);
            }
            EventMsg::CollabWaitingBegin(event) => {
                info!("Subagent wait started: call_id={}", event.call_id);
                Self::collab_waiting_begin(client, &event);
            }
            EventMsg::CollabWaitingEnd(event) => {
                info!("Subagent wait ended: call_id={}", event.call_id);
                Self::collab_waiting_end(client, &event);
            }
            EventMsg::CollabCloseBegin(event) => {
                info!("Subagent close started: call_id={}", event.call_id);
                Self::collab_close_begin(client, &event);
            }
            EventMsg::CollabCloseEnd(event) => {
                info!("Subagent close ended: call_id={}", event.call_id);
                Self::collab_close_end(client, &event);
            }
            EventMsg::CollabResumeBegin(event) => {
                info!("Subagent resume started: call_id={}", event.call_id);
                Self::collab_resume_begin(client, &event);
            }
            EventMsg::CollabResumeEnd(event) => {
                info!("Subagent resume ended: call_id={}", event.call_id);
                Self::collab_resume_end(client, &event);
            }
            _ => unreachable!("non-collab event routed to handle_collab_event"),
        }
    }
}

fn should_complete_web_search_before_event(event: &EventMsg) -> bool {
    matches!(
        event,
        EventMsg::Error(..)
            | EventMsg::StreamError(..)
            | EventMsg::WebSearchBegin(..)
            | EventMsg::UserMessage(..)
            | EventMsg::ExecApprovalRequest(..)
            | EventMsg::ExecCommandBegin(..)
            | EventMsg::ExecCommandOutputDelta(..)
            | EventMsg::ExecCommandEnd(..)
            | EventMsg::McpToolCallBegin(..)
            | EventMsg::McpToolCallEnd(..)
            | EventMsg::ApplyPatchApprovalRequest(..)
            | EventMsg::PatchApplyBegin(..)
            | EventMsg::PatchApplyEnd(..)
            | EventMsg::TurnStarted(..)
            | EventMsg::TurnComplete(..)
            | EventMsg::TurnDiff(..)
            | EventMsg::TurnAborted(..)
            | EventMsg::EnteredReviewMode(..)
            | EventMsg::ExitedReviewMode(..)
            | EventMsg::ShutdownComplete
    )
}
