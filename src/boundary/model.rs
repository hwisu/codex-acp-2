use agent_client_protocol::schema::ModelId;
use codex_protocol::openai_models::ReasoningEffort;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelSelection {
    pub(crate) model: String,
    pub(crate) reasoning_effort: Option<ReasoningEffort>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModelIdParseError {
    MissingReasoningEffort,
    InvalidReasoningEffort(String),
}

#[allow(dead_code)]
pub(crate) fn canonical_model_id(model: &str, effort: ReasoningEffort) -> ModelId {
    ModelId::new(format!("{model}[{effort}]"))
}

pub(crate) fn parse_compound_model_id(id: &ModelId) -> Option<ModelSelection> {
    is_compound_model_id_like(id.0.as_ref())
        .then(|| require_compound_model_id(id).ok())
        .flatten()
}

pub(crate) fn require_compound_model_id(id: &ModelId) -> Result<ModelSelection, ModelIdParseError> {
    parse_optional_compound_model_id(id)?.ok_or(ModelIdParseError::MissingReasoningEffort)
}

pub(crate) fn parse_optional_compound_model_id(
    id: &ModelId,
) -> Result<Option<ModelSelection>, ModelIdParseError> {
    parse_compound_model_id_str(id.0.as_ref())
}

fn parse_compound_model_id_str(value: &str) -> Result<Option<ModelSelection>, ModelIdParseError> {
    let parsed = if let Some(value) = value.strip_suffix(']') {
        let Some((model, reasoning)) = value.rsplit_once('[') else {
            return Err(ModelIdParseError::MissingReasoningEffort);
        };
        Some((model, reasoning))
    } else if value.contains('[') {
        return Err(ModelIdParseError::MissingReasoningEffort);
    } else if let Some((model, reasoning)) = value.split_once('/') {
        Some((model, reasoning))
    } else {
        None
    };

    let Some((model, reasoning)) = parsed else {
        return Ok(None);
    };
    if model.is_empty() || reasoning.is_empty() {
        return Err(ModelIdParseError::MissingReasoningEffort);
    }
    let reasoning_effort = serde_json::from_value(reasoning.into())
        .map_err(|_| ModelIdParseError::InvalidReasoningEffort(reasoning.to_string()))?;

    Ok(Some(ModelSelection {
        model: model.to_owned(),
        reasoning_effort: Some(reasoning_effort),
    }))
}

fn is_compound_model_id_like(value: &str) -> bool {
    value.contains('[') || value.contains('/') || value.ends_with(']')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_canonical_bracket_model_id() {
        assert_eq!(
            require_compound_model_id(&ModelId::new("gpt-5.4[high]")).unwrap(),
            ModelSelection {
                model: "gpt-5.4".to_string(),
                reasoning_effort: Some(ReasoningEffort::High),
            }
        );
    }

    #[test]
    fn parses_legacy_slash_model_id() {
        assert_eq!(
            require_compound_model_id(&ModelId::new("gpt-5.4/high")).unwrap(),
            ModelSelection {
                model: "gpt-5.4".to_string(),
                reasoning_effort: Some(ReasoningEffort::High),
            }
        );
    }

    #[test]
    fn rejects_missing_reasoning_effort_for_strict_parse() {
        assert_eq!(
            require_compound_model_id(&ModelId::new("gpt-5.4")).unwrap_err(),
            ModelIdParseError::MissingReasoningEffort
        );
        assert_eq!(
            require_compound_model_id(&ModelId::new("gpt-5.4[]")).unwrap_err(),
            ModelIdParseError::MissingReasoningEffort
        );
    }

    #[test]
    fn rejects_unknown_reasoning_effort() {
        assert_eq!(
            require_compound_model_id(&ModelId::new("gpt-5.4[warp]")).unwrap_err(),
            ModelIdParseError::InvalidReasoningEffort("warp".to_string())
        );
    }

    #[test]
    fn encodes_canonical_model_id() {
        assert_eq!(
            canonical_model_id("gpt-5.4", ReasoningEffort::XHigh)
                .0
                .as_ref(),
            "gpt-5.4[xhigh]"
        );
    }
}
