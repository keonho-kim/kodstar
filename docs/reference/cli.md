# CLI 명령과 설정

[기능 명세](../../kodstar-spec.md) · [실행 코드](../architecture/execution.md) · [출력 계약](output.md)

## 설치와 시작

`./install.sh`는 저장소의 고정 Rust 도구로 `dist` 빌드를 만들고 실행 파일을
`$HOME/.local/bin/kodstar`에 설치한다. 재실행하면 같은 경로에 새 빌드를 설치한다.
`./install.sh --binary PATH`는 현재 셸 위치에서 실행 파일을 선택해 바로 설치한다.
설치는 홈에 설치 기록과 잠금을 준비하고 sh·Bash·Zsh 또는 해당하는 Fish 설정에 관리 PATH 블록을 등록한다.
전용 빌드 디렉터리는 스크립트가 종료할 때 회수한다. 준비할 도구와 검증 방법은 [개발 문서](../development.md)에 있다.

| 명령 | 동작 |
|---|---|
| `kodstar init [PATH]` | 지정 위치 또는 현재 디렉터리에 이력과 첫 분석을 생성한다. 등록된 루트에서는 관찰한 변경을 반영한다. |
| `kodstar init --full` | 등록된 소스를 다시 추출해 완성된 분석을 새 세대로 공개한다. |
| `kodstar init --verify` | 동기화할 파일의 내용 해시를 확인한다. 같은 입력은 기존 분석을 재사용할 수 있다. |
| `kodstar -h` / `kodstar --help` | 짧은 도움말과 예시가 있는 상세 도움말. 하위 명령에서도 사용할 수 있다. |
| `kodstar --version` | CLI와 엔진 버전을 출력한다. |
| `kodstar --licenses` | 실행 파일에 묶인 제삼자 고지를 출력한다. |
| `kodstar uninstall` | 확인 후 설치 파일, 관리 PATH, 기본 설정, 캐시, 등록부와 등록 프로젝트의 이력을 제거한다. |

첫 초기화는 canonical 경로와 소스 저장 위치를 알리고 확인을 받는다. 기본 응답은 거절이다.
비대화형 호출은 `--yes`를 사용한다. Rust API와 stdio 호출자는 초기화 의사를 확인한 뒤 호출한다.
파일시스템 루트 초기화는 모든 진입점에서 거절한다. `init PATH`와 `--root`를 함께 쓰면 같은 디렉터리여야 한다.
유효한 이력으로 등록 여부를 확인하며, 이미 등록된 프로젝트는 반복 확인 없이 갱신한다.
도움말·버전·라이선스는 상태를 준비하는 경로 앞에서 출력된다.

제거도 기본 거절이며 자동화에는 `--yes`를 쓴다. 설치에 연결된 이력의 원본 작업 파일, 개발용 checkout,
`target`, toolchain과 별도 설정 파일은 보존 대상이다. 등록부가 가리키는 이력을 대상으로 삼으며
이동·접근 불가·identity 불일치, 사용 중인 lease, 알 수 없는 저장 내용과 수정된 설치 흔적은 진단한다.
이동한 프로젝트를 새 위치에서 사용하면 등록부가 해당 경로를 확인한다. 미완료 정리는 오류로 보고한다.

## 조회 명령

