# codex-acp-2

Codex ACP version: `0.141.0` · ACP contract: `100%` advertised (`11/11` handlers), `80%` enabled SDK surface (`11/16`; `session/delete`, `session/fork`, and `session/resume` are not advertised; `mcp/connect` is disabled)

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

- Codex Rust crates: [`openai/codex`](https://github.com/openai/codex/tree/rust-v0.141.0/codex-rs)
  at tag `rust-v0.141.0` (`3fb81667d30d9d24297216ea61fbfcc4351b2aa9` in `Cargo.lock`)
- ACP Rust SDK: [`agent-client-protocol`](https://crates.io/crates/agent-client-protocol)
  from [`agentclientprotocol/rust-sdk`](https://github.com/agentclientprotocol/rust-sdk) -
  `agent-client-protocol = 0.14.0` with `unstable`
  (`agent-client-protocol-schema = 0.13.6` via lockfile)
- Official Codex ACP adapter reference:
  [`agentclientprotocol/codex-acp`](https://github.com/agentclientprotocol/codex-acp),
  npm `@agentclientprotocol/codex-acp = 0.0.46`

## Features

- Codex-backed ACP sessions: create, load, list, close, replay
- Text, resource, link, and image prompt blocks
- Streaming messages, reasoning, tool calls, and background status
- Shell command approval/output and apply-patch edit rendering
- Client-provided MCP servers (HTTP, stdio; SSE ignored)
- Session config: model, reasoning effort, approval preset, service tier,
  collaboration mode
- Slash commands: `/review`, `/status`, `/usage`, `/permissions`, `/agent`,
  `/ps`, `/undo`, `/plan`, `/goal`, `/fast`, `/logout`

ACP `session/delete`, `session/fork`, `session/resume`, and MCP-over-ACP proxy
methods are intentionally not exposed by this adapter yet. Client-provided MCP
server configuration is accepted through ACP session creation and translated into
Codex MCP config; MCP tool output continues to be rendered as ACP tool calls.

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
