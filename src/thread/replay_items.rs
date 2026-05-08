use std::path::PathBuf;

use agent_client_protocol::schema::{
    Diff, ToolCall, ToolCallContent, ToolCallLocation, ToolCallStatus, ToolKind,
};
use codex_apply_patch::parse_patch;
use codex_protocol::models::ResponseItem;
use codex_shell_command::parse_command::parse_command;

use crate::permission::{ParseCommandToolCall, parse_command_tool_call};

use super::{
    actor::ThreadActor,
    deps::Auth,
    web_search::{generate_fallback_id, web_search_action_to_title_and_id},
};

#[derive(serde::Deserialize)]
struct ReplayShellArgs {
    command: Vec<String>,
    #[serde(default)]
    workdir: Option<String>,
}

#[derive(serde::Deserialize)]
struct ReplayShellCommandArgs {
    command: String,
    #[serde(default)]
    workdir: Option<String>,
}

#[derive(serde::Deserialize)]
struct ReplayExecCommandArgs {
    #[serde(default)]
    cmd: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    workdir: Option<String>,
}

fn bash_command(command: String) -> Vec<String> {
    vec!["bash".to_string(), "-lc".to_string(), command]
}

fn validate_replay_command(command: Vec<String>) -> Option<Vec<String>> {
    command
        .first()
        .filter(|program| !program.trim().is_empty())?;
    Some(command)
}

fn parse_replay_shell_script(command: String) -> Option<Vec<String>> {
    if command.trim().is_empty() {
        None
    } else {
        Some(bash_command(command))
    }
}

fn parse_replay_shell_args(name: &str, arguments: &str) -> Option<(Vec<String>, Option<String>)> {
    match name {
        "shell_command" => {
            let args: ReplayShellCommandArgs = serde_json::from_str(arguments).ok()?;
            Some((parse_replay_shell_script(args.command)?, args.workdir))
        }
        "exec_command" => {
            let args: ReplayExecCommandArgs = serde_json::from_str(arguments).ok()?;
            let command = args.cmd.or(args.command)?;
            Some((
                parse_replay_shell_script(command)?,
                args.cwd.or(args.workdir),
            ))
        }
        _ => {
            let args: ReplayShellArgs = serde_json::from_str(arguments).ok()?;
            Some((validate_replay_command(args.command)?, args.workdir))
        }
    }
}

fn is_replay_shell_tool(name: &str) -> bool {
    matches!(
        name,
        "shell" | "container.exec" | "shell_command" | "exec_command"
    )
}

impl<A: Auth> ThreadActor<A> {
    /// Parse `apply_patch` call input to extract patch content for display.
    /// Returns `(title, locations, content)` if successful.
    /// For `CustomToolCall`, the input is the patch string directly.
    fn parse_apply_patch_call(
        &self,
        input: &str,
    ) -> Option<(String, Vec<ToolCallLocation>, Vec<ToolCallContent>)> {
        // Try to parse the patch using codex-apply-patch parser
        let parsed = parse_patch(input).ok()?;

        let mut locations = Vec::new();
        let mut file_names = Vec::new();
        let mut content = Vec::new();

        for hunk in &parsed.hunks {
            match hunk {
                codex_apply_patch::Hunk::AddFile { path, contents } => {
                    let full_path = self.config.cwd.as_path().join(path);
                    file_names.push(path.display().to_string());
                    locations.push(ToolCallLocation::new(full_path.clone()));
                    // New file: no old_text, new_text is the contents
                    content.push(ToolCallContent::Diff(Diff::new(
                        full_path,
                        contents.clone(),
                    )));
                }
                codex_apply_patch::Hunk::DeleteFile { path } => {
                    let full_path = self.config.cwd.as_path().join(path);
                    file_names.push(path.display().to_string());
                    locations.push(ToolCallLocation::new(full_path.clone()));
                    // Delete file: old_text would be original content, new_text is empty
                    content.push(ToolCallContent::Diff(
                        Diff::new(full_path, "").old_text("[file deleted]"),
                    ));
                }
                codex_apply_patch::Hunk::UpdateFile {
                    path,
                    move_path,
                    chunks,
                } => {
                    let full_path = self.config.cwd.as_path().join(path);
                    let dest_path = move_path
                        .as_ref()
                        .map(|p| self.config.cwd.as_path().join(p))
                        .unwrap_or_else(|| full_path.clone());
                    file_names.push(path.display().to_string());
                    locations.push(ToolCallLocation::new(dest_path.clone()));

                    // Build old and new text from chunks
                    let old_lines: Vec<String> = chunks
                        .iter()
                        .flat_map(|c| c.old_lines.iter().cloned())
                        .collect();
                    let new_lines: Vec<String> = chunks
                        .iter()
                        .flat_map(|c| c.new_lines.iter().cloned())
                        .collect();

                    content.push(ToolCallContent::Diff(
                        Diff::new(dest_path, new_lines.join("\n")).old_text(old_lines.join("\n")),
                    ));
                }
            }
        }

        let title = if file_names.is_empty() {
            "Apply patch".to_string()
        } else {
            format!("Edit {}", file_names.join(", "))
        };

        Some((title, locations, content))
    }

