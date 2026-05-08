use codex_protocol::protocol::GuardianAssessmentEvent;

use crate::boundary::{
    effect::BridgeEffect,
    tool_call::{self, GuardianAssessmentEffectPlan},
};

use super::submission::PromptState;

impl PromptState {
    pub(super) fn guardian_assessment(&mut self, event: GuardianAssessmentEvent) -> BridgeEffect {
        match tool_call::guardian_assessment_effect_plan(event) {
            GuardianAssessmentEffectPlan::InProgress { id, start, update } => {
                if self.insert_active_guardian_assessment(id) {
                    start
                } else {
                    update
                }
            }
            GuardianAssessmentEffectPlan::Finished { id, start, update } => {
                if self.remove_active_guardian_assessment(&id) {
                    update
                } else {
                    start
                }
            }
        }
    }
}
