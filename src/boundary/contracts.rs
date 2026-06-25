use std::{fs, path::Path};

const DIRECT_EVENT_JSON_PATTERNS: &[&str] = &[
    "serde_json::json!(&event)",
    "serde_json::json!(event)",
    "serde_json::json!(invocation)",
    "serde_json::json!(result)",
    "serde_json::json!(err)",
    "serde_json::json!(output)",
];

const ADVERTISED_ACP_AGENT_HANDLER_PATTERNS: &[&str] = &[
    "request: InitializeRequest",
    "AuthenticateRequest, authenticate",
    "LogoutRequest, logout",
    "NewSessionRequest, new_session",
    "LoadSessionRequest, load_session",
    "ListSessionsRequest, list_sessions",
    "DeleteSessionRequest, delete_session",
    "ResumeSessionRequest, resume_session",
    "ForkSessionRequest, fork_session",
    "CloseSessionRequest, close_session",
    "PromptRequest, prompt",
    "notification: CancelNotification",
    "SetSessionModeRequest, set_session_mode",
    "SetSessionConfigOptionRequest, set_session_config_option",
];

const ENABLED_SDK_AGENT_METHODS_NOT_ADVERTISED: &[&str] = &["McpConnectRequest"];

#[test]
fn acp_agent_registers_every_advertised_handler() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/codex_agent.rs");
    let source = fs::read_to_string(&path).expect("read codex_agent.rs");
    let source = normalize_whitespace(&source);
    let missing = ADVERTISED_ACP_AGENT_HANDLER_PATTERNS
        .iter()
        .filter(|pattern| !source.contains(*pattern))
        .copied()
        .collect::<Vec<_>>();

    assert!(
        missing.is_empty(),
        "Codex ACP must register every handler it advertises:\n{}",
        missing.join("\n")
    );
}

#[test]
fn acp_agent_does_not_advertise_unimplemented_enabled_sdk_methods() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/codex_agent.rs");
    let source = fs::read_to_string(&path).expect("read codex_agent.rs");
    let source = normalize_whitespace(&source);
    let unsupported = ENABLED_SDK_AGENT_METHODS_NOT_ADVERTISED
        .iter()
        .filter(|request| source.contains(**request))
        .copied()
        .collect::<Vec<_>>();

    assert!(
        unsupported.is_empty(),
        "enabled ACP SDK methods without implementation must stay unregistered and unadvertised:\n{}",
        unsupported.join("\n")
    );
    assert!(
        source.contains(".load_session(true)")
            && source.contains(".close(SessionCloseCapabilities::new())")
            && source.contains(".list(SessionListCapabilities::new())")
            && source.contains(".delete(SessionDeleteCapabilities::new())")
            && source.contains(".fork(SessionForkCapabilities::new())")
            && source.contains(".resume(SessionResumeCapabilities::new())")
            && source.contains(
                ".additional_directories(SessionAdditionalDirectoriesCapabilities::new())"
            ),
        "session capabilities must match the registered ACP handler surface"
    );
}

#[test]
fn readmes_expose_current_acp_support_summary_at_the_top() {
    let version = env!("CARGO_PKG_VERSION");
    for readme in ["README.md", "README.ko.md"] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(readme);
        let source = fs::read_to_string(&path).expect("read README");
        let top = source.lines().take(5).collect::<Vec<_>>().join("\n");

        assert!(
            top.contains(version)
                && top.contains("14/14")
                && top.contains("14/16")
                && top.contains("session/fork")
                && top.contains("mcp/connect"),
            "{readme} must expose the current ACP support summary near the top"
        );
    }
}

