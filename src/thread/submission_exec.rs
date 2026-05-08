use agent_client_protocol::{
    Error,
    schema::{
        ToolCall, ToolCallContent, ToolCallId, ToolCallLocation, ToolCallStatus, ToolCallUpdate,
        ToolCallUpdateFields,
    },
};
use codex_protocol::protocol::{
    ExecApprovalRequestEvent, ExecCommandBeginEvent, ExecCommandEndEvent,
    ExecCommandOutputDeltaEvent, ExecCommandStatus, NetworkApprovalContext, ReviewDecision,
    TerminalInteractionEvent,
};
use itertools::Itertools;

use crate::permission::{
    ParseCommandToolCall, build_exec_permission_options, parse_command_tool_call,
};

use super::{
    approvals::{PendingPermissionRequest, exec_request_key},
    client::SessionClient,
    submission::{PermissionInteractionRequest, PromptState},
    tool_calls::ActiveCommand,
};

impl PromptState {
    pub(super) fn exec_approval(
        &mut self,
        client: &SessionClient,
        event: ExecApprovalRequestEvent,
    ) -> Result<(), Error> {
        let available_decisions = event.effective_available_decisions();
        let raw_input = serde_json::json!(&event);
        let content = exec_approval_content(&event, &available_decisions)?;
        let ExecApprovalRequestEvent {
            call_id,
            command: _,
            turn_id,
            cwd,
            reason: _,
            parsed_cmd,
            proposed_execpolicy_amendment: _,
            approval_id,
            network_approval_context,
            additional_permissions,
            available_decisions: _,
            proposed_network_policy_amendments: _,
        } = event;

        // Create a new tool call for the command execution
        let tool_call_id = ToolCallId::new(call_id.clone());
        let ParseCommandToolCall {
            title,
            terminal_output,
            file_extension,
            locations,
            kind,
        } = parse_command_tool_call(parsed_cmd, &cwd);
        self.insert_active_command(
            call_id.clone(),
            ActiveCommand {
                title: title.clone(),
                kind,
                terminal_output,
                tool_call_id: tool_call_id.clone(),
                output: String::new(),
                file_extension,
            },
        );

        let permission_options = build_exec_permission_options(
            &available_decisions,
            network_approval_context.as_ref(),
            additional_permissions.as_ref(),
        );

        let option_map = permission_options
            .iter()
            .map(|option| (option.option_id.to_string(), option.decision.clone()))
            .collect();

        self.spawn_permission_request(
            client,
            PermissionInteractionRequest {
                request_key: exec_request_key(&call_id),
                pending_request: PendingPermissionRequest::Exec {
                    approval_id: approval_id.unwrap_or_else(|| call_id.clone()),
                    turn_id,
                    option_map,
                },
                tool_call: ToolCallUpdate::new(
                    tool_call_id,
                    ToolCallUpdateFields::new()
                        .kind(kind)
                        .status(ToolCallStatus::Pending)
                        .title(title)
                        .raw_input(raw_input)
                        .content(content)
                        .locations(non_empty_locations(locations)),
                ),
                options: permission_options
                    .into_iter()
                    .map(|option| option.permission_option)
                    .collect(),
            },
        );

        Ok(())
    }

    pub(super) fn exec_command_begin(
        &mut self,
        client: &SessionClient,
        event: ExecCommandBeginEvent,
    ) {
        let raw_input = serde_json::json!(&event);
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
        // Create a new tool call for the command execution
        let tool_call_id = ToolCallId::new(call_id.clone());
        let ParseCommandToolCall {
            title,
            file_extension,
            locations,
            terminal_output,
            kind,
        } = parse_command_tool_call(parsed_cmd, &cwd);

        let active_command = ActiveCommand {
            tool_call_id: tool_call_id.clone(),
            title: title.clone(),
            kind,
            output: String::new(),
            file_extension,
            terminal_output,
        };
        let supports_terminal_output = client.supports_terminal_output(&active_command);
        let (content, meta) =
            active_command.render_initial_content(&call_id, &cwd, supports_terminal_output);

        self.insert_active_command(call_id.clone(), active_command);

        client.send_tool_call(
            ToolCall::new(tool_call_id, title)
                .kind(kind)
                .status(ToolCallStatus::InProgress)
                .locations(locations)
                .raw_input(raw_input)
                .content(content)
                .meta(meta),
        );
    }

