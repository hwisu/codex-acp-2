use super::fixtures::*;

#[tokio::test]
async fn test_replay_history_restores_pending_user_input() -> anyhow::Result<()> {
    let (session_id, client, thread, message_tx, _handle) = setup().await?;
    let (replay_response_tx, replay_response_rx) = tokio::sync::oneshot::channel();

    message_tx.send(ThreadMessage::ReplayHistory {
        history: vec![RolloutItem::EventMsg(EventMsg::RequestUserInput(
            RequestUserInputEvent {
                call_id: "replay-input".to_string(),
                turn_id: "replay-turn".to_string(),
                auto_resolution_ms: None,
                questions: vec![RequestUserInputQuestion {
                    id: "confirm_path".to_string(),
                    header: "Confirm".to_string(),
                    question: "Proceed with the replayed plan?".to_string(),
                    is_other: true,
                    is_secret: false,
                    options: Some(vec![
                        codex_protocol::request_user_input::RequestUserInputQuestionOption {
                            label: "Yes (Recommended)".to_string(),
                            description: "Continue the current plan.".to_string(),
                        },
                        codex_protocol::request_user_input::RequestUserInputQuestionOption {
                            label: "No".to_string(),
                            description: "Stop and revisit the approach.".to_string(),
                        },
                    ]),
                }],
            },
        ))],
        response_tx: replay_response_tx,
    })?;
    replay_response_rx.await??;

    tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            let has_prompt =
                client.has_agent_text(|text| text.contains("Additional input is required"));
            if has_prompt {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await?;

    let stop_reason = submit_prompt_and_wait(&session_id, &message_tx, "yes").await?;
    assert_eq!(stop_reason, StopReason::EndTurn);

    let ops = thread.ops();
    assert!(matches!(
        ops.as_slice(),
        [Op::UserInputAnswer { id, response }]
            if id == "replay-turn"
                && response.answers["confirm_path"].answers
                    == vec!["Yes (Recommended)".to_string()]
    ));

    Ok(())
}

#[tokio::test]
async fn test_replay_history_restores_plan_and_subagent_state() -> anyhow::Result<()> {
    let (session_id, client, _, message_tx, _handle) = setup().await?;
    let (replay_response_tx, replay_response_rx) = tokio::sync::oneshot::channel();
    let sender_thread_id = ThreadId::new();
    let receiver_thread_id = ThreadId::new();
    let saved_path = std::env::current_dir()?
        .join("replayed-generated.png")
        .try_into()?;

    message_tx.send(ThreadMessage::ReplayHistory {
        history: vec![
            RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
                model_context_window: None,
                collaboration_mode_kind: ModeKind::Plan,
                turn_id: "replay-turn".to_string(),
                trace_id: None,
                started_at: None,
            })),
            RolloutItem::EventMsg(EventMsg::PlanUpdate(UpdatePlanArgs {
                explanation: Some("Investigate parity gaps".to_string()),
                plan: vec![PlanItemArg {
                    step: "Audit replay parity".to_string(),
                    status: StepStatus::InProgress,
                }],
            })),
            RolloutItem::EventMsg(EventMsg::ImageGenerationEnd(ImageGenerationEndEvent {
                call_id: "replay-image".to_string(),
                status: "completed".to_string(),
                revised_prompt: Some("Render the replay state".to_string()),
                result: String::new(),
                saved_path: Some(saved_path),
            })),
            RolloutItem::EventMsg(EventMsg::CollabAgentSpawnEnd({
                let mut end = test_fixtures::collab_spawn_end(
                    "replay-spawn",
                    sender_thread_id,
                    Some(receiver_thread_id),
                    "Investigate replay parity",
                    "gpt-5.4",
                    ReasoningEffort::Medium,
                );
                end.new_agent_nickname = Some("Parity Worker".to_string());
                end.new_agent_role = Some("worker".to_string());
                end
            })),
        ],
        response_tx: replay_response_tx,
    })?;
    replay_response_rx.await??;

    let notifications = client.notifications();
    let plan_notification = notifications.iter().find(|notification| {
        matches!(
            &notification.update,
            SessionUpdate::Plan(plan)
                if plan.entries.iter().any(|entry| entry.content == "Audit replay parity")
        )
    });
    let plan_notification = plan_notification.expect("expected replayed plan notification");
    let untyped_plan_notification = plan_notification.to_untyped_message()?;
    assert_eq!(untyped_plan_notification.method(), "session/update");
    assert!(!untyped_plan_notification.method().starts_with("cursor/"));
    assert!(
        !untyped_plan_notification
            .params()
            .to_string()
            .contains("cursor/"),
        "ACP plan updates must not serialize through Cursor-specific methods"
    );

    let tool_calls = client.tool_calls();
    assert!(tool_calls.iter().any(|tool_call| {
        tool_call.tool_call_id.0.as_ref() == "replay-image"
            && tool_call.status == ToolCallStatus::Completed
    }));
    assert!(tool_calls.iter().any(|tool_call| {
        tool_call.tool_call_id.0.as_ref() == "replay-spawn"
            && tool_call.title.contains("Parity Worker")
    }));

    let stop_reason = submit_prompt_and_wait(&session_id, &message_tx, "/status").await?;
    assert_eq!(stop_reason, StopReason::EndTurn);

    assert!(client.has_agent_text(
        |text| text.contains("Collaboration mode: Plan") && text.contains("Known subagents: 1")
    ));

    Ok(())
}

