use super::actor::ThreadActor;
use agent_client_protocol::{
    Error,
    schema::{ContentBlock, PromptRequest, StopReason},
};
use codex_protocol::{
    protocol::Op, request_user_input::RequestUserInputEvent, user_input::UserInput,
};
use itertools::Itertools;
use tokio::sync::oneshot;
use tracing::info;

use crate::user_input::{
    PendingUserInputRequest, parse_request_user_input_response, request_user_input_prompt_text,
};

use super::{
    deps::Auth,
    prompt_items::build_prompt_items,
    slash_commands::PromptSubmission,
    submission::{PromptState, SubmissionState},
};

impl<A: Auth> ThreadActor<A> {
    pub(super) fn prompt_request_text(prompt: Vec<ContentBlock>) -> String {
        build_prompt_items(prompt)
            .into_iter()
            .filter_map(|item| match item {
                UserInput::Text { text, .. } => Some(text),
                _ => None,
            })
            .join("\n")
    }

    pub(super) fn register_pending_user_input(
        &mut self,
        submission_id: String,
        event: RequestUserInputEvent,
    ) {
        let pending_request = PendingUserInputRequest::from_event(submission_id, event);
        self.client
            .send_agent_text(request_user_input_prompt_text(&pending_request));
        self.state.set_pending_user_input(pending_request);
    }

    pub(super) async fn handle_pending_user_input_prompt(
        &mut self,
        request: PromptRequest,
    ) -> Result<oneshot::Receiver<Result<StopReason, Error>>, Error> {
        let pending_request = self
            .state
            .pending_user_input()
            .cloned()
            .ok_or_else(|| Error::internal_error().data("No pending request_user_input"))?;
        let (response_tx, response_rx) = oneshot::channel();
        let response = parse_request_user_input_response(
            &pending_request,
            &Self::prompt_request_text(request.prompt),
        )?;

        if !self.has_submission(&pending_request.submission_id) {
            self.insert_submission(
                pending_request.submission_id.clone(),
                SubmissionState::Prompt(PromptState::for_replay(
                    pending_request.submission_id.clone(),
                    self.thread.clone(),
                    self.resolution_tx(),
                )),
            );
        }

        self.thread
            .submit_ok(Op::UserInputAnswer {
                id: pending_request.turn_id.clone(),
                response,
            })
            .await
            .map_err(|e| Error::internal_error().data(e.to_string()))?;

        let Some(submission) = self.submission_mut(&pending_request.submission_id) else {
            self.state.clear_pending_user_input();
            return Err(Error::internal_error().data(format!(
                "Missing active submission for request_user_input {}",
                pending_request.call_id
            )));
        };

        submission.add_response_tx(response_tx);
        self.state.clear_pending_user_input();

        Ok(response_rx)
    }

    pub(super) async fn handle_prompt(
        &mut self,
        request: PromptRequest,
    ) -> Result<oneshot::Receiver<Result<StopReason, Error>>, Error> {
        if self.state.has_pending_user_input() {
            return self.handle_pending_user_input_prompt(request).await;
        }

        let (response_tx, response_rx) = oneshot::channel();

        let op = match self
            .prompt_submission_for_items(build_prompt_items(request.prompt))
            .await?
        {
            PromptSubmission::Submit { op } => op,
            PromptSubmission::Handled { message } => {
                self.client.send_agent_text(message);
                response_tx.send(Ok(StopReason::EndTurn)).ok();
                return Ok(response_rx);
            }
        };

        let submission_id = self
            .thread
            .submit(*op)
            .await
            .map_err(|e| Error::internal_error().data(e.to_string()))?;

        info!("Submitted prompt with submission_id: {submission_id}");
        info!("Starting to wait for conversation events for submission_id: {submission_id}");

        let prompt_state = PromptState::new(
            submission_id.clone(),
            self.thread.clone(),
            self.resolution_tx(),
            response_tx,
        );
        let state = SubmissionState::Prompt(prompt_state);

        self.insert_submission(submission_id, state);

        Ok(response_rx)
    }
}
