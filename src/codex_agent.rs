use acp::schema::{
    AgentAuthCapabilities, AgentCapabilities, AuthEnvVar, AuthMethod, AuthMethodAgent,
    AuthMethodEnvVar, AuthMethodId, AuthenticateRequest, AuthenticateResponse, CancelNotification,
    ClientCapabilities, CloseSessionRequest, CloseSessionResponse, Implementation,
    InitializeRequest, InitializeResponse, ListSessionsRequest, ListSessionsResponse,
    LoadSessionRequest, LoadSessionResponse, LogoutCapabilities, LogoutRequest, LogoutResponse,
    McpCapabilities, McpServer, McpServerHttp, McpServerStdio, Meta, NewSessionRequest,
    NewSessionResponse, PromptCapabilities, PromptRequest, PromptResponse, ProtocolVersion,
    SessionCapabilities, SessionCloseCapabilities, SessionId, SessionInfo, SessionListCapabilities,
    SetSessionConfigOptionRequest, SetSessionConfigOptionResponse, SetSessionModeRequest,
    SetSessionModeResponse, SetSessionModelRequest, SetSessionModelResponse,
};
use acp::{Agent, Client, ConnectTo, ConnectionTo, Error};
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
use codex_config::{AppToolApproval, McpServerConfig, McpServerEnvVar, McpServerTransportConfig};
use codex_core::{
    NewThread, RolloutRecorder, SortDirection, StateDbHandle, ThreadManager, ThreadSortKey,
    config::Config, find_thread_path_by_id_str, init_state_db, parse_cursor,
    resolve_installation_id, thread_store_from_config,
};
use codex_exec_server::{EnvironmentManager, EnvironmentManagerArgs, ExecServerRuntimePaths};
use codex_login::{
    CODEX_API_KEY_ENV_VAR, OPENAI_API_KEY_ENV_VAR,
    auth::{AuthManager, CodexAuth, read_codex_api_key_from_env, read_openai_api_key_from_env},
};
use codex_protocol::{
    ThreadId,
    protocol::{InitialHistory, RolloutItem, SessionSource},
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
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
    /// Thread manager for handling sessions
    thread_manager: ThreadManager,
    /// Optional sqlite state runtime used by Codex v0.129 thread storage APIs
    state_db: Option<StateDbHandle>,
    /// Active sessions mapped by `SessionId`
    sessions: Arc<Mutex<HashMap<SessionId, Arc<Thread>>>>,
}

const SESSION_LIST_PAGE_SIZE: usize = 25;
const SESSION_TITLE_MAX_GRAPHEMES: usize = 120;

fn lock_agent_state<'a, T>(
    mutex: &'a Mutex<T>,
    state_name: &str,
) -> Result<MutexGuard<'a, T>, Error> {
    mutex
        .lock()
        .map_err(|_| Error::internal_error().data(format!("{state_name} state is poisoned")))
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
    #[serde(alias = "experimental_environment")]
    experimental_environment: Option<String>,
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
        experimental_environment: meta.experimental_environment,
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
) -> std::io::Result<bool> {
    let Some(api_key) = api_key else {
        return Ok(false);
    };

    codex_login::login_with_api_key(
        codex_home,
        &api_key,
        codex_login::AuthCredentialsStoreMode::Ephemeral,
    )?;
    Ok(true)
}

fn seed_ephemeral_api_key_auth_from_env(codex_home: &Path) -> std::io::Result<bool> {
    seed_ephemeral_api_key_auth(
        codex_home,
        select_env_api_key_for_ephemeral_auth(
            read_codex_api_key_from_env(),
            read_openai_api_key_from_env(),
        ),
    )
}

