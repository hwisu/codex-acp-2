use super::*;

fn fixture_cwd() -> PathBuf {
    std::env::current_dir().expect("test fixture should run with a current directory")
}

fn fixture_absolute_path<T>(path: PathBuf) -> T
where
    T: TryFrom<PathBuf>,
    T::Error: std::fmt::Display,
{
    path.try_into()
        .unwrap_or_else(|err| panic!("test fixture path should be absolute: {err}"))
}

pub(in crate::thread::tests) struct StubCodexThread {
    current_id: AtomicUsize,
    active_prompt_id: std::sync::Mutex<Option<String>>,
    ops: std::sync::Mutex<Vec<Op>>,
    thread_goal: std::sync::Mutex<Option<ThreadGoal>>,
    op_tx: mpsc::UnboundedSender<Event>,
    op_rx: Mutex<mpsc::UnboundedReceiver<Event>>,
}

impl StubCodexThread {
    pub(in crate::thread::tests) fn new() -> Self {
        let (op_tx, op_rx) = mpsc::unbounded_channel();
        StubCodexThread {
            current_id: AtomicUsize::new(0),
            active_prompt_id: std::sync::Mutex::default(),
            ops: std::sync::Mutex::default(),
            thread_goal: std::sync::Mutex::default(),
            op_tx,
            op_rx: Mutex::new(op_rx),
        }
    }

    fn send_event(&self, event_id: impl std::fmt::Display, msg: EventMsg) {
        self.op_tx
            .send(Event {
                id: event_id.to_string(),
                msg,
            })
            .expect("stub event receiver should be alive while tests submit events");
    }

    fn send_turn_complete(&self, event_id: impl std::fmt::Display, turn_id: String) {
        self.send_event(
            event_id,
            EventMsg::TurnComplete(TurnCompleteEvent {
                last_agent_message: None,
                turn_id,
                completed_at: None,
                duration_ms: None,
                time_to_first_token_ms: None,
            }),
        );
    }

    pub(in crate::thread::tests) fn ops(&self) -> Vec<Op> {
        self.lock_ops().clone()
    }

    pub(in crate::thread::tests) fn last_op(&self) -> Option<Op> {
        self.lock_ops().last().cloned()
    }

    pub(in crate::thread::tests) fn thread_goal(&self) -> Option<ThreadGoal> {
        self.lock_thread_goal().clone()
    }

    pub(in crate::thread::tests) fn seed_thread_goal(&self, goal: ThreadGoal) {
        *self.lock_thread_goal() = Some(goal);
    }

    fn submit_user_input(&self, id: usize, items: Vec<UserInput>) {
        *self.lock_active_prompt_id() = Some(id.to_string());
        let prompt = items
            .into_iter()
            .map(|item| match item {
                UserInput::Text { text, .. } => text,
                _ => unimplemented!(),
            })
            .join("\n");

        match prompt.as_str() {
            "parallel-exec" => self.emit_parallel_exec(id),
            "thread-goal-update" => self.emit_thread_goal_update(id),
            "approval-block" => self.emit_approval_block(id),
            "needs-input" => self.emit_request_user_input(id),
            "needs-multi-input" => self.emit_multi_request_user_input(id),
            "image-gen" => self.emit_image_generation(id),
            "collab" => self.emit_collab_spawn(id),
            "reasoning-stream" => self.emit_reasoning_stream(id),
            "reasoning-final" => self.emit_reasoning_final(id),
            _ => self.emit_echo_prompt(id, prompt),
        }
    }

