#[cfg(test)]
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::*;

use crate::boundary::effect::{BridgeEffectKind, BridgeEventContext, IgnoredCodexEventReason};

pub(crate) fn classify_event_msg(
    event: &EventMsg,
    context: BridgeEventContext,
) -> BridgeEffectKind {
    use BridgeEffectKind::{Forward, Ignore, RequestPermission};
    use IgnoredCodexEventReason::{
        AlreadyRenderedByAnotherEvent, AlreadyRenderedByClientInput, DiagnosticOnly,
        HandledByActor, HandledByResponseItem, LifecycleOnly, StateOnly, SupersededBySnapshot,
        UnsupportedByAcp,
    };

    match event {
        EventMsg::Error(..)
        | EventMsg::Warning(..)
        | EventMsg::GuardianWarning(..)
        | EventMsg::TokenCount(..)
        | EventMsg::AgentMessage(..)
        | EventMsg::AgentMessageContentDelta(..)
        | EventMsg::AgentReasoning(..)
        | EventMsg::AgentReasoningSectionBreak(..)
        | EventMsg::ReasoningContentDelta(..)
        | EventMsg::ReasoningRawContentDelta(..)
        | EventMsg::ThreadGoalUpdated(..)
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
        | EventMsg::ContextCompacted(..)
        | EventMsg::ThreadRolledBack(..)
        | EventMsg::TurnComplete(..)
        | EventMsg::TurnAborted(TurnAbortedEvent {
            turn_id: Some(_), ..
        })
        | EventMsg::ShutdownComplete
        | EventMsg::ExitedReviewMode(..)
        | EventMsg::PlanUpdate(..)
        | EventMsg::GuardianAssessment(..)
        | EventMsg::ImageGenerationBegin(..)
        | EventMsg::ImageGenerationEnd(..)
        | EventMsg::CollabAgentSpawnBegin(..)
        | EventMsg::CollabAgentSpawnEnd(..)
        | EventMsg::CollabAgentInteractionBegin(..)
        | EventMsg::CollabAgentInteractionEnd(..)
        | EventMsg::CollabWaitingBegin(..)
        | EventMsg::CollabWaitingEnd(..)
        | EventMsg::CollabCloseBegin(..)
        | EventMsg::CollabCloseEnd(..)
        | EventMsg::CollabResumeBegin(..)
        | EventMsg::CollabResumeEnd(..) => Forward,

        EventMsg::ExecApprovalRequest(..)
        | EventMsg::RequestPermissions(..)
        | EventMsg::ElicitationRequest(..)
        | EventMsg::ApplyPatchApprovalRequest(..) => RequestPermission,

        EventMsg::RequestUserInput(..) => match context {
            BridgeEventContext::Live => Ignore(HandledByActor),
            BridgeEventContext::Replay => Forward,
        },

        EventMsg::UserMessage(..) => match context {
            BridgeEventContext::Live => Ignore(AlreadyRenderedByClientInput),
            BridgeEventContext::Replay => Forward,
        },
        EventMsg::AgentReasoningRawContent(..) => match context {
            BridgeEventContext::Live => Ignore(AlreadyRenderedByAnotherEvent),
            BridgeEventContext::Replay => Forward,
        },

        EventMsg::TurnStarted(..) | EventMsg::ThreadSettingsApplied(..) => Ignore(StateOnly),
        EventMsg::TurnAborted(TurnAbortedEvent { turn_id: None, .. }) => Ignore(LifecycleOnly),
        EventMsg::ItemStarted(..) | EventMsg::ItemCompleted(..) => Ignore(LifecycleOnly),
        EventMsg::SessionConfigured(..) => Ignore(LifecycleOnly),
        EventMsg::McpStartupUpdate(..)
        | EventMsg::McpStartupComplete(..)
        | EventMsg::ModelReroute(..)
        | EventMsg::ModelVerification(..)
        | EventMsg::StreamError(..)
        | EventMsg::DeprecationNotice(..) => Ignore(DiagnosticOnly),
        EventMsg::EnteredReviewMode(..) => Ignore(StateOnly),
        EventMsg::TurnDiff(..) | EventMsg::RawResponseItem(..) => Ignore(HandledByResponseItem),
        EventMsg::PlanDelta(..) => Ignore(SupersededBySnapshot),
        EventMsg::HookStarted(..) | EventMsg::HookCompleted(..) => Ignore(UnsupportedByAcp),
        EventMsg::RealtimeConversationStarted(..)
        | EventMsg::RealtimeConversationRealtime(..)
        | EventMsg::RealtimeConversationClosed(..)
        | EventMsg::RealtimeConversationSdp(..)
        | EventMsg::RealtimeConversationListVoicesResponse(..) => Ignore(UnsupportedByAcp),
    }
}

#[cfg(test)]
pub(crate) fn classify_response_item(item: &ResponseItem) -> BridgeEffectKind {
    use BridgeEffectKind::{Forward, Ignore};
    use IgnoredCodexEventReason::{
        AlreadyRenderedByAnotherEvent, MissingToolCallId, StateOnly, UnsupportedByAcp,
    };

    match item {
        ResponseItem::FunctionCall { .. }
        | ResponseItem::FunctionCallOutput { .. }
        | ResponseItem::CustomToolCall { .. }
        | ResponseItem::CustomToolCallOutput { .. }
        | ResponseItem::WebSearchCall { .. } => Forward,
        ResponseItem::LocalShellCall {
            call_id: Some(_), ..
        } => Forward,

        ResponseItem::Message { .. } | ResponseItem::Reasoning { .. } => {
            Ignore(AlreadyRenderedByAnotherEvent)
        }
        ResponseItem::LocalShellCall { call_id: None, .. } => Ignore(MissingToolCallId),
        ResponseItem::ImageGenerationCall { .. } => Ignore(AlreadyRenderedByAnotherEvent),
        ResponseItem::Compaction { .. }
        | ResponseItem::ContextCompaction { .. }
        | ResponseItem::CompactionTrigger => Ignore(StateOnly),
        ResponseItem::ToolSearchCall { .. }
        | ResponseItem::ToolSearchOutput { .. }
        | ResponseItem::Other => Ignore(UnsupportedByAcp),
    }
}

#[cfg(test)]
pub(crate) fn classify_rollout_item(item: &RolloutItem) -> BridgeEffectKind {
    match item {
        RolloutItem::EventMsg(event) => classify_event_msg(event, BridgeEventContext::Replay),
        RolloutItem::ResponseItem(item) => classify_response_item(item),
        RolloutItem::SessionMeta(..)
        | RolloutItem::Compacted(..)
        | RolloutItem::TurnContext(..) => {
            BridgeEffectKind::Ignore(IgnoredCodexEventReason::StateOnly)
        }
    }
}
