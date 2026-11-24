# 분석 인덱스·캐시·잠금

[아키텍처](../architecture.md) · [저장 계약](../reference/storage.md) · [원문 이력 구현](history.md)

## 디스크 상태의 소유자

| 데이터 | 관리 파일 | 사용 범위 |
|---|---|---|
| 현재 분석과 과거 분석 슬롯 | [index/mod.rs](../../crates/kodstar-core/src/infrastructure/index/mod.rs) | 루트 검증, SQLite 연결, format 확인과 SQL adapter 등록 |
| 논리 파일·정의·binding·fact 행 | [index/records](../../crates/kodstar-core/src/infrastructure/index/records), [index/schema.sql](../../crates/kodstar-core/src/infrastructure/index/schema.sql) | 물리 정수 행과 논리 view, 정체성·진단·통계 |
| 이력 원문과 복원 journal | [history/schema.sql](../../crates/kodstar-core/src/infrastructure/history/schema.sql) | 프로젝트에서 보존하는 source of truth |
| 최근 그래프 비교 | [application/history/graph/cache.rs](../../crates/kodstar-core/src/application/history/graph/cache.rs) | 한 체크포인트 쌍의 결과·대표 근거 |
| 캐시 디렉터리와 lease | [cache/mod.rs](../../crates/kodstar-core/src/infrastructure/cache/mod.rs), [paths.rs](../../crates/kodstar-core/src/infrastructure/cache/paths.rs) | canonical 경로 키, 캐시 기록과 최근 사용 갱신 |
| 자동 정리·삭제·공간 계산 | [application/maintenance.rs](../../crates/kodstar-core/src/application/maintenance.rs), [cache/catalog.rs](../../crates/kodstar-core/src/infrastructure/cache/catalog.rs), [removal.rs](../../crates/kodstar-core/src/infrastructure/cache/removal.rs) | 캐시 목록과 제한된 정리 후보, 사용 중 lease 보호 |
| 설치 및 등록 | [infrastructure/installation](../../crates/kodstar-core/src/infrastructure/installation) | 설치 receipt·실행 lease·프로젝트와 이력 identity 연결 |

`index.sqlite`와 `past.sqlite`는 동일한 분석 스키마를 사용한다. 어느 체크포인트를
읽었는지는 metadata에서 확인한다. 해석 도중 필요한 키·후보·탐색 상태는 해당 요청의 임시 테이블에 둔다.

## 정수 행, 사전, 압축 관계

[atoms](../../crates/kodstar-core/src/infrastructure/index/atoms)는 반복 문자열과 참조 계수를 관리한다.
[records](../../crates/kodstar-core/src/infrastructure/index/records)의 trigger와 view는 문자열을 정수 키로
저장하고 읽을 때 필요한 열을 복원한다. 객체 논리 키는 32바이트로 저장하고 공개 결과에서 16진수로
표현한다. 구문과 해석이 다른 값에만 resolved override를 두며, `symbol_aliases`에는 실제 대표 객체로
병합된 행을 저장한다. [definitions.rs](../../crates/kodstar-core/src/infrastructure/index/definitions.rs)와
[display_path.rs](../../crates/kodstar-core/src/infrastructure/index/display_path.rs)가 정의 위치와 표시 경로를 제공한다.

| 구조 | 동작 |
|---|---|
| [dictionary](../../crates/kodstar-core/src/infrastructure/index/dictionary) | lookup key를 접두어 압축한 블록에 두고 정수 ID에서 블록을 찾는 routing page를 유지한다. 큰 key는 내용 후보와 실제 원문을 대조한다. |
| [dependencies](../../crates/kodstar-core/src/infrastructure/index/dependencies) | 파일·lookup key의 소비 fact ID와 found bit를 차이·가변 길이 정수로 기록한다. 부분 공개는 바뀐 슬롯을 같은 페이지의 다른 소비자와 병합한다. |
| [relations](../../crates/kodstar-core/src/infrastructure/index/relations) | file/source별 관계 블록에 fact·ID 차이를 두고 반복 endpoint 그룹을 공유한다. 최대 128행의 블록에서 필요한 근거를 해독한다. |
| [relations/routing](../../crates/kodstar-core/src/infrastructure/index/relations/routing), [addresses.rs](../../crates/kodstar-core/src/infrastructure/index/relations/addresses.rs) | 대상·fact·설명 문자열에서 역조회 페이지를, 관계 ID에서 연속 ID 구간과 블록을 찾는다. |
| [packed](../../crates/kodstar-core/src/infrastructure/index/packed), [adjacency.rs](../../crates/kodstar-core/src/infrastructure/index/adjacency.rs) | virtual table 조건과 필요한 열을 디코딩·검색 경로에 전달하고 탐색 정점의 정렬 key를 읽는다. |
| [ranked_terms.rs](../../crates/kodstar-core/src/infrastructure/index/ranked_terms.rs) | 바뀐 객체와 파일 경로의 단어 색인을 현재 분석에 반영한다. |

