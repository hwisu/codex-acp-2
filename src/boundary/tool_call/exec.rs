use std::path::{Path, PathBuf};

use agent_client_protocol::schema::v1::{
    Meta, Terminal, ToolCall, ToolCallContent, ToolCallId, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields, ToolKind,
};
use codex_protocol::protocol::{ExecCommandBeginEvent, ExecCommandEndEvent, ExecCommandStatus};

use super::{ParseCommandToolCall, parse_command_tool_call};
use crate::{
    boundary::{
        compat, effect::BridgeEffect,
        file_changes::extract_tool_call_content_from_command_output_diff, raw,
    },
    display::tool_call_text_content,
};

pub(crate) struct ActiveCommand {
    pub(crate) tool_call_id: ToolCallId,
    pub(crate) title: String,
    pub(crate) kind: ToolKind,
    pub(crate) terminal_output: bool,
    pub(crate) output: String,
    pub(crate) file_extension: Option<String>,
    pub(crate) cwd: PathBuf,
}

pub(crate) struct ExecCommandBeginPlan {
    pub(crate) call_id: String,
    pub(crate) active_command: ActiveCommand,
    pub(crate) tool_call: ToolCall,
}

pub(crate) struct ExecCommandBeginEffectPlan {
    pub(crate) call_id: String,
    pub(crate) active_command: ActiveCommand,
    pub(crate) effect: BridgeEffect,
}

pub(crate) struct ExecCommandEndPlan {
    pub(crate) streaming_update: Option<ToolCallUpdate>,
    pub(crate) completion_update: ToolCallUpdate,
}

pub(crate) struct ExecCommandEndEffectPlan {
    pub(crate) streaming_update: Option<BridgeEffect>,
    pub(crate) completion_update: BridgeEffect,
}

impl ActiveCommand {
    pub(crate) fn render_streaming_update(
        &self,
        terminal_id: &str,
        supports_terminal_output: bool,
        data: &str,
    ) -> ToolCallUpdate {
        let fields = if supports_terminal_output {
            ToolCallUpdateFields::new()
        } else {
            // Fallback clients only render content snapshots, so replace the content with the
            // cumulative output on every chunk.
            ToolCallUpdateFields::new()
                .kind(self.kind)
                .status(ToolCallStatus::InProgress)
                .title(self.title.clone())
                .content(self.render_output_content(&self.output))
        };

        let update = ToolCallUpdate::new(self.tool_call_id.clone(), fields);
        if supports_terminal_output {
            update.meta(compat::terminal_output_delta_meta(terminal_id, data))
        } else {
            update
        }
    }

    pub(crate) fn render_streaming_effect(
        &self,
        terminal_id: &str,
        supports_terminal_output: bool,
        data: &str,
    ) -> BridgeEffect {
        BridgeEffect::tool_call_update(self.render_streaming_update(
            terminal_id,
            supports_terminal_output,
            data,
        ))
    }

    pub(crate) fn render_output_content(&self, output: &str) -> Vec<ToolCallContent> {
        if let Some(content) = extract_tool_call_content_from_command_output_diff(&self.cwd, output)
        {
            return content;
        }

        let content = match self.file_extension.as_deref() {
            Some(ext) => format!(
                "```{}\n{}\n```\n",
                canonical_fence_language(ext),
                output.trim_end_matches('\n')
            ),
            None => format!("```sh\n{}\n```\n", output.trim_end_matches('\n')),
        };
        vec![content.into()]
    }

    fn render_pending_content(&self) -> Vec<ToolCallContent> {
        vec!["Waiting for command output...".into()]
    }

    fn render_empty_completion_content(
        &self,
        exit_code: i32,
        status: ToolCallStatus,
    ) -> Vec<ToolCallContent> {
        let message = match status {
            ToolCallStatus::Completed => "Command completed with no output.".to_string(),
            ToolCallStatus::Failed if exit_code != 0 => {
                format!("Command exited with code {exit_code} and produced no output.")
            }
            _ => "Command finished with no output.".to_string(),
        };
        vec![tool_call_text_content(message)]
    }

