use super::fixtures::*;

struct PresetModelsManager {
    default_model: String,
    presets: Vec<ModelPreset>,
}

impl ModelsManagerImpl for PresetModelsManager {
    fn get_model(
        &self,
        _model_id: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = String> + Send + '_>> {
        let default_model = self.default_model.clone();
        Box::pin(async move { default_model })
    }

    fn list_models(&self) -> Pin<Box<dyn Future<Output = Vec<ModelPreset>> + Send + '_>> {
        let presets = self.presets.clone();
        Box::pin(async move { presets })
    }
}

#[test]
fn model_filter_keeps_gpt_5_3_and_newer_high_efforts() {
    let presets = filter_model_presets_for_picker(all_model_presets().to_owned(), None);
    let ids = presets
        .iter()
        .map(|preset| preset.id.as_str())
        .collect::<Vec<_>>();

    assert!(ids.contains(&"gpt-5.5"));
    assert!(ids.contains(&"gpt-5.4"));
    assert!(ids.contains(&"gpt-5.3-codex"));
    assert!(!ids.contains(&"gpt-5.2"));
    assert!(
        presets
            .iter()
            .all(|preset| preset
                .supported_reasoning_efforts
                .iter()
                .all(|effort| matches!(
                    effort.effort,
                    ReasoningEffort::High | ReasoningEffort::XHigh
                )))
    );
}

#[tokio::test]
async fn config_model_options_keep_hidden_default_model() -> anyhow::Result<()> {
    let (_, _, _, mut actor) = setup_actor().await?;
    let mut default_model = all_model_presets()[0].clone();
    default_model.show_in_picker = false;
    default_model.is_default = true;
    let mut hidden_model = all_model_presets()[1].clone();
    hidden_model.show_in_picker = false;
    hidden_model.is_default = false;
    actor.models_manager = Arc::new(PresetModelsManager {
        default_model: default_model.model.clone(),
        presets: vec![default_model.clone(), hidden_model.clone()],
    });

    let config_options = actor.config_options().await?;
    let model = config_options
        .iter()
        .find(|option| option.id.0.as_ref() == "model")
        .expect("model option should be present");

    assert!(matches!(
        &model.kind,
        SessionConfigKind::Select(select)
            if select_option_ids(&select.options).contains(&default_model.id)
                && !select_option_ids(&select.options).contains(&hidden_model.id)
    ));

    Ok(())
}

#[test]
fn model_id_accepts_agentclientprotocol_bracket_format() {
    let parsed = ThreadActor::<StubAuth>::parse_model_id(&ModelId::new("gpt-5.4[high]"))
        .expect("bracket model id should parse");

    assert_eq!(parsed, ("gpt-5.4".to_string(), ReasoningEffort::High));
}

#[test]
fn model_id_accepts_legacy_slash_format() {
    let parsed = ThreadActor::<StubAuth>::parse_model_id(&ModelId::new("gpt-5.4/high"))
        .expect("legacy slash model id should parse");

    assert_eq!(parsed, ("gpt-5.4".to_string(), ReasoningEffort::High));
}

