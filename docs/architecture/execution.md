# 명령 실행과 도구 연결

[아키텍처](../architecture.md) · [CLI 계약](../reference/cli.md) · [출력 계약](../reference/output.md)

## 프런트엔드가 만드는 값

| 파일 | 입력에서 만드는 것 |
|---|---|
| [main.rs](../../crates/kodstar-cli/src/main.rs)와 [args.rs](../../crates/kodstar-cli/src/args.rs) | CLI 진입점과 clap 명령 트리. 도움말·버전·라이선스 분기를 분석 준비 전에 처리한다. |
| [args/options.rs](../../crates/kodstar-cli/src/args/options.rs), [queries.rs](../../crates/kodstar-cli/src/args/queries.rs), [history.rs](../../crates/kodstar-cli/src/args/history.rs) | 명령에 붙는 옵션, 위치와 숫자 파싱, 충돌 규칙과 도움말 |
| [invocation.rs](../../crates/kodstar-cli/src/invocation.rs)와 [invocation/history.rs](../../crates/kodstar-cli/src/invocation/history.rs) | clap 값을 `Invocation`으로 번역한다. `ProjectOptions`, 행동·조회 옵션, 출력 형식을 구성한다. |
| [run.rs](../../crates/kodstar-cli/src/run.rs) | 최초 초기화 대상 확인, `Cancellation`, renderer와 `Engine::execute` 연결 |
| [confirmation.rs](../../crates/kodstar-cli/src/confirmation.rs), [signals.rs](../../crates/kodstar-cli/src/signals.rs), [telemetry.rs](../../crates/kodstar-cli/src/telemetry.rs) | 터미널 확인, 신호와 취소 수명, 진행 계측 경계 |
| [output.rs](../../crates/kodstar-cli/src/output.rs)와 [output/](../../crates/kodstar-cli/src/output) | `QuerySink`의 CLI 구현. 행 직렬화·버퍼링과 stdout/stderr·종료 상태 연결 |

`kodstar init`의 확인에는 엔진이 canonical 경로로 검사한 `InitializationTarget`을 사용한다.
`run.rs`는 확인한 경로를 요청에 명시적으로 넣는다. 설치·제거는 CLI의 [installation/](../../crates/kodstar-cli/src/installation)과
엔진의 [installation.rs](../../crates/kodstar-core/src/application/installation.rs)에서 별도 작업으로 연결한다.
설치 기록·실행 lease·프로젝트 등록과 제거는 [인프라 어댑터](../../crates/kodstar-core/src/infrastructure/installation)가 다룬다.

## 공통 실행기

[engine.rs](../../crates/kodstar-core/src/application/engine.rs)의 편의 메서드와
[commands.rs](../../crates/kodstar-core/src/application/execution/commands.rs)의 `Engine::execute`는
[execution::run](../../crates/kodstar-core/src/application/execution/mod.rs)으로 모인다.
`CommandHandler`가 `validate`, `plan`, `progress`, `execute`를 제공한다.
공통 실행 순서는 취소 확인 → 요청 검증 → `Context::prepare` → `Context::synchronize` → 실제 실행이다.

| 정책 | 공통 실행기가 준비하는 상태 |
|---|---|
| `Plan::Init` | 등록할 프로젝트와 현재 소스. 옵션에 따라 내용 확인 또는 전체 추출을 요청한다. |
| `Plan::Project` / `Diagnostics` | 등록 프로젝트와 현재 분석. 진단 모드는 갱신 실패 정보와 이전 정상 분석을 함께 보고한다. |
| `Plan::Revision` | 이름이나 ID로 해석한 고정 체크포인트와 과거 분석 슬롯 |
| `Plan::Optional` | 등록 프로젝트가 있으면 준비한다. 루트를 자동 탐색하는 상황에서 없으면 전역 보고를 계속한다. |
| `Plan::Watch` | 현재 프로젝트의 처음 동기화와 observer |
| `Plan::Detached` | 이력·캐시 관리처럼 해당 handler가 직접 선택하거나 전역에서 처리할 작업 |

[workspace/mod.rs](../../crates/kodstar-core/src/application/workspace/mod.rs)는 현재 위치와 루트를 canonical 경로로
확정하고 [settings.rs](../../crates/kodstar-core/src/application/workspace/settings.rs)가 필요한 설정과 예산을 합친다.
[prepare.rs](../../crates/kodstar-core/src/application/execution/prepare.rs)는 현재·과거 경로에 맞는 이력과 분석
연결을 연다. [state.rs](../../crates/kodstar-core/src/application/execution/state.rs)는 초기화·진단 의도와
현재·watch·과거 입력 상태를 구분한다.

