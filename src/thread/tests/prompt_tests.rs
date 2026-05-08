use super::fixtures::*;

#[tokio::test]
async fn test_prompt() -> anyhow::Result<()> {
    let (session_id, client, _, message_tx, _handle) = setup().await?;
    let stop_reason = submit_prompt_and_wait(&session_id, &message_tx, "Hi").await?;
    assert_eq!(stop_reason, StopReason::EndTurn);
    drop(message_tx);

    assert_eq!(client.agent_texts(), vec!["Hi".to_string()]);

    Ok(())
}

#[tokio::test]
async fn test_thread_goal_updated_is_sent_as_agent_message() -> anyhow::Result<()> {
    let (session_id, client, _, message_tx, _handle) = setup().await?;
    let stop_reason =
        submit_prompt_and_wait(&session_id, &message_tx, "thread-goal-update").await?;
    assert_eq!(stop_reason, StopReason::EndTurn);
    drop(message_tx);

    assert!(client.has_agent_text(|text| text == "Goal updated (active): Ship the goal update"));

    Ok(())
}

#[tokio::test]
async fn test_reasoning_stream_is_sent_as_agent_thought_chunks() -> anyhow::Result<()> {
    let (session_id, client, _, message_tx, _handle) = setup().await?;
    let stop_reason = submit_prompt_and_wait(&session_id, &message_tx, "reasoning-stream").await?;
    assert_eq!(stop_reason, StopReason::EndTurn);
    drop(message_tx);

    assert_eq!(
        client.agent_thoughts(),
        vec![
            "Thinking ".to_string(),
            "\n\n".to_string(),
            "hard!".to_string()
        ]
    );
    assert!(client.agent_texts().is_empty());

    Ok(())
}

#[tokio::test]
async fn test_final_reasoning_is_sent_as_agent_thought_when_not_streamed() -> anyhow::Result<()> {
    let (session_id, client, _, message_tx, _handle) = setup().await?;
    let stop_reason = submit_prompt_and_wait(&session_id, &message_tx, "reasoning-final").await?;
    assert_eq!(stop_reason, StopReason::EndTurn);
    drop(message_tx);

    assert_eq!(
        client.agent_thoughts(),
        vec!["Final reasoning only".to_string()]
    );
    assert!(client.agent_texts().is_empty());

    Ok(())
}

#[tokio::test]
async fn test_goal_command_reports_missing_goal() -> anyhow::Result<()> {
    let (session_id, client, thread, message_tx, _handle) = setup_with_goals().await?;
    let stop_reason = submit_prompt_and_wait(&session_id, &message_tx, "/goal").await?;
    assert_eq!(stop_reason, StopReason::EndTurn);
    drop(message_tx);

    assert!(
        client.has_agent_text(|text| text == "Usage: /goal <objective>\nNo goal is currently set.")
    );
    assert!(thread.ops().is_empty());

    Ok(())
}

#[tokio::test]
async fn test_goal_command_sets_objective() -> anyhow::Result<()> {
    let (session_id, client, thread, message_tx, _handle) = setup_with_goals().await?;
    let stop_reason =
        submit_prompt_and_wait(&session_id, &message_tx, "/goal improve benchmark coverage")
            .await?;
    assert_eq!(stop_reason, StopReason::EndTurn);
    drop(message_tx);

    assert!(
        client.has_agent_text(|text| {
            text == "Goal active\nObjective: improve benchmark coverage"
        })
    );
    let goal = thread.thread_goal().expect("goal should be set");
    assert_eq!(goal.objective, "improve benchmark coverage");
    assert_eq!(goal.status, ThreadGoalStatus::Active);
    assert!(thread.ops().is_empty());

    Ok(())
}

