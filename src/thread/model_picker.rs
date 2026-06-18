use codex_protocol::openai_models::{ModelPreset, ReasoningEffort};

/// Resolve a model preset using the standard fallback chain:
/// 1. Match by model name
/// 2. Fall back to default preset
/// 3. Fall back to first available preset
pub(super) fn resolve_model_preset<'a>(
    presets: &'a [ModelPreset],
    current_model: Option<&str>,
) -> Option<&'a ModelPreset> {
    current_model
        .and_then(|model| presets.iter().find(|p| p.model == model))
        .or_else(|| presets.iter().find(|p| p.is_default))
        .or_else(|| presets.first())
}

pub(super) fn filter_model_presets_for_picker(
    presets: Vec<ModelPreset>,
    current_model: Option<&str>,
) -> Vec<ModelPreset> {
    presets
        .into_iter()
        .filter_map(|mut preset| {
            let is_current =
                current_model.is_some_and(|model| model == preset.model || model == preset.id);
            if !model_is_gpt_5_3_or_newer(&preset.model)
                || !(preset.show_in_picker || preset.is_default || is_current)
            {
                return None;
            }

            preset
                .supported_reasoning_efforts
                .retain(|effort| reasoning_effort_is_high_or_higher(&effort.effort));
            if preset.supported_reasoning_efforts.is_empty() {
                return None;
            }

            if !reasoning_effort_is_high_or_higher(&preset.default_reasoning_effort) {
                preset.default_reasoning_effort =
                    preset.supported_reasoning_efforts[0].effort.clone();
            }

            Some(preset)
        })
        .collect()
}

fn model_is_gpt_5_3_or_newer(model: &str) -> bool {
    let Some(version) = model.strip_prefix("gpt-") else {
        return false;
    };
    let Some((major, rest)) = version.split_once('.') else {
        return false;
    };
    let Ok(major) = major.parse::<u32>() else {
        return false;
    };
    let minor = rest
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    let Ok(minor) = minor.parse::<u32>() else {
        return false;
    };

    major > 5 || (major == 5 && minor >= 3)
}

fn reasoning_effort_is_high_or_higher(effort: &ReasoningEffort) -> bool {
    matches!(effort, ReasoningEffort::High | ReasoningEffort::XHigh)
}
