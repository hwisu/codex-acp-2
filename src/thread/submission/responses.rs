use agent_client_protocol::{Error, schema::v1::StopReason};
use tokio::sync::oneshot;

type PromptResultSender = oneshot::Sender<Result<StopReason, Error>>;

pub(super) struct PromptResponses {
    primary: Option<PromptResultSender>,
    additional: Vec<PromptResultSender>,
}

impl PromptResponses {
    pub(super) fn primary(response_tx: PromptResultSender) -> Self {
        Self {
            primary: Some(response_tx),
            additional: Vec::new(),
        }
    }

    pub(super) fn replay() -> Self {
        Self {
            primary: None,
            additional: Vec::new(),
        }
    }

    pub(super) fn is_active(&self) -> bool {
        self.primary
            .as_ref()
            .is_some_and(|response_tx| !response_tx.is_closed())
            || self
                .additional
                .iter()
                .any(|response_tx| !response_tx.is_closed())
    }

    pub(super) fn add(&mut self, response_tx: PromptResultSender) {
        self.additional.push(response_tx);
    }

    pub(super) fn send(&mut self, result: Result<StopReason, Error>) {
        if let Some(response_tx) = self.primary.take() {
            drop(response_tx.send(result.clone()));
        }
        for response_tx in self.additional.drain(..) {
            drop(response_tx.send(result.clone()));
        }
    }
}
