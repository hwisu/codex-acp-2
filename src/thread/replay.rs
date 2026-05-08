use codex_protocol::protocol::RolloutItem;

use crate::boundary::{
    effect::IgnoredCodexEventReason,
    mapper::{self, ReplayEventAction, ReplayRolloutItemRoute},
};

use super::{actor::ThreadActor, deps::Auth};

impl<A: Auth> ThreadActor<A> {
    /// Replay conversation history to the client via session/update notifications.
    /// This is called when loading a session to stream all prior messages.
    ///
    /// We process both `EventMsg` and `ResponseItem`:
    /// - `EventMsg` for user/agent messages and reasoning (like the TUI does)
    /// - `ResponseItem` for tool calls only (not persisted as `EventMsg`)
    pub(super) fn handle_replay_history(&mut self, history: Vec<RolloutItem>) {
        for item in history {
            match mapper::route_replay_rollout_item(&item) {
                ReplayRolloutItemRoute::Event(plan) => {
                    self.replay_event_plan(plan);
                }
                ReplayRolloutItemRoute::ResponseItem(route) => {
                    self.replay_response_item(route);
                }
                ReplayRolloutItemRoute::Ignore { item, reason } => {
                    log_replay_ignored_rollout_item(item, reason);
                }
            }
        }
    }

    /// Convert and send an `EventMsg` as ACP notification(s) during replay.
    /// Replays enough state to keep loaded sessions useful in external-agent clients.
    fn replay_event_plan(&mut self, plan: mapper::ReplayEventPlan<'_>) {
        self.state.apply_event_updates(plan.state_updates);

        match plan.action {
            ReplayEventAction::Effect(effect) => {
                self.execute_replay_effect(*effect);
            }
            ReplayEventAction::RegisterPendingUserInput { turn_id, event } => {
                self.register_pending_user_input(turn_id.to_string(), event.clone());
            }
            ReplayEventAction::ClearPendingUserInputForSubmission(turn_id) => {
                self.state.clear_pending_user_input_for_submission(turn_id);
            }
            ReplayEventAction::ClearPendingUserInput => {
                self.state.clear_pending_user_input();
            }
            ReplayEventAction::Ignore { event, reason } => {
                log_replay_ignored_event(event, reason);
            }
        }
    }
}

fn log_replay_ignored_event(
    event: &codex_protocol::protocol::EventMsg,
    reason: IgnoredCodexEventReason,
) {
    tracing::info!("Ignoring replay Codex event {event}: {reason:?}");
}

fn log_replay_ignored_rollout_item(item: &RolloutItem, reason: IgnoredCodexEventReason) {
    tracing::info!("Ignoring replay RolloutItem {item:?}: {reason:?}");
}
