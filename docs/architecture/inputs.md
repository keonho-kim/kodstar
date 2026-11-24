# 파일 선택과 구문 추출

[아키텍처](../architecture.md) · [분석 입력 계약](../reference/analysis.md) · [이름 해석](resolution.md)

## 디스크에서 분석 작업까지

```mermaid
flowchart LR
    FS["작업 파일과 제외 규칙"] --> HISTORY["이력 수집"]
    HISTORY --> INPUT["체크포인트의 경로·원문"]
    INPUT --> DISCOVERY["변경 경로와 입력 종류 판정"]
    DISCOVERY --> WORKER["worker의 파일별 구문 추출"]
    WORKER --> WRITER["사실·파일 진단 저장"]
```

| 코드 | 읽는 것과 넘기는 것 |
|---|---|
| [scan/mod.rs](../../crates/kodstar-core/src/infrastructure/scan/mod.rs), [scan/policy.rs](../../crates/kodstar-core/src/infrastructure/scan/policy.rs) | 루트·ignore·제외에 따라 실제 파일과 symlink를 관찰한다. 감시도 같은 정책을 쓴다. |
| [history/inputs/](../../crates/kodstar-core/src/infrastructure/history/inputs) | 고정 체크포인트의 추적 경로와 blob을 제공한다. parser 입력의 크기와 해시는 보존 바이트로 확인한다. |
| [incremental/inputs.rs](../../crates/kodstar-core/src/application/analysis/incremental/inputs.rs) | 기존 분석과 목표 체크포인트의 차이에서 순회 범위를 선택한다. |
| [discovery.rs](../../crates/kodstar-core/src/application/analysis/discovery.rs) | 이전 `FileRecord`, 새 입력·제외·언어 판정·fingerprint에서 `Reuse`, `Skip`, `Remove` 또는 `Work`를 만든다. |
| [module_identity.rs](../../crates/kodstar-core/src/application/analysis/module_identity.rs) | 경로와 snapshot의 manifest 경계를 기준으로 파일의 package·module 값을 계산한다. |
| [languages/mod.rs](../../crates/kodstar-core/src/infrastructure/languages/mod.rs), [header.rs](../../crates/kodstar-core/src/infrastructure/languages/header.rs) | 확장자·특수 이름·shebang·공유 헤더의 언어와 맞는 Tree-sitter 자원을 선택한다. |

