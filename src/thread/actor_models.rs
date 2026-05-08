use agent_client_protocol::{
    Error,
    schema::{ModelId, ModelInfo, SessionConfigValueId, SessionModelState},
};
use codex_protocol::{
    openai_models::{ModelPreset, ReasoningEffort},
    protocol::Op,
};

use super::{
    actor::ThreadActor,
    deps::Auth,
    model_picker::{filter_model_presets_for_picker, resolve_model_preset},
};

impl<A: Auth> ThreadActor<A> {
    pub(super) async fn model_presets(&self) -> Vec<ModelPreset> {
        let current_model = self.get_current_model().await;
        filter_model_presets_for_picker(
            self.models_manager.list_models().await,
            Some(current_model.as_str()),
        )
    }

    pub(super) async fn current_model_id(&self, presets: &[ModelPreset]) -> Option<ModelId> {
        let config_model = self.get_current_model().await;
        current_model_id_for_presets(&config_model, presets)
    }

    pub(super) fn parse_model_id(id: &ModelId) -> Option<(String, ReasoningEffort)> {
        parse_model_id_value(id)
    }
    pub(super) async fn handle_set_config_model(
        &mut self,
        value: SessionConfigValueId,
    ) -> Result<(), Error> {
        let model_id = value.0;

        let presets = self.model_presets().await;
        let selection =
            select_config_model(&model_id, &presets, self.config.model_reasoning_effort)?;

        self.apply_selected_model(selection).await?;

        Ok(())
    }

    pub(super) async fn handle_set_config_reasoning_effort(
        &mut self,
        value: SessionConfigValueId,
    ) -> Result<(), Error> {
        let effort: ReasoningEffort =
            serde_json::from_value(value.0.as_ref().into()).map_err(|_| Error::invalid_params())?;

        let current_model = self.get_current_model().await;
        let presets = self.model_presets().await;
        let Some(preset) = presets.iter().find(|p| p.model == current_model) else {
            return Err(Error::invalid_params()
                .data("Reasoning effort can only be set for known model presets"));
        };

        if !supports_reasoning_effort(preset, effort) {
            return Err(
                Error::invalid_params().data("Unsupported reasoning effort for selected model")
            );
        }

        self.submit_model_override(None, ReasoningEffortOverride::Set(effort))
            .await?;

        self.config.model_reasoning_effort = Some(effort);

        Ok(())
    }

    pub(super) async fn models(&self) -> Result<SessionModelState, Error> {
        let mut available_models = Vec::new();
        let config_model = self.get_current_model().await;
        let presets = self.model_presets().await;

        let current_model_id = self.current_model_id(&presets).await.map_or_else(
            || {
                // If no preset found, return the current model string as-is
                let model_id = ModelId::new(config_model.clone());
                available_models.push(ModelInfo::new(model_id.clone(), model_id.to_string()));
                model_id
            },
            std::convert::identity,
        );

        available_models.extend(
            presets
                .iter()
                .filter(|model| {
                    model.show_in_picker || model.is_default || model.model == config_model
                })
                .map(|preset| {
                    ModelInfo::new(preset.id.clone(), preset.display_name.clone())
                        .description(preset.description.clone())
                }),
        );

        Ok(SessionModelState::new(current_model_id, available_models))
    }

    pub(super) async fn ensure_current_model_selection(&mut self) -> Result<(), Error> {
        let current_model = self.get_current_model().await;
        let presets = self.model_presets().await;
        let Some(selection) = normalized_current_model_selection(
            &current_model,
            &presets,
            self.config.model_reasoning_effort,
        ) else {
            return Ok(());
        };

        self.apply_selected_model(selection).await?;
        Ok(())
    }

    pub(super) async fn get_current_model(&self) -> String {
        self.models_manager
            .get_model(self.config.model.as_deref())
            .await
    }

    pub(super) async fn handle_set_model(&mut self, model: ModelId) -> Result<(), Error> {
        let presets = self.model_presets().await;
        let current_model = if Self::parse_model_id(&model).is_none() && model.0.is_empty() {
            Some(self.get_current_model().await)
        } else {
            None
        };
        let selection = select_model_id(
            &model,
            &presets,
            current_model.as_deref(),
            self.config.model_reasoning_effort,
        )?;

        self.apply_selected_model(selection).await?;

        Ok(())
    }

    async fn apply_selected_model(&mut self, selection: SelectedModel) -> Result<(), Error> {
        self.submit_model_override(
            Some(selection.model.clone()),
            ReasoningEffortOverride::from_selected_effort(selection.effort),
        )
        .await?;
        self.config.model = Some(selection.model);
        self.config.model_reasoning_effort = selection.effort;
        Ok(())
    }

