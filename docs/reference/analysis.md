# 분석 입력과 관계

[기능 명세](../../kodstar-spec.md) · [구문 추출 코드](../architecture/inputs.md) · [해석 코드](../architecture/resolution.md)

## 언어와 분석 범위

한 프로젝트에 여러 언어를 둘 수 있다. 파일별로 언어와 모듈 문맥을 선택한 뒤 공통 인덱스에 결과를 담는다.
각 객체에 언어와 패키지·모듈이 기록되므로, 예를 들어 `src/store.rs`와 `ui/store.ts`의 동명 정의를
별도로 조회할 수 있다. 서로 다른 언어·포맷에 걸친 연결은 명시적 프로젝트·파일·템플릿·HTTP 규칙으로 설명한다.

아래 표에는 대표 확장자와 지원하는 정적 근거를 정리했다. 분석기 빌드의 전체 확장자·구문 변형·capability와
제약은 `kodstar languages`에서 확인한다. 입력 인식, 정의 추출, 파일 관계, 직접 관계와 의미 보강은 각각 표시된다.

| 언어 | 대표 입력 | 정적 분석 근거 |
|---|---|---|
| Python | `.py`, 일부 shebang | 모듈·상대 import·별칭·함수·클래스·타입 힌트·decorator·스코프·확인 가능한 `self` |
| JavaScript | `.js`, `.jsx`, `.mjs`, `.cjs` | 함수·클래스·클로저·ESM·CommonJS·별칭·재수출·명시 receiver와 호출 |
| TypeScript | `.ts`, `.tsx`, `.mts`, `.cts` | JS 구조, 타입·interface·type-only import·경로 별칭·확인 가능한 멤버 |
| Rust | `.rs` | crate·모듈·`mod`·`use`·재수출·타입·trait·`impl`·raw 이름·조건 |
| Go | `.go` | package·import·receiver·메서드·interface 후보·타입 참조·build tag |
| Java | `.java` | package·import·타입·생성자·상속·명시 receiver와 인자 수 |
| C | `.c`, C로 판별한 `.h` | 함수 선언·정의·struct·include·직접 호출·전처리 조건과 선택적 의미 근거 |
| C++ | `.cpp`, `.cc`, `.cxx`, `.hpp`, C++로 판별한 `.h` | namespace·타입·멤버·선언과 정의·include·상속·template 호출 근거 |
| C# | `.cs` | namespace·using·partial·generic·attribute·생성자·타입 기반 멤버·구현 후보 |
| PHP | `.php` | namespace·use·trait·타입 있는 property와 인수·생성자 승격·receiver |
| Kotlin | `.kt`, `.kts` | package·import·class·object·extension·generic parameter·한정 호출·멤버 |
| Swift | `.swift` | 타입·함수·extension·protocol·생성자·`Self`·파일 간 멤버·구현 후보 |
| Ruby | `.rb`, `.rake`, `Gemfile` 등 | module·class·정적 require·상속·생성·receiver |
| Scala | `.scala` | package·import·object·class·trait·함수·타입 참조·상속 |
| Zig | `.zig` | 함수·struct·enum·union·정적 `@import`와 파일 내보내기 |
| Elixir | `.ex`, `.exs` | module·`def`·`defp`·alias·import·use·한정 호출 |
| Erlang | `.erl`, `.hrl`, `.escript` | module·name/arity·export·import·include·record·remote call |
| Common Lisp | `.lisp`, `.cl`, `.lsp`, `.asd` | package·함수·클래스·메서드·매크로 정의·정적 이름 조회 |

대표 언어는 18개다. JSX는 JavaScript, TSX는 TypeScript의 구문 변형이다.
`.h`는 내용 근거로 C/C++를 판별한다. Objective-C·MATLAB로 판단한 입력은 별도 미지원으로 보고한다.
`.inc`처럼 확장자를 공유하는 입력은 설정으로 언어를 지정할 수 있다. 확장자 없는 파일은
선정한 Python·Node·Ruby·PHP·Elixir·Erlang·Common Lisp 실행기 shebang으로도 인식한다.

## 웹 컴포넌트와 명시적 HTTP 연결