#[tokio::test]
async fn test_goal_command_reports_current_goal() -> anyhow::Result<()> {
    let (session_id, client, thread, message_tx, _handle) = setup_with_goals().await?;
    thread.seed_thread_goal(test_thread_goal(ThreadGoalStatus::Paused));

    let stop_reason = submit_prompt_and_wait(&session_id, &message_tx, "/goal").await?;
    assert_eq!(stop_reason, StopReason::EndTurn);
    drop(message_tx);

    assert!(client.has_agent_text(|text| {
        text.contains("Goal")
            && text.contains("Status: paused")
            && text.contains("Objective: Ship the goal update")
            && text.contains("Time used: 2m")
            && text.contains("Tokens used: 1.2k")
            && text.contains("Token budget: 5.0k")
            && text.contains("Commands: /goal resume, /goal clear")
    }));
    assert!(thread.ops().is_empty());

    Ok(())
}

#[tokio::test]
async fn test_goal_command_updates_status_and_clears_goal() -> anyhow::Result<()> {
    let (session_id, client, thread, message_tx, _handle) = setup_with_goals().await?;
    thread.seed_thread_goal(test_thread_goal(ThreadGoalStatus::Active));

    let stop_reason = submit_prompt_and_wait(&session_id, &message_tx, "/goal pause").await?;
    assert_eq!(stop_reason, StopReason::EndTurn);
    assert_eq!(
        thread.thread_goal().expect("goal should remain").status,
        ThreadGoalStatus::Paused
    );

    let stop_reason = submit_prompt_and_wait(&session_id, &message_tx, "/goal resume").await?;
    assert_eq!(stop_reason, StopReason::EndTurn);
    assert_eq!(
        thread.thread_goal().expect("goal should remain").status,
        ThreadGoalStatus::Active
    );

    let stop_reason = submit_prompt_and_wait(&session_id, &message_tx, "/goal clear").await?;
    assert_eq!(stop_reason, StopReason::EndTurn);
    drop(message_tx);

    assert!(thread.thread_goal().is_none());
    assert!(client.has_agent_text(|text| text.starts_with("Goal paused\nObjective: Ship the goal update")));
    assert!(client.has_agent_text(|text| text.starts_with("Goal active\nObjective: Ship the goal update")));
    assert!(client.has_agent_text(|text| text == "Goal cleared"));
    assert!(thread.ops().is_empty());

    Ok(())
}

#[tokio::test]
async fn test_fast_command_toggles_service_tier() -> anyhow::Result<()> {
    let (session_id, client, thread, message_tx, _handle) = setup_with_fast_mode().await?;

    let stop_reason = submit_prompt_and_wait(&session_id, &message_tx, "/fast on").await?;
    assert_eq!(stop_reason, StopReason::EndTurn);

    let stop_reason = submit_prompt_and_wait(&session_id, &message_tx, "/fast status").await?;
    assert_eq!(stop_reason, StopReason::EndTurn);

    let stop_reason = submit_prompt_and_wait(&session_id, &message_tx, "/fast off").await?;
    assert_eq!(stop_reason, StopReason::EndTurn);
    drop(message_tx);

    assert!(client.has_agent_text(|text| text == "Fast mode is on."));
    assert!(client.has_agent_text(|text| text == "Fast mode is off."));

    let ops = thread.ops();
    assert!(matches!(
        ops.as_slice(),
        [
            Op::OverrideTurnContext {
                service_tier: Some(Some(service_tier)),
                ..
            },
            Op::OverrideTurnContext {
                service_tier: Some(None),
                ..
            },
        ] if service_tier == "priority"
    ));

    Ok(())
}

#[tokio::test]
async fn test_review_command_uses_configured_review_branch() -> anyhow::Result<()> {
    let (session_id, _client, thread, message_tx, _handle) = setup().await?;

    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    message_tx.send(ThreadMessage::SetConfigOption {
        config_id: SessionConfigId::new("review_target"),
        value: SessionConfigOptionValue::ValueId {
            value: SessionConfigValueId::new("branch:main"),
        },
        response_tx,
    })?;
    response_rx.await??;

    let stop_reason = submit_prompt_and_wait(&session_id, &message_tx, "/review").await?;
    assert_eq!(stop_reason, StopReason::EndTurn);
    drop(message_tx);

    let ops = thread.ops();
    assert!(matches!(
        ops.as_slice(),
        [Op::Review {
            review_request: ReviewRequest {
                target: ReviewTarget::BaseBranch { branch },
                ..
            }
        }] if branch == "main"
    ));

    Ok(())
}

