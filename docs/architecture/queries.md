# 조회·검색·탐색 코드

[아키텍처](../architecture.md) · [명령 의미](../reference/cli.md#조회-명령) · [저장 인덱스](storage.md)

## 파일과 객체 선택

`execution::reads::Query`가 고정한 인덱스에서 [query/mod.rs](../../crates/kodstar-core/src/application/query/mod.rs)가
`Query` 종류를 분기한다. [selection.rs](../../crates/kodstar-core/src/application/query/selection.rs)는
`requested_files`에서 사용자가 요청한 경로를 파일 ID로 바꾼다. 현재 파일은 canonical 별칭을, 과거 파일은
보존 상대 경로를 사용한다. 객체는 파일 ID, 이름·한정 이름, 선택 위치와 해당 언어 이름 키로 좁힌다.

| 요청 | 실행 경로 |
|---|---|
| `symbols` | `selection::stream_symbols`가 파일의 실제 정의를 정렬해 보낸다. 대표 객체의 정의 위치는 필요할 때 읽는다. |
| `list` | [relationships.rs](../../crates/kodstar-core/src/application/query/relationships.rs)의 `files`가 방향별 관계를 다른 파일로 묶는다. |
| `search` | [search.rs](../../crates/kodstar-core/src/application/query/search.rs)가 이름 atom과 정수 ID로 후보를 좁힌다. |
| `related`, `deps`, `refs`, `path`, `impact` | 시드·필터·깊이를 준비하고 [traversal.rs](../../crates/kodstar-core/src/application/query/traversal.rs)로 넘긴다. |
| `explain` | 정의·양방향 사용·해석 규칙을 읽고 [documentation.rs](../../crates/kodstar-core/src/application/query/documentation.rs)가 문서 참조를 추가한다. |
| `overview`, `search --ranked` | 아래의 집계와 단어 색인 경로를 사용한다. |

모호한 선택은 `selection::select`에서 후보 정의를 `QuerySink`에 남기고 오류로 처리한다.
이름이 없더라도 파일 분석이 불완전하다면 실패 사유와 함께 선택 불가를 보고한다.
[metadata.rs](../../crates/kodstar-core/src/application/query/metadata.rs)는 이 분석 세대의 버전과
범위, 이력 연결을 공통 결과에 붙인다.

## 관계 탐색과 최단 경로

1. `traversal::setup`이 `q_seeds`, `q_edges`, frontier, 방문 집합을 준비한다.
   `q_allowed`가 관계 종류·해석 상태·외부 대상 포함 여부를 적용한다.
2. 파일·객체·멤버 시드를 대표 객체 ID로 맞춘다. [adjacency.rs](../../crates/kodstar-core/src/infrastructure/index/adjacency.rs)의
   `query_nodes`가 물리 ID에 대응하는 경로·위치·이름을 조회한다.
3. [memory.rs](../../crates/kodstar-core/src/application/query/traversal/memory.rs)가 제한된 이웃을
   `petgraph::Bfs`에 공급한다. [visited.rs](../../crates/kodstar-core/src/application/query/traversal/visited.rs)가
   Roaring 집합과 SQLite 방문 상태를 같은 계약으로 관리한다.
4. 메모리 한도에 닿으면 [disk.rs](../../crates/kodstar-core/src/application/query/traversal/disk.rs)가
   진행 중인 정점과 남은 대기열을 이어받는다. 방향과 깊이, 경로·위치·이름 순서,
   부모와 사용한 관계 ID를 보존한다.
5. [relationships.rs](../../crates/kodstar-core/src/application/query/relationships.rs)가 선택한 근거를 스트리밍한다.
   [relationship_rows.rs](../../crates/kodstar-core/src/application/query/relationship_rows.rs)는
   필요한 fact·객체·해석 규칙을 읽고 객체와 제한된 선취 배치를 재사용한다.

경로 탐색은 [path_neighbors.rs](../../crates/kodstar-core/src/application/query/traversal/path_neighbors.rs)에서
이웃마다 대표 사용 위치를 남겨 후보 정렬을 줄인다. 무방향 경로는
[undirected.rs](../../crates/kodstar-core/src/application/query/traversal/undirected.rs)가 부모 순서대로 양쪽
이웃을 확인하면서 현재 정점에서 목적지로 바로 갈 수 있는지 먼저 조회한다.
`restrict_to_path`는 목적지의 부모를 따라 근거만 추출한다. 방향이 같은 탐색과 방향 전환 탐색의
방문 단위·잘림 계약은 [CLI 참조](../reference/cli.md#조회-옵션)에서 확인한다.

## 단어 검색과 프로젝트 개요

[domain/query/terms.rs](../../crates/kodstar-core/src/domain/query/terms.rs)의 tokenizer를 색인 갱신과
조회에서 공유한다. [ranked_terms.sql](../../crates/kodstar-core/src/infrastructure/index/ranked_terms.sql)의
색인과 trigger는 이름·한정 이름·경로의 변경을 기록하고
[ranked_terms.rs](../../crates/kodstar-core/src/infrastructure/index/ranked_terms.rs)가 게시 전에 반영한다.

[ranked.rs](../../crates/kodstar-core/src/application/query/ranked.rs)는 단어 접두 범위에 해당하는 ID를
합쳐 점수화한다. 일치 단어 수, 전체 일치, 일치 필드와 접두 일치 등에 따른 가중치를 비교한다.
정렬이 필요한 상위 후보만 보유하고, 이어서 같은 세대에서 직접 관계를 읽는다.
방문 한도에는 일치 후보가 들어온다.

[overview.rs](../../crates/kodstar-core/src/application/query/overview.rs)는 선택한 파일을 임시 SQLite에 담아
모듈 행을 먼저 보낸다. 결과 요청이 계속되면 압축 관계 블록을 읽는다. 전체 범위는 순차 조회, 부분 범위는
선택 파일의 소스 정점에서 시작한다. 디렉터리와 패키지 경계는 분리해 집계하고 대표 근거 ID를 유지한다.
[cycles.rs](../../crates/kodstar-core/src/application/query/overview/cycles.rs)는 import 계열 파일 간선의
강연결 요소를 제한된 메모리에서 찾아 그룹과 예시를 만든다.

## 변경 영향과 설명의 출처

`impact --since`는 기본 한 세대 조회 앞에서
[history/impact.rs](../../crates/kodstar-core/src/application/history/impact.rs)로 분기한다.
이력의 변경 경로를 양쪽 인덱스에 투영해, 삭제 전 그래프와 변경 후 그래프에서 역방향으로 따라간다.
[impact/rows.rs](../../crates/kodstar-core/src/application/history/impact/rows.rs)는
`Query::Changes`용 시드를 사용하고 결과마다 어느 체크포인트의 근거인지 붙인다.
양쪽 연결의 환경을 비교하고 각각의 읽기 세대를 고정한 뒤 출력한다.

`explain`은 분석의 사용 위치에 이어 보존 원문을 사용한다.
[parse/documentation.rs](../../crates/kodstar-core/src/infrastructure/parse/documentation.rs)가 주석 참조를
구문 사실로 수집하고, 조회 시 [documentation/markdown.rs](../../crates/kodstar-core/src/application/query/documentation/markdown.rs)가
조회 체크포인트의 문서에서 제목·anchor를 확인한다. 작업 파일을 나중에 편집한 경우에도 설명은
해당 분석과 같은 시점의 파일 위치를 사용한다.
