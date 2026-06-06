use agent_client_protocol::{
    Error,
    schema::{
        LoadSessionResponse, SessionConfigId, SessionConfigOption, SessionConfigOptionCategory,
        SessionConfigOptionValue, SessionConfigSelectOption, SessionModeId,
    },
};
use codex_core::config::edit::ConfigEditsBuilder;
use codex_features::Feature;
use codex_protocol::config_types::ServiceTier;
use heck::ToTitleCase;

use crate::boundary::{op, session_update};

use super::{actor::ThreadActor, deps::Auth, model_picker::resolve_model_preset};

const REVIEW_TARGET_CURRENT_CHANGES: &str = "current_changes";
const REVIEW_TARGET_BRANCH_PREFIX: &str = "branch:";

impl<A: Auth> ThreadActor<A> {
    pub(super) async fn config_options(&self) -> Result<Vec<SessionConfigOption>, Error> {
        let mut options = Vec::new();

        let modes = self.modes();
        let mode_select_options = modes
            .available_modes
            .into_iter()
            .map(|m| SessionConfigSelectOption::new(m.id.0, m.name).description(m.description))
            .collect::<Vec<_>>();

        options.push(
            SessionConfigOption::select(
                "mode",
                "Mode",
                modes.current_mode_id.0,
                mode_select_options,
            )
            .category(SessionConfigOptionCategory::Mode)
            .description("Choose whether Codex should plan first or work directly"),
        );

        if let Some((current_preset_id, presets)) = self.approval_preset_config_option() {
            let select_options = presets
                .into_iter()
                .map(|m| SessionConfigSelectOption::new(m.id.0, m.name).description(m.description))
                .collect::<Vec<_>>();

            options.push(
                SessionConfigOption::select(
                    "approval_preset",
                    "Approval Preset",
                    current_preset_id.0,
                    select_options,
                )
                .description("Choose an approval and sandboxing preset for your session"),
            );
        }

        let presets = self.model_presets().await;

        let current_model = self.get_current_model().await;
        let current_preset = resolve_model_preset(&presets, Some(&current_model)).cloned();
        let current_model_value = current_preset
            .as_ref()
            .map_or_else(|| current_model.clone(), |preset| preset.id.clone());

        let mut model_select_options = Vec::new();

        if current_preset.is_none() && presets.is_empty() {
            model_select_options.push(SessionConfigSelectOption::new(
                current_model_value.clone(),
                current_model_value.clone(),
            ));
        }

        model_select_options.extend(
            presets
                .into_iter()
                .filter(|model| {
                    model.show_in_picker || model.is_default || model.model == current_model
                })
                .map(|preset| {
                    SessionConfigSelectOption::new(preset.id, preset.display_name)
                        .description(preset.description)
                }),
        );

        options.push(
            SessionConfigOption::select(
                "model",
                "Model",
                current_model_value,
                model_select_options,
            )
            .category(SessionConfigOptionCategory::Model)
            .description("Choose which model Codex should use"),
        );

        // Reasoning effort selector (only if the current preset exists and has >1 supported effort)
        if let Some(preset) = current_preset
            && preset.supported_reasoning_efforts.len() > 1
        {
            let supported = &preset.supported_reasoning_efforts;

            let current_effort = self
                .config
                .model_reasoning_effort
                .and_then(|effort| {
                    supported
                        .iter()
                        .find_map(|e| (e.effort == effort).then_some(effort))
                })
                .unwrap_or(preset.default_reasoning_effort);

            let effort_select_options = supported
                .iter()
                .map(|e| {
                    SessionConfigSelectOption::new(
                        e.effort.to_string(),
                        e.effort.to_string().to_title_case(),
                    )
                    .description(e.description.clone())
                })
                .collect::<Vec<_>>();

            options.push(
                SessionConfigOption::select(
                    "reasoning_effort",
                    "Reasoning Effort",
                    current_effort.to_string(),
                    effort_select_options,
                )
                .category(SessionConfigOptionCategory::ThoughtLevel)
                .description("Choose how much reasoning effort the model should use"),
            );
        }

        if self.fast_mode_configurable() {
            options.push(
                SessionConfigOption::select(
                    "service_tier",
                    "Service Tier",
                    self.current_service_tier_value(),
                    vec![
                        SessionConfigSelectOption::new("default", "Default")
                            .description("Use the account and model default service tier"),
                        SessionConfigSelectOption::new("fast", "Fast")
                            .description("Use Codex Fast mode for future turns"),
                        SessionConfigSelectOption::new("flex", "Flex")
                            .description("Use the flex service tier for future turns"),
                    ],
                )
                .description("Choose the Codex service tier used for future turns"),
            );
        }

        Ok(options)
    }