    fn render_initial_content(
        &self,
        terminal_id: &str,
        cwd: &Path,
        supports_terminal_output: bool,
    ) -> (Vec<ToolCallContent>, Option<Meta>) {
        let (default_open, reason) = self.initial_output_display();
        if supports_terminal_output {
            let content = vec![ToolCallContent::Terminal(Terminal::new(
                terminal_id.to_owned(),
            ))];
            (
                content,
                Some(compat::terminal_info_meta(
                    terminal_id,
                    cwd,
                    default_open,
                    reason,
                )),
            )
        } else {
            (
                self.render_pending_content(),
                Some(compat::tool_call_output_display_meta(default_open, reason)),
            )
        }
    }

    fn render_completion_content(
        &self,
        terminal_id: &str,
        supports_terminal_output: bool,
        output_snapshot: &str,
        exit_code: i32,
        status: ToolCallStatus,
    ) -> Option<Vec<ToolCallContent>> {
        if output_snapshot.is_empty() {
            return Some(self.render_empty_completion_content(exit_code, status));
        }

        let output_content = self.render_output_content(output_snapshot);
        if supports_terminal_output {
            if output_content
                .iter()
                .any(|content| matches!(content, ToolCallContent::Diff(_)))
            {
                return Some(output_content);
            }

            Some(vec![ToolCallContent::Terminal(Terminal::new(
                terminal_id.to_owned(),
            ))])
        } else {
            Some(output_content)
        }
    }

    fn completion_terminal_output_delta(&self, output_snapshot: &str) -> Option<String> {
        if output_snapshot.is_empty() {
            return None;
        }

        if self.output.is_empty() {
            return Some(output_snapshot.to_string());
        }

        if let Some(suffix) = output_snapshot.strip_prefix(&self.output) {
            return (!suffix.is_empty()).then(|| suffix.to_string());
        }

        (output_snapshot != self.output).then(|| output_snapshot.to_string())
    }

    fn render_completion_meta(
        &self,
        terminal_id: &str,
        supports_terminal_output: bool,
        output_snapshot: &str,
        status: ToolCallStatus,
        exit_code: i32,
    ) -> Meta {
        let (default_open, reason) = self.completion_output_display(output_snapshot, status);
        let mut meta = if supports_terminal_output {
            compat::terminal_exit_meta(terminal_id, exit_code)
        } else {
            Meta::new()
        };
        compat::insert_tool_call_output_display_meta(&mut meta, default_open, reason);
        meta
    }

    fn initial_output_display(&self) -> (bool, &'static str) {
        if is_direct_answer_command(&self.title) {
            (true, "directAnswerCommand")
        } else {
            (false, "defaultCollapsed")
        }
    }

    fn completion_output_display(
        &self,
        output_snapshot: &str,
        status: ToolCallStatus,
    ) -> (bool, &'static str) {
        if status == ToolCallStatus::Failed {
            return (true, "failedCommand");
        }
        if is_direct_answer_command(&self.title) {
            return (true, "directAnswerCommand");
        }
        if output_snapshot.is_empty() {
            return (true, "emptyOutput");
        }
        if is_short_output(output_snapshot) {
            return (true, "shortOutput");
        }

        (false, "defaultCollapsed")
    }
}

pub(crate) fn exec_command_begin_plan(
    event: ExecCommandBeginEvent,
    supports_terminal_output: impl FnOnce(&ActiveCommand) -> bool,
) -> ExecCommandBeginPlan {
    let raw_input = raw::exec_command_begin(&event);
    let ExecCommandBeginEvent {
        turn_id: _,
        source: _,
        interaction_input: _,
        call_id,
        command: _,
        cwd,
        parsed_cmd,
        process_id: _,
        ..
    } = event;
    let cwd_path = cwd.to_path_buf();
    let tool_call_id = ToolCallId::new(call_id.clone());
    let ParseCommandToolCall {
        title,
        file_extension,
        locations,
        terminal_output,
        kind,
    } = parse_command_tool_call(parsed_cmd, &cwd_path);

    let active_command = ActiveCommand {
        tool_call_id: tool_call_id.clone(),
        title: title.clone(),
        kind,
        output: String::new(),
        file_extension,
        cwd: cwd_path.clone(),
        terminal_output,
    };
    let supports_terminal_output = supports_terminal_output(&active_command);
    let (content, meta) =
        active_command.render_initial_content(&call_id, &cwd_path, supports_terminal_output);
    let tool_call = ToolCall::new(tool_call_id, title)
        .kind(kind)
        .status(ToolCallStatus::InProgress)
        .locations(locations)
        .raw_input(raw_input)
        .content(content)
        .meta(meta);

    ExecCommandBeginPlan {
        call_id,
        active_command,
        tool_call,
    }
}

