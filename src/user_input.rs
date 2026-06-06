use std::collections::HashMap;

use agent_client_protocol::schema::Error;
use codex_protocol::request_user_input::{
    RequestUserInputAnswer, RequestUserInputEvent, RequestUserInputQuestion,
    RequestUserInputResponse,
};
use itertools::Itertools;

pub(crate) const REQUEST_USER_INPUT_OTHER_OPTION_LABEL: &str = "None of the above";

#[derive(Clone)]
pub(crate) struct PendingUserInputRequest {
    pub submission_id: String,
    pub call_id: String,
    pub turn_id: String,
    pub questions: Vec<RequestUserInputQuestion>,
}

impl PendingUserInputRequest {
    pub fn from_event(submission_id: String, event: RequestUserInputEvent) -> Self {
        Self {
            submission_id,
            call_id: event.call_id,
            turn_id: event.turn_id,
            questions: event.questions,
        }
    }
}

pub(crate) fn request_user_input_prompt_text(request: &PendingUserInputRequest) -> String {
    let intro = if request.questions.len() == 1 {
        "Additional input is required before Codex can continue.\nThis ACP client renders structured questions as plain text. Reply with your answer in the next message."
    } else {
        "Additional input is required before Codex can continue.\nThis ACP client renders structured questions as plain text. Reply in the next message with one line per question using `<id>: <answer>`."
    };

    let guidance = "For option questions, `<answer>` can be the option number or label. Append `| note` for extra context. Send `/cancel` to submit an empty response.";

    let questions = request
        .questions
        .iter()
        .enumerate()
        .map(|(index, question)| {
            let mut lines = vec![format!(
                "{}. {} [{}]",
                index + 1,
                question.question,
                question.id
            )];
            if let Some(options) = question.options.as_ref()
                && !options.is_empty()
            {
                lines.extend(options.iter().enumerate().map(|(option_index, option)| {
                    format!(
                        "   {}. {} - {}",
                        option_index + 1,
                        option.label,
                        option.description
                    )
                }));
                if question.is_other {
                    lines.push(format!(
                        "   {}. {}",
                        options.len() + 1,
                        REQUEST_USER_INPUT_OTHER_OPTION_LABEL
                    ));
                }
            } else {
                lines.push("   Reply with freeform text.".to_string());
            }
            if question.is_secret {
                lines.push("   This answer is marked secret.".to_string());
            }
            lines.join("\n")
        })
        .join("\n\n");

    format!("{intro}\n\n{guidance}\n\n{questions}")
}

