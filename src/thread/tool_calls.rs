use std::path::Path;

use agent_client_protocol::schema::{
    Meta, Terminal, ToolCallContent, ToolCallId, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields, ToolKind,
};

use crate::display::{
    insert_tool_call_output_display_meta, tool_call_output_display_meta, tool_call_text_content,
};

pub(super) struct ActiveCommand {
    pub(super) tool_call_id: ToolCallId,
    pub(super) title: String,
    pub(super) kind: ToolKind,
    pub(super) terminal_output: bool,
    pub(super) output: String,
    pub(super) file_extension: Option<String>,
}

impl ActiveCommand {
    pub(super) fn render_pending_content(&self) -> Vec<ToolCallContent> {
        vec!["Waiting for command output...".into()]
    }

    pub(super) fn render_empty_completion_content(
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

    pub(super) fn render_output_content(&self, output: &str) -> Vec<ToolCallContent> {
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

    pub(super) fn render_initial_content(
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
            let mut meta = Meta::from_iter([(
                "terminal_info".to_owned(),
                serde_json::json!({
                    "terminal_id": terminal_id,
                    "cwd": cwd
                }),
            )]);
            insert_tool_call_output_display_meta(&mut meta, default_open, reason);
            (content, Some(meta))
        } else {
            (
                self.render_pending_content(),
                Some(tool_call_output_display_meta(default_open, reason)),
            )
        }
    }

    pub(super) fn render_streaming_update(
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
            update.meta(Meta::from_iter([(
                "terminal_output".to_owned(),
                serde_json::json!({
                    "terminal_id": terminal_id,
                    "data": data
                }),
            )]))
        } else {
            update
        }
    }

    pub(super) fn render_completion_content(
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

        if supports_terminal_output {
            let mut content = vec![ToolCallContent::Terminal(Terminal::new(
                terminal_id.to_owned(),
            ))];
            content.extend(self.render_output_content(output_snapshot));
            Some(content)
        } else {
            Some(self.render_output_content(output_snapshot))
        }
    }

    pub(super) fn completion_terminal_output_delta(&self, output_snapshot: &str) -> Option<String> {
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

    pub(super) fn render_terminal_exit_meta(terminal_id: &str, exit_code: i32) -> Meta {
        Meta::from_iter([(
            "terminal_exit".into(),
            serde_json::json!({
                "terminal_id": terminal_id,
                "exit_code": exit_code,
                "signal": null
            }),
        )])
    }

    pub(super) fn render_completion_meta(
        &self,
        terminal_id: &str,
        supports_terminal_output: bool,
        output_snapshot: &str,
        status: ToolCallStatus,
        exit_code: i32,
    ) -> Meta {
        let (default_open, reason) = self.completion_output_display(output_snapshot, status);
        let mut meta = if supports_terminal_output {
            Self::render_terminal_exit_meta(terminal_id, exit_code)
        } else {
            Meta::new()
        };
        insert_tool_call_output_display_meta(&mut meta, default_open, reason);
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
