use agent_client_protocol::{
    Error,
    schema::{AvailableCommand, AvailableCommandInput, SessionModeId, UnstructuredCommandInput},
};
use codex_features::Feature;
use codex_protocol::{
    config_types::{ModeKind, ServiceTier},
    protocol::{Op, ReviewTarget, ThreadGoal, ThreadGoalStatus, validate_thread_goal_objective},
    user_input::UserInput,
};

use crate::{
    boundary::op,
    display::{
        format_thread_goal_status_label, format_thread_goal_summary,
        format_thread_goal_usage_summary,
    },
};

use super::{
    INIT_COMMAND_PROMPT,
    actor::ThreadActor,
    deps::{Auth, ThreadGoalSetRequest},
    prompt_items::{extract_slash_command, replace_first_text_item},
};

const GOAL_USAGE: &str = "Usage: /goal <objective>";
const GOAL_USAGE_HINT: &str = "Example: /goal improve benchmark coverage";

pub(super) enum PromptSubmission {
    Submit { op: Box<Op> },
    Handled { message: String },
}

impl<A: Auth> ThreadActor<A> {
    pub(super) fn available_commands(&self) -> Vec<AvailableCommand> {
        Self::builtin_commands(self.goals_enabled(), self.fast_mode_configurable())
    }

    fn builtin_commands(goals_enabled: bool, fast_mode_enabled: bool) -> Vec<AvailableCommand> {
        let mut commands = vec![
            AvailableCommand::new("review", "Review my current changes and find issues").input(
                AvailableCommandInput::Unstructured(UnstructuredCommandInput::new(
                    "optional custom review instructions",
                )),
            ),
            AvailableCommand::new(
                "review-branch",
                "Review the code changes against a specific branch",
            )
            .input(AvailableCommandInput::Unstructured(
                UnstructuredCommandInput::new("branch name"),
            )),
            AvailableCommand::new(
                "review-commit",
                "Review the code changes introduced by a commit",
            )
            .input(AvailableCommandInput::Unstructured(
                UnstructuredCommandInput::new("commit sha"),
            )),
            AvailableCommand::new(
                "init",
                "create an AGENTS.md file with instructions for Codex",
            ),
            AvailableCommand::new(
                "compact",
                "summarize conversation to prevent hitting the context limit",
            ),
            AvailableCommand::new(
                "status",
                "show the current model, approval preset, and session state",
            ),
            AvailableCommand::new(
                "usage",
                "show the current token usage, context window, rate limits, and credits",
            ),
            AvailableCommand::new("agent", "show the subagents this ACP session knows about"),
            AvailableCommand::new(
                "ps",
                "list active background tool calls tracked by this adapter",
            ),
            AvailableCommand::new("permissions", "show or change the approval preset").input(
                AvailableCommandInput::Unstructured(UnstructuredCommandInput::new(
                    "optional preset: read-only | auto | full-access",
                )),
            ),
            AvailableCommand::new("undo", "undo Codex’s most recent turn"),
            AvailableCommand::new("logout", "logout of Codex"),
        ];

        if goals_enabled {
            commands.push(
                AvailableCommand::new("goal", "set or view the goal for a long-running task")
                    .input(AvailableCommandInput::Unstructured(
                        UnstructuredCommandInput::new(
                            "optional: clear | pause | resume | objective",
                        ),
                    )),
            );
        }

        if fast_mode_enabled {
            commands.push(
                AvailableCommand::new("fast", "toggle Fast mode for future turns").input(
                    AvailableCommandInput::Unstructured(UnstructuredCommandInput::new(
                        "optional: on | off | status",
                    )),
                ),
            );
        }

        commands
    }

