use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use agent_client_protocol::{
    Error,
    schema::v1::{RequestPermissionResponse, StopReason},
};
use codex_protocol::protocol::EventMsg;
use tokio::sync::{mpsc, oneshot};
use tracing::info;

use crate::boundary::{effect::BridgeEffect, tool_call::ActiveCommand};

use super::{
    ThreadMessage, approvals::PendingPermissionRequest, client::SessionClient,
    deps::CodexThreadImpl,
};

mod interactions;
mod responses;

use self::interactions::PermissionInteractionState;
use self::responses::PromptResponses;

pub(super) struct PermissionInteractionRequest {
    pub(super) request_key: String,
    pub(super) pending_request: PendingPermissionRequest,
    pub(super) request_effect: BridgeEffect,
}

pub(super) enum SubmissionState {
    /// User prompts, including slash commands like /init, /review, /compact, /undo.
    Prompt(PromptState),
}

impl SubmissionState {
    pub(super) fn is_active(&self) -> bool {
        match self {
            Self::Prompt(state) => state.is_active(),
        }
    }

    pub(super) async fn handle_event(&mut self, client: &SessionClient, event: EventMsg) {
        match self {
            Self::Prompt(state) => state.handle_event(client, event).await,
        }
    }

    pub(super) async fn handle_permission_request_resolved(
        &mut self,
        client: &SessionClient,
        request_key: String,
        response: Result<RequestPermissionResponse, Error>,
    ) -> Result<(), Error> {
        match self {
            Self::Prompt(state) => {
                state
                    .handle_permission_request_resolved(client, request_key, response)
                    .await
            }
        }
    }

    pub(super) fn abort_pending_interactions(&mut self) {
        match self {
            Self::Prompt(state) => {
                state.abort_pending_interactions();
            }
        }
    }

    pub(super) fn add_response_tx(
        &mut self,
        response_tx: oneshot::Sender<Result<StopReason, Error>>,
    ) {
        match self {
            Self::Prompt(state) => state.add_response_tx(response_tx),
        }
    }

    pub(super) fn active_command_summaries(&self) -> Vec<String> {
        match self {
            Self::Prompt(state) => state.active_command_summaries(),
        }
    }

    pub(super) fn fail(&mut self, err: Error) {
        let Self::Prompt(state) = self;
        state.send_result(Err(err));
    }
}

pub(super) struct PromptState {
    pub(super) submission_id: String,
    active_tools: ActiveToolState,
    pub(super) thread: Arc<dyn CodexThreadImpl>,
    pub(super) resolution_tx: mpsc::UnboundedSender<ThreadMessage>,
    permission_interactions: PermissionInteractionState,
    event_count: usize,
    responses: PromptResponses,
    streaming_messages: StreamingMessageState,
}

#[derive(Default)]
struct ActiveToolState {
    commands: HashMap<String, ActiveCommand>,
    web_search: Option<String>,
    guardian_assessments: HashSet<String>,
}

#[derive(Default)]
struct StreamingMessageState {
    seen_message_deltas: bool,
    message_delta_text: String,
    seen_reasoning_deltas: bool,
}

impl PromptState {
    pub(super) fn insert_active_command(&mut self, call_id: String, command: ActiveCommand) {
        self.active_tools.commands.insert(call_id, command);
    }

    pub(super) fn remove_active_command(&mut self, call_id: &str) -> Option<ActiveCommand> {
        self.active_tools.commands.remove(call_id)
    }

    pub(super) fn stream_active_command_output(
        &mut self,
        client: &SessionClient,
        call_id: &str,
        data: &str,
    ) -> Option<BridgeEffect> {
        // Stream output bytes to the display-only terminal when supported. For clients
        // without terminal support, skip incremental updates to avoid O(n²) memory growth
        // from repeatedly sending the full accumulated buffer. The completion update will
        // deliver the final output snapshot exactly once.
        if let Some(active_command) = self.active_tools.commands.get_mut(call_id) {
            active_command.output.push_str(data);
            let supports_terminal_output = client.supports_terminal_output(active_command);
            if supports_terminal_output {
                return Some(active_command.render_streaming_effect(
                    call_id,
                    supports_terminal_output,
                    data,
                ));
            }
        }
        None
    }

