use agent_client_protocol::schema::{ToolCall, ToolCallUpdate, ToolCallUpdateFields, ToolKind};
use codex_protocol::protocol::{GuardianAssessmentEvent, GuardianAssessmentStatus};

use crate::{
    boundary::{effect::BridgeEffect, raw},
    guardian::{
        guardian_assessment_content, guardian_assessment_tool_call_id,
        guardian_assessment_tool_call_status,
    },
};

pub(crate) enum GuardianAssessmentEffectPlan {
    InProgress {
        id: String,
        start: BridgeEffect,
        update: BridgeEffect,
    },
    Finished {
        id: String,
        start: BridgeEffect,
        update: BridgeEffect,
    },
}

pub(crate) fn guardian_assessment_effect_plan(
    event: GuardianAssessmentEvent,
) -> GuardianAssessmentEffectPlan {
    let id = event.id.clone();
    let call_id = guardian_assessment_tool_call_id(&event.id);
    let raw_event = raw::guardian_assessment(&event);
    let status = guardian_assessment_tool_call_status(&event.status);
    let content = guardian_assessment_content(&event);
    let start = BridgeEffect::tool_call(
        ToolCall::new(call_id.clone(), "Guardian Review")
            .kind(ToolKind::Think)
            .status(status)
            .content(content.clone())
            .raw_input(raw_event.clone()),
    );
    let update = BridgeEffect::tool_call_update(ToolCallUpdate::new(
        call_id,
        ToolCallUpdateFields::new()
            .status(status)
            .content(content)
            .raw_output(raw_event),
    ));

    match event.status {
        GuardianAssessmentStatus::InProgress => {
            GuardianAssessmentEffectPlan::InProgress { id, start, update }
        }
        GuardianAssessmentStatus::TimedOut
        | GuardianAssessmentStatus::Approved
        | GuardianAssessmentStatus::Denied
        | GuardianAssessmentStatus::Aborted => {
            GuardianAssessmentEffectPlan::Finished { id, start, update }
        }
    }
}
