use super::fixtures::*;

use crate::boundary::constants::meta;

fn poison_mutex<T>(mutex: &std::sync::Mutex<T>) {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = mutex.lock().expect("mutex should lock before poisoning");
        panic!("poison mutex for test");
    }));
    std::panic::set_hook(previous_hook);
    assert!(result.is_err());
}

fn terminal_output_capabilities() -> ClientCapabilities {
    ClientCapabilities::new().meta(Meta::from_iter([(
        meta::TERMINAL_OUTPUT_CAPABILITY.to_string(),
        serde_json::json!(true),
    )]))
}

fn tool_call_default_open(meta: Option<&Meta>) -> Option<bool> {
    meta.and_then(|meta| meta.get(meta::CODEX_ACP))
        .and_then(|value| value.get(meta::TOOL_CALL_OUTPUT))
        .and_then(|value| value.get(meta::TOOL_CALL_OUTPUT_DEFAULT_OPEN))
        .and_then(serde_json::Value::as_bool)
}

fn tool_call_output_reason(meta: Option<&Meta>) -> Option<&str> {
    meta.and_then(|meta| meta.get(meta::CODEX_ACP))
        .and_then(|value| value.get(meta::TOOL_CALL_OUTPUT))
        .and_then(|value| value.get(meta::TOOL_CALL_OUTPUT_REASON))
        .and_then(serde_json::Value::as_str)
}

fn zed_client_info() -> Implementation {
    Implementation::new("zed", "0.0.0").title("Zed")
}

fn toad_client_info() -> Implementation {
    Implementation::new("toad", "0.0.0").title("Toad")
}

fn terminal_active_command() -> ActiveCommand {
    ActiveCommand {
        tool_call_id: ToolCallId::new("call-id"),
        title: "Shell".to_string(),
        kind: ToolKind::Other,
        terminal_output: true,
        output: String::new(),
        file_extension: None,
        cwd: std::env::current_dir().expect("current dir should be available"),
    }
}

fn rendered_output_text_for_extension(extension: Option<&str>) -> String {
    let command = ActiveCommand {
        tool_call_id: ToolCallId::new("call-id"),
        title: "Read file".to_string(),
        kind: ToolKind::Read,
        terminal_output: false,
        output: String::new(),
        file_extension: extension.map(str::to_string),
        cwd: std::env::current_dir().expect("current dir should be available"),
    };

    match command.render_output_content("body\n").into_iter().next() {
        Some(ToolCallContent::Content(Content {
            content: ContentBlock::Text(TextContent { text, .. }),
            ..
        })) => text,
        content => panic!("expected text content, got {content:?}"),
    }
}

fn rendered_output_content(output: &str, cwd: PathBuf) -> Vec<ToolCallContent> {
    let command = ActiveCommand {
        tool_call_id: ToolCallId::new("call-id"),
        title: "git diff".to_string(),
        kind: ToolKind::Other,
        terminal_output: true,
        output: String::new(),
        file_extension: None,
        cwd,
    };

    command.render_output_content(output)
}

#[test]
fn submission_exec_raw_json_goes_through_boundary_helpers() {
    let source = include_str!("../submission_exec.rs");
    assert!(
        !source.contains("serde_json::json!(&event)"),
        "submission_exec.rs should build raw payloads through boundary::raw"
    );
}

#[test]
fn test_read_output_uses_canonical_fence_languages() {
    assert_eq!(
        rendered_output_text_for_extension(Some("rs")),
        "```rust\nbody\n```\n"
    );
    assert_eq!(
        rendered_output_text_for_extension(Some("js")),
        "```javascript\nbody\n```\n"
    );
    assert_eq!(
        rendered_output_text_for_extension(Some("ts")),
        "```typescript\nbody\n```\n"
    );
    assert_eq!(
        rendered_output_text_for_extension(Some("py")),
        "```python\nbody\n```\n"
    );
    assert_eq!(
        rendered_output_text_for_extension(Some("sh")),
        "```bash\nbody\n```\n"
    );
    assert_eq!(
        rendered_output_text_for_extension(Some("zsh")),
        "```bash\nbody\n```\n"
    );
    assert_eq!(
        rendered_output_text_for_extension(Some("yml")),
        "```yaml\nbody\n```\n"
    );
    assert_eq!(
        rendered_output_text_for_extension(Some("md")),
        "```markdown\nbody\n```\n"
    );
    assert_eq!(
        rendered_output_text_for_extension(Some("go")),
        "```go\nbody\n```\n"
    );
    assert_eq!(
        rendered_output_text_for_extension(None),
        "```sh\nbody\n```\n"
    );
}

