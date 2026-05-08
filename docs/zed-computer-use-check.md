# Zed Computer Use Check Routine

Use this routine to smoke-test `codex-acp` in Zed with the Computer Use plugin. It is intentionally UI-focused: the goal is to confirm that Zed can start Codex, exchange ACP messages, surface tool calls, and keep the thread usable.

Reference docs:

- Zed Codex external agent docs: https://zed.dev/docs/ai/external-agents#codex-cli
- Zed Agent Panel docs: https://zed.dev/docs/ai/agent-panel
- Zed ACP logs docs: https://zed.dev/docs/ai/external-agents#debugging-agents

## Preflight

Run these from the repository before checking Zed:

```sh
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
bash npm/testing/validate.sh
node npm/testing/test-platform-detection.js
```

Confirm Zed is running with Computer Use:

1. Call `list_apps` and verify `Zed` is running.
2. Call `get_app_state` for `Zed`.
3. Record the window title, current project, and whether the Agent Panel is visible.

## Zed Smoke Test

1. Open the Agent Panel in Zed.
   - Use the Agent Panel command or the UI control documented by Zed.
   - If the panel is already open, use Computer Use `get_app_state` to confirm the composer and thread controls are visible.

2. Start a new Codex thread.
   - Use the Agent Panel `+` menu and choose Codex.
   - Pass if a Codex thread opens without an immediate ACP or authentication error.
   - If authentication is requested, complete it using the desired Codex method, then restart this step.

3. Send a status prompt:

```text
/status
```

Pass criteria:

- Zed shows a Codex response.
- The response includes current model/session/approval information.
- The thread remains interactive after the response completes.

4. Send a usage prompt:

```text
/usage
```

Pass criteria:

- Zed renders a usage response rather than an ACP protocol error.
- Empty or unavailable usage data is acceptable if the response explains the state.

5. Send a permissions prompt:

```text
/permissions
```

Pass criteria:

- Zed shows the current approval preset.
- Available presets include `read-only`, `auto`, and `full-access`.
- The thread remains interactive after the response completes.

6. Check read-only context handling:

```text
Summarize README.md in two sentences. Do not edit files.
```

Pass criteria:

- Codex can read project context.
- Zed shows any file/tool-call UI in a stable way.
- No unexpected file edits appear in the editor or git status.

7. Check command/tool-call rendering:

```text
Run pwd and then stop. Do not edit files.
```

Pass criteria:

- Zed shows a command/tool-call entry.
- Permission UI appears if required by the active approval mode.
- Terminal or output snapshot is visible after completion.
- The final answer mentions the working directory.

8. Check background tool-call reporting:

```text
/ps
```

Pass criteria:

- Zed shows either active tool calls or the "No background tool calls" message.
- The command returns without starting a Codex model turn.

9. Check cancellation behavior, if the response is still running.
   - Use Zed's stop/cancel control.
   - Pass if the thread returns to an idle state and accepts another message.

10. Open ACP logs.
   - Use Zed command `dev: open acp logs`.
   - Pass if initialize, prompt, tool-call, and completion messages are visible for the smoke-test thread.
   - Save relevant log snippets only when filing an issue.

## Computer Use Observations To Record

For each run, capture these with `get_app_state` before and after the smoke test:

- Zed window title and active project.
- Whether the Agent Panel is visible.
- Whether a Codex thread is selected.
- Any visible ACP/authentication error.
- Whether command output rendered as terminal output or content snapshots.
- Whether the UI returned to an idle composer after each prompt.

## Failure Triage

- If Codex is not listed in Zed, check the installed Zed version and external agent settings.
- If Zed starts Codex but prompts fail, open `dev: open acp logs` and inspect initialize/auth/prompt errors.
- If command output is missing but the command completed, compare whether Zed advertised terminal output support and whether content snapshots were sent.
- If the thread hangs after cancel or close, inspect actor shutdown and permission interaction handling.
- If managed Zed Codex works but local development does not, verify Zed is configured to run the intended local `codex-acp` binary rather than its managed install.