pub(crate) fn exec_command_begin_effect_plan(
    event: ExecCommandBeginEvent,
    supports_terminal_output: impl FnOnce(&ActiveCommand) -> bool,
) -> ExecCommandBeginEffectPlan {
    let plan = exec_command_begin_plan(event, supports_terminal_output);
    ExecCommandBeginEffectPlan {
        call_id: plan.call_id,
        active_command: plan.active_command,
        effect: BridgeEffect::tool_call(plan.tool_call),
    }
}

pub(crate) fn exec_command_end_plan(
    event: ExecCommandEndEvent,
    active_command: &ActiveCommand,
    supports_terminal_output: bool,
) -> ExecCommandEndPlan {
    let raw_output = raw::exec_command_end(&event);
    let ExecCommandEndEvent {
        turn_id: _,
        command: _,
        cwd: _,
        parsed_cmd: _,
        source: _,
        interaction_input: _,
        call_id,
        exit_code,
        stdout: _,
        stderr: _,
        aggregated_output,
        duration: _,
        formatted_output,
        process_id: _,
        status,
        ..
    } = event;

    let status = exec_completion_status(status, exit_code);
    let output_snapshot =
        exec_completion_output_snapshot(formatted_output, active_command, aggregated_output);
    let streaming_update = supports_terminal_output
        .then(|| active_command.completion_terminal_output_delta(&output_snapshot))
        .flatten()
        .map(|data| {
            active_command.render_streaming_update(&call_id, supports_terminal_output, &data)
        });
    let content = active_command.render_completion_content(
        &call_id,
        supports_terminal_output,
        &output_snapshot,
        exit_code,
        status,
    );
    let completion_update = ToolCallUpdate::new(
        active_command.tool_call_id.clone(),
        ToolCallUpdateFields::new()
            .kind(active_command.kind)
            .status(status)
            .title(active_command.title.clone())
            .content(content)
            .raw_output(raw_output),
    )
    .meta(active_command.render_completion_meta(
        &call_id,
        supports_terminal_output,
        &output_snapshot,
        status,
        exit_code,
    ));

    ExecCommandEndPlan {
        streaming_update,
        completion_update,
    }
}

pub(crate) fn exec_command_end_effect_plan(
    event: ExecCommandEndEvent,
    active_command: &ActiveCommand,
    supports_terminal_output: bool,
) -> ExecCommandEndEffectPlan {
    let plan = exec_command_end_plan(event, active_command, supports_terminal_output);
    ExecCommandEndEffectPlan {
        streaming_update: plan.streaming_update.map(BridgeEffect::tool_call_update),
        completion_update: BridgeEffect::tool_call_update(plan.completion_update),
    }
}

fn exec_completion_status(status: ExecCommandStatus, exit_code: i32) -> ToolCallStatus {
    match status {
        ExecCommandStatus::Completed => ToolCallStatus::Completed,
        _ if exit_code == 0 => ToolCallStatus::Completed,
        ExecCommandStatus::Failed | ExecCommandStatus::Declined => ToolCallStatus::Failed,
    }
}

fn exec_completion_output_snapshot(
    formatted_output: String,
    active_command: &ActiveCommand,
    aggregated_output: String,
) -> String {
    if !formatted_output.is_empty() {
        formatted_output
    } else if !active_command.output.is_empty() {
        active_command.output.clone()
    } else {
        aggregated_output
    }
}

fn canonical_fence_language(extension: &str) -> &str {
    match extension {
        "rs" => "rust",
        "js" => "javascript",
        "ts" => "typescript",
        "py" => "python",
        "sh" | "zsh" => "bash",
        "yml" => "yaml",
        "md" => "markdown",
        ext => ext,
    }
}

fn is_short_output(output: &str) -> bool {
    output.chars().count() <= 800 && output.lines().count() <= 8
}

fn is_direct_answer_command(command: &str) -> bool {
    let argv = shlex::split(command)
        .unwrap_or_else(|| command.split_whitespace().map(ToOwned::to_owned).collect());
    let Some(program) = argv.first().and_then(|program| {
        Path::new(program)
            .file_name()
            .and_then(|name| name.to_str())
    }) else {
        return false;
    };

    matches!(program, "date" | "pwd" | "whoami")
}
