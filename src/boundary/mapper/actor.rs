use codex_protocol::protocol::*;

use crate::boundary::effect::BridgeEventContext;

use super::{classify_event_msg, types::*};

pub(crate) fn plan_actor_event(event: &EventMsg) -> ActorEventPlan {
    ActorEventPlan {
        state_updates: actor_state_updates(event),
        action: route_actor_event(event),
    }
}

fn route_actor_event(msg: &EventMsg) -> ActorEventAction {
    use ActorPendingUserInputClear::{None as NoClear, Submission};

    match msg {
        EventMsg::RequestUserInput(event) => {
            ActorEventAction::RegisterPendingUserInput(event.clone())
        }

        EventMsg::ExecApprovalRequest(event) => actor_route_to_submission(
            msg,
            NoClear,
            Some(ActorAutoApproval::Exec {
                id: event
                    .approval_id
                    .clone()
                    .unwrap_or_else(|| event.call_id.clone()),
                turn_id: event.turn_id.clone(),
            }),
        ),
        EventMsg::ApplyPatchApprovalRequest(event) => actor_route_to_submission(
            msg,
            NoClear,
            Some(ActorAutoApproval::Patch {
                id: event.call_id.clone(),
            }),
        ),

        EventMsg::Error(..)
        | EventMsg::TurnComplete(..)
        | EventMsg::TurnAborted(..)
        | EventMsg::ShutdownComplete => actor_route_to_submission(msg, Submission, None),

        EventMsg::Warning(..)
        | EventMsg::GuardianWarning(..)
        | EventMsg::RealtimeConversationStarted(..)
        | EventMsg::RealtimeConversationRealtime(..)
        | EventMsg::RealtimeConversationClosed(..)
        | EventMsg::RealtimeConversationSdp(..)
        | EventMsg::ModelReroute(..)
        | EventMsg::ModelVerification(..)
        | EventMsg::TurnModerationMetadata(..)
        | EventMsg::SafetyBuffering(..)
        | EventMsg::ContextCompacted(..)
        | EventMsg::ThreadRolledBack(..)
        | EventMsg::ThreadSettingsApplied(..)
        | EventMsg::TurnStarted(..)
        | EventMsg::TokenCount(..)
        | EventMsg::AgentMessage(..)
        | EventMsg::UserMessage(..)
        | EventMsg::AgentReasoning(..)
        | EventMsg::AgentReasoningRawContent(..)
        | EventMsg::AgentReasoningSectionBreak(..)
        | EventMsg::SessionConfigured(..)
        | EventMsg::ThreadGoalUpdated(..)
        | EventMsg::McpStartupUpdate(..)
        | EventMsg::McpStartupComplete(..)
        | EventMsg::McpToolCallBegin(..)
        | EventMsg::McpToolCallEnd(..)
        | EventMsg::WebSearchBegin(..)
        | EventMsg::WebSearchEnd(..)
        | EventMsg::ImageGenerationBegin(..)
        | EventMsg::ImageGenerationEnd(..)
        | EventMsg::ExecCommandBegin(..)
        | EventMsg::ExecCommandOutputDelta(..)
        | EventMsg::TerminalInteraction(..)
        | EventMsg::ExecCommandEnd(..)
        | EventMsg::ViewImageToolCall(..)
        | EventMsg::RequestPermissions(..)
        | EventMsg::DynamicToolCallRequest(..)
        | EventMsg::DynamicToolCallResponse(..)
        | EventMsg::ElicitationRequest(..)
        | EventMsg::GuardianAssessment(..)
        | EventMsg::DeprecationNotice(..)
        | EventMsg::StreamError(..)
        | EventMsg::PatchApplyBegin(..)
        | EventMsg::PatchApplyUpdated(..)
        | EventMsg::PatchApplyEnd(..)
        | EventMsg::TurnDiff(..)
        | EventMsg::RealtimeConversationListVoicesResponse(..)
        | EventMsg::PlanUpdate(..)
        | EventMsg::EnteredReviewMode(..)
        | EventMsg::ExitedReviewMode(..)
        | EventMsg::RawResponseItem(..)
        | EventMsg::ItemStarted(..)
        | EventMsg::ItemCompleted(..)
        | EventMsg::HookStarted(..)
        | EventMsg::HookCompleted(..)
        | EventMsg::AgentMessageContentDelta(..)
        | EventMsg::PlanDelta(..)
        | EventMsg::ReasoningContentDelta(..)
        | EventMsg::ReasoningRawContentDelta(..)
        | EventMsg::CollabAgentSpawnBegin(..)
        | EventMsg::CollabAgentSpawnEnd(..)
        | EventMsg::CollabAgentInteractionBegin(..)
        | EventMsg::CollabAgentInteractionEnd(..)
        | EventMsg::CollabWaitingBegin(..)
        | EventMsg::CollabWaitingEnd(..)
        | EventMsg::CollabCloseBegin(..)
        | EventMsg::CollabCloseEnd(..)
        | EventMsg::CollabResumeBegin(..)
        | EventMsg::CollabResumeEnd(..)
        | EventMsg::SubAgentActivity(..) => actor_route_to_submission(msg, NoClear, None),
    }
}

