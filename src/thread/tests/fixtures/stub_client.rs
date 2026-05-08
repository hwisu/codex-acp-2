use super::*;

pub(in crate::thread::tests) struct StubClient {
    notifications: std::sync::Mutex<Vec<SessionNotification>>,
    permission_requests: std::sync::Mutex<Vec<RequestPermissionRequest>>,
    permission_responses: std::sync::Mutex<VecDeque<RequestPermissionResponse>>,
    block_permission_requests: Option<Arc<Notify>>,
}

impl StubClient {
    pub(in crate::thread::tests) fn new() -> Self {
        StubClient {
            notifications: std::sync::Mutex::default(),
            permission_requests: std::sync::Mutex::default(),
            permission_responses: std::sync::Mutex::default(),
            block_permission_requests: None,
        }
    }

    pub(in crate::thread::tests) fn with_permission_responses(
        responses: Vec<RequestPermissionResponse>,
    ) -> Self {
        StubClient {
            notifications: std::sync::Mutex::default(),
            permission_requests: std::sync::Mutex::default(),
            permission_responses: std::sync::Mutex::new(responses.into()),
            block_permission_requests: None,
        }
    }

    pub(in crate::thread::tests) fn with_blocked_permission_requests(
        responses: Vec<RequestPermissionResponse>,
        notify: Arc<Notify>,
    ) -> Self {
        StubClient {
            notifications: std::sync::Mutex::default(),
            permission_requests: std::sync::Mutex::default(),
            permission_responses: std::sync::Mutex::new(responses.into()),
            block_permission_requests: Some(notify),
        }
    }

    pub(in crate::thread::tests) fn tool_calls(&self) -> Vec<ToolCall> {
        self.lock_notifications()
            .iter()
            .filter_map(|notification| match &notification.update {
                SessionUpdate::ToolCall(tool_call) => Some(tool_call.clone()),
                _ => None,
            })
            .collect()
    }

    pub(in crate::thread::tests) fn tool_call_updates(&self) -> Vec<ToolCallUpdate> {
        self.lock_notifications()
            .iter()
            .filter_map(|notification| match &notification.update {
                SessionUpdate::ToolCallUpdate(update) => Some(update.clone()),
                _ => None,
            })
            .collect()
    }

    pub(in crate::thread::tests) fn completed_tool_call_updates(&self) -> Vec<ToolCallUpdate> {
        self.tool_call_updates()
            .into_iter()
            .filter(|update| update.fields.status == Some(ToolCallStatus::Completed))
            .collect()
    }

    pub(in crate::thread::tests) fn agent_texts(&self) -> Vec<String> {
        self.lock_notifications()
            .iter()
            .filter_map(|notification| match &notification.update {
                SessionUpdate::AgentMessageChunk(ContentChunk {
                    content: ContentBlock::Text(TextContent { text, .. }),
                    ..
                }) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    pub(in crate::thread::tests) fn agent_thoughts(&self) -> Vec<String> {
        self.lock_notifications()
            .iter()
            .filter_map(|notification| match &notification.update {
                SessionUpdate::AgentThoughtChunk(ContentChunk {
                    content: ContentBlock::Text(TextContent { text, .. }),
                    ..
                }) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    pub(in crate::thread::tests) fn has_agent_text(
        &self,
        mut predicate: impl FnMut(&str) -> bool,
    ) -> bool {
        self.agent_texts().iter().any(|text| predicate(text))
    }

    pub(in crate::thread::tests) fn permission_requests(&self) -> Vec<RequestPermissionRequest> {
        self.lock_permission_requests().clone()
    }

    pub(in crate::thread::tests) fn has_permission_requests(&self) -> bool {
        !self.lock_permission_requests().is_empty()
    }

    pub(in crate::thread::tests) fn notifications(&self) -> Vec<SessionNotification> {
        self.lock_notifications().clone()
    }

    fn lock_notifications(&self) -> std::sync::MutexGuard<'_, Vec<SessionNotification>> {
        self.notifications
            .lock()
            .expect("stub client notifications mutex should not be poisoned")
    }

    fn lock_permission_requests(&self) -> std::sync::MutexGuard<'_, Vec<RequestPermissionRequest>> {
        self.permission_requests
            .lock()
            .expect("stub client permission requests mutex should not be poisoned")
    }

    fn lock_permission_responses(
        &self,
    ) -> std::sync::MutexGuard<'_, VecDeque<RequestPermissionResponse>> {
        self.permission_responses
            .lock()
            .expect("stub client permission responses mutex should not be poisoned")
    }
}

impl ClientSender for StubClient {
    fn send_session_notification(&self, args: SessionNotification) -> Result<(), Error> {
        self.lock_notifications().push(args);
        Ok(())
    }

    fn request_permission(
        &self,
        args: RequestPermissionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<RequestPermissionResponse, Error>> + Send + '_>> {
        Box::pin(async move {
            self.lock_permission_requests().push(args);
            if let Some(notify) = &self.block_permission_requests {
                notify.notified().await;
            }
            Ok(self
                .lock_permission_responses()
                .pop_front()
                .unwrap_or_else(|| {
                    RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled)
                }))
        })
    }
}
