use agent_client_protocol::schema::v1::{
    Content, ContentBlock, ResourceLink, ToolCall, ToolCallContent, ToolCallLocation,
    ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use codex_protocol::{
    models::WebSearchAction,
    protocol::{ImageGenerationBeginEvent, ImageGenerationEndEvent, ViewImageToolCallEvent},
};

use crate::{
    boundary::{effect::BridgeEffect, raw},
    display::format_image_generation_content,
};

pub(crate) fn web_search_begin_tool_call(call_id: String) -> ToolCall {
    ToolCall::new(call_id, "Searching the Web").kind(ToolKind::Fetch)
}

pub(crate) fn web_search_begin_effect(call_id: String) -> BridgeEffect {
    BridgeEffect::tool_call(web_search_begin_tool_call(call_id))
}

pub(crate) fn web_search_update_tool_call_update(
    call_id: String,
    query: &str,
    action: &WebSearchAction,
) -> ToolCallUpdate {
    ToolCallUpdate::new(
        call_id,
        ToolCallUpdateFields::new()
            .status(ToolCallStatus::InProgress)
            .title(web_search_update_title(action))
            .raw_input(raw::web_search_update(query, action)),
    )
}

pub(crate) fn web_search_update_effect(
    call_id: String,
    query: &str,
    action: &WebSearchAction,
) -> BridgeEffect {
    BridgeEffect::tool_call_update(web_search_update_tool_call_update(call_id, query, action))
}

pub(crate) fn web_search_complete_tool_call_update(call_id: String) -> ToolCallUpdate {
    ToolCallUpdate::new(
        call_id,
        ToolCallUpdateFields::new().status(ToolCallStatus::Completed),
    )
}

pub(crate) fn web_search_complete_effect(call_id: String) -> BridgeEffect {
    BridgeEffect::tool_call_update(web_search_complete_tool_call_update(call_id))
}

pub(crate) fn image_generation_begin_tool_call(event: ImageGenerationBeginEvent) -> ToolCall {
    ToolCall::new(event.call_id, "Generate image")
        .kind(ToolKind::Other)
        .status(ToolCallStatus::InProgress)
}

pub(crate) fn image_generation_begin_effect(event: ImageGenerationBeginEvent) -> BridgeEffect {
    BridgeEffect::tool_call(image_generation_begin_tool_call(event))
}

pub(crate) fn image_generation_replay_tool_call(event: &ImageGenerationEndEvent) -> ToolCall {
    let status = if event.status == "completed" {
        ToolCallStatus::Completed
    } else {
        ToolCallStatus::Failed
    };
    let locations = event
        .saved_path
        .clone()
        .map(|path| vec![ToolCallLocation::new(path)]);
    let content = format_image_generation_content(event);
    let mut tool_call = ToolCall::new(event.call_id.clone(), "Generate image")
        .kind(ToolKind::Other)
        .status(status)
        .content(content);
    if let Some(locations) = locations {
        tool_call = tool_call.locations(locations);
    }
    tool_call
}

pub(crate) fn image_generation_replay_effect(event: &ImageGenerationEndEvent) -> BridgeEffect {
    BridgeEffect::tool_call(image_generation_replay_tool_call(event))
}

pub(crate) fn image_generation_end_tool_call_update(
    event: ImageGenerationEndEvent,
) -> ToolCallUpdate {
    let raw_output = raw::image_generation_end(&event);
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

    ToolCallUpdate::new(
        call_id,
        ToolCallUpdateFields::new()
            .status(status)
            .title(Some("Generate image".to_string()))
            .locations(locations)
            .content((!content.is_empty()).then_some(content))
            .raw_output(raw_output),
    )
}

pub(crate) fn image_generation_end_effect(event: ImageGenerationEndEvent) -> BridgeEffect {
    BridgeEffect::tool_call_update(image_generation_end_tool_call_update(event))
}

pub(crate) fn view_image_tool_call(event: ViewImageToolCallEvent) -> ToolCall {
    let display_path = event.path.display().to_string();
    ToolCall::new(event.call_id, format!("View Image {display_path}"))
        .kind(ToolKind::Read)
        .status(ToolCallStatus::Completed)
        .content(vec![ToolCallContent::Content(Content::new(
            ContentBlock::ResourceLink(ResourceLink::new(display_path.clone(), display_path)),
        ))])
        .locations(vec![ToolCallLocation::new(event.path)])
}

pub(crate) fn view_image_effect(event: ViewImageToolCallEvent) -> BridgeEffect {
    BridgeEffect::tool_call(view_image_tool_call(event))
}

fn web_search_update_title(action: &WebSearchAction) -> Option<String> {
    Some(match action {
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
    })
}
