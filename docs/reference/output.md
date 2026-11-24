# 출력과 도구 연동

[기능 명세](../../kodstar-spec.md) · [CLI 조회 예제](cli.md#조회-명령) · [전송 구현](../architecture/execution.md#stdio-전송)

## CLI 출력

| 형식 | 읽는 방법 |
|---|---|
| `text` | 프로젝트·세대·최신성을 확인한 뒤 정의 `definition`과 관계 사용 위치 `use`를 읽는다. |
| `json` | 공통 메타데이터, `results` 배열, 마지막 집계 필드를 한 객체로 읽는다. |
| `jsonl` | `type`이 `metadata`, `result`, `summary`인 평면 JSON 행을 순서대로 읽는다. 마지막 `summary`가 완료 판단에 필요하다. |
| `tsv` | 탭으로 구분한 결과 행을 파이프나 후처리에서 읽는다. |

### 진행과 진단

CLI는 결과를 stdout에, 단계별 진행과 진단을 stderr에 쓴다. `--verbose` 기본값은 `true`이며
값을 명시한다. `false`는 일반 진행·시간·개수 계측을 끈다. 오류, 분석 제한과 미완료 경고는 항상 보고한다.
리다이렉션과 파이프에도 같은 규칙을 적용한다. 진행에서는 현재 단계와 작업 파일을, 계측에서는 단계별 시간과 개수를 확인할 수 있다.
stdio 요청의 구조화된 진행 이벤트는 별도의 protocol 계약을 따른다.

### 공통 메타데이터

| 필드 | 의미 |
|---|---|
| `schema_version` | CLI 결과 형식 버전 2 |
| `relation_registry_version`, `engine_version` | 관계 코드의 registry 버전, 분석 엔진 버전 |
| `root`, `root_encoding` | 루트의 표시 경로와 필요할 때 제공하는 손실 없는 경로. 전역 보고는 null |
| `generation`, `indexed_at` | 분석 세대와 게시 시각. 인덱스 없는 결과에서는 null |
| `freshness` | 변경 확인 수준 또는 고정 시점. 값은 [저장 계약](storage.md#최신성과-과거-조회)에 정의한다. |
| `coverage`, `unresolved_count` | 관측 범위와 미해결·모호한 구문 사용 수 |
| `history` | 저장소와 현재·분석 체크포인트 및 비교 정보. 해당 상태가 없으면 null |
| `maintenance` | 자동 캐시 관리 보고. 적용된 보고가 없으면 null |
| `complete`, `truncated`, `reasons` | 완료 여부, 출력·작업 한도로 잘렸는지 여부, 관련 이유 |
| `result_count`, `visited_count` | 반환한 행 수와 요청에서 정의한 탐색 단위의 방문 수 |

`complete=true`는 선택한 인덱스·필터·한도에서 요청을 완료했음을 뜻한다. 관계 그래프의 정적 근거와
언어별 범위는 [분석 계약](analysis.md)에 정의한다. `history`는 `repository`, `branch`, `checkpoint`,
`created`, `changed_files`, `tracked_files`, `analysis_checkpoint`, `analysis_error`, `comparison`을 가진다.
비교는 실제 `from`과 `to`를 보고하며 그래프·변경 영향 비교는 양쪽 coverage, 설정 fingerprint, 추출 버전을 포함한다.

### 범위와 행 타입

| 필드 | 읽는 기준 |
|---|---|
| `coverage.scanned_files`, `source_bytes` | 스캔한 파일과 집계한 소스 바이트 |
| `analyzed_files`, `parsed_files` | 전자는 지원 언어·컴포넌트 분석 파일, `init`의 후자는 보조 입력을 포함해 이번에 추출한 수 |
| `unsupported_files`, `skipped_files` | 지원 밖 입력과 크기·실패 등에 따라 건너뛴 입력 |
| `partial_files`, `failed_files` | 구문 복구·문장 매크로 미확인 파일과 입력·인코딩·작업 한도로 처리에 실패한 파일. 실패는 스킵에 포함된다. |
| `fragment_files` | 독립 선언 해석이 어려운 매크로 치환 조각이 있는 파일 |
| `configuration_incomplete_files` | 상속·참조 구성을 사용할 수 없는 설정 파일 |
| `native_incomplete_files`, `native_limited_files` | 선택적 C/C++ 의미 근거의 미완료와 한도 파일 |

부분 구문, 실패, 치환 조각, 설정 누락은 `complete=false`와 구체적인 사유를 낸다. native 경고만 있는
경우 구문 조회를 계속 완료할 수 있다. 불완전 파일에서 찾지 못한 객체는 `source_unavailable`로 보고한다.
잘못된 JS/TS 설정은 `configuration_invalid`로 격리한다. 다른 입력과 이미 보존한 소스는 계속 사용할 수 있다.

- 객체는 `definition`, 관계는 `site.file`과 `site.span`에서 위치를 읽는다.
- 파일 이력의 `kind`에는 `checkpoint`, `branch`, `change`, `diff_line`, `diff_omitted`, `operation`이 있다.
  `impact`는 근거 `checkpoint`와 `relationship`을, `graph_change`는 `change`, `before`, `after`를 가진다.
- 개요는 `module`, `module_dependency`, `package_dependency`, `hub`, `import_cycle`을 구분한다.
  `documentation_reference`는 주석 `source`, 문서 `target`, `resolution`과 미해결 `reason`을 갖는다.

위치 범위는 원본 바이트 오프셋의 `[start_byte, end_byte)`로 표현하고, 줄·열은 1부터 시작한다. 열의 단위는 원본 바이트다.
명시 인코딩에서 UTF-8로 변환한 입력도 원문 위치로 돌려놓는다.
비 UTF-8 경로는 표시값과 함께 platform 및 OS 바이트의 base64 표현을 제공한다.
조회별 정렬 키는 관계·경로·위치·한정 이름을 사용하며 탐색 깊이가 있는 조회는 깊이를 우선한다.

### 종료 코드

| 코드 | 의미 |
|---|---|
| 0 | 요청 완료. 존재하는 대상에 연결이 없는 빈 결과도 포함한다. |
| 1 | 작업·파일·저장소 오류 |
| 2 | 명령 또는 요청 인자 오류 |
| 3 | 한도 또는 범위 불완전. 결과를 반환하면 요약으로 이유를 확인한다. |
| 4 | 인덱스나 대상 없음, 또는 객체 선택 모호함 |
| 130 | 사용자 취소 |

파이프가 닫히면 출력 작업을 종료한다. JSONL 소비자는 파이프 종료와 마지막 요약의 도착 여부를 함께 확인한다.

## Rust API

`Engine`의 호출에는 `ProjectOptions`, `Command` 또는 개별 요청, `Cancellation`을 전달한다.
`Engine::execute` 및 조회 계열 메서드는 `QuerySink`에 메타데이터·행·진행·요약을 전달한다.
`Engine::init`은 `InitOptions`, 진행 콜백을 받아 `InitReport`를 반환하는 직접 진입점도 제공한다.
터미널 표시와 stdin/stdout 전송은 호출자가 소유한다.

공개 타입은 [lib.rs](../../crates/kodstar-core/src/lib.rs), [results.rs](../../crates/kodstar-core/src/domain/results.rs),
[history.rs](../../crates/kodstar-core/src/domain/history.rs)에 있다.

## Stdio 프로토콜

`kodstar serve --stdio`는 protocol 2의 UTF-8 JSON을 요청당 한 줄 읽는다.
`protocol_version`, `id`, `method`, 선택적인 `params`가 요청의 최상위 필드다.
요청 ID는 문자열 또는 정수이고 문자열은 1~256바이트다. 활성 요청에 ID를 재사용하면 코드 2다.

```jsonl
{"protocol_version":2,"id":"init-1","method":"init","params":{"project":{"root":"/work/project"}}}
{"protocol_version":2,"id":"q-1","method":"query","params":{"project":{"root":"/work/project"},"query":{"kind":"refs","file":"src/api.rs","name":"handle_request"},"options":{"refresh":"none","relations":["calls"]}}}
{"protocol_version":2,"id":"stop-1","method":"cancel","params":{"request_id":"q-1"}}
{"protocol_version":2,"id":"end-1","method":"shutdown"}
```

| method | params |
|---|---|
| `init` | 선택 `project`, `path`, `full`, `verify` |
| `query` | 필수 `query`, 선택 `project`, `options` |
| `history` | 필수 `request`, 선택 `project` |
| `status`, `doctor`, `languages` | 선택 `project`, `options.refresh` |
| `cache` | 필수 `action` 및 선택 `project`, `all`, `dry_run`. `list`에서 `options.refresh` 사용 가능 |
| `cancel` | 필수 `request_id` |
| `shutdown` | 생략 또는 빈 객체 |

`project`는 `root`, `config`, `jobs`, `memory_budget`, `max_file_bytes`를 받는다.
경로 기준은 세션 시작 위치이며 크기는 정수 바이트다. `cache.action`은 `list`, `clean`, `gc`다.

`query.kind`는 `symbols`, `list`, `related`, `deps`, `refs`, `explain`, `impact`, `changes`, `path`, `search`, `overview`다.
`changes.since`는 `impact --since`, `search.ranked: true`는 순위 검색에 해당한다.
객체의 `at`는 `[line, byte_column]`이고 경로 탐색의 `from`과 `to`는 `file::qualified-name` 문자열이다.
`options`는 CLI 의미를 가진 snake_case 필드와 `relations` 배열을 사용한다.
`revision`은 `--rev`, `direction: "undirected"`는 `path --undirected`에 해당한다.
`refresh` 값은 기본 `metadata`, `verify`, `none`이다.

`history.request.action`은 `list`, `save`, `diff`, `graph_diff`, `restore`, `upgrade`, `branches`,
`create_branch`, `switch_branch`, `delete_branch`다. 저장과 브랜치 변경은 `name`, 복원은 `revision`과
선택인 `path`, 두 diff는 함께 쓰는 `from`과 `to`를 받는다. `list`와 두 diff는 `refresh`를 받을 수 있다.
`graph_diff.limit: 0`은 출력 행 한도를 해제한다.

이벤트는 `protocol_version`, `id`, `type`을 공유한다. `metadata`, `result`, `progress`, `summary`의
내용은 `data`에 있고 객체 ID도 `data.id`에 있다. `error`는 최상위 `code`·`message`,
마지막 `complete`는 최상위 `exit_code`를 가진다. 호출자는 마지막 **`complete`**를 받고 상태를 확정한다.

입력 프레임은 최대 64 KiB다. 큰 프레임은 다음 줄까지 소비한 뒤 세션을 계속한다.
실행 worker는 하나, 대기 작업은 두 개이고 대기열 초과는 코드 3이다. 제어 요청은 대기열을 우회한다.
다른 protocol 버전·중복 활성 ID·잘못된 필드나 옵션은 코드 2로 처리한다.
stdout writer 하나가 결과·제어 프레임을 쓰며 활성 프레임은 각각 2개와 8개다.
프레임당 최대 64 KiB를 메모리에 두고 더 큰 출력은 0600 임시 파일을 통해 전달한다.
제어 큐까지 포화되면 오류로 세션을 종료한다.

EOF는 수락한 작업을 처리한 뒤 종료한다. `shutdown`과 SIGINT는 요청을 취소하고 worker 종료를 기다린다.
`shutdown`의 막힌 출력 배출 유예는 500ms다. 배출 실패·출력 단절·SIGINT가 있으면 완료 이벤트 도착 여부를
확인해야 한다. 요청이 분석하는 내용은 저장된 파일이며 편집기의 미저장 화면은 디스크에 저장한 뒤 조회한다.