    pub(super) async fn prompt_submission_for_items(
        &mut self,
        mut items: Vec<UserInput>,
    ) -> Result<PromptSubmission, Error> {
        let Some((name, rest)) =
            extract_slash_command(&items).map(|(name, rest)| (name.to_string(), rest.to_string()))
        else {
            return Ok(PromptSubmission::Submit {
                op: Box::new(op::user_input(items)),
            });
        };

        let op = match name.as_str() {
            "compact" => op::compact(),
            "undo" => op::undo_last_turn(),
            "init" => op::user_input(vec![UserInput::Text {
                text: INIT_COMMAND_PROMPT.into(),
                text_elements: vec![],
            }]),
            "review" => {
                let instructions = rest.trim();
                let target = if instructions.is_empty() {
                    self.default_review_target()
                } else {
                    ReviewTarget::Custom {
                        instructions: instructions.to_owned(),
                    }
                };
                op::review(target)
            }
            "review-branch" if !rest.is_empty() => op::review(ReviewTarget::BaseBranch {
                branch: rest.trim().to_owned(),
            }),
            "review-commit" if !rest.is_empty() => op::review(ReviewTarget::Commit {
                sha: rest.trim().to_owned(),
                title: None,
            }),
            "status" => {
                return Ok(PromptSubmission::Handled {
                    message: self.current_status_summary().await,
                });
            }
            "usage" => {
                return Ok(PromptSubmission::Handled {
                    message: self.current_usage_summary(),
                });
            }
            "goal" => {
                return Ok(PromptSubmission::Handled {
                    message: self.handle_goal_command(&rest).await?,
                });
            }
            "fast" => {
                return Ok(PromptSubmission::Handled {
                    message: self.handle_fast_command(&rest).await?,
                });
            }
            "agent" | "subagents" => {
                return Ok(PromptSubmission::Handled {
                    message: self.subagent_summary(),
                });
            }
            "ps" => {
                return Ok(PromptSubmission::Handled {
                    message: self.background_tasks_summary(),
                });
            }
            "approvals" | "permissions" => {
                let message = if let Some(mode) = Self::permission_preset_from_arg(rest.trim()) {
                    self.handle_set_approval_preset(SessionModeId::new(mode))
                        .await?;
                    self.maybe_emit_config_options_update().await;
                    format!("Approval preset set to {mode}.")
                } else {
                    self.current_approval_preset_summary()
                };
                return Ok(PromptSubmission::Handled { message });
            }
            "plan" => {
                let trimmed = rest.trim().to_string();
                if matches!(trimmed.as_str(), "" | "on") {
                    self.switch_collaboration_mode_and_emit(ModeKind::Plan)
                        .await?;
                    return Ok(PromptSubmission::Handled {
                        message: "Plan mode enabled. Future turns will run in plan mode until you switch back with `/plan off`."
                            .to_string(),
                    });
                }

                if matches!(trimmed.as_str(), "off" | "default" | "code") {
                    self.switch_collaboration_mode_and_emit(ModeKind::Default)
                        .await?;
                    return Ok(PromptSubmission::Handled {
                        message: "Plan mode disabled. Future turns will run in default mode."
                            .to_string(),
                    });
                }

                self.switch_collaboration_mode_and_emit(ModeKind::Plan)
                    .await?;
                replace_first_text_item(&mut items, trimmed);
                op::user_input(items)
            }
            "logout" => {
                self.auth.logout().await?;
                return Err(Error::auth_required());
            }
            _ => op::user_input(items),
        };

        Ok(PromptSubmission::Submit { op: Box::new(op) })
    }

    async fn switch_collaboration_mode_and_emit(&mut self, kind: ModeKind) -> Result<(), Error> {
        self.apply_collaboration_mode_kind(kind).await?;
        self.maybe_emit_config_options_update().await;
        Ok(())
    }

    fn default_review_target(&self) -> ReviewTarget {
        self.state
            .review_base_branch()
            .map_or(ReviewTarget::UncommittedChanges, |branch| {
                ReviewTarget::BaseBranch {
                    branch: branch.to_string(),
                }
            })
    }

    fn goals_enabled(&self) -> bool {
        self.config.features.enabled(Feature::Goals)
    }