    fn emit_parallel_exec(&self, id: usize) {
        // Emit interleaved exec events: Begin A, Begin B, End A, End B
        let turn_id = id.to_string();
        let cwd = fixture_cwd();
        self.send_event(
            id,
            EventMsg::ExecCommandBegin(ExecCommandBeginEvent {
                call_id: "call-a".into(),
                process_id: None,
                turn_id: turn_id.clone(),
                started_at_ms: 0,
                command: vec!["echo".into(), "a".into()],
                cwd: fixture_absolute_path(cwd.clone()),
                parsed_cmd: vec![ParsedCommand::Unknown {
                    cmd: "echo a".into(),
                }],
                source: ExecCommandSource::default(),
                interaction_input: None,
            }),
        );
        self.send_event(
            id,
            EventMsg::ExecCommandBegin(ExecCommandBeginEvent {
                call_id: "call-b".into(),
                process_id: None,
                turn_id: turn_id.clone(),
                started_at_ms: 0,
                command: vec!["echo".into(), "b".into()],
                cwd: fixture_absolute_path(cwd.clone()),
                parsed_cmd: vec![ParsedCommand::Unknown {
                    cmd: "echo b".into(),
                }],
                source: ExecCommandSource::default(),
                interaction_input: None,
            }),
        );
        self.send_event(
            id,
            EventMsg::ExecCommandEnd(ExecCommandEndEvent {
                call_id: "call-a".into(),
                process_id: None,
                turn_id: turn_id.clone(),
                completed_at_ms: 0,
                command: vec!["echo".into(), "a".into()],
                cwd: fixture_absolute_path(cwd.clone()),
                parsed_cmd: vec![],
                source: ExecCommandSource::default(),
                interaction_input: None,
                stdout: "a\n".into(),
                stderr: String::new(),
                aggregated_output: "a\n".into(),
                exit_code: 0,
                duration: std::time::Duration::from_millis(10),
                formatted_output: "a\n".into(),
                status: ExecCommandStatus::Completed,
            }),
        );
        self.send_event(
            id,
            EventMsg::ExecCommandEnd(ExecCommandEndEvent {
                call_id: "call-b".into(),
                process_id: None,
                turn_id: turn_id.clone(),
                completed_at_ms: 0,
                command: vec!["echo".into(), "b".into()],
                cwd: fixture_absolute_path(cwd.clone()),
                parsed_cmd: vec![],
                source: ExecCommandSource::default(),
                interaction_input: None,
                stdout: "b\n".into(),
                stderr: String::new(),
                aggregated_output: "b\n".into(),
                exit_code: 0,
                duration: std::time::Duration::from_millis(10),
                formatted_output: "b\n".into(),
                status: ExecCommandStatus::Completed,
            }),
        );
        self.send_turn_complete(id, turn_id);
    }

    fn emit_thread_goal_update(&self, id: usize) {
        let turn_id = id.to_string();
        let thread_id = ThreadId::default();
        self.send_event(
            id,
            EventMsg::ThreadGoalUpdated(ThreadGoalUpdatedEvent {
                thread_id,
                turn_id: Some(turn_id.clone()),
                goal: ThreadGoal {
                    thread_id,
                    objective: "Ship the goal update".to_string(),
                    status: ThreadGoalStatus::Active,
                    token_budget: Some(100),
                    tokens_used: 10,
                    time_used_seconds: 2,
                    created_at: 1,
                    updated_at: 2,
                },
            }),
        );
        self.send_turn_complete(id, turn_id);
    }

    fn emit_approval_block(&self, id: usize) {
        self.send_event(
            id,
            EventMsg::ExecApprovalRequest(ExecApprovalRequestEvent {
                call_id: "call-id".to_string(),
                approval_id: Some("approval-id".to_string()),
                turn_id: id.to_string(),
                command: vec!["echo".to_string(), "hi".to_string()],
                cwd: fixture_absolute_path(fixture_cwd()),
                reason: None,
                network_approval_context: None,
                proposed_execpolicy_amendment: None,
                proposed_network_policy_amendments: None,
                additional_permissions: None,
                available_decisions: Some(vec![ReviewDecision::Approved, ReviewDecision::Abort]),
                parsed_cmd: vec![ParsedCommand::Unknown {
                    cmd: "echo hi".to_string(),
                }],
            }),
        );
    }

