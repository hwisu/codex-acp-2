use acp::schema::{
    ProtocolVersion,
    v1::{
        AgentAuthCapabilities, AgentCapabilities, AuthEnvVar, AuthMethod, AuthMethodAgent,
        AuthMethodEnvVar, AuthMethodId, AuthenticateRequest, AuthenticateResponse,
        CancelNotification, ClientCapabilities, CloseSessionRequest, CloseSessionResponse,
        DeleteSessionRequest, DeleteSessionResponse, Implementation, InitializeRequest,
        InitializeResponse, ListSessionsRequest, ListSessionsResponse, LoadSessionRequest,
        LoadSessionResponse, LogoutCapabilities, LogoutRequest, LogoutResponse, McpCapabilities,
        McpServer, McpServerHttp, McpServerStdio, Meta, NewSessionRequest, NewSessionResponse,
        PromptCapabilities, PromptRequest, PromptResponse, ResumeSessionRequest,
        ResumeSessionResponse, SessionAdditionalDirectoriesCapabilities, SessionCapabilities,
        SessionCloseCapabilities, SessionConfigId, SessionConfigOptionValue,
        SessionDeleteCapabilities, SessionId, SessionInfo, SessionListCapabilities,
        SessionResumeCapabilities, SetSessionConfigOptionRequest, SetSessionConfigOptionResponse,
        SetSessionModeRequest, SetSessionModeResponse,
    },
};
use acp::{Agent, Client, ConnectTo, ConnectionTo, Error, Handled, Responder, UntypedMessage};
use agent_client_protocol as acp;

use crate::boundary::constants::meta as boundary_meta;

macro_rules! acp_handler {
    ($agent:expr, $req:ty, $method:ident) => {{
        let agent = $agent.clone();
        async move |request: $req, responder, cx: ConnectionTo<Client>| {
            let agent = agent.clone();
            cx.spawn(async move { responder.respond_with_result(agent.$method(request).await) })?;
            Ok(())
        }
    }};
}

macro_rules! acp_handler_with_cx {
    ($agent:expr, $req:ty, $method:ident) => {{
        let agent = $agent.clone();
        async move |request: $req, responder, cx: ConnectionTo<Client>| {
            let agent = agent.clone();
            let session_cx = cx.clone();
            cx.spawn(async move {
                responder.respond_with_result(agent.$method(request, session_cx).await)
            })?;
            Ok(())
        }
    }};
}
use codex_config::{
    AbsolutePathBuf, AppToolApproval, McpServerConfig, McpServerEnvVar, McpServerTransportConfig,
};
use codex_core::{
    NewThread, RolloutRecorder, SortDirection, StateDbHandle, ThreadManager, ThreadSortKey,
    config::Config, find_thread_path_by_id_str, init_state_db, parse_cursor,
    resolve_installation_id, thread_store_from_config,
};
use codex_exec_server::{EnvironmentManager, ExecServerRuntimePaths};
use codex_extension_api::ExtensionRegistryBuilder;
use codex_features::Feature;
use codex_home::CodexHomeUserInstructionsProvider;
use codex_login::{
    AuthKeyringBackendKind, CODEX_API_KEY_ENV_VAR, OPENAI_API_KEY_ENV_VAR,
    auth::{AuthManager, CodexAuth, read_codex_api_key_from_env, read_openai_api_key_from_env},
};
use codex_model_provider_info::{ModelProviderInfo, WireApi};
use codex_protocol::{
    ThreadId,
    protocol::{InitialHistory, RolloutItem, SessionSource},
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard},
    time::Duration,
};
use tracing::{debug, info};
use unicode_segmentation::UnicodeSegmentation;

use crate::thread::{CodexThreadImpl, Thread, ThreadInit};

/// The Codex implementation of the ACP Agent.
///
/// This bridges the ACP protocol with the existing codex-rs infrastructure,
/// allowing codex to be used as an ACP agent.
pub struct CodexAgent {
    /// Handle to the current authentication
    auth_manager: Arc<AuthManager>,
    /// Capabilities of the connected client
    client_capabilities: Arc<Mutex<ClientCapabilities>>,
    /// Information about the connected client implementation
    client_info: Arc<Mutex<Option<Implementation>>>,
    /// The underlying codex configuration
    config: Config,
    /// Custom gateway auth requested by ACP clients.
    gateway_auth: Arc<Mutex<Option<GatewayAuthConfig>>>,
    /// Thread manager for handling sessions
    thread_manager: Arc<ThreadManager>,
    /// Optional sqlite state runtime used by Codex v0.129 thread storage APIs
    state_db: Option<StateDbHandle>,
    /// Active sessions mapped by `SessionId`
    sessions: Arc<RwLock<HashMap<SessionId, Arc<Thread>>>>,
}

const SESSION_LIST_PAGE_SIZE: usize = 25;
const SESSION_TITLE_MAX_GRAPHEMES: usize = 120;
const OFFICIAL_API_KEY_AUTH_METHOD_ID: &str = "api-key";
const OFFICIAL_CHAT_GPT_AUTH_METHOD_ID: &str = "chat-gpt";
const GATEWAY_AUTH_METHOD_ID: &str = "gateway";
const CUSTOM_GATEWAY_PROVIDER_ID: &str = "custom-gateway";
const CUSTOM_GATEWAY_FEATURE_HEADER: &str = "X-Client-Feature-ID";
const AUTHENTICATION_STATUS_METHOD: &str = "authentication/status";
const AUTHENTICATION_LOGOUT_METHOD: &str = "authentication/logout";
const LEGACY_SET_SESSION_MODEL_METHOD: &str = "session/set_model";

#[derive(Debug, Clone)]
struct GatewayAuthConfig {
    base_url: String,
    headers: HashMap<String, String>,
    provider_name: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct GatewayAuthMeta {
    base_url: String,
    #[serde(default)]
    headers: HashMap<String, String>,
    provider_name: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct GatewayAuthRequestMeta {
    gateway: GatewayAuthMeta,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
struct ApiKeyAuthRequestMeta {
    api_key: ApiKeyAuthPayload,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiKeyAuthPayload {
    api_key: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacySetSessionModelParams {
    session_id: SessionId,
    model_id: String,
}

fn lock_agent_state<'a, T>(
    mutex: &'a Mutex<T>,
    state_name: &str,
) -> Result<MutexGuard<'a, T>, Error> {
    mutex
        .lock()
        .map_err(|_| Error::internal_error().data(format!("{state_name} state is poisoned")))
}

fn read_sessions(
    sessions: &RwLock<HashMap<SessionId, Arc<Thread>>>,
) -> Result<RwLockReadGuard<'_, HashMap<SessionId, Arc<Thread>>>, Error> {
    sessions
        .read()
        .map_err(|_| Error::internal_error().data("sessions state is poisoned"))
}

fn write_sessions(
    sessions: &RwLock<HashMap<SessionId, Arc<Thread>>>,
) -> Result<std::sync::RwLockWriteGuard<'_, HashMap<SessionId, Arc<Thread>>>, Error> {
    sessions
        .write()
        .map_err(|_| Error::internal_error().data("sessions state is poisoned"))
}

fn rollout_items_from_history(history: InitialHistory) -> Vec<RolloutItem> {
    match history {
        InitialHistory::Resumed(resumed) => resumed.history,
        InitialHistory::Forked(items) => items,
        InitialHistory::Cleared | InitialHistory::New => Vec::new(),
    }
}

#[derive(Debug, Default, Clone, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct CodexMcpServerMeta {
    #[serde(alias = "enabled")]
    enabled: Option<bool>,
    #[serde(alias = "required")]
    required: Option<bool>,
    #[serde(alias = "startup_timeout_sec")]
    startup_timeout_sec: Option<f64>,
    #[serde(alias = "tool_timeout_sec")]
    tool_timeout_sec: Option<f64>,
    #[serde(alias = "supports_parallel_tool_calls")]
    supports_parallel_tool_calls: Option<bool>,
    #[serde(alias = "default_tools_approval_mode")]
    default_tools_approval_mode: Option<AppToolApproval>,
    #[serde(alias = "enabled_tools")]
    enabled_tools: Option<Vec<String>>,
    #[serde(alias = "disabled_tools")]
    disabled_tools: Option<Vec<String>>,
    #[serde(alias = "scopes")]
    scopes: Option<Vec<String>>,
    #[serde(alias = "oauth_resource")]
    oauth_resource: Option<String>,
    #[serde(
        alias = "environment_id",
        alias = "experimentalEnvironment",
        alias = "experimental_environment"
    )]
    environment_id: Option<String>,
    #[serde(alias = "bearer_token_env_var")]
    bearer_token_env_var: Option<String>,
    #[serde(alias = "env_http_headers")]
    env_http_headers: Option<HashMap<String, String>>,
    #[serde(alias = "env_vars")]
    env_vars: Option<Vec<McpServerEnvVar>>,
    #[serde(alias = "cwd")]
    cwd: Option<PathBuf>,
}

