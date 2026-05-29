use super::fixtures::*;

use crate::boundary::constants::permission_option;

#[tokio::test]
async fn test_exec_approval_uses_available_decisions() -> anyhow::Result<()> {
    let session_id = SessionId::new("test");
    let client = Arc::new(StubClient::with_permission_responses(vec![
        RequestPermissionResponse::new(RequestPermissionOutcome::Selected(
            SelectedPermissionOutcome::new(permission_option::DENIED),
        )),
    ]));
    let session_client =
        SessionClient::with_client(session_id, client.clone(), Arc::default(), Arc::default());
    let thread = Arc::new(StubCodexThread::new());
    let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
    let (message_tx, mut message_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut prompt_state = PromptState::new(
        "submission-id".to_string(),
        thread.clone(),
        message_tx,
        response_tx,
    );

    prompt_state.exec_approval(
        &session_client,
        test_fixtures::exec_approval_request(
            "call-id",
            "turn-id",
            std::env::current_dir()?.try_into()?,
            vec!["echo".to_string(), "hi".to_string()],
            vec![ParsedCommand::Unknown {
                cmd: "echo hi".to_string(),
            }],
            Some(vec![ReviewDecision::Approved, ReviewDecision::Denied]),
        ),
    )?;

    let ThreadMessage::PermissionRequestResolved {
        submission_id,
        request_key,
        response,
    } = message_rx.recv().await.unwrap()
    else {
        panic!("expected permission resolution message");
    };
    assert_eq!(submission_id, "submission-id");
    prompt_state
        .handle_permission_request_resolved(&session_client, request_key, response)
        .await?;

    let requests = client.permission_requests();
    let request = requests.last().unwrap();
    let option_ids = request
        .options
        .iter()
        .map(|option| option.option_id.0.to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        option_ids,
        vec![
            permission_option::APPROVED.to_string(),
            permission_option::DENIED.to_string()
        ]
    );

    let ops = thread.ops();
    assert!(matches!(
        ops.last(),
        Some(Op::ExecApproval {
            id,
            turn_id,
            decision: ReviewDecision::Denied,
        }) if id == "approval-id" && turn_id.as_deref() == Some("turn-id")
    ));

    Ok(())
}

#[tokio::test]
async fn test_patch_rejection_denies_without_cancelling_turn() -> anyhow::Result<()> {
    let session_id = SessionId::new("test");
    let client = Arc::new(StubClient::with_permission_responses(vec![
        RequestPermissionResponse::new(RequestPermissionOutcome::Selected(
            SelectedPermissionOutcome::new(permission_option::DENIED),
        )),
    ]));
    let session_client =
        SessionClient::with_client(session_id, client.clone(), Arc::default(), Arc::default());
    let thread = Arc::new(StubCodexThread::new());
    let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
    let (message_tx, mut message_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut prompt_state = PromptState::new(
        "submission-id".to_string(),
        thread.clone(),
        message_tx,
        response_tx,
    );

    let mut changes = HashMap::new();
    changes.insert(
        PathBuf::from("src/lib.rs"),
        FileChange::Update {
            unified_diff: "@@\n-old\n+new\n".to_string(),
            move_path: None,
        },
    );

    prompt_state.patch_approval(
        &session_client,
        test_fixtures::apply_patch_approval_request("patch-call", "turn-id", changes, None),
    );

    let ThreadMessage::PermissionRequestResolved {
        submission_id,
        request_key,
        response,
    } = message_rx.recv().await.unwrap()
    else {
        panic!("expected permission resolution message");
    };
    assert_eq!(submission_id, "submission-id");
    prompt_state
        .handle_permission_request_resolved(&session_client, request_key, response)
        .await?;

    let request = client
        .permission_requests()
        .pop()
        .expect("expected patch permission request");
    let option_ids = request
        .options
        .iter()
        .map(|option| option.option_id.0.to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        option_ids,
        vec![
            permission_option::APPROVED.to_string(),
            permission_option::DENIED.to_string()
        ]
    );

    let ops = thread.ops();
    assert!(matches!(
        ops.last(),
        Some(Op::PatchApproval {
            id,
            decision: ReviewDecision::Denied,
        }) if id == "patch-call"
    ));

    Ok(())
}

#[tokio::test]
async fn test_mcp_tool_approval_elicitation_routes_to_permission_request() -> anyhow::Result<()> {
    let session_id = SessionId::new("test");
    let client = Arc::new(StubClient::with_permission_responses(vec![
        RequestPermissionResponse::new(RequestPermissionOutcome::Selected(
            SelectedPermissionOutcome::new(MCP_TOOL_APPROVAL_ALLOW_SESSION_OPTION_ID),
        )),
    ]));
    let session_client =
        SessionClient::with_client(session_id, client.clone(), Arc::default(), Arc::default());
    let thread = Arc::new(StubCodexThread::new());
    let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
    let (message_tx, mut message_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut prompt_state = PromptState::new(
        "submission-id".to_string(),
        thread.clone(),
        message_tx,
        response_tx,
    );

    let request_id = format!("{MCP_TOOL_APPROVAL_REQUEST_ID_PREFIX}call-123");
    prompt_state
        .mcp_elicitation(
            &session_client,
            ElicitationRequestEvent {
                turn_id: Some("turn-id".to_string()),
                server_name: "test-server".to_string(),
                id: codex_protocol::mcp::RequestId::String(request_id.clone()),
                request: ElicitationRequest::Form {
                    meta: Some(serde_json::json!({
                        "codex_approval_kind": "mcp_tool_call",
                        "persist": ["session", "always"],
                        "connector_name": "Docs",
                        "tool_title": "search_docs",
                        "tool_description": "Search project documentation",
                        "tool_params_display": [
                            {
                                "display_name": "Query",
                                "name": "query",
                                "value": "approval flow"
                            }
                        ]
                    })),
                    message: "Allow Docs to run tool \"search_docs\"?".to_string(),
                    requested_schema: serde_json::json!({
                        "type": "object",
                        "properties": {}
                    }),
                },
            },
        )
        .await?;

    let ThreadMessage::PermissionRequestResolved {
        submission_id,
        request_key,
        response,
    } = message_rx.recv().await.unwrap()
    else {
        panic!("expected permission resolution message");
    };
    assert_eq!(submission_id, "submission-id");

    {
        let requests = client.permission_requests();
        let request = requests.last().unwrap();
        assert_eq!(request.tool_call.tool_call_id.0.as_ref(), "call-123");
        assert_eq!(
            request
                .options
                .iter()
                .map(|option| option.option_id.0.to_string())
                .collect::<Vec<_>>(),
            vec![
                MCP_TOOL_APPROVAL_ALLOW_OPTION_ID.to_string(),
                MCP_TOOL_APPROVAL_ALLOW_SESSION_OPTION_ID.to_string(),
                MCP_TOOL_APPROVAL_ALLOW_ALWAYS_OPTION_ID.to_string(),
                MCP_TOOL_APPROVAL_CANCEL_OPTION_ID.to_string(),
            ]
        );
    }

    prompt_state
        .handle_permission_request_resolved(&session_client, request_key, response)
        .await?;

    let op = thread.last_op().unwrap();
    match op {
        Op::ResolveElicitation {
            server_name,
            request_id: codex_protocol::mcp::RequestId::String(id),
            decision,
            content,
            meta,
        } => {
            assert_eq!(server_name, "test-server");
            assert_eq!(id, request_id);
            assert_eq!(decision, ElicitationAction::Accept);
            assert!(content.is_none());
            assert_eq!(
                meta.as_ref()
                    .and_then(|value| value.get("persist"))
                    .and_then(serde_json::Value::as_str),
                Some(MCP_TOOL_APPROVAL_PERSIST_SESSION)
            );
        }
        other => panic!("unexpected op: {other:?}"),
    }

    Ok(())
}

#[tokio::test]
async fn test_mcp_elicitation_declines_unsupported_form_requests() -> anyhow::Result<()> {
    let session_id = SessionId::new("test");
    let client = Arc::new(StubClient::with_permission_responses(vec![
        RequestPermissionResponse::new(RequestPermissionOutcome::Selected(
            SelectedPermissionOutcome::new("decline"),
        )),
    ]));
    let session_client =
        SessionClient::with_client(session_id, client.clone(), Arc::default(), Arc::default());
    let thread = Arc::new(StubCodexThread::new());
    let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
    let (message_tx, _message_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut prompt_state = PromptState::new(
        "submission-id".to_string(),
        thread.clone(),
        message_tx,
        response_tx,
    );

    prompt_state
        .mcp_elicitation(
            &session_client,
            ElicitationRequestEvent {
                turn_id: Some("turn-id".to_string()),
                server_name: "test-server".to_string(),
                id: codex_protocol::mcp::RequestId::String("request-id".to_string()),
                request: ElicitationRequest::Form {
                    meta: None,
                    message: "Need some structured input".to_string(),
                    requested_schema: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" }
                        }
                    }),
                },
            },
        )
        .await?;

    assert!(
        client.permission_requests().is_empty(),
        "unsupported MCP elicitations should be auto-declined"
    );

    let ops = thread.ops();
    assert!(matches!(
        ops.last(),
        Some(Op::ResolveElicitation {
            server_name,
            request_id: codex_protocol::mcp::RequestId::String(request_id),
            decision: ElicitationAction::Decline,
            content: None,
            meta: None,
        }) if server_name == "test-server" && request_id == "request-id"
    ));

    Ok(())
}

#[tokio::test]
async fn test_blocked_approval_does_not_block_followup_events() -> anyhow::Result<()> {
    let session_id = SessionId::new("test");
    let client = Arc::new(StubClient::with_blocked_permission_requests(
        vec![],
        Arc::new(Notify::new()),
    ));
    let session_client =
        SessionClient::with_client(session_id, client.clone(), Arc::default(), Arc::default());
    let thread = Arc::new(StubCodexThread::new());
    let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
    let (message_tx, _message_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut prompt_state =
        PromptState::new("submission-id".to_string(), thread, message_tx, response_tx);

    prompt_state
        .handle_event(
            &session_client,
            EventMsg::ExecApprovalRequest(test_fixtures::exec_approval_request(
                "call-id",
                "turn-id",
                std::env::current_dir()?.try_into()?,
                vec!["echo".to_string(), "hi".to_string()],
                vec![ParsedCommand::Unknown {
                    cmd: "echo hi".to_string(),
                }],
                Some(vec![ReviewDecision::Approved, ReviewDecision::Abort]),
            )),
        )
        .await;

    prompt_state
        .handle_event(
            &session_client,
            EventMsg::AgentMessage(AgentMessageEvent {
                message: "still flowing".to_string(),
                phase: None,
                memory_citation: None,
            }),
        )
        .await;

    assert!(client.has_agent_text(|text| text == "still flowing"));
    prompt_state.abort_pending_interactions();

    Ok(())
}

#[tokio::test]
async fn test_thread_shutdown_bypasses_blocked_permission_request() -> anyhow::Result<()> {
    let session_id = SessionId::new("test");
    let client = Arc::new(StubClient::with_blocked_permission_requests(
        vec![RequestPermissionResponse::new(
            RequestPermissionOutcome::Cancelled,
        )],
        Arc::new(Notify::new()),
    ));
    let session_client = SessionClient::with_client(
        session_id.clone(),
        client.clone(),
        Arc::default(),
        Arc::default(),
    );
    let conversation = Arc::new(StubCodexThread::new());
    let models_manager = Arc::new(StubModelsManager);
    let config =
        Config::load_with_cli_overrides_and_harness_overrides(vec![], ConfigOverrides::default())
            .await?;
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
    let thread = Thread {
        thread: conversation.clone(),
        message_tx,
        _handle: handle,
    };

    let stop_reason_rx = submit_prompt(&session_id, &thread.message_tx, "approval-block").await?;

    tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            if client.has_permission_requests() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await?;

    tokio::time::timeout(Duration::from_millis(100), thread.shutdown()).await??;
    let stop_reason = tokio::time::timeout(Duration::from_millis(100), stop_reason_rx).await??;
    assert_eq!(stop_reason?, StopReason::Cancelled);

    assert!(matches!(conversation.last_op(), Some(Op::Shutdown)));

    Ok(())
}

#[tokio::test]
async fn test_full_access_auto_approves_patch_permission_requests() -> anyhow::Result<()> {
    let (_, _, thread, mut actor) = setup_actor().await?;
    actor
        .handle_set_mode(SessionModeId::new("full-access"))
        .await?;

    let mut changes = HashMap::new();
    changes.insert(
        PathBuf::from("src/lib.rs"),
        FileChange::Update {
            unified_diff: "@@\n-old\n+new\n".to_string(),
            move_path: None,
        },
    );

    actor
        .handle_event(Event {
            id: "submission-id".to_string(),
            msg: EventMsg::ApplyPatchApprovalRequest(test_fixtures::apply_patch_approval_request(
                "patch-call",
                "turn-id",
                changes,
                None,
            )),
        })
        .await;

    let ops = thread.ops();
    assert!(matches!(
        ops.as_slice(),
        [
            Op::ThreadSettings {
                thread_settings: ThreadSettingsOverrides {
                    permission_profile: Some(PermissionProfile::Disabled),
                    ..
                },
            },
            Op::PatchApproval {
                id,
                decision: ReviewDecision::Approved,
            }
        ] if id == "patch-call"
    ));

    Ok(())
}

#[tokio::test]
async fn test_thread_request_fails_when_actor_channel_is_closed() -> anyhow::Result<()> {
    let thread = thread_with_closed_actor_channel();

    let result = tokio::time::timeout(Duration::from_millis(100), thread.load()).await?;
    let Err(err) = result else {
        panic!("closed actor channel should return an error");
    };
    assert!(format!("{err:?}").contains("thread actor is not running"));

    Ok(())
}

#[tokio::test]
async fn test_thread_prompt_fails_when_actor_channel_is_closed() -> anyhow::Result<()> {
    let thread = thread_with_closed_actor_channel();
    let request = PromptRequest::new(
        SessionId::new("session-id"),
        vec!["hello".to_string().into()],
    );

    let result = tokio::time::timeout(Duration::from_millis(100), thread.prompt(request)).await?;
    let Err(err) = result else {
        panic!("closed actor channel should return an error");
    };
    assert!(format!("{err:?}").contains("thread actor is not running"));

    Ok(())
}

fn thread_with_closed_actor_channel() -> Thread {
    let conversation = Arc::new(StubCodexThread::new());
    let (message_tx, message_rx) = tokio::sync::mpsc::unbounded_channel();
    drop(message_rx);

    Thread {
        thread: conversation,
        message_tx,
        _handle: tokio::spawn(async {}),
    }
}
