use agent_client_protocol::schema::v1::{
    Content, ContentBlock, ResourceLink, ToolCall, ToolCallContent, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields,
};
use codex_protocol::{
    dynamic_tools::DynamicToolCallOutputContentItem, protocol::DynamicToolCallResponseEvent,
};

use crate::boundary::{effect::BridgeEffect, raw};

pub(crate) fn dynamic_tool_call_begin(
    call_id: String,
    tool: &str,
    arguments: &serde_json::Value,
) -> ToolCall {
    ToolCall::new(call_id, format!("Tool: {tool}"))
        .status(ToolCallStatus::InProgress)
        .raw_input(raw::dynamic_tool_arguments(arguments))
}

pub(crate) fn dynamic_tool_call_begin_effect(
    call_id: String,
    tool: &str,
    arguments: &serde_json::Value,
) -> BridgeEffect {
    BridgeEffect::tool_call(dynamic_tool_call_begin(call_id, tool, arguments))
}

pub(crate) fn dynamic_tool_call_end(event: DynamicToolCallResponseEvent) -> ToolCallUpdate {
    let raw_output = raw::dynamic_tool_response(&event);
    let DynamicToolCallResponseEvent {
        call_id,
        turn_id: _,
        tool: _,
        arguments: _,
        namespace: _,
        content_items,
        success,
        error,
        duration: _,
        ..
    } = event;

    ToolCallUpdate::new(
        call_id,
        ToolCallUpdateFields::new()
            .status(if success {
                ToolCallStatus::Completed
            } else {
                ToolCallStatus::Failed
            })
            .raw_output(raw_output)
            .content(dynamic_tool_call_content(content_items, error)),
    )
}

pub(crate) fn dynamic_tool_call_end_effect(event: DynamicToolCallResponseEvent) -> BridgeEffect {
    BridgeEffect::tool_call_update(dynamic_tool_call_end(event))
}

fn dynamic_tool_call_content(
    content_items: Vec<DynamicToolCallOutputContentItem>,
    error: Option<String>,
) -> Vec<ToolCallContent> {
    content_items
        .into_iter()
        .map(|item| match item {
            DynamicToolCallOutputContentItem::InputText { text } => {
                ToolCallContent::Content(Content::new(text))
            }
            DynamicToolCallOutputContentItem::InputImage { image_url } => {
                ToolCallContent::Content(Content::new(ContentBlock::ResourceLink(
                    ResourceLink::new(image_url.clone(), image_url),
                )))
            }
        })
        .chain(error.map(|e| ToolCallContent::Content(Content::new(e))))
        .collect()
}

#[cfg(test)]
mod tests {
    use agent_client_protocol::schema::v1::ToolCallStatus;
    use codex_protocol::protocol::DynamicToolCallResponseEvent;

    use super::dynamic_tool_call_end;

    #[test]
    fn dynamic_tool_call_end_marks_failed_when_unsuccessful() {
        let update = dynamic_tool_call_end(DynamicToolCallResponseEvent {
            call_id: "dynamic-call".to_string(),
            turn_id: "turn-id".to_string(),
            completed_at_ms: 0,
            tool: "lookup".to_string(),
            arguments: serde_json::json!({ "query": "x" }),
            namespace: None,
            content_items: Vec::new(),
            success: false,
            error: Some("failed".to_string()),
            duration: std::time::Duration::from_millis(1),
        });

        assert_eq!(update.tool_call_id.0.as_ref(), "dynamic-call");
        assert_eq!(update.fields.status, Some(ToolCallStatus::Failed));
    }
}