fn normalize_mcp_server_name(name: &str) -> String {
    name.replace(|c: char| c.is_whitespace(), "_")
}

fn parse_meta_duration(field_name: &str, secs: Option<f64>) -> Result<Option<Duration>, Error> {
    secs.map(|secs| {
        Duration::try_from_secs_f64(secs).map_err(|err| {
            Error::invalid_params().data(format!("invalid MCP server _meta.{field_name}: {err}"))
        })
    })
    .transpose()
}

fn parse_codex_mcp_server_meta(meta: Option<Meta>) -> Result<CodexMcpServerMeta, Error> {
    let Some(meta) = meta else {
        return Ok(CodexMcpServerMeta::default());
    };

    let value = if let Some(value) = meta
        .get("codex")
        .or_else(|| meta.get(boundary_meta::CODEX_ACP))
    {
        if !value.is_object() {
            return Err(Error::invalid_params().data("MCP server _meta.codex must be an object"));
        }
        value.clone()
    } else {
        serde_json::Value::Object(meta)
    };

    serde_json::from_value(value)
        .map_err(|err| Error::invalid_params().data(format!("invalid MCP server _meta: {err}")))
}

fn resolve_mcp_server_cwd(session_cwd: &Path, override_cwd: Option<PathBuf>) -> Option<PathBuf> {
    override_cwd
        .map(|path| {
            if path.is_relative() {
                session_cwd.join(path)
            } else {
                path
            }
        })
        .or_else(|| Some(session_cwd.to_path_buf()))
}

struct ConvertedMcpServer {
    name: String,
    transport: McpServerTransportConfig,
    oauth_resource: Option<String>,
    meta: CodexMcpServerMeta,
}

fn convert_mcp_server(
    session_cwd: &Path,
    mcp_server: McpServer,
) -> Result<Option<(String, McpServerConfig)>, Error> {
    let converted = match mcp_server {
        McpServer::Sse(..) => return Ok(None),
        McpServer::Http(server) => convert_http_mcp_server(server)?,
        McpServer::Stdio(server) => convert_stdio_mcp_server(session_cwd, server)?,
        _ => return Ok(None),
    };

    build_mcp_server_config(converted).map(Some)
}

fn convert_http_mcp_server(server: McpServerHttp) -> Result<ConvertedMcpServer, Error> {
    let McpServerHttp {
        name,
        url,
        headers,
        meta,
        ..
    } = server;
    let meta = parse_codex_mcp_server_meta(meta)?;
    if meta.cwd.is_some() || meta.env_vars.is_some() {
        return Err(Error::invalid_params()
            .data("MCP server _meta.cwd and _meta.envVars are only supported for stdio servers"));
    }

    let transport = McpServerTransportConfig::StreamableHttp {
        url,
        bearer_token_env_var: meta.bearer_token_env_var.clone(),
        http_headers: if headers.is_empty() {
            None
        } else {
            Some(headers.into_iter().map(|h| (h.name, h.value)).collect())
        },
        env_http_headers: meta.env_http_headers.clone(),
    };

    Ok(ConvertedMcpServer {
        name,
        transport,
        oauth_resource: meta.oauth_resource.clone(),
        meta,
    })
}

fn convert_stdio_mcp_server(
    session_cwd: &Path,
    server: McpServerStdio,
) -> Result<ConvertedMcpServer, Error> {
    let McpServerStdio {
        name,
        command,
        args,
        env,
        meta,
        ..
    } = server;
    let meta = parse_codex_mcp_server_meta(meta)?;
    if meta.bearer_token_env_var.is_some()
        || meta.env_http_headers.is_some()
        || meta.oauth_resource.is_some()
    {
        return Err(Error::invalid_params()
            .data("HTTP-only MCP server meta fields are not supported for stdio servers"));
    }

    let env_vars = meta.env_vars.clone().unwrap_or_default();
    for env_var in &env_vars {
        env_var
            .validate_source()
            .map_err(|err| Error::invalid_params().data(err))?;
    }

    let transport = McpServerTransportConfig::Stdio {
        command: command.display().to_string(),
        args,
        env: if env.is_empty() {
            None
        } else {
            Some(env.into_iter().map(|env| (env.name, env.value)).collect())
        },
        env_vars,
        cwd: resolve_mcp_server_cwd(session_cwd, meta.cwd.clone()),
    };

    Ok(ConvertedMcpServer {
        name,
        transport,
        oauth_resource: None,
        meta,
    })
}

fn build_mcp_server_config(
    converted: ConvertedMcpServer,
) -> Result<(String, McpServerConfig), Error> {
    let ConvertedMcpServer {
        name,
        transport,
        oauth_resource,
        meta,
    } = converted;
    let config = McpServerConfig {
        transport,
        environment_id: meta
            .environment_id
            .unwrap_or_else(|| codex_config::DEFAULT_MCP_SERVER_ENVIRONMENT_ID.to_string()),
        enabled: meta.enabled.unwrap_or(true),
        required: meta.required.unwrap_or(false),
        supports_parallel_tool_calls: meta.supports_parallel_tool_calls.unwrap_or(false),
        disabled_reason: None,
        startup_timeout_sec: parse_meta_duration("startupTimeoutSec", meta.startup_timeout_sec)?,
        tool_timeout_sec: parse_meta_duration("toolTimeoutSec", meta.tool_timeout_sec)?,
        default_tools_approval_mode: meta.default_tools_approval_mode,
        enabled_tools: meta.enabled_tools,
        disabled_tools: meta.disabled_tools,
        scopes: meta.scopes,
        oauth: None,
        oauth_resource,
        tools: HashMap::default(),
    };
    Ok((normalize_mcp_server_name(&name), config))
}

fn select_env_api_key_for_ephemeral_auth(
    codex_api_key: Option<String>,
    openai_api_key: Option<String>,
) -> Option<String> {
    codex_api_key.or(openai_api_key)
}

fn seed_ephemeral_api_key_auth(
    codex_home: &Path,
    api_key: Option<String>,
    keyring_backend_kind: AuthKeyringBackendKind,
) -> std::io::Result<bool> {
    let Some(api_key) = api_key else {
        return Ok(false);
    };

    codex_login::login_with_api_key(
        codex_home,
        &api_key,
        codex_login::AuthCredentialsStoreMode::Ephemeral,
        keyring_backend_kind,
    )?;
    Ok(true)
}

fn seed_ephemeral_api_key_auth_from_env(
    codex_home: &Path,
    keyring_backend_kind: AuthKeyringBackendKind,
) -> std::io::Result<bool> {
    seed_ephemeral_api_key_auth(
        codex_home,
        select_env_api_key_for_ephemeral_auth(
            read_codex_api_key_from_env(),
            read_openai_api_key_from_env(),
        ),
        keyring_backend_kind,
    )
}

fn meta_value(meta: &Option<Meta>, key: &str) -> Option<serde_json::Value> {
    meta.as_ref().and_then(|meta| meta.get(key)).cloned()
}

fn api_key_from_auth_meta(meta: Option<Meta>) -> Result<Option<String>, Error> {
    let Some(meta) = meta else {
        return Ok(None);
    };
    let ApiKeyAuthRequestMeta { api_key } = serde_json::from_value(serde_json::Value::Object(meta))
        .map_err(|err| {
            Error::invalid_params().data(format!("invalid api-key authentication metadata: {err}"))
        })?;
    Ok(Some(api_key.api_key))
}

fn gateway_auth_from_meta(meta: Option<Meta>) -> Result<GatewayAuthConfig, Error> {
    let Some(meta) = meta else {
        return Err(Error::invalid_params().data("gateway authentication requires _meta.gateway"));
    };
    let GatewayAuthRequestMeta { gateway } =
        serde_json::from_value(serde_json::Value::Object(meta)).map_err(|err| {
            Error::invalid_params().data(format!("invalid gateway authentication metadata: {err}"))
        })?;

    if gateway.base_url.trim().is_empty() {
        return Err(Error::invalid_params().data("gateway baseUrl must not be empty"));
    }

    Ok(GatewayAuthConfig {
        base_url: gateway.base_url,
        headers: gateway.headers,
        provider_name: gateway
            .provider_name
            .unwrap_or_else(|| CUSTOM_GATEWAY_PROVIDER_ID.to_string()),
    })
}

fn client_supports_gateway_auth(client_capabilities: &ClientCapabilities) -> bool {
    client_capabilities
        .auth
        .meta
        .as_ref()
        .and_then(|meta| meta.get(GATEWAY_AUTH_METHOD_ID))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn additional_directories_from_meta(meta: &Option<Meta>) -> Vec<PathBuf> {
    meta_value(meta, "additionalRoots")
        .or_else(|| meta_value(meta, "additionalDirectories"))
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| value.as_str().map(PathBuf::from))
        .collect()
}

