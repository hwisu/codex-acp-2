use agent_client_protocol::schema::{
    ToolCall, ToolCallContent, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use codex_protocol::protocol::{
    PatchApplyBeginEvent, PatchApplyEndEvent, PatchApplyStatus, PatchApplyUpdatedEvent,
};

use crate::boundary::{
    effect::BridgeEffect,
    file_changes::{FileChangeRenderContext, extract_tool_call_content_from_changes},
    permission, raw,
};

pub(crate) fn patch_apply_begin_tool_call(event: PatchApplyBeginEvent) -> ToolCall {
    let raw_input = raw::patch_apply_begin(&event);
    let PatchApplyBeginEvent {
        call_id,
        auto_approved: _,
        changes,
        turn_id: _,
    } = event;

    let (title, locations, content) =
        extract_tool_call_content_from_changes(changes, FileChangeRenderContext::BeforeApply);
    let meta = permission::patch_edit_output_display_meta(locations.len());

    ToolCall::new(call_id, title)
        .kind(ToolKind::Edit)
        .status(ToolCallStatus::InProgress)
        .locations(locations)
        .content(content.collect::<Vec<ToolCallContent>>())
        .raw_input(raw_input)
        .meta(meta)
}

pub(crate) fn patch_apply_begin_effect(event: PatchApplyBeginEvent) -> BridgeEffect {
    BridgeEffect::tool_call(patch_apply_begin_tool_call(event))
}

pub(crate) fn patch_apply_updated_tool_call_update(
    event: PatchApplyUpdatedEvent,
) -> Option<ToolCallUpdate> {
    let raw_input = raw::patch_apply_updated(&event);
    let PatchApplyUpdatedEvent { call_id, changes } = event;

    if changes.is_empty() {
        return None;
    }

    let (title, locations, content) =
        extract_tool_call_content_from_changes(changes, FileChangeRenderContext::BeforeApply);
    let meta = permission::patch_edit_output_display_meta(locations.len());

    Some(
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
    )
}

pub(crate) fn patch_apply_updated_effect(event: PatchApplyUpdatedEvent) -> Option<BridgeEffect> {
    patch_apply_updated_tool_call_update(event).map(BridgeEffect::tool_call_update)
}

pub(crate) fn patch_apply_end_tool_call_update(event: PatchApplyEndEvent) -> ToolCallUpdate {
    let raw_output = raw::patch_apply_end(&event);
    let PatchApplyEndEvent {
        call_id,
        stdout: _,
        stderr: _,
        success,
        changes,
        turn_id: _,
        status,
    } = event;

    let status = patch_apply_tool_call_status(status, success);
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
        let meta = permission::patch_edit_output_display_meta(locations.len());
        (
            Some(title),
            Some(locations),
            Some(content.collect::<Vec<ToolCallContent>>()),
            Some(meta),
        )
    };

    ToolCallUpdate::new(
        call_id,
        ToolCallUpdateFields::new()
            .status(status)
            .raw_output(raw_output)
            .title(title)
            .locations(locations)
            .content(content),
    )
    .meta(meta)
}

pub(crate) fn patch_apply_end_effect(event: PatchApplyEndEvent) -> BridgeEffect {
    BridgeEffect::tool_call_update(patch_apply_end_tool_call_update(event))
}

fn patch_apply_tool_call_status(status: PatchApplyStatus, success: bool) -> ToolCallStatus {
    match status {
        PatchApplyStatus::Completed => ToolCallStatus::Completed,
        PatchApplyStatus::Failed if success => ToolCallStatus::Completed,
        PatchApplyStatus::Declined if success => ToolCallStatus::Completed,
        PatchApplyStatus::Failed | PatchApplyStatus::Declined => ToolCallStatus::Failed,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use agent_client_protocol::schema::ToolCallStatus;
    use codex_protocol::protocol::{PatchApplyEndEvent, PatchApplyStatus, PatchApplyUpdatedEvent};

    use super::{patch_apply_end_tool_call_update, patch_apply_updated_tool_call_update};

    #[test]
    fn patch_apply_updated_skips_empty_changes() {
        let update = patch_apply_updated_tool_call_update(PatchApplyUpdatedEvent {
            call_id: "patch-call".to_string(),
            changes: HashMap::new(),
        });

        assert!(update.is_none());
    }

    #[test]
    fn patch_apply_end_uses_success_fallback_for_completed_status() {
        let update = patch_apply_end_tool_call_update(PatchApplyEndEvent {
            call_id: "patch-call".to_string(),
            turn_id: "turn-id".to_string(),
            stdout: String::new(),
            stderr: String::new(),
            success: true,
            changes: HashMap::new(),
            status: PatchApplyStatus::Failed,
        });

        assert_eq!(update.tool_call_id.0.as_ref(), "patch-call");
        assert_eq!(update.fields.status, Some(ToolCallStatus::Completed));
    }

    #[test]
    fn patch_apply_end_marks_declined_as_failed() {
        let update = patch_apply_end_tool_call_update(PatchApplyEndEvent {
            call_id: "patch-call".to_string(),
            turn_id: "turn-id".to_string(),
            stdout: String::new(),
            stderr: String::new(),
            success: false,
            changes: HashMap::new(),
            status: PatchApplyStatus::Declined,
        });

        assert_eq!(update.fields.status, Some(ToolCallStatus::Failed));
    }
}
