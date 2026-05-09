# codex-acp-2

Codex 최신 Rust 런타임을 ACP stdio agent로 노출하는 로컬 어댑터입니다.

## Codex 최신 스펙 구현

- Codex Rust crates: `openai/codex` `rust-v0.130.0`
- ACP SDK: `agent-client-protocol = 0.11.1` with `unstable`
- ACP schema: `agent-client-protocol-schema = 0.12.0`
- Binary: `codex-acp`
- Transport: stdio

구현된 주요 ACP surface:

- `initialize`
- `authenticate`, `logout`
- `session/new`, `session/load`, `session/list`, `session/close`
- `session/prompt`, `session/cancel`
- `session/set_mode`
- `session/set_model`
- `session/set_config_option`

Codex 최신 slash command surface에 맞춰 `/goal`은 기본 활성화되어 ACP client에 광고됩니다.

릴리즈 바이너리 빌드:

```sh
cargo build --release
```

실행 파일:

```sh
/Users/hwisookim/codex-acp-2/target/release/codex-acp
```

## Zed에서 실행

`~/.config/zed/settings.json`의 `agent_servers`에 로컬 바이너리를 지정합니다.

```json
{
  "agent_servers": {
    "codex-acp": {
      "command": "/Users/hwisookim/codex-acp-2/target/release/codex-acp",
      "args": []
    }
  }
}
```

Zed를 재시작한 뒤 Agent Panel에서 `codex-acp`를 선택합니다.

## Toad에서 실행

직접 실행:

```sh
toad acp "/Users/hwisookim/codex-acp-2/target/release/codex-acp" /path/to/project
```

Toad의 Codex 항목을 기본으로 쓰려면 Toad 내장 agent TOML의 `run_command`를 이 바이너리로 지정합니다.

```toml
run_command."*" = "/Users/hwisookim/codex-acp-2/target/release/codex-acp"
```

현재 로컬 Toad 설정에서 수정한 파일:

```sh
/Users/hwisookim/.local/share/uv/tools/batrachian-toad/lib/python3.14/site-packages/toad/data/agents/openai.com.toml
```

이후 Toad UI에서 Codex를 선택하거나 아래처럼 실행합니다.

```sh
toad run -a openai.com /path/to/project
```
