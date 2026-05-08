use codex_protocol::models::WebSearchAction;

use crate::boundary::{effect::BridgeEffect, tool_call};

use super::submission::PromptState;

impl PromptState {
    pub(super) fn start_web_search(&mut self, call_id: String) -> BridgeEffect {
        self.start_active_web_search(call_id.clone());
        tool_call::web_search_begin_effect(call_id)
    }

    pub(super) fn update_web_search_query(
        call_id: String,
        query: String,
        action: WebSearchAction,
    ) -> BridgeEffect {
        tool_call::web_search_update_effect(call_id, &query, &action)
    }

    pub(super) fn complete_web_search(&mut self) -> Option<BridgeEffect> {
        self.take_active_web_search()
            .map(tool_call::web_search_complete_effect)
    }
}