    async fn submit_model_override(
        &self,
        model: Option<String>,
        effort: ReasoningEffortOverride,
    ) -> Result<(), Error> {
        self.thread
            .submit_ok(Op::OverrideTurnContext {
                cwd: None,
                approval_policy: None,
                sandbox_policy: None,
                model,
                effort: Some(effort.into_op_value()),
                summary: None,
                collaboration_mode: None,
                personality: None,
                windows_sandbox_level: None,
                service_tier: None,
                approvals_reviewer: None,
                permission_profile: None,
            })
            .await
    }
}

struct SelectedModel {
    model: String,
    effort: Option<ReasoningEffort>,
}

enum ReasoningEffortOverride {
    Set(ReasoningEffort),
    Clear,
}

impl ReasoningEffortOverride {
    fn from_selected_effort(effort: Option<ReasoningEffort>) -> Self {
        effort.map_or(Self::Clear, Self::Set)
    }

    fn into_op_value(self) -> Option<ReasoningEffort> {
        match self {
            Self::Set(effort) => Some(effort),
            Self::Clear => None,
        }
    }
}

fn current_model_id_for_presets(config_model: &str, presets: &[ModelPreset]) -> Option<ModelId> {
    let preset = resolve_model_preset(presets, Some(config_model))?;
    Some(ModelId::new(preset.id.clone()))
}

fn select_config_model(
    model_id: &str,
    presets: &[ModelPreset],
    configured_effort: Option<ReasoningEffort>,
) -> Result<SelectedModel, Error> {
    let preset = presets.iter().find(|preset| preset.id == model_id);
    let model = preset.map_or_else(|| model_id.to_string(), |preset| preset.model.clone());

    if model.is_empty() {
        return Err(Error::invalid_params().data("No model selected"));
    }

    let effort = preset.map_or(configured_effort, |preset| {
        Some(effective_reasoning_effort(preset, configured_effort))
    });

    Ok(SelectedModel { model, effort })
}

fn normalized_current_model_selection(
    current_model: &str,
    presets: &[ModelPreset],
    configured_effort: Option<ReasoningEffort>,
) -> Option<SelectedModel> {
    let preset = resolve_model_preset(presets, Some(current_model))?;
    let effort = effective_reasoning_effort(preset, configured_effort);

    if preset.model == current_model && configured_effort == Some(effort) {
        return None;
    }

    Some(SelectedModel {
        model: preset.model.clone(),
        effort: Some(effort),
    })
}

fn select_model_id(
    model: &ModelId,
    presets: &[ModelPreset],
    current_model: Option<&str>,
    configured_effort: Option<ReasoningEffort>,
) -> Result<SelectedModel, Error> {
    if let Some((requested_model, requested_effort)) = parse_model_id_value(model) {
        let preset = find_preset_by_id_or_model(presets, &requested_model)
            .ok_or_else(|| Error::invalid_params().data(format!("Unknown model {}", model.0)))?;
        if !supports_reasoning_effort(preset, requested_effort) {
            return Err(Error::invalid_params().data(format!(
                "Unsupported reasoning effort {requested_effort} for model {}",
                preset.model
            )));
        }
        return Ok(SelectedModel {
            model: preset.model.clone(),
            effort: Some(requested_effort),
        });
    }

    let model_str = model.0.to_string();
    let preset = if model_str.is_empty() {
        resolve_model_preset(presets, current_model)
    } else {
        find_preset_by_id_or_model(presets, &model_str)
    }
    .ok_or_else(|| Error::invalid_params().data(format!("Unknown model {}", model.0)))?;

    if preset.model.is_empty() {
        return Err(Error::invalid_params().data("No model parsed or configured"));
    }

    Ok(SelectedModel {
        model: preset.model.clone(),
        effort: Some(effective_reasoning_effort(preset, configured_effort)),
    })
}

fn find_preset_by_id_or_model<'a>(
    presets: &'a [ModelPreset],
    requested_model: &str,
) -> Option<&'a ModelPreset> {
    presets
        .iter()
        .find(|preset| preset.id == requested_model || preset.model == requested_model)
}

fn effective_reasoning_effort(
    preset: &ModelPreset,
    configured_effort: Option<ReasoningEffort>,
) -> ReasoningEffort {
    configured_effort
        .filter(|effort| supports_reasoning_effort(preset, *effort))
        .unwrap_or(preset.default_reasoning_effort)
}

fn supports_reasoning_effort(preset: &ModelPreset, effort: ReasoningEffort) -> bool {
    preset
        .supported_reasoning_efforts
        .iter()
        .any(|supported| supported.effort == effort)
}

fn parse_model_id_value(id: &ModelId) -> Option<(String, ReasoningEffort)> {
    let (model, reasoning) = if let Some(value) = id.0.strip_suffix(']') {
        value.rsplit_once('[')?
    } else {
        id.0.split_once('/')?
    };
    let reasoning = serde_json::from_value(reasoning.into()).ok()?;
    Some((model.to_owned(), reasoning))
}