fn read_additional_directories(
    cwd: &Path,
    mut additional_directories: Vec<PathBuf>,
    meta: Option<Meta>,
) -> Result<Vec<PathBuf>, Error> {
    if additional_directories.is_empty() {
        additional_directories = additional_directories_from_meta(&meta);
    }

    let mut seen = HashSet::from([cwd.to_path_buf()]);
    let mut normalized = Vec::new();

    for directory in additional_directories {
        if !directory.is_absolute() {
            return Err(Error::invalid_params().data(format!(
                "additionalDirectories entries must be absolute: {}",
                directory.display()
            )));
        }

        AbsolutePathBuf::try_from(directory.clone()).map_err(|err| {
            Error::invalid_params().data(format!(
                "invalid additionalDirectories entry {}: {err}",
                directory.display()
            ))
        })?;

        if seen.insert(directory.clone()) {
            normalized.push(directory);
        }
    }

    Ok(normalized)
}

fn effective_config_has_feature_setting(
    effective_config: &codex_config::TomlValue,
    active_profile: Option<&str>,
    feature: Feature,
) -> bool {
    let key = feature.key();
    effective_config
        .get("features")
        .and_then(|features| features.get(key))
        .is_some()
        || active_profile.is_some_and(|profile| {
            effective_config
                .get("profiles")
                .and_then(|profiles| profiles.get(profile))
                .and_then(|profile| profile.get("features"))
                .and_then(|features| features.get(key))
                .is_some()
        })
}

fn apply_codex_acp_default_features(config: &mut Config) {
    let effective_config = config.config_layer_stack.effective_config();
    let active_profile = config.permissions.active_permission_profile();
    if effective_config_has_feature_setting(
        &effective_config,
        active_profile.as_ref().map(|profile| profile.id.as_str()),
        Feature::Goals,
    ) {
        return;
    }

    if let Err(err) = config.features.enable(Feature::Goals) {
        debug!(?err, "Unable to enable goals feature by default");
    }
}

impl CodexAgent {
    /// Create a new `CodexAgent` with the given configuration
    pub async fn new(
        mut config: Config,
        codex_linux_sandbox_exe: Option<PathBuf>,
    ) -> std::io::Result<Self> {
        apply_codex_acp_default_features(&mut config);
        let auth_keyring_backend_kind = config.auth_keyring_backend_kind();
        seed_ephemeral_api_key_auth_from_env(&config.codex_home, auth_keyring_backend_kind)?;
        let auth_manager = AuthManager::shared(
            config.codex_home.to_path_buf(),
            false,
            config.cli_auth_credentials_store_mode,
            config.forced_chatgpt_workspace_id.clone(),
            Some(config.chatgpt_base_url.clone()),
            auth_keyring_backend_kind,
            config.auth_route_config(),
        )
        .await;

        let client_capabilities: Arc<Mutex<ClientCapabilities>> = Arc::default();
        let client_info: Arc<Mutex<Option<Implementation>>> = Arc::default();
        let state_db = init_state_db(&config).await;
        let thread_store = thread_store_from_config(&config, state_db.clone());
        let installation_id = resolve_installation_id(&config.codex_home).await?;
        let environment_manager = Arc::new(
            EnvironmentManager::from_codex_home(
                config.codex_home.as_path(),
                Some(ExecServerRuntimePaths::new(
                    std::env::current_exe()?,
                    codex_linux_sandbox_exe,
                )?),
            )
            .await
            .map_err(std::io::Error::other)?,
        );
        let goal_service = crate::thread::goal_service();
        let analytics_events_client = codex_analytics::AnalyticsEventsClient::disabled();
        let thread_manager = Arc::new_cyclic(|thread_manager| {
            let mut extensions = ExtensionRegistryBuilder::new();
            if let Some(state_db) = state_db.clone() {
                codex_goal_extension::install_with_backend(
                    &mut extensions,
                    state_db,
                    analytics_events_client.clone(),
                    None,
                    thread_manager.clone(),
                    goal_service.clone(),
                    |config: &Config| config.features.enabled(Feature::Goals),
                );
            }
            ThreadManager::new(
                &config,
                auth_manager.clone(),
                SessionSource::Unknown,
                environment_manager.clone(),
                Arc::new(extensions.build()),
                Arc::new(CodexHomeUserInstructionsProvider::new(
                    config.codex_home.clone(),
                )),
                None,
                Arc::clone(&thread_store),
                state_db.clone(),
                installation_id.clone(),
                None,
                None,
            )
        });
        Ok(Self {
            auth_manager,
            client_capabilities,
            client_info,
            config,
            gateway_auth: Arc::default(),
            thread_manager,
            state_db,
            sessions: Arc::default(),
        })
    }

    /// Build and run the ACP agent, serving requests over the given transport.
    pub async fn serve(
        self: Arc<Self>,
        transport: impl ConnectTo<Agent> + 'static,
    ) -> acp::Result<()> {
        let agent = self;
        Agent
            .builder()
            .name("codex-acp")
            .on_receive_request(
                {
                    let agent = agent.clone();
                    async move |request: InitializeRequest, responder, _cx| {
                        responder.respond_with_result(agent.initialize(request).await)
                    }
                },
                acp::on_receive_request!(),
            )
            .on_receive_request(
                acp_handler!(agent, AuthenticateRequest, authenticate),
                acp::on_receive_request!(),
            )
            .on_receive_request(
                acp_handler!(agent, LogoutRequest, logout),
                acp::on_receive_request!(),
            )
            .on_receive_request(
                acp_handler_with_cx!(agent, NewSessionRequest, new_session),
                acp::on_receive_request!(),
            )
            .on_receive_request(
                acp_handler_with_cx!(agent, LoadSessionRequest, load_session),
                acp::on_receive_request!(),
            )
            .on_receive_request(
                acp_handler!(agent, ListSessionsRequest, list_sessions),
                acp::on_receive_request!(),
            )
            .on_receive_request(
                acp_handler!(agent, DeleteSessionRequest, delete_session),
                acp::on_receive_request!(),
            )
            .on_receive_request(
                acp_handler_with_cx!(agent, ResumeSessionRequest, resume_session),
                acp::on_receive_request!(),
            )
            .on_receive_request(
                acp_handler!(agent, CloseSessionRequest, close_session),
                acp::on_receive_request!(),
            )
            .on_receive_request(
                acp_handler!(agent, PromptRequest, prompt),
                acp::on_receive_request!(),
            )
            .on_receive_notification(
                {
                    let agent = agent.clone();
                    async move |notification: CancelNotification, cx: ConnectionTo<Client>| {
                        let agent = agent.clone();
                        cx.spawn(async move {
                            if let Err(e) = agent.cancel(notification).await {
                                tracing::error!("Error handling cancel: {:?}", e);
                            }
                            Ok(())
                        })?;
                        Ok(())
                    }
                },
                acp::on_receive_notification!(),
            )
            .on_receive_request(
                acp_handler!(agent, SetSessionModeRequest, set_session_mode),
                acp::on_receive_request!(),
            )
            .on_receive_request(
                acp_handler!(
                    agent,
                    SetSessionConfigOptionRequest,
                    set_session_config_option
                ),
                acp::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let agent = agent.clone();
                    async move |request: UntypedMessage,
                                responder: Responder<serde_json::Value>,
                                cx: ConnectionTo<Client>| {
                        let agent = agent.clone();
                        match request.method.as_str() {
                            AUTHENTICATION_STATUS_METHOD
                            | AUTHENTICATION_LOGOUT_METHOD
                            | LEGACY_SET_SESSION_MODEL_METHOD => {
                                cx.spawn(async move {
                                    responder.respond_with_result(
                                        agent.handle_extension_request(request).await,
                                    )
                                })?;
                                Ok(Handled::Yes)
                            }
                            _ => Ok(Handled::No {
                                message: (request, responder),
                                retry: false,
                            }),
                        }
                    }
                },
                acp::on_receive_request!(),
            )
            .connect_to(transport)
            .await
    }

    fn session_id_from_thread_id(thread_id: ThreadId) -> SessionId {
        SessionId::new(thread_id.to_string())
    }

    fn get_thread(&self, session_id: &SessionId) -> Result<Arc<Thread>, Error> {
        let thread = read_sessions(&self.sessions)?.get(session_id).cloned();
        thread.ok_or_else(|| Error::resource_not_found(None))
    }

    fn gateway_auth(&self) -> Result<Option<GatewayAuthConfig>, Error> {
        Ok(lock_agent_state(&self.gateway_auth, "gateway auth")?.clone())
    }

    async fn check_auth(&self) -> Result<(), Error> {
        if self.gateway_auth()?.is_some() {
            return Ok(());
        }

        if self.config.model_provider_id == "openai"
            && self.auth_manager.auth().await.is_none()
            // Check if anything changed on disk since the last reload
            && !self.auth_manager.reload().await
        {
            return Err(Error::auth_required());
        }
        Ok(())
    }

    /// Build a session config from base config, working directory, and MCP servers.
    /// This is shared between `new_session` and `load_session`.
    fn build_session_config(
        &self,
        cwd: &Path,
        additional_directories: &[PathBuf],
        mcp_servers: Vec<McpServer>,
    ) -> Result<Config, Error> {
        let mut config = self.config.clone();
        config.cwd = cwd.try_into().map_err(Error::into_internal_error)?;

        let mut workspace_roots = Vec::with_capacity(additional_directories.len() + 1);
        workspace_roots.push(config.cwd.clone());
        for directory in additional_directories {
            workspace_roots.push(
                AbsolutePathBuf::try_from(directory.clone()).map_err(Error::into_internal_error)?,
            );
        }
        config
            .permissions
            .set_workspace_roots(workspace_roots.clone());
        config.workspace_roots = workspace_roots;
        config.workspace_roots_explicit = true;

        if let Some(gateway) = self.gateway_auth()? {
            let mut http_headers = gateway.headers;
            http_headers
                .entry(CUSTOM_GATEWAY_FEATURE_HEADER.to_string())
                .or_insert_with(|| "codex".to_string());
            let provider = ModelProviderInfo {
                name: gateway.provider_name,
                base_url: Some(gateway.base_url),
                env_key: None,
                env_key_instructions: None,
                experimental_bearer_token: None,
                auth: None,
                aws: None,
                wire_api: WireApi::Responses,
                query_params: None,
                http_headers: Some(http_headers),
                env_http_headers: None,
                request_max_retries: None,
                stream_max_retries: None,
                stream_idle_timeout_ms: None,
                websocket_connect_timeout_ms: None,
                requires_openai_auth: false,
                supports_websockets: false,
            };
            config.model_provider_id = CUSTOM_GATEWAY_PROVIDER_ID.to_string();
            config.model_provider = provider.clone();
            config
                .model_providers
                .insert(CUSTOM_GATEWAY_PROVIDER_ID.to_string(), provider);
        }

        // Propagate any client-provided MCP servers that codex-rs supports.
        let mut new_mcp_servers = config.mcp_servers.get().clone();
        for mcp_server in mcp_servers {
            if let Some((name, mcp_server_config)) =
                convert_mcp_server(config.cwd.as_path(), mcp_server)?
            {
                new_mcp_servers.insert(name, mcp_server_config);
            }
        }

        config
            .mcp_servers
            .set(new_mcp_servers)
            .map_err(|e| anyhow::anyhow!(e))?;

        Ok(config)
    }
}

