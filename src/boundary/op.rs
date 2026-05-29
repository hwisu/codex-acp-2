use codex_core::review_prompts::user_facing_hint;
use codex_protocol::{
    config_types::CollaborationMode,
    models::ActivePermissionProfile,
    openai_models::ReasoningEffort,
    protocol::{Op, ReviewRequest, ReviewTarget, ThreadSettingsOverrides},
    request_user_input::RequestUserInputResponse,
    user_input::UserInput,
};
use codex_utils_approval_presets::ApprovalPreset;

use crate::session_mode::active_profile_id_for_session_mode;

pub(crate) enum ReasoningEffortOverride {
    Set(ReasoningEffort),
    Clear,
}

impl ReasoningEffortOverride {
    pub(crate) fn from_selected_effort(effort: Option<ReasoningEffort>) -> Self {
        effort.map_or(Self::Clear, Self::Set)
    }

    fn into_op_value(self) -> Option<ReasoningEffort> {
        match self {
            Self::Set(effort) => Some(effort),
            Self::Clear => None,
        }
    }
}

pub(crate) fn user_input(items: Vec<UserInput>) -> Op {
    Op::UserInput {
        items,
        final_output_json_schema: None,
        environments: None,
        responsesapi_client_metadata: None,
        additional_context: Default::default(),
        thread_settings: ThreadSettingsOverrides::default(),
    }
}

pub(crate) fn user_input_answer(id: String, response: RequestUserInputResponse) -> Op {
    Op::UserInputAnswer { id, response }
}

pub(crate) fn compact() -> Op {
    Op::Compact
}

pub(crate) fn undo_last_turn() -> Op {
    Op::ThreadRollback { num_turns: 1 }
}

pub(crate) fn review(target: ReviewTarget) -> Op {
    Op::Review {
        review_request: ReviewRequest {
            user_facing_hint: Some(user_facing_hint(&target)),
            target,
        },
    }
}

pub(crate) fn interrupt() -> Op {
    Op::Interrupt
}

pub(crate) fn shutdown() -> Op {
    Op::Shutdown
}

pub(crate) fn override_model(model: Option<String>, effort: ReasoningEffortOverride) -> Op {
    Op::ThreadSettings {
        thread_settings: ThreadSettingsOverrides {
            model,
            effort: Some(effort.into_op_value()),
            ..ThreadSettingsOverrides::default()
        },
    }
}

pub(crate) fn override_approval_preset(preset: &ApprovalPreset) -> Op {
    Op::ThreadSettings {
        thread_settings: ThreadSettingsOverrides {
            approval_policy: Some(preset.approval),
            permission_profile: Some(preset.permission_profile.clone()),
            active_permission_profile: active_profile_id_for_session_mode(preset.id)
                .map(ActivePermissionProfile::new),
            ..ThreadSettingsOverrides::default()
        },
    }
}

pub(crate) fn override_collaboration_mode(collaboration_mode: CollaborationMode) -> Op {
    Op::ThreadSettings {
        thread_settings: ThreadSettingsOverrides {
            collaboration_mode: Some(collaboration_mode),
            ..ThreadSettingsOverrides::default()
        },
    }
}

pub(crate) fn override_service_tier(service_tier: Option<String>) -> Op {
    Op::ThreadSettings {
        thread_settings: ThreadSettingsOverrides {
            service_tier: Some(service_tier),
            ..ThreadSettingsOverrides::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_op_includes_user_facing_hint() {
        let op = review(ReviewTarget::UncommittedChanges);

        assert!(matches!(
            op,
            Op::Review {
                review_request: ReviewRequest {
                    user_facing_hint: Some(_),
                    target: ReviewTarget::UncommittedChanges,
                },
            }
        ));
    }

    #[test]
    fn override_model_can_clear_reasoning_effort() {
        let op = override_model(Some("gpt-5.4".to_string()), ReasoningEffortOverride::Clear);

        assert!(matches!(
            op,
            Op::ThreadSettings {
                thread_settings: ThreadSettingsOverrides {
                    model: Some(model),
                    effort: Some(None),
                    ..
                },
                ..
            } if model == "gpt-5.4"
        ));
    }
}
