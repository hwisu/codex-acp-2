use super::*;

pub(in crate::thread::tests) async fn setup() -> anyhow::Result<(
    SessionId,
    Arc<StubClient>,
    Arc<StubCodexThread>,
    UnboundedSender<ThreadMessage>,
    tokio::task::JoinHandle<()>,
)> {
    setup_with_config(|_| Ok(())).await
}

pub(in crate::thread::tests) async fn setup_with_goals() -> anyhow::Result<(
    SessionId,
    Arc<StubClient>,
    Arc<StubCodexThread>,
    UnboundedSender<ThreadMessage>,
    tokio::task::JoinHandle<()>,
)> {
    setup_with_config(|config| {
        config.features.enable(Feature::Goals)?;
        Ok(())
    })
    .await
}

pub(in crate::thread::tests) async fn setup_with_fast_mode() -> anyhow::Result<(
    SessionId,
    Arc<StubClient>,
    Arc<StubCodexThread>,
    UnboundedSender<ThreadMessage>,
    tokio::task::JoinHandle<()>,
)> {
    setup_with_config(configure_fast_mode).await
}

async fn setup_with_config(
    configure: impl FnOnce(&mut Config) -> anyhow::Result<()>,
) -> anyhow::Result<(
    SessionId,
    Arc<StubClient>,
    Arc<StubCodexThread>,
    UnboundedSender<ThreadMessage>,
    tokio::task::JoinHandle<()>,
)> {
    let session_id = SessionId::new("test");
    let client = Arc::new(StubClient::new());
    let session_client = SessionClient::with_client(
        session_id.clone(),
        client.clone(),
        Arc::default(),
        Arc::default(),
    );
    let conversation = Arc::new(StubCodexThread::new());
    let models_manager = Arc::new(StubModelsManager);
    let mut config =
        Config::load_with_cli_overrides_and_harness_overrides(vec![], ConfigOverrides::default())
            .await?;
    configure(&mut config)?;
    let (message_tx, message_rx) = tokio::sync::mpsc::unbounded_channel();
    let (resolution_tx, resolution_rx) = tokio::sync::mpsc::unbounded_channel();

    let actor = ThreadActor::new(ThreadActorInit {
        auth: StubAuth,
        client: session_client,
        thread: conversation.clone(),
        thread_id: ThreadId::default(),
        models_manager,
        config,
        message_rx,
        resolution_tx,
        resolution_rx,
    });

    let handle = tokio::spawn(actor.spawn());
    Ok((session_id, client, conversation, message_tx, handle))
}

pub(in crate::thread::tests) async fn setup_actor() -> anyhow::Result<(
    SessionId,
    Arc<StubClient>,
    Arc<StubCodexThread>,
    ThreadActor<StubAuth>,
)> {
    setup_actor_with_config(|_| Ok(())).await
}

pub(in crate::thread::tests) async fn setup_actor_with_fast_mode() -> anyhow::Result<(
    SessionId,
    Arc<StubClient>,
    Arc<StubCodexThread>,
    ThreadActor<StubAuth>,
)> {
    setup_actor_with_config(configure_fast_mode).await
}

fn configure_fast_mode(config: &mut Config) -> anyhow::Result<()> {
    config.features.enable(Feature::FastMode)?;
    config.service_tier = None;
    config.active_profile = None;
    assign_test_codex_home(config)
}

fn assign_test_codex_home(config: &mut Config) -> anyhow::Result<()> {
    let path = std::env::temp_dir().join(format!("codex-acp-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&path)?;
    config.codex_home = path.try_into()?;
    Ok(())
}

async fn setup_actor_with_config(
    configure: impl FnOnce(&mut Config) -> anyhow::Result<()>,
) -> anyhow::Result<(
    SessionId,
    Arc<StubClient>,
    Arc<StubCodexThread>,
    ThreadActor<StubAuth>,
)> {
    let session_id = SessionId::new("test");
    let client = Arc::new(StubClient::new());
    let session_client = SessionClient::with_client(
        session_id.clone(),
        client.clone(),
        Arc::default(),
        Arc::default(),
    );
    let conversation = Arc::new(StubCodexThread::new());
    let models_manager = Arc::new(StubModelsManager);
    let mut config =
        Config::load_with_cli_overrides_and_harness_overrides(vec![], ConfigOverrides::default())
            .await?;
    configure(&mut config)?;
    let (_message_tx, message_rx) = tokio::sync::mpsc::unbounded_channel();
    let (resolution_tx, resolution_rx) = tokio::sync::mpsc::unbounded_channel();

    Ok((
        session_id,
        client,
        conversation.clone(),
        ThreadActor::new(ThreadActorInit {
            auth: StubAuth,
            client: session_client,
            thread: conversation,
            thread_id: ThreadId::default(),
            models_manager,
            config,
            message_rx,
            resolution_tx,
            resolution_rx,
        }),
    ))
}

pub(in crate::thread::tests) async fn submit_prompt(
    session_id: &SessionId,
    message_tx: &UnboundedSender<ThreadMessage>,
    prompt: impl Into<String>,
) -> anyhow::Result<tokio::sync::oneshot::Receiver<Result<StopReason, Error>>> {
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    message_tx.send(ThreadMessage::Prompt {
        request: PromptRequest::new(session_id.clone(), vec![prompt.into().into()]),
        response_tx,
    })?;

    Ok(response_rx.await??)
}

pub(in crate::thread::tests) async fn submit_prompt_and_wait(
    session_id: &SessionId,
    message_tx: &UnboundedSender<ThreadMessage>,
    prompt: impl Into<String>,
) -> anyhow::Result<StopReason> {
    Ok(submit_prompt(session_id, message_tx, prompt)
        .await?
        .await??)
}