#[test]
fn readmes_expose_upstream_acp_and_codex_versions() {
    let required = [
        "https://github.com/agentclientprotocol/codex-acp",
        "@agentclientprotocol/codex-acp = 1.0.0",
        "https://crates.io/crates/agent-client-protocol",
        "https://github.com/agentclientprotocol/rust-sdk",
        "agent-client-protocol = 1.0.0",
        "agent-client-protocol-schema = 1.1.0",
        "https://github.com/openai/codex/tree/rust-v0.142.1/codex-rs",
        "95da8fd25193fd58d1c5984eee20d1ef7bd50e77",
    ];

    for readme in ["README.md", "README.ko.md"] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(readme);
        let source = fs::read_to_string(&path).expect("read README");
        let missing = required
            .iter()
            .filter(|text| !source.contains(**text))
            .copied()
            .collect::<Vec<_>>();

        assert!(
            missing.is_empty(),
            "{readme} must document upstream ACP/Codex references and pinned versions:\n{}",
            missing.join("\n")
        );
    }
}

#[test]
fn thread_submission_modules_do_not_serialize_codex_events_directly() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread");
    let mut violations = Vec::new();
    scan_rust_files(&root, &mut |path, source| {
        if path
            .components()
            .any(|component| component.as_os_str() == "tests")
        {
            return;
        }

        for pattern in DIRECT_EVENT_JSON_PATTERNS {
            if source.contains(pattern) {
                violations.push(format!("{} contains {pattern}", path.display()));
            }
        }
    });

    assert!(
        violations.is_empty(),
        "raw Codex event JSON must go through boundary::raw:\n{}",
        violations.join("\n")
    );
}

#[test]
fn thread_runtime_does_not_construct_acp_tool_calls_directly() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread");
    let forbidden = [
        "ToolCall::new",
        "ToolCallUpdate::new",
        "ToolCallStatus::",
        "ToolCallUpdateFields::new",
        "ToolCallContent",
        "ToolCallLocation",
        "ToolKind::",
        "raw::",
        "parse_command_tool_call",
        "parse_patch",
    ];
    let mut violations = Vec::new();
    scan_rust_files(&root, &mut |path, source| {
        if path
            .components()
            .any(|component| component.as_os_str() == "tests")
        {
            return;
        }

        for pattern in forbidden {
            if source.contains(pattern) {
                violations.push(format!("{} contains {pattern}", path.display()));
            }
        }
    });

    assert!(
        violations.is_empty(),
        "ACP tool-call rendering and raw conversion must stay in boundary modules:\n{}",
        violations.join("\n")
    );
}

#[test]
fn thread_runtime_does_not_construct_general_session_updates_directly() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread");
    let forbidden = [
        "SessionUpdate::",
        "ContentChunk::new",
        "UsageUpdate::new",
        "Plan::new",
        "ConfigOptionUpdate::new",
        "AvailableCommandsUpdate::new",
        ".send_user_message(",
        ".send_agent_text(",
        ".send_agent_warning(",
        ".send_agent_thought(",
        ".update_plan(",
    ];
    let mut violations = Vec::new();
    scan_rust_files(&root, &mut |path, source| {
        if path
            .components()
            .any(|component| component.as_os_str() == "tests")
        {
            return;
        }

        for pattern in forbidden {
            if source.contains(pattern) {
                violations.push(format!("{} contains {pattern}", path.display()));
            }
        }
    });

    assert!(
        violations.is_empty(),
        "general ACP SessionUpdate rendering must stay in boundary::session_update:\n{}",
        violations.join("\n")
    );
}

#[test]
fn thread_runtime_does_not_build_permission_request_wire_payloads() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread");
    let forbidden = [
        "RequestPermissionRequest::new",
        "PermissionOption::new",
        "ToolCallUpdate::new",
        "request_permission_effect(tool_call",
        "request_permission_effect(supported_request.tool_call",
    ];
    let mut violations = Vec::new();
    scan_rust_files(&root, &mut |path, source| {
        if path
            .components()
            .any(|component| component.as_os_str() == "tests")
        {
            return;
        }

        for pattern in forbidden {
            if source.contains(pattern) {
                violations.push(format!("{} contains {pattern}", path.display()));
            }
        }
    });

    assert!(
        violations.is_empty(),
        "ACP permission request wire payloads must be built in boundary modules:\n{}",
        violations.join("\n")
    );
}

