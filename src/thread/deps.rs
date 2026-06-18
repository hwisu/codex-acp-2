use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, LazyLock},
};

use agent_client_protocol::Error;
use codex_core::CodexThread;
use codex_features::Feature;
use codex_goal_extension::{
    GoalObjectiveUpdate, GoalService, GoalServiceError, GoalSetRequest as CodexGoalSetRequest,
    GoalTokenBudgetUpdate,
};
use codex_login::auth::AuthManager;
use codex_models_manager::manager::{ModelsManager, RefreshStrategy};
use codex_protocol::{
    ThreadId,
    error::CodexErr,
    openai_models::ModelPreset,
    protocol::{Event, Op, ThreadGoal, ThreadGoalStatus},
};

static GOAL_SERVICE: LazyLock<Arc<GoalService>> = LazyLock::new(|| Arc::new(GoalService::new()));

pub(crate) fn goal_service() -> Arc<GoalService> {
    Arc::clone(&GOAL_SERVICE)
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ThreadGoalSetRequest {
    pub(crate) objective: Option<String>,
    pub(crate) status: Option<ThreadGoalStatus>,
    pub(crate) token_budget: Option<Option<i64>>,
}

/// Trait for abstracting over the `CodexThread` to make testing easier.
pub trait CodexThreadImpl: Send + Sync {
    fn submit(&self, op: Op)
    -> Pin<Box<dyn Future<Output = Result<String, CodexErr>> + Send + '_>>;
    fn next_event(&self) -> Pin<Box<dyn Future<Output = Result<Event, CodexErr>> + Send + '_>>;
    fn thread_goal_get(
        &self,
        _thread_id: ThreadId,
    ) -> Pin<Box<dyn Future<Output = Result<Option<ThreadGoal>, Error>> + Send + '_>> {
        Box::pin(async { Err(thread_goals_unsupported_error()) })
    }

    fn thread_goal_set(
        &self,
        _thread_id: ThreadId,
        _request: ThreadGoalSetRequest,
    ) -> Pin<Box<dyn Future<Output = Result<ThreadGoal, Error>> + Send + '_>> {
        Box::pin(async { Err(thread_goals_unsupported_error()) })
    }

    fn thread_goal_clear(
        &self,
        _thread_id: ThreadId,
    ) -> Pin<Box<dyn Future<Output = Result<bool, Error>> + Send + '_>> {
        Box::pin(async { Err(thread_goals_unsupported_error()) })
    }

    fn submit_ok(&self, op: Op) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + '_>> {
        Box::pin(async move {
            self.submit(op)
                .await
                .map_err(|e| Error::from(anyhow::anyhow!(e)))?;
            Ok(())
        })
    }
}

impl CodexThreadImpl for CodexThread {
    fn submit(
        &self,
        op: Op,
    ) -> Pin<Box<dyn Future<Output = Result<String, CodexErr>> + Send + '_>> {
        Box::pin(self.submit(op))
    }

    fn next_event(&self) -> Pin<Box<dyn Future<Output = Result<Event, CodexErr>> + Send + '_>> {
        Box::pin(self.next_event())
    }

    fn thread_goal_get(
        &self,
        thread_id: ThreadId,
    ) -> Pin<Box<dyn Future<Output = Result<Option<ThreadGoal>, Error>> + Send + '_>> {
        Box::pin(async move {
            let state_db = state_db_for_thread_goals(self)?;
            goal_service()
                .get_thread_goal(&state_db, thread_id)
                .await
                .map_err(goal_service_error)
        })
    }

    fn thread_goal_set(
        &self,
        thread_id: ThreadId,
        request: ThreadGoalSetRequest,
    ) -> Pin<Box<dyn Future<Output = Result<ThreadGoal, Error>> + Send + '_>> {
        Box::pin(async move {
            let state_db = state_db_for_thread_goals(self)?;
            let ThreadGoalSetRequest {
                objective,
                status,
                token_budget,
            } = request;
            let service = goal_service();
            let objective = objective
                .as_deref()
                .map_or(GoalObjectiveUpdate::Keep, GoalObjectiveUpdate::Set);
            let token_budget =
                token_budget.map_or(GoalTokenBudgetUpdate::Keep, GoalTokenBudgetUpdate::Set);
            let outcome = service
                .set_thread_goal(
                    &state_db,
                    CodexGoalSetRequest {
                        thread_id,
                        objective,
                        status,
                        token_budget,
                    },
                )
                .await
                .map_err(goal_service_error)?;
            outcome.apply_runtime_effects(&service).await;
            Ok(outcome.goal)
        })
    }

    fn thread_goal_clear(
        &self,
        thread_id: ThreadId,
    ) -> Pin<Box<dyn Future<Output = Result<bool, Error>> + Send + '_>> {
        Box::pin(async move {
            let state_db = state_db_for_thread_goals(self)?;
            goal_service()
                .clear_thread_goal(&state_db, thread_id)
                .await
                .map_err(goal_service_error)
        })
    }
}

fn goal_service_error(err: GoalServiceError) -> Error {
    match err {
        GoalServiceError::InvalidRequest(message) => Error::invalid_params().data(message),
        GoalServiceError::Internal(message) => Error::internal_error().data(message),
    }
}

fn thread_goals_unsupported_error() -> Error {
    Error::invalid_params().data("thread goals are not supported by this thread")
}

fn state_db_for_thread_goals(
    thread: &CodexThread,
) -> Result<Arc<codex_state::StateRuntime>, Error> {
    if !thread.enabled(Feature::Goals) {
        return Err(Error::invalid_params().data("goals feature is disabled"));
    }
    if thread.rollout_path().is_none() {
        return Err(Error::invalid_params().data("ephemeral thread does not support goals"));
    }
    thread
        .state_db()
        .ok_or_else(|| Error::internal_error().data("sqlite state db unavailable for thread goals"))
}

pub(crate) trait ModelsManagerImpl: Send + Sync {
    fn get_model(
        &self,
        model_id: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = String> + Send + '_>>;
    fn list_models(&self) -> Pin<Box<dyn Future<Output = Vec<ModelPreset>> + Send + '_>>;
}

impl ModelsManagerImpl for Arc<dyn ModelsManager> {
    fn get_model(
        &self,
        model_id: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = String> + Send + '_>> {
        let model_id = model_id.map(ToOwned::to_owned);
        Box::pin(async move {
            self.get_default_model(&model_id, RefreshStrategy::Online)
                .await
        })
    }

    fn list_models(&self) -> Pin<Box<dyn Future<Output = Vec<ModelPreset>> + Send + '_>> {
        Box::pin(
            async move { ModelsManager::list_models(self.as_ref(), RefreshStrategy::Online).await },
        )
    }
}

pub(super) trait Auth {
    fn logout(&self) -> impl Future<Output = Result<bool, Error>> + Send;
}

impl Auth for Arc<AuthManager> {
    async fn logout(&self) -> Result<bool, Error> {
        self.as_ref()
            .logout()
            .await
            .map_err(|e| Error::internal_error().data(e.to_string()))
    }
}
