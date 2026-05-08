use codex_protocol::{models::*, protocol::*};

use crate::boundary::{
    effect::{BridgeEffect, IgnoredCodexEventReason},
    session_update, tool_call,
};

use super::{actor_state_updates, types::*};

pub(crate) fn route_replay_rollout_item(item: &RolloutItem) -> ReplayRolloutItemRoute<'_> {
    match item {
        RolloutItem::EventMsg(event) => {
            ReplayRolloutItemRoute::Event(route_replay_event_msg(event))
        }
        RolloutItem::ResponseItem(item) => {
            ReplayRolloutItemRoute::ResponseItem(route_replay_response_item(item))
        }
        RolloutItem::SessionMeta(..)
        | RolloutItem::Compacted(..)
        | RolloutItem::TurnContext(..) => ReplayRolloutItemRoute::Ignore {
            item,
            reason: IgnoredCodexEventReason::StateOnly,
        },
    }
}

pub(crate) fn route_replay_event_msg(event: &EventMsg) -> ReplayEventPlan<'_> {
    ReplayEventPlan {
        state_updates: actor_state_updates(event),
        action: route_replay_event_action(event),
    }
}

fn route_replay_event_action(event: &EventMsg) -> ReplayEventAction<'_> {
    use IgnoredCodexEventReason::{
        AlreadyRenderedByAnotherEvent, DiagnosticOnly, HandledByResponseItem, LifecycleOnly,
        StateOnly, SupersededBySnapshot, UnsupportedByAcp,
    };

    match event {
        EventMsg::UserMessage(event) => {
            effect_action(session_update::user_message_effect(event.message.clone()))
        }
        EventMsg::AgentMessage(event) => {
            effect_action(session_update::agent_text_effect(event.message.clone()))
        }
        EventMsg::AgentReasoning(event) => {
            effect_action(session_update::agent_thought_effect(event.text.clone()))
        }
        EventMsg::AgentReasoningRawContent(event) => {
            effect_action(session_update::agent_thought_effect(event.text.clone()))
        }
        EventMsg::ThreadGoalUpdated(event) => {
            effect_action(session_update::thread_goal_updated(event))
        }
        EventMsg::PlanUpdate(event) => {
            effect_action(session_update::plan_effect(event.plan.clone()))
        }
        EventMsg::ImageGenerationEnd(event) => {
            effect_action(tool_call::image_generation_replay_effect(event))
        }
        EventMsg::CollabAgentSpawnEnd(event) => {
            effect_action(tool_call::collab_spawn_replay_effect(event))
        }
        EventMsg::CollabAgentInteractionEnd(event) => {
            effect_action(tool_call::collab_interaction_replay_effect(event))
        }
        EventMsg::CollabWaitingEnd(event) => {
            effect_action(tool_call::collab_waiting_replay_effect(event))
        }
        EventMsg::CollabCloseEnd(event) => {
            effect_action(tool_call::collab_close_replay_effect(event))
        }
        EventMsg::CollabResumeEnd(event) => {
            effect_action(tool_call::collab_resume_replay_effect(event))
        }
        EventMsg::SkillsUpdateAvailable => effect_action(session_update::skills_update_available()),
        EventMsg::RequestUserInput(event) => ReplayEventAction::RegisterPendingUserInput {
            turn_id: &event.turn_id,
            event,
        },
        EventMsg::TurnComplete(event) => {
            ReplayEventAction::ClearPendingUserInputForSubmission(&event.turn_id)
        }
        EventMsg::TurnAborted(TurnAbortedEvent {
            turn_id: Some(turn_id),
            ..
        }) => ReplayEventAction::ClearPendingUserInputForSubmission(turn_id),
        EventMsg::ShutdownComplete | EventMsg::Error(..) => {
            ReplayEventAction::ClearPendingUserInput
        }

        EventMsg::Warning(..)
        | EventMsg::GuardianWarning(..)
        | EventMsg::McpStartupUpdate(..)
        | EventMsg::McpStartupComplete(..)
        | EventMsg::ModelReroute(..)
        | EventMsg::ModelVerification(..)
        | EventMsg::StreamError(..)
        | EventMsg::DeprecationNotice(..) => ReplayEventAction::Ignore {
            event,
            reason: DiagnosticOnly,
        },
        EventMsg::RealtimeConversationStarted(..)
        | EventMsg::RealtimeConversationRealtime(..)
        | EventMsg::RealtimeConversationClosed(..)
        | EventMsg::RealtimeConversationSdp(..)
        | EventMsg::RealtimeConversationListVoicesResponse(..)
        | EventMsg::HookStarted(..)
        | EventMsg::HookCompleted(..) => ReplayEventAction::Ignore {
            event,
            reason: UnsupportedByAcp,
        },
        EventMsg::TurnDiff(..) | EventMsg::RawResponseItem(..) => ReplayEventAction::Ignore {
            event,
            reason: HandledByResponseItem,
        },
        EventMsg::PlanDelta(..) => ReplayEventAction::Ignore {
            event,
            reason: SupersededBySnapshot,
        },
        EventMsg::AgentMessageContentDelta(..)
        | EventMsg::ReasoningContentDelta(..)
        | EventMsg::ReasoningRawContentDelta(..)
        | EventMsg::AgentReasoningSectionBreak(..)
        | EventMsg::ImageGenerationBegin(..)
        | EventMsg::CollabAgentSpawnBegin(..)
        | EventMsg::CollabAgentInteractionBegin(..)
        | EventMsg::CollabWaitingBegin(..)
        | EventMsg::CollabCloseBegin(..)
        | EventMsg::CollabResumeBegin(..) => ReplayEventAction::Ignore {
            event,
            reason: AlreadyRenderedByAnotherEvent,
        },
        EventMsg::TurnStarted(..)
        | EventMsg::SessionConfigured(..)
        | EventMsg::EnteredReviewMode(..)
        | EventMsg::ContextCompacted(..)
        | EventMsg::TokenCount(..) => ReplayEventAction::Ignore {
            event,
            reason: StateOnly,
        },
        EventMsg::TurnAborted(TurnAbortedEvent { turn_id: None, .. })
        | EventMsg::ItemStarted(..)
        | EventMsg::ItemCompleted(..) => ReplayEventAction::Ignore {
            event,
            reason: LifecycleOnly,
        },
        EventMsg::ThreadRolledBack(..)
        | EventMsg::WebSearchBegin(..)
        | EventMsg::WebSearchEnd(..)
        | EventMsg::McpToolCallBegin(..)
        | EventMsg::McpToolCallEnd(..)
        | EventMsg::ExecCommandBegin(..)
        | EventMsg::ExecCommandOutputDelta(..)
        | EventMsg::TerminalInteraction(..)
        | EventMsg::ExecCommandEnd(..)
        | EventMsg::DynamicToolCallRequest(..)
        | EventMsg::DynamicToolCallResponse(..)
        | EventMsg::PatchApplyBegin(..)
        | EventMsg::PatchApplyUpdated(..)
        | EventMsg::PatchApplyEnd(..)
        | EventMsg::ViewImageToolCall(..)
        | EventMsg::ExitedReviewMode(..)
        | EventMsg::GuardianAssessment(..)
        | EventMsg::ExecApprovalRequest(..)
        | EventMsg::RequestPermissions(..)
        | EventMsg::ElicitationRequest(..)
        | EventMsg::ApplyPatchApprovalRequest(..) => ReplayEventAction::Ignore {
            event,
            reason: HandledByResponseItem,
        },
    }
}