#[tokio::test]
async fn test_replay_exec_command_function_call_preserves_shell_metadata() -> anyhow::Result<()> {
    let (_, client, _, message_tx, _handle) = setup().await?;
    let (replay_response_tx, replay_response_rx) = tokio::sync::oneshot::channel();

    message_tx.send(ThreadMessage::ReplayHistory {
        history: vec![RolloutItem::ResponseItem(ResponseItem::FunctionCall {
            id: None,
            name: "exec_command".to_string(),
            namespace: None,
            arguments: serde_json::json!({
                "cmd": "cat README.md",
            })
            .to_string(),
            call_id: "exec-replay".to_string(),
            internal_chat_message_metadata_passthrough: None,
        })],
        response_tx: replay_response_tx,
    })?;
    replay_response_rx.await??;

    let tool_call = client
        .tool_calls()
        .into_iter()
        .find(|tool_call| tool_call.tool_call_id.0.as_ref() == "exec-replay")
        .expect("expected replayed exec_command tool call");
    assert_ne!(tool_call.title, "exec_command");
    assert_eq!(tool_call.title, "Read README.md");
    assert_eq!(tool_call.kind, ToolKind::Read);
    assert_eq!(tool_call.status, ToolCallStatus::Completed);

    Ok(())
}

#[tokio::test]
async fn test_replay_invalid_exec_command_arguments_remain_generic() -> anyhow::Result<()> {
    let (_, client, _, message_tx, _handle) = setup().await?;
    let (replay_response_tx, replay_response_rx) = tokio::sync::oneshot::channel();

    message_tx.send(ThreadMessage::ReplayHistory {
        history: vec![RolloutItem::ResponseItem(ResponseItem::FunctionCall {
            id: None,
            name: "exec_command".to_string(),
            namespace: None,
            arguments: serde_json::json!({
                "cmd": "   ",
            })
            .to_string(),
            call_id: "invalid-exec-replay".to_string(),
            internal_chat_message_metadata_passthrough: None,
        })],
        response_tx: replay_response_tx,
    })?;
    replay_response_rx.await??;

    let tool_call = client
        .tool_calls()
        .into_iter()
        .find(|tool_call| tool_call.tool_call_id.0.as_ref() == "invalid-exec-replay")
        .expect("expected replayed exec_command tool call");
    assert_eq!(tool_call.title, "exec_command");
    assert_eq!(tool_call.kind, ToolKind::Other);
    assert_eq!(tool_call.status, ToolCallStatus::Completed);
    assert_eq!(
        tool_call.raw_input,
        Some(serde_json::json!({
            "cmd": "   ",
        }))
    );

    Ok(())
}
