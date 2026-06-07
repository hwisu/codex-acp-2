mod setup;
mod stub_client;
mod stub_thread;
mod test_deps;

pub(super) use setup::{
    setup, setup_actor, setup_actor_with_fast_mode, setup_with_fast_mode, setup_with_goals,
    submit_prompt, submit_prompt_and_wait,
};
pub(super) use stub_client::StubClient;
pub(super) use stub_thread::StubCodexThread;
pub(super) use test_deps::{StubAuth, StubModelsManager};

pub(super) use std::collections::{HashMap, VecDeque};
pub(super) use std::future::Future;
pub(super) use std::path::PathBuf;
pub(super) use std::pin::Pin;
pub(super) use std::sync::Arc;
pub(super) use std::sync::atomic::AtomicUsize;
pub(super) use std::time::Duration;

pub(super) use agent_client_protocol::{
    Error, JsonRpcMessage,
    schema::{
        ClientCapabilities, Content, ContentBlock, ContentChunk, Implementation, Meta,
        PromptRequest, RequestPermissionOutcome, RequestPermissionRequest,
        RequestPermissionResponse, SelectedPermissionOutcome, SessionConfigId, SessionConfigKind,
        SessionConfigOptionCategory, SessionConfigOptionValue, SessionConfigSelectOptions,
        SessionConfigValueId, SessionId, SessionModeId, SessionNotification, SessionUpdate,
        StopReason, Terminal, TextContent, ToolCall, ToolCallContent, ToolCallId, ToolCallStatus,
        ToolCallUpdate, ToolKind,
    },
};
pub(super) use codex_config::LoaderOverrides;
pub(super) use codex_core::{
    config::{Config, ConfigBuilder, ConfigOverrides, PermissionProfileSnapshot},
    review_prompts::user_facing_hint,
    test_support::all_model_presets,
};
pub(super) use codex_features::Feature;
pub(super) use codex_protocol::ThreadId;
pub(super) use codex_protocol::approvals::{
    ElicitationRequest, ElicitationRequestEvent, GuardianAssessmentAction, GuardianAssessmentEvent,
    GuardianAssessmentStatus, GuardianCommandSource, GuardianRiskLevel, NetworkApprovalProtocol,
};
pub(super) use codex_protocol::config_types::{CollaborationMode, ModeKind};
pub(super) use codex_protocol::error::CodexErr;
pub(super) use codex_protocol::models::{
    ActivePermissionProfile, MessagePhase, PermissionProfile, ResponseItem,
};
pub(super) use codex_protocol::openai_models::{ModelPreset, ReasoningEffort};
pub(super) use codex_protocol::parse_command::ParsedCommand;
pub(super) use codex_protocol::plan_tool::{PlanItemArg, StepStatus, UpdatePlanArgs};
pub(super) use codex_protocol::protocol::{
    AgentMessageContentDeltaEvent, AgentMessageEvent, AgentReasoningEvent,
    AgentReasoningSectionBreakEvent, ElicitationAction, Event, EventMsg,
    ExecCommandOutputDeltaEvent, ExitedReviewModeEvent, FileChange, ImageGenerationBeginEvent,
    ImageGenerationEndEvent, Op, RateLimitSnapshot, ReasoningContentDeltaEvent, ReviewDecision,
    ReviewOutputEvent, ReviewRequest, ReviewTarget, RolloutItem, ThreadGoal, ThreadGoalStatus,
    ThreadGoalUpdatedEvent, ThreadSettingsOverrides, TokenCountEvent, TokenUsageInfo,
    TurnAbortedEvent, TurnCompleteEvent, TurnStartedEvent, WarningEvent,
};
pub(super) use codex_protocol::request_permissions::RequestPermissionProfile;
pub(super) use codex_protocol::request_user_input::{
    RequestUserInputEvent, RequestUserInputQuestion,
};
pub(super) use codex_protocol::user_input::UserInput;
pub(super) use itertools::Itertools;
pub(super) use tokio::sync::{Mutex, Notify, mpsc, mpsc::UnboundedSender};

pub(super) use crate::boundary::tool_call::ActiveCommand;
pub(super) use crate::boundary::tool_call::parse_command_tool_call;
pub(super) use crate::guardian::{guardian_action_summary, guardian_assessment_content};
pub(super) use crate::session_mode::{
    CODEX_WORKSPACE_PROFILE_ID, current_session_mode_id, mode_trusts_project,
};
pub(super) use crate::test_fixtures;
pub(super) use crate::thread::actor::{ThreadActor, ThreadActorInit};
pub(super) use crate::thread::approvals::{
    MCP_TOOL_APPROVAL_ALLOW_ALWAYS_OPTION_ID, MCP_TOOL_APPROVAL_ALLOW_OPTION_ID,
    MCP_TOOL_APPROVAL_ALLOW_SESSION_OPTION_ID, MCP_TOOL_APPROVAL_CANCEL_OPTION_ID,
    MCP_TOOL_APPROVAL_PERSIST_SESSION, MCP_TOOL_APPROVAL_REQUEST_ID_PREFIX,
};
pub(super) use crate::thread::client::{ClientSender, SessionClient};
pub(super) use crate::thread::deps::{
    Auth, CodexThreadImpl, ModelsManagerImpl, ThreadGoalSetRequest,
};
pub(super) use crate::thread::model_picker::filter_model_presets_for_picker;
pub(super) use crate::thread::submission::{PromptState, SubmissionState};
pub(super) use crate::thread::{INIT_COMMAND_PROMPT, Thread, ThreadMessage};
pub(super) use crate::user_input::REQUEST_USER_INPUT_OTHER_OPTION_LABEL;
pub(super) use codex_utils_absolute_path::AbsolutePathBuf;