impl CodexAgent {
    async fn initialize(&self, request: InitializeRequest) -> Result<InitializeResponse, Error> {
        let InitializeRequest {
            protocol_version,
            client_capabilities,
            client_info,
            ..
        } = request;
        debug!("Received initialize request with protocol version {protocol_version:?}",);
        let protocol_version = ProtocolVersion::V1;
        let supports_gateway_auth = client_supports_gateway_auth(&client_capabilities);

        *lock_agent_state(&self.client_capabilities, "client capabilities")? = client_capabilities;
        *lock_agent_state(&self.client_info, "client info")? = client_info;

        let mut agent_capabilities = AgentCapabilities::new()
            .prompt_capabilities(PromptCapabilities::new().embedded_context(true).image(true))
            .mcp_capabilities(McpCapabilities::new().http(true))
            .load_session(true)
            .auth(AgentAuthCapabilities::new().logout(LogoutCapabilities::new()));

        agent_capabilities.session_capabilities = SessionCapabilities::new()
            .close(SessionCloseCapabilities::new())
            .list(SessionListCapabilities::new())
            .delete(SessionDeleteCapabilities::new())
            .resume(SessionResumeCapabilities::new())
            .additional_directories(SessionAdditionalDirectoriesCapabilities::new());

        let mut auth_methods = vec![
            CodexAuthMethod::OfficialApiKey.into(),
            CodexAuthMethod::OfficialChatGpt.into(),
            CodexAuthMethod::ChatGpt.into(),
            CodexAuthMethod::CodexApiKey.into(),
            CodexAuthMethod::OpenAiApiKey.into(),
        ];
        if supports_gateway_auth {
            auth_methods.push(CodexAuthMethod::Gateway.into());
        }
        // Until codex device code auth works, we can't use this in remote ssh projects
        if std::env::var("NO_BROWSER").is_ok() {
            auth_methods.retain(|method: &AuthMethod| method.id().0.as_ref() != "chatgpt");
            auth_methods.retain(|method: &AuthMethod| {
                method.id().0.as_ref() != OFFICIAL_CHAT_GPT_AUTH_METHOD_ID
            });
        }

        Ok(InitializeResponse::new(protocol_version)
            .agent_capabilities(agent_capabilities)
            .agent_info(Implementation::new("codex-acp", env!("CARGO_PKG_VERSION")).title("Codex"))
            .auth_methods(auth_methods))
    }

    async fn authenticate(
        &self,
        request: AuthenticateRequest,
    ) -> Result<AuthenticateResponse, Error> {
        let AuthenticateRequest {
            method_id, meta, ..
        } = request;
        let auth_method = CodexAuthMethod::try_from(method_id)?;

        if auth_method == CodexAuthMethod::Gateway {
            let gateway = gateway_auth_from_meta(meta)?;
            *lock_agent_state(&self.gateway_auth, "gateway auth")? = Some(gateway);
            return Ok(AuthenticateResponse::new());
        }

        // Check before starting login flow if already authenticated with the same method
        if let Some(auth) = self.auth_manager.auth().await {
            match (auth, auth_method) {
                (
                    CodexAuth::ApiKey(..),
                    CodexAuthMethod::OfficialApiKey
                    | CodexAuthMethod::CodexApiKey
                    | CodexAuthMethod::OpenAiApiKey,
                )
                | (
                    CodexAuth::Chatgpt(..) | CodexAuth::ChatgptAuthTokens(..),
                    CodexAuthMethod::OfficialChatGpt | CodexAuthMethod::ChatGpt,
                ) => {
                    return Ok(AuthenticateResponse::new());
                }
                _ => {}
            }
        }

        match auth_method {
            CodexAuthMethod::OfficialChatGpt | CodexAuthMethod::ChatGpt => {
                // Perform browser/device login via codex-rs, then report success/failure to the client.
                let opts = codex_login::ServerOptions::new(
                    self.config.codex_home.to_path_buf(),
                    codex_login::auth::CLIENT_ID.to_string(),
                    self.config.forced_chatgpt_workspace_id.clone(),
                    self.config.cli_auth_credentials_store_mode,
                    self.config.auth_keyring_backend_kind(),
                    self.config.auth_route_config(),
                );

                let server =
                    codex_login::run_login_server(opts).map_err(Error::into_internal_error)?;

                server
                    .block_until_done()
                    .await
                    .map_err(Error::into_internal_error)?;
            }
            CodexAuthMethod::OfficialApiKey => {
                let api_key = api_key_from_auth_meta(meta)?
                    .or_else(|| {
                        select_env_api_key_for_ephemeral_auth(
                            read_codex_api_key_from_env(),
                            read_openai_api_key_from_env(),
                        )
                    })
                    .ok_or_else(|| {
                        Error::internal_error().data(format!(
                            "{CODEX_API_KEY_ENV_VAR} or {OPENAI_API_KEY_ENV_VAR} is not set"
                        ))
                    })?;
                Self::login_with_api_key(
                    &self.config.codex_home,
                    &api_key,
                    self.config.cli_auth_credentials_store_mode,
                    self.config.auth_keyring_backend_kind(),
                )?;
            }
            CodexAuthMethod::CodexApiKey => {
                let api_key = read_codex_api_key_from_env().ok_or_else(|| {
                    Error::internal_error().data(format!("{CODEX_API_KEY_ENV_VAR} is not set"))
                })?;
                Self::login_with_api_key(
                    &self.config.codex_home,
                    &api_key,
                    self.config.cli_auth_credentials_store_mode,
                    self.config.auth_keyring_backend_kind(),
                )?;
            }
            CodexAuthMethod::OpenAiApiKey => {
                let api_key = read_openai_api_key_from_env().ok_or_else(|| {
                    Error::internal_error().data(format!("{OPENAI_API_KEY_ENV_VAR} is not set"))
                })?;
                Self::login_with_api_key(
                    &self.config.codex_home,
                    &api_key,
                    self.config.cli_auth_credentials_store_mode,
                    self.config.auth_keyring_backend_kind(),
                )?;
            }
            CodexAuthMethod::Gateway => unreachable!("gateway auth returns before login flow"),
        }

        *lock_agent_state(&self.gateway_auth, "gateway auth")? = None;
        self.auth_manager.reload().await;

        Ok(AuthenticateResponse::new())
    }

