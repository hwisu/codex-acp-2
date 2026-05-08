use std::path::{Path, PathBuf};

use agent_client_protocol::schema::{
    Diff, ToolCall, ToolCallContent, ToolCallId, ToolCallLocation, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields, ToolKind,
};
use codex_apply_patch::parse_patch;
use codex_protocol::models::{ResponseItem, WebSearchAction};
use codex_shell_command::parse_command::parse_command;
use uuid::Uuid;

use super::{ParseCommandToolCall, parse_command_tool_call};
use crate::boundary::{
    effect::{BridgeEffect, IgnoredCodexEventReason},
    mapper::{ReplayResponseItemRoute, ReplayToolCallStatus},
    raw,
};

pub(crate) enum ReplayResponseItemPlan<'a> {
    Effect(Box<BridgeEffect>),
    Ignore {
        item: &'a ResponseItem,
        reason: IgnoredCodexEventReason,
    },
}

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

pub(crate) fn replay_response_item_plan<'a>(
    route: ReplayResponseItemRoute<'a>,
    cwd: &Path,
) -> ReplayResponseItemPlan<'a> {
    match route {
        ReplayResponseItemRoute::ShellFunctionCall {
            name,
            arguments,
            call_id,
        } => replay_shell_function_call_tool_call(call_id, name, arguments, cwd).map_or_else(
            || {
                send_tool_call(completed_tool_call(
                    call_id.to_string(),
                    name.to_string(),
                    ToolKind::Other,
                    serde_json::from_str(arguments).ok(),
                ))
            },
            send_tool_call,
        ),
        ReplayResponseItemRoute::GenericFunctionCall {
            call_id,
            name,
            arguments,
        } => send_tool_call(completed_tool_call(
            call_id.to_string(),
            name.to_string(),
            ToolKind::Other,
            serde_json::from_str(arguments).ok(),
        )),
        ReplayResponseItemRoute::FunctionCallOutput { call_id, output } => {
            send_tool_call_update(completed_tool_call_update(
                call_id.to_string(),
                raw::response_item_function_call_output(output),
            ))
        }
        ReplayResponseItemRoute::LocalShellCall {
            call_id,
            command,
            working_directory,
            status,
        } => send_tool_call(replay_local_shell_call_tool_call(
            call_id,
            command,
            working_directory,
            status,
            cwd,
        )),
        ReplayResponseItemRoute::ApplyPatchCustomToolCall { input, call_id } => {
            replay_apply_patch_tool_call(call_id, input, cwd).map_or_else(
                || {
                    send_tool_call(completed_tool_call(
                        call_id.to_string(),
                        "apply_patch".to_string(),
                        ToolKind::Other,
                        serde_json::from_str(input).ok(),
                    ))
                },
                send_tool_call,
            )
        }
        ReplayResponseItemRoute::GenericCustomToolCall {
            call_id,
            name,
            input,
        } => send_tool_call(completed_tool_call(
            call_id.to_string(),
            name.to_string(),
            ToolKind::Other,
            serde_json::from_str(input).ok(),
        )),
        ReplayResponseItemRoute::CustomToolCallOutput { call_id, output } => {
            send_tool_call_update(completed_tool_call_update(
                call_id.to_string(),
                Some(raw::response_item_custom_tool_call_output(output)),
            ))
        }
        ReplayResponseItemRoute::WebSearchCall { id, action } => {
            send_tool_call(replay_web_search_tool_call(id, action))
        }
        ReplayResponseItemRoute::Ignore { item, reason } => {
            ReplayResponseItemPlan::Ignore { item, reason }
        }
    }
}

fn send_tool_call<'a>(tool_call: ToolCall) -> ReplayResponseItemPlan<'a> {
    ReplayResponseItemPlan::Effect(Box::new(BridgeEffect::tool_call(tool_call)))
}

fn send_tool_call_update<'a>(update: ToolCallUpdate) -> ReplayResponseItemPlan<'a> {
    ReplayResponseItemPlan::Effect(Box::new(BridgeEffect::tool_call_update(update)))
}

fn completed_tool_call(
    call_id: impl Into<ToolCallId>,
    title: impl Into<String>,
    kind: ToolKind,
    raw_input: Option<serde_json::Value>,
) -> ToolCall {
    let mut tool_call = ToolCall::new(call_id, title)
        .kind(kind)
        .status(ToolCallStatus::Completed);
    if let Some(input) = raw_input {
        tool_call = tool_call.raw_input(input);
    }
    tool_call
}

fn completed_tool_call_update(
    call_id: impl Into<ToolCallId>,
    raw_output: Option<serde_json::Value>,
) -> ToolCallUpdate {
    let mut fields = ToolCallUpdateFields::new().status(ToolCallStatus::Completed);
    if let Some(output) = raw_output {
        fields = fields.raw_output(output);
    }
    ToolCallUpdate::new(call_id, fields)
}

fn replay_shell_function_call_tool_call(
    call_id: &str,
    name: &str,
    arguments: &str,
    cwd: &Path,
) -> Option<ToolCall> {
    let (command_vec, workdir) = parse_replay_shell_args(name, arguments)?;
    let command_cwd = workdir
        .map(PathBuf::from)
        .unwrap_or_else(|| cwd.to_path_buf());
    let parsed_cmd = parse_command(&command_vec);
    let ParseCommandToolCall {
        title,
        file_extension: _,
        terminal_output: _,
        locations,
        kind,
    } = parse_command_tool_call(parsed_cmd, &command_cwd);

    Some(
        ToolCall::new(call_id.to_string(), title)
            .kind(kind)
            .status(ToolCallStatus::Completed)
            .locations(locations)
            .raw_input(serde_json::from_str::<serde_json::Value>(arguments).ok()),
    )
}

