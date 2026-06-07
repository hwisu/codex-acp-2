use agent_client_protocol::{
    Error,
    schema::SessionConfigValueId,
};
use codex_protocol::openai_models::{ModelPreset, ReasoningEffort};

use crate::boundary::{
    model::ModelSelection,
    op::{self, ReasoningEffortOverride},
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

    async fn apply_selected_model(&mut self, selection: ModelSelection) -> Result<(), Error> {
        self.submit_model_override(
            Some(selection.model.clone()),
            ReasoningEffortOverride::from_selected_effort(selection.reasoning_effort),
        )
        .await?;
        self.config.model = Some(selection.model);
        self.config.model_reasoning_effort = selection.reasoning_effort;
        Ok(())
    }

    async fn submit_model_override(
        &self,
        model: Option<String>,
        effort: ReasoningEffortOverride,
    ) -> Result<(), Error> {
        self.thread
            .submit_ok(op::override_model(model, effort))
            .await
    }
}

fn select_config_model(
    model_id: &str,
    presets: &[ModelPreset],
    configured_effort: Option<ReasoningEffort>,
) -> Result<ModelSelection, Error> {
    let preset = presets.iter().find(|preset| preset.id == model_id);
    let model = preset.map_or_else(|| model_id.to_string(), |preset| preset.model.clone());

    if model.is_empty() {
        return Err(Error::invalid_params().data("No model selected"));
    }

    let reasoning_effort = preset.map_or(configured_effort, |preset| {
        Some(effective_reasoning_effort(preset, configured_effort))
    });

    Ok(ModelSelection {
        model,
        reasoning_effort,
    })
}

fn normalized_current_model_selection(
    current_model: &str,
    presets: &[ModelPreset],
    configured_effort: Option<ReasoningEffort>,
) -> Option<ModelSelection> {
    let preset = resolve_model_preset(presets, Some(current_model))?;
    let effort = effective_reasoning_effort(preset, configured_effort);

    if preset.model == current_model && configured_effort == Some(effort) {
        return None;
    }

    Some(ModelSelection {
        model: preset.model.clone(),
        reasoning_effort: Some(effort),
    })
}

fn supports_reasoning_effort(preset: &ModelPreset, effort: ReasoningEffort) -> bool {
    preset
        .supported_reasoning_efforts
        .iter()
        .any(|supported| supported.effort == effort)
}

fn effective_reasoning_effort(
    preset: &ModelPreset,
    configured_effort: Option<ReasoningEffort>,
) -> ReasoningEffort {
    configured_effort
        .filter(|effort| supports_reasoning_effort(preset, *effort))
        .unwrap_or(preset.default_reasoning_effort)
}
