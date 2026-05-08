use crate::boundary::{
    effect::IgnoredCodexEventReason,
    mapper::ReplayResponseItemRoute,
    tool_call::{self, ReplayResponseItemPlan},
};

use super::{actor::ThreadActor, deps::Auth};

impl<A: Auth> ThreadActor<A> {
    /// Convert and send a single `ResponseItem` as ACP notification(s) during replay.
    /// Only handles tool calls - messages/reasoning are handled via `EventMsg`.
    pub(super) fn replay_response_item(&self, route: ReplayResponseItemRoute<'_>) {
        match tool_call::replay_response_item_plan(route, self.config.cwd.as_path()) {
            ReplayResponseItemPlan::Effect(effect) => {
                self.execute_replay_effect(*effect);
            }
            ReplayResponseItemPlan::Ignore { item, reason } => {
                log_replay_ignored_response_item(item, reason);
            }
        }
    }
}

fn log_replay_ignored_response_item(
    item: &codex_protocol::models::ResponseItem,
    reason: IgnoredCodexEventReason,
) {
    tracing::info!("Ignoring replay ResponseItem {item:?}: {reason:?}");
}