    fn emit_request_user_input(&self, id: usize) {
        self.send_event(
            id,
            EventMsg::RequestUserInput(RequestUserInputEvent {
                call_id: "user-input-call".to_string(),
                turn_id: id.to_string(),
                questions: vec![RequestUserInputQuestion {
                    id: "confirm_path".to_string(),
                    header: "Confirm".to_string(),
                    question: "Proceed with the plan?".to_string(),
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
            }),
        );
    }

    fn emit_multi_request_user_input(&self, id: usize) {
        self.send_event(
            id,
            EventMsg::RequestUserInput(RequestUserInputEvent {
                call_id: "user-input-multi".to_string(),
                turn_id: id.to_string(),
                questions: vec![
                    RequestUserInputQuestion {
                        id: "choice".to_string(),
                        header: "Choice".to_string(),
                        question: "Pick an option".to_string(),
                        is_other: true,
                        is_secret: false,
                        options: Some(vec![
                            codex_protocol::request_user_input::RequestUserInputQuestionOption {
                                label: "Option 1".to_string(),
                                description: "Take the first option.".to_string(),
                            },
                            codex_protocol::request_user_input::RequestUserInputQuestionOption {
                                label: "Option 2".to_string(),
                                description: "Take the second option.".to_string(),
                            },
                        ]),
                    },
                    RequestUserInputQuestion {
                        id: "details".to_string(),
                        header: "Details".to_string(),
                        question: "Provide context".to_string(),
                        is_other: false,
                        is_secret: false,
                        options: None,
                    },
                ],
            }),
        );
    }

    fn emit_image_generation(&self, id: usize) {
        let saved_path = fixture_absolute_path(fixture_cwd().join("generated.png"));
        self.send_event(
            id,
            EventMsg::ImageGenerationBegin(ImageGenerationBeginEvent {
                call_id: "image-call".to_string(),
            }),
        );
        self.send_event(
            id,
            EventMsg::ImageGenerationEnd(ImageGenerationEndEvent {
                call_id: "image-call".to_string(),
                status: "completed".to_string(),
                revised_prompt: Some("Render a parity diagram".to_string()),
                result: String::new(),
                saved_path: Some(saved_path),
            }),
        );
        self.send_turn_complete(id, id.to_string());
    }

    fn emit_collab_spawn(&self, id: usize) {
        let sender_thread_id = ThreadId::new();
        let receiver_thread_id = ThreadId::new();
        self.send_event(
            id,
            EventMsg::CollabAgentSpawnBegin(CollabAgentSpawnBeginEvent {
                call_id: "spawn-1".to_string(),
                started_at_ms: 0,
                sender_thread_id,
                prompt: "Investigate parity gaps".to_string(),
                model: "gpt-5.4".to_string(),
                reasoning_effort: ReasoningEffort::Medium,
            }),
        );
        self.send_event(
            id,
            EventMsg::CollabAgentSpawnEnd(CollabAgentSpawnEndEvent {
                call_id: "spawn-1".to_string(),
                completed_at_ms: 0,
                sender_thread_id,
                new_thread_id: Some(receiver_thread_id),
                new_agent_nickname: Some("Parity Worker".to_string()),
                new_agent_role: Some("worker".to_string()),
                prompt: "Investigate parity gaps".to_string(),
                model: "gpt-5.4".to_string(),
                reasoning_effort: ReasoningEffort::Medium,
                status: codex_protocol::protocol::AgentStatus::Running,
            }),
        );
        self.send_turn_complete(id, id.to_string());
    }

    fn emit_reasoning_stream(&self, id: usize) {
        let turn_id = id.to_string();
        let item_id = format!("reasoning-{id}");
        self.send_event(
            id,
            EventMsg::ReasoningContentDelta(ReasoningContentDeltaEvent {
                thread_id: ThreadId::default().to_string(),
                turn_id: turn_id.clone(),
                item_id: item_id.clone(),
                delta: "Thinking ".to_string(),
                summary_index: 0,
            }),
        );
        self.send_event(
            id,
            EventMsg::AgentReasoningSectionBreak(AgentReasoningSectionBreakEvent {
                item_id: item_id.clone(),
                summary_index: 1,
            }),
        );
        self.send_event(
            id,
            EventMsg::ReasoningContentDelta(ReasoningContentDeltaEvent {
                thread_id: ThreadId::default().to_string(),
                turn_id: turn_id.clone(),
                item_id: item_id.clone(),
                delta: "hard!".to_string(),
                summary_index: 1,
            }),
        );
        self.send_event(
            id,
            EventMsg::AgentReasoning(AgentReasoningEvent {
                text: "Thinking \n\nhard!".to_string(),
            }),
        );
        self.send_turn_complete(id, turn_id);
    }

    fn emit_reasoning_final(&self, id: usize) {
        let turn_id = id.to_string();
        self.send_event(
            id,
            EventMsg::AgentReasoning(AgentReasoningEvent {
                text: "Final reasoning only".to_string(),
            }),
        );
        self.send_turn_complete(id, turn_id);
    }

    fn emit_echo_prompt(&self, id: usize, prompt: String) {
        self.send_event(
            id,
            EventMsg::AgentMessageContentDelta(AgentMessageContentDeltaEvent {
                thread_id: id.to_string(),
                turn_id: id.to_string(),
                item_id: id.to_string(),
                delta: prompt.clone(),
            }),
        );
        // Send non-delta event (should be deduplicated, but handled by deduplication)
        self.send_event(
            id,
            EventMsg::AgentMessage(AgentMessageEvent {
                message: prompt,
                phase: None,
                memory_citation: None,
            }),
        );
        self.send_turn_complete(id, id.to_string());
    }

    fn lock_ops(&self) -> std::sync::MutexGuard<'_, Vec<Op>> {
        self.ops
            .lock()
            .expect("stub thread ops mutex should not be poisoned")
    }

    fn lock_active_prompt_id(&self) -> std::sync::MutexGuard<'_, Option<String>> {
        self.active_prompt_id
            .lock()
            .expect("stub thread active prompt mutex should not be poisoned")
    }