[context.rs](../../crates/kodstar-core/src/application/execution/context.rs)가 설치 lease, 저장소, 캐시 lease와
연결의 수명을 보유한다. 일반 모드는 `capture::run`으로 원문을 보존하고 `Snapshot`을 분석에 넘긴다.
고정 시점은 이미 정한 `Snapshot`을 사용한다. 캐시만 읽는 모드는 준비된 세대와 이력 정보를 읽는다.
분석이 필요하면 writer를 획득해 [analysis::run](../../crates/kodstar-core/src/application/analysis/mod.rs)을 수행한다.

## 읽기 전환과 결과의 수명

[reads.rs](../../crates/kodstar-core/src/application/execution/reads.rs)는 조회 transaction을 시작한다.
[query::run](../../crates/kodstar-core/src/application/query/mod.rs)이 파일 ID와 메타데이터를 읽어 WAL의
스냅샷을 고정한다. 작업 중 파일 별칭이 바뀌더라도 이미 선택한 대상이 유지된다.
출력을 시작하기 전에 저장소 잠금을 놓고 읽기 transaction과 캐시 lease는 행을 읽는 동안 유지한다.
느린 stdout 소비자가 기다리는 동안 다른 명령이 저장소 잠금을 획득할 수 있는 경계다.

`QuerySink::begin`은 메타데이터, `row`는 행, `finish`는 요약을 받는다.
`progress`는 동기화나 조회 중의 진행 이벤트를 전달한다. writer 실패·요청 취소는 이 경계로 돌아와
현재 작업을 종료한다. 이력 전용 handler는 [execution/history.rs](../../crates/kodstar-core/src/application/execution/history.rs)에서
[application/history](history.md)로 위임하고 그 유스케이스가 자신의 읽기 transaction을 고정한다.
캐시 조회와 정리에는 [cache.rs](../../crates/kodstar-core/src/application/cache.rs)와
[maintenance.rs](../../crates/kodstar-core/src/application/maintenance.rs)가 별도 정책을 적용한다.

## watch를 따라갈 때

[watch.rs](../../crates/kodstar-core/src/application/execution/watch.rs)는 최초 분석 보고를 출력한 뒤
observer가 준 변경 신호를 모은다. [infrastructure/watch.rs](../../crates/kodstar-core/src/infrastructure/watch.rs)는
파일 감시와 제외 정책을 연결하고 오류를 재스캔 신호로 바꾼다. watch loop는 debounce와 주기 확인으로
`Context::synchronize`를 다시 호출한다. 동기화가 끝나면 저장소 잠금을 반환하고, 설치·캐시 lease는
감시 중 보유한다. 갱신 실패는 진단 행으로 보낸 뒤 다음 동기화 시점에 재시도할 수 있다.

## Stdio 전송

| 파일 | 역할 |
|---|---|
| [session/input.rs](../../crates/kodstar-cli/src/session/input.rs) | 취소 가능한 stdin 읽기와 입력 프레임 한도 |
| [session/protocol.rs](../../crates/kodstar-cli/src/session/protocol.rs), [session/query.rs](../../crates/kodstar-cli/src/session/query.rs) | 버전·ID·메서드·요청 데이터 검증과 엔진 타입 변환 |
| [session/mod.rs](../../crates/kodstar-cli/src/session/mod.rs) | 작업 큐·활성 요청 ID·취소 토큰과 EOF·shutdown 수명 |
| [session/worker.rs](../../crates/kodstar-cli/src/session/worker.rs) | 수락한 요청을 한 worker에서 실행하고 완료 상태 작성 |
| [session/output.rs](../../crates/kodstar-cli/src/session/output.rs), [session/frame.rs](../../crates/kodstar-cli/src/session/frame.rs) | 세션용 `QuerySink`, 데이터·제어 슬롯, 큰 프레임의 임시 파일 |
| [session/transport.rs](../../crates/kodstar-cli/src/session/transport.rs) | stdout writer 하나와 출력 정체·종료 시 배출 관리 |

세션 진입점은 설치 lease를 갖고 연결을 유지한다. 개별 요청의 프로젝트·이력·분석 수명은 실행기가 관리한다.
취소·종료 프레임은 작업 대기열과 분리해 현재 실행에 전달한다. 종료 시 작업과 writer를 join하고 프레임을
회수한다. stdin과 stdout의 크기·종료 제한 및 완료 이벤트는 [protocol 계약](../reference/output.md#stdio-프로토콜)을 따른다.
