use std::{collections::HashMap, sync::Arc, time::Duration};

use agent_client_protocol::Error;
use codex_core::config::Config;
use codex_protocol::{ThreadId, models::PermissionProfile, protocol::Event};
use tokio::sync::{mpsc, oneshot};
use tracing::{error, warn};

use crate::boundary::{
    effect::{BridgeEffect, BridgeEffectKind},
    mapper::{self, ActorEventAction, ActorPendingUserInputClear},
    op, session_update,
};

use super::{
    ThreadMessage,
    actor_state::ActorState,
    client::SessionClient,
    deps::{Auth, CodexThreadImpl, ModelsManagerImpl},
    submission::SubmissionState,
};

pub(super) struct ThreadActor<A> {
    /// Allows for logging out from slash commands
    pub(super) auth: A,
    /// Used for sending messages back to the client.
    pub(super) client: SessionClient,
    /// The thread associated with this task.
    pub(super) thread: Arc<dyn CodexThreadImpl>,
    /// The stable Codex thread id backing this ACP session.
    pub(super) thread_id: ThreadId,
    /// The configuration for the thread.
    pub(super) config: Config,
    /// The models available for this thread.
    pub(super) models_manager: Arc<dyn ModelsManagerImpl>,
    /// Internal message sender used to route spawned interaction results back to the actor.
    resolution_tx: mpsc::UnboundedSender<ThreadMessage>,
    /// A sender for each interested `Op` submission that needs events routed.
    submissions: HashMap<String, SubmissionState>,
    /// A receiver for incoming thread messages.
    pub(super) message_rx: mpsc::UnboundedReceiver<ThreadMessage>,
    /// A receiver for spawned interaction results.
    pub(super) resolution_rx: mpsc::UnboundedReceiver<ThreadMessage>,
    /// Mutable session state that should not be edited directly by sibling modules.
    pub(super) state: ActorState,
}

pub(super) struct ThreadActorInit<A> {
    pub(super) auth: A,
    pub(super) client: SessionClient,
    pub(super) thread: Arc<dyn CodexThreadImpl>,
    pub(super) thread_id: ThreadId,
    pub(super) models_manager: Arc<dyn ModelsManagerImpl>,
    pub(super) config: Config,
    pub(super) message_rx: mpsc::UnboundedReceiver<ThreadMessage>,
    pub(super) resolution_tx: mpsc::UnboundedSender<ThreadMessage>,
    pub(super) resolution_rx: mpsc::UnboundedReceiver<ThreadMessage>,
}

impl<A: Auth> ThreadActor<A> {
    pub(super) fn new(init: ThreadActorInit<A>) -> Self {
        Self {
            auth: init.auth,
            client: init.client,
            thread: init.thread,
            thread_id: init.thread_id,
            config: init.config,
            models_manager: init.models_manager,
            resolution_tx: init.resolution_tx,
            submissions: HashMap::new(),
            message_rx: init.message_rx,
            resolution_rx: init.resolution_rx,
            state: ActorState::default(),
        }
    }

    pub(super) fn resolution_tx(&self) -> mpsc::UnboundedSender<ThreadMessage> {
        self.resolution_tx.clone()
    }

    pub(super) fn has_submission(&self, submission_id: &str) -> bool {
        self.submissions.contains_key(submission_id)
    }

    pub(super) fn insert_submission(&mut self, submission_id: String, submission: SubmissionState) {
        self.submissions.insert(submission_id, submission);
    }

    pub(super) fn submission_mut(&mut self, submission_id: &str) -> Option<&mut SubmissionState> {
        self.submissions.get_mut(submission_id)
    }

    pub(super) fn active_command_summaries(&self) -> Vec<String> {
        self.submissions
            .values()
            .flat_map(SubmissionState::active_command_summaries)
            .collect()
    }

    pub(super) fn active_command_count(&self) -> usize {
        self.submissions
            .values()
            .flat_map(SubmissionState::active_command_summaries)
            .count()
    }

    pub(super) async fn spawn(mut self) {
        let mut message_rx_open = true;
        loop {
            tokio::select! {
                biased;
                message = self.message_rx.recv(), if message_rx_open => match message {
                    Some(message) => self.handle_message(message).await,
                    None => message_rx_open = false,
                },
                message = self.resolution_rx.recv() => if let Some(message) = message {
                    self.handle_message(message).await;
                },
                event = self.thread.next_event() => match event {
                    Ok(event) => self.handle_event(event).await,
                    Err(e) => {
                        error!("Error getting next event: {:?}", e);
                        break;
                    }
                }
            }
            // Litter collection of senders with no receivers
            let pending_submission_id = self.state.pending_submission_id();
            self.submissions.retain(|submission_id, submission| {
                submission.is_active()
                    || pending_submission_id.is_some_and(|pending| pending == submission_id)
            });

            if !message_rx_open && self.submissions.is_empty() {
                break;
            }
        }
    }

