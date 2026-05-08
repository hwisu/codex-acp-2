use codex_protocol::{
    ThreadId,
    approvals::ElicitationRequestEvent,
    config_types::ModeKind,
    models::{FunctionCallOutputPayload, ResponseItem},
    protocol::*,
    request_permissions::RequestPermissionsEvent,
    request_user_input::RequestUserInputEvent,
};

use crate::boundary::effect::{BridgeEffect, BridgeEffectKind, IgnoredCodexEventReason};

pub(crate) enum LiveEventRoute {
    Forward(LiveForwardEvent),
    RequestPermission(LivePermissionEvent),
    Ignore {
        event: EventMsg,
        reason: IgnoredCodexEventReason,
    },
}

pub(crate) enum LiveForwardEvent {
    Effect(Box<BridgeEffect>),
    AgentMessageContentDelta(AgentMessageContentDeltaEvent),
    ReasoningContentDelta(LiveReasoningContentDeltaEvent),
    AgentReasoningSectionBreak(AgentReasoningSectionBreakEvent),
    AgentMessage(AgentMessageEvent),
    AgentReasoning(AgentReasoningEvent),
    WebSearchBegin(WebSearchBeginEvent),
    WebSearchEnd(WebSearchEndEvent),
    Exec(LiveExecEvent),
    TurnComplete(TurnCompleteEvent),
    Error(ErrorEvent),
    TurnAborted(TurnAbortedEvent),
    ShutdownComplete,
    GuardianAssessment(GuardianAssessmentEvent),
}

pub(crate) enum LivePermissionEvent {
    ExecApprovalRequest(ExecApprovalRequestEvent),
    RequestPermissions(RequestPermissionsEvent),
    ElicitationRequest(ElicitationRequestEvent),
    ApplyPatchApprovalRequest(ApplyPatchApprovalRequestEvent),
}

pub(crate) enum LiveExecEvent {
    CommandBegin(ExecCommandBeginEvent),
    OutputDelta(ExecCommandOutputDeltaEvent),
    CommandEnd(ExecCommandEndEvent),
    TerminalInteraction(TerminalInteractionEvent),
}

pub(crate) struct LiveReasoningContentDeltaEvent {
    pub(crate) thread_id: String,
    pub(crate) turn_id: String,
    pub(crate) item_id: String,
    pub(crate) index: i64,
    pub(crate) delta: String,
}

pub(crate) struct ActorEventPlan {
    pub(crate) state_updates: Vec<ActorStateUpdate>,
    pub(crate) action: ActorEventAction,
}

pub(crate) enum ActorEventAction {
    RegisterPendingUserInput(RequestUserInputEvent),
    RouteToSubmission {
        bridge_effect: BridgeEffectKind,
        clear_pending_user_input: ActorPendingUserInputClear,
        full_access_auto_approval: Option<ActorAutoApproval>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActorPendingUserInputClear {
    None,
    Submission,
}

pub(crate) enum ActorAutoApproval {
    Exec { id: String, turn_id: String },
    Patch { id: String },
}

impl ActorAutoApproval {
    pub(crate) fn into_op(self) -> Op {
        match self {
            ActorAutoApproval::Exec { id, turn_id } => Op::ExecApproval {
                id,
                turn_id: Some(turn_id),
                decision: ReviewDecision::Approved,
            },
            ActorAutoApproval::Patch { id } => Op::PatchApproval {
                id,
                decision: ReviewDecision::Approved,
            },
        }
    }
}

pub(crate) enum ActorStateUpdate {
    LatestUsage {
        info: Option<TokenUsageInfo>,
        rate_limits: Option<RateLimitSnapshot>,
    },
    CollaborationMode(ModeKind),
    RememberCollabAgent(ActorCollabAgentUpdate),
    RememberCollabAgentEntries(Vec<CollabAgentStatusEntry>),
    RemoveCollabAgent(ThreadId),
}

pub(crate) struct ActorCollabAgentUpdate {
    pub(crate) thread_id: ThreadId,
    pub(crate) agent_nickname: Option<String>,
    pub(crate) agent_role: Option<String>,
    pub(crate) status: AgentStatus,
}

pub(crate) enum ReplayRolloutItemRoute<'a> {
    Event(ReplayEventPlan<'a>),
    ResponseItem(ReplayResponseItemRoute<'a>),
    Ignore {
        item: &'a RolloutItem,
        reason: IgnoredCodexEventReason,
    },
}

pub(crate) struct ReplayEventPlan<'a> {
    pub(crate) state_updates: Vec<ActorStateUpdate>,
    pub(crate) action: ReplayEventAction<'a>,
}

pub(crate) enum ReplayEventAction<'a> {
    Effect(Box<BridgeEffect>),
    RegisterPendingUserInput {
        turn_id: &'a str,
        event: &'a RequestUserInputEvent,
    },
    ClearPendingUserInputForSubmission(&'a str),
    ClearPendingUserInput,
    Ignore {
        event: &'a EventMsg,
        reason: IgnoredCodexEventReason,
    },
}

pub(crate) enum ReplayResponseItemRoute<'a> {
    ShellFunctionCall {
        call_id: &'a str,
        name: &'a str,
        arguments: &'a str,
    },
    GenericFunctionCall {
        call_id: &'a str,
        name: &'a str,
        arguments: &'a str,
    },
    FunctionCallOutput {
        call_id: &'a str,
        output: &'a FunctionCallOutputPayload,
    },
    LocalShellCall {
        call_id: &'a str,
        command: &'a [String],
        working_directory: Option<&'a str>,
        status: ReplayToolCallStatus,
    },
    ApplyPatchCustomToolCall {
        call_id: &'a str,
        input: &'a str,
    },
    GenericCustomToolCall {
        call_id: &'a str,
        name: &'a str,
        input: &'a str,
    },
    CustomToolCallOutput {
        call_id: &'a str,
        output: &'a FunctionCallOutputPayload,
    },
    WebSearchCall {
        id: Option<&'a str>,
        action: Option<&'a codex_protocol::models::WebSearchAction>,
    },
    Ignore {
        item: &'a ResponseItem,
        reason: IgnoredCodexEventReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReplayToolCallStatus {
    Completed,
    Failed,
}
