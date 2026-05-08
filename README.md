# codex-acp-2

`codex-acp-2` is a rewritten Rust ACP adapter for the Codex runtime.
It runs as a stdio [Agent Client Protocol](https://agentclientprotocol.com)
agent and lets ACP-compatible clients drive Codex sessions without going
through the Codex TUI.

This repository should be treated as a source-built fork/rewrite, not as a
thin mirror of the original adapter. The implementation is organized around a
strict boundary layer that translates Codex protocol events into ACP session
updates, permission requests, and tool-call payloads.

## Status

- Package version: `0.13.0`
- ACP crate: `agent-client-protocol = 0.11.1` with `unstable` enabled
- Codex crates: OpenAI Codex Rust crates pinned to `rust-v0.129.0`
- Transport: stdio ACP agent
- Primary use case: local development and ACP client integration testing

If you are using this fork from a client such as Zed, point the client at the
binary you built from this repository. Do not assume the upstream client
distribution installs this fork automatically.

## What It Supports

- Create, load, list, and close Codex-backed ACP sessions
- Text prompts, embedded text resources, resource links, and image prompt blocks
- Streaming agent messages and reasoning chunks
- Codex tool calls rendered as ACP `ToolCall` / `ToolCallUpdate`
- Shell command approval and output rendering
- Apply-patch approval and edit rendering
- Client-provided MCP servers over HTTP or stdio
- MCP tool approval elicitations mapped to ACP permission requests
- Dynamic tool calls, web search, generated image, guardian, and subagent status updates
- Session configuration for collaboration mode, model, reasoning effort,
  approval preset, and service tier
- Conversation replay for loaded sessions

Terminal output uses an ACP `_meta` compatibility extension. It is enabled for
Zed clients that advertise the capability. Other clients can opt in with
`CODEX_ACP_ENABLE_EXPERIMENTAL_TERMINAL_OUTPUT=1`; it can be disabled with
`CODEX_ACP_DISABLE_TERMINAL_OUTPUT=1`.

## Repository Layout

| Path | Purpose |
| --- | --- |
| `src/main.rs` | Binary entry point. |
| `src/lib.rs` | Initializes config, tracing, auth residency, and stdio ACP serving. |
| `src/codex_agent.rs` | ACP agent lifecycle, authentication, session creation, loading, listing, and closing. |
| `src/thread/` | Per-session actor, prompt submission, slash commands, config changes, replay, and permission interaction flow. |
| `src/boundary/` | Codex-to-ACP mapping, raw payload policy, compatibility metadata, permissions, tool-call rendering, and contract tests. |
| `src/file_changes.rs` | Converts Codex file changes into ACP diff content. |
| `npm/` | Optional npm launcher and platform package scaffolding. |
| `docs/` | Manual smoke-test notes. |

The boundary layer is intentional. Runtime code in `src/thread/` should mostly
execute plans and effects; wire-shape details should live under `src/boundary/`.
The contract tests enforce that split.

## Build And Run

Build the adapter:

```sh
cargo build
```

Run the debug binary directly:

```sh
OPENAI_API_KEY=sk-... ./target/debug/codex-acp
```

You can also authenticate with:

- ChatGPT login through Codex auth, when browser login is available
- `CODEX_API_KEY`
- `OPENAI_API_KEY`

For local npm-launcher testing, point the launcher at the binary you just
built:

```sh
cargo build
CODEX_ACP_BIN="$PWD/target/debug/codex-acp" node npm/bin/codex-acp.js
```

If this fork's npm packages and release binaries have been published, the base
package can be used as a normal launcher:

```sh
npx @hwisu/codex-acp
```

Otherwise, prefer `CODEX_ACP_BIN` while developing.

## ACP Client Setup

ACP clients should launch `codex-acp` as a stdio agent. The exact configuration
format is client-specific, but the command should point at either:

- `./target/debug/codex-acp` for local development
- `./target/release/codex-acp` for a local release build
- a published release binary for the current OS and CPU

For Zed-oriented manual checks, see
[`docs/zed-computer-use-check.md`](docs/zed-computer-use-check.md).

## Slash Commands

| Command | Description |
| --- | --- |
| `/review [instructions]` | Review current changes, or run custom review instructions. |
| `/review-branch <branch>` | Review changes against a branch. |
| `/review-commit <sha>` | Review changes introduced by a commit. |
| `/init` | Submit the built-in AGENTS.md initialization prompt. |
| `/compact` | Ask Codex to compact conversation context. |
| `/status` | Show model, reasoning effort, approval preset, service tier, branch, MCP count, tool calls, subagents, pending input, and cwd. |
| `/usage` | Show token usage, context window, rate limits, and credits when available. |
| `/permissions [read-only|auto|full-access]` | Show or change the approval preset. `/approvals` is also accepted. |
| `/agent` | Show tracked subagents for this ACP session. `/subagents` is also accepted. |
| `/ps` | List active background tool calls tracked by the adapter. |
| `/undo` | Roll back Codex's most recent turn. |
| `/plan [prompt|on|off]` | Switch future turns into or out of plan mode, or submit an inline plan-mode prompt. |
| `/goal [clear|pause|resume|objective]` | Set or view a long-running task goal when the Codex Goals feature is enabled. |
| `/fast [on|off|status]` | Toggle Codex Fast mode when the Fast Mode feature is enabled. |
| `/logout` | Log out of Codex and require authentication again. |

## MCP Server Metadata

Client-provided MCP servers are converted into Codex MCP server config when
Codex supports the transport:

- HTTP MCP servers map to streamable HTTP config.
- Stdio MCP servers map to stdio config.
- SSE MCP servers are ignored.

Additional Codex-specific settings can be passed in `_meta.codex` or
`_meta.codex_acp`, including:

- `enabled`, `required`
- `startupTimeoutSec`, `toolTimeoutSec`
- `supportsParallelToolCalls`
- `defaultToolsApprovalMode`
- `enabledTools`, `disabledTools`
- `scopes`
- `oauthResource`
- `experimentalEnvironment`
- HTTP-only: `bearerTokenEnvVar`, `envHttpHeaders`
- stdio-only: `cwd`, `envVars`

Relative stdio `cwd` values are resolved against the session working directory.

## Development Checks

Run the same checks used for local validation:

```sh
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
bash npm/testing/validate.sh
node npm/testing/test-platform-detection.js
```

The current source tree has contract tests that intentionally fail when new
Codex enum variants, direct wire-payload construction, or mapper fallbacks
break the rewrite boundaries.

## Release Notes

The npm package under `npm/` is a launcher. It expects platform-specific binary
packages to exist for macOS, Linux, and Windows on `arm64` and `x64`. If those
artifacts are not published for this fork, use `CODEX_ACP_BIN` instead.

Release binaries should be built from this repository and tested through the
development checks above before publishing.

## License

This repository is licensed under Apache-2.0. It is a permissive license
similar in practical use to MIT, with an additional explicit patent grant. See
[`LICENSE`](LICENSE).