#[test]
fn submission_dispatch_does_not_use_unreachable_event_routes() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread/submission_dispatch.rs");
    let source = fs::read_to_string(&path).expect("read submission_dispatch.rs");

    assert!(
        !source.contains("unreachable!"),
        "submission dispatch routes should use typed route enums, not unreachable!()"
    );
}

#[test]
fn submission_dispatch_does_not_match_codex_event_variants() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread/submission_dispatch.rs");
    let source = fs::read_to_string(&path).expect("read submission_dispatch.rs");

    assert!(
        !source.contains("EventMsg::"),
        "submission dispatch must execute boundary routes instead of matching Codex event variants"
    );
}

#[test]
fn actor_runtime_does_not_match_codex_event_variants() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread");
    let checked_files = ["actor.rs", "actor_state.rs"];
    let mut violations = Vec::new();

    for file in checked_files {
        let path = root.join(file);
        let source = fs::read_to_string(&path).expect("read actor runtime source");
        if source.contains("EventMsg::") {
            violations.push(path.display().to_string());
        }
    }

    assert!(
        violations.is_empty(),
        "actor runtime must execute boundary actor plans instead of matching Codex event variants:\n{}",
        violations.join("\n")
    );
}

#[test]
fn replay_runtime_does_not_match_codex_event_or_response_variants() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread");
    let checked_files = ["replay.rs", "replay_items.rs"];
    let forbidden = ["EventMsg::", "ResponseItem::", "RolloutItem::"];
    let mut violations = Vec::new();

    for file in checked_files {
        let path = root.join(file);
        let source = fs::read_to_string(&path).expect("read replay runtime source");
        for pattern in forbidden {
            if source.contains(pattern) {
                violations.push(format!("{} contains {pattern}", path.display()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "replay runtime must execute boundary replay routes instead of matching Codex variants:\n{}",
        violations.join("\n")
    );
}

#[test]
fn permission_resolution_executes_boundary_approval_ops() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread/submission/interactions.rs");
    let source = fs::read_to_string(&path).expect("read permission interaction source");
    let forbidden = [
        "RequestPermissionOutcome::",
        "Op::ExecApproval",
        "Op::PatchApproval",
        "Op::RequestPermissionsResponse",
        "Op::ResolveElicitation",
        "thread.submit_ok",
    ];
    let violations = forbidden
        .into_iter()
        .filter(|pattern| source.contains(pattern))
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "permission resolution must execute boundary::approval ops instead of mapping ACP outcomes in runtime:\n{}",
        violations.join("\n")
    );
}

#[test]
fn permission_interactions_execute_bridge_effects() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread/submission/interactions.rs");
    let source = fs::read_to_string(&path).expect("read permission interaction source");
    let required = ["BridgeEffect::RequestPermission", "BridgeEffect::SubmitOp"];
    let missing = required
        .into_iter()
        .filter(|pattern| !source.contains(pattern))
        .collect::<Vec<_>>();
    let forbidden = ["client.request_permission(", ".submit_ok("];
    let violations = forbidden
        .into_iter()
        .filter(|pattern| source.contains(pattern))
        .collect::<Vec<_>>();

    assert!(
        missing.is_empty() && violations.is_empty(),
        "permission interactions must execute request/submit through BridgeEffect; missing: {}; violations: {}",
        missing.join(", "),
        violations.join(", ")
    );
}

#[test]
fn thread_runtime_does_not_construct_codex_ops_directly() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread");
    let mut violations = Vec::new();
    scan_rust_files(&root, &mut |path, source| {
        if path
            .components()
            .any(|component| component.as_os_str() == "tests")
        {
            return;
        }

        if source.contains("Op::") {
            violations.push(path.display().to_string());
        }
    });

    assert!(
        violations.is_empty(),
        "Codex Op construction must stay in boundary::op, boundary::approval, or boundary::mapper:\n{}",
        violations.join("\n")
    );
}