#[tokio::test]
async fn config_options_expose_mode_model_and_reasoning_separately() -> anyhow::Result<()> {
    let (_, _, _, mut actor) = setup_actor().await?;
    actor.config.model_reasoning_effort = Some(ReasoningEffort::XHigh);
    actor.state.set_collaboration_mode_kind(ModeKind::Plan);

    let config_options = actor.config_options().await?;

    let mode = config_options
        .iter()
        .find(|option| option.id.0.as_ref() == "mode")
        .expect("mode option should be present");
    assert_eq!(mode.category, Some(SessionConfigOptionCategory::Mode));
    assert!(matches!(
        &mode.kind,
        SessionConfigKind::Select(select) if select.current_value.0.as_ref() == "plan"
    ));

    let model = config_options
        .iter()
        .find(|option| option.id.0.as_ref() == "model")
        .expect("model option should be present");
    assert_eq!(model.category, Some(SessionConfigOptionCategory::Model));
    assert!(matches!(
        &model.kind,
        SessionConfigKind::Select(select)
            if select.current_value.0.as_ref() == "gpt-5.5"
                && select_option_ids(&select.options).iter().all(|id| !id.contains('['))
    ));

    let reasoning = config_options
        .iter()
        .find(|option| option.id.0.as_ref() == "reasoning_effort")
        .expect("reasoning effort option should be present");
    assert_eq!(
        reasoning.category,
        Some(SessionConfigOptionCategory::ThoughtLevel)
    );
    assert!(matches!(
        &reasoning.kind,
        SessionConfigKind::Select(select)
            if select.current_value.0.as_ref() == "xhigh"
                && select_option_ids(&select.options).contains(&"xhigh".to_string())
    ));

    let approval = config_options
        .iter()
        .find(|option| option.id.0.as_ref() == "approval_preset")
        .expect("approval preset should remain configurable");
    assert_eq!(approval.category, None);

    assert!(
        config_options
            .iter()
            .all(|option| option.id.0.as_ref() != "review_target"),
        "review target should not be exposed as a session config option"
    );

    Ok(())
}

#[tokio::test]
async fn fast_mode_config_option_sets_service_tier() -> anyhow::Result<()> {
    let (_, _, thread, mut actor) = setup_actor_with_fast_mode().await?;

    let config_options = actor.config_options().await?;
    let service_tier = config_options
        .iter()
        .find(|option| option.id.0.as_ref() == "service_tier")
        .expect("service tier option should be present when fast mode is enabled");
    assert!(matches!(
        &service_tier.kind,
        SessionConfigKind::Select(select)
            if select.current_value.0.as_ref() == "default"
                && select_option_ids(&select.options).contains(&"fast".to_string())
    ));

    let config_path = actor.config.codex_home.as_path().join("config.toml");

    actor
        .handle_set_config_option(
            SessionConfigId::new("service_tier"),
            SessionConfigOptionValue::ValueId {
                value: SessionConfigValueId::new("fast"),
            },
        )
        .await?;

    assert_eq!(actor.config.service_tier.as_deref(), Some("priority"));
    let contents = std::fs::read_to_string(&config_path)?;
    assert!(contents.contains("service_tier = \"fast\""));

    let ops = thread.ops();
    assert!(matches!(
        ops.as_slice(),
        [Op::OverrideTurnContext {
            service_tier: Some(Some(service_tier)),
            ..
        }] if service_tier == "priority"
    ));

    actor
        .handle_set_config_option(
            SessionConfigId::new("service_tier"),
            SessionConfigOptionValue::ValueId {
                value: SessionConfigValueId::new("flex"),
            },
        )
        .await?;

    assert_eq!(actor.config.service_tier.as_deref(), Some("flex"));
    let contents = std::fs::read_to_string(&config_path)?;
    assert!(contents.contains("service_tier = \"flex\""));

    actor
        .handle_set_config_option(
            SessionConfigId::new("service_tier"),
            SessionConfigOptionValue::ValueId {
                value: SessionConfigValueId::new("default"),
            },
        )
        .await?;

    assert_eq!(actor.config.service_tier, None);
    let contents = std::fs::read_to_string(config_path)?;
    assert!(!contents.contains("service_tier"));

    Ok(())
}

#[tokio::test]
async fn hidden_review_target_option_still_sets_review_branch() -> anyhow::Result<()> {
    let (_, _, _, mut actor) = setup_actor().await?;

    actor
        .handle_set_config_option(
            SessionConfigId::new("review_target"),
            SessionConfigOptionValue::ValueId {
                value: SessionConfigValueId::new("branch:main"),
            },
        )
        .await?;

    assert_eq!(actor.state.review_base_branch(), Some("main"));

    actor
        .handle_set_config_option(
            SessionConfigId::new("review_target"),
            SessionConfigOptionValue::ValueId {
                value: SessionConfigValueId::new("current_changes"),
            },
        )
        .await?;

    assert_eq!(actor.state.review_base_branch(), None);

    Ok(())
}

