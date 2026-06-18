use agent_client_protocol::schema::{
    Content, ContentBlock, ResourceLink, TextContent, ToolCallContent,
};
use codex_protocol::ThreadId;
use codex_protocol::protocol::{
    AgentStatus, CollabAgentRef, CollabAgentStatusEntry, ImageGenerationEndEvent, ThreadGoal,
    ThreadGoalStatus, ThreadGoalUpdatedEvent,
};

use itertools::Itertools as _;

pub(crate) fn format_thread_goal_update(event: &ThreadGoalUpdatedEvent) -> String {
    let status = match event.goal.status {
        ThreadGoalStatus::Active => "active",
        ThreadGoalStatus::Paused => "paused",
        ThreadGoalStatus::Blocked => "blocked",
        ThreadGoalStatus::UsageLimited => "usage limited",
        ThreadGoalStatus::BudgetLimited => "budget limited",
        ThreadGoalStatus::Complete => "complete",
    };

    let objective = event.goal.objective.trim();
    if objective.contains('\n') {
        format!("Goal updated ({status}):\n{objective}")
    } else {
        format!("Goal updated ({status}): {objective}")
    }
}

pub(crate) fn format_goal_elapsed_seconds(seconds: i64) -> String {
    let seconds = seconds.max(0) as u64;
    if seconds < 60 {
        return format!("{seconds}s");
    }

    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m");
    }

    let hours = minutes / 60;
    let remaining_minutes = minutes % 60;
    if hours >= 24 {
        let days = hours / 24;
        let remaining_hours = hours % 24;
        return format!("{days}d {remaining_hours}h {remaining_minutes}m");
    }

    if remaining_minutes == 0 {
        format!("{hours}h")
    } else {
        format!("{hours}h {remaining_minutes}m")
    }
}

pub(crate) fn format_thread_goal_status_label(status: ThreadGoalStatus) -> &'static str {
    match status {
        ThreadGoalStatus::Active => "active",
        ThreadGoalStatus::Paused => "paused",
        ThreadGoalStatus::Blocked => "blocked",
        ThreadGoalStatus::UsageLimited => "limited by usage",
        ThreadGoalStatus::BudgetLimited => "limited by budget",
        ThreadGoalStatus::Complete => "complete",
    }
}

pub(crate) fn format_thread_goal_usage_summary(goal: &ThreadGoal) -> String {
    let mut parts = vec![format!("Objective: {}", goal.objective.trim())];
    if goal.time_used_seconds > 0 {
        parts.push(format!(
            "Time: {}.",
            format_goal_elapsed_seconds(goal.time_used_seconds)
        ));
    }
    if let Some(token_budget) = goal.token_budget {
        parts.push(format!(
            "Tokens: {}/{}.",
            format_token_count_compact(goal.tokens_used),
            format_token_count_compact(token_budget)
        ));
    }
    parts.join(" ")
}

pub(crate) fn format_thread_goal_summary(goal: &ThreadGoal) -> String {
    let mut lines = vec![
        "Goal".to_string(),
        format!("Status: {}", format_thread_goal_status_label(goal.status)),
        format!("Objective: {}", goal.objective.trim()),
        format!(
            "Time used: {}",
            format_goal_elapsed_seconds(goal.time_used_seconds)
        ),
        format!(
            "Tokens used: {}",
            format_token_count_compact(goal.tokens_used)
        ),
    ];
    if let Some(token_budget) = goal.token_budget {
        lines.push(format!(
            "Token budget: {}",
            format_token_count_compact(token_budget)
        ));
    }
    lines.push(String::new());
    lines.push(match goal.status {
        ThreadGoalStatus::Active => "Commands: /goal pause, /goal clear".to_string(),
        ThreadGoalStatus::Paused => "Commands: /goal resume, /goal clear".to_string(),
        ThreadGoalStatus::Blocked
        | ThreadGoalStatus::UsageLimited
        | ThreadGoalStatus::BudgetLimited
        | ThreadGoalStatus::Complete => "Commands: /goal clear".to_string(),
    });
    lines.join("\n")
}

pub(crate) fn preview_text(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    let mut preview = trimmed.chars().take(max_chars).collect::<String>();
    if trimmed.chars().count() > max_chars {
        preview.push_str("...");
    }
    preview
}

