use std::sync::{Arc, LazyLock, Mutex};

use agent_client_protocol::{
    Client, ConnectionTo, Error,
    schema::{
        ClientCapabilities, Implementation, LoadSessionResponse, ModelId, PromptRequest,
        RequestPermissionResponse, SessionConfigId, SessionConfigOption, SessionConfigOptionValue,
        SessionId, SessionModeId, StopReason,
    },
};
use codex_core::config::Config;
use codex_login::auth::AuthManager;
use codex_models_manager::collaboration_mode_presets::builtin_collaboration_mode_presets;
use codex_protocol::{
    ThreadId,
    config_types::{CollaborationMode, ModeKind, Settings},
    openai_models::ReasoningEffort,
    protocol::RolloutItem,
};
use tokio::sync::{mpsc, oneshot};

mod actor;
mod actor_config;
mod actor_models;
mod actor_modes;
mod actor_prompt;
mod actor_state;
mod actor_status;
mod approvals;
mod client;
mod deps;
mod model_picker;
mod prompt_items;
mod replay;
mod replay_items;
mod slash_commands;
mod submission;
mod submission_dispatch;
mod submission_exec;
mod submission_guardian;
mod submission_lifecycle;
mod submission_mcp;
mod submission_patch;
mod submission_permissions;
mod submission_web_image;

use client::SessionClient;
pub use deps::CodexThreadImpl;
use deps::ModelsManagerImpl;

use crate::boundary::op;

static DISABLE_TERMINAL_OUTPUT: LazyLock<bool> = LazyLock::new(|| {
    std::env::var("CODEX_ACP_DISABLE_TERMINAL_OUTPUT")
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
});

static ENABLE_EXPERIMENTAL_TERMINAL_OUTPUT: LazyLock<bool> = LazyLock::new(|| {
    std::env::var("CODEX_ACP_ENABLE_EXPERIMENTAL_TERMINAL_OUTPUT")
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
});

const INIT_COMMAND_PROMPT: &str = include_str!("./prompt_for_init_command.md");

enum ThreadMessage {
    Load {
        response_tx: oneshot::Sender<Result<LoadSessionResponse, Error>>,
    },
    GetConfigOptions {
        response_tx: oneshot::Sender<Result<Vec<SessionConfigOption>, Error>>,
    },
    Prompt {
        request: PromptRequest,
        response_tx: oneshot::Sender<Result<oneshot::Receiver<Result<StopReason, Error>>, Error>>,
    },
    SetMode {
        mode: SessionModeId,
        response_tx: oneshot::Sender<Result<(), Error>>,
    },
    SetModel {
        model: ModelId,
        response_tx: oneshot::Sender<Result<(), Error>>,
    },
    SetConfigOption {
        config_id: SessionConfigId,
        value: SessionConfigOptionValue,
        response_tx: oneshot::Sender<Result<(), Error>>,
    },
    Cancel {
        response_tx: oneshot::Sender<Result<(), Error>>,
    },
    Shutdown {
        response_tx: oneshot::Sender<Result<(), Error>>,
    },
    ReplaceClient {
        client: SessionClient,
        response_tx: oneshot::Sender<Result<(), Error>>,
    },
    ReplayHistory {
        history: Vec<RolloutItem>,
        response_tx: oneshot::Sender<Result<(), Error>>,
    },
    PermissionRequestResolved {
        submission_id: String,
        request_key: String,
        response: Result<RequestPermissionResponse, Error>,
    },
}

pub struct Thread {
    /// Direct handle to the underlying Codex thread for out-of-band shutdown.
    thread: Arc<dyn CodexThreadImpl>,
    /// A sender for interacting with the thread.
    message_tx: mpsc::UnboundedSender<ThreadMessage>,
    /// Keep the actor task alive for the lifetime of the thread wrapper.
    _handle: tokio::task::JoinHandle<()>,
}

pub(crate) struct ThreadInit {
    pub(crate) session_id: SessionId,
    pub(crate) thread_id: ThreadId,
    pub(crate) thread: Arc<dyn CodexThreadImpl>,
    pub(crate) auth: Arc<AuthManager>,
    pub(crate) models_manager: Arc<dyn ModelsManagerImpl>,
    pub(crate) client_capabilities: Arc<Mutex<ClientCapabilities>>,
    pub(crate) client_info: Arc<Mutex<Option<Implementation>>>,
    pub(crate) config: Config,
    pub(crate) cx: ConnectionTo<Client>,
}