#[tokio::test]
async fn setting_session_mode_applies_collaboration_mode() -> anyhow::Result<()> {
    let (_, _, thread, mut actor) = setup_actor().await?;

    actor.handle_set_mode(SessionModeId::new("plan")).await?;

    let modes = actor.modes();
    assert_eq!(modes.current_mode_id.0.as_ref(), "plan");

    let ops = thread.ops();
    assert!(matches!(
        ops.as_slice(),
        [Op::OverrideTurnContext {
            collaboration_mode: Some(CollaborationMode {
                mode: ModeKind::Plan,
                ..
            }),
            ..
        }]
    ));

    Ok(())
}

#[tokio::test]
async fn legacy_full_access_session_mode_applies_approval_preset() -> anyhow::Result<()> {
    let (_, _, thread, mut actor) = setup_actor().await?;

    actor
        .handle_set_mode(SessionModeId::new("full-access"))
        .await?;

    let ops = thread.ops();
    assert!(matches!(
        ops.as_slice(),
        [Op::OverrideTurnContext {
            approval_policy: Some(_),
            permission_profile: Some(PermissionProfile::Disabled),
            collaboration_mode: None,
            ..
        }]
    ));

    assert_eq!(
        current_session_mode_id(&actor.config)
            .expect("mode should resolve")
            .0
            .as_ref(),
        "full-access"
    );

    Ok(())
}

#[tokio::test]
async fn legacy_full_access_session_mode_alias_is_canonicalized() -> anyhow::Result<()> {
    let (_, _, thread, mut actor) = setup_actor().await?;

    actor
        .handle_set_mode(SessionModeId::new("fullaccess"))
        .await?;

    let ops = thread.ops();
    assert!(matches!(
        ops.as_slice(),
        [Op::OverrideTurnContext {
            permission_profile: Some(PermissionProfile::Disabled),
            ..
        }]
    ));
    assert_eq!(
        current_session_mode_id(&actor.config)
            .expect("mode should resolve")
            .0
            .as_ref(),
        "full-access"
    );

    Ok(())
}

#[test]
fn test_guardian_execve_summary_uses_argv_without_duplication() -> anyhow::Result<()> {
    let action = GuardianAssessmentAction::Execve {
        source: GuardianCommandSource::UnifiedExec,
        program: "/bin/ls".to_string(),
        argv: vec!["/bin/ls".to_string(), "-l".to_string()],
        cwd: std::env::current_dir()?.try_into()?,
    };

    assert_eq!(
        guardian_action_summary(&action),
        "exec /bin/ls -l".to_string()
    );

    Ok(())
}

#[test]
fn guardian_action_summary_covers_all_action_kinds() -> anyhow::Result<()> {
    let cases = vec![
        (
            GuardianAssessmentAction::Command {
                source: GuardianCommandSource::Shell,
                command: "cargo test".to_string(),
                cwd: std::env::current_dir()?.try_into()?,
            },
            "shell cargo test",
        ),
        (
            GuardianAssessmentAction::ApplyPatch {
                cwd: std::env::current_dir()?.try_into()?,
                files: vec![std::env::current_dir()?.join("src/lib.rs").try_into()?],
            },
            "apply_patch touching ",
        ),
        (
            GuardianAssessmentAction::NetworkAccess {
                target: "api.openai.com".to_string(),
                host: "openai.com".to_string(),
                port: 443,
                protocol: NetworkApprovalProtocol::Https,
            },
            "network access to api.openai.com",
        ),
        (
            GuardianAssessmentAction::McpToolCall {
                server: "docs".to_string(),
                tool_name: "search".to_string(),
                connector_id: Some("docs".to_string()),
                connector_name: Some("Docs".to_string()),
                tool_title: Some("Search Docs".to_string()),
            },
            "MCP search on Docs",
        ),
        (
            GuardianAssessmentAction::RequestPermissions {
                permissions: RequestPermissionProfile::default(),
                reason: None,
            },
            "request additional permissions",
        ),
    ];

    for (action, expected) in cases {
        let summary = guardian_action_summary(&action);
        assert!(
            summary.contains(expected),
            "expected {summary:?} to contain {expected:?}"
        );
    }

    Ok(())
}