#[test]
fn mcp_elicitation_decline_executes_submit_op_effect() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread/submission_mcp.rs");
    let source = fs::read_to_string(&path).expect("read submission_mcp.rs");

    assert!(
        source.contains("BridgeEffect::SubmitOp"),
        "unsupported MCP elicitation decline must be submitted through BridgeEffect::SubmitOp"
    );
    assert!(
        !source.contains("self.thread.submit_ok"),
        "submission_mcp.rs must not submit Codex ops directly"
    );
}

#[test]
fn thread_runtime_does_not_build_mcp_elicitation_resolution_ops() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread");
    let mut violations = Vec::new();
    scan_rust_files(&root, &mut |path, source| {
        if path
            .components()
            .any(|component| component.as_os_str() == "tests")
        {
            return;
        }

        if source.contains("Op::ResolveElicitation") {
            violations.push(path.display().to_string());
        }
    });

    assert!(
        violations.is_empty(),
        "MCP elicitation resolution ops must be built in boundary::approval or boundary::mcp_approval:\n{}",
        violations.join("\n")
    );
}

#[test]
fn request_permissions_submission_uses_boundary_builder() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread/submission_permissions.rs");
    let source = fs::read_to_string(&path).expect("read submission_permissions.rs");
    let forbidden = [
        "PermissionOption::new",
        "ToolCallUpdate::new",
        "ToolCallId::new",
        "ToolCallStatus::",
    ];
    let violations = forbidden
        .into_iter()
        .filter(|pattern| source.contains(pattern))
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "request permission ACP tool-call rendering must stay in boundary::permission:\n{}",
        violations.join("\n")
    );
}

#[test]
fn exec_approval_submission_uses_boundary_builder() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread/submission_exec.rs");
    let source = fs::read_to_string(&path).expect("read submission_exec.rs");
    let forbidden = [
        "build_exec_permission_options",
        "exec_request_key",
        "fn exec_approval_content",
        "NetworkApprovalContext",
        "ReviewDecision",
    ];
    let violations = forbidden
        .into_iter()
        .filter(|pattern| source.contains(pattern))
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "exec approval ACP permission rendering must stay in boundary::permission:\n{}",
        violations.join("\n")
    );
}

#[test]
fn exec_command_submission_uses_boundary_tool_call_builder() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread/submission_exec.rs");
    let source = fs::read_to_string(&path).expect("read submission_exec.rs");
    let forbidden = [
        "ToolCall::new",
        "ToolCallUpdate::new",
        "ToolCallStatus::",
        "ToolCallUpdateFields::new",
        "ToolCallId::new",
        "raw::exec_command_",
        "parse_command_tool_call",
        "ParseCommandToolCall",
        "ExecCommandStatus",
    ];
    let violations = forbidden
        .into_iter()
        .filter(|pattern| source.contains(pattern))
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "exec command ACP tool-call rendering must stay in boundary::tool_call:\n{}",
        violations.join("\n")
    );
}

#[test]
fn patch_approval_submission_uses_boundary_builder() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread/submission_patch.rs");
    let source = fs::read_to_string(&path).expect("read submission_patch.rs");
    let forbidden = [
        "PermissionOption::new",
        "patch_request_key",
        "ReviewDecision",
        "raw::patch_approval_request",
    ];
    let violations = forbidden
        .into_iter()
        .filter(|pattern| source.contains(pattern))
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "patch approval ACP permission rendering must stay in boundary::permission:\n{}",
        violations.join("\n")
    );
}

#[test]
fn patch_apply_submission_uses_boundary_tool_call_builder() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread/submission_patch.rs");
    let source = fs::read_to_string(&path).expect("read submission_patch.rs");
    let forbidden = [
        "ToolCall::new",
        "ToolCallUpdate::new",
        "ToolCallStatus::",
        "ToolCallUpdateFields::new",
        "PatchApplyStatus",
        "raw::patch_apply_",
        "extract_tool_call_content_from_changes",
        "FileChangeRenderContext",
    ];
    let violations = forbidden
        .into_iter()
        .filter(|pattern| source.contains(pattern))
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "patch apply ACP tool-call rendering must stay in boundary::tool_call:\n{}",
        violations.join("\n")
    );
}