impl CodexAgent {
    /// Create a new `CodexAgent` with the given configuration
    pub async fn new(
        config: Config,
        codex_linux_sandbox_exe: Option<PathBuf>,
    ) -> std::io::Result<Self> {
        seed_ephemeral_api_key_auth_from_env(&config.codex_home)?;
        let auth_manager = AuthManager::shared(
            config.codex_home.to_path_buf(),
            false,
            config.cli_auth_credentials_store_mode,
            Some(config.chatgpt_base_url.clone()),
        )
        .await;

        let client_capabilities: Arc<Mutex<ClientCapabilities>> = Arc::default();
        let client_info: Arc<Mutex<Option<Implementation>>> = Arc::default();
        let state_db = init_state_db(&config).await;
        let thread_store = thread_store_from_config(&config, state_db.clone());
        let installation_id = resolve_installation_id(&config.codex_home).await?;
        let thread_manager = ThreadManager::new(
            &config,
            auth_manager.clone(),
            SessionSource::Unknown,
            Arc::new(
                EnvironmentManager::new(EnvironmentManagerArgs::new(ExecServerRuntimePaths::new(
                    std::env::current_exe()?,
                    codex_linux_sandbox_exe,
                )?))
                .await,
            ),
            None,
            thread_store,
            state_db.clone(),
            installation_id,
        );
        Ok(Self {
            auth_manager,
            client_capabilities,
            client_info,
            config,
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
                acp_handler!(agent, SetSessionModelRequest, set_session_model),
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
            .connect_to(transport)
            .await
    }

    fn session_id_from_thread_id(thread_id: ThreadId) -> SessionId {
        SessionId::new(thread_id.to_string())
    }

    fn get_thread(&self, session_id: &SessionId) -> Result<Arc<Thread>, Error> {
        Ok(lock_agent_state(&self.sessions, "sessions")?
            .get(session_id)
            .ok_or_else(|| Error::resource_not_found(None))?
            .clone())
    }

    async fn check_auth(&self) -> Result<(), Error> {
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
        mcp_servers: Vec<McpServer>,
    ) -> Result<Config, Error> {
        let mut config = self.config.clone();
        config.include_apply_patch_tool = true;
        config.cwd = cwd.try_into().map_err(Error::into_internal_error)?;
        let cwd = config.cwd.clone();

        // Propagate any client-provided MCP servers that codex-rs supports.
        let mut new_mcp_servers = config.mcp_servers.get().clone();
        for mcp_server in mcp_servers {
            if let Some((name, mcp_server_config)) = convert_mcp_server(cwd.as_path(), mcp_server)?
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

        *lock_agent_state(&self.client_capabilities, "client capabilities")? = client_capabilities;
        *lock_agent_state(&self.client_info, "client info")? = client_info;

        let mut agent_capabilities = AgentCapabilities::new()
            .prompt_capabilities(PromptCapabilities::new().embedded_context(true).image(true))
            .mcp_capabilities(McpCapabilities::new().http(true))
            .load_session(true)
            .auth(AgentAuthCapabilities::new().logout(LogoutCapabilities::new()));

        agent_capabilities.session_capabilities = SessionCapabilities::new()
            .close(SessionCloseCapabilities::new())
            .list(SessionListCapabilities::new());

        let mut auth_methods = vec![
            CodexAuthMethod::ChatGpt.into(),
            CodexAuthMethod::CodexApiKey.into(),
            CodexAuthMethod::OpenAiApiKey.into(),
        ];
        // Until codex device code auth works, we can't use this in remote ssh projects
        if std::env::var("NO_BROWSER").is_ok() {
            auth_methods.remove(0);
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
        let auth_method = CodexAuthMethod::try_from(request.method_id)?;

        // Check before starting login flow if already authenticated with the same method
        if let Some(auth) = self.auth_manager.auth().await {
            match (auth, auth_method) {
                (
                    CodexAuth::ApiKey(..),
                    CodexAuthMethod::CodexApiKey | CodexAuthMethod::OpenAiApiKey,
                )
                | (CodexAuth::Chatgpt(..), CodexAuthMethod::ChatGpt) => {
                    return Ok(AuthenticateResponse::new());
                }
                _ => {}
            }
        }

        match auth_method {
            CodexAuthMethod::ChatGpt => {
                // Perform browser/device login via codex-rs, then report success/failure to the client.
                let opts = codex_login::ServerOptions::new(
                    self.config.codex_home.to_path_buf(),
                    codex_login::auth::CLIENT_ID.to_string(),
                    None,
                    self.config.cli_auth_credentials_store_mode,
                );

                let server =
                    codex_login::run_login_server(opts).map_err(Error::into_internal_error)?;

                server
                    .block_until_done()
                    .await
                    .map_err(Error::into_internal_error)?;
            }
            CodexAuthMethod::CodexApiKey => {
                let api_key = read_codex_api_key_from_env().ok_or_else(|| {
                    Error::internal_error().data(format!("{CODEX_API_KEY_ENV_VAR} is not set"))
                })?;
                Self::login_with_api_key(
                    &self.config.codex_home,
                    &api_key,
                    self.config.cli_auth_credentials_store_mode,
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
                )?;
            }
        }

        self.auth_manager.reload().await;

        Ok(AuthenticateResponse::new())
    }

    async fn logout(&self, _request: LogoutRequest) -> Result<LogoutResponse, Error> {
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
    ) -> Result<(), Error> {
        codex_login::login_with_api_key(codex_home, api_key, store_mode)
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
            cwd, mcp_servers, ..
        } = request;
        info!("Creating new session with cwd: {}", cwd.display());

        let config = self.build_session_config(&cwd, mcp_servers)?;
        let num_mcp_servers = config.mcp_servers.len();

        let NewThread {
            thread_id,
            thread,
            session_configured: _,
        } = Box::pin(self.thread_manager.start_thread(config.clone()))
            .await
            .map_err(|_e| Error::internal_error())?;

        let session_id = Self::session_id_from_thread_id(thread_id);
        let thread = self.create_thread(session_id.clone(), thread_id, thread, config, cx);
        let load = thread.load().await?;

        lock_agent_state(&self.sessions, "sessions")?.insert(session_id.clone(), thread);

        debug!("Created new session with {} MCP servers", num_mcp_servers);

        Ok(NewSessionResponse::new(session_id)
            .modes(load.modes)
            .models(load.models)
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
            mcp_servers,
            ..
        } = request;

        let existing_thread = {
            lock_agent_state(&self.sessions, "sessions")?
                .get(&session_id)
                .cloned()
        };
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
                .models(load.models)
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

        let config = self.build_session_config(&cwd, mcp_servers)?;

        let NewThread {
            thread_id,
            thread,
            session_configured: _,
        } = Box::pin(self.thread_manager.resume_thread_from_rollout(
            config.clone(),
            rollout_path,
            self.auth_manager.clone(),
            None,
        ))
        .await
        .map_err(|e| Error::internal_error().data(e.to_string()))?;

        let thread = self.create_thread(session_id.clone(), thread_id, thread, config.clone(), cx);

        thread.replay_history(rollout_items).await?;

        let load = thread.load().await?;

        lock_agent_state(&self.sessions, "sessions")?.insert(session_id, thread);

        Ok(LoadSessionResponse::new()
            .modes(load.modes)
            .models(load.models)
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

                Some(
                    SessionInfo::new(SessionId::new(thread_id.to_string()), item_cwd)
                        .title(title)
                        .updated_at(updated_at),
                )
            })
            .collect::<Vec<_>>();

        let next_cursor = page
            .next_cursor
            .as_ref()
            .and_then(|next_cursor| serde_json::to_value(next_cursor).ok())
            .and_then(|value| value.as_str().map(str::to_owned));

        Ok(ListSessionsResponse::new(sessions).next_cursor(next_cursor))
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
        lock_agent_state(&self.sessions, "sessions")?.remove(&request.session_id);
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

    async fn set_session_model(
        &self,
        args: SetSessionModelRequest,
    ) -> Result<SetSessionModelResponse, Error> {
        info!("Setting session model for session: {}", args.session_id);

        self.get_thread(&args.session_id)?
            .set_model(args.model_id)
            .await?;

        Ok(SetSessionModelResponse::default())
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexAuthMethod {
    ChatGpt,
    CodexApiKey,
    OpenAiApiKey,
}

impl From<CodexAuthMethod> for AuthMethodId {
    fn from(method: CodexAuthMethod) -> Self {
        Self::new(match method {
            CodexAuthMethod::ChatGpt => "chatgpt",
            CodexAuthMethod::CodexApiKey => "codex-api-key",
            CodexAuthMethod::OpenAiApiKey => "openai-api-key",
        })
    }
}

impl From<CodexAuthMethod> for AuthMethod {
    fn from(method: CodexAuthMethod) -> Self {
        match method {
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
        }
    }
}

impl TryFrom<AuthMethodId> for CodexAuthMethod {
    type Error = Error;

    fn try_from(value: AuthMethodId) -> Result<Self, Self::Error> {
        match value.0.as_ref() {
            "chatgpt" => Ok(Self::ChatGpt),
            "codex-api-key" => Ok(Self::CodexApiKey),
            "openai-api-key" => Ok(Self::OpenAiApiKey),
            _ => Err(Error::invalid_params().data("unsupported authentication method")),
        }
    }
}

fn truncate_graphemes(text: &str, max_graphemes: usize) -> String {
    let mut graphemes = text.grapheme_indices(true);

    if let Some((byte_index, _)) = graphemes.nth(max_graphemes) {
        if max_graphemes >= 3 {
            let mut truncate_graphemes = text.grapheme_indices(true);
            if let Some((truncate_byte_index, _)) = truncate_graphemes.nth(max_graphemes - 3) {
                let truncated = &text[..truncate_byte_index];
                format!("{truncated}...")
            } else {
                text.to_string()
            }
        } else {
            let truncated = &text[..byte_index];
            truncated.to_string()
        }
    } else {
        text.to_string()
    }
}

fn format_session_title(message: &str) -> Option<String> {
    let normalized = message.replace(['\r', '\n'], " ");
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(truncate_graphemes(trimmed, SESSION_TITLE_MAX_GRAPHEMES))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acp::schema::{EnvVariable, HttpHeader, McpServerSse};
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
        let auth_file = codex_home.join("auth.json");

        assert!(seed_ephemeral_api_key_auth(
            &codex_home,
            Some("sk-ephemeral".to_string())
        )?);

        let auth_manager = AuthManager::shared(
            codex_home.clone(),
            false,
            codex_login::AuthCredentialsStoreMode::File,
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
        ));
        drop(std::fs::remove_dir_all(&codex_home));
        Ok(())
    }

    #[tokio::test]
    async fn missing_ephemeral_api_key_preserves_existing_storage_auth() -> anyhow::Result<()> {
        let codex_home = temp_codex_home();
        codex_login::login_with_api_key(
            &codex_home,
            "sk-stored",
            codex_login::AuthCredentialsStoreMode::File,
        )?;

        assert!(!seed_ephemeral_api_key_auth(&codex_home, None)?);

        let auth_manager = AuthManager::shared(
            codex_home.clone(),
            false,
            codex_login::AuthCredentialsStoreMode::File,
            None,
        )
        .await;
        let auth = auth_manager.auth().await.expect("stored auth should load");

        assert_eq!(auth.api_key(), Some("sk-stored"));

        drop(codex_login::logout(
            &codex_home,
            codex_login::AuthCredentialsStoreMode::File,
        ));
        drop(std::fs::remove_dir_all(&codex_home));
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
        assert_eq!(
            config.experimental_environment.as_deref(),
            Some("remote-linux")
        );
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
        assert_eq!(config.experimental_environment.as_deref(), Some("sandbox"));
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