fn actor_route_to_submission(
    event: &EventMsg,
    clear_pending_user_input: ActorPendingUserInputClear,
    full_access_auto_approval: Option<ActorAutoApproval>,
) -> ActorEventAction {
    ActorEventAction::RouteToSubmission {
        bridge_effect: classify_event_msg(event, BridgeEventContext::Live),
        clear_pending_user_input,
        full_access_auto_approval,
    }
}

pub(crate) fn actor_state_updates(event: &EventMsg) -> Vec<ActorStateUpdate> {
    match event {
        EventMsg::TokenCount(event) => vec![ActorStateUpdate::LatestUsage {
            info: event.info.clone().map(Box::new),
            rate_limits: event.rate_limits.clone().map(Box::new),
        }],
        EventMsg::TurnStarted(event) => vec![ActorStateUpdate::CollaborationMode(
            event.collaboration_mode_kind,
        )],
        EventMsg::CollabAgentSpawnEnd(event) => event
            .new_thread_id
            .as_ref()
            .map(|thread_id| {
                vec![ActorStateUpdate::RememberCollabAgent(
                    ActorCollabAgentUpdate {
                        thread_id: *thread_id,
                        agent_nickname: event.new_agent_nickname.clone(),
                        agent_role: event.new_agent_role.clone(),
                        status: event.status.clone(),
                    },
                )]
            })
            .unwrap_or_default(),
        EventMsg::CollabAgentInteractionEnd(event) => {
            vec![ActorStateUpdate::RememberCollabAgent(
                ActorCollabAgentUpdate {
                    thread_id: event.receiver_thread_id,
                    agent_nickname: event.receiver_agent_nickname.clone(),
                    agent_role: event.receiver_agent_role.clone(),
                    status: event.status.clone(),
                },
            )]
        }
        EventMsg::CollabWaitingEnd(event) => {
            if !event.agent_statuses.is_empty() {
                vec![ActorStateUpdate::RememberCollabAgentEntries(
                    event.agent_statuses.clone(),
                )]
            } else {
                event
                    .statuses
                    .iter()
                    .map(|(thread_id, status)| {
                        ActorStateUpdate::RememberCollabAgent(ActorCollabAgentUpdate {
                            thread_id: *thread_id,
                            agent_nickname: None,
                            agent_role: None,
                            status: status.clone(),
                        })
                    })
                    .collect()
            }
        }
        EventMsg::CollabResumeEnd(event) => {
            vec![ActorStateUpdate::RememberCollabAgent(
                ActorCollabAgentUpdate {
                    thread_id: event.receiver_thread_id,
                    agent_nickname: event.receiver_agent_nickname.clone(),
                    agent_role: event.receiver_agent_role.clone(),
                    status: event.status.clone(),
                },
            )]
        }
        EventMsg::CollabCloseEnd(event) => {
            vec![ActorStateUpdate::RemoveCollabAgent(
                event.receiver_thread_id,
            )]
        }

        EventMsg::Error(..)
        | EventMsg::Warning(..)
        | EventMsg::GuardianWarning(..)
        | EventMsg::RealtimeConversationStarted(..)
        | EventMsg::RealtimeConversationRealtime(..)
        | EventMsg::RealtimeConversationClosed(..)
        | EventMsg::RealtimeConversationSdp(..)
        | EventMsg::ModelReroute(..)
        | EventMsg::ModelVerification(..)
        | EventMsg::TurnModerationMetadata(..)
        | EventMsg::SafetyBuffering(..)
        | EventMsg::ContextCompacted(..)
        | EventMsg::ThreadRolledBack(..)
        | EventMsg::ThreadSettingsApplied(..)
        | EventMsg::TurnComplete(..)
        | EventMsg::UserMessage(..)
        | EventMsg::AgentMessage(..)
        | EventMsg::AgentReasoning(..)
        | EventMsg::AgentReasoningRawContent(..)
        | EventMsg::AgentReasoningSectionBreak(..)
        | EventMsg::SessionConfigured(..)
        | EventMsg::ThreadGoalUpdated(..)
        | EventMsg::McpStartupUpdate(..)
        | EventMsg::McpStartupComplete(..)
        | EventMsg::McpToolCallBegin(..)
        | EventMsg::McpToolCallEnd(..)
        | EventMsg::WebSearchBegin(..)
        | EventMsg::WebSearchEnd(..)
        | EventMsg::ImageGenerationBegin(..)
        | EventMsg::ImageGenerationEnd(..)
        | EventMsg::ExecCommandBegin(..)
        | EventMsg::ExecCommandOutputDelta(..)
        | EventMsg::TerminalInteraction(..)
        | EventMsg::ExecCommandEnd(..)
        | EventMsg::ViewImageToolCall(..)
        | EventMsg::ExecApprovalRequest(..)
        | EventMsg::RequestPermissions(..)
        | EventMsg::RequestUserInput(..)
        | EventMsg::DynamicToolCallRequest(..)
        | EventMsg::DynamicToolCallResponse(..)
        | EventMsg::ElicitationRequest(..)
        | EventMsg::ApplyPatchApprovalRequest(..)
        | EventMsg::GuardianAssessment(..)
        | EventMsg::DeprecationNotice(..)
        | EventMsg::StreamError(..)
        | EventMsg::PatchApplyBegin(..)
        | EventMsg::PatchApplyUpdated(..)
        | EventMsg::PatchApplyEnd(..)
        | EventMsg::TurnDiff(..)
        | EventMsg::RealtimeConversationListVoicesResponse(..)
        | EventMsg::PlanUpdate(..)
        | EventMsg::TurnAborted(..)
        | EventMsg::ShutdownComplete
        | EventMsg::EnteredReviewMode(..)
        | EventMsg::ExitedReviewMode(..)
        | EventMsg::RawResponseItem(..)
        | EventMsg::ItemStarted(..)
        | EventMsg::ItemCompleted(..)
        | EventMsg::HookStarted(..)
        | EventMsg::HookCompleted(..)
        | EventMsg::AgentMessageContentDelta(..)
        | EventMsg::PlanDelta(..)
        | EventMsg::ReasoningContentDelta(..)
        | EventMsg::ReasoningRawContentDelta(..)
        | EventMsg::CollabAgentSpawnBegin(..)
        | EventMsg::CollabAgentInteractionBegin(..)
        | EventMsg::CollabWaitingBegin(..)
        | EventMsg::CollabCloseBegin(..)
        | EventMsg::CollabResumeBegin(..)
        | EventMsg::SubAgentActivity(..) => Vec::new(),
    }
}
