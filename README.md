# codex-acp-2

Codex ACP version: `0.142.1` · ACP contract: `100%` advertised (`14/14` handlers), `94%` enabled SDK surface (`15/16`; `session/fork` and `mcp/connect` enabled)

[Korean](README.ko.md)

`codex-acp-2` is a Rust stdio [Agent Client Protocol](https://agentclientprotocol.com)
adapter for the Codex runtime. ACP-compatible clients run Codex sessions through
it without going through the Codex TUI.

## Update Principles

This fork tracks upstream Codex releases under three rules:

1. **Respect the latest Codex implementation.** Behavior follows Codex
   semantics; the dependency tag moves whenever upstream releases.
2. **Map to ACP wherever a matching concept exists.** Sessions, prompts, tool
   calls, permissions, plans, and diffs are surfaced through native ACP shapes.
3. **Work around the gap when ACP has no concept** so users keep full
   functionality. Examples: `/usage`, `/status`, `/ps`, `/permissions`,
   `/agent`, `/plan`, `/goal` are bridged as slash commands and `_meta`
   extensions instead of bespoke ACP methods.

## Pinned Versions

- Codex Rust crates: [`openai/codex`](https://github.com/openai/codex/tree/rust-v0.142.1/codex-rs)
  at tag `rust-v0.142.1` (`95da8fd25193fd58d1c5984eee20d1ef7bd50e77` in `Cargo.lock`)
- ACP Rust SDK: [`agent-client-protocol`](https://crates.io/crates/agent-client-protocol)
  from [`agentclientprotocol/rust-sdk`](https://github.com/agentclientprotocol/rust-sdk) -
  `agent-client-protocol = 1.0.0` with `unstable`
  (`agent-client-protocol-schema = 1.1.0` via lockfile)
- Official Codex ACP adapter reference:
  [`agentclientprotocol/codex-acp`](https://github.com/agentclientprotocol/codex-acp),
  npm `@agentclientprotocol/codex-acp = 1.0.0`

## Features

- Codex-backed ACP sessions: create, load, resume, fork, list, delete, close,
  replay
- Text, resource, link, and image prompt blocks
- Streaming messages, reasoning, tool calls, and background status
- Shell command approval/output and apply-patch edit rendering
- Client-provided MCP servers (HTTP, stdio, MCP-over-ACP; SSE ignored) and
  `additionalDirectories` workspace roots
- Authentication: ChatGPT, API key, custom model gateway, status/logout
  compatibility extensions
- Session config: model, reasoning effort, approval preset, service tier,
  collaboration mode, legacy `session/set_model`
- Slash commands: `/review`, `/status`, `/usage`, `/permissions`, `/agent`,
  `/mcp`, `/skills`, `/ps`, `/undo`, `/plan`, `/goal`, `/fast`, `/logout`

ACP `session/fork` snapshots the source rollout and starts an independent Codex
thread with a fresh session ID. MCP-over-ACP `mcp/connect` is exposed through a
session-scoped loopback Streamable HTTP bridge: ACP `type: "acp"` MCP servers
are connected over `mcp/connect`, proxied through `mcp/message`, and disconnected
on session shutdown. Client-provided MCP server configuration is accepted
through ACP session creation and translated into Codex MCP config; MCP tool
output continues to be rendered as ACP tool calls.

Terminal output uses an ACP `_meta` compatibility extension. Zed picks it up
automatically; other clients can set
`CODEX_ACP_ENABLE_EXPERIMENTAL_TERMINAL_OUTPUT=1`
(or `CODEX_ACP_DISABLE_TERMINAL_OUTPUT=1` to opt out).

## Build And Run

```sh
cargo build --release
./target/release/codex-acp
```

Auth also accepts ChatGPT browser login and `CODEX_API_KEY`. For the npm
launcher with a local binary:

```sh
CODEX_ACP_BIN="$PWD/target/release/codex-acp" node npm/bin/codex-acp.js
```

If platform binaries are published: `npx @hwisu/codex-acp`.

## Zed

Point `agent_servers` in `~/.config/zed/settings.json` at the release binary.

```json
{
  "agent_servers": {
    "codex-acp": {
      "command": "/absolute/path/to/codex-acp-2/target/release/codex-acp",
      "args": []
    }
  }
}
```

Restart Zed, then choose `codex-acp` in the Agent Panel.

## Toad

Run the adapter directly:

```sh
toad acp "$PWD/target/release/codex-acp" /path/to/project
```

To use this adapter through Toad's built-in Codex entry, point the agent TOML
`run_command` at the release binary:

```toml
run_command."*" = "/absolute/path/to/codex-acp-2/target/release/codex-acp"
```

Then select Codex in the Toad UI or run:

```sh
toad run -a openai.com /path/to/project
```

Compatibility notes live in
[`docs/toad-compatibility-check.md`](docs/toad-compatibility-check.md).

## Development

```sh
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
bash npm/testing/validate.sh
```

Contract tests in `src/boundary/contracts.rs` intentionally fail when new Codex
enum variants, direct wire-payload construction, or mapper fallbacks break the
boundary design. Runtime flow lives in `src/thread/`; wire mapping in
`src/boundary/`. Manual Zed checks: [`docs/zed-computer-use-check.md`](docs/zed-computer-use-check.md).

## License

Apache-2.0. See [`LICENSE`](LICENSE).
