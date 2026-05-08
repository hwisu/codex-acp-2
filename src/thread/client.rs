use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex, MutexGuard},
};

use agent_client_protocol::{
    Client, ConnectionTo, Error,
    schema::{
        ClientCapabilities, Implementation, RequestPermissionRequest, RequestPermissionResponse,
        SessionId, SessionNotification, SessionUpdate,
    },
};
use tracing::error;

use crate::boundary::{
    compat,
    effect::{BridgeEffect, PermissionRequestSeed},
    tool_call::ActiveCommand,
};

use super::{DISABLE_TERMINAL_OUTPUT, ENABLE_EXPERIMENTAL_TERMINAL_OUTPUT};

/// Abstraction over the ACP connection for sending notifications and requests
/// back to the client. This replaces the old `Client` trait usage.
pub(super) trait ClientSender: Send + Sync + 'static {
    fn send_session_notification(&self, notif: SessionNotification) -> Result<(), Error>;
    fn request_permission(
        &self,
        req: RequestPermissionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<RequestPermissionResponse, Error>> + Send + '_>>;
}

/// Production implementation that wraps a `ConnectionTo<Client>`.
struct AcpConnection(ConnectionTo<Client>);

impl ClientSender for AcpConnection {
    fn send_session_notification(&self, notif: SessionNotification) -> Result<(), Error> {
        self.0.send_notification(notif)
    }

    fn request_permission(
        &self,
        req: RequestPermissionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<RequestPermissionResponse, Error>> + Send + '_>> {
        Box::pin(async move { self.0.send_request(req).block_task().await })
    }
}

#[derive(Clone)]
pub(super) struct SessionClient {
    session_id: SessionId,
    client: Arc<dyn ClientSender>,
    client_capabilities: Arc<Mutex<ClientCapabilities>>,
    client_info: Arc<Mutex<Option<Implementation>>>,
}

impl SessionClient {
    pub(super) fn new(
        session_id: SessionId,
        cx: ConnectionTo<Client>,
        client_capabilities: Arc<Mutex<ClientCapabilities>>,
        client_info: Arc<Mutex<Option<Implementation>>>,
    ) -> Self {
        Self {
            session_id,
            client: Arc::new(AcpConnection(cx)),
            client_capabilities,
            client_info,
        }
    }

    #[cfg(test)]
    pub(super) fn with_client(
        session_id: SessionId,
        client: Arc<dyn ClientSender>,
        client_capabilities: Arc<Mutex<ClientCapabilities>>,
        client_info: Arc<Mutex<Option<Implementation>>>,
    ) -> Self {
        Self {
            session_id,
            client,
            client_capabilities,
            client_info,
        }
    }

    fn is_zed_client(&self) -> bool {
        {
            let client_info = lock_client_state(&self.client_info, "client info");
            compat::implementation_is_zed(client_info.as_ref())
        }
    }

    pub(super) fn supports_terminal_output(&self, active_command: &ActiveCommand) -> bool {
        if *DISABLE_TERMINAL_OUTPUT || !active_command.terminal_output {
            return false;
        }

        let client_supports_terminal_output = {
            let client_capabilities =
                lock_client_state(&self.client_capabilities, "client capabilities");
            compat::client_advertises_terminal_output(client_capabilities.meta.as_ref())
        };

        if !client_supports_terminal_output {
            return false;
        }

        // Zed currently renders command output via the display-only terminal bridge in ACP
        // `_meta`; other clients can opt in until we switch to terminal/create.
        self.is_zed_client() || *ENABLE_EXPERIMENTAL_TERMINAL_OUTPUT
    }

    pub(super) fn send_notification(&self, update: SessionUpdate) {
        if let Err(e) = self
            .client
            .send_session_notification(SessionNotification::new(self.session_id.clone(), update))
        {
            error!("Failed to send session notification: {:?}", e);
        }
    }

    pub(super) fn request_permission_effect(&self, request: PermissionRequestSeed) -> BridgeEffect {
        BridgeEffect::request_permission(self.session_id.clone(), request)
    }

    pub(super) async fn request_permission_request(
        &self,
        request: RequestPermissionRequest,
    ) -> Result<RequestPermissionResponse, Error> {
        self.client.request_permission(request).await
    }
}

fn lock_client_state<'a, T>(mutex: &'a Mutex<T>, state_name: &str) -> MutexGuard<'a, T> {
    mutex.lock().unwrap_or_else(|err| {
        error!("{state_name} mutex was poisoned; continuing with inner state");
        err.into_inner()
    })
}