pub(crate) fn format_agent_status(status: &AgentStatus) -> String {
    match status {
        AgentStatus::PendingInit => "pending init".to_string(),
        AgentStatus::Running => "running".to_string(),
        AgentStatus::Interrupted => "interrupted".to_string(),
        AgentStatus::Completed(Some(message)) if !message.trim().is_empty() => {
            format!("completed: {}", preview_text(message, 120))
        }
        AgentStatus::Completed(None) | AgentStatus::Completed(Some(_)) => "completed".to_string(),
        AgentStatus::Errored(message) => format!("errored: {}", preview_text(message, 120)),
        AgentStatus::Shutdown => "shutdown".to_string(),
        AgentStatus::NotFound => "not found".to_string(),
    }
}

pub(crate) fn format_agent_label(
    nickname: Option<&str>,
    role: Option<&str>,
    thread_id: Option<&ThreadId>,
) -> String {
    let nickname = nickname.map(str::trim).filter(|value| !value.is_empty());
    let role = role.map(str::trim).filter(|value| !value.is_empty());
    let base = match (nickname, role) {
        (Some(nickname), Some(role)) => format!("{nickname} [{role}]"),
        (Some(nickname), None) => nickname.to_string(),
        (None, Some(role)) => format!("Agent [{role}]"),
        (None, None) => "Agent".to_string(),
    };

    thread_id
        .map(|thread_id| format!("{base} ({thread_id})"))
        .unwrap_or(base)
}

pub(crate) fn format_collab_agent_ref(agent: &CollabAgentRef) -> String {
    format_agent_label(
        agent.agent_nickname.as_deref(),
        agent.agent_role.as_deref(),
        Some(&agent.thread_id),
    )
}

pub(crate) fn format_collab_status_entry(entry: &CollabAgentStatusEntry) -> String {
    format!(
        "{}: {}",
        format_agent_label(
            entry.agent_nickname.as_deref(),
            entry.agent_role.as_deref(),
            Some(&entry.thread_id),
        ),
        format_agent_status(&entry.status)
    )
}

pub(crate) fn format_collab_prompt(prompt: &str) -> Option<String> {
    let prompt = prompt.trim();
    (!prompt.is_empty()).then(|| format!("Prompt: {}", preview_text(prompt, 160)))
}

pub(crate) fn format_collab_waiting_statuses(entries: &[CollabAgentStatusEntry]) -> Option<String> {
    (!entries.is_empty()).then(|| {
        let lines = entries.iter().map(format_collab_status_entry).join("\n");
        format!("Agents:\n{lines}")
    })
}

pub(crate) fn tool_call_text_content(text: impl Into<String>) -> ToolCallContent {
    ToolCallContent::Content(Content::new(ContentBlock::Text(TextContent::new(
        text.into(),
    ))))
}

pub(crate) fn format_token_count_compact(value: i64) -> String {
    let abs = value.unsigned_abs();
    let sign = if value < 0 { "-" } else { "" };

    if abs >= 1_000_000 {
        format!("{sign}{:.1}M", abs as f64 / 1_000_000.0)
    } else if abs >= 1_000 {
        format!("{sign}{:.1}k", abs as f64 / 1_000.0)
    } else {
        format!("{sign}{abs}")
    }
}

pub(crate) fn format_image_generation_content(
    event: &ImageGenerationEndEvent,
) -> Vec<ToolCallContent> {
    let mut content = Vec::new();

    if let Some(revised_prompt) = event.revised_prompt.as_ref()
        && !revised_prompt.trim().is_empty()
    {
        content.push(tool_call_text_content(format!(
            "Revised prompt: {revised_prompt}"
        )));
    }

    if let Some(saved_path) = event.saved_path.as_ref() {
        let display_path = saved_path.display().to_string();
        content.push(ToolCallContent::Content(Content::new(
            ContentBlock::ResourceLink(ResourceLink::new(
                display_path.clone(),
                display_path.clone(),
            )),
        )));
    } else if !event.result.trim().is_empty() {
        content.push(tool_call_text_content(preview_text(&event.result, 400)));
    }

    content
}