fn test_thread_goal(status: ThreadGoalStatus) -> ThreadGoal {
    ThreadGoal {
        thread_id: ThreadId::default(),
        objective: "Ship the goal update".to_string(),
        status,
        token_budget: Some(5_000),
        tokens_used: 1_200,
        time_used_seconds: 120,
        created_at: 1,
        updated_at: 2,
    }
}

#[tokio::test]
async fn test_compact() -> anyhow::Result<()> {
    let (session_id, client, thread, message_tx, _handle) = setup().await?;
    let stop_reason = submit_prompt_and_wait(&session_id, &message_tx, "/compact").await?;
    assert_eq!(stop_reason, StopReason::EndTurn);
    drop(message_tx);

    assert_eq!(
        client.agent_texts(),
        vec!["Compact task completed".to_string()]
    );
    let ops = thread.ops();
    assert_eq!(ops.as_slice(), &[Op::Compact]);

    Ok(())
}

#[tokio::test]
async fn test_undo() -> anyhow::Result<()> {
    let (session_id, client, thread, message_tx, _handle) = setup().await?;
    let stop_reason = submit_prompt_and_wait(&session_id, &message_tx, "/undo").await?;
    assert_eq!(stop_reason, StopReason::EndTurn);
    drop(message_tx);

    assert_eq!(client.agent_texts(), vec!["Undo completed.".to_string()]);

    let ops = thread.ops();
    assert_eq!(ops.as_slice(), &[Op::ThreadRollback { num_turns: 1 }]);

    Ok(())
}

#[tokio::test]
async fn test_init() -> anyhow::Result<()> {
    let (session_id, client, thread, message_tx, _handle) = setup().await?;
    let stop_reason = submit_prompt_and_wait(&session_id, &message_tx, "/init").await?;
    assert_eq!(stop_reason, StopReason::EndTurn);
    drop(message_tx);

    assert_eq!(client.agent_texts(), vec![INIT_COMMAND_PROMPT.to_string()]);
    let ops = thread.ops();
    assert_eq!(
        ops.as_slice(),
        &[Op::UserInput {
            items: vec![UserInput::Text {
                text: INIT_COMMAND_PROMPT.to_string(),
                text_elements: vec![]
            }],
            final_output_json_schema: None,
            environments: None,
            responsesapi_client_metadata: None,
        }],
        "ops don't match {ops:?}"
    );

    Ok(())
}

#[tokio::test]
async fn test_review() -> anyhow::Result<()> {
    let (session_id, client, thread, message_tx, _handle) = setup().await?;
    let stop_reason = submit_prompt_and_wait(&session_id, &message_tx, "/review").await?;
    assert_eq!(stop_reason, StopReason::EndTurn);
    drop(message_tx);

    assert_eq!(client.agent_texts(), vec!["current changes".to_string()]);

    let ops = thread.ops();
    assert_eq!(
        ops.as_slice(),
        &[Op::Review {
            review_request: ReviewRequest {
                user_facing_hint: Some(user_facing_hint(&ReviewTarget::UncommittedChanges)),
                target: ReviewTarget::UncommittedChanges,
            }
        }],
        "ops don't match {ops:?}"
    );

    Ok(())
}