impl Thread {
    pub(crate) fn new(init: ThreadInit) -> Self {
        let ThreadInit {
            session_id,
            thread_id,
            thread,
            auth,
            models_manager,
            client_capabilities,
            client_info,
            config,
            cx,
        } = init;
        let (message_tx, message_rx) = mpsc::unbounded_channel();
        let (resolution_tx, resolution_rx) = mpsc::unbounded_channel();

        let actor = actor::ThreadActor::new(actor::ThreadActorInit {
            auth,
            client: SessionClient::new(session_id, cx, client_capabilities, client_info),
            thread: thread.clone(),
            thread_id,
            models_manager,
            config,
            message_rx,
            resolution_tx,
            resolution_rx,
        });
        let handle = tokio::spawn(actor.spawn());

        Self {
            thread,
            message_tx,
            _handle: handle,
        }
    }

    pub async fn load(&self) -> Result<LoadSessionResponse, Error> {
        self.request_actor(|response_tx| ThreadMessage::Load { response_tx })
            .await
    }

    pub async fn config_options(&self) -> Result<Vec<SessionConfigOption>, Error> {
        self.request_actor(|response_tx| ThreadMessage::GetConfigOptions { response_tx })
            .await
    }

    pub async fn prompt(&self, request: PromptRequest) -> Result<StopReason, Error> {
        self.request_actor(|response_tx| ThreadMessage::Prompt {
            request,
            response_tx,
        })
        .await?
        .await
        .map_err(|_| thread_actor_not_running_error())?
    }

    pub async fn set_mode(&self, mode: SessionModeId) -> Result<(), Error> {
        self.request_actor(|response_tx| ThreadMessage::SetMode { mode, response_tx })
            .await
    }

    pub async fn set_model(&self, model: ModelId) -> Result<(), Error> {
        self.request_actor(|response_tx| ThreadMessage::SetModel { model, response_tx })
            .await
    }

    pub async fn set_config_option(
        &self,
        config_id: SessionConfigId,
        value: SessionConfigOptionValue,
    ) -> Result<(), Error> {
        self.request_actor(|response_tx| ThreadMessage::SetConfigOption {
            config_id,
            value,
            response_tx,
        })
        .await
    }

    pub async fn cancel(&self) -> Result<(), Error> {
        self.request_actor(|response_tx| ThreadMessage::Cancel { response_tx })
            .await
    }

    pub async fn replay_history(&self, history: Vec<RolloutItem>) -> Result<(), Error> {
        self.request_actor(|response_tx| ThreadMessage::ReplayHistory {
            history,
            response_tx,
        })
        .await
    }

    pub async fn replace_client(
        &self,
        session_id: SessionId,
        cx: ConnectionTo<Client>,
        client_capabilities: Arc<Mutex<ClientCapabilities>>,
        client_info: Arc<Mutex<Option<Implementation>>>,
    ) -> Result<(), Error> {
        let client = SessionClient::new(session_id, cx, client_capabilities, client_info);
        self.request_actor(|response_tx| ThreadMessage::ReplaceClient {
            client,
            response_tx,
        })
        .await
    }

    pub async fn shutdown(&self) -> Result<(), Error> {
        let (response_tx, response_rx) = oneshot::channel();
        let message = ThreadMessage::Shutdown { response_tx };

        if self.message_tx.send(message).is_err() {
            self.thread.submit_ok(op::shutdown()).await?;
        } else {
            response_rx
                .await
                .map_err(|_| thread_actor_not_running_error())??;
        }
        // Let the actor drain the resulting turn-aborted/shutdown events so any in-flight
        // prompt callers observe a clean cancellation instead of a dropped response channel.
        Ok(())
    }

    async fn request_actor<T>(
        &self,
        build_message: impl FnOnce(oneshot::Sender<Result<T, Error>>) -> ThreadMessage,
    ) -> Result<T, Error> {
        let (response_tx, response_rx) = oneshot::channel();
        self.message_tx
            .send(build_message(response_tx))
            .map_err(|_| thread_actor_not_running_error())?;

        response_rx
            .await
            .map_err(|_| thread_actor_not_running_error())?
    }
}

fn thread_actor_not_running_error() -> Error {
    Error::internal_error().data("thread actor is not running")
}

fn collaboration_mode_for_kind(
    kind: ModeKind,
    model: String,
    reasoning_effort: Option<ReasoningEffort>,
) -> Option<CollaborationMode> {
    let base = CollaborationMode {
        mode: ModeKind::Default,
        settings: Settings {
            model,
            reasoning_effort,
            developer_instructions: None,
        },
    };
    let mask = builtin_collaboration_mode_presets()
        .into_iter()
        .find(|mask| mask.mode == Some(kind))?;
    Some(base.apply_mask(&mask))
}

#[cfg(test)]
mod tests;
