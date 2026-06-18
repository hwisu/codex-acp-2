use std::collections::HashMap;

use agent_client_protocol::schema::SessionConfigOption;
use codex_protocol::{
    ThreadId,
    protocol::{AgentStatus, CollabAgentStatusEntry, RateLimitSnapshot, TokenUsageInfo},
};

use crate::{
    boundary::mapper::{ActorCollabAgentUpdate, ActorStateUpdate},
    display::format_agent_label,
    user_input::PendingUserInputRequest,
};

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
    active_collaboration_mode_kind: Option<codex_protocol::config_types::ModeKind>,
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

    pub(super) fn set_collaboration_mode_kind(
        &mut self,
        kind: codex_protocol::config_types::ModeKind,
    ) {
        self.active_collaboration_mode_kind = Some(kind);
    }

    pub(super) fn collaboration_mode_kind_or_default(
        &self,
    ) -> codex_protocol::config_types::ModeKind {
        self.active_collaboration_mode_kind
            .unwrap_or(codex_protocol::config_types::ModeKind::Default)
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
        let mut agents: Vec<_> = self.known_collab_agents.values().cloned().collect();
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

    pub(super) fn apply_event_updates(&mut self, updates: Vec<ActorStateUpdate>) {
        for update in updates {
            self.apply_event_update(update);
        }
    }

    fn apply_event_update(&mut self, update: ActorStateUpdate) {
        match update {
            ActorStateUpdate::LatestUsage { info, rate_limits } => {
                self.set_latest_usage(info, rate_limits);
            }
            ActorStateUpdate::CollaborationMode(kind) => {
                self.set_collaboration_mode_kind(kind);
            }
            ActorStateUpdate::RememberCollabAgent(update) => {
                let ActorCollabAgentUpdate {
                    thread_id,
                    agent_nickname,
                    agent_role,
                    status,
                } = update;
                self.remember_collab_agent(thread_id, agent_nickname, agent_role, status);
            }
            ActorStateUpdate::RememberCollabAgentEntries(entries) => {
                for entry in entries {
                    self.remember_collab_agent_entry(entry);
                }
            }
            ActorStateUpdate::RemoveCollabAgent(thread_id) => {
                self.remove_collab_agent(&thread_id);
            }
        }
    }
}
