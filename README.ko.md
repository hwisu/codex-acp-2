# codex-acp-2

Codex ACP 버전: `0.142.1` · ACP 계약 구현: advertised `100%` (`11/11` handler), enabled SDK surface `11/16` (`session/delete`, `session/fork`, `session/resume`은 광고하지 않음; `mcp/connect` 비활성화)

[English](README.md)

`codex-acp-2`는 Codex 런타임을 위한 Rust stdio
[Agent Client Protocol](https://agentclientprotocol.com) 어댑터입니다.
ACP 호환 클라이언트는 Codex TUI를 거치지 않고 Codex 세션을 실행할 수 있습니다.

## 업데이트 원칙

이 fork는 upstream Codex 릴리스를 따라가며 아래 세 가지 원칙으로 갱신됩니다.

1. **최신 Codex 구현을 존중한다.** 동작은 Codex semantics를 우선 따르며,
   upstream 릴리스가 나올 때마다 의존 태그를 옮긴다.
2. **ACP에 매칭되는 개념이 있으면 최대한 반영한다.** 세션·프롬프트·tool
   call·permission·plan·diff는 ACP 표준 형태로 노출한다.
3. **ACP에 개념이 없으면 우회해서라도 사용에 불편함이 없도록 한다.**
   예: `/usage`, `/status`, `/ps`, `/permissions`, `/agent`, `/plan`, `/goal`
   등은 별도 ACP 메서드 대신 slash command와 `_meta` 확장으로 노출한다.

## 고정 버전

- Codex Rust crates: [`openai/codex`](https://github.com/openai/codex/tree/rust-v0.142.1/codex-rs)
  tag `rust-v0.142.1` (`Cargo.lock` 기준 `95da8fd25193fd58d1c5984eee20d1ef7bd50e77`)
- ACP Rust SDK: [`agent-client-protocol`](https://crates.io/crates/agent-client-protocol)
  ([`agentclientprotocol/rust-sdk`](https://github.com/agentclientprotocol/rust-sdk)) -
  `agent-client-protocol = 1.0.0` + `unstable`
  (`agent-client-protocol-schema = 1.1.0`, lockfile 기준)
- 공식 Codex ACP adapter 참조:
  [`agentclientprotocol/codex-acp`](https://github.com/agentclientprotocol/codex-acp),
  npm `@agentclientprotocol/codex-acp = 1.0.0`

## 기능

- Codex 기반 ACP 세션: 생성·로드·목록·종료·리플레이
- 텍스트·리소스·링크·이미지 프롬프트 블록
- 메시지·reasoning·tool call·백그라운드 상태 스트리밍
- 셸 명령 승인/출력 및 apply-patch 편집 렌더링
- 클라이언트 제공 MCP 서버 (HTTP, stdio; SSE는 무시)
- 세션 설정: 모델, reasoning effort, approval preset, service tier,
  collaboration mode
- Slash command: `/review`, `/status`, `/usage`, `/permissions`, `/agent`,
  `/ps`, `/undo`, `/plan`, `/goal`, `/fast`, `/logout`

ACP `session/delete`, `session/fork`, `session/resume`, MCP-over-ACP proxy
method는 아직 직접 노출하지 않습니다. 클라이언트가 세션 생성 시 넘기는 MCP
서버 설정은 Codex MCP config로 변환하며, MCP tool 출력은 기존처럼 ACP tool call로
렌더링합니다.

터미널 출력은 ACP `_meta` 호환 확장을 사용합니다. Zed는 자동으로 인식하며,
다른 클라이언트는 `CODEX_ACP_ENABLE_EXPERIMENTAL_TERMINAL_OUTPUT=1`로 켜고
`CODEX_ACP_DISABLE_TERMINAL_OUTPUT=1`로 끌 수 있습니다.

## 빌드와 실행

```sh
cargo build --release
./target/release/codex-acp
```

인증은 ChatGPT 브라우저 로그인과 `CODEX_API_KEY`도 지원합니다. npm launcher를
로컬 바이너리로 테스트:

```sh
CODEX_ACP_BIN="$PWD/target/release/codex-acp" node npm/bin/codex-acp.js
```

플랫폼 바이너리가 게시되어 있다면: `npx @hwisu/codex-acp`.

## Zed에서 실행

`~/.config/zed/settings.json`의 `agent_servers`에 릴리즈 바이너리를 지정합니다.

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

Zed를 재시작한 뒤 Agent Panel에서 `codex-acp`를 선택합니다.

## Toad에서 실행

직접 실행:

```sh
toad acp "$PWD/target/release/codex-acp" /path/to/project
```

Toad의 Codex 항목을 기본으로 쓰려면 Toad 내장 agent TOML의 `run_command`를 이
바이너리로 지정합니다.

```toml
run_command."*" = "/absolute/path/to/codex-acp-2/target/release/codex-acp"
```

이후 Toad UI에서 Codex를 선택하거나 아래처럼 실행합니다.

```sh
toad run -a openai.com /path/to/project
```

호환성 점검 기록:
[`docs/toad-compatibility-check.md`](docs/toad-compatibility-check.md).

## 개발

```sh
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
bash npm/testing/validate.sh
```

`src/boundary/contracts.rs`의 contract test는 새 Codex enum variant, 직접 wire
payload 생성, mapper fallback이 boundary 설계를 깨면 의도적으로 실패합니다.
런타임 흐름은 `src/thread/`, wire 매핑은 `src/boundary/`에 둡니다. Zed 수동
점검: [`docs/zed-computer-use-check.md`](docs/zed-computer-use-check.md).

## 라이선스

Apache-2.0. [`LICENSE`](LICENSE)를 참고하세요.