    pub(super) async fn handle_message(&mut self, message: ThreadMessage) {
        match message {
            ThreadMessage::Load { response_tx } => {
                let result = self.handle_load().await;
                let available_commands = self.available_commands();
                send_actor_response(response_tx, result);
                let client = self.client.clone();
                // Have this happen after the session is loaded by putting it
                // in a separate task
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    client
                        .send_notification(session_update::available_commands(available_commands));
                });
            }
            ThreadMessage::GetConfigOptions { response_tx } => {
                let result = self.config_options().await;
                send_actor_response(response_tx, result);
            }
            ThreadMessage::Prompt {
                request,
                response_tx,
            } => {
                let result = self.handle_prompt(request).await;
                send_actor_response(response_tx, result);
            }
            ThreadMessage::SetMode { mode, response_tx } => {
                let result = self.handle_set_mode(mode).await;
                send_actor_response(response_tx, result);
                self.maybe_emit_config_options_update().await;
            }
            ThreadMessage::SetConfigOption {
                config_id,
                value,
                response_tx,
            } => {
                let result = self.handle_set_config_option(config_id, value).await;
                send_actor_response(response_tx, result);
            }
            ThreadMessage::Cancel { response_tx } => {
                let result = self.handle_cancel().await;
                send_actor_response(response_tx, result);
            }
            ThreadMessage::Shutdown { response_tx } => {
                let result = self.handle_shutdown().await;
                send_actor_response(response_tx, result);
            }
            ThreadMessage::ReplaceClient {
                client,
                response_tx,
            } => {
                self.client = client;
                send_actor_response(response_tx, Ok(()));
            }
            ThreadMessage::ReplayHistory {
                history,
                response_tx,
            } => {
                self.handle_replay_history(history);
                send_actor_response(response_tx, Ok(()));
            }
            ThreadMessage::PermissionRequestResolved {
                submission_id,
                request_key,
                response,
            } => {
                let Some(submission) = self.submissions.get_mut(&submission_id) else {
                    warn!(
                        "Ignoring permission response for unknown submission ID: {submission_id}"
                    );
                    return;
                };

                if let Err(err) = submission
                    .handle_permission_request_resolved(&self.client, request_key, response)
                    .await
                {
                    submission.abort_pending_interactions();
                    submission.fail(err);
                }
            }
        }
    }

    pub(super) async fn handle_cancel(&mut self) -> Result<(), Error> {
        self.abort_pending_interactions();
        self.thread.submit_ok(op::interrupt()).await?;
        Ok(())
    }

    pub(super) async fn handle_shutdown(&mut self) -> Result<(), Error> {
        self.abort_pending_interactions();
        self.thread.submit_ok(op::shutdown()).await?;
        Ok(())
    }

    pub(super) fn abort_pending_interactions(&mut self) {
        for submission in self.submissions.values_mut() {
            submission.abort_pending_interactions();
        }
    }

    pub(super) async fn handle_event(&mut self, Event { id, msg }: Event) {
        let plan = mapper::plan_actor_event(&msg);
        self.state.apply_event_updates(plan.state_updates);

        let (bridge_effect, clear_pending_user_input, full_access_auto_approval) = match plan.action
        {
            ActorEventAction::RegisterPendingUserInput(event) => {
                self.register_pending_user_input(id, event);
                return;
            }
            ActorEventAction::RouteToSubmission {
                bridge_effect,
                clear_pending_user_input,
                full_access_auto_approval,
            } => (
                bridge_effect,
                clear_pending_user_input,
                full_access_auto_approval,
            ),
        };

        if self
            .maybe_auto_approve_permission_request(full_access_auto_approval)
            .await
        {
            return;
        }

        if let Some(submission) = self.submissions.get_mut(&id) {
            submission.handle_event(&self.client, msg).await;
        } else if let BridgeEffectKind::Ignore(reason) = bridge_effect {
            tracing::info!("Ignoring Codex event for unknown submission {id}: {reason:?}");
        } else {
            warn!("Received event for unknown submission ID: {id} {msg:?}");
        }

        match clear_pending_user_input {
            ActorPendingUserInputClear::Submission => {
                self.state.clear_pending_user_input_for_submission(&id);
            }
            ActorPendingUserInputClear::None => {}
        }
    }

    async fn maybe_auto_approve_permission_request(
        &self,
        auto_approval: Option<mapper::ActorAutoApproval>,
    ) -> bool {
        if !matches!(
            self.config.permissions.permission_profile(),
            PermissionProfile::Disabled
        ) {
            return false;
        }

        let Some(auto_approval) = auto_approval else {
            return false;
        };

        if let Err(err) = self.thread.submit_ok(auto_approval.into_op()).await {
            warn!("failed to auto-approve permission request in full-access mode: {err}");
        }
        true
    }

    pub(super) fn execute_actor_effect(&self, effect: BridgeEffect) {
        match effect {
            BridgeEffect::Forward(update) => {
                self.client.send_notification(update);
            }
            BridgeEffect::Ignore(reason) => {
                tracing::info!("Ignoring replay bridge effect: {reason:?}");
            }
            BridgeEffect::RequestPermission(_) => {
                warn!("Ignoring replay permission request effect");
            }
            BridgeEffect::SubmitOp(_) => {
                warn!("Ignoring replay submit-op effect");
            }
        }
    }

    pub(super) fn execute_replay_effect(&self, effect: BridgeEffect) {
        self.execute_actor_effect(effect);
    }
}

fn send_actor_response<T>(
    response_tx: oneshot::Sender<Result<T, Error>>,
    result: Result<T, Error>,
) {
    drop(response_tx.send(result));
}