    /// Parse shell function call arguments to extract command info for rich display.
    /// Returns `(title, kind, locations)` if successful.
    ///
    /// Handles both:
    /// - `shell` / `container.exec`: `command` is `Vec<String>`
    /// - `shell_command`: `command` is a `String` (shell script)
    /// - `exec_command`: `cmd` or `command` is a `String` (shell script)
    fn parse_shell_function_call(
        &self,
        name: &str,
        arguments: &str,
    ) -> Option<(String, ToolKind, Vec<ToolCallLocation>)> {
        let (command_vec, workdir) = parse_replay_shell_args(name, arguments)?;
        let cwd = workdir
            .map(PathBuf::from)
            .unwrap_or_else(|| self.config.cwd.clone().into());

        let parsed_cmd = parse_command(&command_vec);
        let ParseCommandToolCall {
            title,
            file_extension: _,
            terminal_output: _,
            locations,
            kind,
        } = parse_command_tool_call(parsed_cmd, &cwd);

        Some((title, kind, locations))
    }

    /// Convert and send a single `ResponseItem` as ACP notification(s) during replay.
    /// Only handles tool calls - messages/reasoning are handled via `EventMsg`.
    pub(super) fn replay_response_item(&self, item: &ResponseItem) {
        match item {
            ResponseItem::FunctionCall {
                name,
                arguments,
                call_id,
                ..
            } => {
                // Check if this is a shell command - parse it like we do for `LocalShellCall`.
                if is_replay_shell_tool(name)
                    && let Some((title, kind, locations)) =
                        self.parse_shell_function_call(name, arguments)
                {
                    self.client.send_tool_call(
                        ToolCall::new(call_id.clone(), title)
                            .kind(kind)
                            .status(ToolCallStatus::Completed)
                            .locations(locations)
                            .raw_input(serde_json::from_str::<serde_json::Value>(arguments).ok()),
                    );
                    return;
                }

                // Fall through to generic function call handling
                self.client.send_completed_tool_call(
                    call_id.clone(),
                    name.clone(),
                    ToolKind::Other,
                    serde_json::from_str(arguments).ok(),
                );
            }
            ResponseItem::FunctionCallOutput { call_id, output } => {
                self.client
                    .send_tool_call_completed(call_id.clone(), serde_json::to_value(output).ok());
            }
            ResponseItem::LocalShellCall {
                call_id: Some(call_id),
                action,
                status,
                ..
            } => {
                let codex_protocol::models::LocalShellAction::Exec(exec) = action;
                let cwd = exec
                    .working_directory
                    .as_ref()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| self.config.cwd.clone().into());

                // Parse the command to get rich info like the live event handler does
                let parsed_cmd = parse_command(&exec.command);
                let ParseCommandToolCall {
                    title,
                    file_extension: _,
                    terminal_output: _,
                    locations,
                    kind,
                } = parse_command_tool_call(parsed_cmd, &cwd);

                let tool_status = match status {
                    codex_protocol::models::LocalShellStatus::Completed => {
                        ToolCallStatus::Completed
                    }
                    codex_protocol::models::LocalShellStatus::InProgress
                    | codex_protocol::models::LocalShellStatus::Incomplete => {
                        ToolCallStatus::Failed
                    }
                };
                self.client.send_tool_call(
                    ToolCall::new(call_id.clone(), title)
                        .kind(kind)
                        .status(tool_status)
                        .locations(locations),
                );
            }
            ResponseItem::CustomToolCall {
                name,
                input,
                call_id,
                ..
            } => {
                // Check if this is an apply_patch call - show the patch content
                if name == "apply_patch"
                    && let Some((title, locations, content)) = self.parse_apply_patch_call(input)
                {
                    self.client.send_tool_call(
                        ToolCall::new(call_id.clone(), title)
                            .kind(ToolKind::Edit)
                            .status(ToolCallStatus::Completed)
                            .locations(locations)
                            .content(content)
                            .raw_input(serde_json::from_str::<serde_json::Value>(input).ok()),
                    );
                    return;
                }

                // Fall through to generic custom tool call handling
                self.client.send_completed_tool_call(
                    call_id.clone(),
                    name.clone(),
                    ToolKind::Other,
                    serde_json::from_str(input).ok(),
                );
            }
            ResponseItem::CustomToolCallOutput {
                name: _,
                call_id,
                output,
            } => {
                self.client
                    .send_tool_call_completed(call_id.clone(), Some(serde_json::json!(output)));
            }
            ResponseItem::WebSearchCall { id, action, .. } => {
                let (title, call_id) = action.as_ref().map_or_else(
                    || ("Web Search".into(), generate_fallback_id("web_search")),
                    |action| web_search_action_to_title_and_id(id.as_deref(), action),
                );
                self.client.send_tool_call(
                    ToolCall::new(call_id, title)
                        .kind(ToolKind::Search)
                        .status(ToolCallStatus::Completed),
                );
            }
            // Skip Message/Reasoning handled via EventMsg, plus GhostSnapshot, Compaction,
            // Other, and LocalShellCall without call_id.
            _ => {}
        }
    }
}
