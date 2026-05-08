# codex-acp-2

Codex ACP version: `0.13.1` · ACP contract: `100%` advertised (`12/12` handlers), `86%` enabled SDK surface (`12/14`; `session/fork` and `session/resume` are not advertised)

[Korean](README.ko.md)

`codex-acp-2` is a Rust stdio
[Agent Client Protocol](https://agentclientprotocol.com) adapter for the Codex
runtime. ACP-compatible clients can use it to run Codex sessions without going
through the Codex TUI.

This repository is a source-built fork/rewrite. Point your ACP client at a
binary built from this repository; upstream client distributions do not install
this fork automatically.

## Status

- Package version: `0.13.1`
- ACP contract implementation: `100%` of advertised agent handlers (`12/12`);
  `86%` of enabled SDK agent methods (`12/14`)
- Official Codex ACP adapter reference:
  [`agentclientprotocol/codex-acp`](https://github.com/agentclientprotocol/codex-acp),
  npm package `@agentclientprotocol/codex-acp = 0.0.43`
- ACP Rust SDK crate:
  [`agent-client-protocol`](https://crates.io/crates/agent-client-protocol)
  from [`agentclientprotocol/rust-sdk`](https://github.com/agentclientprotocol/rust-sdk),
  pinned here as `agent-client-protocol = 0.11.1` with `unstable`
  (`agent-client-protocol-schema = 0.12.0` via lockfile)
- Codex Rust crates:
  [`openai/codex`](https://github.com/openai/codex/tree/rust-v0.129.0/codex-rs)
  pinned to tag `rust-v0.129.0`
  (`2808a4deb181e5ca2b1293a1a5980938cb746861` in `Cargo.lock`)
- Transport: stdio ACP agent
- Main use: local development and ACP client integration testing

## Features

- Codex-backed ACP sessions: create, load, list, close, and replay
- Text, resource, link, and image prompt blocks
- Streaming messages, reasoning, tool calls, and background status
- Shell command approval/output and apply-patch edit rendering
- Client-provided MCP servers over HTTP or stdio
- Session configuration for model, reasoning effort, approval preset, service
  tier, and collaboration mode
- Slash commands including `/review`, `/status`, `/usage`, `/permissions`,
  `/agent`, `/ps`, `/undo`, `/plan`, `/goal`, `/fast`, and `/logout`

Terminal output uses an ACP `_meta` compatibility extension. Zed gets it when
the client advertises support. Other clients can set
`CODEX_ACP_ENABLE_EXPERIMENTAL_TERMINAL_OUTPUT=1`; set
`CODEX_ACP_DISABLE_TERMINAL_OUTPUT=1` to turn it off.

## Build And Run

```sh
cargo build
OPENAI_API_KEY=sk-... ./target/debug/codex-acp
```

Codex authentication can also use ChatGPT login when browser auth is available,
or `CODEX_API_KEY`.

For npm-launcher testing:

```sh
cargo build
CODEX_ACP_BIN="$PWD/target/debug/codex-acp" node npm/bin/codex-acp.js
```

If this fork's npm packages and platform binaries are published:

```sh
npx @hwisu/codex-acp
```

## Client Setup

ACP clients should launch `codex-acp` as a stdio agent. Use one of:

- `./target/debug/codex-acp` for local development
- `./target/release/codex-acp` for a local release build
- a published binary for the current OS and CPU

For Zed manual checks, see
[`docs/zed-computer-use-check.md`](docs/zed-computer-use-check.md).

## Repository Layout

- `src/main.rs`: binary entry point
- `src/lib.rs`: config, tracing, auth residency, and stdio serving
- `src/codex_agent.rs`: ACP agent lifecycle and session management
- `src/thread/`: per-session actor, prompts, slash commands, replay, permissions
- `src/boundary/`: Codex-to-ACP mapping, compatibility metadata, contract tests
- `src/boundary/file_changes.rs`: ACP diff content extraction
- `npm/`: npm launcher and platform package scaffolding
- `docs/`: manual smoke-test notes

Runtime flow belongs mostly in `src/thread/`; wire-shape mapping belongs under
`src/boundary/`.

## Development Checks

```sh
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
bash npm/testing/validate.sh
node npm/testing/test-platform-detection.js
```

Contract tests intentionally fail when new Codex enum variants, direct wire
payload construction, or mapper fallbacks break the boundary design.

## Notes

Client MCP metadata can be passed through `_meta.codex` or `_meta.codex_acp`.
HTTP and stdio MCP servers are mapped into Codex config; SSE servers are ignored.

The npm package under `npm/` is only a launcher. If platform binary packages are
not published for this fork, use `CODEX_ACP_BIN`.

## License

Apache-2.0. See [`LICENSE`](LICENSE).
