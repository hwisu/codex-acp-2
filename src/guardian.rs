use agent_client_protocol::schema::{
    Content, ContentBlock, TextContent, ToolCallContent, ToolCallStatus,
};
use codex_protocol::approvals::{GuardianAssessmentAction, GuardianCommandSource};
use codex_protocol::protocol::GuardianAssessmentEvent;
use std::path::Path;

pub(crate) fn guardian_assessment_tool_call_id(id: &str) -> String {
    format!("guardian_assessment:{id}")
}

pub(crate) fn guardian_assessment_tool_call_status(
    status: &codex_protocol::protocol::GuardianAssessmentStatus,
) -> ToolCallStatus {
    match status {
        codex_protocol::protocol::GuardianAssessmentStatus::InProgress => {
            ToolCallStatus::InProgress
        }
        codex_protocol::protocol::GuardianAssessmentStatus::Approved => ToolCallStatus::Completed,
        codex_protocol::protocol::GuardianAssessmentStatus::Denied
        | codex_protocol::protocol::GuardianAssessmentStatus::Aborted
        | codex_protocol::protocol::GuardianAssessmentStatus::TimedOut => ToolCallStatus::Failed,
    }
}

pub(crate) fn guardian_assessment_content(event: &GuardianAssessmentEvent) -> Vec<ToolCallContent> {
    let mut lines = vec![format!(
        "Status: {}",
        match event.status {
            codex_protocol::protocol::GuardianAssessmentStatus::InProgress => "In progress",
            codex_protocol::protocol::GuardianAssessmentStatus::Approved => "Approved",
            codex_protocol::protocol::GuardianAssessmentStatus::Denied => "Denied",
            codex_protocol::protocol::GuardianAssessmentStatus::Aborted => "Aborted",
            codex_protocol::protocol::GuardianAssessmentStatus::TimedOut => "Timed out",
        }
    )];

    lines.push(format!(
        "Action: {}",
        guardian_action_summary(&event.action)
    ));

    if let Some(level) = event.risk_level {
        lines.push(format!("Risk: {}", format!("{level:?}").to_lowercase()));
    }

    if let Some(rationale) = event.rationale.as_ref()
        && !rationale.trim().is_empty()
    {
        lines.push(format!("Rationale: {rationale}"));
    }

    vec![ToolCallContent::Content(Content::new(ContentBlock::Text(
        TextContent::new(lines.join("\n")),
    )))]
}

pub(crate) fn guardian_action_summary(action: &GuardianAssessmentAction) -> String {
    match action {
        GuardianAssessmentAction::Command {
            source,
            command,
            cwd: _,
        } => {
            let label = guardian_command_source_label(source);
            format!("{label} {command}")
        }
        GuardianAssessmentAction::Execve {
            source,
            program,
            argv,
            cwd: _,
        } => {
            let label = guardian_command_source_label(source);
            let command: Vec<&str> = if argv.is_empty() {
                vec![program.as_str()]
            } else {
                argv.iter().map(String::as_str).collect()
            };
            let joined = shlex::try_join(command.iter().copied())
                .ok()
                .unwrap_or_else(|| command.join(" "));
            format!("{label} {joined}")
        }
        GuardianAssessmentAction::ApplyPatch { files, cwd: _ } => {
            if files.len() == 1 {
                format!("apply_patch touching {}", files[0].display())
            } else {
                format!("apply_patch touching {} files", files.len())
            }
        }
        GuardianAssessmentAction::NetworkAccess { target, host, .. } => {
            let label = if target.is_empty() { host } else { target };
            format!("network access to {label}")
        }
        GuardianAssessmentAction::McpToolCall {
            server,
            tool_name,
            connector_name,
            ..
        } => {
            let label = connector_name.as_deref().unwrap_or(server.as_str());
            format!("MCP {tool_name} on {label}")
        }
        GuardianAssessmentAction::RequestPermissions { reason, .. } => reason
            .clone()
            .unwrap_or_else(|| "request additional permissions".to_string()),
    }
}

pub(crate) fn guardian_command_source_label(source: &GuardianCommandSource) -> &'static str {
    match source {
        GuardianCommandSource::Shell => "shell",
        GuardianCommandSource::UnifiedExec => "exec",
    }
}

pub(crate) fn format_file_system_entries<'a>(
    entries: impl Iterator<Item = &'a codex_protocol::permissions::FileSystemSandboxEntry>,
) -> String {
    entries
        .map(format_file_system_entry)
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn format_file_system_entry(
    entry: &codex_protocol::permissions::FileSystemSandboxEntry,
) -> String {
    match &entry.path {
        codex_protocol::permissions::FileSystemPath::Path { path } => path.display().to_string(),
        codex_protocol::permissions::FileSystemPath::GlobPattern { pattern } => {
            format!("glob `{pattern}`")
        }
        codex_protocol::permissions::FileSystemPath::Special { value } => {
            format_file_system_special(value)
        }
    }
}

pub(crate) fn format_file_system_special(
    value: &codex_protocol::permissions::FileSystemSpecialPath,
) -> String {
    match value {
        codex_protocol::permissions::FileSystemSpecialPath::Root => ":root".to_string(),
        codex_protocol::permissions::FileSystemSpecialPath::Minimal => ":minimal".to_string(),
        codex_protocol::permissions::FileSystemSpecialPath::ProjectRoots { subpath } => {
            format_file_system_subpath(":project_roots", subpath.as_deref())
        }
        codex_protocol::permissions::FileSystemSpecialPath::Tmpdir => ":tmpdir".to_string(),
        codex_protocol::permissions::FileSystemSpecialPath::SlashTmp => "/tmp".to_string(),
        codex_protocol::permissions::FileSystemSpecialPath::Unknown { path, subpath } => {
            format_file_system_subpath(path, subpath.as_deref())
        }
    }
}

pub(crate) fn format_file_system_subpath(base: &str, subpath: Option<&Path>) -> String {
    match subpath {
        Some(subpath) => format!("{base}/{}", subpath.display()),
        None => base.to_string(),
    }
}
