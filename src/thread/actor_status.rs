use super::actor::ThreadActor;
use codex_git_utils::current_branch_name;
use codex_protocol::config_types::ServiceTier;
use itertools::Itertools;

use crate::{
    display::{format_collab_status_entry, format_token_count_compact},
    session_mode::{APPROVAL_PRESETS, current_session_mode_id},
};

use super::deps::Auth;

impl<A: Auth> ThreadActor<A> {
    pub(super) fn permission_preset_from_arg(input: &str) -> Option<&'static str> {
        match input.trim() {
            "read-only" | "readonly" | "read_only" => Some("read-only"),
            "auto" | "workspace" => Some("auto"),
            "full-access" | "fullaccess" | "danger" => Some("full-access"),
            _ => None,
        }
    }

    pub(super) fn current_approval_preset_summary(&self) -> String {
        let current = current_session_mode_id(&self.config)
            .map_or_else(|| "custom".to_string(), |mode| mode.0.to_string());
        let available = APPROVAL_PRESETS
            .iter()
            .map(|preset| format!("- {}: {}", preset.id, preset.description))
            .join("\n");
        format!("Approval preset: {current}\n\nAvailable presets:\n{available}")
    }

    pub(super) fn background_tasks_summary(&self) -> String {
        let tasks = self
            .active_command_summaries()
            .into_iter()
            .sorted()
            .collect::<Vec<_>>();

        if tasks.is_empty() {
            "No background tool calls are currently running.".to_string()
        } else {
            format!("Active tool calls:\n{}", tasks.join("\n"))
        }
    }

    pub(super) fn subagent_summary(&self) -> String {
        let collaboration_mode = self
            .state
            .collaboration_mode_kind_or_default()
            .display_name();

        if self.state.known_collab_agent_count() == 0 {
            return format!(
                "Collaboration mode: {collaboration_mode}\n\nNo known subagents are being tracked in this session.\nSpawned subagent activity will still appear inline when Codex uses it."
            );
        }

        let agents = self
            .state
            .known_collab_agents_sorted()
            .iter()
            .map(format_collab_status_entry)
            .join("\n");

        format!("Collaboration mode: {collaboration_mode}\n\nKnown subagents:\n{agents}")
    }

    pub(super) async fn current_status_summary(&self) -> String {
        let model = self.get_current_model().await;
        let reasoning_effort = self
            .config
            .model_reasoning_effort
            .as_ref()
            .map_or_else(|| "default".to_string(), |effort| effort.to_string());
        let approval_preset = current_session_mode_id(&self.config)
            .map_or_else(|| "custom".to_string(), |mode| mode.0.to_string());
        let collaboration_mode = self
            .state
            .collaboration_mode_kind_or_default()
            .display_name();
        let service_tier = format_service_tier(self.config.service_tier.as_deref());
        let review_target = self
            .state
            .review_base_branch()
            .map(|branch| format!("base branch {branch}"))
            .unwrap_or_else(|| "current changes".to_string());
        let git_branch = current_branch_name(&self.config.cwd)
            .await
            .unwrap_or_else(|| "unavailable".to_string());
        let pending_user_input = if self.state.has_pending_user_input() {
            "yes"
        } else {
            "no"
        };
        let active_tool_calls = self.active_command_count();
        let lines = vec![
            format!("Model: {model}"),
            format!("Reasoning effort: {reasoning_effort}"),
            format!("Approval preset: {approval_preset}"),
            format!("Collaboration mode: {collaboration_mode}"),
            format!("Service tier: {service_tier}"),
            format!("Git branch: {git_branch}"),
            format!("Review target: {review_target}"),
            format!("Configured MCP servers: {}", self.config.mcp_servers.len()),
            format!("Active tool calls: {active_tool_calls}"),
            format!("Known subagents: {}", self.state.known_collab_agent_count()),
            format!("Pending user input: {pending_user_input}"),
            format!("Working directory: {}", self.config.cwd.display()),
            "Run /usage for token, context window, and rate-limit details.".to_string(),
        ];
        lines.join("\n")
    }

    pub(super) fn current_usage_summary_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();

        let usage = self.state.latest_usage();
        if let Some(info) = usage.info() {
            let total = &info.total_token_usage;
            lines.push("Token usage:".to_string());
            lines.push(format!(
                "Total: {}",
                format_token_count_compact(total.blended_total()),
            ));
            lines.push(format!(
                "Input: {}",
                format_token_count_compact(total.non_cached_input()),
            ));
            lines.push(format!(
                "Output: {}",
                format_token_count_compact(total.output_tokens.max(0)),
            ));

            if total.cached_input() > 0 {
                lines.push(format!(
                    "Cached input: {}",
                    format_token_count_compact(total.cached_input())
                ));
            }
            if total.reasoning_output_tokens > 0 {
                lines.push(format!(
                    "Reasoning output: {}",
                    format_token_count_compact(total.reasoning_output_tokens)
                ));
            }
            if let Some(window) = info.model_context_window {
                let context = &info.last_token_usage;
                lines.push(String::new());
                lines.push("Context window:".to_string());
                lines.push(format!(
                    "Remaining: {}% left",
                    context.percent_of_context_window_remaining(window),
                ));
                lines.push(format!(
                    "Used: {} / {}",
                    format_token_count_compact(context.tokens_in_context_window()),
                    format_token_count_compact(window),
                ));
            }
        } else {
            lines.push("Token usage: not available yet".to_string());
            lines.push("Start a turn to populate token usage.".to_string());
        }

        if let Some(snapshot) = usage.rate_limits() {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.push("Rate limits:".to_string());
            if let Some(primary) = snapshot.primary.as_ref() {
                lines.push(format!(
                    "Primary: {:.0}% left",
                    (100.0 - primary.used_percent).clamp(0.0, 100.0)
                ));
            }
            if let Some(secondary) = snapshot.secondary.as_ref() {
                lines.push(format!(
                    "Secondary: {:.0}% left",
                    (100.0 - secondary.used_percent).clamp(0.0, 100.0)
                ));
            }
            if let Some(credits) = snapshot.credits.as_ref() {
                let credits_summary = if credits.unlimited {
                    "unlimited".to_string()
                } else if let Some(balance) = credits.balance.as_ref() {
                    balance.clone()
                } else if credits.has_credits {
                    "available".to_string()
                } else {
                    "unavailable".to_string()
                };
                lines.push(format!("Credits: {credits_summary}"));
            }
            lines.push("Details: https://chatgpt.com/codex/settings/usage".to_string());
        }

        lines
    }

    pub(super) fn current_usage_summary(&self) -> String {
        self.current_usage_summary_lines().join("\n")
    }
}

fn format_service_tier(service_tier: Option<&str>) -> &'static str {
    match service_tier.and_then(ServiceTier::from_request_value) {
        None => "default",
        Some(ServiceTier::Fast) => "fast",
        Some(ServiceTier::Flex) => "flex",
    }
}
