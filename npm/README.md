# @hwisu/codex-acp

npm launcher for `codex-acp-2`, a rewritten Rust stdio ACP adapter for the
Codex runtime.

This package is only a launcher. It resolves and executes the platform-specific
binary package for the current OS and CPU:

- `@hwisu/codex-acp-darwin-arm64`
- `@hwisu/codex-acp-darwin-x64`
- `@hwisu/codex-acp-linux-arm64`
- `@hwisu/codex-acp-linux-x64`
- `@hwisu/codex-acp-win32-arm64`
- `@hwisu/codex-acp-win32-x64`

## Run

```sh
npx @hwisu/codex-acp
```

Authentication is handled by Codex. You can use ChatGPT login when browser auth
is available, or set `CODEX_API_KEY` / `OPENAI_API_KEY`.

## Local Development

Build the Rust binary and point the launcher at it:

```sh
cargo build
CODEX_ACP_BIN="$PWD/target/debug/codex-acp" npx @hwisu/codex-acp
```

If the platform binary packages have not been published for this fork, use
`CODEX_ACP_BIN`.

## Repository

Source: https://github.com/hwisu/codex-acp-2

## License

Apache-2.0