| 포맷 | 다루는 구조 |
|---|---|
| HTML `.html`·`.htm` | JavaScript script, 정적 htmx 요청과 같은 파일의 ID 참조 |
| Vue `.vue` | 내부 JS/TS, import와 템플릿 컴포넌트·이벤트 참조 |
| Svelte `.svelte` | script·선정한 내장 구문·명시 import·템플릿 참조 |
| Astro `.astro` | 서버 frontmatter, 브라우저 script, 템플릿 컴포넌트 |
| EJS `.ejs` | 서버 코드 태그, 정적 include, 브라우저 script |
| Blade `.blade.php` | 정적 layout·include·component, PHP 구간과 Livewire 참조 |
| Razor `.razor`·`.cshtml` | page·model·inherits·타입·handler·component 참조 |

일곱 포맷의 정의와 사용 위치는 host 파일의 원문을 기준으로 한다. React·Next.js의 JSX/TSX는
해당 언어 분석 경로에서 다룬다. CSS와 실행 시 생성되는 라우팅·DOM·프레임워크 코드는 분석 범위 밖이다.

Hono는 canonical `hono` import, lexical 앱 생성과 명시적 export·mount를 확인한다.
정적 HTTP 메서드 등록(`get`, `post`, `put`, `patch`, `delete`, `head`, `options`, `all`, `on`),
`use`, `basePath`, `route`를 다룬다. 등록은 `handles`, 확인한 Context에서 반환하는 JSX는 `renders`다.
가려지거나 재할당된 이름, 사용자 모듈 import, 동적 앱 생성은 정적 근거를 다시 확인한다.

htmx는 `hx-get`·`hx-post`·`hx-put`·`hx-patch`·`hx-delete`와 `data-hx-*`를
`http_request` 사용으로 기록한다. 주석·raw text·`hx-disable` 구간을 거르고, `hx-target="#id"`는
같은 파일의 유일한 ID를 확인한다. 요청과 등록은 HTTP 메서드와 정적 경로, `:name`, 말단 `*`로 대조한다.
루트 기준 URL의 query와 fragment는 경로에서 구분한다. 동적 HTML 생성, 상대 경로의 browser base, 외부 host,
임의 selector와 런타임 DOM은 정적 대상 확정 범위 밖이다.
HTTP 관계는 실행 순서·배포가 별도인 후보이므로 `--include-candidates`로 본다.
mount는 깊이 16, 등록과 투영은 각각 최대 16,384개, 투영 경로는 최대 65,536바이트다.
순환과 작업 한도 초과는 부분 결과를 공개하기 전에 진단한다.

## 코드 해석용 보조 입력

패키지·workspace·모듈 소속을 파악하기 위해 `package.json`, `tsconfig`, Cargo, Go, Python,
Maven, Composer, SwiftPM 등 인식하는 설정의 명시적 구조를 읽는다. 설정은 해당 언어 ecosystem과 루트에서만
적용한다. JSON 보조 입력에서는 인식된 프로젝트 참조·경로·의존 항목을 읽는다.
`.sln`·`.slnx`는 프로젝트 소속과 참조, `.csproj` 등은 정적 소스 포함과 프로젝트 참조,
XAML은 code-behind·이벤트·binding·resource·ViewModel 근거를 제공한다.
`.fsproj`·`.vbproj`는 프로젝트 참조를 읽는 쪽에 해당한다.

상속·참조 설정이 실패하면 해당 구성의 소비 경계를 진단한다. 가장 가까운 JS/TS 설정이 실패하거나
한도로 스킵된 범위에서는 상위 설정이나 package fallback 적용을 차단한다.

## 객체와 관계의 의미

논리 키는 언어, 패키지·모듈, 소유 범위, 이름·종류·서명·조건과 필요한 동명 구분자를 반영한다.
표시 이름과 내부 정수 ID의 수명은 별도다. 전체 재빌드에 걸쳐 대상을 대조할 때는 논리 키와 근거를 사용한다.
partial·분리 구현처럼 같은 객체의 다중 정의는 대표 객체와 각 정의 위치로 나타낸다.
파일 자체도 객체로 표현해 top-level import와 초기화 사용의 출처를 기록한다.