#[test]
fn test_unified_diff_output_uses_diff_content() -> anyhow::Result<()> {
    let cwd = std::env::temp_dir()
        .join("codex-acp-exec-diff-tests")
        .join(uuid::Uuid::new_v4().to_string());
    let path = cwd.join("src/lib.rs");
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(&path, "fn main() { println!(\"hi\"); }\n")?;
    let output = "\
diff --git a/src/lib.rs b/src/lib.rs
index 1111111..2222222 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1 +1 @@
-fn main(){println!(\"hi\");}
+fn main() { println!(\"hi\"); }
";

    let content = rendered_output_content(output, cwd);

    assert!(matches!(
        content.first(),
        Some(ToolCallContent::Diff(diff))
            if diff.path == path
                && diff.old_text.as_deref() == Some("fn main(){println!(\"hi\");}\n")
                && diff.new_text == "fn main() { println!(\"hi\"); }\n"
    ));

    Ok(())
}

#[tokio::test]
async fn test_parallel_exec_commands() -> anyhow::Result<()> {
    let (session_id, client, _, message_tx, _handle) = setup().await?;
    let stop_reason = submit_prompt_and_wait(&session_id, &message_tx, "parallel-exec").await?;
    assert_eq!(stop_reason, StopReason::EndTurn);
    drop(message_tx);

    let tool_calls = client.tool_calls();
    let completed_updates = client.completed_tool_call_updates();

    // Both commands A and B should have produced a ToolCall (begin).
    assert_eq!(
        tool_calls.len(),
        2,
        "expected 2 ToolCall begin notifications, got {tool_calls:?}"
    );

    // Both commands A and B should have produced a completed ToolCallUpdate.
    assert_eq!(
        completed_updates.len(),
        2,
        "expected 2 completed ToolCallUpdate notifications, got {completed_updates:?}"
    );

    // The completed updates should reference the same tool_call_ids as the begins.
    let begin_ids: std::collections::HashSet<_> = tool_calls
        .iter()
        .map(|tc| tc.tool_call_id.clone())
        .collect();
    let end_ids: std::collections::HashSet<_> = completed_updates
        .iter()
        .map(|u| u.tool_call_id.clone())
        .collect();
    assert_eq!(
        begin_ids, end_ids,
        "completed update tool_call_ids should match begin tool_call_ids"
    );

    Ok(())
}

#[test]
fn test_terminal_output_support_recovers_poisoned_client_capabilities() {
    let session_id = SessionId::new("test");
    let client = Arc::new(StubClient::new());
    let client_capabilities = Arc::new(std::sync::Mutex::new(terminal_output_capabilities()));
    poison_mutex(&client_capabilities);
    let client_info = Arc::new(std::sync::Mutex::new(Some(zed_client_info())));
    let session_client =
        SessionClient::with_client(session_id, client, client_capabilities, client_info);

    assert!(session_client.supports_terminal_output(&terminal_active_command()));
}

#[test]
fn test_terminal_output_support_recovers_poisoned_client_info() {
    let session_id = SessionId::new("test");
    let client = Arc::new(StubClient::new());
    let client_capabilities = Arc::new(std::sync::Mutex::new(terminal_output_capabilities()));
    let client_info = Arc::new(std::sync::Mutex::new(Some(zed_client_info())));
    poison_mutex(&client_info);
    let session_client =
        SessionClient::with_client(session_id, client, client_capabilities, client_info);

    assert!(session_client.supports_terminal_output(&terminal_active_command()));
}

#[test]
fn test_toad_client_without_terminal_meta_uses_content_snapshots() {
    let session_id = SessionId::new("test");
    let client = Arc::new(StubClient::new());
    let client_capabilities = Arc::new(std::sync::Mutex::new(ClientCapabilities::new()));
    let client_info = Arc::new(std::sync::Mutex::new(Some(toad_client_info())));
    let session_client =
        SessionClient::with_client(session_id, client, client_capabilities, client_info);

    assert!(!session_client.supports_terminal_output(&terminal_active_command()));
}

#[test]
fn test_search_commands_render_without_terminal_output() -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;

    let search = parse_command_tool_call(
        vec![ParsedCommand::Search {
            cmd: "rg parity".to_string(),
            query: Some("parity".to_string()),
            path: None,
        }],
        &cwd,
    );
    assert!(!search.terminal_output);
    assert!(matches!(search.kind, ToolKind::Search));

    let wrapped_search = parse_command_tool_call(
        vec![ParsedCommand::Unknown {
            cmd: "command rg parity src".to_string(),
        }],
        &cwd,
    );
    assert!(!wrapped_search.terminal_output);
    assert!(matches!(wrapped_search.kind, ToolKind::Search));

    let list = parse_command_tool_call(
        vec![ParsedCommand::ListFiles {
            cmd: "find src".to_string(),
            path: Some("src".to_string()),
        }],
        &cwd,
    );
    assert!(list.terminal_output);

    let read = parse_command_tool_call(
        vec![ParsedCommand::Read {
            cmd: "cat README.md".to_string(),
            name: "README.md".to_string(),
            path: PathBuf::from("README.md"),
        }],
        &cwd,
    );
    assert!(!read.terminal_output);

    let diff = parse_command_tool_call(
        vec![ParsedCommand::Unknown {
            cmd: "git diff -- README.md".to_string(),
        }],
        &cwd,
    );
    assert!(!diff.terminal_output);
    assert!(matches!(diff.kind, ToolKind::Read));
    assert_eq!(diff.file_extension.as_deref(), Some("diff"));

    let diff_stat = parse_command_tool_call(
        vec![ParsedCommand::Unknown {
            cmd: "git diff --stat".to_string(),
        }],
        &cwd,
    );
    assert!(diff_stat.terminal_output);

    Ok(())
}

#[test]
fn test_unknown_commands_reparse_with_codex_bash_rules() -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;

    let batcat_read = parse_command_tool_call(
        vec![ParsedCommand::Unknown {
            cmd: "command batcat README.md".to_string(),
        }],
        &cwd,
    );
    assert!(!batcat_read.terminal_output);
    assert!(matches!(batcat_read.kind, ToolKind::Read));
    assert_eq!(batcat_read.file_extension.as_deref(), Some("md"));
    assert_eq!(batcat_read.title, "Read README.md");

    let git_grep = parse_command_tool_call(
        vec![ParsedCommand::Unknown {
            cmd: "command git grep TODO src".to_string(),
        }],
        &cwd,
    );
    assert!(!git_grep.terminal_output);
    assert!(matches!(git_grep.kind, ToolKind::Search));
    assert_eq!(git_grep.title, "Search TODO in src");

    let bash_list = parse_command_tool_call(
        vec![ParsedCommand::Unknown {
            cmd: "bash -lc 'rg --files | head -n 50'".to_string(),
        }],
        &cwd,
    );
    assert!(bash_list.terminal_output);
    assert!(matches!(bash_list.kind, ToolKind::Search));
    assert!(bash_list.title.starts_with("List "));

    let piped_read = parse_command_tool_call(
        vec![ParsedCommand::Unknown {
            cmd: "nl -ba src/lib.rs | sed -n '1,20p'".to_string(),
        }],
        &cwd,
    );
    assert!(!piped_read.terminal_output);
    assert!(matches!(piped_read.kind, ToolKind::Read));
    assert_eq!(piped_read.file_extension.as_deref(), Some("rs"));
    assert_eq!(piped_read.title, "Read lib.rs");

    for cmd in [
        "rg -l TODO src | xargs perl -pi -e 's/TODO/DONE/g'",
        "cat README.md | xargs perl -pi -e 's/a/b/g'",
        "git diff -- README.md | xargs perl -pi -e 's/a/b/g'",
        "cat README.md > README.ko.md",
        "rg TODO src > README.ko.md",
        "git diff -- README.md > README.ko.md",
    ] {
        let execute = parse_command_tool_call(
            vec![ParsedCommand::Unknown {
                cmd: cmd.to_string(),
            }],
            &cwd,
        );
        assert!(execute.terminal_output, "{cmd}");
        assert!(matches!(execute.kind, ToolKind::Execute), "{cmd}");
    }

    Ok(())
}

#[tokio::test]
async fn test_rg_exec_completion_does_not_embed_terminal() -> anyhow::Result<()> {
    let session_id = SessionId::new("test");
    let client = Arc::new(StubClient::new());
    let client_capabilities = Arc::new(std::sync::Mutex::new(terminal_output_capabilities()));
    let client_info = Arc::new(std::sync::Mutex::new(Some(zed_client_info())));
    let session_client =
        SessionClient::with_client(session_id, client.clone(), client_capabilities, client_info);
    let thread = Arc::new(StubCodexThread::new());
    let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
    let (message_tx, _message_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut prompt_state =
        PromptState::new("submission-id".to_string(), thread, message_tx, response_tx);
    let cwd = std::env::current_dir()?;

    prompt_state
        .handle_event(
            &session_client,
            EventMsg::ExecCommandBegin(test_fixtures::exec_command_begin(
                "call-id",
                "turn-id",
                cwd.clone().try_into()?,
                vec!["rg".to_string(), "Search".to_string(), "src".to_string()],
                vec![ParsedCommand::Unknown {
                    cmd: "rg Search src".to_string(),
                }],
            )),
        )
        .await;
    prompt_state
        .handle_event(
            &session_client,
            EventMsg::ExecCommandEnd(test_fixtures::exec_command_end(
                "call-id",
                "turn-id",
                cwd.try_into()?,
                vec!["rg".to_string(), "Search".to_string(), "src".to_string()],
                "src/permission.rs:Search result\n",
            )),
        )
        .await;

    let tool_call = client
        .tool_calls()
        .into_iter()
        .next()
        .expect("expected initial tool call");
    assert!(matches!(
        tool_call.content.first(),
        Some(ToolCallContent::Content(Content {
            content: ContentBlock::Text(TextContent { text, .. }),
            ..
        })) if text.contains("Waiting for command output")
    ));

    let update = client
        .tool_call_updates()
        .into_iter()
        .next()
        .expect("expected completion update");

    assert_eq!(update.fields.status, Some(ToolCallStatus::Completed));
    assert_eq!(update.fields.kind, Some(ToolKind::Search));
    assert!(matches!(
        update.fields.content.as_ref().and_then(|content| content.first()),
        Some(ToolCallContent::Content(Content {
            content: ContentBlock::Text(TextContent { text, .. }),
            ..
        })) if text == "```sh\nsrc/permission.rs:Search result\n```\n"
    ));

    Ok(())
}

#[test]
fn test_unknown_file_reads_are_rendered_as_read_content() -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;

    for cmd in [
        "sed -n '1,120p' src/lib.rs",
        "sed -n -e '1,120p' -- src/lib.rs",
        "sed -ne'/fn main/p' src/lib.rs",
        "sed --quiet --expression='1,120p' src/lib.rs",
        "command sed -nE '1,120p' src/lib.rs",
    ] {
        let rust_read = parse_command_tool_call(
            vec![ParsedCommand::Unknown {
                cmd: cmd.to_string(),
            }],
            &cwd,
        );

        assert!(!rust_read.terminal_output, "{cmd}");
        assert!(matches!(rust_read.kind, ToolKind::Read), "{cmd}");
        assert_eq!(rust_read.file_extension.as_deref(), Some("rs"), "{cmd}");
        assert_eq!(rust_read.title, "Read lib.rs", "{cmd}");
        assert_eq!(rust_read.locations.len(), 1, "{cmd}");

        assert_eq!(
            rendered_output_text_for_extension(rust_read.file_extension.as_deref()),
            "```rust\nbody\n```\n",
            "{cmd}"
        );
    }

    let markdown_read = parse_command_tool_call(
        vec![ParsedCommand::Unknown {
            cmd: "sed -n '1,80p' README.md".to_string(),
        }],
        &cwd,
    );

    assert!(!markdown_read.terminal_output);
    assert!(matches!(markdown_read.kind, ToolKind::Read));
    assert_eq!(markdown_read.file_extension.as_deref(), Some("md"));
    assert_eq!(markdown_read.title, "Read README.md");
    assert_eq!(
        rendered_output_text_for_extension(markdown_read.file_extension.as_deref()),
        "```markdown\nbody\n```\n"
    );

    let env_prefixed_full_path_read = parse_command_tool_call(
        vec![ParsedCommand::Unknown {
            cmd: "NO_COLOR=1 /usr/bin/cat README.md".to_string(),
        }],
        &cwd,
    );
    assert!(!env_prefixed_full_path_read.terminal_output);
    assert!(matches!(env_prefixed_full_path_read.kind, ToolKind::Read));
    assert_eq!(
        env_prefixed_full_path_read.file_extension.as_deref(),
        Some("md")
    );

    let sed_stdout_transform_read = parse_command_tool_call(
        vec![ParsedCommand::Unknown {
            cmd: "sed 's/main/test/' src/lib.rs".to_string(),
        }],
        &cwd,
    );
    assert!(!sed_stdout_transform_read.terminal_output);
    assert!(matches!(sed_stdout_transform_read.kind, ToolKind::Read));
    assert_eq!(
        sed_stdout_transform_read.file_extension.as_deref(),
        Some("rs")
    );

    let jq_read = parse_command_tool_call(
        vec![ParsedCommand::Unknown {
            cmd: "jq . npm/package.json".to_string(),
        }],
        &cwd,
    );
    assert!(!jq_read.terminal_output);
    assert!(matches!(jq_read.kind, ToolKind::Read));
    assert_eq!(jq_read.file_extension.as_deref(), Some("json"));
    assert_eq!(
        rendered_output_text_for_extension(jq_read.file_extension.as_deref()),
        "```json\nbody\n```\n"
    );

    let temp_cwd = std::env::temp_dir()
        .join("codex-acp-read-highlight-tests")
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&temp_cwd)?;
    std::fs::write(temp_cwd.join("CONFIG.JSON"), "{}\n")?;
    let uppercase_extension_read = parse_command_tool_call(
        vec![ParsedCommand::Unknown {
            cmd: "sed -n '1,20p' CONFIG.JSON".to_string(),
        }],
        &temp_cwd,
    );
    assert!(!uppercase_extension_read.terminal_output);
    assert_eq!(
        uppercase_extension_read.file_extension.as_deref(),
        Some("json")
    );
    assert_eq!(
        rendered_output_text_for_extension(uppercase_extension_read.file_extension.as_deref()),
        "```json\nbody\n```\n"
    );

    let mutating_sed = parse_command_tool_call(
        vec![ParsedCommand::Unknown {
            cmd: "sed -i 's/a/b/' src/lib.rs".to_string(),
        }],
        &cwd,
    );
    assert!(mutating_sed.terminal_output);
    assert!(matches!(mutating_sed.kind, ToolKind::Execute));

    let mutating_yq = parse_command_tool_call(
        vec![ParsedCommand::Unknown {
            cmd: "yq -i '.name = \"changed\"' npm/package.json".to_string(),
        }],
        &cwd,
    );
    assert!(mutating_yq.terminal_output);
    assert!(matches!(mutating_yq.kind, ToolKind::Execute));

    Ok(())
}

#[tokio::test]
async fn test_git_diff_exec_completion_uses_diff_content() -> anyhow::Result<()> {
    let session_id = SessionId::new("test");
    let client = Arc::new(StubClient::new());
    let session_client =
        SessionClient::with_client(session_id, client.clone(), Arc::default(), Arc::default());
    let thread = Arc::new(StubCodexThread::new());
    let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
    let (message_tx, _message_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut prompt_state =
        PromptState::new("submission-id".to_string(), thread, message_tx, response_tx);
    let cwd = std::env::temp_dir()
        .join("codex-acp-git-diff-exec-tests")
        .join(uuid::Uuid::new_v4().to_string());
    let path = cwd.join("src/lib.rs");
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(&path, "fn main() { println!(\"hi\"); }\n")?;
    let diff_output = "\
diff --git i/src/lib.rs w/src/lib.rs
index 1111111..2222222 100644
--- i/src/lib.rs
+++ w/src/lib.rs
@@ -1 +1 @@
-fn main(){println!(\"hi\");}
+fn main() { println!(\"hi\"); }
";

    prompt_state
        .handle_event(
            &session_client,
            EventMsg::ExecCommandBegin(test_fixtures::exec_command_begin(
                "call-id",
                "turn-id",
                cwd.clone().try_into()?,
                vec![
                    "git".to_string(),
                    "diff".to_string(),
                    "--".to_string(),
                    "src/lib.rs".to_string(),
                ],
                vec![ParsedCommand::Unknown {
                    cmd: "git diff -- src/lib.rs".to_string(),
                }],
            )),
        )
        .await;
    prompt_state
        .handle_event(
            &session_client,
            EventMsg::ExecCommandEnd(test_fixtures::exec_command_end(
                "call-id",
                "turn-id",
                cwd.try_into()?,
                vec![
                    "git".to_string(),
                    "diff".to_string(),
                    "--".to_string(),
                    "src/lib.rs".to_string(),
                ],
                diff_output,
            )),
        )
        .await;

    let update = client
        .tool_call_updates()
        .into_iter()
        .next()
        .expect("expected completion update");

    assert_eq!(update.fields.status, Some(ToolCallStatus::Completed));
    assert_eq!(update.fields.kind, Some(ToolKind::Read));
    assert!(matches!(
        update
            .fields
            .content
            .as_ref()
            .and_then(|content| content.first()),
        Some(ToolCallContent::Diff(diff))
            if diff.path == path
                && diff.old_text.as_deref() == Some("fn main(){println!(\"hi\");}\n")
                && diff.new_text == "fn main() { println!(\"hi\"); }\n"
    ));

    Ok(())
}

#[tokio::test]
async fn test_zed_client_keeps_terminal_meta_streaming() -> anyhow::Result<()> {
    let session_id = SessionId::new("test");
    let client = Arc::new(StubClient::new());
    let client_capabilities = Arc::new(std::sync::Mutex::new(ClientCapabilities::new().meta(
        Meta::from_iter([(
            meta::TERMINAL_OUTPUT_CAPABILITY.to_string(),
            serde_json::json!(true),
        )]),
    )));
    let client_info = Arc::new(std::sync::Mutex::new(Some(
        Implementation::new("zed", "0.0.0").title("Zed"),
    )));
    let session_client =
        SessionClient::with_client(session_id, client.clone(), client_capabilities, client_info);
    let thread = Arc::new(StubCodexThread::new());
    let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
    let (message_tx, _message_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut prompt_state =
        PromptState::new("submission-id".to_string(), thread, message_tx, response_tx);
    let cwd = std::env::current_dir()?;

    prompt_state
        .handle_event(
            &session_client,
            EventMsg::ExecCommandBegin(test_fixtures::exec_command_begin(
                "call-id",
                "turn-id",
                cwd.clone().try_into()?,
                vec!["echo".to_string(), "hello".to_string()],
                vec![ParsedCommand::Unknown {
                    cmd: "echo hello".to_string(),
                }],
            )),
        )
        .await;
    prompt_state
        .handle_event(
            &session_client,
            EventMsg::ExecCommandOutputDelta(ExecCommandOutputDeltaEvent {
                call_id: "call-id".to_string(),
                chunk: b"hello\n".to_vec(),
                stream: codex_protocol::protocol::ExecOutputStream::Stdout,
            }),
        )
        .await;
    prompt_state
        .handle_event(
            &session_client,
            EventMsg::ExecCommandEnd(test_fixtures::exec_command_end(
                "call-id",
                "turn-id",
                cwd.try_into()?,
                vec!["echo".to_string(), "hello".to_string()],
                "hello\n",
            )),
        )
        .await;

    let tool_call = client
        .tool_calls()
        .into_iter()
        .next()
        .expect("expected initial tool call");
    assert_eq!(tool_call_default_open(tool_call.meta.as_ref()), Some(false));
    assert!(matches!(
        tool_call.content.first(),
        Some(ToolCallContent::Terminal(Terminal { terminal_id, .. }))
            if terminal_id.0.as_ref() == "call-id"
    ));
    assert_eq!(
        tool_call
            .meta
            .as_ref()
            .and_then(|meta| meta.get(meta::TERMINAL_INFO))
            .and_then(|value| value.get("terminal_id"))
            .and_then(serde_json::Value::as_str),
        Some("call-id")
    );

    let tool_updates = client.tool_call_updates();

    let streaming_update = tool_updates.first().expect("expected streaming update");
    assert!(streaming_update.fields.content.is_none());
    assert_eq!(
        streaming_update
            .meta
            .as_ref()
            .and_then(|meta| meta.get(meta::TERMINAL_OUTPUT))
            .and_then(|value| value.get("terminal_id"))
            .and_then(serde_json::Value::as_str),
        Some("call-id")
    );

    let completion_update = tool_updates.last().expect("expected completion update");
    assert_eq!(
        tool_call_output_reason(completion_update.meta.as_ref()),
        Some("shortOutput")
    );
    assert_eq!(
        completion_update.fields.status,
        Some(ToolCallStatus::Completed)
    );
    assert_eq!(
        completion_update.fields.content.as_ref().map(Vec::len),
        Some(1)
    );
    assert!(matches!(
        completion_update
            .fields
            .content
            .as_ref()
            .and_then(|content| content.as_slice().first()),
        Some(ToolCallContent::Terminal(Terminal { terminal_id, .. }))
            if terminal_id.0.as_ref() == "call-id"
    ));
    assert_eq!(
        completion_update
            .meta
            .as_ref()
            .and_then(|meta| meta.get(meta::TERMINAL_EXIT))
            .and_then(|value| value.get("terminal_id"))
            .and_then(serde_json::Value::as_str),
        Some("call-id")
    );

    Ok(())
}

#[tokio::test]
async fn test_zed_client_replays_completion_output_without_delta() -> anyhow::Result<()> {
    let session_id = SessionId::new("test");
    let client = Arc::new(StubClient::new());
    let client_capabilities = Arc::new(std::sync::Mutex::new(ClientCapabilities::new().meta(
        Meta::from_iter([(
            meta::TERMINAL_OUTPUT_CAPABILITY.to_string(),
            serde_json::json!(true),
        )]),
    )));
    let client_info = Arc::new(std::sync::Mutex::new(Some(
        Implementation::new("zed", "0.0.0").title("Zed"),
    )));
    let session_client =
        SessionClient::with_client(session_id, client.clone(), client_capabilities, client_info);
    let thread = Arc::new(StubCodexThread::new());
    let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
    let (message_tx, _message_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut prompt_state =
        PromptState::new("submission-id".to_string(), thread, message_tx, response_tx);
    let cwd = std::env::current_dir()?;

    prompt_state
        .handle_event(
            &session_client,
            EventMsg::ExecCommandBegin(test_fixtures::exec_command_begin(
                "call-id",
                "turn-id",
                cwd.try_into()?,
                vec!["date".to_string()],
                vec![ParsedCommand::Unknown {
                    cmd: "date".to_string(),
                }],
            )),
        )
        .await;
    prompt_state
        .handle_event(
            &session_client,
            EventMsg::ExecCommandEnd(test_fixtures::exec_command_end(
                "call-id",
                "turn-id",
                std::env::current_dir()?.try_into()?,
                vec!["date".to_string()],
                "hello\n",
            )),
        )
        .await;

    let tool_updates = client.tool_call_updates();

    assert_eq!(tool_updates.len(), 2);

    let replayed_output = tool_updates
        .first()
        .expect("expected replayed output update");
    assert_eq!(
        replayed_output
            .meta
            .as_ref()
            .and_then(|meta| meta.get(meta::TERMINAL_OUTPUT))
            .and_then(|value| value.get("data"))
            .and_then(serde_json::Value::as_str),
        Some("hello\n")
    );

    let completion_update = tool_updates.last().expect("expected completion update");
    assert_eq!(
        tool_call_default_open(completion_update.meta.as_ref()),
        Some(true)
    );
    assert_eq!(
        tool_call_output_reason(completion_update.meta.as_ref()),
        Some("directAnswerCommand")
    );
    assert_eq!(
        completion_update.fields.content.as_ref().map(Vec::len),
        Some(1)
    );
    assert!(matches!(
        completion_update
            .fields
            .content
            .as_ref()
            .and_then(|content| content.as_slice().first()),
        Some(ToolCallContent::Terminal(Terminal { terminal_id, .. }))
            if terminal_id.0.as_ref() == "call-id"
    ));
    assert_eq!(
        completion_update
            .meta
            .as_ref()
            .and_then(|meta| meta.get(meta::TERMINAL_EXIT))
            .and_then(|value| value.get("terminal_id"))
            .and_then(serde_json::Value::as_str),
        Some("call-id")
    );

    Ok(())
}

#[tokio::test]
async fn test_terminal_capability_falls_back_to_content_snapshots() -> anyhow::Result<()> {
    let session_id = SessionId::new("test");
    let client = Arc::new(StubClient::new());
    let client_capabilities = Arc::new(std::sync::Mutex::new(ClientCapabilities::new().meta(
        Meta::from_iter([(
            meta::TERMINAL_OUTPUT_CAPABILITY.to_string(),
            serde_json::json!(true),
        )]),
    )));
    let session_client = SessionClient::with_client(
        session_id,
        client.clone(),
        client_capabilities,
        Arc::default(),
    );
    let thread = Arc::new(StubCodexThread::new());
    let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
    let (message_tx, _message_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut prompt_state =
        PromptState::new("submission-id".to_string(), thread, message_tx, response_tx);
    let cwd = std::env::current_dir()?;

    prompt_state
        .handle_event(
            &session_client,
            EventMsg::ExecCommandBegin(test_fixtures::exec_command_begin(
                "call-id",
                "turn-id",
                cwd.clone().try_into()?,
                vec!["echo".to_string(), "hello".to_string()],
                vec![ParsedCommand::Unknown {
                    cmd: "echo hello".to_string(),
                }],
            )),
        )
        .await;
    prompt_state
        .handle_event(
            &session_client,
            EventMsg::ExecCommandOutputDelta(ExecCommandOutputDeltaEvent {
                call_id: "call-id".to_string(),
                chunk: b"hello\n".to_vec(),
                stream: codex_protocol::protocol::ExecOutputStream::Stdout,
            }),
        )
        .await;
    prompt_state
        .handle_event(
            &session_client,
            EventMsg::ExecCommandEnd(test_fixtures::exec_command_end(
                "call-id",
                "turn-id",
                cwd.try_into()?,
                vec!["echo".to_string(), "hello".to_string()],
                "hello\n",
            )),
        )
        .await;

    let tool_call = client
        .tool_calls()
        .into_iter()
        .next()
        .expect("expected initial tool call");
    assert!(matches!(
        tool_call.content.first(),
        Some(ToolCallContent::Content(Content {
            content: ContentBlock::Text(TextContent { text, .. }),
            ..
        })) if text.contains("Waiting for command output")
    ));

    let tool_updates = client.tool_call_updates();

    // Non-terminal clients no longer receive streaming updates to avoid O(n²) memory
    // growth. Only the completion update is sent with the full output snapshot.
    assert_eq!(tool_updates.len(), 1);

    let update = tool_updates.last().expect("expected completion update");

    assert_eq!(update.fields.status, Some(ToolCallStatus::Completed));
    assert!(matches!(
        update
            .fields
            .content
            .as_ref()
            .and_then(|content| content.first()),
        Some(ToolCallContent::Content(Content {
            content: ContentBlock::Text(TextContent { text, .. }),
            ..
        })) if text.contains("hello")
    ));
    assert_eq!(tool_call_default_open(update.meta.as_ref()), Some(true));
    assert_eq!(
        tool_call_output_reason(update.meta.as_ref()),
        Some("shortOutput")
    );

    Ok(())
}

#[tokio::test]
async fn test_non_terminal_exec_completion_includes_output_snapshot() -> anyhow::Result<()> {
    let session_id = SessionId::new("test");
    let client = Arc::new(StubClient::new());
    let session_client =
        SessionClient::with_client(session_id, client.clone(), Arc::default(), Arc::default());
    let thread = Arc::new(StubCodexThread::new());
    let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
    let (message_tx, _message_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut prompt_state =
        PromptState::new("submission-id".to_string(), thread, message_tx, response_tx);
    let cwd = std::env::current_dir()?;

    prompt_state
        .handle_event(
            &session_client,
            EventMsg::ExecCommandBegin(test_fixtures::exec_command_begin(
                "call-id",
                "turn-id",
                cwd.clone().try_into()?,
                vec!["echo".to_string(), "hello".to_string()],
                vec![ParsedCommand::Unknown {
                    cmd: "echo hello".to_string(),
                }],
            )),
        )
        .await;
    prompt_state
        .handle_event(
            &session_client,
            EventMsg::ExecCommandOutputDelta(ExecCommandOutputDeltaEvent {
                call_id: "call-id".to_string(),
                chunk: b"hello\n".to_vec(),
                stream: codex_protocol::protocol::ExecOutputStream::Stdout,
            }),
        )
        .await;
    prompt_state
        .handle_event(
            &session_client,
            EventMsg::ExecCommandEnd(test_fixtures::exec_command_end(
                "call-id",
                "turn-id",
                cwd.try_into()?,
                vec!["echo".to_string(), "hello".to_string()],
                "hello\n",
            )),
        )
        .await;

    let tool_updates = client.tool_call_updates();

    // Non-terminal clients no longer receive streaming updates to avoid O(n²) memory
    // growth. Only the completion update is sent with the full output snapshot.
    assert_eq!(tool_updates.len(), 1);

    let update = tool_updates.last().expect("expected completion update");
    assert_eq!(update.fields.status, Some(ToolCallStatus::Completed));
    assert!(matches!(
        update.fields.content.as_ref().and_then(|content| content.first()),
        Some(ToolCallContent::Content(Content {
            content: ContentBlock::Text(TextContent { text, .. }),
            ..
        })) if text.contains("hello")
    ));
    let raw_output = update
        .fields
        .raw_output
        .as_ref()
        .expect("expected sanitized raw output metadata");
    assert_eq!(
        raw_output.get("stdout").and_then(serde_json::Value::as_str),
        None
    );
    assert_eq!(
        raw_output.get("stderr").and_then(serde_json::Value::as_str),
        None
    );
    assert_eq!(
        raw_output
            .get("aggregated_output")
            .and_then(serde_json::Value::as_str),
        None
    );
    assert_eq!(
        raw_output
            .get("stdout_bytes")
            .and_then(serde_json::Value::as_u64),
        Some("hello\n".len() as u64)
    );
    assert_eq!(
        raw_output
            .get("output_omitted")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );

    Ok(())
}

#[tokio::test]
async fn test_large_exec_output_is_folded_by_default() -> anyhow::Result<()> {
    let session_id = SessionId::new("test");
    let client = Arc::new(StubClient::new());
    let session_client =
        SessionClient::with_client(session_id, client.clone(), Arc::default(), Arc::default());
    let thread = Arc::new(StubCodexThread::new());
    let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
    let (message_tx, _message_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut prompt_state =
        PromptState::new("submission-id".to_string(), thread, message_tx, response_tx);
    let cwd = std::env::current_dir()?;
    let output = (0..12)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";

    prompt_state
        .handle_event(
            &session_client,
            EventMsg::ExecCommandBegin(test_fixtures::exec_command_begin(
                "call-id",
                "turn-id",
                cwd.clone().try_into()?,
                vec!["printf".to_string(), "lots".to_string()],
                vec![ParsedCommand::Unknown {
                    cmd: "printf lots".to_string(),
                }],
            )),
        )
        .await;
    prompt_state
        .handle_event(
            &session_client,
            EventMsg::ExecCommandEnd(test_fixtures::exec_command_end(
                "call-id",
                "turn-id",
                cwd.try_into()?,
                vec!["printf".to_string(), "lots".to_string()],
                output,
            )),
        )
        .await;

    let update = client
        .tool_call_updates()
        .into_iter()
        .next()
        .expect("expected completion update");

    assert_eq!(tool_call_default_open(update.meta.as_ref()), Some(false));
    assert_eq!(
        tool_call_output_reason(update.meta.as_ref()),
        Some("defaultCollapsed")
    );

    Ok(())
}

#[tokio::test]
async fn test_read_exec_completion_uses_canonical_rust_fence() -> anyhow::Result<()> {
    let session_id = SessionId::new("test");
    let client = Arc::new(StubClient::new());
    let session_client =
        SessionClient::with_client(session_id, client.clone(), Arc::default(), Arc::default());
    let thread = Arc::new(StubCodexThread::new());
    let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
    let (message_tx, _message_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut prompt_state =
        PromptState::new("submission-id".to_string(), thread, message_tx, response_tx);
    let cwd = std::env::current_dir()?;
    let read_path = PathBuf::from("src/foo.rs");

    prompt_state
        .handle_event(
            &session_client,
            EventMsg::ExecCommandBegin(test_fixtures::exec_command_begin(
                "call-id",
                "turn-id",
                cwd.clone().try_into()?,
                vec![
                    "sed".to_string(),
                    "-n".to_string(),
                    "1,120p".to_string(),
                    "src/foo.rs".to_string(),
                ],
                vec![ParsedCommand::Read {
                    cmd: "sed -n '1,120p' src/foo.rs".to_string(),
                    name: "src/foo.rs".to_string(),
                    path: read_path,
                }],
            )),
        )
        .await;
    prompt_state
        .handle_event(
            &session_client,
            EventMsg::ExecCommandEnd(test_fixtures::exec_command_end(
                "call-id",
                "turn-id",
                cwd.try_into()?,
                vec![
                    "sed".to_string(),
                    "-n".to_string(),
                    "1,120p".to_string(),
                    "src/foo.rs".to_string(),
                ],
                "fn main() {}\n",
            )),
        )
        .await;

    let update = client
        .tool_call_updates()
        .into_iter()
        .next()
        .expect("expected completion update");

    assert_eq!(update.fields.status, Some(ToolCallStatus::Completed));
    assert!(matches!(
        update.fields.content.as_ref().and_then(|content| content.first()),
        Some(ToolCallContent::Content(Content {
            content: ContentBlock::Text(TextContent { text, .. }),
            ..
        })) if text == "```rust\nfn main() {}\n```\n"
    ));

    Ok(())
}