    async fn logout(&self, _request: LogoutRequest) -> Result<LogoutResponse, Error> {
        *lock_agent_state(&self.gateway_auth, "gateway auth")? = None;
        self.auth_manager
            .logout()
            .await
            .map_err(Error::into_internal_error)?;
        Ok(LogoutResponse::new())
    }

    fn create_thread(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
        thread: Arc<dyn CodexThreadImpl>,
        config: Config,
        additional_directories: Vec<PathBuf>,
        cx: ConnectionTo<Client>,
    ) -> Arc<Thread> {
        Arc::new(Thread::new(ThreadInit {
            session_id,
            thread_id,
            thread,
            auth: self.auth_manager.clone(),
            models_manager: Arc::new(self.thread_manager.get_models_manager()),
            client_capabilities: self.client_capabilities.clone(),
            client_info: self.client_info.clone(),
            config,
            additional_directories,
            cx,
        }))
    }

    async fn live_session_rollout_items(&self, session_id: &SessionId) -> Vec<RolloutItem> {
        match find_thread_path_by_id_str(
            &self.config.codex_home,
            session_id.0.as_ref(),
            self.state_db.as_deref(),
        )
        .await
        {
            Ok(Some(rollout_path)) => {
                match RolloutRecorder::get_rollout_history(&rollout_path).await {
                    Ok(history) => rollout_items_from_history(history),
                    Err(err) => {
                        info!("Skipping replay for live session {session_id}: {err}");
                        Vec::new()
                    }
                }
            }
            Ok(None) => Vec::new(),
            Err(err) => {
                info!("Skipping replay for live session {session_id}: {err}");
                Vec::new()
            }
        }
    }

    fn login_with_api_key(
        codex_home: &Path,
        api_key: &str,
        store_mode: codex_login::AuthCredentialsStoreMode,
        keyring_backend_kind: AuthKeyringBackendKind,
    ) -> Result<(), Error> {
        codex_login::login_with_api_key(codex_home, api_key, store_mode, keyring_backend_kind)
            .map_err(Error::into_internal_error)
    }

    async fn new_session(
        &self,
        request: NewSessionRequest,
        cx: ConnectionTo<Client>,
    ) -> Result<NewSessionResponse, Error> {
        // Check before sending if authentication was successful or not
        self.check_auth().await?;

        let NewSessionRequest {
            cwd,
            additional_directories,
            mcp_servers,
            meta,
            ..
        } = request;
        info!("Creating new session with cwd: {}", cwd.display());

        let additional_directories =
            read_additional_directories(&cwd, additional_directories, meta)?;
        let config = self.build_session_config(&cwd, &additional_directories, mcp_servers)?;
        let num_mcp_servers = config.mcp_servers.len();

        let NewThread {
            thread_id,
            thread,
            session_configured: _,
        } = Box::pin(self.thread_manager.start_thread(config.clone()))
            .await
            .map_err(|_e| Error::internal_error())?;

        let session_id = Self::session_id_from_thread_id(thread_id);
        let thread = self.create_thread(
            session_id.clone(),
            thread_id,
            thread,
            config,
            additional_directories,
            cx,
        );
        let load = thread.load().await?;

        write_sessions(&self.sessions)?.insert(session_id.clone(), thread);

        debug!("Created new session with {} MCP servers", num_mcp_servers);

        Ok(NewSessionResponse::new(session_id)
            .modes(load.modes)
            .config_options(load.config_options))
    }

    async fn load_session(
        &self,
        request: LoadSessionRequest,
        cx: ConnectionTo<Client>,
    ) -> Result<LoadSessionResponse, Error> {
        info!("Loading session: {}", request.session_id);
        // Check before sending if authentication was successful or not
        self.check_auth().await?;

        let LoadSessionRequest {
            session_id,
            cwd,
            additional_directories,
            mcp_servers,
            meta,
            ..
        } = request;
        let additional_directories =
            read_additional_directories(&cwd, additional_directories, meta)?;

        let existing_thread = { read_sessions(&self.sessions)?.get(&session_id).cloned() };
        if let Some(thread) = existing_thread {
            let rollout_items = self.live_session_rollout_items(&session_id).await;
            thread
                .replace_client(
                    session_id.clone(),
                    cx,
                    self.client_capabilities.clone(),
                    self.client_info.clone(),
                )
                .await?;
            if !rollout_items.is_empty() {
                thread.replay_history(rollout_items).await?;
            }
            let load = thread.load().await?;

            return Ok(LoadSessionResponse::new()
                .modes(load.modes)
                .config_options(load.config_options));
        }

        let rollout_path = find_thread_path_by_id_str(
            &self.config.codex_home,
            session_id.0.as_ref(),
            self.state_db.as_deref(),
        )
        .await
        .map_err(|e| Error::internal_error().data(e.to_string()))?
        .ok_or_else(|| Error::resource_not_found(None))?;

        let history = RolloutRecorder::get_rollout_history(&rollout_path)
            .await
            .map_err(|e| Error::internal_error().data(e.to_string()))?;

        let rollout_items = rollout_items_from_history(history);

        let config = self.build_session_config(&cwd, &additional_directories, mcp_servers)?;

        let NewThread {
            thread_id,
            thread,
            session_configured: _,
        } = Box::pin(self.thread_manager.resume_thread_from_rollout(
            config.clone(),
            rollout_path,
            self.auth_manager.clone(),
            None,
            false,
        ))
        .await
        .map_err(|e| Error::internal_error().data(e.to_string()))?;

        let thread = self.create_thread(
            session_id.clone(),
            thread_id,
            thread,
            config.clone(),
            additional_directories,
            cx,
        );

        thread.replay_history(rollout_items).await?;

        let load = thread.load().await?;

        write_sessions(&self.sessions)?.insert(session_id, thread);

        Ok(LoadSessionResponse::new()
            .modes(load.modes)
            .config_options(load.config_options))
    }

    async fn resume_session(
        &self,
        request: ResumeSessionRequest,
        cx: ConnectionTo<Client>,
    ) -> Result<ResumeSessionResponse, Error> {
        info!("Resuming session: {}", request.session_id);
        self.check_auth().await?;

        let ResumeSessionRequest {
            session_id,
            cwd,
            additional_directories,
            mcp_servers,
            meta,
            ..
        } = request;
        let additional_directories =
            read_additional_directories(&cwd, additional_directories, meta)?;

        let existing_thread = { read_sessions(&self.sessions)?.get(&session_id).cloned() };
        if let Some(thread) = existing_thread {
            thread
                .replace_client(
                    session_id.clone(),
                    cx,
                    self.client_capabilities.clone(),
                    self.client_info.clone(),
                )
                .await?;
            let load = thread.load().await?;

            return Ok(ResumeSessionResponse::new()
                .modes(load.modes)
                .config_options(load.config_options));
        }

        let rollout_path = find_thread_path_by_id_str(
            &self.config.codex_home,
            session_id.0.as_ref(),
            self.state_db.as_deref(),
        )
        .await
        .map_err(|e| Error::internal_error().data(e.to_string()))?
        .ok_or_else(|| Error::resource_not_found(None))?;

        let config = self.build_session_config(&cwd, &additional_directories, mcp_servers)?;

        let NewThread {
            thread_id,
            thread,
            session_configured: _,
        } = Box::pin(self.thread_manager.resume_thread_from_rollout(
            config.clone(),
            rollout_path,
            self.auth_manager.clone(),
            None,
            false,
        ))
        .await
        .map_err(|e| Error::internal_error().data(e.to_string()))?;

        let thread = self.create_thread(
            session_id.clone(),
            thread_id,
            thread,
            config,
            additional_directories,
            cx,
        );
        let load = thread.load().await?;

        write_sessions(&self.sessions)?.insert(session_id, thread);

        Ok(ResumeSessionResponse::new()
            .modes(load.modes)
            .config_options(load.config_options))
    }

    async fn list_sessions(
        &self,
        request: ListSessionsRequest,
    ) -> Result<ListSessionsResponse, Error> {
        self.check_auth().await?;

        let ListSessionsRequest { cwd, cursor, .. } = request;
        let cursor_obj = cursor.as_deref().and_then(parse_cursor);

        let page = RolloutRecorder::list_threads(
            self.state_db.clone(),
            &self.config,
            SESSION_LIST_PAGE_SIZE,
            cursor_obj.as_ref(),
            ThreadSortKey::UpdatedAt,
            SortDirection::Desc,
            &[
                SessionSource::Cli,
                SessionSource::VSCode,
                SessionSource::Unknown,
            ],
            None,
            None,
            self.config.model_provider_id.as_str(),
            None,
        )
        .await
        .map_err(|err| Error::internal_error().data(format!("failed to list sessions: {err}")))?;

        let sessions = page
            .items
            .into_iter()
            .filter_map(|item| {
                let thread_id = item.thread_id?;
                let item_cwd = item.cwd?;

                if let Some(filter_cwd) = cwd.as_ref()
                    && item_cwd != *filter_cwd
                {
                    return None;
                }

                let title = item
                    .first_user_message
                    .as_deref()
                    .and_then(format_session_title);
                let updated_at = item.updated_at.or(item.created_at);

                Some({
                    let session_id = SessionId::new(thread_id.to_string());
                    let mut info = SessionInfo::new(session_id.clone(), item_cwd)
                        .title(title)
                        .updated_at(updated_at);
                    if let Some(thread) = read_sessions(&self.sessions)
                        .ok()?
                        .get(&session_id)
                        .cloned()
                    {
                        info =
                            info.additional_directories(thread.additional_directories().to_vec());
                    }
                    info
                })
            })
            .collect::<Vec<_>>();

        let next_cursor = page
            .next_cursor
            .as_ref()
            .and_then(|next_cursor| serde_json::to_value(next_cursor).ok())
            .and_then(|value| value.as_str().map(str::to_owned));

        Ok(ListSessionsResponse::new(sessions).next_cursor(next_cursor))
    }