#[test]
fn mcp_tool_call_submission_uses_boundary_tool_call_builder() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread/submission_mcp.rs");
    let source = fs::read_to_string(&path).expect("read submission_mcp.rs");
    let forbidden = [
        "ToolCall::new",
        "ToolCallUpdate::new",
        "ToolCallStatus::",
        "ToolCallUpdateFields::new",
        "ContentBlock",
        "ToolCallContent",
        "raw::mcp_",
    ];
    let violations = forbidden
        .into_iter()
        .filter(|pattern| source.contains(pattern))
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "MCP ACP tool-call rendering must stay in boundary::tool_call:\n{}",
        violations.join("\n")
    );
}

#[test]
fn dynamic_tool_call_runtime_executes_mapper_effects() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread/submission_dispatch.rs");
    let source = fs::read_to_string(&path).expect("read submission_dispatch.rs");
    let forbidden = [
        "LiveForwardEvent::DynamicToolCallRequest",
        "LiveForwardEvent::DynamicToolCallResponse",
        "start_dynamic_tool_call",
        "end_dynamic_tool_call",
    ];
    let violations = forbidden
        .into_iter()
        .filter(|pattern| source.contains(pattern))
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "dynamic live events must execute mapper effects instead of choosing ACP builders:\n{}",
        violations.join("\n")
    );

    let mapper_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/boundary/mapper/live.rs");
    let mapper_source = fs::read_to_string(&mapper_path).expect("read mapper live.rs");
    assert!(mapper_source.contains("dynamic_tool_call_begin_effect"));
    assert!(mapper_source.contains("dynamic_tool_call_end_effect"));
}

#[test]
fn guardian_submission_uses_boundary_tool_call_builder() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread/submission_guardian.rs");
    let source = fs::read_to_string(&path).expect("read submission_guardian.rs");
    let forbidden = [
        "ToolCall::new",
        "ToolCallUpdate::new",
        "ToolCallUpdateFields::new",
        "ToolKind",
        "GuardianAssessmentStatus",
        "raw::guardian_assessment",
        "guardian_assessment_content",
        "guardian_assessment_tool_call_id",
        "guardian_assessment_tool_call_status",
    ];
    let violations = forbidden
        .into_iter()
        .filter(|pattern| source.contains(pattern))
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "guardian ACP tool-call rendering must stay in boundary::tool_call:\n{}",
        violations.join("\n")
    );
}

#[test]
fn web_and_image_submission_uses_boundary_tool_call_builder() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread/submission_web_image.rs");
    let source = fs::read_to_string(&path).expect("read submission_web_image.rs");
    let forbidden = [
        "ToolCall::new",
        "ToolCallUpdate::new",
        "ToolCallStatus::",
        "ToolCallUpdateFields::new",
        "ToolCallLocation",
        "ToolKind",
        "raw::web_search",
        "raw::image_generation",
        "format_image_generation_content",
    ];
    let violations = forbidden
        .into_iter()
        .filter(|pattern| source.contains(pattern))
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "web/image ACP tool-call rendering must stay in boundary::tool_call:\n{}",
        violations.join("\n")
    );
}

#[test]
fn lifecycle_view_image_uses_boundary_tool_call_builder() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread/submission_lifecycle.rs");
    let source = fs::read_to_string(&path).expect("read submission_lifecycle.rs");
    let forbidden = [
        "ToolCall::new",
        "ToolCallContent",
        "ToolCallLocation",
        "ToolCallStatus::",
        "ToolKind::Read",
        "ResourceLink",
        "ContentBlock",
    ];
    let violations = forbidden
        .into_iter()
        .filter(|pattern| source.contains(pattern))
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "view-image ACP tool-call rendering must stay in boundary::tool_call:\n{}",
        violations.join("\n")
    );
}

