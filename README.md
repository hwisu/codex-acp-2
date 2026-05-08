# ACP adapter for Codex

Use [Codex](https://github.com/openai/codex) from [ACP-compatible](https://agentclientprotocol.com) clients such as [Zed](https://zed.dev)!

`codex-acp` is a stdio ACP agent that bridges ACP clients to the Codex CLI/runtime. It supports:

- Context @-mentions
- Images
- Tool calls (with permission requests)
- Following
- Edit review
- TODO lists
- Session mode, model, reasoning effort, approval preset, and service tier configuration
- Session list, load, and close
- Background tool-call and subagent status
- Client MCP servers over HTTP or stdio
- Auth Methods:
  - ChatGPT subscription (requires paid subscription and doesn't work in remote projects)
  - CODEX_API_KEY
  - OPENAI_API_KEY

Learn more about the [Agent Client Protocol](https://agentclientprotocol.com/).

## Slash commands

| Command | Description |
| --- | --- |
| `/review [instructions]` | Review current changes or run a custom review prompt. |
| `/review-branch <branch>` | Review changes against a branch. |
| `/review-commit <sha>` | Review changes introduced by a commit. |
| `/init` | Create an `AGENTS.md` contributor guide for the repository. |
| `/compact` | Summarize the conversation before the context limit is reached. |
| `/status` | Show model, reasoning effort, approval preset, service tier, git branch, MCP count, and session state. |
| `/usage` | Show token usage, context window, rate limits, and credits. |
| `/permissions [read-only\|auto\|full-access]` | Show or change the approval preset. `/approvals` is also accepted. |
| `/agent` | Show tracked subagents for this ACP session. `/subagents` is also accepted. |
| `/ps` | List active background tool calls tracked by the adapter. |
| `/undo` | Undo Codex's most recent turn. |
| `/plan [prompt\|on\|off]` | Switch future turns into or out of plan mode, optionally submitting an inline plan-mode prompt. |
| `/goal [clear\|pause\|resume\|objective]` | Set or view a long-running task goal when the Codex Goals feature is enabled. |
| `/fast [on\|off\|status]` | Toggle Codex Fast mode when the Fast Mode feature is enabled. |
| `/logout` | Log out of Codex and require authentication again. |

## How to use

### Zed

Zed 0.208 and newer can use Codex directly from the Agent Panel. Zed installs and manages this adapter for built-in Codex threads.

To use Codex, open the Agent Panel and click "New Codex Thread" from the `+` button menu in the top-right.

Read the docs on [External Agent](https://zed.dev/docs/ai/external-agents) support.

For a repeatable UI smoke test, see [Zed Computer Use Check Routine](docs/zed-computer-use-check.md).

### Other clients

Try it with any other [ACP-compatible client](https://agentclientprotocol.com/get-started/clients).

#### Installation

Install the adapter from the latest release for your architecture and OS: https://github.com/hwisu/codex-acp/releases

Release binaries are built for macOS, Linux, and Windows on x64 and arm64. Linux release artifacts include both GNU and musl variants; the npm launcher installs the GNU Linux binary.

You can then use `codex-acp` as a regular ACP agent:

```
OPENAI_API_KEY=sk-... codex-acp
```

Or via npm:

```
npx @zed-industries/codex-acp
```

For local development, build the binary and point the npm launcher at it so
client restarts use your modified adapter immediately:

```
cargo build
CODEX_ACP_BIN="$PWD/target/debug/codex-acp" npx @zed-industries/codex-acp
```

## Development checks

Run the same local checks that CI expects before opening a pull request:

```
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
bash npm/testing/validate.sh
node npm/testing/test-platform-detection.js
```

## License

Apache-2.0
