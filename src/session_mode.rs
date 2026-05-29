use std::sync::LazyLock;

use agent_client_protocol::schema::SessionModeId;
use codex_core::config::Config;
use codex_protocol::models::PermissionProfile;
use codex_utils_approval_presets::ApprovalPreset;

pub(crate) static APPROVAL_PRESETS: LazyLock<Vec<ApprovalPreset>> =
    LazyLock::new(codex_utils_approval_presets::builtin_approval_presets);
pub(crate) const CODEX_READ_ONLY_PROFILE_ID: &str = ":read-only";
pub(crate) const CODEX_WORKSPACE_PROFILE_ID: &str = ":workspace";
pub(crate) const CODEX_DANGER_NO_SANDBOX_PROFILE_ID: &str = ":danger-no-sandbox";

pub(crate) fn session_mode_id_for_active_profile(profile_id: &str) -> Option<&'static str> {
    match profile_id {
        CODEX_READ_ONLY_PROFILE_ID => Some("read-only"),
        CODEX_WORKSPACE_PROFILE_ID => Some("auto"),
        CODEX_DANGER_NO_SANDBOX_PROFILE_ID => Some("full-access"),
        _ => None,
    }
}

pub(crate) fn active_profile_id_for_session_mode(mode_id: &str) -> Option<&'static str> {
    match mode_id {
        "read-only" => Some(CODEX_READ_ONLY_PROFILE_ID),
        "auto" => Some(CODEX_WORKSPACE_PROFILE_ID),
        "full-access" => Some(CODEX_DANGER_NO_SANDBOX_PROFILE_ID),
        _ => None,
    }
}

pub(crate) fn approval_matches_current_config(preset: &ApprovalPreset, config: &Config) -> bool {
    std::mem::discriminant(&preset.approval)
        == std::mem::discriminant(config.permissions.approval_policy.get())
}

pub(crate) fn mode_id_if_approval_matches(
    mode_id: &'static str,
    config: &Config,
) -> Option<SessionModeId> {
    APPROVAL_PRESETS
        .iter()
        .find(|preset| preset.id == mode_id && approval_matches_current_config(preset, config))
        .map(|preset| SessionModeId::new(preset.id))
}

pub(crate) fn untrusted_read_only_mode_id(config: &Config) -> Option<SessionModeId> {
    config
        .active_project
        .is_untrusted()
        .then(|| SessionModeId::new("read-only"))
}

pub(crate) fn semantic_session_mode_id_for_permission_profile(
    config: &Config,
) -> Option<&'static str> {
    let permission_profile = config.permissions.permission_profile();

    match permission_profile {
        PermissionProfile::Managed { .. } => {
            let workspace_preset = APPROVAL_PRESETS.iter().find(|preset| preset.id == "auto")?;
            if permission_profile.network_sandbox_policy()
                != workspace_preset.permission_profile.network_sandbox_policy()
            {
                return None;
            }

            let file_system = permission_profile.file_system_sandbox_policy();
            let cwd = config.cwd.as_path();
            if file_system.has_full_disk_read_access()
                && !file_system.has_full_disk_write_access()
                && file_system.can_write_path_with_cwd(cwd, cwd)
            {
                Some("auto")
            } else {
                None
            }
        }
        PermissionProfile::Disabled => Some("full-access"),
        PermissionProfile::External { .. } => None,
    }
}

pub(crate) fn current_session_mode_id(config: &Config) -> Option<SessionModeId> {
    if let Some(active_profile) = config.permissions.active_permission_profile().as_ref() {
        return session_mode_id_for_active_profile(&active_profile.id)
            .and_then(|mode_id| mode_id_if_approval_matches(mode_id, config))
            .or_else(|| untrusted_read_only_mode_id(config));
    }

    if let Some(preset) = APPROVAL_PRESETS.iter().find(|preset| {
        approval_matches_current_config(preset, config)
            && &preset.permission_profile == config.permissions.permission_profile()
    }) {
        return Some(SessionModeId::new(preset.id));
    }

    semantic_session_mode_id_for_permission_profile(config)
        .and_then(|mode_id| mode_id_if_approval_matches(mode_id, config))
        .or_else(|| untrusted_read_only_mode_id(config))
}

pub(crate) fn mode_trusts_project(mode_id: &str) -> bool {
    matches!(mode_id, "auto" | "full-access")
}