#[tokio::test]
async fn test_custom_review() -> anyhow::Result<()> {
    let (session_id, client, thread, message_tx, _handle) = setup().await?;
    let instructions = "Review what we did in agents.md";

    let stop_reason =
        submit_prompt_and_wait(&session_id, &message_tx, format!("/review {instructions}")).await?;
    assert_eq!(stop_reason, StopReason::EndTurn);
    drop(message_tx);

    assert_eq!(
        client.agent_texts(),
        vec!["Review what we did in agents.md".to_string()]
    );

    let ops = thread.ops();
    assert_eq!(
        ops.as_slice(),
        &[Op::Review {
            review_request: ReviewRequest {
                user_facing_hint: Some(user_facing_hint(&ReviewTarget::Custom {
                    instructions: instructions.to_owned()
                })),
                target: ReviewTarget::Custom {
                    instructions: instructions.to_owned()
                },
            }
        }],
        "ops don't match {ops:?}"
    );

    Ok(())
}

#[tokio::test]
async fn test_commit_review() -> anyhow::Result<()> {
    let (session_id, client, thread, message_tx, _handle) = setup().await?;
    let stop_reason =
        submit_prompt_and_wait(&session_id, &message_tx, "/review-commit 123456").await?;
    assert_eq!(stop_reason, StopReason::EndTurn);
    drop(message_tx);

    assert_eq!(client.agent_texts(), vec!["commit 123456".to_string()]);

    let ops = thread.ops();
    assert_eq!(
        ops.as_slice(),
        &[Op::Review {
            review_request: ReviewRequest {
                user_facing_hint: Some(user_facing_hint(&ReviewTarget::Commit {
                    sha: "123456".to_owned(),
                    title: None
                })),
                target: ReviewTarget::Commit {
                    sha: "123456".to_owned(),
                    title: None
                },
            }
        }],
        "ops don't match {ops:?}"
    );

    Ok(())
}

#[tokio::test]
async fn test_branch_review() -> anyhow::Result<()> {
    let (session_id, client, thread, message_tx, _handle) = setup().await?;
    let stop_reason =
        submit_prompt_and_wait(&session_id, &message_tx, "/review-branch feature").await?;
    assert_eq!(stop_reason, StopReason::EndTurn);
    drop(message_tx);

    assert_eq!(
        client.agent_texts(),
        vec!["changes against 'feature'".to_string()]
    );

    let ops = thread.ops();
    assert_eq!(
        ops.as_slice(),
        &[Op::Review {
            review_request: ReviewRequest {
                user_facing_hint: Some(user_facing_hint(&ReviewTarget::BaseBranch {
                    branch: "feature".to_owned()
                })),
                target: ReviewTarget::BaseBranch {
                    branch: "feature".to_owned()
                },
            }
        }],
        "ops don't match {ops:?}"
    );

    Ok(())
}

#[tokio::test]
async fn test_plan_command_with_inline_prompt_sets_plan_mode() -> anyhow::Result<()> {
    let (session_id, client, thread, message_tx, _handle) = setup().await?;
    let prompt = "Investigate the adapter parity gap";

    let stop_reason =
        submit_prompt_and_wait(&session_id, &message_tx, format!("/plan {prompt}")).await?;
    assert_eq!(stop_reason, StopReason::EndTurn);
    drop(message_tx);

    assert!(client.has_agent_text(|text| text == prompt));
    assert!(client.notifications().iter().any(|notification| matches!(
        &notification.update,
        SessionUpdate::ConfigOptionUpdate(update)
            if update.config_options.iter().any(|option| {
                option.id.0.as_ref() == "mode"
                    && matches!(
                        &option.kind,
                        SessionConfigKind::Select(select)
                            if select.current_value.0.as_ref() == "plan"
                    )
            })
    )));

    let ops = thread.ops();
    assert!(matches!(
        ops.as_slice(),
        [
            Op::OverrideTurnContext {
                collaboration_mode:
                    Some(CollaborationMode {
                        mode: ModeKind::Plan,
                        ..
                    }),
                ..
            },
            Op::UserInput { items, .. }
        ] if matches!(
            items.as_slice(),
            [UserInput::Text { text, .. }] if text == prompt
        )
    ));

    Ok(())
}