    pub(super) fn exec_command_output_delta(
        &mut self,
        client: &SessionClient,
        event: ExecCommandOutputDeltaEvent,
    ) {
        let ExecCommandOutputDeltaEvent {
            call_id,
            chunk,
            stream: _,
        } = event;
        let data_str = String::from_utf8_lossy(&chunk).to_string();
        self.stream_active_command_output(client, &call_id, &data_str);
    }

    pub(super) fn exec_command_end(&mut self, client: &SessionClient, event: ExecCommandEndEvent) {
        let raw_output = exec_command_end_raw_output(&event);
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
        if let Some(active_command) = self.remove_active_command(&call_id) {
            let status = exec_completion_status(status, exit_code);
            let supports_terminal_output = client.supports_terminal_output(&active_command);
            let output_snapshot = exec_completion_output_snapshot(
                formatted_output,
                &active_command,
                aggregated_output,
            );
            if supports_terminal_output
                && let Some(data) =
                    active_command.completion_terminal_output_delta(&output_snapshot)
            {
                let update = active_command.render_streaming_update(
                    &call_id,
                    supports_terminal_output,
                    &data,
                );
                client.send_tool_call_update(update);
            }
            let content = active_command.render_completion_content(
                &call_id,
                supports_terminal_output,
                &output_snapshot,
                exit_code,
                status,
            );

            client.send_tool_call_update(
                ToolCallUpdate::new(
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
                )),
            );
        }
    }

    pub(super) fn terminal_interaction(
        &mut self,
        client: &SessionClient,
        event: TerminalInteractionEvent,
    ) {
        let TerminalInteractionEvent {
            call_id,
            process_id: _,
            stdin,
        } = event;

        let stdin = format!("\n{stdin}\n");
        self.stream_active_command_output(client, &call_id, &stdin);
    }
}

fn exec_command_end_raw_output(event: &ExecCommandEndEvent) -> serde_json::Value {
    serde_json::json!({
        "call_id": &event.call_id,
        "process_id": &event.process_id,
        "turn_id": &event.turn_id,
        "command": &event.command,
        "cwd": &event.cwd,
        "parsed_cmd": &event.parsed_cmd,
        "source": &event.source,
        "interaction_input": &event.interaction_input,
        "exit_code": event.exit_code,
        "duration": event.duration,
        "status": &event.status,
        "stdout_bytes": event.stdout.len(),
        "stderr_bytes": event.stderr.len(),
        "aggregated_output_bytes": event.aggregated_output.len(),
        "formatted_output_bytes": event.formatted_output.len(),
        "output_omitted": true,
    })
}

fn exec_approval_content(
    event: &ExecApprovalRequestEvent,
    available_decisions: &[ReviewDecision],
) -> Result<Option<Vec<ToolCallContent>>, Error> {
    let mut content = vec![];

    if let Some(reason) = event.reason.as_ref() {
        content.push(reason.clone());
    }
    if let Some(amendment) = event.proposed_execpolicy_amendment.as_ref() {
        content.push(format!(
            "Proposed Amendment: {}",
            amendment.command().join("\n")
        ));
    }
    if let Some(policy) = event.network_approval_context.as_ref() {
        let NetworkApprovalContext { host, protocol } = policy;
        content.push(format!("Network Approval Context: {:?} {}", protocol, host));
    }
    if let Some(permissions) = event.additional_permissions.as_ref() {
        content.push(format!(
            "Additional Permissions: {}",
            serde_json::to_string_pretty(&permissions)?
        ));
    }
    content.push(format!(
        "Available Decisions: {}",
        available_decisions
            .iter()
            .map(ToString::to_string)
            .join("\n")
    ));
    if let Some(amendments) = event.proposed_network_policy_amendments.as_ref() {
        content.push(format!(
            "Proposed Network Policy Amendments: {}",
            amendments
                .iter()
                .map(|amendment| format!("{:?} {:?}", amendment.action, amendment.host))
                .join("\n")
        ));
    }

    Ok((!content.is_empty()).then(|| vec![content.join("\n").into()]))
}

fn non_empty_locations(locations: Vec<ToolCallLocation>) -> Option<Vec<ToolCallLocation>> {
    (!locations.is_empty()).then_some(locations)
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