#[test]
fn guardian_assessment_content_includes_action_risk_and_rationale() {
    let event = GuardianAssessmentEvent {
        id: "guardian-1".to_string(),
        target_item_id: None,
        turn_id: "turn-1".to_string(),
        status: GuardianAssessmentStatus::Denied,
        started_at_ms: 0,
        completed_at_ms: None,
        risk_level: Some(GuardianRiskLevel::High),
        user_authorization: None,
        rationale: Some("The network request was not authorized.".to_string()),
        decision_source: None,
        action: GuardianAssessmentAction::NetworkAccess {
            target: String::new(),
            host: "api.example.com".to_string(),
            protocol: NetworkApprovalProtocol::Https,
            port: 443,
        },
    };

    let content = guardian_assessment_content(&event);

    assert!(matches!(
        content.as_slice(),
        [ToolCallContent::Content(Content {
            content: ContentBlock::Text(TextContent { text, .. }),
            ..
        })] if text.contains("Status: Denied")
            && text.contains("Action: network access to api.example.com")
            && text.contains("Risk: high")
            && text.contains("Rationale: The network request was not authorized.")
    ));
}

#[tokio::test]
async fn setting_model_with_bracket_effort_sends_explicit_effort() -> anyhow::Result<()> {
    let (_, _, thread, mut actor) = setup_actor().await?;

    actor
        .handle_set_model(ModelId::new("gpt-5.4[high]"))
        .await?;

    let ops = thread.ops();
    assert!(matches!(
        ops.as_slice(),
        [Op::OverrideTurnContext {
            model: Some(model),
            effort: Some(Some(ReasoningEffort::High)),
            ..
        }] if model == "gpt-5.4"
    ));

    Ok(())
}

#[tokio::test]
async fn setting_model_with_unsupported_effort_fails() -> anyhow::Result<()> {
    let (_, _, thread, mut actor) = setup_actor().await?;

    assert!(
        actor
            .handle_set_model(ModelId::new("gpt-5.4[warp]"))
            .await
            .is_err()
    );
    assert!(thread.ops().is_empty());

    Ok(())
}

#[tokio::test]
async fn setting_plain_model_preserves_supported_reasoning_effort() -> anyhow::Result<()> {
    let (_, _, thread, mut actor) = setup_actor().await?;
    actor.config.model_reasoning_effort = Some(ReasoningEffort::XHigh);

    actor.handle_set_model(ModelId::new("gpt-5.4")).await?;

    let ops = thread.ops();
    assert!(matches!(
        ops.as_slice(),
        [Op::OverrideTurnContext {
            model: Some(model),
            effort: Some(Some(ReasoningEffort::XHigh)),
            ..
        }] if model == "gpt-5.4"
    ));

    Ok(())
}

#[tokio::test]
async fn setting_custom_config_model_clears_reasoning_effort() -> anyhow::Result<()> {
    let (_, _, thread, mut actor) = setup_actor().await?;
    actor.config.model_reasoning_effort = None;

    actor
        .handle_set_config_option(
            SessionConfigId::new("model"),
            SessionConfigOptionValue::ValueId {
                value: SessionConfigValueId::new("custom-model"),
            },
        )
        .await?;

    let ops = thread.ops();
    assert!(matches!(
        ops.as_slice(),
        [Op::OverrideTurnContext {
            model: Some(model),
            effort: Some(None),
            ..
        }] if model == "custom-model"
    ));

    Ok(())
}

#[tokio::test]
async fn setting_approval_preset_uses_dedicated_config_option() -> anyhow::Result<()> {
    let (_, _, thread, mut actor) = setup_actor().await?;

    actor
        .handle_set_config_option(
            SessionConfigId::new("approval_preset"),
            SessionConfigOptionValue::ValueId {
                value: SessionConfigValueId::new("full-access"),
            },
        )
        .await?;

    let ops = thread.ops();
    assert!(matches!(
        ops.as_slice(),
        [Op::OverrideTurnContext {
            approval_policy: Some(_),
            permission_profile: Some(PermissionProfile::Disabled),
            collaboration_mode: None,
            ..
        }]
    ));

    Ok(())
}