    pub(super) fn start_active_web_search(&mut self, call_id: String) {
        self.active_tools.web_search = Some(call_id);
    }

    pub(super) fn take_active_web_search(&mut self) -> Option<String> {
        self.active_tools.web_search.take()
    }

    pub(super) fn insert_active_guardian_assessment(&mut self, id: String) -> bool {
        self.active_tools.guardian_assessments.insert(id)
    }

    pub(super) fn remove_active_guardian_assessment(&mut self, id: &str) -> bool {
        self.active_tools.guardian_assessments.remove(id)
    }

    pub(super) fn record_message_delta(&mut self, delta: &str) {
        self.streaming_messages.seen_message_deltas = true;
        self.streaming_messages.message_delta_text.push_str(delta);
    }

    pub(super) fn mark_reasoning_delta_seen(&mut self) {
        self.streaming_messages.seen_reasoning_deltas = true;
    }

    pub(super) fn take_message_delta_text(&mut self) -> Option<String> {
        if std::mem::take(&mut self.streaming_messages.seen_message_deltas) {
            Some(std::mem::take(
                &mut self.streaming_messages.message_delta_text,
            ))
        } else {
            None
        }
    }

    pub(super) fn take_seen_reasoning_deltas(&mut self) -> bool {
        std::mem::take(&mut self.streaming_messages.seen_reasoning_deltas)
    }

    pub(super) fn increment_event_count(&mut self) {
        self.event_count += 1;
    }

    pub(super) fn event_count(&self) -> usize {
        self.event_count
    }

    pub(super) fn new(
        submission_id: String,
        thread: Arc<dyn CodexThreadImpl>,
        resolution_tx: mpsc::UnboundedSender<ThreadMessage>,
        response_tx: oneshot::Sender<Result<StopReason, Error>>,
    ) -> Self {
        Self {
            submission_id,
            active_tools: ActiveToolState::default(),
            thread,
            resolution_tx,
            permission_interactions: PermissionInteractionState::default(),
            event_count: 0,
            responses: PromptResponses::primary(response_tx),
            streaming_messages: StreamingMessageState::default(),
        }
    }

    pub(super) fn for_replay(
        submission_id: String,
        thread: Arc<dyn CodexThreadImpl>,
        resolution_tx: mpsc::UnboundedSender<ThreadMessage>,
    ) -> Self {
        Self {
            submission_id,
            active_tools: ActiveToolState::default(),
            thread,
            resolution_tx,
            permission_interactions: PermissionInteractionState::default(),
            event_count: 0,
            responses: PromptResponses::replay(),
            streaming_messages: StreamingMessageState::default(),
        }
    }

    pub(super) fn is_active(&self) -> bool {
        self.responses.is_active()
    }

    pub(super) fn add_response_tx(
        &mut self,
        response_tx: oneshot::Sender<Result<StopReason, Error>>,
    ) {
        self.responses.add(response_tx);
    }

    pub(super) fn send_result(&mut self, result: Result<StopReason, Error>) {
        self.responses.send(result);
    }

    pub(super) async fn execute_bridge_effect(
        &mut self,
        client: &SessionClient,
        effect: BridgeEffect,
    ) -> Result<Option<RequestPermissionResponse>, Error> {
        match effect {
            BridgeEffect::Forward(update) => {
                client.send_notification(update);
                Ok(None)
            }
            BridgeEffect::RequestPermission(request) => {
                client.request_permission_request(request).await.map(Some)
            }
            BridgeEffect::SubmitOp(op) => {
                self.thread.submit_ok(op).await?;
                Ok(None)
            }
            BridgeEffect::Ignore(reason) => {
                info!("Ignoring bridge effect: {reason:?}");
                Ok(None)
            }
        }
    }

    pub(super) fn active_command_summaries(&self) -> Vec<String> {
        let mut summaries = self
            .active_tools
            .commands
            .values()
            .map(|command| format!("- {} ({})", command.title, command.tool_call_id.0.as_ref()))
            .collect::<Vec<_>>();
        if self.active_tools.web_search.is_some() {
            summaries.push("- Searching the Web".to_string());
        }
        summaries.sort();
        summaries
    }
}