#[tokio::test]
async fn test_request_user_input_single_question_is_bridged() -> anyhow::Result<()> {
    let (session_id, client, thread, message_tx, _handle) = setup().await?;
    let initial_stop_reason_rx = submit_prompt(&session_id, &message_tx, "needs-input").await?;

    tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            if client.has_agent_text(|text| {
                text.contains("Additional input is required")
                    && text.contains("renders structured questions as plain text")
            }) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await?;

    let followup_stop_reason_rx = submit_prompt(&session_id, &message_tx, "yes").await?;

    let initial_stop_reason =
        tokio::time::timeout(Duration::from_millis(100), initial_stop_reason_rx).await??;
    let followup_stop_reason =
        tokio::time::timeout(Duration::from_millis(100), followup_stop_reason_rx).await??;
    assert_eq!(initial_stop_reason?, StopReason::EndTurn);
    assert_eq!(followup_stop_reason?, StopReason::EndTurn);

    let ops = thread.ops();
    assert!(matches!(
        ops.as_slice(),
        [Op::UserInput { .. }, Op::UserInputAnswer { id, response }]
            if !id.is_empty()
                && response.answers["confirm_path"].answers
                    == vec!["Yes (Recommended)".to_string()]
    ));

    Ok(())
}

#[tokio::test]
async fn test_request_user_input_multi_question_is_bridged() -> anyhow::Result<()> {
    let (session_id, client, thread, message_tx, _handle) = setup().await?;
    let initial_stop_reason_rx =
        submit_prompt(&session_id, &message_tx, "needs-multi-input").await?;

    tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            if client.has_agent_text(|text| {
                text.contains("one line per question")
                    && text.contains("renders structured questions as plain text")
            }) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await?;

    let reply = "choice: 3: Custom option\ndetails: this needs more context";
    let followup_stop_reason_rx = submit_prompt(&session_id, &message_tx, reply).await?;

    let initial_stop_reason =
        tokio::time::timeout(Duration::from_millis(100), initial_stop_reason_rx).await??;
    let followup_stop_reason =
        tokio::time::timeout(Duration::from_millis(100), followup_stop_reason_rx).await??;
    assert_eq!(initial_stop_reason?, StopReason::EndTurn);
    assert_eq!(followup_stop_reason?, StopReason::EndTurn);

    let ops = thread.ops();
    assert!(matches!(
        ops.as_slice(),
        [
            Op::UserInput { .. },
            Op::UserInputAnswer { response, .. }
        ] if response.answers["choice"].answers
            == vec![
                REQUEST_USER_INPUT_OTHER_OPTION_LABEL.to_string(),
                "user_note: 3: Custom option".to_string()
            ]
            && response.answers["details"].answers
                == vec!["user_note: this needs more context".to_string()]
    ));

    Ok(())
}

#[tokio::test]
async fn test_image_generation_events_are_surfaced() -> anyhow::Result<()> {
    let (session_id, client, _, message_tx, _handle) = setup().await?;
    let stop_reason = submit_prompt_and_wait(&session_id, &message_tx, "image-gen").await?;
    assert_eq!(stop_reason, StopReason::EndTurn);
    drop(message_tx);

    assert!(client.tool_calls().iter().any(|tool_call| {
        tool_call.tool_call_id.0.as_ref() == "image-call" && tool_call.title == "Generate image"
    }));
    assert!(client.tool_call_updates().iter().any(|update| {
        update.tool_call_id.0.as_ref() == "image-call"
            && update.fields.status == Some(ToolCallStatus::Completed)
    }));

    Ok(())
}

#[tokio::test]
async fn test_collab_events_are_surfaced() -> anyhow::Result<()> {
    let (session_id, client, _, message_tx, _handle) = setup().await?;
    let stop_reason = submit_prompt_and_wait(&session_id, &message_tx, "collab").await?;
    assert_eq!(stop_reason, StopReason::EndTurn);
    drop(message_tx);

    assert!(client.tool_calls().iter().any(|tool_call| {
        tool_call.tool_call_id.0.as_ref() == "spawn-1" && tool_call.title == "Spawn subagent"
    }));
    assert!(client.tool_call_updates().iter().any(|update| {
        update.tool_call_id.0.as_ref() == "spawn-1"
            && update.fields.status == Some(ToolCallStatus::Completed)
    }));

    Ok(())
}

