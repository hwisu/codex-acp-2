use std::collections::HashMap;

use agent_client_protocol::schema::{
    PermissionOption, PermissionOptionKind, ToolCall, ToolCallContent, ToolCallStatus,
    ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use codex_core::review_format::format_review_findings_block;
use codex_protocol::protocol::{
    ApplyPatchApprovalRequestEvent, ExitedReviewModeEvent, PatchApplyBeginEvent,
    PatchApplyEndEvent, PatchApplyStatus, PatchApplyUpdatedEvent, ReviewDecision,
    ReviewOutputEvent,
};

use crate::display::tool_call_output_display_meta;

use super::{
    approvals::{PendingPermissionRequest, patch_request_key},
    client::SessionClient,
    file_changes::{FileChangeRenderContext, extract_tool_call_content_from_changes},
    submission::{PermissionInteractionRequest, PromptState},
};

const MAX_DEFAULT_OPEN_EDIT_FILES: usize = 3;

impl PromptState {
    pub(super) fn review_mode_exit(client: &SessionClient, event: ExitedReviewModeEvent) {
        let ExitedReviewModeEvent { review_output } = event;
        let Some(ReviewOutputEvent {
            findings,
            overall_correctness: _,
            overall_explanation,
            overall_confidence_score: _,
        }) = review_output
        else {
            return;
        };

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

        client.send_agent_text(&text);
    }

    pub(super) fn patch_approval(
        &mut self,
        client: &SessionClient,
        event: ApplyPatchApprovalRequestEvent,
    ) {
        let raw_input = serde_json::json!(&event);
        let ApplyPatchApprovalRequestEvent {
            call_id,
            changes,
            reason,
            // grant_root doesn't seem to be set anywhere on the codex side
            grant_root: _,
            turn_id: _,
        } = event;
        let (title, locations, content) =
            extract_tool_call_content_from_changes(changes, FileChangeRenderContext::BeforeApply);
        let mut content = content.collect::<Vec<ToolCallContent>>();
        let meta = edit_output_display_meta(locations.len());
        if let Some(reason) = reason {
            content.push(reason.into());
        }
        let request_key = patch_request_key(&call_id);
        let options = vec![
            PermissionOption::new("approved", "Yes", PermissionOptionKind::AllowOnce),
            PermissionOption::new(
                "denied",
                "No, continue without these edits",
                PermissionOptionKind::RejectOnce,
            ),
        ];
        self.spawn_permission_request(
            client,
            PermissionInteractionRequest {
                request_key,
                pending_request: PendingPermissionRequest::Patch {
                    call_id: call_id.clone(),
                    option_map: HashMap::from([
                        ("approved".to_string(), ReviewDecision::Approved),
                        ("denied".to_string(), ReviewDecision::Denied),
                    ]),
                },
                tool_call: ToolCallUpdate::new(
                    call_id,
                    ToolCallUpdateFields::new()
                        .kind(ToolKind::Edit)
                        .status(ToolCallStatus::Pending)
                        .title(title)
                        .locations(locations)
                        .content(content)
                        .raw_input(raw_input),
                )
                .meta(meta),
                options,
            },
        );
    }

    pub(super) fn start_patch_apply(client: &SessionClient, event: PatchApplyBeginEvent) {
        let raw_input = serde_json::json!(&event);
        let PatchApplyBeginEvent {
            call_id,
            auto_approved: _,
            changes,
            turn_id: _,
        } = event;

        let (title, locations, content) =
            extract_tool_call_content_from_changes(changes, FileChangeRenderContext::BeforeApply);
        let meta = edit_output_display_meta(locations.len());

        client.send_tool_call(
            ToolCall::new(call_id, title)
                .kind(ToolKind::Edit)
                .status(ToolCallStatus::InProgress)
                .locations(locations)
                .content(content.collect::<Vec<ToolCallContent>>())
                .raw_input(raw_input)
                .meta(meta),
        );
    }

    pub(super) fn update_patch_apply(client: &SessionClient, event: PatchApplyUpdatedEvent) {
        let raw_input = serde_json::json!(&event);
        let PatchApplyUpdatedEvent { call_id, changes } = event;

        if changes.is_empty() {
            return;
        }

        let (title, locations, content) =
            extract_tool_call_content_from_changes(changes, FileChangeRenderContext::BeforeApply);
        let meta = edit_output_display_meta(locations.len());

        client.send_tool_call_update(
            ToolCallUpdate::new(
                call_id,
                ToolCallUpdateFields::new()
                    .kind(ToolKind::Edit)
                    .status(ToolCallStatus::InProgress)
                    .title(title)
                    .locations(locations)
                    .content(content.collect::<Vec<ToolCallContent>>())
                    .raw_input(raw_input),
            )
            .meta(meta),
        );
    }

    pub(super) fn end_patch_apply(client: &SessionClient, event: PatchApplyEndEvent) {
        let raw_output = serde_json::json!(&event);
        let PatchApplyEndEvent {
            call_id,
            stdout: _,
            stderr: _,
            success,
            changes,
            turn_id: _,
            status,
        } = event;

        let status = match status {
            PatchApplyStatus::Completed => ToolCallStatus::Completed,
            _ if success => ToolCallStatus::Completed,
            PatchApplyStatus::Failed | PatchApplyStatus::Declined => ToolCallStatus::Failed,
        };
        let render_context = if status == ToolCallStatus::Completed {
            FileChangeRenderContext::AfterApply
        } else {
            FileChangeRenderContext::BeforeApply
        };

        let (title, locations, content, meta) = if changes.is_empty() {
            (None, None, None, None)
        } else {
            let (title, locations, content) =
                extract_tool_call_content_from_changes(changes, render_context);
            let meta = edit_output_display_meta(locations.len());
            (
                Some(title),
                Some(locations),
                Some(content.collect::<Vec<ToolCallContent>>()),
                Some(meta),
            )
        };

        client.send_tool_call_update(
            ToolCallUpdate::new(
                call_id,
                ToolCallUpdateFields::new()
                    .status(status)
                    .raw_output(raw_output)
                    .title(title)
                    .locations(locations)
                    .content(content),
            )
            .meta(meta),
        );
    }
}

fn edit_output_display_meta(file_count: usize) -> agent_client_protocol::schema::Meta {
    let default_open = file_count <= MAX_DEFAULT_OPEN_EDIT_FILES;
    tool_call_output_display_meta(
        default_open,
        if default_open {
            "smallFileEdit"
        } else {
            "manyFileEdits"
        },
    )
}

#[cfg(test)]
mod tests {
    use agent_client_protocol::schema::Meta;

    use super::*;

    fn default_open(meta: &Meta) -> Option<bool> {
        meta.get("codex_acp")
            .and_then(|value| value.get("toolCallOutput"))
            .and_then(|value| value.get("defaultOpen"))
            .and_then(serde_json::Value::as_bool)
    }

    #[test]
    fn edit_output_display_meta_opens_small_file_edits() {
        let meta = edit_output_display_meta(2);

        assert_eq!(default_open(&meta), Some(true));
    }

    #[test]
    fn edit_output_display_meta_folds_many_file_edits() {
        let meta = edit_output_display_meta(MAX_DEFAULT_OPEN_EDIT_FILES + 1);

        assert_eq!(default_open(&meta), Some(false));
    }
}