    pub(super) async fn maybe_emit_config_options_update(&mut self) {
        let config_options = self.config_options().await.unwrap_or_default();

        if self
            .state
            .last_sent_config_options()
            .is_some_and(|prev| prev == config_options.as_slice())
        {
            return;
        }

        self.state
            .set_last_sent_config_options(config_options.clone());

        self.execute_actor_effect(session_update::config_options_effect(config_options));
    }

    pub(super) async fn handle_set_config_option(
        &mut self,
        config_id: SessionConfigId,
        value: SessionConfigOptionValue,
    ) -> Result<(), Error> {
        let SessionConfigOptionValue::ValueId { value } = value else {
            return Err(Error::invalid_params().data("Unsupported config option value"));
        };
        match config_id.0.as_ref() {
            "mode" => self.handle_set_mode(SessionModeId::new(value.0)).await,
            "approval_preset" => {
                self.handle_set_approval_preset(SessionModeId::new(value.0))
                    .await
            }
            "review_target" => self.handle_set_review_target(value.0.as_ref()),
            "model" => self.handle_set_config_model(value).await,
            "reasoning_effort" => self.handle_set_config_reasoning_effort(value).await,
            "service_tier" => self.handle_set_service_tier(value.0.as_ref()).await,
            _ => Err(Error::invalid_params().data("Unsupported config option")),
        }
    }

    pub(super) fn fast_mode_configurable(&self) -> bool {
        self.config.features.enabled(Feature::FastMode)
    }

    pub(super) fn current_service_tier_value(&self) -> &'static str {
        match self
            .config
            .service_tier
            .as_deref()
            .and_then(ServiceTier::from_request_value)
        {
            None => "default",
            Some(ServiceTier::Fast) => "fast",
            Some(ServiceTier::Flex) => "flex",
        }
    }

    pub(super) async fn set_service_tier(
        &mut self,
        service_tier: Option<ServiceTier>,
    ) -> Result<(), Error> {
        let service_tier_value = service_tier.map(|tier| tier.request_value().to_string());
        ConfigEditsBuilder::for_config(&self.config)
            .set_service_tier(service_tier_value.clone())
            .apply()
            .await
            .map_err(|e| Error::from(anyhow::anyhow!(e)))?;

        self.thread
            .submit_ok(op::override_service_tier(service_tier_value.clone()))
            .await?;

        self.config.service_tier = service_tier_value;
        Ok(())
    }

    async fn handle_set_service_tier(&mut self, value: &str) -> Result<(), Error> {
        if !self.fast_mode_configurable() {
            return Err(Error::invalid_params().data("Fast mode is not available"));
        }

        let service_tier = match value {
            "default" | "off" => None,
            "fast" | "on" => Some(ServiceTier::Fast),
            "flex" => Some(ServiceTier::Flex),
            _ => return Err(Error::invalid_params().data("Unsupported service tier")),
        };

        self.set_service_tier(service_tier).await
    }

    fn handle_set_review_target(&mut self, value: &str) -> Result<(), Error> {
        if value == REVIEW_TARGET_CURRENT_CHANGES {
            self.state.set_review_base_branch(None);
            return Ok(());
        }

        let Some(branch) = value.strip_prefix(REVIEW_TARGET_BRANCH_PREFIX) else {
            return Err(Error::invalid_params().data("Unsupported review target"));
        };

        let branch = branch.trim();
        if branch.is_empty() {
            return Err(Error::invalid_params().data("Review branch cannot be empty"));
        }

        self.state.set_review_base_branch(Some(branch.to_string()));
        Ok(())
    }

    pub(super) async fn handle_load(&mut self) -> Result<LoadSessionResponse, Error> {
        self.ensure_current_model_selection().await?;

        Ok(LoadSessionResponse::new()
            .modes(self.modes())
            .config_options(self.config_options().await?))
    }
}