fn effect_action<'a>(effect: BridgeEffect) -> ReplayEventAction<'a> {
    ReplayEventAction::Effect(Box::new(effect))
}

pub(crate) fn route_replay_response_item(item: &ResponseItem) -> ReplayResponseItemRoute<'_> {
    use IgnoredCodexEventReason::{
        AlreadyRenderedByAnotherEvent, MissingToolCallId, StateOnly, UnsupportedByAcp,
    };

    match item {
        ResponseItem::FunctionCall {
            name,
            arguments,
            call_id,
            ..
        } if is_replay_shell_tool(name) => ReplayResponseItemRoute::ShellFunctionCall {
            call_id,
            name,
            arguments,
        },
        ResponseItem::FunctionCall {
            name,
            arguments,
            call_id,
            ..
        } => ReplayResponseItemRoute::GenericFunctionCall {
            call_id,
            name,
            arguments,
        },
        ResponseItem::FunctionCallOutput { call_id, output } => {
            ReplayResponseItemRoute::FunctionCallOutput { call_id, output }
        }
        ResponseItem::LocalShellCall {
            call_id: Some(call_id),
            action,
            status,
            ..
        } => {
            let LocalShellAction::Exec(exec) = action;
            ReplayResponseItemRoute::LocalShellCall {
                call_id,
                command: &exec.command,
                working_directory: exec.working_directory.as_deref(),
                status: match status {
                    LocalShellStatus::Completed => ReplayToolCallStatus::Completed,
                    LocalShellStatus::InProgress | LocalShellStatus::Incomplete => {
                        ReplayToolCallStatus::Failed
                    }
                },
            }
        }
        ResponseItem::LocalShellCall { call_id: None, .. } => ReplayResponseItemRoute::Ignore {
            item,
            reason: MissingToolCallId,
        },
        ResponseItem::CustomToolCall {
            name,
            input,
            call_id,
            ..
        } if name == "apply_patch" => {
            ReplayResponseItemRoute::ApplyPatchCustomToolCall { call_id, input }
        }
        ResponseItem::CustomToolCall {
            name,
            input,
            call_id,
            ..
        } => ReplayResponseItemRoute::GenericCustomToolCall {
            call_id,
            name,
            input,
        },
        ResponseItem::CustomToolCallOutput {
            name: _,
            call_id,
            output,
        } => ReplayResponseItemRoute::CustomToolCallOutput { call_id, output },
        ResponseItem::WebSearchCall { id, action, .. } => ReplayResponseItemRoute::WebSearchCall {
            id: id.as_deref(),
            action: action.as_ref(),
        },
        ResponseItem::Message { .. }
        | ResponseItem::Reasoning { .. }
        | ResponseItem::ImageGenerationCall { .. } => ReplayResponseItemRoute::Ignore {
            item,
            reason: AlreadyRenderedByAnotherEvent,
        },
        ResponseItem::Compaction { .. } | ResponseItem::ContextCompaction { .. } => {
            ReplayResponseItemRoute::Ignore {
                item,
                reason: StateOnly,
            }
        }
        ResponseItem::ToolSearchCall { .. }
        | ResponseItem::ToolSearchOutput { .. }
        | ResponseItem::Other => ReplayResponseItemRoute::Ignore {
            item,
            reason: UnsupportedByAcp,
        },
    }
}

fn is_replay_shell_tool(name: &str) -> bool {
    matches!(
        name,
        "shell" | "container.exec" | "shell_command" | "exec_command"
    )
}