| 명령 | 질문과 반환 범위 |
|---|---|
| `list FILE` | 파일과 직접 연결된 다른 파일은 무엇인가. 다른 파일의 경로·방향·관계 종류별 건수를 표시한다. 기본 방향은 양쪽이다. |
| `symbols FILE` | 이 파일에 무엇이 정의됐는가. 객체의 종류·이름·소유 범위·위치를 표시한다. |
| `related FILE NAME` | 이 객체와 어떤 관계가 있는가. 기본 깊이 1에서 들어오는 관계와 나가는 관계를 구분한다. |
| `deps FILE NAME` | 이 객체가 무엇을 사용하는가. 나가는 관계를 조회한다. |
| `refs FILE NAME` | 누가 이 객체를 사용하는가. 들어오는 관계를 조회한다. |
| `explain FILE NAME` | 어떤 정의·사용 위치와 해석 규칙이 이 연결을 뒷받침하는가. 후보와 미해결 이유를 포함한다. |
| `impact FILE [NAME]` | 무엇을 검토해야 하는가. 객체 또는 파일의 정의 집합에서 역방향으로 탐색하고 선택 객체의 멤버를 시드에 포함한다. |
| `impact --since REV` | 체크포인트 이후 무엇이 영향을 받았는가. 변경 전후 분석을 사용해 삭제된 정의의 기존 사용처도 탐색한다. |
| `path FROM TO` | 두 객체가 어떻게 이어지는가. 조건을 만족하는 최단 경로 하나의 근거 위치를 반환한다. |
| `search NAME` | 이름과 일치하는 정의는 무엇인가. `--prefix`로 접두 이름을 검색한다. |
| `search WORDS --ranked` | 입력 단어에 가까운 정의는 무엇인가. 순위에 따라 최대 8개 정의와 직접 관계를 출력한다. |
| `overview` | 분석 범위의 구조는 어떤가. 디렉터리, 디렉터리·패키지 간 관계, 사용처가 많은 객체와 import 순환을 보여준다. |

서로 다른 정의가 같은 이름을 사용할 수 있다. 한정 이름 또는 `--at LINE:BYTE_COLUMN`으로 좁힌다.
모호한 선택은 후보와 위치를 출력하고 코드 4로 종료한다. `path`의 양쪽 인자는 `FILE::NAME`이다.
이름의 `::`는 유지되므로 `'src/api.rs::Api::load'`처럼 쓸 수 있다. 공백이 있는 값은 셸에서 인용한다.

```bash
kodstar related src/order.rs OrderService::create_order
kodstar refs src/order.rs create_order --sites
kodstar related src/order.rs create_order --transitive --all --max-nodes 0 --format jsonl
kodstar path 'src/api.rs::Api::load' 'src/store.rs::fetch' --undirected
```

`list`는 같은 파일에서 발생하는 내부 사용을 제외한다. `related --direction both`의 전이 조회는
정방향과 역방향 도달 집합을 따로 탐색한다. `path --undirected`는 매 단계에서 방향을 바꿀 수 있어
`A → C ← B` 같은 경로를 찾고 단계의 실제 방향을 기록한다. 기본 깊이는 `related`·`deps`·`refs`에
1, `impact`·`path`에 제한 없음이며 방문·출력·작업 예산을 함께 적용한다.

`overview`의 객체 사용 수는 서로 다른 소스 객체 수다. 파일 import 순환은 각각 양방향으로 도달할 수 있는
파일 그룹이며 최대 32개 경로 예시와 전체 파일 수를 표시한다. 출력 한도가 디렉터리 행에서 끝나면
관계 집계가 필요 없다. 순위 검색은 식별자·한정 이름·경로에 일치하는 단어를 점수화한다.
밑줄과 대소문자 경계로 단어를 나누며 입력은 256바이트 이내여야 한다.

`explain`은 객체 안이나 바로 앞 독립 주석의 `ADR:`, `RFC:`, `@adr`, `@rfc` Markdown 참조도 표시한다.
`./`와 `../`는 주석 파일의 위치를, 나머지 상대 경로는 프로젝트 루트를 기준으로 한다.
조회 세대의 체크포인트에 저장된 일반 UTF-8 Markdown 파일에서 첫 제목이나 지정 anchor를 찾는다.
파일당 한도는 256 KiB와 파일 분석 한도 중 작은 값, 전체 한도는 1 MiB와 메모리 예산의 1/16 중 작은 값이다.
경로나 제목을 찾지 못하면 출처 위치와 이유를 남긴다.

