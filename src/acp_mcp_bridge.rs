use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};

use acp::{
    Client, ConnectionTo, Error,
    schema::v1::{
        AgentNotification, AgentRequest, ClientResponse, ConnectMcpRequest, DisconnectMcpRequest,
        McpConnectionId, McpServerAcpId, MessageMcpNotification, MessageMcpRequest, Meta,
    },
};
use agent_client_protocol as acp;
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::{Map, Value, json};
use tokio::{
    net::TcpListener,
    sync::{Mutex, oneshot},
    task::JoinHandle,
};
use tracing::{debug, warn};

#[derive(Clone)]
pub(crate) struct AcpMcpBridge {
    url: String,
    inner: Arc<AcpMcpBridgeInner>,
}

struct AcpMcpBridgeInner {
    acp_id: McpServerAcpId,
    meta: Option<Meta>,
    state: Mutex<BridgeConnectionState>,
    shutdown_tx: StdMutex<Option<oneshot::Sender<()>>>,
    task: StdMutex<Option<JoinHandle<()>>>,
}

struct BridgeConnectionState {
    cx: ConnectionTo<Client>,
    connection_id: McpConnectionId,
}

impl AcpMcpBridge {
    pub(crate) async fn start(
        cx: ConnectionTo<Client>,
        acp_id: McpServerAcpId,
        meta: Option<Meta>,
    ) -> Result<Self, Error> {
        let connection_id = connect_mcp_server(&cx, acp_id.clone(), meta.clone()).await?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|err| {
                Error::internal_error().data(format!("failed to bind ACP MCP bridge: {err}"))
            })?;
        let local_addr = listener.local_addr().map_err(|err| {
            Error::internal_error().data(format!("failed to inspect ACP MCP bridge address: {err}"))
        })?;
        let url = format!("http://{}/mcp", format_socket_addr(local_addr));
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        let bridge = Self {
            url,
            inner: Arc::new(AcpMcpBridgeInner {
                acp_id,
                meta,
                state: Mutex::new(BridgeConnectionState { cx, connection_id }),
                shutdown_tx: StdMutex::new(Some(shutdown_tx)),
                task: StdMutex::new(None),
            }),
        };

        let app = Router::new()
            .route("/mcp", post(handle_mcp_post).delete(handle_mcp_delete))
            .with_state(bridge.clone());
        let task = tokio::spawn(async move {
            let server = axum::serve(listener, app).with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            });
            if let Err(err) = server.await {
                warn!("ACP MCP bridge server stopped with error: {err}");
            }
        });

        if let Ok(mut task_slot) = bridge.inner.task.lock() {
            *task_slot = Some(task);
        }

        Ok(bridge)
    }

    pub(crate) fn url(&self) -> &str {
        &self.url
    }

    pub(crate) async fn replace_connection(&self, cx: ConnectionTo<Client>) -> Result<(), Error> {
        let new_connection_id =
            connect_mcp_server(&cx, self.inner.acp_id.clone(), self.inner.meta.clone()).await?;
        let old_state = {
            let mut state = self.inner.state.lock().await;
            std::mem::replace(
                &mut *state,
                BridgeConnectionState {
                    cx,
                    connection_id: new_connection_id,
                },
            )
        };

        if let Err(err) = old_state
            .cx
            .send_request(AgentRequest::DisconnectMcpRequest(
                DisconnectMcpRequest::new(old_state.connection_id),
            ))
            .block_task()
            .await
        {
            debug!("failed to disconnect old ACP MCP connection during reconnect: {err:?}");
        }

        Ok(())
    }

    pub(crate) async fn shutdown(&self) {
        self.stop_http_server().await;

        let (cx, connection_id) = {
            let state = self.inner.state.lock().await;
            (state.cx.clone(), state.connection_id.clone())
        };
        if let Err(err) = cx
            .send_request(AgentRequest::DisconnectMcpRequest(
                DisconnectMcpRequest::new(connection_id),
            ))
            .block_task()
            .await
        {
            debug!("failed to disconnect ACP MCP bridge: {err:?}");
        }
    }

    async fn stop_http_server(&self) {
        let shutdown_tx = self
            .inner
            .shutdown_tx
            .lock()
            .ok()
            .and_then(|mut shutdown_tx| shutdown_tx.take());
        if let Some(shutdown_tx) = shutdown_tx {
            let _ = shutdown_tx.send(());
        }

        let task = self.inner.task.lock().ok().and_then(|mut task| task.take());
        if let Some(task) = task
            && tokio::time::timeout(Duration::from_secs(1), task)
                .await
                .is_err()
        {
            debug!("timed out waiting for ACP MCP bridge server shutdown");
        }
    }

    async fn handle_payload(&self, payload: Value) -> Option<Value> {
        match payload {
            Value::Array(items) if items.is_empty() => Some(json_rpc_error(
                None,
                -32600,
                "JSON-RPC batch must not be empty",
                None,
            )),
            Value::Array(items) => {
                let mut responses = Vec::new();
                for item in items {
                    if let Some(response) = self.handle_message(item).await {
                        responses.push(response);
                    }
                }
                if responses.is_empty() {
                    None
                } else {
                    Some(Value::Array(responses))
                }
            }
            message => self.handle_message(message).await,
        }
    }

    async fn handle_message(&self, message: Value) -> Option<Value> {
        let Value::Object(object) = message else {
            return Some(json_rpc_error(
                None,
                -32600,
                "JSON-RPC message must be an object",
                None,
            ));
        };

        let id = object.get("id").cloned();
        let Some(Value::String(method)) = object.get("method") else {
            return Some(json_rpc_error(
                id,
                -32600,
                "JSON-RPC message method must be a string",
                None,
            ));
        };
        let params = match mcp_params(object.get("params")) {
            Ok(params) => params,
            Err(message) => {
                return Some(json_rpc_error(id, -32602, message, None));
            }
        };

        match id {
            Some(id) => Some(self.forward_request(id, method.clone(), params).await),
            None => {
                if let Err(err) = self.forward_notification(method.clone(), params).await {
                    debug!("failed to forward ACP MCP notification `{method}`: {err:?}");
                }
                None
            }
        }
    }

    async fn forward_request(
        &self,
        id: Value,
        method: String,
        params: Option<Map<String, Value>>,
    ) -> Value {
        match self.send_mcp_request(method, params).await {
            Ok(result) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result,
            }),
            Err(err) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": acp_error_value(err),
            }),
        }
    }

    async fn send_mcp_request(
        &self,
        method: String,
        params: Option<Map<String, Value>>,
    ) -> Result<Value, Error> {
        let (cx, connection_id) = {
            let state = self.inner.state.lock().await;
            (state.cx.clone(), state.connection_id.clone())
        };
        let response = cx
            .send_request(AgentRequest::MessageMcpRequest(
                MessageMcpRequest::new(connection_id, method).params(params),
            ))
            .block_task()
            .await?;
        let response =
            <ClientResponse as acp::JsonRpcResponse>::from_value("mcp/message", response)?;
        let ClientResponse::MessageMcpResponse(response) = response else {
            return Err(
                Error::internal_error().data("unexpected ACP response for mcp/message request")
            );
        };
        serde_json::from_str(response.0.get()).map_err(|err| {
            Error::internal_error().data(format!("invalid ACP MCP response payload: {err}"))
        })
    }

    async fn forward_notification(
        &self,
        method: String,
        params: Option<Map<String, Value>>,
    ) -> Result<(), Error> {
        let (cx, connection_id) = {
            let state = self.inner.state.lock().await;
            (state.cx.clone(), state.connection_id.clone())
        };
        cx.send_notification(AgentNotification::MessageMcpNotification(
            MessageMcpNotification::new(connection_id, method).params(params),
        ))
    }
}