#[tokio::test]
async fn test_agent_command_reports_known_subagents() -> anyhow::Result<()> {
    let (session_id, client, _, message_tx, _handle) = setup().await?;

    let stop_reason = submit_prompt_and_wait(&session_id, &message_tx, "collab").await?;
    assert_eq!(stop_reason, StopReason::EndTurn);

    let stop_reason = submit_prompt_and_wait(&session_id, &message_tx, "/agent").await?;
    assert_eq!(stop_reason, StopReason::EndTurn);
    drop(message_tx);

    assert!(
        client.has_agent_text(
            |text| text.contains("Known subagents") && text.contains("Parity Worker")
        )
    );

    Ok(())
}

#[tokio::test]
async fn test_warning_messages_are_tagged_with_metadata() -> anyhow::Result<()> {
    let session_id = SessionId::new("test");
    let client = Arc::new(StubClient::new());
    let session_client =
        SessionClient::with_client(session_id, client.clone(), Arc::default(), Arc::default());
    let thread = Arc::new(StubCodexThread::new());
    let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
    let (message_tx, _message_rx) = tokio::sync::mpsc::unbounded_channel();
    let _prompt_state =
        PromptState::new("submission-id".to_string(), thread, message_tx, response_tx);

    PromptState::warning(
        &session_client,
        WarningEvent {
            message: "Model metadata missing".to_string(),
        },
    );

    let warning_chunk = client
        .notifications()
        .into_iter()
        .find_map(|notification| match notification.update {
            SessionUpdate::AgentMessageChunk(chunk) => Some(chunk),
            _ => None,
        })
        .expect("expected warning agent message chunk");

    assert!(matches!(
        warning_chunk.content,
        ContentBlock::Text(TextContent { text, .. }) if text == "Model metadata missing"
    ));
    assert_eq!(
        warning_chunk
            .meta
            .as_ref()
            .and_then(|meta| meta.get("codex_acp"))
            .and_then(|value| value.get("kind"))
            .and_then(serde_json::Value::as_str),
        Some("warning")
    );

    Ok(())
}