같은 블록의 source·endpoint 반복을 줄여도 관계 ID와 사용 위치는 각각 보존한다.
대상·fact의 역조회와 블록 공유 참조는 원래 블록과 함께 transaction에서 갱신한다.
reader가 보유한 역조회 페이지는 연결의 쓰기·rollback 경계를 넘을 때 무효화된다.
정수 블록을 보여 주는 논리 view의 SQL 함수는 연결을 열 때 등록한다.
독립 DB 검사가 필요하면 [benchmarks/postings.py](../../scripts/benchmarks/postings.py)와 관련 decoder를 사용한다.

## 공개와 호환성

[analysis/mod.rs](../../crates/kodstar-core/src/application/analysis/mod.rs)에서 repository·목표 체크포인트·
환경 fingerprint·추출 버전·관계 registry를 확인한다. 정상 분석이 재사용 가능하면 기존 세대를 반환한다.
변경이 있으면 한 쓰기 transaction에서 파일 사실, 삭제, 구성, 이름 해석, 조회 색인, coverage와 metadata를
게시한다. [statistics.rs](../../crates/kodstar-core/src/infrastructure/index/statistics.rs)와
[statistics/](../../crates/kodstar-core/src/infrastructure/index/statistics)는 재사용 가능한 통계 또는
이전·이후 행의 제한된 차이를 읽는다. 변경량이 작업 예산을 넘으면 전체 집계를 사용한다.

부분 관계 게시에서는 이전과 동일한 결과를 확인할 때 해당 관계 ID와 블록을 재사용할 수 있다.
교체할 관계의 역조회·공유 참조를 같은 작업에서 반영한다. 실패·취소에서는 기존 인덱스로 rollback한 뒤
실패 상태를 기록한다. 먼저 커밋한 이력 체크포인트는 별도의 DB에 있다.
조회 transaction은 객체 선택과 출력이 진행되는 동안 공개 세대 하나를 읽는다.

[doctor 실행부](../../crates/kodstar-core/src/application/execution/reads.rs)는 이력 원문·참조, SQLite
무결성·FK, 사전과 압축 관계·lookup의 논리 참조를 확인한다. 분석 파일 형식과 추출 버전은
[index/mod.rs](../../crates/kodstar-core/src/infrastructure/index/mod.rs), 관계 registry는
[relations.rs](../../crates/kodstar-core/src/domain/relations.rs)에 있다.
사용자 캐시의 호환성과 명시적 재생성 절차는 [저장 계약](../reference/storage.md#어디에-무엇이-저장되는가)을 따른다.

## 잠금 획득과 해제

설치 lease → 이력 저장소 잠금 → cache lease 및 writer 순서로 작업을 준비한다.
cache catalog의 열기·삭제는 레지스트리 잠금과 캐시 lease로 연결된다.
이력 작성과 분석 작성은 각각의 저장 경계를 직렬화한다. 조회가 입력 경로와 WAL snapshot을 고정한 뒤
이력 잠금을 반환해도, cache lease는 읽는 DB가 제거되는 것을 막는다.
`watch`는 설치·캐시 lease를 유지하면서 동기화 단위로 이력 잠금을 획득한다.
[locks.rs](../../crates/kodstar-core/src/infrastructure/cache/locks.rs)와
[공통 실행 Context](execution.md#공통-실행기)를 보면 소유 범위를 확인할 수 있다.

## 자원 수명

| 자원 | 어디서 확보하고 회수하는가 |
|---|---|
| 입력·fact·대기열 permit | [budget.rs](../../crates/kodstar-core/src/application/analysis/budget.rs)가 바이트를 제한한다. 발견·worker·writer가 넘기는 값의 수명에 따라 반납한다. |
| AST·파서·조각 병합 | worker가 현재 파일의 원문과 tree를 들고 파일 종료 후 해제한다. parser 자체는 같은 worker에서 재사용한다. |
| SQLite page cache와 statement | [index/cached.rs](../../crates/kodstar-core/src/infrastructure/index/cached.rs)가 중첩 호출의 활성 statement를 유휴 풀과 구분한다. 반환 시 binding을 지우고 바이트를 확인한다. |
| 정렬·탐색 frontier·visited | 읽기 요청의 제한된 메모리와 임시 테이블을 사용한다. 해제는 scope와 connection 수명에 따른다. |
| 블록 reader·객체 선취 | 필요한 관계와 읽기 배치를 조회 작업 안에서 공유한다. [reader.rs](../../crates/kodstar-core/src/infrastructure/index/relations/reader.rs)와 [조회 행 소유자](queries.md#관계-탐색과-최단-경로)가 관리한다. |
| stdio worker·프레임 | session 소유. 종료·취소·전송 실패에서 작업을 거두고 출력 파일을 회수한다. |

분석 연결의 설정 cache는 기본 8 MiB이며 writer의 main·temp 합계는 이 예산의 1/4을 쓴다.
유휴 SQL은 최대 64개, 읽기 예산의 1/4과 1 MiB 중 작은 값으로 제한한다.
이력과 snapshot의 main·temp page cache 합계는 각각 4 MiB와 2 MiB다.
전체 설정은 동시에 살아 있는 입력·큐·DB cache와 worker 스택을 예약하는 경계다.
실제 RSS에는 parser·컴파일러·allocator 등의 사용량이 함께 반영된다.
