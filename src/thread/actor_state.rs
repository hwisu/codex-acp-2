use std::collections::HashMap;

use agent_client_protocol::schema::SessionConfigOption;
use codex_protocol::{
    ThreadId,
    config_types::ModeKind,
    protocol::{AgentStatus, CollabAgentStatusEntry, EventMsg, RateLimitSnapshot, TokenUsageInfo},
};

use crate::{display::format_agent_label, user_input::PendingUserInputRequest};

#[derive(Debug, Clone, Default)]
pub(super) struct UsageSnapshot {
    info: Option<TokenUsageInfo>,
    rate_limits: Option<RateLimitSnapshot>,
}

impl UsageSnapshot {
    pub(super) fn info(&self) -> Option<&TokenUsageInfo> {
        self.info.as_ref()
    }

    pub(super) fn rate_limits(&self) -> Option<&RateLimitSnapshot> {
        self.rate_limits.as_ref()
    }
}

#[derive(Default)]
pub(super) struct ActorState {
    last_sent_config_options: Option<Vec<SessionConfigOption>>,
    active_collaboration_mode_kind: Option<ModeKind>,
    review_base_branch: Option<String>,
    known_collab_agents: HashMap<ThreadId, CollabAgentStatusEntry>,
    pending_user_input: Option<PendingUserInputRequest>,
    latest_usage: UsageSnapshot,
}

impl ActorState {
    pub(super) fn last_sent_config_options(&self) -> Option<&[SessionConfigOption]> {
        self.last_sent_config_options.as_deref()
    }

    pub(super) fn set_last_sent_config_options(
        &mut self,
        config_options: Vec<SessionConfigOption>,
    ) {
        self.last_sent_config_options = Some(config_options);
    }

    pub(super) fn set_latest_usage(
        &mut self,
        info: Option<TokenUsageInfo>,
        rate_limits: Option<RateLimitSnapshot>,
    ) {
        self.latest_usage = UsageSnapshot { info, rate_limits };
    }

    pub(super) fn latest_usage(&self) -> &UsageSnapshot {
        &self.latest_usage
    }

    pub(super) fn set_collaboration_mode_kind(&mut self, kind: ModeKind) {
        self.active_collaboration_mode_kind = Some(kind);
    }

    pub(super) fn collaboration_mode_kind_or_default(&self) -> ModeKind {
        self.active_collaboration_mode_kind
            .unwrap_or(ModeKind::Default)
    }

    pub(super) fn review_base_branch(&self) -> Option<&str> {
        self.review_base_branch.as_deref()
    }

    pub(super) fn set_review_base_branch(&mut self, branch: Option<String>) {
        self.review_base_branch = branch;
    }

    pub(super) fn remember_collab_agent(
        &mut self,
        thread_id: ThreadId,
        agent_nickname: Option<String>,
        agent_role: Option<String>,
        status: AgentStatus,
    ) {
        self.known_collab_agents.insert(
            thread_id,
            CollabAgentStatusEntry {
                thread_id,
                agent_nickname,
                agent_role,
                status,
            },
        );
    }

    pub(super) fn remember_collab_agent_entry(&mut self, entry: CollabAgentStatusEntry) {
        self.known_collab_agents.insert(entry.thread_id, entry);
    }

    pub(super) fn remove_collab_agent(&mut self, thread_id: &ThreadId) {
        self.known_collab_agents.remove(thread_id);
    }

    pub(super) fn known_collab_agent_count(&self) -> usize {
        self.known_collab_agents.len()
    }

    pub(super) fn known_collab_agents_sorted(&self) -> Vec<CollabAgentStatusEntry> {
        let mut agents = self
            .known_collab_agents
            .values()
            .cloned()
            .collect::<Vec<_>>();
        agents.sort_by(|left, right| {
            format_agent_label(
                left.agent_nickname.as_deref(),
                left.agent_role.as_deref(),
                Some(&left.thread_id),
            )
            .cmp(&format_agent_label(
                right.agent_nickname.as_deref(),
                right.agent_role.as_deref(),
                Some(&right.thread_id),
            ))
        });
        agents
    }

    pub(super) fn set_pending_user_input(&mut self, pending_request: PendingUserInputRequest) {
        self.pending_user_input = Some(pending_request);
    }

    pub(super) fn pending_user_input(&self) -> Option<&PendingUserInputRequest> {
        self.pending_user_input.as_ref()
    }

    pub(super) fn has_pending_user_input(&self) -> bool {
        self.pending_user_input.is_some()
    }

    pub(super) fn pending_submission_id(&self) -> Option<&str> {
        self.pending_user_input
            .as_ref()
            .map(|pending| pending.submission_id.as_str())
    }

    pub(super) fn clear_pending_user_input(&mut self) {
        self.pending_user_input = None;
    }

    pub(super) fn clear_pending_user_input_for_submission(&mut self, submission_id: &str) {
        if self
            .pending_user_input
            .as_ref()
            .is_some_and(|pending| pending.submission_id == submission_id)
        {
            self.clear_pending_user_input();
        }
    }

    pub(super) fn update_from_event(&mut self, msg: &EventMsg) {
        self.update_known_collab_agents(msg);
        self.update_known_collaboration_mode(msg);
        self.update_latest_usage(msg);
    }

    fn update_latest_usage(&mut self, msg: &EventMsg) {
        if let EventMsg::TokenCount(event) = msg {
            self.set_latest_usage(event.info.clone(), event.rate_limits.clone());
        }
    }

    fn update_known_collaboration_mode(&mut self, msg: &EventMsg) {
        if let EventMsg::TurnStarted(event) = msg {
            self.set_collaboration_mode_kind(event.collaboration_mode_kind);
        }
    }

    fn update_known_collab_agents(&mut self, msg: &EventMsg) {
        match msg {
            EventMsg::CollabAgentSpawnEnd(event) => {
                if let Some(thread_id) = event.new_thread_id.as_ref() {
                    self.remember_collab_agent(
                        *thread_id,
                        event.new_agent_nickname.clone(),
                        event.new_agent_role.clone(),
                        event.status.clone(),
                    );
                }
            }
            EventMsg::CollabAgentInteractionEnd(event) => {
                self.remember_collab_agent(
                    event.receiver_thread_id,
                    event.receiver_agent_nickname.clone(),
                    event.receiver_agent_role.clone(),
                    event.status.clone(),
                );
            }
            EventMsg::CollabWaitingEnd(event) => {
                if !event.agent_statuses.is_empty() {
                    for entry in &event.agent_statuses {
                        self.remember_collab_agent_entry(entry.clone());
                    }
                } else {
                    for (thread_id, status) in &event.statuses {
                        self.remember_collab_agent(*thread_id, None, None, status.clone());
                    }
                }
            }
            EventMsg::CollabResumeEnd(event) => {
                self.remember_collab_agent(
                    event.receiver_thread_id,
                    event.receiver_agent_nickname.clone(),
                    event.receiver_agent_role.clone(),
                    event.status.clone(),
                );
            }
            EventMsg::CollabCloseEnd(event) => {
                self.remove_collab_agent(&event.receiver_thread_id);
            }
            _ => {}
        }
    }
}