    fn lock_thread_goal(&self) -> std::sync::MutexGuard<'_, Option<ThreadGoal>> {
        self.thread_goal
            .lock()
            .expect("stub thread goal mutex should not be poisoned")
    }
}

impl CodexThreadImpl for StubCodexThread {
    fn submit(
        &self,
        op: Op,
    ) -> Pin<Box<dyn Future<Output = Result<String, CodexErr>> + Send + '_>> {
        Box::pin(async move {
            let id = self
                .current_id
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

            self.lock_ops().push(op.clone());

            match op {
                Op::UserInput { items, .. } => {
                    self.submit_user_input(id, items);
                }
                Op::Compact => {
                    self.send_event(
                        id,
                        EventMsg::TurnStarted(TurnStartedEvent {
                            model_context_window: None,
                            collaboration_mode_kind: ModeKind::default(),
                            turn_id: id.to_string(),
                            started_at: None,
                        }),
                    );
                    self.send_event(
                        id,
                        EventMsg::AgentMessage(AgentMessageEvent {
                            message: "Compact task completed".to_string(),
                            phase: None,
                            memory_citation: None,
                        }),
                    );
                    self.send_turn_complete(id, id.to_string());
                }
                Op::ThreadRollback { num_turns } => {
                    self.send_event(
                        id,
                        EventMsg::ThreadRolledBack(
                            codex_protocol::protocol::ThreadRolledBackEvent { num_turns },
                        ),
                    );
                    self.send_turn_complete(id, id.to_string());
                }
                Op::Review { review_request } => {
                    self.send_event(id, EventMsg::EnteredReviewMode(review_request.clone()));
                    self.send_event(
                        id,
                        EventMsg::ExitedReviewMode(ExitedReviewModeEvent {
                            review_output: Some(ReviewOutputEvent {
                                findings: vec![],
                                overall_correctness: String::new(),
                                overall_explanation: review_request
                                    .user_facing_hint
                                    .clone()
                                    .unwrap_or_default(),
                                overall_confidence_score: 1.,
                            }),
                        }),
                    );
                    self.send_turn_complete(id, id.to_string());
                }
                Op::UserInputAnswer {
                    id: turn_id,
                    response,
                } => {
                    self.send_event(
                        &turn_id,
                        EventMsg::AgentMessage(AgentMessageEvent {
                            message: format!("received {} answer set(s)", response.answers.len()),
                            phase: None,
                            memory_citation: None,
                        }),
                    );
                    let event_id = turn_id.clone();
                    self.send_turn_complete(event_id, turn_id);
                }
                Op::OverrideTurnContext { .. } => {}
                Op::ExecApproval { .. }
                | Op::ResolveElicitation { .. }
                | Op::RequestPermissionsResponse { .. }
                | Op::PatchApproval { .. }
                | Op::Interrupt => {}
                Op::Shutdown => {
                    if let Some(active_prompt_id) = self.lock_active_prompt_id().take() {
                        let event_id = active_prompt_id.clone();
                        self.send_event(
                            event_id,
                            EventMsg::TurnAborted(TurnAbortedEvent {
                                turn_id: Some(active_prompt_id),
                                reason: codex_protocol::protocol::TurnAbortReason::Interrupted,
                                completed_at: None,
                                duration_ms: None,
                            }),
                        );
                    }
                }
                _ => {
                    unimplemented!()
                }
            }
            Ok(id.to_string())
        })
    }

    fn next_event(&self) -> Pin<Box<dyn Future<Output = Result<Event, CodexErr>> + Send + '_>> {
        Box::pin(async {
            let Some(event) = self.op_rx.lock().await.recv().await else {
                return Err(CodexErr::InternalAgentDied);
            };
            Ok(event)
        })
    }

    fn thread_goal_get(
        &self,
        _thread_id: ThreadId,
    ) -> Pin<Box<dyn Future<Output = Result<Option<ThreadGoal>, Error>> + Send + '_>> {
        Box::pin(async { Ok(self.thread_goal()) })
    }

    fn thread_goal_set(
        &self,
        thread_id: ThreadId,
        request: ThreadGoalSetRequest,
    ) -> Pin<Box<dyn Future<Output = Result<ThreadGoal, Error>> + Send + '_>> {
        Box::pin(async move {
            let ThreadGoalSetRequest {
                objective,
                status,
                token_budget,
            } = request;

            if let Some(value) = token_budget.flatten()
                && value <= 0
            {
                return Err(
                    Error::invalid_params().data("goal budgets must be positive when provided")
                );
            }

            let mut goal = self.lock_thread_goal();
            let next_updated_at = goal
                .as_ref()
                .map(|goal| goal.updated_at.saturating_add(1))
                .unwrap_or(1);

            if let Some(objective) = objective {
                let objective = objective.trim().to_string();
                codex_protocol::protocol::validate_thread_goal_objective(&objective)
                    .map_err(|message| Error::invalid_params().data(message))?;

                let should_update_existing = goal.as_ref().is_some_and(|goal| {
                    goal.objective == objective && goal.status != ThreadGoalStatus::Complete
                });

                if should_update_existing {
                    let existing = goal
                        .as_mut()
                        .expect("matching goal should still be present");
                    if let Some(status) = status {
                        existing.status = status;
                    }
                    if let Some(token_budget) = token_budget {
                        existing.token_budget = token_budget;
                    }
                    existing.updated_at = next_updated_at;
                    return Ok(existing.clone());
                }

                let new_goal = ThreadGoal {
                    thread_id,
                    objective,
                    status: status.unwrap_or(ThreadGoalStatus::Active),
                    token_budget: token_budget.flatten(),
                    tokens_used: 0,
                    time_used_seconds: 0,
                    created_at: next_updated_at,
                    updated_at: next_updated_at,
                };
                *goal = Some(new_goal.clone());
                return Ok(new_goal);
            }

            let Some(existing) = goal.as_mut() else {
                return Err(Error::invalid_params().data(format!(
                    "cannot update goal for thread {thread_id}: no goal exists"
                )));
            };
            if let Some(status) = status {
                existing.status = status;
            }
            if let Some(token_budget) = token_budget {
                existing.token_budget = token_budget;
            }
            existing.updated_at = next_updated_at;
            Ok(existing.clone())
        })
    }

    fn thread_goal_clear(
        &self,
        _thread_id: ThreadId,
    ) -> Pin<Box<dyn Future<Output = Result<bool, Error>> + Send + '_>> {
        Box::pin(async {
            let mut goal = self.lock_thread_goal();
            let cleared = goal.is_some();
            *goal = None;
            Ok(cleared)
        })
    }
}
