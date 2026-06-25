use agent_client_protocol::schema::v1::{
    AvailableCommand, AvailableCommandsUpdate, ConfigOptionUpdate, ContentChunk, Meta, Plan,
    PlanEntry, PlanEntryPriority, PlanEntryStatus, SessionConfigOption, SessionUpdate, UsageUpdate,
};
use codex_core::review_format::format_review_findings_block;
use codex_protocol::{
    plan_tool::{PlanItemArg, StepStatus},
    protocol::{
        ExitedReviewModeEvent, ReviewOutputEvent, ThreadGoalUpdatedEvent, ThreadRolledBackEvent,
        TokenCountEvent, WarningEvent,
    },
};
use serde_json::json;

use crate::{
    boundary::{
        constants::meta,
        effect::{BridgeEffect, IgnoredCodexEventReason},
    },
    display::format_thread_goal_update,
};

pub(crate) fn user_message(text: impl Into<String>) -> SessionUpdate {
    SessionUpdate::UserMessageChunk(ContentChunk::new(text.into().into()))
}

pub(crate) fn user_message_effect(text: impl Into<String>) -> BridgeEffect {
    BridgeEffect::Forward(user_message(text))
}

pub(crate) fn agent_text(text: impl Into<String>) -> SessionUpdate {
    SessionUpdate::AgentMessageChunk(ContentChunk::new(text.into().into()))
}

pub(crate) fn agent_text_effect(text: impl Into<String>) -> BridgeEffect {
    BridgeEffect::Forward(agent_text(text))
}

pub(crate) fn agent_warning(WarningEvent { message }: WarningEvent) -> SessionUpdate {
    SessionUpdate::AgentMessageChunk(ContentChunk::new(message.into()).meta(Meta::from_iter([(
        meta::CODEX_ACP.to_string(),
        json!({ meta::KIND: meta::WARNING_KIND }),
    )])))
}

pub(crate) fn agent_warning_effect(event: WarningEvent) -> BridgeEffect {
    BridgeEffect::Forward(agent_warning(event))
}

pub(crate) fn agent_thought(text: impl Into<String>) -> SessionUpdate {
    SessionUpdate::AgentThoughtChunk(ContentChunk::new(text.into().into()))
}

pub(crate) fn agent_thought_effect(text: impl Into<String>) -> BridgeEffect {
    BridgeEffect::Forward(agent_thought(text))
}

pub(crate) fn plan(plan: Vec<PlanItemArg>) -> SessionUpdate {
    SessionUpdate::Plan(Plan::new(
        plan.into_iter()
            .map(|entry| {
                PlanEntry::new(
                    entry.step,
                    PlanEntryPriority::Medium,
                    match entry.status {
                        StepStatus::Pending => PlanEntryStatus::Pending,
                        StepStatus::InProgress => PlanEntryStatus::InProgress,
                        StepStatus::Completed => PlanEntryStatus::Completed,
                    },
                )
            })
            .collect(),
    ))
}

pub(crate) fn plan_effect(plan: Vec<PlanItemArg>) -> BridgeEffect {
    BridgeEffect::Forward(self::plan(plan))
}

pub(crate) fn usage(
    TokenCountEvent {
        info,
        rate_limits: _,
    }: TokenCountEvent,
) -> Result<SessionUpdate, IgnoredCodexEventReason> {
    let info = info
        .as_ref()
        .ok_or(IgnoredCodexEventReason::MissingUsageInfo)?;
    let context_window = info
        .model_context_window
        .ok_or(IgnoredCodexEventReason::MissingUsageContextWindow)?;
    let size = u64::try_from(context_window)
        .map_err(|_| IgnoredCodexEventReason::InvalidUsageContextWindow)?;
    let used = u64::try_from(info.last_token_usage.tokens_in_context_window()).unwrap_or_default();

    Ok(SessionUpdate::UsageUpdate(
        UsageUpdate::new(used, size).meta(Meta::from_iter([(
            meta::TOKEN_USAGE.to_string(),
            json!({
                "total": &info.total_token_usage,
                "last": &info.last_token_usage,
            }),
        )])),
    ))
}

pub(crate) fn usage_effect(
    event: TokenCountEvent,
) -> Result<BridgeEffect, IgnoredCodexEventReason> {
    usage(event).map(BridgeEffect::Forward)
}

pub(crate) fn review_mode_exit_effect(
    ExitedReviewModeEvent { review_output }: ExitedReviewModeEvent,
) -> Result<BridgeEffect, IgnoredCodexEventReason> {
    let ReviewOutputEvent {
        findings,
        overall_correctness: _,
        overall_explanation,
        overall_confidence_score: _,
    } = review_output.ok_or(IgnoredCodexEventReason::MissingReviewOutput)?;

    let text = if findings.is_empty() {
        let explanation = overall_explanation.trim();
        if explanation.is_empty() {
            "Reviewer failed to output a response"
        } else {
            explanation
        }
        .to_string()
    } else {
        format_review_findings_block(&findings, None)
    };

    Ok(agent_text_effect(text))
}

pub(crate) fn thread_goal_updated(event: &ThreadGoalUpdatedEvent) -> BridgeEffect {
    agent_text_effect(format_thread_goal_update(event))
}

pub(crate) fn thread_rolled_back(
    ThreadRolledBackEvent { num_turns }: ThreadRolledBackEvent,
) -> BridgeEffect {
    if num_turns == 1 {
        agent_text_effect("Undo completed.")
    } else {
        agent_text_effect(format!("Rolled back {num_turns} turns."))
    }
}

pub(crate) fn context_compacted() -> BridgeEffect {
    agent_text_effect("Context compacted\n")
}

pub(crate) fn available_commands(commands: Vec<AvailableCommand>) -> SessionUpdate {
    SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(commands))
}

pub(crate) fn config_options(options: Vec<SessionConfigOption>) -> SessionUpdate {
    SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(options))
}

pub(crate) fn config_options_effect(options: Vec<SessionConfigOption>) -> BridgeEffect {
    BridgeEffect::Forward(config_options(options))
}
