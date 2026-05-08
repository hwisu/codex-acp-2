use codex_protocol::models::WebSearchAction;
use uuid::Uuid;

/// Extract title and call ID from a `WebSearchAction` (used for replay).
pub(super) fn web_search_action_to_title_and_id(
    id: Option<&str>,
    action: &WebSearchAction,
) -> (String, String) {
    match action {
        WebSearchAction::Search { query, queries } => {
            let title = queries
                .as_ref()
                .map(|q| q.join(", "))
                .or_else(|| query.clone())
                .unwrap_or_else(|| "Web search".to_string());
            let call_id = id
                .map(str::to_string)
                .unwrap_or_else(|| generate_fallback_id("web_search"));
            (title, call_id)
        }
        WebSearchAction::OpenPage { url } => {
            let title = url.clone().unwrap_or_else(|| "Open page".to_string());
            let call_id = id
                .map(str::to_string)
                .unwrap_or_else(|| generate_fallback_id("web_open"));
            (title, call_id)
        }
        WebSearchAction::FindInPage { pattern, .. } => {
            let title = pattern
                .clone()
                .unwrap_or_else(|| "Find in page".to_string());
            let call_id = id
                .map(str::to_string)
                .unwrap_or_else(|| generate_fallback_id("web_find"));
            (title, call_id)
        }
        WebSearchAction::Other => ("Unknown".to_string(), generate_fallback_id("web_search")),
    }
}

pub(super) fn generate_fallback_id(prefix: &str) -> String {
    format!("{}_{}", prefix, Uuid::new_v4())
}
