use std::{future::Future, pin::Pin, sync::Arc};

use agent_client_protocol::Error;
use codex_core::{CodexThread, ExternalGoalPreviousStatus, ExternalGoalSet};
use codex_features::Feature;
use codex_login::auth::AuthManager;
use codex_models_manager::manager::{ModelsManager, RefreshStrategy};
use codex_protocol::{
    ThreadId,
    error::CodexErr,
    openai_models::ModelPreset,
    protocol::{Event, Op, ThreadGoal, ThreadGoalStatus, validate_thread_goal_objective},
};

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
            state_db
                .get_thread_goal(thread_id)
                .await
                .map(|goal| goal.map(protocol_goal_from_state))
                .map_err(|err| {
                    Error::internal_error().data(format!("failed to read thread goal: {err}"))
                })
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
            let objective = objective.map(|objective| objective.trim().to_string());
            validate_goal_budget(token_budget.flatten())?;
            if let Some(objective) = objective.as_deref() {
                validate_thread_goal_objective(objective)
                    .map_err(|message| Error::invalid_params().data(message))?;
            }

            self.prepare_external_goal_mutation().await;
            let previous_goal = state_db.get_thread_goal(thread_id).await.map_err(|err| {
                Error::internal_error().data(format!("failed to read thread goal: {err}"))
            })?;
            let previous_status = previous_goal
                .as_ref()
                .map_or(ExternalGoalPreviousStatus::NewGoal, |goal| {
                    ExternalGoalPreviousStatus::Existing(goal.status)
                });

            let status = status.map(state_goal_status_from_protocol);
            let goal = if let Some(objective) = objective.as_deref() {
                if let Some(goal) = previous_goal.as_ref().filter(|goal| {
                    goal.objective == objective
                        && goal.status != codex_state::ThreadGoalStatus::Complete
                }) {
                    state_db
                        .update_thread_goal(
                            thread_id,
                            codex_state::ThreadGoalUpdate {
                                status,
                                token_budget,
                                expected_goal_id: Some(goal.goal_id.clone()),
                            },
                        )
                        .await
                        .map_err(|err| {
                            Error::internal_error()
                                .data(format!("failed to update thread goal: {err}"))
                        })?
                        .ok_or_else(|| {
                            Error::invalid_params().data(format!(
                                "cannot update goal for thread {thread_id}: no goal exists"
                            ))
                        })?
                } else {
                    state_db
                        .replace_thread_goal(
                            thread_id,
                            objective,
                            status.unwrap_or(codex_state::ThreadGoalStatus::Active),
                            token_budget.flatten(),
                        )
                        .await
                        .map_err(|err| {
                            Error::internal_error()
                                .data(format!("failed to replace thread goal: {err}"))
                        })?
                }
            } else {
                state_db
                    .update_thread_goal(
                        thread_id,
                        codex_state::ThreadGoalUpdate {
                            status,
                            token_budget,
                            expected_goal_id: None,
                        },
                    )
                    .await
                    .map_err(|err| {
                        Error::internal_error().data(format!("failed to update thread goal: {err}"))
                    })?
                    .ok_or_else(|| {
                        Error::invalid_params().data(format!(
                            "cannot update goal for thread {thread_id}: no goal exists"
                        ))
                    })?
            };

            let protocol_goal = protocol_goal_from_state(goal.clone());
            self.apply_external_goal_set(ExternalGoalSet {
                goal,
                previous_status,
            })
            .await;
            Ok(protocol_goal)
        })
    }

    fn thread_goal_clear(
        &self,
        thread_id: ThreadId,
    ) -> Pin<Box<dyn Future<Output = Result<bool, Error>> + Send + '_>> {
        Box::pin(async move {
            let state_db = state_db_for_thread_goals(self)?;
            self.prepare_external_goal_mutation().await;
            let cleared = state_db
                .delete_thread_goal(thread_id)
                .await
                .map_err(|err| {
                    Error::internal_error().data(format!("failed to clear thread goal: {err}"))
                })?;
            if cleared {
                self.apply_external_goal_clear().await;
            }
            Ok(cleared)
        })
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

fn validate_goal_budget(value: Option<i64>) -> Result<(), Error> {
    if value.is_some_and(|value| value <= 0) {
        return Err(Error::invalid_params().data("goal budgets must be positive when provided"));
    }
    Ok(())
}

fn state_goal_status_from_protocol(status: ThreadGoalStatus) -> codex_state::ThreadGoalStatus {
    match status {
        ThreadGoalStatus::Active => codex_state::ThreadGoalStatus::Active,
        ThreadGoalStatus::Paused => codex_state::ThreadGoalStatus::Paused,
        ThreadGoalStatus::BudgetLimited => codex_state::ThreadGoalStatus::BudgetLimited,
        ThreadGoalStatus::Complete => codex_state::ThreadGoalStatus::Complete,
    }
}

fn protocol_goal_from_state(goal: codex_state::ThreadGoal) -> ThreadGoal {
    ThreadGoal {
        thread_id: goal.thread_id,
        objective: goal.objective,
        status: protocol_goal_status_from_state(goal.status),
        token_budget: goal.token_budget,
        tokens_used: goal.tokens_used,
        time_used_seconds: goal.time_used_seconds,
        created_at: goal.created_at.timestamp(),
        updated_at: goal.updated_at.timestamp(),
    }
}

fn protocol_goal_status_from_state(status: codex_state::ThreadGoalStatus) -> ThreadGoalStatus {
    match status {
        codex_state::ThreadGoalStatus::Active => ThreadGoalStatus::Active,
        codex_state::ThreadGoalStatus::Paused => ThreadGoalStatus::Paused,
        codex_state::ThreadGoalStatus::BudgetLimited => ThreadGoalStatus::BudgetLimited,
        codex_state::ThreadGoalStatus::Complete => ThreadGoalStatus::Complete,
    }
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