    async fn delete_session(
        &self,
        request: DeleteSessionRequest,
    ) -> Result<DeleteSessionResponse, Error> {
        let DeleteSessionRequest { session_id, .. } = request;
        let thread_id = ThreadId::from_string(&session_id.0).map_err(Error::into_internal_error)?;

        let active_thread = { write_sessions(&self.sessions)?.remove(&session_id) };
        let mut deleted = active_thread.is_some();
        if let Some(thread) = active_thread {
            thread.shutdown().await?;
            self.thread_manager.remove_thread(&thread_id).await;
        }

        let rollout_path = find_thread_path_by_id_str(
            &self.config.codex_home,
            session_id.0.as_ref(),
            self.state_db.as_deref(),
        )
        .await
        .map_err(|e| Error::internal_error().data(e.to_string()))?;

        if let Some(rollout_path) = rollout_path {
            match tokio::fs::remove_file(&rollout_path).await {
                Ok(()) => deleted = true,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => {
                    return Err(Error::internal_error()
                        .data(format!("failed to delete rollout file: {err}")));
                }
            }
        }

        if let Some(state_db) = self.state_db.as_ref() {
            let rows = state_db
                .delete_thread(thread_id)
                .await
                .map_err(|err| Error::internal_error().data(err.to_string()))?;
            deleted |= rows > 0;
        }

        if deleted {
            Ok(DeleteSessionResponse::new())
        } else {
            Err(Error::resource_not_found(None))
        }
    }

    async fn close_session(
        &self,
        request: CloseSessionRequest,
    ) -> Result<CloseSessionResponse, Error> {
        self.get_thread(&request.session_id)?.shutdown().await?;
        self.thread_manager
            .remove_thread(
                &ThreadId::from_string(&request.session_id.0)
                    .map_err(Error::into_internal_error)?,
            )
            .await;
        write_sessions(&self.sessions)?.remove(&request.session_id);
        Ok(CloseSessionResponse::new())
    }

    async fn prompt(&self, request: PromptRequest) -> Result<PromptResponse, Error> {
        info!("Processing prompt for session: {}", request.session_id);
        // Check before sending if authentication was successful or not
        self.check_auth().await?;

        // Get the session state
        let thread = self.get_thread(&request.session_id)?;
        let stop_reason = thread.prompt(request).await?;

        Ok(PromptResponse::new(stop_reason))
    }

    async fn cancel(&self, args: CancelNotification) -> Result<(), Error> {
        info!("Cancelling operations for session: {}", args.session_id);
        self.get_thread(&args.session_id)?.cancel().await?;
        Ok(())
    }

    async fn set_session_mode(
        &self,
        args: SetSessionModeRequest,
    ) -> Result<SetSessionModeResponse, Error> {
        info!("Setting session mode for session: {}", args.session_id);
        self.get_thread(&args.session_id)?
            .set_mode(args.mode_id)
            .await?;
        Ok(SetSessionModeResponse::default())
    }

    async fn set_session_config_option(
        &self,
        args: SetSessionConfigOptionRequest,
    ) -> Result<SetSessionConfigOptionResponse, Error> {
        info!(
            "Setting session config option for session: {} (config_id: {}, value: {:?})",
            args.session_id, args.config_id.0, args.value
        );

        let thread = self.get_thread(&args.session_id)?;

        thread.set_config_option(args.config_id, args.value).await?;

        let config_options = thread.config_options().await?;

        Ok(SetSessionConfigOptionResponse::new(config_options))
    }

    async fn handle_extension_request(
        &self,
        request: UntypedMessage,
    ) -> Result<serde_json::Value, Error> {
        let (method, params) = request.into_parts();
        match method.as_str() {
            AUTHENTICATION_STATUS_METHOD => self.authentication_status().await,
            AUTHENTICATION_LOGOUT_METHOD => {
                self.logout(LogoutRequest::new()).await?;
                Ok(serde_json::json!({}))
            }
            LEGACY_SET_SESSION_MODEL_METHOD => {
                let params: LegacySetSessionModelParams =
                    serde_json::from_value(params).map_err(|err| {
                        Error::invalid_params()
                            .data(format!("invalid session/set_model params: {err}"))
                    })?;
                let thread = self.get_thread(&params.session_id)?;
                thread
                    .set_config_option(
                        SessionConfigId::new("model"),
                        SessionConfigOptionValue::value_id(params.model_id),
                    )
                    .await?;
                Ok(serde_json::json!({}))
            }
            _ => Err(Error::method_not_found()),
        }
    }