처음 분석하거나 패키지 경계가 바뀌면 필요한 전체 구성을 순회한다. 입력 차이가 작고 경계가 유지되면
바뀐 경로와 관측 의존성만 다시 읽을 수 있다. 변경된 크기 한도 내 `.h`는 전체 보존 입력을
판별에 사용하고 그 버퍼를 worker에 넘긴다. 같은 내용 해시의 헤더는 이전 언어 판정을 재사용할 수 있다.
지원 밖·크기 초과·보조 입력은 인덱스에서 각각의 상태를 갖는다. 추적과 복원 범위는 [이력 계약](../reference/storage.md#무엇이-체크포인트에-들어가는가)이 정한다.

## 파이프라인의 단일 작성자

[analysis/pipeline.rs](../../crates/kodstar-core/src/application/analysis/pipeline.rs)는 discovery와 worker를
scoped thread로 실행하고 자신이 보유한 분석 transaction에 이벤트를 쓴다.
[events.rs](../../crates/kodstar-core/src/application/analysis/events.rs)는 `Work`와 `Event`의 단위를 정의한다.
`Work`는 보존 입력과 record, 이전 fingerprint, 필요 시 추가 선언 증명 자료와 입력 permit을 가진다.
[routing.rs](../../crates/kodstar-core/src/application/analysis/routing.rs)가 일반·native 큐를 구분한다.

[workers.rs](../../crates/kodstar-core/src/application/analysis/workers.rs)의 파일 작업은 Parser를 지연 초기화하고
입력 종류에 따라 구문·포맷 어댑터를 고른다. 사전 추출과 재사용 판단은
[workers/extraction.rs](../../crates/kodstar-core/src/application/analysis/workers/extraction.rs)와
[replay.rs](../../crates/kodstar-core/src/application/analysis/replay.rs)에서 연결한다.
작은 추출은 사실 배치를 재사용하고, 범위가 넓어지면 파일 수명의 AST로 다시 순회한다.
결과는 `Begin → Facts → End`의 파일 이벤트 또는 `UnchangedFacts`·`Failed` 등 유효한 종료 상태로 넘어간다.

[pipeline/start.rs](../../crates/kodstar-core/src/application/analysis/pipeline/start.rs)가 파일 재사용이나 교체 방식을 고르고
[pipeline/writer.rs](../../crates/kodstar-core/src/application/analysis/pipeline/writer.rs)가 binding·정의·scope·사용 사실,
위치와 추출·native 진단을 저장한다. [index/failures.rs](../../crates/kodstar-core/src/infrastructure/index/failures.rs)는
파일의 불완전 상태와 사실 제거를 담당한다. producer·worker가 실패하면 최초 원인을 전달하고
취소·큐 종료·join을 완료한다. 부분 구문은 진단을 단 세대의 입력으로, 실행 실패는 세대 transaction의
rollback 사유로 다룬다.

## 한 파일의 공통 구문 틀

| 코드 | 책임 |
|---|---|
| [parse/session.rs](../../crates/kodstar-core/src/infrastructure/parse/session.rs) | worker 단위 parser와 파일별 트리 재사용 |
| [parse/extraction.rs](../../crates/kodstar-core/src/infrastructure/parse/extraction.rs) | 디코딩·grammar 선택·투영·하이브리드 분석 결합 |
| [parse/walk.rs](../../crates/kodstar-core/src/infrastructure/parse/walk.rs), [context.rs](../../crates/kodstar-core/src/infrastructure/parse/context.rs) | AST 순회 경계에서 현재 노드·상위 노드·scope·소유자·조건을 관리 |
| [definitions.rs](../../crates/kodstar-core/src/infrastructure/parse/definitions.rs), [declarations.rs](../../crates/kodstar-core/src/infrastructure/parse/declarations.rs), [ownership.rs](../../crates/kodstar-core/src/infrastructure/parse/ownership.rs) | 선언을 찾아 파일 단위 ID와 객체·소유 범위를 구성 |
| [bindings.rs](../../crates/kodstar-core/src/infrastructure/parse/bindings.rs), [imports.rs](../../crates/kodstar-core/src/infrastructure/parse/imports.rs), [parameters.rs](../../crates/kodstar-core/src/infrastructure/parse/parameters.rs) | 선언 위치, import·alias, 매개변수 등 이름 선택의 입력 |
| [references.rs](../../crates/kodstar-core/src/infrastructure/parse/references.rs), [call_expressions.rs](../../crates/kodstar-core/src/infrastructure/parse/call_expressions.rs), [type_uses.rs](../../crates/kodstar-core/src/infrastructure/parse/type_uses.rs) | 구문 사용의 이름·식·종류와 원문 위치 |
| [domain/facts.rs](../../crates/kodstar-core/src/domain/facts.rs) | `Definition`, `Scope`, `Binding`, `Use`, `Module`의 공통 표현과 크기 계산 |

예를 들어 Python에서 `load()`라는 호출을 읽으면 현재 소유 함수 ID, scope, 식 `load`,
관계 종류 `calls`와 span의 `Fact::Use`를 발행한다. 다른 입력의 동명 정의 선택은 이후
[해석 단계](resolution.md)의 책임이다. 그 사이 파일 ID와 파일 내부 ID·언어를 함께 쓴다.

| 언어별 구현을 찾을 때 | 추출 코드에서 먼저 볼 곳 |
|---|---|
| Python·Ruby | [python/](../../crates/kodstar-core/src/infrastructure/parse/python), [ruby/](../../crates/kodstar-core/src/infrastructure/parse/ruby) |
| JS·TS | [javascript/](../../crates/kodstar-core/src/infrastructure/parse/javascript)와 [decorators.rs](../../crates/kodstar-core/src/infrastructure/parse/decorators.rs) |
| Rust·Go·Zig | [rust/](../../crates/kodstar-core/src/infrastructure/parse/rust), [go/](../../crates/kodstar-core/src/infrastructure/parse/go), [zig/](../../crates/kodstar-core/src/infrastructure/parse/zig) |
| Java·Kotlin·Scala | [java.rs](../../crates/kodstar-core/src/infrastructure/parse/java.rs), [kotlin/](../../crates/kodstar-core/src/infrastructure/parse/kotlin), [scala/](../../crates/kodstar-core/src/infrastructure/parse/scala) |
| C/C++·C#·PHP·Swift | [cxx/](../../crates/kodstar-core/src/infrastructure/parse/cxx), [csharp/](../../crates/kodstar-core/src/infrastructure/parse/csharp), [php/](../../crates/kodstar-core/src/infrastructure/parse/php), [swift/](../../crates/kodstar-core/src/infrastructure/parse/swift) |
| Elixir·Erlang·Common Lisp | [elixir/](../../crates/kodstar-core/src/infrastructure/parse/elixir), [erlang/](../../crates/kodstar-core/src/infrastructure/parse/erlang), [lisp/](../../crates/kodstar-core/src/infrastructure/parse/lisp) |

어떤 grammar 노드를 어느 정의·스코프로 읽는지는 [languages/syntax.rs](../../crates/kodstar-core/src/infrastructure/languages/syntax.rs)
및 extractor의 분기에서 확인한다. 구문 제공 범위를 바꿀 때는 언어별 테스트와 교차 파일 resolver를 함께 본다.

## 인코딩·혼합 구간·오류 복구

[parse/input/encoding.rs](../../crates/kodstar-core/src/infrastructure/parse/input/encoding.rs)의 입력 변환은
parser가 읽을 UTF-8과 원문 바이트의 대응을 갖는다. parse가 만든 범위를 `restore`로 돌려놓은 뒤
배치에 넣는다. [recovery.rs](../../crates/kodstar-core/src/infrastructure/parse/recovery.rs)는 오류가 있는 트리에서
선언부와 소유 범위를 검증하고 부분 진단을 남긴다. 불확실한 범위를 통과한 후보는 해석 단계에서 제한한다.

[parse/conditional](../../crates/kodstar-core/src/infrastructure/parse/conditional)은 조건 분기와 원본 위치를
유지하는 투영을 관리한다. `directives.rs`가 분기·조건의 범위를 추출하고 `choices.rs`와
`coverage.rs`가 필요한 후보와 덮인 영역을 관리한다. 파일별 병합이 메모리 예산을 넘으면
[spill.rs](../../crates/kodstar-core/src/infrastructure/parse/conditional/spill.rs)의 전용 SQLite에서 키를
비교한다. 조건·범위의 계약과 작업 한도는 [분석 참조](../reference/analysis.md#불완전-입력과-cc-의미-보강)에 있다.
C/C++ 구조 매크로 표식과 같은 입력의 제한된 재파싱은 [내장 Clang](native.md)과 연결된다.

[formats/components.rs](../../crates/kodstar-core/src/infrastructure/formats/components.rs)는 host 파일과
code range의 언어를 함께 제공한다. script 외 영역을 위치 보존 방식으로 가리거나 EJS·Astro 전용 추출기를 사용한다.
Blade·Razor는 필요한 parser wrapper를 추가하고 각 사실의 범위를 원래 위치로 환산한다.
[components/](../../crates/kodstar-core/src/infrastructure/formats/components)의 `scripts`, `references`, `events`와
포맷별 하위 모듈에서 템플릿 이름·이벤트·include와 server/client 영역을 찾을 수 있다.
[formats/htmx](../../crates/kodstar-core/src/infrastructure/formats/htmx)와
[parse/javascript/hono](../../crates/kodstar-core/src/infrastructure/parse/javascript/hono)는 정적 문자열·속성과
라우트 등록 사실을 추출한다. 최종 연결은 [HTTP resolver](resolution.md#언어와-포맷별-선택)에서 한다.

[formats/auxiliary](../../crates/kodstar-core/src/infrastructure/formats/auxiliary)는 manifest·정적 설정 사실을,
[xml.rs](../../crates/kodstar-core/src/infrastructure/formats/xml.rs)와
[dotnet.rs](../../crates/kodstar-core/src/infrastructure/formats/dotnet.rs)는 XML과 .NET 관련 입력을 투영한다.
[configurations.rs](../../crates/kodstar-core/src/application/analysis/configurations.rs)는 JS/TS 상속·참조
설정의 불완전 상태를 분석 transaction에서 공개한다. 파일 분류·구문 실패·프로젝트 구성의 진단이
서로 다른 이유를 갖는 것은 경계와 처리할 수 있는 나머지 입력이 다르기 때문이다.