#[tokio::test]
async fn setting_reasoning_effort_sends_effort_without_model_override() -> anyhow::Result<()> {
    let (_, _, thread, mut actor) = setup_actor().await?;
    actor
        .handle_set_model(ModelId::new("gpt-5.4[high]"))
        .await?;

    actor
        .handle_set_config_option(
            SessionConfigId::new("reasoning_effort"),
            SessionConfigOptionValue::ValueId {
                value: SessionConfigValueId::new("xhigh"),
            },
        )
        .await?;

    let ops = thread.ops();
    assert!(matches!(
        ops.as_slice(),
        [
            Op::OverrideTurnContext {
                model: Some(model),
                effort: Some(Some(ReasoningEffort::High)),
                ..
            },
            Op::OverrideTurnContext {
                model: None,
                effort: Some(Some(ReasoningEffort::XHigh)),
                ..
            },
        ] if model == "gpt-5.4"
    ));

    Ok(())
}

#[tokio::test]
async fn modes_match_augmented_workspace_permission_profile() -> anyhow::Result<()> {
    let mut config =
        Config::load_with_cli_overrides_and_harness_overrides(vec![], ConfigOverrides::default())
            .await?;
    config
        .permissions
        .approval_policy
        .set(codex_protocol::protocol::AskForApproval::OnRequest)?;

    let workspace_profile = PermissionProfile::workspace_write();
    let extra_roots = vec![config.codex_home.as_path().join("memories").try_into()?];
    let file_system_policy = workspace_profile
        .file_system_sandbox_policy()
        .with_additional_writable_roots(config.cwd.as_path(), &extra_roots);
    let augmented_profile = PermissionProfile::from_runtime_permissions(
        &file_system_policy,
        workspace_profile.network_sandbox_policy(),
    );
    assert_ne!(augmented_profile, workspace_profile);

    config
        .permissions
        .set_permission_profile_with_active_profile(
            augmented_profile,
            Some(ActivePermissionProfile::new(CODEX_WORKSPACE_PROFILE_ID)),
        )?;

    let mode_id = current_session_mode_id(&config).expect("mode should be recognized");
    assert_eq!(mode_id.0.as_ref(), "auto");

    Ok(())
}

#[tokio::test]
async fn modes_match_legacy_augmented_workspace_permission_profile() -> anyhow::Result<()> {
    let mut config =
        Config::load_with_cli_overrides_and_harness_overrides(vec![], ConfigOverrides::default())
            .await?;
    config
        .permissions
        .approval_policy
        .set(codex_protocol::protocol::AskForApproval::OnRequest)?;

    let workspace_profile = PermissionProfile::workspace_write();
    let extra_roots = vec![config.codex_home.as_path().join("memories").try_into()?];
    let file_system_policy = workspace_profile
        .file_system_sandbox_policy()
        .with_additional_writable_roots(config.cwd.as_path(), &extra_roots);
    let augmented_profile = PermissionProfile::from_runtime_permissions(
        &file_system_policy,
        workspace_profile.network_sandbox_policy(),
    );
    assert_ne!(augmented_profile, workspace_profile);

    config
        .permissions
        .set_permission_profile(augmented_profile)?;
    assert!(config.permissions.active_permission_profile().is_none());

    let mode_id = current_session_mode_id(&config).expect("mode should be recognized");
    assert_eq!(mode_id.0.as_ref(), "auto");

    Ok(())
}

#[test]
fn read_only_mode_does_not_trust_project() {
    assert!(!mode_trusts_project("read-only"));
    assert!(mode_trusts_project("auto"));
    assert!(mode_trusts_project("full-access"));
}

fn select_option_ids(options: &SessionConfigSelectOptions) -> Vec<String> {
    match options {
        SessionConfigSelectOptions::Ungrouped(options) => options
            .iter()
            .map(|option| option.value.0.to_string())
            .collect(),
        SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| group.options.iter())
            .map(|option| option.value.0.to_string())
            .collect(),
        _ => Vec::new(),
    }
}
