use agent_client_protocol::Error;
use codex_protocol::protocol::{
    ExecApprovalRequestEvent, ExecCommandBeginEvent, ExecCommandEndEvent,
    ExecCommandOutputDeltaEvent, TerminalInteractionEvent,
};

use crate::boundary::{effect::BridgeEffect, permission, tool_call};

use super::{
    approvals::PendingPermissionRequest,
    client::SessionClient,
    submission::{PermissionInteractionRequest, PromptState},
};

impl PromptState {
    pub(super) fn exec_approval(
        &mut self,
        client: &SessionClient,
        event: ExecApprovalRequestEvent,
    ) -> Result<(), Error> {
        let request = permission::exec_approval_interaction(event)?;
        let permission::ExecApprovalInteraction {
            request_key,
            approval_id,
            turn_id,
            option_map,
            active_command,
            permission_request,
        } = request;
        let permission::ActiveCommandSeed {
            call_id,
            tool_call_id,
            title,
            kind,
            terminal_output,
            file_extension,
            cwd,
        } = active_command;
        self.insert_active_command(
            call_id.clone(),
            tool_call::ActiveCommand {
                title,
                kind,
                terminal_output,
                tool_call_id,
                output: String::new(),
                file_extension,
                cwd,
            },
        );

        self.spawn_permission_request(
            client,
            PermissionInteractionRequest {
                request_key,
                pending_request: PendingPermissionRequest::Exec {
                    approval_id,
                    turn_id,
                    option_map,
                },
                request_effect: client.request_permission_effect(permission_request),
            },
        );

        Ok(())
    }

    pub(super) fn exec_command_begin(
        &mut self,
        client: &SessionClient,
        event: ExecCommandBeginEvent,
    ) -> BridgeEffect {
        let plan = tool_call::exec_command_begin_effect_plan(event, |command| {
            client.supports_terminal_output(command)
        });
        self.insert_active_command(plan.call_id, plan.active_command);
        plan.effect
    }

    pub(super) fn exec_command_output_delta(
        &mut self,
        client: &SessionClient,
        event: ExecCommandOutputDeltaEvent,
    ) -> Option<BridgeEffect> {
        let ExecCommandOutputDeltaEvent {
            call_id,
            chunk,
            stream: _,
        } = event;
        let data_str = String::from_utf8_lossy(&chunk).to_string();
        self.stream_active_command_output(client, &call_id, &data_str)
    }

    pub(super) fn exec_command_end(
        &mut self,
        client: &SessionClient,
        event: ExecCommandEndEvent,
    ) -> Vec<BridgeEffect> {
        let call_id = event.call_id.clone();
        if let Some(active_command) = self.remove_active_command(&call_id) {
            let supports_terminal_output = client.supports_terminal_output(&active_command);
            let plan = tool_call::exec_command_end_effect_plan(
                event,
                &active_command,
                supports_terminal_output,
            );
            return plan
                .streaming_update
                .into_iter()
                .chain(std::iter::once(plan.completion_update))
                .collect();
        }
        Vec::new()
    }

    pub(super) fn terminal_interaction(
        &mut self,
        client: &SessionClient,
        event: TerminalInteractionEvent,
    ) -> Option<BridgeEffect> {
        let TerminalInteractionEvent {
            call_id,
            process_id: _,
            stdin,
        } = event;

        let stdin = format!("\n{stdin}\n");
        self.stream_active_command_output(client, &call_id, &stdin)
    }
}
