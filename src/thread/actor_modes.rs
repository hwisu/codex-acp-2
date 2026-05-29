use agent_client_protocol::{
    Error,
    schema::{SessionMode, SessionModeId, SessionModeState},
};
use codex_core::config::{PermissionProfileSnapshot, set_project_trust_level};
use codex_protocol::{
    config_types::{ModeKind, TrustLevel},
    models::ActivePermissionProfile,
};

use crate::{
    boundary::op,
    session_mode::{
        APPROVAL_PRESETS, active_profile_id_for_session_mode, current_session_mode_id,
        mode_trusts_project,
    },
};

use super::{actor::ThreadActor, collaboration_mode_for_kind, deps::Auth};

impl<A: Auth> ThreadActor<A> {
    pub(super) fn modes(&self) -> SessionModeState {
        SessionModeState::new(
            collaboration_mode_id(self.state.collaboration_mode_kind_or_default()),
            vec![
                SessionMode::new(collaboration_mode_id(ModeKind::Default), "Default")
                    .description("Implement and iterate directly"),
                SessionMode::new(collaboration_mode_id(ModeKind::Plan), "Plan")
                    .description("Plan first, then wait before making code changes"),
            ],
        )
    }

    pub(super) fn approval_preset_config_option(
        &self,
    ) -> Option<(SessionModeId, Vec<SessionMode>)> {
        let current_mode_id = current_session_mode_id(&self.config)?;
        Some((
            current_mode_id,
            APPROVAL_PRESETS
                .iter()
                .map(|preset| {
                    SessionMode::new(preset.id, preset.label).description(preset.description)
                })
                .collect(),
        ))
    }

    pub(super) async fn handle_set_mode(&mut self, mode: SessionModeId) -> Result<(), Error> {
        if let Some(preset_id) = Self::permission_preset_from_arg(mode.0.as_ref()) {
            return self
                .handle_set_approval_preset(SessionModeId::new(preset_id))
                .await;
        }

        let kind = collaboration_mode_kind_from_id(mode.0.as_ref())
            .ok_or_else(|| Error::invalid_params().data("Unsupported collaboration mode"))?;

        self.apply_collaboration_mode_kind(kind).await
    }

    pub(super) async fn handle_set_approval_preset(
        &mut self,
        mode: SessionModeId,
    ) -> Result<(), Error> {
        let preset = APPROVAL_PRESETS
            .iter()
            .find(|preset| mode.0.as_ref() == preset.id)
            .ok_or_else(Error::invalid_params)?;

        self.thread
            .submit_ok(op::override_approval_preset(preset))
            .await?;

        self.config
            .permissions
            .approval_policy
            .set(preset.approval)
            .map_err(|e| Error::from(anyhow::anyhow!(e)))?;
        self.config
            .permissions
            .set_permission_profile_from_session_snapshot(
                PermissionProfileSnapshot::from_session_snapshot(
                    preset.permission_profile.clone(),
                    active_profile_id_for_session_mode(preset.id).map(ActivePermissionProfile::new),
                ),
            )
            .map_err(|e| Error::from(anyhow::anyhow!(e)))?;

        if mode_trusts_project(preset.id) {
            set_project_trust_level(
                &self.config.codex_home,
                &self.config.cwd,
                TrustLevel::Trusted,
            )?;
        }

        Ok(())
    }

    pub(super) async fn apply_collaboration_mode_kind(
        &mut self,
        kind: ModeKind,
    ) -> Result<(), Error> {
        let model = self.get_current_model().await;
        let collaboration_mode =
            collaboration_mode_for_kind(kind, model, self.config.model_reasoning_effort)
                .ok_or_else(|| {
                    Error::internal_error().data(format!(
                        "No collaboration preset found for {} mode",
                        kind.display_name()
                    ))
                })?;

        self.thread
            .submit_ok(op::override_collaboration_mode(collaboration_mode))
            .await?;

        self.state.set_collaboration_mode_kind(kind);
        Ok(())
    }
}

fn collaboration_mode_id(kind: ModeKind) -> SessionModeId {
    SessionModeId::new(match kind {
        ModeKind::Default | ModeKind::PairProgramming | ModeKind::Execute => "default",
        ModeKind::Plan => "plan",
    })
}

fn collaboration_mode_kind_from_id(id: &str) -> Option<ModeKind> {
    match id {
        "default" | "code" => Some(ModeKind::Default),
        "plan" => Some(ModeKind::Plan),
        _ => None,
    }
}
