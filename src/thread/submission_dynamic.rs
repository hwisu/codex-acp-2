use agent_client_protocol::schema::{
    Content, ContentBlock, ResourceLink, ToolCall, ToolCallContent, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields,
};
use codex_protocol::{
    dynamic_tools::DynamicToolCallOutputContentItem, protocol::DynamicToolCallResponseEvent,
};

use super::{client::SessionClient, submission::PromptState};

impl PromptState {
    pub(super) fn start_dynamic_tool_call(
        client: &SessionClient,
        call_id: String,
        tool: &str,
        arguments: &serde_json::Value,
    ) {
        client.send_tool_call(
            ToolCall::new(call_id, format!("Tool: {tool}"))
                .status(ToolCallStatus::InProgress)
                .raw_input(serde_json::json!(arguments)),
        );
    }
    pub(super) fn end_dynamic_tool_call(
        client: &SessionClient,
        event: DynamicToolCallResponseEvent,
    ) {
        let raw_output = serde_json::json!(event);
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

        client.send_tool_call_update(ToolCallUpdate::new(
            call_id,
            ToolCallUpdateFields::new()
                .status(if success {
                    ToolCallStatus::Completed
                } else {
                    ToolCallStatus::Failed
                })
                .raw_output(raw_output)
                .content(
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
                        .collect::<Vec<_>>(),
                ),
        ));
    }
}