#[test]
fn collab_runtime_executes_mapper_effects() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread/submission_dispatch.rs");
    let source = fs::read_to_string(&path).expect("read submission_dispatch.rs");
    let forbidden = [
        "LiveForwardEvent::Collab",
        "LiveCollabEvent",
        "handle_collab_event",
        "collab_spawn_begin",
        "collab_spawn_end",
        "collab_interaction_begin",
        "collab_interaction_end",
        "collab_waiting_begin",
        "collab_waiting_end",
        "collab_close_begin",
        "collab_close_end",
        "collab_resume_begin",
        "collab_resume_end",
    ];
    let violations = forbidden
        .into_iter()
        .filter(|pattern| source.contains(pattern))
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "collab live events must execute mapper effects instead of choosing ACP builders:\n{}",
        violations.join("\n")
    );

    let mapper_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/boundary/mapper/live.rs");
    let mapper_source = fs::read_to_string(&mapper_path).expect("read mapper live.rs");
    assert!(mapper_source.contains("collab_spawn_begin_effect"));
    assert!(mapper_source.contains("collab_resume_end_effect"));
}

#[test]
fn replay_event_tool_calls_use_boundary_builders() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread/replay.rs");
    let source = fs::read_to_string(&path).expect("read replay.rs");
    let forbidden = [
        "ToolCall::new",
        "ToolCallStatus::",
        "ToolCallLocation",
        "ToolKind",
        "format_image_generation_content",
    ];
    let violations = forbidden
        .into_iter()
        .filter(|pattern| source.contains(pattern))
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "replay event ACP tool-call rendering must stay in boundary::tool_call:\n{}",
        violations.join("\n")
    );
}

#[test]
fn replay_event_runtime_executes_mapper_effects_only() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread/replay.rs");
    let source = fs::read_to_string(&path).expect("read replay.rs");
    let forbidden = ["session_update::", "tool_call::", "ReplayCollabEvent"];
    let violations = forbidden
        .into_iter()
        .filter(|pattern| source.contains(pattern))
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "replay EventMsg runtime must execute mapper effects instead of choosing ACP builders:\n{}",
        violations.join("\n")
    );
}

#[test]
fn live_stateless_runtime_executes_mapper_effects_only() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread/submission_dispatch.rs");
    let source = fs::read_to_string(&path).expect("read submission_dispatch.rs");
    let forbidden = [
        "LiveForwardEvent::ThreadGoalUpdated",
        "LiveForwardEvent::PlanUpdate",
        "LiveForwardEvent::ThreadRolledBack",
        "LiveForwardEvent::ViewImageToolCall",
        "LiveForwardEvent::Warning",
        "LiveForwardEvent::ContextCompacted",
        "LiveForwardEvent::ImageGenerationBegin",
        "LiveForwardEvent::ImageGenerationEnd",
        "LiveForwardEvent::SkillsUpdateAvailable",
        "LiveForwardEvent::TokenCount",
        "LiveForwardEvent::ExitedReviewMode",
        "LiveForwardEvent::DynamicToolCallRequest",
        "LiveForwardEvent::DynamicToolCallResponse",
        "LiveForwardEvent::McpToolCallBegin",
        "LiveForwardEvent::McpToolCallEnd",
        "LiveForwardEvent::Patch",
        "LivePatchEvent",
        "Self::thread_goal_updated",
        "Self::plan_update",
        "Self::thread_rolled_back",
        "Self::warning",
        "Self::context_compacted",
        "Self::skills_update_available",
        "Self::view_image_tool_call",
        "Self::image_generation_begin",
        "Self::image_generation_end",
        "Self::token_count",
        "Self::review_mode_exit",
        "Self::start_dynamic_tool_call",
        "Self::end_dynamic_tool_call",
        "Self::start_mcp_tool_call",
        "Self::end_mcp_tool_call",
        "Self::start_patch_apply",
        "Self::update_patch_apply",
        "Self::end_patch_apply",
    ];
    let violations = forbidden
        .into_iter()
        .filter(|pattern| source.contains(pattern))
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "stateless live EventMsg runtime must execute mapper effects instead of choosing ACP builders:\n{}",
        violations.join("\n")
    );
}

