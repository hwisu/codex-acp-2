use codex_protocol::{dynamic_tools::DynamicToolCallRequest, protocol::*};

use crate::boundary::{
    effect::{BridgeEffect, IgnoredCodexEventReason},
    session_update, tool_call,
};

use super::types::*;

pub(crate) fn route_live_event(event: EventMsg) -> LiveEventRoute {
    use IgnoredCodexEventReason::{
        AlreadyRenderedByAnotherEvent, AlreadyRenderedByClientInput, DiagnosticOnly,
        EmptyToolCallUpdate, HandledByActor, HandledByResponseItem, LifecycleOnly, StateOnly,
        SupersededBySnapshot, UnsupportedByAcp,
    };
    use LiveEventRoute::{Forward, Ignore, RequestPermission};

    match event {
        EventMsg::Error(event) => Forward(LiveForwardEvent::Error(event)),
        EventMsg::Warning(event) | EventMsg::GuardianWarning(event) => {
            effect_route(session_update::agent_warning_effect(event))
        }
        EventMsg::RealtimeConversationStarted(event) => Ignore {
            event: EventMsg::RealtimeConversationStarted(event),
            reason: UnsupportedByAcp,
        },
        EventMsg::RealtimeConversationRealtime(event) => Ignore {
            event: EventMsg::RealtimeConversationRealtime(event),
            reason: UnsupportedByAcp,
        },
        EventMsg::RealtimeConversationClosed(event) => Ignore {
            event: EventMsg::RealtimeConversationClosed(event),
            reason: UnsupportedByAcp,
        },
        EventMsg::RealtimeConversationSdp(event) => Ignore {
            event: EventMsg::RealtimeConversationSdp(event),
            reason: UnsupportedByAcp,
        },
        EventMsg::ModelReroute(event) => Ignore {
            event: EventMsg::ModelReroute(event),
            reason: DiagnosticOnly,
        },
        EventMsg::ModelVerification(event) => Ignore {
            event: EventMsg::ModelVerification(event),
            reason: DiagnosticOnly,
        },
        EventMsg::ContextCompacted(_event) => effect_route(session_update::context_compacted()),
        EventMsg::ThreadRolledBack(event) => {
            effect_route(session_update::thread_rolled_back(event))
        }
        EventMsg::ThreadSettingsApplied(event) => Ignore {
            event: EventMsg::ThreadSettingsApplied(event),
            reason: StateOnly,
        },
        EventMsg::TurnStarted(event) => Ignore {
            event: EventMsg::TurnStarted(event),
            reason: StateOnly,
        },
        EventMsg::TurnComplete(event) => Forward(LiveForwardEvent::TurnComplete(event)),
        EventMsg::TokenCount(event) => effect_result_or_ignore(
            EventMsg::TokenCount(event.clone()),
            session_update::usage_effect(event),
        ),
        EventMsg::AgentMessage(event) => Forward(LiveForwardEvent::AgentMessage(event)),
        EventMsg::UserMessage(event) => Ignore {
            event: EventMsg::UserMessage(event),
            reason: AlreadyRenderedByClientInput,
        },
        EventMsg::AgentReasoning(event) => Forward(LiveForwardEvent::AgentReasoning(event)),
        EventMsg::AgentReasoningRawContent(event) => Ignore {
            event: EventMsg::AgentReasoningRawContent(event),
            reason: AlreadyRenderedByAnotherEvent,
        },
        EventMsg::AgentReasoningSectionBreak(event) => {
            Forward(LiveForwardEvent::AgentReasoningSectionBreak(event))
        }
        EventMsg::SessionConfigured(event) => Ignore {
            event: EventMsg::SessionConfigured(event),
            reason: LifecycleOnly,
        },
        EventMsg::ThreadGoalUpdated(event) => {
            effect_route(session_update::thread_goal_updated(&event))
        }
        EventMsg::McpStartupUpdate(event) => Ignore {
            event: EventMsg::McpStartupUpdate(event),
            reason: DiagnosticOnly,
        },
        EventMsg::McpStartupComplete(event) => Ignore {
            event: EventMsg::McpStartupComplete(event),
            reason: DiagnosticOnly,
        },
        EventMsg::McpToolCallBegin(event) => {
            let McpToolCallBeginEvent {
                call_id,
                invocation,
                mcp_app_resource_uri: _,
                plugin_id: _,
            } = event;
            effect_route(tool_call::mcp_tool_call_begin_effect(call_id, &invocation))
        }
        EventMsg::McpToolCallEnd(event) => {
            let McpToolCallEndEvent {
                call_id,
                invocation: _,
                mcp_app_resource_uri: _,
                plugin_id: _,
                duration: _,
                result,
            } = event;
            effect_route(tool_call::mcp_tool_call_end_effect(call_id, result))
        }
        EventMsg::WebSearchBegin(event) => Forward(LiveForwardEvent::WebSearchBegin(event)),
        EventMsg::WebSearchEnd(event) => Forward(LiveForwardEvent::WebSearchEnd(event)),
        EventMsg::ImageGenerationBegin(event) => {
            effect_route(tool_call::image_generation_begin_effect(event))
        }
        EventMsg::ImageGenerationEnd(event) => {
            effect_route(tool_call::image_generation_end_effect(event))
        }
        EventMsg::ExecCommandBegin(event) => {
            Forward(LiveForwardEvent::Exec(LiveExecEvent::CommandBegin(event)))
        }
        EventMsg::ExecCommandOutputDelta(event) => {
            Forward(LiveForwardEvent::Exec(LiveExecEvent::OutputDelta(event)))
        }
        EventMsg::TerminalInteraction(event) => Forward(LiveForwardEvent::Exec(
            LiveExecEvent::TerminalInteraction(event),
        )),
        EventMsg::ExecCommandEnd(event) => {
            Forward(LiveForwardEvent::Exec(LiveExecEvent::CommandEnd(event)))
        }
        EventMsg::ViewImageToolCall(event) => effect_route(tool_call::view_image_effect(event)),
        EventMsg::ExecApprovalRequest(event) => {
            RequestPermission(LivePermissionEvent::ExecApprovalRequest(event))
        }
        EventMsg::RequestPermissions(event) => {
            RequestPermission(LivePermissionEvent::RequestPermissions(event))
        }
        EventMsg::RequestUserInput(event) => Ignore {
            event: EventMsg::RequestUserInput(event),
            reason: HandledByActor,
        },
        EventMsg::DynamicToolCallRequest(event) => {
            let DynamicToolCallRequest {
                call_id,
                turn_id: _,
                started_at_ms: _,
                namespace: _,
                tool,
                arguments,
            } = event;
            effect_route(tool_call::dynamic_tool_call_begin_effect(
                call_id, &tool, &arguments,
            ))
        }
        EventMsg::DynamicToolCallResponse(event) => {
            effect_route(tool_call::dynamic_tool_call_end_effect(event))
        }
        EventMsg::ElicitationRequest(event) => {
            RequestPermission(LivePermissionEvent::ElicitationRequest(event))
        }
        EventMsg::ApplyPatchApprovalRequest(event) => {
            RequestPermission(LivePermissionEvent::ApplyPatchApprovalRequest(event))
        }
        EventMsg::GuardianAssessment(event) => Forward(LiveForwardEvent::GuardianAssessment(event)),
        EventMsg::DeprecationNotice(event) => Ignore {
            event: EventMsg::DeprecationNotice(event),
            reason: DiagnosticOnly,
        },
        EventMsg::StreamError(event) => Ignore {
            event: EventMsg::StreamError(event),
            reason: DiagnosticOnly,
        },
        EventMsg::PatchApplyBegin(event) => {
            effect_route(tool_call::patch_apply_begin_effect(event))
        }
        EventMsg::PatchApplyUpdated(event) if event.changes.is_empty() => Ignore {
            event: EventMsg::PatchApplyUpdated(event),
            reason: EmptyToolCallUpdate,
        },
        EventMsg::PatchApplyUpdated(event) => {
            let effect = tool_call::patch_apply_updated_effect(event)
                .unwrap_or(BridgeEffect::Ignore(EmptyToolCallUpdate));
            effect_route(effect)
        }
        EventMsg::PatchApplyEnd(event) => effect_route(tool_call::patch_apply_end_effect(event)),
        EventMsg::TurnDiff(event) => Ignore {
            event: EventMsg::TurnDiff(event),
            reason: HandledByResponseItem,
        },
        EventMsg::RealtimeConversationListVoicesResponse(event) => Ignore {
            event: EventMsg::RealtimeConversationListVoicesResponse(event),
            reason: UnsupportedByAcp,
        },
        EventMsg::PlanUpdate(event) => effect_route(session_update::plan_effect(event.plan)),
        EventMsg::TurnAborted(
            event @ TurnAbortedEvent {
                turn_id: Some(_), ..
            },
        ) => Forward(LiveForwardEvent::TurnAborted(event)),
        EventMsg::TurnAborted(event @ TurnAbortedEvent { turn_id: None, .. }) => Ignore {
            event: EventMsg::TurnAborted(event),
            reason: LifecycleOnly,
        },
        EventMsg::ShutdownComplete => Forward(LiveForwardEvent::ShutdownComplete),
        EventMsg::EnteredReviewMode(event) => Ignore {
            event: EventMsg::EnteredReviewMode(event),
            reason: StateOnly,
        },
        EventMsg::ExitedReviewMode(event) => effect_result_or_ignore(
            EventMsg::ExitedReviewMode(event.clone()),
            session_update::review_mode_exit_effect(event),
        ),
        EventMsg::RawResponseItem(event) => Ignore {
            event: EventMsg::RawResponseItem(event),
            reason: HandledByResponseItem,
        },
        EventMsg::ItemStarted(event) => Ignore {
            event: EventMsg::ItemStarted(event),
            reason: LifecycleOnly,
        },
        EventMsg::ItemCompleted(event) => Ignore {
            event: EventMsg::ItemCompleted(event),
            reason: LifecycleOnly,
        },
        EventMsg::HookStarted(event) => Ignore {
            event: EventMsg::HookStarted(event),
            reason: UnsupportedByAcp,
        },
        EventMsg::HookCompleted(event) => Ignore {
            event: EventMsg::HookCompleted(event),
            reason: UnsupportedByAcp,
        },
        EventMsg::AgentMessageContentDelta(event) => {
            Forward(LiveForwardEvent::AgentMessageContentDelta(event))
        }
        EventMsg::PlanDelta(event) => Ignore {
            event: EventMsg::PlanDelta(event),
            reason: SupersededBySnapshot,
        },
        EventMsg::ReasoningContentDelta(ReasoningContentDeltaEvent {
            thread_id,
            turn_id,
            item_id,
            delta,
            summary_index,
        }) => Forward(LiveForwardEvent::ReasoningContentDelta(
            LiveReasoningContentDeltaEvent {
                thread_id,
                turn_id,
                item_id,
                index: summary_index,
                delta,
            },
        )),
        EventMsg::ReasoningRawContentDelta(ReasoningRawContentDeltaEvent {
            thread_id,
            turn_id,
            item_id,
            delta,
            content_index,
        }) => Forward(LiveForwardEvent::ReasoningContentDelta(
            LiveReasoningContentDeltaEvent {
                thread_id,
                turn_id,
                item_id,
                index: content_index,
                delta,
            },
        )),
        EventMsg::CollabAgentSpawnBegin(event) => {
            effect_route(tool_call::collab_spawn_begin_effect(&event))
        }
        EventMsg::CollabAgentSpawnEnd(event) => {
            effect_route(tool_call::collab_spawn_end_effect(&event))
        }
        EventMsg::CollabAgentInteractionBegin(event) => {
            effect_route(tool_call::collab_interaction_begin_effect(&event))
        }
        EventMsg::CollabAgentInteractionEnd(event) => {
            effect_route(tool_call::collab_interaction_end_effect(&event))
        }
        EventMsg::CollabWaitingBegin(event) => {
            effect_route(tool_call::collab_waiting_begin_effect(&event))
        }
        EventMsg::CollabWaitingEnd(event) => {
            effect_route(tool_call::collab_waiting_end_effect(&event))
        }
        EventMsg::CollabCloseBegin(event) => {
            effect_route(tool_call::collab_close_begin_effect(&event))
        }
        EventMsg::CollabCloseEnd(event) => effect_route(tool_call::collab_close_end_effect(&event)),
        EventMsg::CollabResumeBegin(event) => {
            effect_route(tool_call::collab_resume_begin_effect(&event))
        }
        EventMsg::CollabResumeEnd(event) => {
            effect_route(tool_call::collab_resume_end_effect(&event))
        }
    }
}

fn effect_route(effect: BridgeEffect) -> LiveEventRoute {
    LiveEventRoute::Forward(LiveForwardEvent::Effect(Box::new(effect)))
}

fn effect_result_or_ignore(
    event: EventMsg,
    result: Result<BridgeEffect, IgnoredCodexEventReason>,
) -> LiveEventRoute {
    match result {
        Ok(effect) => effect_route(effect),
        Err(reason) => LiveEventRoute::Ignore { event, reason },
    }
}

pub(crate) fn completes_active_web_search_before(event: &EventMsg) -> bool {
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