## 이력·상태·캐시 명령

| 명령 | 동작 |
|---|---|
| `history` | 현재 브랜치 조상 체크포인트를 최신 순으로 표시한다. |
| `history save NAME` | 현재 변경을 내용 해시로 확인해 보존하고 체크포인트에 이름을 부여한다. |
| `history diff` | 최신 체크포인트와 부모의 파일 차이를 표시한다. 첫 체크포인트는 빈 상태와 비교한다. |
| `history diff --from REV --to REV` | 지정한 두 체크포인트의 파일 차이를 읽는다. |
| `history diff --graph` | 선택한 체크포인트 쌍의 관계 변화와 대표 근거를 반환한다. |
| `history restore REV [PATH]` | 작업 파일을 별도로 보존한 뒤 전체 범위나 지정 경로를 복원한다. |
| `history upgrade` | 보존 내용을 검증하면서 지원 형식을 전환하거나 현재 형식을 재압축한다. |
| `branch` | 브랜치·끝 체크포인트·현재 브랜치를 표시한다. |
| `branch create NAME` | 현재 변경을 보존한 체크포인트를 가리키는 브랜치를 만든다. 기존 브랜치에서 작업을 계속한다. |
| `branch switch NAME` | 현재 변경을 보존한 뒤 대상 브랜치의 파일과 참조를 확인해 전환한다. |
| `branch delete NAME` | 현재 브랜치 이외의 참조를 삭제한다. |
| `status` | 현재 소스 상태와 분석 세대·범위·최신성을 보고한다. |
| `doctor` | 이력과 인덱스의 무결성, 호환성, 분석 실패와 누락을 진단한다. |
| `languages` | 실제 지원 입력과 관계별 capability를 출력한다. |
| `cache list` | 분석 캐시의 위치·크기·최근 사용·사용 중 여부를 표시한다. |
| `cache clean [--all]` | 선택한 프로젝트 또는 전체의 유휴 분석 캐시를 삭제한다. |
| `cache gc [--dry-run]` | 수명·용량 정책에 따라 유휴 캐시를 정리한다. `--dry-run`은 후보만 보여 준다. |
| `watch` | 실행 중 저장된 파일의 변경을 확인하고 체크포인트와 분석을 갱신한다. |
| `serve --stdio` | 여러 요청을 실행하고 진행과 결과를 받는 [도구 연동 세션](output.md#stdio-프로토콜)을 연다. |

`history diff --from`과 `--to`는 함께 지정한다. 고정 비교에서는 저장된 입력을 사용하며
`--verify` 조합은 거절한다. `history diff --graph`의 `--limit N`과 `--all`은 변화 행 수를 제어한다.
`cache clean --all`과 `--root`는 충돌한다. `cache gc`는 환경 전체에 적용되는 캐시 정책을 읽는다.
파일 diff·그래프 diff·복원의 비교 단위와 실패 계약은 [이력과 저장소](storage.md)에 있다.
`history diff --root PATH`처럼 옵션은 대상 하위 명령 뒤에 붙인다.

## 조회 옵션

| 옵션 | 적용 의미 |
|---|---|
| `--root PATH`, `--config FILE` | 프로젝트 루트 또는 명시적인 TOML 설정 선택. |
| `--no-refresh`, `--verify` | 변경 확인 생략 또는 내용 해시 확인. 두 옵션은 충돌한다. |
| `--rev REV` | `list`부터 `overview`까지 위 조회 표의 분석 명령에서 저장한 체크포인트 선택. `impact --since`와 충돌한다. |
| `--direction in/out/both` | 관계 방향을 지정한다. `deps`는 `out`, `refs`와 `impact`는 `in`을 사용한다. |
| `--undirected` | `path`에서 단계별 방향 전환을 허용한다. `--direction`과 충돌한다. |
| `--relation KIND` | 관계 종류로 필터한다. 반복한 값의 합집합을 사용한다. |
| `--include-candidates`, `--include-external` | 후보·모호한 관계 또는 외부 대상을 포함한다. |
| `--depth N`, `--transitive` | 깊이 상한 지정 또는 해제. 두 옵션은 충돌한다. |
| `--members`, `--sites` | 해당 객체의 멤버를 시드에 넣거나 사용 위치를 개별 행으로 출력한다. |
| `--format text/json/jsonl/tsv` | stdout 형식 선택. 기본값은 `text`다. |
| `--limit N`, `--all` | 양수 출력 행 상한 지정 또는 해제. 두 옵션은 충돌한다. |
| `--max-nodes N` | 탐색 객체, 순위 검색의 일치 후보, 개요 파일의 방문 상한. 0은 이 상한의 해제다. |
| `--jobs N`, `--memory-budget SIZE`, `--max-file-bytes N` | 지원하는 명령에서 분석 자원을 덮어쓴다. 메모리 크기는 `256MiB` 형식으로 입력할 수 있다. |
| `--verbose true/false` | [일반 진행과 시간 표시](output.md#진행과-진단)를 제어한다. 하위 명령 앞과 뒤에 둘 수 있다. |

각 명령이 노출하는 옵션은 `--help`에 나온다. `--all`은 출력 행 한도만 해제한다.
`--rev --no-refresh`는 해당 체크포인트의 사용 가능한 분석 캐시를 요구한다. `--since`는 파일·객체 시드와 충돌한다.
`--prefix`와 `--ranked`는 각각의 검색 모드이며 동시에 지정하면 인자 오류다.

## 루트와 설정

명시 루트가 없으면 현재 디렉터리부터 가장 가까운 등록 상위 루트를 찾는다. 최초 `init`은 현재
디렉터리를 기본 루트로 사용한다. 각 경로를 canonical 형태로 확인해 symlink 별칭을 처리한다.
파일 인자는 셸의 현재 디렉터리를 기준으로 해석하고, 결과 경로는 프로젝트 상대 경로로 출력한다.
같은 이름의 디렉터리라도 실제 루트가 다르면 별도 캐시를 사용한다. 역사 조회의 경로 선택은
[보존한 시점](storage.md#최신성과-과거-조회)을 기준으로 한다.

설정 적용 순서는 내장 기본값, `$HOME/.kodstar/config.toml`, `--config` 파일, 지원 CLI 옵션이다.
명시 파일의 경로는 현재 위치 기준이며 실제로 존재해야 한다. 프로젝트 설정도 필요하면
`--config ./kodstar.toml`로 선택한다. 설정 파일은 UTF-8 TOML이고 각 파일을 최대 1 MiB 읽는다.

| 설정 키 | 기본값과 단위 |
|---|---|
| `exclude`, `language_overrides` | 빈 목록과 빈 경로 glob 매핑 |
| `jobs` | 2개. 1~64개 허용 |
| `memory_budget`, `max_file_bytes` | 256 MiB, 2 MiB |
| `fact_batch_bytes`, `queue_bytes`, `db_cache_bytes` | 1 MiB, 8 MiB, 8 MiB |
| `limit`, `max_nodes` | 200행, 100,000개 방문 |
| `cache_ttl_days`, `cache_budget_bytes`, `auto_gc_interval_seconds` | 30일, 2 GiB, 86,400초 |

`*_bytes`와 `memory_budget`의 TOML 값은 바이트 정수다. 동시 입력·사실·큐·DB cache·스택을
예약할 수 있어야 하며, `fact_batch_bytes`가 `queue_bytes`보다 크면 설정 오류다.
세부 자원 소유권은 [저장 구현](../architecture/storage.md#자원-수명)에 설명한다.
기본값과 검증의 구현은 [settings.rs](../../crates/kodstar-core/src/application/workspace/settings.rs)에 있다.
