# codex-acp-2 계획 아웃라인

## 요약

- 작성 대상: `/Users/hwisookim/codex-acp-2/plan.md`.
- 조사 기준: 로컬 `/Users/hwisookim/codex-acp`, [hwisu/codex-acp](https://github.com/hwisu/codex-acp), upstream [zed-industries/codex-acp](https://github.com/zed-industries/codex-acp), 별도 구현 [agentclientprotocol/codex-acp](https://github.com/agentclientprotocol/codex-acp).
- 기본 방향: 로컬 Rust 구현을 베이스로 삼고, Zed upstream의 배포/호환성 안정성, agentclientprotocol TypeScript 구현의 generated type discipline, event routing, snapshot-style 검증 방식을 흡수한다.
- 핵심 목표: Codex crate 타입과 ACP 타입의 변환 지점을 한눈에 보이게 만들고, 숨은 fallback이나 `_ => ignore`식 처리를 없앤다.

## 핵심 변경

- `src/boundary/` 계층을 만든다.
  - `codex`: `codex_protocol`, `codex_core`, `codex_config`에서 들어오는 타입만 다룬다.
  - `acp`: `agent_client_protocol::schema` 타입만 다룬다.
  - `mapper`: Codex 타입을 ACP update/effect로 변환하는 유일한 장소.
  - `raw`: `raw_input`, `raw_output`, `_meta` 생성 정책을 중앙화한다.
  - `compat`: Zed terminal output 같은 client-specific 확장을 격리한다.

- actor/runtime은 변환을 직접 하지 않는다.
  - runtime은 `BridgeEffect`를 실행만 한다.
  - Codex `EventMsg`, `ResponseItem`, approval request, MCP elicitation은 mapper에서 `Forward`, `RequestPermission`, `SubmitOp`, `Ignore(reason)` 중 하나로 명시 변환한다.

- exhaustiveness를 계약으로 삼는다.
  - Codex enum match에는 무의미한 catch-all을 두지 않는다.
  - 의도적으로 무시하는 이벤트도 `IgnoredCodexEventReason`에 이유를 남긴다.
  - Codex crate 업데이트 때 새 variant가 생기면 compile/test에서 드러나게 한다.

## 인터페이스/타입

- 새 내부 타입:
  - `ModelSelection { model: String, reasoning_effort: Option<ReasoningEffort> }`
  - `BridgeEffect`
  - `ToolCallLifecycle`
  - `ApprovalSelection`
  - `McpServerSpec`
  - `IgnoredCodexEventReason`

- ACP model id는 `model[effort]`를 canonical format으로 쓴다.
  - 기존 `model/effort`는 legacy decode만 허용한다.
  - unsupported reasoning effort는 fallback하지 않고 `invalid_params`로 드러낸다.

- permission option id, MCP approval meta key, terminal meta key는 상수 모듈로 모은다.
  - 문자열 literal이 submission/event handler에 흩어지지 않게 한다.

- 기존 public surface는 유지한다.
  - binary 이름 `codex-acp` 유지.
  - MCP re-export 유지.
  - npm launcher/release scripts는 유지하되 repo metadata만 새 repo에 맞춘다.

## 테스트 계획

- 기본 체크:
  - `cargo fmt --all -- --check`
  - `cargo test --all-targets`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `bash npm/testing/validate.sh`
  - `node npm/testing/test-platform-detection.js`

- mapping contract tests:
  - `EventMsg` 전체 variant가 `Forward/Ignore/RequestPermission/SubmitOp` 중 하나로 분류되는지 테스트한다.
  - `ResponseItem` replay와 live event가 같은 mapper를 거치도록 검증한다.
  - `raw_input/raw_output/_meta` 생성은 `boundary/raw` 밖에서 직접 `serde_json::json!(&event)`를 쓰지 못하게 `rg` 기반 boundary check를 추가한다.

- upstream behavior 흡수 테스트:
  - agentclientprotocol TS fixture 성격의 케이스를 Rust 테스트로 포팅한다: terminal output, file change, approval, MCP approval, token usage, model filtering, session load/list, multi-session notification routing.
  - Zed 호환 smoke path는 기존 `docs/zed-computer-use-check.md` 기준으로 유지한다.

## 가정

- 새 repo는 로컬 Rust fork를 베이스로 시작한다. 이유는 목표가 "codex crate에서 오는 타입 매칭"이므로 app-server subprocess 방식보다 direct Rust crate integration이 맞다.
- `zed-industries/codex-acp`는 upstream 안정성/배포 기준으로 참조하고, `agentclientprotocol/codex-acp`는 typed app-server client와 event-driven 테스트 방식만 흡수한다.

## 현재 구현 상태

- `src/boundary/` 계층을 실제 코드에 도입했다.
  - `model`, `constants`, `raw`, `compat`, `effect`, `approval`, `permission`, `mcp_approval`, `op`, `session_update`, `tool_call`, `mapper`가 생겼다.
  - model id는 canonical `model[effort]`를 쓰고 legacy `model/effort`는 decode만 허용한다.
  - terminal output 호환성, permission option id, MCP approval meta key, raw input/output 생성 정책은 boundary 아래로 모았다.

- thread runtime의 직접 변환 책임을 크게 줄였다.
  - production `src/thread`는 더 이상 `EventMsg::`, `ResponseItem::`, `RolloutItem::`를 직접 match하지 않는다.
  - production `src/thread`는 더 이상 `Op::`, `SessionUpdate::`, `ToolCall::new`, `ToolCallUpdate::new`, `RequestPermissionRequest::new`를 직접 만들지 않는다.
  - replay `EventMsg`, live stateless event, dynamic tool call, MCP tool call, patch apply, collab event, token usage, review-mode exit은 mapper/session_update/tool_call boundary에서 `BridgeEffect` 또는 명시적 ignore reason으로 변환된다.
  - `DynamicToolCallRequest`의 `started_at_ms` 같은 Codex struct 필드 추가가 compile error로 드러나는 구조가 확인됐다.

- 계약 테스트를 추가했다.
  - thread runtime에서 Codex enum matching, ACP wire payload 생성, direct raw event serialization을 막는다.
  - mapper에서 wildcard `_ =>` 사용을 막는다.
  - replay/live converted path는 `BridgeEffect`만 실행하도록 검사한다.

## 다음 구현 계획

- stateful live path를 더 좁힌다.
  - streaming delta는 runtime이 state만 갱신하고 mapper가 만든 text/thought effect를 실행하는 구조로 바꾼다.
  - web-search begin/update/complete는 active search state와 ACP tool-call effect 생성을 분리한다.
  - guardian assessment는 boundary가 effect plan을 만들고 runtime은 active-id state transition만 고르는 현재 구조를 유지하되 계약 테스트를 더 명확히 한다.
  - exec path는 마지막으로 다룬다. terminal compat, active command, output buffer가 얽혀 있어 가장 위험하다.

- mapper contract를 강화한다.
  - `LiveForwardEvent`가 runtime state transition만 표현하도록 이름과 variant를 정리한다.
  - `IgnoredCodexEventReason`은 상태 없음, missing field, unsupported ACP, replay-only/snapshot-only 이유를 더 세분화한다.
  - `classify_event_msg`와 `route_live_event`의 classification이 어긋나지 않는 테스트를 추가한다.

- 클라이언트 호환성 검증을 확장한다.
  - Zed terminal meta smoke path는 유지한다.
  - Toad/IntelliJ는 ACP 표준 `SessionUpdate`, `ToolCall`, `ToolCallUpdate`, `RequestPermission`만으로 동작하도록 client-specific 분기를 `boundary/compat` 밖에 두지 않는다.