impl Drop for AcpMcpBridgeInner {
    fn drop(&mut self) {
        if let Ok(mut shutdown_tx) = self.shutdown_tx.lock()
            && let Some(shutdown_tx) = shutdown_tx.take()
        {
            let _ = shutdown_tx.send(());
        }
        if let Ok(mut task) = self.task.lock()
            && let Some(task) = task.take()
        {
            task.abort();
        }
    }
}

async fn connect_mcp_server(
    cx: &ConnectionTo<Client>,
    acp_id: McpServerAcpId,
    meta: Option<Meta>,
) -> Result<McpConnectionId, Error> {
    let response = cx
        .send_request(AgentRequest::ConnectMcpRequest(
            ConnectMcpRequest::new(acp_id).meta(meta),
        ))
        .block_task()
        .await?;
    let response = <ClientResponse as acp::JsonRpcResponse>::from_value("mcp/connect", response)?;
    let ClientResponse::ConnectMcpResponse(response) = response else {
        return Err(Error::internal_error().data("unexpected ACP response for mcp/connect request"));
    };
    Ok(response.connection_id)
}

async fn handle_mcp_post(
    State(bridge): State<AcpMcpBridge>,
    Json(payload): Json<Value>,
) -> Response {
    match bridge.handle_payload(payload).await {
        Some(response) => (StatusCode::OK, Json(response)).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

async fn handle_mcp_delete() -> StatusCode {
    StatusCode::ACCEPTED
}

fn mcp_params(params: Option<&Value>) -> Result<Option<Map<String, Value>>, &'static str> {
    match params {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(params)) => Ok(Some(params.clone())),
        Some(_) => Err("MCP-over-ACP only supports object params"),
    }
}

fn json_rpc_error(
    id: Option<Value>,
    code: i32,
    message: impl Into<String>,
    data: Option<Value>,
) -> Value {
    let mut error = Map::new();
    error.insert("code".to_string(), Value::from(code));
    error.insert("message".to_string(), Value::String(message.into()));
    if let Some(data) = data {
        error.insert("data".to_string(), data);
    }

    json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "error": Value::Object(error),
    })
}

fn acp_error_value(err: Error) -> Value {
    serde_json::to_value(err).unwrap_or_else(|err| {
        json!({
            "code": -32000,
            "message": "ACP MCP bridge error",
            "data": err.to_string(),
        })
    })
}

fn format_socket_addr(addr: SocketAddr) -> String {
    match addr {
        SocketAddr::V4(addr) => addr.to_string(),
        SocketAddr::V6(addr) => format!("[{}]:{}", addr.ip(), addr.port()),
    }
}