fn replay_local_shell_call_tool_call(
    call_id: &str,
    command: &[String],
    working_directory: Option<&str>,
    status: ReplayToolCallStatus,
    cwd: &Path,
) -> ToolCall {
    let command_cwd = working_directory
        .map(PathBuf::from)
        .unwrap_or_else(|| cwd.to_path_buf());
    let parsed_cmd = parse_command(command);
    let ParseCommandToolCall {
        title,
        file_extension: _,
        terminal_output: _,
        locations,
        kind,
    } = parse_command_tool_call(parsed_cmd, &command_cwd);
    let tool_status = match status {
        ReplayToolCallStatus::Completed => ToolCallStatus::Completed,
        ReplayToolCallStatus::Failed => ToolCallStatus::Failed,
    };

    ToolCall::new(call_id.to_string(), title)
        .kind(kind)
        .status(tool_status)
        .locations(locations)
}

fn replay_apply_patch_tool_call(call_id: &str, input: &str, cwd: &Path) -> Option<ToolCall> {
    let (title, locations, content) = parse_replay_apply_patch_call(input, cwd)?;
    Some(
        ToolCall::new(call_id.to_string(), title)
            .kind(ToolKind::Edit)
            .status(ToolCallStatus::Completed)
            .locations(locations)
            .content(content)
            .raw_input(serde_json::from_str::<serde_json::Value>(input).ok()),
    )
}

fn replay_web_search_tool_call(id: Option<&str>, action: Option<&WebSearchAction>) -> ToolCall {
    let (title, call_id) = action.map_or_else(
        || ("Web Search".into(), generate_fallback_id("web_search")),
        |action| web_search_action_to_title_and_id(id, action),
    );
    ToolCall::new(call_id, title)
        .kind(ToolKind::Search)
        .status(ToolCallStatus::Completed)
}

fn parse_replay_apply_patch_call(
    input: &str,
    cwd: &Path,
) -> Option<(String, Vec<ToolCallLocation>, Vec<ToolCallContent>)> {
    let parsed = parse_patch(input).ok()?;
    let mut locations = Vec::new();
    let mut file_names = Vec::new();
    let mut content = Vec::new();

    for hunk in &parsed.hunks {
        match hunk {
            codex_apply_patch::Hunk::AddFile { path, contents } => {
                let full_path = cwd.join(path);
                file_names.push(path.display().to_string());
                locations.push(ToolCallLocation::new(full_path.clone()));
                content.push(ToolCallContent::Diff(Diff::new(
                    full_path,
                    contents.clone(),
                )));
            }
            codex_apply_patch::Hunk::DeleteFile { path } => {
                let full_path = cwd.join(path);
                file_names.push(path.display().to_string());
                locations.push(ToolCallLocation::new(full_path.clone()));
                content.push(ToolCallContent::Diff(
                    Diff::new(full_path, "").old_text("[file deleted]"),
                ));
            }
            codex_apply_patch::Hunk::UpdateFile {
                path,
                move_path,
                chunks,
            } => {
                let full_path = cwd.join(path);
                let dest_path = move_path
                    .as_ref()
                    .map(|path| cwd.join(path))
                    .unwrap_or_else(|| full_path.clone());
                file_names.push(path.display().to_string());
                locations.push(ToolCallLocation::new(dest_path.clone()));

                let old_lines = chunks
                    .iter()
                    .flat_map(|chunk| chunk.old_lines.iter().cloned())
                    .collect::<Vec<_>>();
                let new_lines = chunks
                    .iter()
                    .flat_map(|chunk| chunk.new_lines.iter().cloned())
                    .collect::<Vec<_>>();

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

fn web_search_action_to_title_and_id(
    id: Option<&str>,
    action: &WebSearchAction,
) -> (String, String) {
    match action {
        WebSearchAction::Search { query, queries } => {
            let title = queries
                .as_ref()
                .map(|query| query.join(", "))
                .or_else(|| query.clone())
                .unwrap_or_else(|| "Web search".to_string());
            let call_id = id
                .map(str::to_string)
                .unwrap_or_else(|| generate_fallback_id("web_search"));
            (title, call_id)
        }
        WebSearchAction::OpenPage { url } => {
            let title = url.clone().unwrap_or_else(|| "Open page".to_string());
            let call_id = id
                .map(str::to_string)
                .unwrap_or_else(|| generate_fallback_id("web_open"));
            (title, call_id)
        }
        WebSearchAction::FindInPage { pattern, .. } => {
            let title = pattern
                .clone()
                .unwrap_or_else(|| "Find in page".to_string());
            let call_id = id
                .map(str::to_string)
                .unwrap_or_else(|| generate_fallback_id("web_find"));
            (title, call_id)
        }
        WebSearchAction::Other => ("Unknown".to_string(), generate_fallback_id("web_search")),
    }
}

fn generate_fallback_id(prefix: &str) -> String {
    format!("{}_{}", prefix, Uuid::new_v4())
}
