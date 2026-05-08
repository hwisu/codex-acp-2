# codex-acp-2

Codex ACP 버전: `0.13.1` · ACP 계약 구현: advertised `100%` (`12/12` handler), enabled SDK surface `86%` (`12/14`; `session/fork`, `session/resume`은 광고하지 않음)

[English](README.md)

`codex-acp-2`는 Codex 런타임을 위한 Rust stdio
[Agent Client Protocol](https://agentclientprotocol.com) 어댑터입니다.
ACP 호환 클라이언트는 Codex TUI를 거치지 않고 이 어댑터로 Codex 세션을
실행할 수 있습니다.

이 저장소는 소스에서 빌드해 쓰는 fork/rewrite입니다. Zed 같은 ACP 클라이언트에서
사용하려면 이 저장소에서 직접 빌드한 바이너리를 지정해야 합니다. upstream
클라이언트 배포본이 이 fork를 자동 설치한다고 가정하지 마세요.

## 상태

- 패키지 버전: `0.13.1`
- ACP 계약 구현: advertised agent handler `100%` (`12/12`), enabled SDK
  agent method `86%` (`12/14`)
- 공식 Codex ACP adapter 참조:
  [`agentclientprotocol/codex-acp`](https://github.com/agentclientprotocol/codex-acp),
  npm package `@agentclientprotocol/codex-acp = 0.0.43`
- ACP Rust SDK crate:
  [`agent-client-protocol`](https://crates.io/crates/agent-client-protocol)
  ([`agentclientprotocol/rust-sdk`](https://github.com/agentclientprotocol/rust-sdk)),
  현재 `agent-client-protocol = 0.11.1` 및 `unstable` feature로 고정
  (`Cargo.lock` 기준 `agent-client-protocol-schema = 0.12.0`)
- Codex Rust crates:
  [`openai/codex`](https://github.com/openai/codex/tree/rust-v0.129.0/codex-rs)
  tag `rust-v0.129.0`에 고정
  (`Cargo.lock` 기준 `2808a4deb181e5ca2b1293a1a5980938cb746861`)
- 전송 방식: stdio ACP agent
- 주요 용도: 로컬 개발 및 ACP 클라이언트 통합 테스트

## 기능

- Codex 기반 ACP 세션 생성, 로드, 목록 조회, 종료, 리플레이
- 텍스트, 리소스, 링크, 이미지 프롬프트 블록
- 메시지, reasoning, tool call, 백그라운드 상태 스트리밍
- 셸 명령 승인/출력 및 apply-patch 편집 렌더링
- 클라이언트 제공 HTTP/stdio MCP 서버
- 모델, reasoning effort, approval preset, service tier, collaboration mode 설정
- `/review`, `/status`, `/usage`, `/permissions`, `/agent`, `/ps`, `/undo`,
  `/plan`, `/goal`, `/fast`, `/logout` 등 slash command

터미널 출력은 ACP `_meta` 호환 확장을 사용합니다. Zed는 클라이언트가 지원을
알리면 자동으로 사용합니다. 다른 클라이언트는
`CODEX_ACP_ENABLE_EXPERIMENTAL_TERMINAL_OUTPUT=1`로 켤 수 있고,
`CODEX_ACP_DISABLE_TERMINAL_OUTPUT=1`로 끌 수 있습니다.

## 빌드와 실행

```sh
cargo build
OPENAI_API_KEY=sk-... ./target/debug/codex-acp
```

Codex 인증은 브라우저 로그인이 가능한 경우 ChatGPT login을 사용할 수 있고,
`CODEX_API_KEY`도 지원합니다.

npm launcher를 로컬 바이너리로 테스트하려면:

```sh
cargo build
CODEX_ACP_BIN="$PWD/target/debug/codex-acp" node npm/bin/codex-acp.js
```

이 fork의 npm 패키지와 플랫폼 바이너리가 게시되어 있다면:

```sh
npx @hwisu/codex-acp
```

## 클라이언트 설정

ACP 클라이언트는 `codex-acp`를 stdio agent로 실행해야 합니다. 다음 중 하나를
지정하세요.

- 로컬 개발: `./target/debug/codex-acp`
- 로컬 release 빌드: `./target/release/codex-acp`
- 현재 OS/CPU용 게시 바이너리

Zed 수동 점검은
[`docs/zed-computer-use-check.md`](docs/zed-computer-use-check.md)를 참고하세요.

## 저장소 구조

- `src/main.rs`: 바이너리 진입점
- `src/lib.rs`: config, tracing, auth residency, stdio serving
- `src/codex_agent.rs`: ACP agent lifecycle 및 세션 관리
- `src/thread/`: 세션 actor, prompt, slash command, replay, permission
- `src/boundary/`: Codex-to-ACP mapping, compatibility metadata, contract test
- `src/boundary/file_changes.rs`: ACP diff content 추출
- `npm/`: npm launcher 및 platform package scaffolding
- `docs/`: 수동 smoke test 노트

런타임 흐름은 주로 `src/thread/`에, wire-shape 매핑은 `src/boundary/`에 둡니다.

## 개발 체크

```sh
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
bash npm/testing/validate.sh
node npm/testing/test-platform-detection.js
```

Contract test는 새 Codex enum variant, 직접 wire payload 생성, mapper fallback이
boundary 설계를 깨면 의도적으로 실패합니다.

## 참고

클라이언트 MCP metadata는 `_meta.codex` 또는 `_meta.codex_acp`로 전달할 수
있습니다. HTTP와 stdio MCP 서버는 Codex config로 변환되며, SSE 서버는 무시됩니다.

`npm/` 아래 패키지는 launcher일 뿐입니다. 이 fork의 platform binary package가
게시되지 않았다면 `CODEX_ACP_BIN`을 사용하세요.

## 라이선스

Apache-2.0. [`LICENSE`](LICENSE)를 참고하세요.