    async fn authentication_status(&self) -> Result<serde_json::Value, Error> {
        if let Some(gateway) = self.gateway_auth()? {
            return Ok(serde_json::json!({
                "type": "gateway",
                "name": gateway.provider_name,
            }));
        }

        let auth = self.auth_manager.auth().await;
        let status = match auth {
            Some(CodexAuth::ApiKey(..)) => serde_json::json!({ "type": "api-key" }),
            Some(auth) => {
                if let Some(email) = auth.get_account_email() {
                    serde_json::json!({ "type": "chat-gpt", "email": email })
                } else if matches!(
                    auth,
                    CodexAuth::Chatgpt(..) | CodexAuth::ChatgptAuthTokens(..)
                ) {
                    serde_json::json!({ "type": "chat-gpt", "email": "" })
                } else {
                    serde_json::json!({ "type": "api-key" })
                }
            }
            None => serde_json::json!({ "type": "unauthenticated" }),
        };
        Ok(status)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexAuthMethod {
    OfficialApiKey,
    OfficialChatGpt,
    ChatGpt,
    CodexApiKey,
    OpenAiApiKey,
    Gateway,
}

impl From<CodexAuthMethod> for AuthMethodId {
    fn from(method: CodexAuthMethod) -> Self {
        Self::new(match method {
            CodexAuthMethod::OfficialApiKey => OFFICIAL_API_KEY_AUTH_METHOD_ID,
            CodexAuthMethod::OfficialChatGpt => OFFICIAL_CHAT_GPT_AUTH_METHOD_ID,
            CodexAuthMethod::ChatGpt => "chatgpt",
            CodexAuthMethod::CodexApiKey => "codex-api-key",
            CodexAuthMethod::OpenAiApiKey => "openai-api-key",
            CodexAuthMethod::Gateway => GATEWAY_AUTH_METHOD_ID,
        })
    }
}

impl From<CodexAuthMethod> for AuthMethod {
    fn from(method: CodexAuthMethod) -> Self {
        match method {
            CodexAuthMethod::OfficialApiKey => {
                let mut meta = Meta::new();
                meta.insert(
                    OFFICIAL_API_KEY_AUTH_METHOD_ID.to_string(),
                    serde_json::json!({ "provider": "openai" }),
                );
                Self::Agent(
                    AuthMethodAgent::new(method, "API Key")
                        .description("Use an API key to authenticate")
                        .meta(meta),
                )
            }
            CodexAuthMethod::OfficialChatGpt => Self::Agent(
                AuthMethodAgent::new(method, "ChatGPT").description("Use ChatGPT to authenticate"),
            ),
            CodexAuthMethod::ChatGpt => Self::Agent(
                AuthMethodAgent::new(method, "Login with ChatGPT").description(
                    "Use your ChatGPT login with Codex CLI (requires a paid ChatGPT subscription)",
                ),
            ),
            CodexAuthMethod::CodexApiKey => Self::EnvVar(
                AuthMethodEnvVar::new(
                    method,
                    format!("Use {CODEX_API_KEY_ENV_VAR}"),
                    vec![AuthEnvVar::new(CODEX_API_KEY_ENV_VAR)],
                )
                .description(format!(
                    "Requires setting the `{CODEX_API_KEY_ENV_VAR}` environment variable."
                )),
            ),
            CodexAuthMethod::OpenAiApiKey => Self::EnvVar(
                AuthMethodEnvVar::new(
                    method,
                    format!("Use {OPENAI_API_KEY_ENV_VAR}"),
                    vec![AuthEnvVar::new(OPENAI_API_KEY_ENV_VAR)],
                )
                .description(format!(
                    "Requires setting the `{OPENAI_API_KEY_ENV_VAR}` environment variable."
                )),
            ),
            CodexAuthMethod::Gateway => {
                let mut meta = Meta::new();
                meta.insert(
                    GATEWAY_AUTH_METHOD_ID.to_string(),
                    serde_json::json!({
                        "protocol": "openai",
                        "restartRequired": false,
                    }),
                );
                Self::Agent(
                    AuthMethodAgent::new(method, "Custom model gateway")
                        .description("Use a custom gateway to authenticate and access models")
                        .meta(meta),
                )
            }
        }
    }
}

impl TryFrom<AuthMethodId> for CodexAuthMethod {
    type Error = Error;

    fn try_from(value: AuthMethodId) -> Result<Self, Self::Error> {
        match value.0.as_ref() {
            OFFICIAL_API_KEY_AUTH_METHOD_ID => Ok(Self::OfficialApiKey),
            OFFICIAL_CHAT_GPT_AUTH_METHOD_ID => Ok(Self::OfficialChatGpt),
            "chatgpt" => Ok(Self::ChatGpt),
            "codex-api-key" => Ok(Self::CodexApiKey),
            "openai-api-key" => Ok(Self::OpenAiApiKey),
            GATEWAY_AUTH_METHOD_ID => Ok(Self::Gateway),
            _ => Err(Error::invalid_params().data("unsupported authentication method")),
        }
    }
}

fn truncate_graphemes(text: &str, max_graphemes: usize) -> std::borrow::Cow<'_, str> {
    let mut graphemes = text.grapheme_indices(true);

    if let Some((byte_index, _)) = graphemes.nth(max_graphemes) {
        if max_graphemes >= 3 {
            let mut truncate_graphemes = text.grapheme_indices(true);
            if let Some((truncate_byte_index, _)) = truncate_graphemes.nth(max_graphemes - 3) {
                let truncated = &text[..truncate_byte_index];
                format!("{truncated}...").into()
            } else {
                text.into()
            }
        } else {
            let truncated = &text[..byte_index];
            truncated.to_string().into()
        }
    } else {
        text.into()
    }
}

fn format_session_title(message: &str) -> Option<String> {
    let normalized = message.replace(['\r', '\n'], " ");
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(truncate_graphemes(trimmed, SESSION_TITLE_MAX_GRAPHEMES).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acp::schema::v1::{EnvVariable, HttpHeader, McpServerSse};
    use serde_json::json;

    fn poison_mutex<T>(mutex: &Mutex<T>) {
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = mutex.lock().expect("mutex should lock before poisoning");
            panic!("poison mutex for test");
        }));
        std::panic::set_hook(previous_hook);
        assert!(result.is_err());
    }

    fn temp_codex_home() -> PathBuf {
        std::env::temp_dir().join(format!("codex-acp-auth-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn ephemeral_env_auth_selection_prefers_codex_api_key() {
        assert_eq!(
            select_env_api_key_for_ephemeral_auth(
                Some("codex-key".to_string()),
                Some("openai-key".to_string())
            ),
            Some("codex-key".to_string())
        );
        assert_eq!(
            select_env_api_key_for_ephemeral_auth(None, Some("openai-key".to_string())),
            Some("openai-key".to_string())
        );
        assert_eq!(select_env_api_key_for_ephemeral_auth(None, None), None);
    }

    #[tokio::test]
    async fn seed_ephemeral_api_key_auth_does_not_write_auth_json() -> anyhow::Result<()> {
        let codex_home = temp_codex_home();
        let keyring_backend_kind = AuthKeyringBackendKind::default();
        let auth_file = codex_home.join("auth.json");

        assert!(seed_ephemeral_api_key_auth(
            &codex_home,
            Some("sk-ephemeral".to_string()),
            keyring_backend_kind,
        )?);

        let auth_manager = AuthManager::shared(
            codex_home.clone(),
            false,
            codex_login::AuthCredentialsStoreMode::File,
            None,
            None,
            keyring_backend_kind,
            None,
        )
        .await;
        let auth = auth_manager
            .auth()
            .await
            .expect("ephemeral auth should load");

        assert_eq!(auth.api_key(), Some("sk-ephemeral"));
        assert!(!auth_file.exists());

        drop(codex_login::logout(
            &codex_home,
            codex_login::AuthCredentialsStoreMode::Ephemeral,
            keyring_backend_kind,
        ));
        drop(std::fs::remove_dir_all(&codex_home));
        Ok(())
    }

    #[tokio::test]
    async fn missing_ephemeral_api_key_preserves_existing_storage_auth() -> anyhow::Result<()> {
        let codex_home = temp_codex_home();
        let keyring_backend_kind = AuthKeyringBackendKind::default();
        codex_login::login_with_api_key(
            &codex_home,
            "sk-stored",
            codex_login::AuthCredentialsStoreMode::File,
            keyring_backend_kind,
        )?;

        assert!(!seed_ephemeral_api_key_auth(
            &codex_home,
            None,
            keyring_backend_kind,
        )?);

        let auth_manager = AuthManager::shared(
            codex_home.clone(),
            false,
            codex_login::AuthCredentialsStoreMode::File,
            None,
            None,
            keyring_backend_kind,
            None,
        )
        .await;
        let auth = auth_manager.auth().await.expect("stored auth should load");

        assert_eq!(auth.api_key(), Some("sk-stored"));

        drop(codex_login::logout(
            &codex_home,
            codex_login::AuthCredentialsStoreMode::File,
            keyring_backend_kind,
        ));
        drop(std::fs::remove_dir_all(&codex_home));
        Ok(())
    }

    #[test]
    fn default_goals_feature_applies_only_when_unconfigured() -> anyhow::Result<()> {
        let unconfigured = codex_config::TomlValue::Table(Default::default());
        assert!(!effective_config_has_feature_setting(
            &unconfigured,
            None,
            Feature::Goals
        ));

        let top_level: codex_config::TomlValue = serde_json::from_value(serde_json::json!({
            "features": {
                "goals": false,
            },
        }))?;
        assert!(effective_config_has_feature_setting(
            &top_level,
            None,
            Feature::Goals
        ));

        let profile: codex_config::TomlValue = serde_json::from_value(serde_json::json!({
            "profiles": {
                "toad": {
                    "features": {
                        "goals": false,
                    },
                },
            },
        }))?;
        assert!(effective_config_has_feature_setting(
            &profile,
            Some("toad"),
            Feature::Goals
        ));
        assert!(!effective_config_has_feature_setting(
            &profile,
            Some("other"),
            Feature::Goals
        ));

        Ok(())
    }

    #[test]
    fn lock_agent_state_reports_poisoned_mutex() {
        let mutex = Mutex::new(HashMap::<SessionId, Arc<Thread>>::new());
        poison_mutex(&mutex);

        let Err(err) = lock_agent_state(&mutex, "sessions") else {
            panic!("poisoned lock should fail");
        };
        assert!(format!("{err:?}").contains("sessions state is poisoned"));
    }

    #[test]
    fn convert_http_mcp_server_preserves_codex_meta() {
        let meta: Meta = serde_json::from_value(json!({
            "codex": {
                "required": true,
                "startupTimeoutSec": 1.5,
                "toolTimeoutSec": 3.25,
                "supportsParallelToolCalls": true,
                "defaultToolsApprovalMode": "approve",
                "enabledTools": ["lookup", "create"],
                "disabledTools": ["delete"],
                "scopes": ["calendar.read", "calendar.write"],
                "oauthResource": "https://api.example.com",
                "experimentalEnvironment": "remote-linux",
                "bearerTokenEnvVar": "CALENDAR_TOKEN",
                "envHttpHeaders": {
                    "X-Api-Key": "CALENDAR_API_KEY"
                }
            }
        }))
        .expect("valid meta");

        let server = McpServer::Http(
            McpServerHttp::new("Calendar Server", "https://example.com/mcp")
                .headers(vec![HttpHeader::new("X-Test", "1")])
                .meta(meta),
        );

        let (name, config) = convert_mcp_server(Path::new("/workspace"), server)
            .expect("conversion should succeed")
            .expect("HTTP server should be supported");

        assert_eq!(name, "Calendar_Server");
        assert!(config.required);
        assert_eq!(config.environment_id, "remote-linux");
        assert_eq!(
            config.startup_timeout_sec,
            Some(Duration::from_secs_f64(1.5))
        );
        assert_eq!(config.tool_timeout_sec, Some(Duration::from_secs_f64(3.25)));
        assert!(config.supports_parallel_tool_calls);
        assert_eq!(
            config.default_tools_approval_mode,
            Some(AppToolApproval::Approve)
        );
        assert_eq!(
            config.enabled_tools,
            Some(vec!["lookup".to_string(), "create".to_string()])
        );
        assert_eq!(config.disabled_tools, Some(vec!["delete".to_string()]));
        assert_eq!(
            config.scopes,
            Some(vec![
                "calendar.read".to_string(),
                "calendar.write".to_string()
            ])
        );
        assert_eq!(
            config.oauth_resource.as_deref(),
            Some("https://api.example.com")
        );

        match config.transport {
            McpServerTransportConfig::StreamableHttp {
                url,
                bearer_token_env_var,
                http_headers,
                env_http_headers,
            } => {
                assert_eq!(url, "https://example.com/mcp");
                assert_eq!(bearer_token_env_var.as_deref(), Some("CALENDAR_TOKEN"));
                assert_eq!(
                    http_headers
                        .as_ref()
                        .and_then(|headers| headers.get("X-Test"))
                        .map(String::as_str),
                    Some("1")
                );
                assert_eq!(
                    env_http_headers
                        .as_ref()
                        .and_then(|headers| headers.get("X-Api-Key"))
                        .map(String::as_str),
                    Some("CALENDAR_API_KEY")
                );
            }
            other => panic!("unexpected transport: {other:?}"),
        }
    }

    #[test]
    fn convert_stdio_mcp_server_preserves_codex_meta() {
        let meta: Meta = serde_json::from_value(json!({
            "codex": {
                "enabled": true,
                "required": true,
                "startupTimeoutSec": 2.0,
                "toolTimeoutSec": 5.0,
                "supportsParallelToolCalls": true,
                "defaultToolsApprovalMode": "prompt",
                "enabledTools": ["search"],
                "scopes": ["docs.read"],
                "experimentalEnvironment": "sandbox",
                "cwd": "tools/mcp",
                "envVars": [
                    "TOKEN",
                    { "name": "REMOTE_TOKEN", "source": "remote" }
                ]
            }
        }))
        .expect("valid meta");

        let server = McpServer::Stdio(
            McpServerStdio::new("Docs Server", PathBuf::from("/bin/docs-mcp"))
                .args(vec!["serve".to_string()])
                .env(vec![EnvVariable::new("FOO", "bar")])
                .meta(meta),
        );

        let (name, config) = convert_mcp_server(Path::new("/workspace"), server)
            .expect("conversion should succeed")
            .expect("stdio server should be supported");

        assert_eq!(name, "Docs_Server");
        assert!(config.required);
        assert_eq!(config.environment_id, "sandbox");
        assert_eq!(config.startup_timeout_sec, Some(Duration::from_secs(2)));
        assert_eq!(config.tool_timeout_sec, Some(Duration::from_secs(5)));
        assert!(config.supports_parallel_tool_calls);
        assert_eq!(
            config.default_tools_approval_mode,
            Some(AppToolApproval::Prompt)
        );
        assert_eq!(config.enabled_tools, Some(vec!["search".to_string()]));
        assert_eq!(config.scopes, Some(vec!["docs.read".to_string()]));

        match config.transport {
            McpServerTransportConfig::Stdio {
                command,
                args,
                env,
                env_vars,
                cwd,
            } => {
                assert_eq!(command, "/bin/docs-mcp");
                assert_eq!(args, vec!["serve".to_string()]);
                assert_eq!(
                    env.as_ref()
                        .and_then(|env| env.get("FOO"))
                        .map(String::as_str),
                    Some("bar")
                );
                assert_eq!(env_vars.len(), 2);
                assert_eq!(env_vars[0].name(), "TOKEN");
                assert_eq!(env_vars[1].name(), "REMOTE_TOKEN");
                assert_eq!(env_vars[1].source(), Some("remote"));
                assert_eq!(cwd, Some(PathBuf::from("/workspace/tools/mcp")));
            }
            other => panic!("unexpected transport: {other:?}"),
        }
    }

    #[test]
    fn convert_mcp_server_preserves_top_level_meta() {
        let meta: Meta = serde_json::from_value(json!({
            "enabled": false,
            "required": true,
            "startupTimeoutSec": 4.0,
            "toolTimeoutSec": 8.0,
            "supportsParallelToolCalls": true,
            "defaultToolsApprovalMode": "auto",
            "enabledTools": ["read"],
            "disabledTools": ["write"],
            "scopes": ["files.read"],
            "oauthResource": "https://files.example.com"
        }))
        .expect("valid meta");

        let server = McpServer::Http(
            McpServerHttp::new("Files Server", "https://example.com/mcp").meta(meta),
        );

        let (name, config) = convert_mcp_server(Path::new("/workspace"), server)
            .expect("conversion should succeed")
            .expect("HTTP server should be supported");

        assert_eq!(name, "Files_Server");
        assert!(!config.enabled);
        assert!(config.required);
        assert_eq!(config.startup_timeout_sec, Some(Duration::from_secs(4)));
        assert_eq!(config.tool_timeout_sec, Some(Duration::from_secs(8)));
        assert!(config.supports_parallel_tool_calls);
        assert_eq!(
            config.default_tools_approval_mode,
            Some(AppToolApproval::Auto)
        );
        assert_eq!(config.enabled_tools, Some(vec!["read".to_string()]));
        assert_eq!(config.disabled_tools, Some(vec!["write".to_string()]));
        assert_eq!(config.scopes, Some(vec!["files.read".to_string()]));
        assert_eq!(
            config.oauth_resource.as_deref(),
            Some("https://files.example.com")
        );
    }

    #[test]
    fn convert_mcp_server_accepts_codex_acp_meta_alias() {
        let mut meta_value = serde_json::Map::new();
        meta_value.insert(
            boundary_meta::CODEX_ACP.to_string(),
            json!({
                "enabled": false,
                "startupTimeoutSec": 1.25,
                "enabledTools": ["read"]
            }),
        );
        let meta: Meta =
            serde_json::from_value(serde_json::Value::Object(meta_value)).expect("valid meta");

        let server = McpServer::Http(
            McpServerHttp::new("Files Server", "https://example.com/mcp").meta(meta),
        );

        let (_, config) = convert_mcp_server(Path::new("/workspace"), server)
            .expect("conversion should succeed")
            .expect("HTTP server should be supported");

        assert!(!config.enabled);
        assert_eq!(
            config.startup_timeout_sec,
            Some(Duration::from_secs_f64(1.25))
        );
        assert_eq!(config.enabled_tools, Some(vec!["read".to_string()]));
    }

    #[test]
    fn convert_mcp_server_rejects_invalid_duration_meta() {
        let meta: Meta = serde_json::from_value(json!({
            "codex": {
                "startupTimeoutSec": -1.0
            }
        }))
        .expect("valid meta");

        let server =
            McpServer::Http(McpServerHttp::new("Bad Server", "https://example.com/mcp").meta(meta));

        let err = convert_mcp_server(Path::new("/workspace"), server)
            .expect_err("negative duration should fail");
        assert!(format!("{err:?}").contains("startupTimeoutSec"));
    }

    #[test]
    fn convert_mcp_server_rejects_non_object_codex_meta() {
        let meta: Meta = serde_json::from_value(json!({
            "codex": "enabled"
        }))
        .expect("valid meta");

        let server =
            McpServer::Http(McpServerHttp::new("Bad Server", "https://example.com/mcp").meta(meta));

        let err = convert_mcp_server(Path::new("/workspace"), server)
            .expect_err("non-object codex meta should fail");
        assert!(format!("{err:?}").contains("_meta.codex must be an object"));
    }

    #[test]
    fn convert_mcp_server_ignores_sse_servers() {
        let server = McpServer::Sse(McpServerSse::new(
            "Events Server",
            "https://example.com/events",
        ));

        let converted = convert_mcp_server(Path::new("/workspace"), server)
            .expect("unsupported SSE server should be ignored without error");
        assert!(converted.is_none());
    }

    #[test]
    fn convert_mcp_server_rejects_wrong_transport_meta() {
        let meta: Meta = serde_json::from_value(json!({
            "codex": {
                "bearerTokenEnvVar": "TOKEN"
            }
        }))
        .expect("valid meta");

        let server = McpServer::Stdio(
            McpServerStdio::new("Docs Server", PathBuf::from("/bin/docs-mcp")).meta(meta),
        );

        let err = convert_mcp_server(Path::new("/workspace"), server)
            .expect_err("wrong transport meta should fail");
        assert!(format!("{err:?}").contains("HTTP-only MCP server meta fields"));
    }

    #[test]
    fn convert_http_mcp_server_rejects_stdio_only_meta() {
        let meta: Meta = serde_json::from_value(json!({
            "codex": {
                "cwd": "tools/mcp",
                "envVars": ["TOKEN"]
            }
        }))
        .expect("valid meta");

        let server =
            McpServer::Http(McpServerHttp::new("Bad Server", "https://example.com/mcp").meta(meta));

        let err = convert_mcp_server(Path::new("/workspace"), server)
            .expect_err("stdio-only meta should fail for HTTP servers");
        assert!(format!("{err:?}").contains("_meta.cwd and _meta.envVars"));
    }
}
