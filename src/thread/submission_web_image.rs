use agent_client_protocol::schema::{
    ToolCall, ToolCallLocation, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use codex_protocol::{
    models::WebSearchAction,
    protocol::{ImageGenerationBeginEvent, ImageGenerationEndEvent},
};

use crate::display::format_image_generation_content;

use super::{client::SessionClient, submission::PromptState};

impl PromptState {
    pub(super) fn start_web_search(&mut self, client: &SessionClient, call_id: String) {
        self.start_active_web_search(call_id.clone());
        client.send_tool_call(ToolCall::new(call_id, "Searching the Web").kind(ToolKind::Fetch));
    }

    pub(super) fn update_web_search_query(
        client: &SessionClient,
        call_id: String,
        query: String,
        action: WebSearchAction,
    ) {
        let title = match &action {
            WebSearchAction::Search { query, queries } => queries.as_ref().map_or_else(
                || {
                    query.as_ref().map_or_else(
                        || "Web search".to_string(),
                        |q| format!("Searching for: {q}"),
                    )
                },
                |q| format!("Searching for: {}", q.join(", ")),
            ),
            WebSearchAction::OpenPage { url } => url
                .as_ref()
                .map_or_else(|| "Open page".to_string(), |u| format!("Opening: {u}")),
            WebSearchAction::FindInPage { pattern, url } => match (pattern, url) {
                (Some(p), Some(u)) => format!("Finding: {p} in {u}"),
                (Some(p), None) => format!("Finding: {p}"),
                (None, Some(u)) => format!("Find in page: {u}"),
                (None, None) => "Find in page".to_string(),
            },
            WebSearchAction::Other => "Web search".to_string(),
        };

        client.send_tool_call_update(ToolCallUpdate::new(
            call_id,
            ToolCallUpdateFields::new()
                .status(ToolCallStatus::InProgress)
                .title(title)
                .raw_input(serde_json::json!({
                    "query": query,
                    "action": action
                })),
        ));
    }

    pub(super) fn complete_web_search(&mut self, client: &SessionClient) {
        if let Some(call_id) = self.take_active_web_search() {
            client.send_tool_call_update(ToolCallUpdate::new(
                call_id,
                ToolCallUpdateFields::new().status(ToolCallStatus::Completed),
            ));
        }
    }

    pub(super) fn image_generation_begin(client: &SessionClient, event: ImageGenerationBeginEvent) {
        client.send_tool_call(
            ToolCall::new(event.call_id, "Generate image")
                .kind(ToolKind::Other)
                .status(ToolCallStatus::InProgress),
        );
    }

    pub(super) fn image_generation_end(client: &SessionClient, event: ImageGenerationEndEvent) {
        let raw_output = serde_json::json!(&event);
        let call_id = event.call_id.clone();
        let status = if event.status == "completed" {
            ToolCallStatus::Completed
        } else {
            ToolCallStatus::Failed
        };
        let locations = event
            .saved_path
            .clone()
            .map(|path| vec![ToolCallLocation::new(path)]);
        let content = format_image_generation_content(&event);

        client.send_tool_call_update(ToolCallUpdate::new(
            call_id,
            ToolCallUpdateFields::new()
                .status(status)
                .title(Some("Generate image".to_string()))
                .locations(locations)
                .content((!content.is_empty()).then_some(content))
                .raw_output(raw_output),
        ));
    }
}
