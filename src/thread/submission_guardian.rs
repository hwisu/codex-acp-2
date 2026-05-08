use agent_client_protocol::schema::{ToolCall, ToolCallUpdate, ToolCallUpdateFields, ToolKind};
use codex_protocol::protocol::{GuardianAssessmentEvent, GuardianAssessmentStatus};

use crate::guardian::{
    guardian_assessment_content, guardian_assessment_tool_call_id,
    guardian_assessment_tool_call_status,
};

use super::{client::SessionClient, submission::PromptState};

impl PromptState {
    pub(super) fn guardian_assessment(
        &mut self,
        client: &SessionClient,
        event: GuardianAssessmentEvent,
    ) {
        let call_id = guardian_assessment_tool_call_id(&event.id);
        let status = guardian_assessment_tool_call_status(&event.status);
        let content = guardian_assessment_content(&event);
        let raw_event = serde_json::json!(&event);

        match event.status {
            GuardianAssessmentStatus::InProgress => {
                if self.insert_active_guardian_assessment(event.id.clone()) {
                    client.send_tool_call(
                        ToolCall::new(call_id, "Guardian Review")
                            .kind(ToolKind::Think)
                            .status(status)
                            .content(content)
                            .raw_input(raw_event),
                    );
                } else {
                    client.send_tool_call_update(ToolCallUpdate::new(
                        call_id,
                        ToolCallUpdateFields::new()
                            .status(status)
                            .content(content)
                            .raw_output(raw_event),
                    ));
                }
            }
            GuardianAssessmentStatus::TimedOut
            | GuardianAssessmentStatus::Approved
            | GuardianAssessmentStatus::Denied
            | GuardianAssessmentStatus::Aborted => {
                if self.remove_active_guardian_assessment(&event.id) {
                    client.send_tool_call_update(ToolCallUpdate::new(
                        call_id,
                        ToolCallUpdateFields::new()
                            .status(status)
                            .content(content)
                            .raw_output(raw_event),
                    ));
                } else {
                    client.send_tool_call(
                        ToolCall::new(call_id, "Guardian Review")
                            .kind(ToolKind::Think)
                            .status(status)
                            .content(content)
                            .raw_input(raw_event),
                    );
                }
            }
        }
    }
}
