# 이름 해석과 증분 갱신

[아키텍처](../architecture.md) · [구문 사실](inputs.md) · [관계와 상태 계약](../reference/analysis.md#객체와-관계의-의미)

## 사실을 관계에 연결하는 순서

[analysis/mod.rs](../../crates/kodstar-core/src/application/analysis/mod.rs)의 작성 transaction에서
파일 사실과 설정을 준비한 다음 [resolve/mod.rs](../../crates/kodstar-core/src/application/analysis/resolve/mod.rs)의
`run`을 호출한다. 해석 결과와 조회 의존성을 같은 transaction으로 게시한다.

1. [preparation.rs](../../crates/kodstar-core/src/application/analysis/resolve/preparation.rs)가
   필요한 구문 정체성·소유자 힌트·namespace 경로를 준비한다. [identities.rs](../../crates/kodstar-core/src/application/analysis/resolve/identities.rs)가
   동일 객체의 여러 정의를 대응한다.
2. [staging.sql](../../crates/kodstar-core/src/application/analysis/resolve/staging.sql)과
   [prepared/](../../crates/kodstar-core/src/application/analysis/resolve/prepared)가 후보·의존성·게시 작업
   집합과 재사용할 SQL을 만든다. C/C++ 보강, HTTP 등록의 투영, Lisp package 등의 필요한 환경을 준비한다.
3. [facts.rs](../../crates/kodstar-core/src/application/analysis/resolve/facts.rs)와
   [fact_batch.rs](../../crates/kodstar-core/src/application/analysis/resolve/fact_batch.rs)가 상속·구현·mixin을
   먼저 처리하고 나머지 사용을 제한된 배치로 읽는다. 중첩 조회 전에 바깥 cursor를 닫는다.
4. lexical·import·소유자·receiver 근거를 순서와 언어 규칙에 따라 확인한다. 대상 ID 또는 외부 key에
   해석 상태·규칙·미해결 이유를 붙여 [publication.rs](../../crates/kodstar-core/src/application/analysis/resolve/publication.rs)에 넘긴다.
5. 영향을 받는 [dispatch.rs](../../crates/kodstar-core/src/application/analysis/resolve/dispatch.rs)와
   [go/interfaces.rs](../../crates/kodstar-core/src/application/analysis/resolve/go/interfaces.rs)의 구현·override·호출
   후보를 계산한다. Go 구현 후보는 소비하는 Go dispatch 전에 게시한다.

바로 다음 해석에서 사용하는 기반 관계는 해당 시점에 기록한다. 파생 override·dispatch 후보는
디스크 staging에 모아 구간별로 게시한다. 작은 scope 조회는 현재 작업에서 즉시 사용될 수 있어
의존성을 바로 기록하고, 최종 expression 의존성은 제한된 배치로 비운다.

## 어떤 근거를 사용하는가

| 코드 | 선택하는 근거 |
|---|---|
| [lexical.rs](../../crates/kodstar-core/src/application/analysis/resolve/lexical.rs), [bindings.rs](../../crates/kodstar-core/src/application/analysis/resolve/bindings.rs), [lookup_context.rs](../../crates/kodstar-core/src/application/analysis/resolve/lookup_context.rs) | 파일·언어·scope·위치·namespace 내 바인딩과 shadowing의 우선순위 |
| [scope_imports.rs](../../crates/kodstar-core/src/application/analysis/resolve/scope_imports.rs), [modules/](../../crates/kodstar-core/src/application/analysis/resolve/modules) | import·별칭·재수출, 모듈 경로·owner와 가시성 |
| [namespaces.rs](../../crates/kodstar-core/src/application/analysis/resolve/namespaces.rs), [namespaces/cxx.rs](../../crates/kodstar-core/src/application/analysis/resolve/namespaces/cxx.rs) | Rust·C#·PHP·Elixir의 준비된 namespace 경로와 C++의 정규화 정수 이름 조회 |
| [members.rs](../../crates/kodstar-core/src/application/analysis/resolve/members.rs), [field_types.rs](../../crates/kodstar-core/src/application/analysis/resolve/field_types.rs), [returns/](../../crates/kodstar-core/src/application/analysis/resolve/returns) | 멤버 소유 타입, 상속, 확인할 수 있는 반환 기반 receiver |
| [qualified.rs](../../crates/kodstar-core/src/application/analysis/resolve/qualified.rs), [callable_values.rs](../../crates/kodstar-core/src/application/analysis/resolve/callable_values.rs), [builtins.rs](../../crates/kodstar-core/src/application/analysis/resolve/builtins.rs) | 한정 이름, 전달된 함수 값, 내장 이름 여부 |
| [source_quality.rs](../../crates/kodstar-core/src/application/analysis/resolve/source_quality.rs) | 부분 복구·실패한 근거를 사용한 대상의 확정 수준 |

lexical 조회 재사용은 한 해석 작업 안의 동등한 위치·scope와 언어 문맥에 적용한다.
재사용한 경우에도 호출별로 선택한 이름과 조회 의존성을 기록한다.
모듈 후보는 언어·패키지·정규화한 이름으로 좁힌다. 모듈 조회가 깊이별 후보·owner 작업 공간을 관리해
다른 lookup의 임시 행이 섞이는 것을 막는다. 중첩 해석은 서로 다른 깊이를 사용한다.

### 언어와 포맷별 선택

| 사례 | 해석 파일 | 확인하는 경계 |
|---|---|---|
| Rust의 `use`, raw 이름, `Self`·macro namespace | [rust/](../../crates/kodstar-core/src/application/analysis/resolve/rust) | 파일 module·owner와 type/value/macro의 구분 |
| Python의 import·`self`·상속 | [python/](../../crates/kodstar-core/src/application/analysis/resolve/python) | 모듈·class MRO·scope binding |
| JS/TS의 ESM·CommonJS·workspace alias | [javascript/](../../crates/kodstar-core/src/application/analysis/resolve/javascript) | 가장 가까운 설정, 패키지·export, 경로 후보 순서와 실패한 조회 |
| Go package·지역 replace·interface | [go/](../../crates/kodstar-core/src/application/analysis/resolve/go) | workspace·모듈 provider와 receiver method set |
| Java의 overloaded 메서드 | [java.rs](../../crates/kodstar-core/src/application/analysis/resolve/java.rs) | 후보의 인자 수·최소 varargs 수를 확인한 뒤 남은 후보를 판정 |
| C/C++의 include·전방 선언·오버로드 | [cxx/](../../crates/kodstar-core/src/application/analysis/resolve/cxx), [clang.rs](../../crates/kodstar-core/src/application/analysis/resolve/clang.rs) | 가시 include, 소유 및 native callee의 선언 위치 |
| PHP trait·Ruby 동적 소유·Swift extension 등 | [php/](../../crates/kodstar-core/src/application/analysis/resolve/php), [ruby/](../../crates/kodstar-core/src/application/analysis/resolve/ruby), [swift/](../../crates/kodstar-core/src/application/analysis/resolve/swift) | 각 언어의 소유·다중 선언과 후보 판정 |
| HTML·JSX htmx와 Hono 라우트 | [http/](../../crates/kodstar-core/src/application/analysis/resolve/http), [domain/web/](../../crates/kodstar-core/src/domain/web) | 앱의 lexical origin, export와 mount, 메서드·정규 경로·HTTP 후보 |

예를 들어 `src/page.html`의 `hx-get="/items"`는 위치를 가진 요청 fact로 들어온다. Hono에서
`app.get("/items", handler)`의 앱 생성과 import가 확인되면 등록을 메서드·경로 색인에 올린다.
`http/requests.rs`가 같은 메서드·경로의 후보를 읽어 요청과 등록 근거를 연결한다.
이 관계의 근거는 HTTP 메서드, 정규 경로와 확인된 앱 등록이다.

## 재분석 범위가 결정되는 이유

[incremental.rs](../../crates/kodstar-core/src/application/analysis/incremental.rs)는
`Resolution::None`, `Selected`, `All`로 해석 범위를 표현한다.
[index/invalidation.rs](../../crates/kodstar-core/src/infrastructure/index/invalidation.rs)와
[index/dependencies/](../../crates/kodstar-core/src/infrastructure/index/dependencies)가 provider의 성공·실패
lookup key, 소비 fact, 삭제될 대상의 역참조를 관리한다. `affected_facts`는 다시 해석할 소비 사실,
`dependent_files`는 lookup 데이터를 검사할 파일 범위다.

| 변경 증거 | 작업 |
|---|---|
| 체크포인트·환경·버전과 정상 세대가 일치 | 세대와 coverage를 재사용한다. |
| 내용이 바뀌어도 유효한 사실·위치가 같거나 scope 끝만 바뀜 | 확인된 위치·file metadata를 갱신하고 기존 관계를 보존한다. |
| 정의·scope·binding·기반 사실이 유지된 본문 수정 | 호출·참조와 파생 호출 후보를 비교하고 변경된 사실을 다시 해석한다. |
| 안전 조건을 충족한 선언 추가·삭제·이름 변경 | 대상 참조와 성공·실패 lookup key에 연결된 소비 사실을 선택한다. |
| 타입·method set·프로젝트 구성·검증되지 않은 의존 경계 변화 | 관계 해석 범위를 전체로 확장한다. |

본문은 [domain/facts/fingerprint.rs](../../crates/kodstar-core/src/domain/facts/fingerprint.rs)에서
전체와 환경 fingerprint를 구분한다. Go의 package 함수는 선언 위치가 밀려도 같은 binding 의미를
가질 수 있다. [index/facts/retained.rs](../../crates/kodstar-core/src/infrastructure/index/facts/retained.rs)와
[facts.rs](../../crates/kodstar-core/src/infrastructure/index/facts.rs)는 재사용한 사실과 binding의 원문 좌표를
새 범위에 맞춘다. [body_reuse/](../../crates/kodstar-core/src/infrastructure/index/body_reuse)는 해시 후보를
실제 필드와 비교해 같은 사용의 ID·관계·의존성을 보존한다. 변경 범위 판정이 불확실하면 파일 단위로 교체한다.

[domain/facts/append.rs](../../crates/kodstar-core/src/domain/facts/append.rs)와
[index/append_reuse/](../../crates/kodstar-core/src/infrastructure/index/append_reuse)는 단일 선언 삽입 구간을
제외한 원문 해시·기존 사실·독립된 소유 범위를 증명한다. 새 이름이 과거 조회를 바꿀 가능성이 있으면
더 넓은 경로를 사용한다. Go package 함수의 부분 갱신과 정적 모듈 선언의 부분 갱신에는 각 언어·소유 경계
검사가 적용된다. 혼합 프로젝트에 변화가 있을 때도 같은 증명에 따라 안전한 범위를 고른다.

예를 들어 처음 `load()`가 미해결이면 대상이 없다는 lookup을 기록한다. 나중에 모듈이 그 이름을
내보내면 새 provider가 이전 lookup의 소비 fact를 무효화한다. 반대로 확정 정의를 삭제하거나 동명
후보를 추가하면 기존 관계에 대한 재해석이 일어난다. 사용 파일의 바이트가 같은 상황에서도 필요하다.
의미 결과와 통계·좌표가 완성되는 시점은 [분석 공개](storage.md#공개와-호환성)에서 확인한다.