    async fn handle_fast_command(&mut self, args: &str) -> Result<String, Error> {
        if !self.fast_mode_configurable() {
            return Ok("Fast mode is not available.".to_string());
        }

        match args.trim().to_ascii_lowercase().as_str() {
            "" => {
                let service_tier = if matches!(
                    self.config
                        .service_tier
                        .as_deref()
                        .and_then(ServiceTier::from_request_value),
                    Some(ServiceTier::Fast)
                ) {
                    None
                } else {
                    Some(ServiceTier::Fast)
                };
                self.set_service_tier(service_tier).await?;
                self.maybe_emit_config_options_update().await;
                Ok(format_fast_mode_status(self.config.service_tier.as_deref()))
            }
            "on" => {
                self.set_service_tier(Some(ServiceTier::Fast)).await?;
                self.maybe_emit_config_options_update().await;
                Ok(format_fast_mode_status(self.config.service_tier.as_deref()))
            }
            "off" => {
                self.set_service_tier(None).await?;
                self.maybe_emit_config_options_update().await;
                Ok(format_fast_mode_status(self.config.service_tier.as_deref()))
            }
            "status" => Ok(format_fast_mode_status(self.config.service_tier.as_deref())),
            _ => Ok("Usage: /fast [on|off|status]".to_string()),
        }
    }

    async fn handle_goal_command(&self, args: &str) -> Result<String, Error> {
        if !self.goals_enabled() {
            return Ok("Goals feature is disabled.".to_string());
        }

        let args = args.trim();
        match args.to_ascii_lowercase().as_str() {
            "" => self.show_thread_goal().await,
            "clear" => self.clear_thread_goal().await,
            "pause" => {
                self.set_thread_goal_status(ThreadGoalStatus::Paused, "update")
                    .await
            }
            "resume" => {
                self.set_thread_goal_status(ThreadGoalStatus::Active, "update")
                    .await
            }
            _ => self.set_thread_goal_objective(args).await,
        }
    }

    async fn show_thread_goal(&self) -> Result<String, Error> {
        let Some(goal) = self.thread.thread_goal_get(self.thread_id).await? else {
            return Ok(format!("{GOAL_USAGE}\nNo goal is currently set."));
        };

        Ok(format_thread_goal_summary(&goal))
    }

    async fn clear_thread_goal(&self) -> Result<String, Error> {
        if self.thread.thread_goal_clear(self.thread_id).await? {
            Ok("Goal cleared".to_string())
        } else {
            Ok("No goal to clear\nThis thread does not currently have a goal.".to_string())
        }
    }

    async fn set_thread_goal_status(
        &self,
        status: ThreadGoalStatus,
        action: &'static str,
    ) -> Result<String, Error> {
        match self
            .thread
            .thread_goal_set(
                self.thread_id,
                ThreadGoalSetRequest {
                    status: Some(status),
                    ..ThreadGoalSetRequest::default()
                },
            )
            .await
        {
            Ok(goal) => Ok(format_thread_goal_set_message(&goal)),
            Err(err) => Ok(format!("Failed to {action} thread goal: {err}")),
        }
    }

    async fn set_thread_goal_objective(&self, objective: &str) -> Result<String, Error> {
        if let Err(message) = validate_thread_goal_objective(objective) {
            return Ok(format!("{message}\n\n{GOAL_USAGE}\n{GOAL_USAGE_HINT}"));
        }

        let goal = self
            .thread
            .thread_goal_set(
                self.thread_id,
                ThreadGoalSetRequest {
                    objective: Some(objective.to_string()),
                    status: Some(ThreadGoalStatus::Active),
                    ..ThreadGoalSetRequest::default()
                },
            )
            .await?;

        Ok(format_thread_goal_set_message(&goal))
    }
}

fn format_thread_goal_set_message(goal: &ThreadGoal) -> String {
    format!(
        "Goal {}\n{}",
        format_thread_goal_status_label(goal.status),
        format_thread_goal_usage_summary(goal)
    )
}

fn format_fast_mode_status(service_tier: Option<&str>) -> String {
    let status = if matches!(
        service_tier.and_then(ServiceTier::from_request_value),
        Some(ServiceTier::Fast)
    ) {
        "on"
    } else {
        "off"
    };
    format!("Fast mode is {status}.")
}