#[tokio::test]
async fn test_final_agent_message_is_not_dropped_after_commentary_delta() -> anyhow::Result<()> {
    let session_id = SessionId::new("test");
    let client = Arc::new(StubClient::new());
    let session_client =
        SessionClient::with_client(session_id, client.clone(), Arc::default(), Arc::default());
    let thread = Arc::new(StubCodexThread::new());
    let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
    let (message_tx, _message_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut prompt_state =
        PromptState::new("submission-id".to_string(), thread, message_tx, response_tx);

    prompt_state.agent_message_content_delta(
        &session_client,
        AgentMessageContentDeltaEvent {
            thread_id: "thread-id".to_string(),
            turn_id: "turn-id".to_string(),
            item_id: "commentary-item".to_string(),
            delta: "Checking context.\n".to_string(),
        },
    );
    prompt_state.agent_message(
        &session_client,
        AgentMessageEvent {
            message: "Final answer.".to_string(),
            phase: Some(MessagePhase::FinalAnswer),
            memory_citation: None,
        },
    );

    assert_eq!(
        client.agent_texts(),
        vec![
            "Checking context.\n".to_string(),
            "Final answer.".to_string()
        ]
    );

    Ok(())
}

#[tokio::test]
async fn test_final_agent_message_is_deduped_when_already_streamed() -> anyhow::Result<()> {
    let session_id = SessionId::new("test");
    let client = Arc::new(StubClient::new());
    let session_client =
        SessionClient::with_client(session_id, client.clone(), Arc::default(), Arc::default());
    let thread = Arc::new(StubCodexThread::new());
    let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
    let (message_tx, _message_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut prompt_state =
        PromptState::new("submission-id".to_string(), thread, message_tx, response_tx);

    prompt_state.agent_message_content_delta(
        &session_client,
        AgentMessageContentDeltaEvent {
            thread_id: "thread-id".to_string(),
            turn_id: "turn-id".to_string(),
            item_id: "final-item".to_string(),
            delta: "Final answer.".to_string(),
        },
    );
    prompt_state.agent_message(
        &session_client,
        AgentMessageEvent {
            message: "Final answer.".to_string(),
            phase: Some(MessagePhase::FinalAnswer),
            memory_citation: None,
        },
    );

    assert_eq!(client.agent_texts(), vec!["Final answer.".to_string()]);

    Ok(())
}

#[tokio::test]
async fn test_usage_update_includes_token_usage_metadata() -> anyhow::Result<()> {
    let session_id = SessionId::new("test");
    let client = Arc::new(StubClient::new());
    let session_client =
        SessionClient::with_client(session_id, client.clone(), Arc::default(), Arc::default());

    PromptState::token_count(
        &session_client,
        TokenCountEvent {
            info: Some(TokenUsageInfo {
                total_token_usage: codex_protocol::protocol::TokenUsage {
                    input_tokens: 1_500,
                    cached_input_tokens: 250,
                    output_tokens: 350,
                    reasoning_output_tokens: 125,
                    total_tokens: 2_000,
                },
                last_token_usage: codex_protocol::protocol::TokenUsage {
                    input_tokens: 100,
                    cached_input_tokens: 10,
                    output_tokens: 20,
                    reasoning_output_tokens: 5,
                    total_tokens: 120,
                },
                model_context_window: Some(128_000),
            }),
            rate_limits: None,
        },
    );

    let usage_update = client
        .notifications()
        .into_iter()
        .find_map(|notification| match notification.update {
            SessionUpdate::UsageUpdate(update) => Some(update),
            _ => None,
        })
        .expect("expected usage update");

    assert_eq!(usage_update.used, 120);
    assert_eq!(usage_update.size, 128_000);
    assert_eq!(
        usage_update
            .meta
            .as_ref()
            .and_then(|meta| meta.get("codex_token_usage"))
            .and_then(|value| value.get("total"))
            .and_then(|value| value.get("input_tokens"))
            .and_then(serde_json::Value::as_i64),
        Some(1_500)
    );
    assert_eq!(
        usage_update
            .meta
            .as_ref()
            .and_then(|meta| meta.get("codex_token_usage"))
            .and_then(|value| value.get("last"))
            .and_then(|value| value.get("output_tokens"))
            .and_then(serde_json::Value::as_i64),
        Some(20)
    );

    Ok(())
}

#[tokio::test]
async fn test_status_command_reports_session_state_without_usage_details() -> anyhow::Result<()> {
    let (session_id, client, _, mut actor) = setup_actor().await?;

    actor
        .handle_event(Event {
            id: "usage-state".to_string(),
            msg: EventMsg::TokenCount(TokenCountEvent {
                info: Some(TokenUsageInfo {
                    total_token_usage: codex_protocol::protocol::TokenUsage {
                        input_tokens: 1_500,
                        cached_input_tokens: 250,
                        output_tokens: 350,
                        reasoning_output_tokens: 125,
                        total_tokens: 48_000,
                    },
                    last_token_usage: codex_protocol::protocol::TokenUsage {
                        total_tokens: 48_000,
                        ..Default::default()
                    },
                    model_context_window: Some(128_000),
                }),
                rate_limits: Some(RateLimitSnapshot {
                    limit_id: None,
                    limit_name: None,
                    primary: Some(codex_protocol::protocol::RateLimitWindow {
                        used_percent: 92.0,
                        window_minutes: None,
                        resets_at: None,
                    }),
                    secondary: None,
                    credits: None,
                    plan_type: None,
                    rate_limit_reached_type: None,
                }),
            }),
        })
        .await;

    let stop_reason = actor
        .handle_prompt(PromptRequest::new(session_id, vec!["/status".into()]))
        .await?
        .await??;
    assert_eq!(stop_reason, StopReason::EndTurn);

    assert!(client.has_agent_text(|text| {
        text.contains("Model:")
            && text.contains("Configured MCP servers:")
            && text.contains("Active tool calls: 0")
            && text.contains("Run /usage for token, context window, and rate-limit details.")
            && !text.contains("Token usage:")
            && !text.contains("Context window:")
            && !text.contains("Primary limit:")
    }));

    Ok(())
}

#[tokio::test]
async fn test_usage_command_reports_usage_summary() -> anyhow::Result<()> {
    let (session_id, client, _, mut actor) = setup_actor().await?;

    actor
        .handle_event(Event {
            id: "usage-state".to_string(),
            msg: EventMsg::TokenCount(TokenCountEvent {
                info: Some(TokenUsageInfo {
                    total_token_usage: codex_protocol::protocol::TokenUsage {
                        input_tokens: 800,
                        cached_input_tokens: 0,
                        output_tokens: 200,
                        reasoning_output_tokens: 0,
                        total_tokens: 24_000,
                    },
                    last_token_usage: codex_protocol::protocol::TokenUsage {
                        total_tokens: 24_000,
                        ..Default::default()
                    },
                    model_context_window: Some(64_000),
                }),
                rate_limits: None,
            }),
        })
        .await;

    let stop_reason = actor
        .handle_prompt(PromptRequest::new(session_id, vec!["/usage".into()]))
        .await?
        .await??;
    assert_eq!(stop_reason, StopReason::EndTurn);

    assert!(client.has_agent_text(|text| {
        text.contains("Token usage:")
            && text.contains("Total: 1.0k")
            && text.contains("Input: 800")
            && text.contains("Output: 200")
            && text.contains("Context window:")
            && text.contains("Remaining: 77% left")
            && text.contains("Used: 24.0k / 64.0k")
    }));

    Ok(())
}

#[tokio::test]
async fn test_ps_command_reports_active_background_tool_calls() -> anyhow::Result<()> {
    let (session_id, client, conversation, mut actor) = setup_actor().await?;
    let (running_tx, _running_rx) = tokio::sync::oneshot::channel();
    let mut prompt_state = PromptState::new(
        "running-submission".to_string(),
        conversation,
        actor.resolution_tx(),
        running_tx,
    );
    prompt_state.insert_active_command(
        "call-1".to_string(),
        ActiveCommand {
            tool_call_id: ToolCallId::new("call-1"),
            title: "List /tmp".to_string(),
            kind: ToolKind::Search,
            terminal_output: true,
            output: String::new(),
            file_extension: None,
        },
    );
    actor.insert_submission(
        "running-submission".to_string(),
        SubmissionState::Prompt(prompt_state),
    );

    let stop_reason = actor
        .handle_prompt(PromptRequest::new(session_id, vec!["/ps".into()]))
        .await?
        .await??;
    assert_eq!(stop_reason, StopReason::EndTurn);

    assert!(
        client.has_agent_text(|text| text.contains("Active tool calls")
            && text.contains("List /tmp"))
    );

    Ok(())
}

#[tokio::test]
async fn test_delta_deduplication() -> anyhow::Result<()> {
    let (session_id, client, _, message_tx, _handle) = setup().await?;
    let stop_reason = submit_prompt_and_wait(&session_id, &message_tx, "test delta").await?;
    assert_eq!(stop_reason, StopReason::EndTurn);
    drop(message_tx);

    // We should only get ONE notification, not duplicates from both delta and non-delta
    let texts = client.agent_texts();
    assert_eq!(
        texts,
        vec!["test delta".to_string()],
        "Should only receive delta event, not duplicate non-delta."
    );

    Ok(())
}
