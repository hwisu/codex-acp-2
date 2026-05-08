# Toad Compatibility Check Routine

Use this routine to smoke-test `codex-acp` in Toad. It focuses on the ACP flow
Toad currently exercises: initialize, new/load session, prompt content, tool
calls, permissions, and command-output fallback.

Reference notes:

- Toad can launch an arbitrary ACP command with `toad acp`.
- Toad's built-in Codex store entry currently runs `npx @zed-industries/codex-acp`.
- Toad does not call ACP `authenticate`, so use an environment API key or an
  existing Codex login.

## Preflight

Run these from the repository before checking Toad:

```sh
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
bash npm/testing/validate.sh
node npm/testing/test-platform-detection.js
```

Build the local binary:

```sh
cargo build
```

Launch Toad against this fork, not the built-in Codex store entry:

```sh
OPENAI_API_KEY=sk-... toad acp "$PWD/target/debug/codex-acp" "$PWD"
```

`CODEX_API_KEY` is also accepted. The key must be present in the Toad process
environment; `codex-acp` seeds it into in-memory auth only.

## Toad Smoke Test

1. Start Toad with the `toad acp` command above.
   - Pass if Toad opens a conversation without an immediate ACP or auth error.
   - If using the store UI instead, confirm the selected command is this fork's
     binary or npm launcher, not `@zed-industries/codex-acp`.

2. Send a status prompt:

```text
/status
```

Pass criteria:

- Toad shows a Codex response.
- The response includes current model/session/approval information.
- The composer returns to an idle state.

3. Send a usage prompt:

```text
/usage
```

Pass criteria:

- Toad renders a usage response rather than an ACP protocol error.
- Empty or unavailable usage data is acceptable if the response explains it.

4. Check text resource handling:

```text
Summarize @README.md in two sentences. Do not edit files.
```

Pass criteria:

- Toad sends the prompt successfully.
- Codex summarizes the file content.
- No unexpected file edits appear in `git status`.

5. Check image resource handling:

```text
Describe @path/to/a/small-image.png. Do not edit files.
```

Pass criteria:

- Codex receives the image as an image input, not as garbled text.
- If the image cannot be read from the project directory, the turn still
  completes with an omitted-resource marker rather than a large binary context.

6. Check non-image binary resource handling:

```text
Tell me what kind of file @path/to/archive.zip appears to be. Do not edit files.
```

Pass criteria:

- The prompt does not include base64 or binary bytes in model context.
- Codex can still see the URI and MIME-type marker.

7. Check command/tool-call rendering:

```text
Run pwd and then stop. Do not edit files.
```

Pass criteria:

- Toad shows a command/tool-call entry.
- Permission UI appears if required by the active approval mode.
- Command output is visible as tool-call content after completion.
- The final answer mentions the working directory.

8. Check permission behavior:

```text
Create a temporary file named codex-acp-toad-smoke.txt containing "ok", then stop.
```

Pass criteria:

- Toad shows a permission request when the active mode requires approval.
- Selecting an allow option lets the tool call complete.
- Selecting a reject/cancel option leaves the turn usable and reports failure
  without hanging.

9. Check background tool-call reporting:

```text
/ps
```

Pass criteria:

- Toad shows either active tool calls or the "No background tool calls" message.
- The command returns without starting a Codex model turn.

10. Check resume, if the session was saved.
    - Exit Toad and reopen the same session.
    - Pass if Toad uses `session/load` and the thread accepts another prompt.

## Observations To Record

- Toad version or git commit.
- Launch command used for the ACP agent.
- Whether the environment key was `OPENAI_API_KEY` or `CODEX_API_KEY`.
- Whether the selected agent was launched via `toad acp` or store UI.
- Any visible ACP/authentication error.
- Whether command output rendered as content snapshots.
- Whether image resources arrived as image inputs.

## Failure Triage

- If Toad starts upstream Codex instead of this fork, use `toad acp` or edit the
  agent command so it points at the intended binary.
- If auth fails and Toad did not call `authenticate`, confirm the API key is in
  the Toad process environment.
- If image prompts look like binary text, inspect whether Toad sent
  `resource.blob` or `resource.text` and whether the file URI is inside the
  project directory.
- If command output is missing, verify that content snapshots were sent; Toad
  does not currently opt into this repo's Zed terminal-output `_meta` bridge.
- If permissions hang, inspect `session/request_permission` request/response
  payloads in the Toad log.
