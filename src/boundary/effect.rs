use agent_client_protocol::schema::v1::{
    PermissionOption, RequestPermissionRequest, SessionId, SessionUpdate, ToolCall, ToolCallUpdate,
};
use codex_protocol::protocol::Op;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BridgeEffectKind {
    Forward,
    RequestPermission,
    #[allow(dead_code)]
    SubmitOp,
    Ignore(IgnoredCodexEventReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BridgeEventContext {
    Live,
    #[allow(dead_code)]
    Replay,
}

pub(crate) enum BridgeEffect {
    Forward(SessionUpdate),
    RequestPermission(RequestPermissionRequest),
    SubmitOp(Op),
    Ignore(IgnoredCodexEventReason),
}

pub(crate) struct PermissionRequestSeed {
    pub(in crate::boundary) tool_call: ToolCallUpdate,
    pub(in crate::boundary) options: Vec<PermissionOption>,
}

impl PermissionRequestSeed {
    pub(in crate::boundary) fn new(
        tool_call: ToolCallUpdate,
        options: Vec<PermissionOption>,
    ) -> Self {
        Self { tool_call, options }
    }

    #[cfg(test)]
    pub(crate) fn tool_call_id(&self) -> &agent_client_protocol::schema::v1::ToolCallId {
        &self.tool_call.tool_call_id
    }

    #[cfg(test)]
    pub(crate) fn option_ids(&self) -> Vec<String> {
        self.options
            .iter()
            .map(|option| option.option_id.0.to_string())
            .collect()
    }
}

impl BridgeEffect {
    pub(crate) fn tool_call(tool_call: ToolCall) -> Self {
        Self::Forward(SessionUpdate::ToolCall(tool_call))
    }

    pub(crate) fn tool_call_update(update: ToolCallUpdate) -> Self {
        Self::Forward(SessionUpdate::ToolCallUpdate(update))
    }

    pub(crate) fn request_permission(
        session_id: SessionId,
        request: PermissionRequestSeed,
    ) -> Self {
        Self::RequestPermission(RequestPermissionRequest::new(
            session_id,
            request.tool_call,
            request.options,
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IgnoredCodexEventReason {
    LifecycleOnly,
    AlreadyRenderedByAnotherEvent,
    AlreadyRenderedByClientInput,
    HandledByActor,
    HandledByResponseItem,
    MissingToolCallId,
    MissingUsageInfo,
    MissingUsageContextWindow,
    InvalidUsageContextWindow,
    MissingReviewOutput,
    EmptyToolCallUpdate,
    StateOnly,
    UnsupportedByAcp,
    DiagnosticOnly,
    SupersededBySnapshot,
}
