use std::sync::Arc;

use codex_protocol::openai_models::ReasoningEffort;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ModelId(pub(crate) Arc<str>);

#[allow(dead_code)]
impl ModelId {
    pub(crate) fn new(id: impl Into<Arc<str>>) -> Self {
        Self(id.into())
    }
}

impl From<String> for ModelId {
    fn from(s: String) -> Self {
        Self(s.into())
    }
}

impl std::fmt::Display for ModelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelSelection {
    pub(crate) model: String,
    pub(crate) reasoning_effort: Option<ReasoningEffort>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum ModelIdParseError {
    MissingReasoningEffort,
    InvalidReasoningEffort(String),
}

#[allow(dead_code)]
pub(crate) fn parse_compound_model_id(id: &ModelId) -> Option<ModelSelection> {
    is_compound_model_id_like(id.0.as_ref())
        .then(|| require_compound_model_id(id).ok())
        .flatten()
}

#[allow(dead_code)]
pub(crate) fn require_compound_model_id(id: &ModelId) -> Result<ModelSelection, ModelIdParseError> {
    parse_optional_compound_model_id(id)?.ok_or(ModelIdParseError::MissingReasoningEffort)
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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
}