pub(crate) fn normalize_request_user_input_token(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

pub(crate) fn match_request_user_input_option(
    question: &RequestUserInputQuestion,
    answer: &str,
) -> Option<String> {
    let options = question.options.as_ref()?;
    if options.is_empty() {
        return None;
    }

    let trimmed = answer.trim();
    if let Ok(index) = trimmed.parse::<usize>() {
        if (1..=options.len()).contains(&index) {
            return Some(options[index - 1].label.clone());
        }
        if question.is_other && index == options.len() + 1 {
            return Some(REQUEST_USER_INPUT_OTHER_OPTION_LABEL.to_string());
        }
    }

    let normalized_answer = normalize_request_user_input_token(trimmed);
    if normalized_answer.is_empty() {
        return None;
    }

    let other_label = question
        .is_other
        .then(|| REQUEST_USER_INPUT_OTHER_OPTION_LABEL.to_string());
    let labels = options
        .iter()
        .map(|option| option.label.clone())
        .chain(other_label)
        .collect::<Vec<_>>();

    let exact_matches = labels
        .iter()
        .filter(|label| normalize_request_user_input_token(label) == normalized_answer)
        .cloned()
        .collect::<Vec<_>>();
    if let [matched] = exact_matches.as_slice() {
        return Some(matched.clone());
    }

    let prefix_matches = labels
        .iter()
        .filter(|label| {
            let normalized_label = normalize_request_user_input_token(label);
            normalized_label.starts_with(&normalized_answer)
                || normalized_answer.starts_with(&normalized_label)
        })
        .cloned()
        .collect::<Vec<_>>();
    if let [matched] = prefix_matches.as_slice() {
        return Some(matched.clone());
    }

    None
}

pub(crate) fn parse_request_user_input_answer(
    question: &RequestUserInputQuestion,
    raw_answer: &str,
) -> Result<RequestUserInputAnswer, Error> {
    let trimmed = raw_answer.trim();
    let mut answers = Vec::new();

    if let Some(options) = question.options.as_ref()
        && !options.is_empty()
    {
        let (selection_part, note_part) = trimmed
            .split_once('|')
            .map(|(selection, note)| (selection.trim(), Some(note.trim())))
            .unwrap_or((trimmed, None));

        if let Some(label) = match_request_user_input_option(question, selection_part) {
            answers.push(label.clone());
            if let Some(note) = note_part.filter(|note| !note.is_empty()) {
                answers.push(format!("user_note: {note}"));
            }
            return Ok(RequestUserInputAnswer { answers });
        }

        if question.is_other && !trimmed.is_empty() {
            answers.push(REQUEST_USER_INPUT_OTHER_OPTION_LABEL.to_string());
            answers.push(format!("user_note: {trimmed}"));
            return Ok(RequestUserInputAnswer { answers });
        }

        return Err(Error::invalid_params().data(format!(
            "Could not match `{trimmed}` to an available option for question `{}`",
            question.id
        )));
    }

    if !trimmed.is_empty() {
        answers.push(format!("user_note: {trimmed}"));
    }

    Ok(RequestUserInputAnswer { answers })
}

pub(crate) fn empty_request_user_input_response(
    request: &PendingUserInputRequest,
) -> RequestUserInputResponse {
    RequestUserInputResponse {
        answers: request
            .questions
            .iter()
            .map(|question| {
                (
                    question.id.clone(),
                    RequestUserInputAnswer { answers: vec![] },
                )
            })
            .collect(),
    }
}

pub(crate) fn parse_request_user_input_response(
    request: &PendingUserInputRequest,
    raw_response: &str,
) -> Result<RequestUserInputResponse, Error> {
    let trimmed = raw_response.trim();
    if trimmed.is_empty() || trimmed == "/cancel" || trimmed.eq_ignore_ascii_case("cancel") {
        return Ok(empty_request_user_input_response(request));
    }

    if request.questions.len() == 1 {
        let question = &request.questions[0];
        return Ok(RequestUserInputResponse {
            answers: HashMap::from([(
                question.id.clone(),
                parse_request_user_input_answer(question, trimmed)?,
            )]),
        });
    }

    let lines = trimmed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();

    if lines.len() == request.questions.len() && !lines.iter().all(|line| line.contains(':')) {
        let answers = request
            .questions
            .iter()
            .zip(lines.iter())
            .map(|(question, line)| {
                Ok((
                    question.id.clone(),
                    parse_request_user_input_answer(question, line)?,
                ))
            })
            .collect::<Result<HashMap<_, _>, Error>>()?;
        return Ok(RequestUserInputResponse { answers });
    }

    let mut answers = HashMap::new();
    for line in lines {
        let Some((raw_key, raw_value)) = line.split_once(':') else {
            return Err(Error::invalid_params().data(format!(
                "Expected `<id>: <answer>` for multi-question input, got `{line}`"
            )));
        };

        let key = raw_key.trim();
        let value = raw_value.trim();
        let question = if let Ok(index) = key.parse::<usize>() {
            request.questions.get(index.saturating_sub(1))
        } else {
            let normalized_key = normalize_request_user_input_token(key);
            request.questions.iter().find(|question| {
                normalize_request_user_input_token(&question.id) == normalized_key
                    || normalize_request_user_input_token(&question.header) == normalized_key
            })
        }
        .ok_or_else(|| {
            Error::invalid_params().data(format!("Unknown request_user_input question key `{key}`"))
        })?;

        answers.insert(
            question.id.clone(),
            parse_request_user_input_answer(question, value)?,
        );
    }

    if answers.len() != request.questions.len() {
        let missing = request
            .questions
            .iter()
            .filter(|question| !answers.contains_key(&question.id))
            .map(|question| question.id.clone())
            .join(", ");
        return Err(Error::invalid_params().data(format!("Missing answers for: {missing}")));
    }

    Ok(RequestUserInputResponse { answers })
}