#[test]
fn replay_response_item_tool_calls_use_boundary_builders() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread/replay_items.rs");
    let source = fs::read_to_string(&path).expect("read replay_items.rs");
    let forbidden = [
        "ToolCall::new",
        "ToolCallUpdate::new",
        "ToolCallStatus::",
        "ToolCallUpdateFields::new",
        "ToolCallLocation",
        "ToolCallContent",
        "ToolKind",
        "raw::",
        "parse_command_tool_call",
        "parse_patch",
        "generate_fallback_id",
        "web_search_action_to_title",
    ];
    let violations = forbidden
        .into_iter()
        .filter(|pattern| source.contains(pattern))
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "replay ResponseItem ACP tool-call rendering must stay in boundary::tool_call:\n{}",
        violations.join("\n")
    );
}

#[test]
fn replay_tool_call_paths_execute_bridge_effects() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread");
    let checked_files = ["replay.rs", "replay_items.rs"];
    let forbidden = ["send_tool_call(", "send_tool_call_update("];
    let mut violations = Vec::new();

    for file in checked_files {
        let path = root.join(file);
        let source = fs::read_to_string(&path).expect("read replay source");
        for pattern in forbidden {
            if source.contains(pattern) {
                violations.push(format!("{} contains {pattern}", path.display()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "replay tool-call paths must return BridgeEffect and let runtime execute it:\n{}",
        violations.join("\n")
    );
}

#[test]
fn tool_call_facade_does_not_render_acp_payloads() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/boundary/tool_call.rs");
    let source = fs::read_to_string(&path).expect("read boundary tool_call facade");
    let forbidden = [
        "agent_client_protocol::",
        "codex_protocol::",
        "ToolCall::new",
        "ToolCallUpdate::new",
        "ToolCallStatus::",
        "ToolCallUpdateFields::new",
        "raw::",
        "serde_json::json!",
    ];
    let violations = forbidden
        .into_iter()
        .filter(|pattern| source.contains(pattern))
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "boundary::tool_call facade must only wire submodules and re-export builders:\n{}",
        violations.join("\n")
    );
}

#[test]
fn converted_live_tool_call_paths_execute_bridge_effects() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/thread");
    let checked_files = [
        "submission_exec.rs",
        "submission_guardian.rs",
        "submission_lifecycle.rs",
        "submission_mcp.rs",
        "submission_patch.rs",
        "submission_web_image.rs",
    ];
    let forbidden = ["send_tool_call(", "send_tool_call_update("];
    let mut violations = Vec::new();

    for file in checked_files {
        let path = root.join(file);
        let source = fs::read_to_string(&path).expect("read converted submission source");
        for pattern in forbidden {
            if source.contains(pattern) {
                violations.push(format!("{} contains {pattern}", path.display()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "converted live tool-call paths must return BridgeEffect and let runtime execute it:\n{}",
        violations.join("\n")
    );
}

#[test]
fn mapper_does_not_use_wildcard_matches() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/boundary/mapper");
    let mut violations = Vec::new();
    scan_rust_files(&root, &mut |path, source| {
        if path.file_name().is_some_and(|name| name == "tests.rs") {
            return;
        }

        if source.contains("_ =>") {
            violations.push(path.display().to_string());
        }
    });

    assert!(
        violations.is_empty(),
        "boundary::mapper must classify Codex variants explicitly:\n{}",
        violations.join("\n")
    );
}

fn scan_rust_files(root: &Path, visit: &mut impl FnMut(&Path, &str)) {
    for entry in fs::read_dir(root).expect("read source directory") {
        let entry = entry.expect("read directory entry");
        let path = entry.path();
        if path.is_dir() {
            scan_rust_files(&path, visit);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let source = fs::read_to_string(&path).expect("read rust source");
            visit(&path, &source);
        }
    }
}

fn normalize_whitespace(source: &str) -> String {
    source.split_whitespace().collect::<Vec<_>>().join(" ")
}