| 관계 | 근거 |
|---|---|
| `calls`, `indirect_call` | 직접 호출과 함수 값의 잠재 호출 |
| `references`, `uses_type` | 명시 객체 참조와 타입 사용 |
| `imports`, `reexports`, `includes` | import·재수출·소스 include |
| `inherits`, `mixes_in`, `implements`, `overrides` | 상속·mixin·구현·상위 메서드 재정의 |
| `dispatch_candidate` | 알려진 호출 구현 후보 |
| `defines`, `contains` | 파일·모듈 정의 또는 멤버 소유 |
| `template_include`, `template_extends` | 템플릿·컴포넌트 포함 또는 layout 상속 |
| `depends_on`, `event_handler` | 명시적 프로젝트 의존과 사용자 인터페이스 이벤트 연결 |
| `http_request`, `handles`, `renders` | HTTP 요청 후보, 검증된 Hono 등록, 응답에서 참조한 컴포넌트 |

같은 두 객체 사이의 관계 종류와 각 사용 위치를 보존한다. 재귀 호출도 포함한다.
함수 값의 확인된 대상은 `references`로, 잠재 실행은 `indirect_call`로 기록한다.
일반 변수·필드 전체의 reads/writes와 데이터 흐름은 정적 근거의 제공 범위 밖이다.
관계 코드와 이름은 [registry](../../crates/kodstar-core/src/domain/relations.rs)가 관리하며 버전은 3이다.

| 해석 상태 | 의미 |
|---|---|
| `resolved` | 해석 규칙이 대상을 식별함 |
| `candidate` | 추가 가정이 필요한 대상 |
| `ambiguous` | 후보가 여럿 남음 |
| `unresolved` | 규칙으로 대상을 식별할 근거가 부족함 |

내부·외부 분류는 해석 상태와 독립적이다. 일반 관계 탐색의 기본 범위는 확정 내부 관계다.
`defines`와 `contains`는 기본 의존성 순회에서 제외한다. 선택한 후보·외부 관계는 상태와 근거가 따라온다.
lexical scope, shadowing, 언어별 가시성, 명시 import·재수출과 추적 가능한 receiver가 먼저 고려된다.
Java는 고정 인자 수와 varargs의 최소 수를 필터로 사용한다. 수가 맞는 대상이 여럿이면 모호함을 보존한다.

## 불완전 입력과 C/C++ 의미 보강

동적 이름·reflection·eval, 전체 generic·overload·alias 추론, 런타임 dispatch, 임의 compile-time 실행은
별도 런타임 근거를 요구한다. 조건부 선언은 관측한 조건과 함께 공개한다. 조건이 구문을 나누는 파일은
원래 위치를 유지하는 투영 입력으로 처리하고, 도달 가능 영역 누락이나 작업 한도를 발견하면 실패로 표시한다.

조건부 분석은 조건 이름 1,024개, 중첩 128단계, 체인당 분기 128개, 파일당 투영 256개를 한도로 둔다.
조건 만족 탐색은 이름 128개·16,384단계 이내다. 작업 예산을 넘기면 불완전한 투영을 진단한다.
복구 가능한 구문은 입증된 정의와 불확실한 관계를 구분해 공개한다. C/C++ 구조 매크로의
`unexpanded_native_macros`, 문장 매크로가 확인되지 않았을 때의 `source_macro_incomplete`도 이 계약을 따른다.
native용 구문 보정과 재파싱은 최대 8회다.

잘못된 인코딩은 입력 실패다. Python과 Ruby의 첫 두 줄에 명시된 ASCII·UTF-8·Latin-1·ISO-8859-5·
Windows-1251/1252·KOI8-R/U는 엄격히 처리한다. 위치와 이력의 기준은 원본 바이트다.
Kotlin multi-dollar 문자열도 활성 보간의 원래 위치를 보존한다.

C/C++는 Tree-sitter의 기본 사실에 내장 Clang 23.1.4의 선택적 TU 의미 근거를 더한다.
현재 분석 체크포인트의 include와 compile database를 쓰며, 입력의 제한된 옵션에 C17·C++20 기본 문맥을 적용한다.
완료한 TU에서 얻은 callee와 선언 근거를 USR·정의 위치로 대조한다. 외부 SDK·지원하지 않는 옵션·충돌은
`clang_semantic_incomplete`로, 작업 한도는 `clang_semantic_limit`로 보고하고 가능한 구문 결과를 제공한다.
Clang의 입력·작업별 한도와 확인 방법은 [내장 Clang](../architecture/native.md#작업-예산과-진단)에 있다.
