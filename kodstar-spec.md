# KodStar 기능 명세

| 항목 | 정의 |
|---|---|
| 프로젝트 이름 | **KodStar** |
| 시각적 브랜드 표기 | **Kod\*** |
| 실행 명령 | `kodstar` |
| 구현 언어 | 분석 엔진과 CLI 모두 Rust |
| 제품 형태 | 로컬 프로젝트의 소스코드 관계를 탐색하는 CLI와 재사용 가능한 분석 엔진 |
| 분석기 기반 | Graphify의 선정 언어 추출·해석 로직을 참고하고 Rust로 포팅 |
| 핵심 우선순위 | 분석 근거와 한계의 명확성, 낮은 메모리 사용량, 빠른 조회와 증분 갱신 |
| 분석 대상 | 프로그래밍 언어 19개, 웹 컴포넌트·템플릿 6개, 코드 해석용 보조 입력 |
| 저장 위치 | 사용자의 홈 디렉터리 아래 프로젝트 경로별 캐시 |
| 저장 방식 | SQLite 기반 관계 인덱스 |

이 문서는 제품의 기능·동작·외부 계약을 정의한다. 현재 구현 범위는 [README](README.md)와 `kodstar languages`에서 확인한다. 내부 구조는 [아키텍처 문서](docs/architecture.md), 개발·검증 절차는 [개발 문서](docs/development.md)에 둔다.

## 1. 제품 목표

KodStar는 현재 프로젝트에서 다음 질문에 답하는 도구다.

- 이 소스 파일과 연결된 다른 파일은 무엇인가?
- 이 파일에 어떤 함수·클래스·타입·메서드가 정의되어 있는가?
- 이 객체가 사용하는 객체는 무엇인가?
- 이 객체를 사용하는 다른 객체는 무엇인가?
- 이 연결은 어느 파일의 어느 위치에서 발생하는가?
- 이 객체를 변경하면 어떤 코드가 정적으로 영향을 받을 수 있는가?
- 두 객체는 어떤 관계 경로로 연결되는가?
- 연결이 확정되지 않은 이유와 남아 있는 후보는 무엇인가?

핵심 사용 단위는 **파일**과 **객체**다. 여기서 객체는 언어별 함수, 메서드, 클래스, 구조체, 인터페이스, 트레이트, 열거형, 모듈 등 분석 가능한 소스 요소를 뜻한다.

관계는 이름이 비슷하다는 사실이 아니라 소스에 나타나는 정의·사용·import·상속과 그 해석 근거를 바탕으로 만든다. 연결 대상을 결정할 수 없으면 후보 또는 미해결로 남긴다.

### 1.1 기본 사용 모습

```bash
cd /work/my-project
kodstar build

kodstar list src/order.rs
kodstar symbols src/order.rs
kodstar related src/order.rs OrderService::create_order
```

최초 `build` 이후에는 일반 명령 실행 전에 변경을 확인하고 필요한 부분을 자동으로 갱신한다. 이 공통 준비는 조회·진단·언어 보고·캐시 목록에 적용한다.

## 2. 기능 일람

| 기능군 | 제공 기능 |
|---|---|
| 프로젝트 인식 | 현재 디렉터리 기준 등록, 명시적 루트 지정, 등록된 상위 루트 탐색, 중첩 프로젝트 구분 |
| 입력 판별 | 확장자·선정 언어의 shebang·특수 파일명·명시적 override 기반 판별 |
| 파일 선별 | 제외 패턴, Git ignore 규칙, 의존성·빌드 산출물 제외, 파일 크기 제한 |
| 구조 추출 | 파일·모듈·함수·타입·메서드·멤버·스코프와 정의 위치 추출 |
| 바인딩 해석 | lexical scope, shadowing, 명시적 import·별칭·재수출·namespace 해석 |
| 객체 관계 | 직접 호출, 객체 참조, 타입 사용, 상속, 구현, override, 확인 가능한 receiver 연결 |
| 웹 입력 | Vue·Svelte·Astro의 내장 코드와 템플릿 참조, EJS·Blade·Razor 구조 참조 |
| 파일 관계 | import·include·객체 참조를 다른 파일 단위로 집계 |
| 관계 탐색 | 정방향·역방향·양방향 조회, 깊이 제한, 전이 탐색, 관계 종류 필터 |
| 변경 영향 | 객체·타입 멤버·파일을 시드로 역방향 영향 후보 탐색 |
| 경로 조회 | 두 객체 사이의 관계 경로와 최단 경로 조회 |
| 객체 검색 | 정확 이름과 접두 이름 검색, 동명 후보 선택 |
| 설명 | 정의·사용 위치, 해석 규칙, 후보·미해결 이유와 분석 출처 표시 |
| 증분 갱신 | 조회 시 변경 확인, 변경 파일 재분석, 의존 관계 재해석, 삭제·rename 반영 |
| 결과 일관성 | 커밋된 인덱스 세대 단위 조회, 갱신 실패 시 마지막 정상 세대 보존 |
| 캐시 관리 | 홈 디렉터리의 경로별 캐시, 목록, 현재·전체 정리, TTL·용량 기반 GC |
| 진단 | 인덱스 상태, 분석 범위, 미지원·실패·스킵, 언어별 실제 capability 표시 |
| 출력 | 사람이 읽는 text, JSON, JSONL, TSV, 스트리밍, 결과·방문 한도와 잘림 표시 |
| 자원 관리 | 메모리 예산, 제한된 병렬 처리, 바이트 기준 대기열 제한, 대규모 탐색의 디스크 보조 |
| 파일 감시 | 사용자가 실행한 동안만 변경을 감시하고 갱신 |
| 도구 연동 | Rust 엔진 API, 구조화된 CLI 결과, 확장용 stdio 세션 |
| 에디터 확장 | 같은 엔진 결과를 이용하는 VS Code 관계 목록·위치 이동·선택적 그래프 UI |

## 3. 지원 언어

지원 언어는 Graphify에 존재하는 분석기 중 제품 목적에 맞게 선정한다. Stack Overflow 설문 사용률을 지원 여부의 기준으로 사용하지 않는다.

**19개 언어 모두 분석 목표에 포함한다.** Java·Go를 비롯한 주요 언어를 파일명 판별만 지원하는 형태로 취급하지 않는다. 다만 언어마다 정적으로 확인 가능한 관계가 다르므로, 제공하는 구문과 관계 종류를 별도로 공개한다.

| 언어 | 대표 입력 | 분석 대상 기능 |
|---|---|---|
| Python | `.py`, 선정된 Python shebang 입력 | 모듈·상대 import·별칭·함수·클래스·메서드·타입 힌트·decorator·스코프·확인 가능한 `self` 연결 |
| JavaScript | `.js`, `.jsx`, `.mjs`, `.cjs` | 함수·클래스·클로저·ESM·CommonJS·별칭·재수출·확인 가능한 receiver와 호출 |
| TypeScript | `.ts`, `.tsx`, `.mts`, `.cts` | JavaScript 기능, 타입·interface·타입 참조·type-only import·경로 별칭·정적으로 확인 가능한 멤버 연결 |
| Rust | `.rs` | crate·module·`mod`·`use` tree·`pub use`·함수·타입·trait·`impl`·분리된 impl·`Self`/`self`·조건부 선언 |
| Go | `.go` | package·import·함수·타입·receiver·메서드·interface 후보·제네릭 타입 참조·build tag |
| Java | `.java` | package·import·클래스·interface·생성자·메서드·타입 참조·상속·명시 타입 기반 receiver |
| C | `.c`, C로 판별한 `.h` | 함수 선언·정의·struct·include·직접 호출·전처리 조건의 보존 |
| C++ | `.cpp`, `.cc`, `.cxx`, `.hpp`, C++로 판별한 `.h` | namespace·클래스·멤버·선언과 정의·include·상속·정적으로 확인 가능한 호출·template 후보 |
| C# | `.cs` | namespace·using·클래스·interface·partial·generic·attribute·생성자·타입 기반 멤버 연결·구현 후보 |
| PHP | `.php` | namespace·use·함수·클래스·trait·타입 있는 property·인수·생성자 승격·확인 가능한 receiver |
| Kotlin | `.kt`, `.kts` | package·import·class·object·함수·extension·generic parameter·한정 호출·확인 가능한 멤버 연결 |
| Swift | `.swift` | type·함수·메서드·extension·protocol·constructor·`Self`·파일 간 멤버 연결·구현 후보 |
| Ruby | `.rb`, `.rake` | module·class·함수·메서드·require·상속·정적인 생성·receiver 바인딩 |
| Scala | `.scala` | package·import·object·class·trait·함수·메서드·타입 참조·상속 |
| Zig | `.zig` | 함수·struct·enum·union·정적 `@import`·확인 가능한 직접 호출 |
| Elixir | `.ex`, `.exs` | module·`def`/`defp`·alias·import·use·정적 한정 호출·파일 간 대상 연결 |
| Erlang | `.erl`, `.hrl`, `.escript` | module·name/arity·export·import·include·record·정적 remote call |
| Common Lisp | `.lisp`, `.cl`, `.lsp`, `.asd` | package·함수·클래스·메서드·매크로 정의·명시적 참조·정적으로 확인 가능한 호출 |
| Pascal / Delphi | `.pas`, `.pp`, `.dpr`, `.dpk`, `.lpr`, `.inc` | unit·uses·procedure·function·class·property·상속·선언과 구현·확인 가능한 상속 메서드 호출 |

확장자 표는 대표 입력이다. 실제 등록된 확장자·구문 변형과 지원 관계는 `kodstar languages`로 확인한다. 확장자가 겹치는 입력에는 자동 판별과 사용자의 명시적 override를 적용한다.

### 3.1 구문 변형과 혼합 입력

- JSX는 JavaScript에, TSX는 TypeScript에 포함한다.
- CUDA·Metal 입력은 Graphify의 C++ 경로에서 실제 처리하는 구문 범위만 표시한다. 독립적인 GPU 언어 의미 분석을 약속하지 않는다.
- `.h`는 C/C++를 구분한다. Objective-C 입력으로 판단되면 현재 범위에서 지원하지 않는다고 표시한다.
- Objective-C·MATLAB 입력을 C/C++로 강제 해석하지 않는다.
- `.inc`처럼 언어 간에 공유하는 확장자는 명시적 override로 의미를 지정할 수 있다.
- 확장자가 없는 실행 파일은 지원 언어에 해당하는 shebang일 때만 분석한다. Bash 지원이 자동으로 생기지는 않는다.

### 3.2 언어 지원의 의미

| 표시 항목 | 의미 |
|---|---|
| 입력 인식 | 언어·구문 변형을 판별할 수 있음 |
| 정의 추출 | 객체와 정의 위치를 추출할 수 있음 |
| 파일 관계 | 명시적인 모듈·import·include·파일 참조를 연결할 수 있음 |
| 직접 관계 | 확인 가능한 객체 호출·참조·타입 사용을 연결할 수 있음 |
| 의미 보강 | receiver·상속·구현·언어별 추가 규칙을 적용할 수 있음 |
| 제한 사항 | 동적 구문·지원하지 않는 문법·fallback·외부 도구 의존 여부 |

grammar가 등록되어 있다는 사실만으로 전체 관계 분석을 지원한다고 표시하지 않는다. AST 경로와 regex·scanner fallback도 구분한다. 지원 수준은 언어 이름 하나의 등급 대신 기능별 capability로 보여준다.

## 4. 웹 컴포넌트·템플릿 지원

| 대상 | 입력 | 분석 대상 기능 |
|---|---|---|
| Vue | `.vue` | 내부 JavaScript/TypeScript, import, 정의, 확인 가능한 템플릿·컴포넌트 참조 |
| Svelte | `.svelte` | script 블록·선정된 내장 구문·import·정의·확인 가능한 템플릿 참조 |
| Astro | `.astro` | frontmatter·script·import·정의·확인 가능한 템플릿·컴포넌트 참조 |
| EJS | `.ejs` | 태그 안의 코드와 명시적 include·파일 참조 |
| Laravel Blade | `.blade.php` | layout 상속·include·component·확인 가능한 PHP 참조 |
| Razor | `.razor`, `.cshtml` | page·model·inherits·타입·handler·component 참조 |

Vue·Svelte·Astro는 모두 지원 목표다. 혼합 파일에서 추출한 정의와 사용 위치는 원본 파일의 위치로 표시한다. script 블록 분리·masking 때문에 byte 위치가 틀어지지 않도록 원본 위치 관계를 보존한다.

React·Next.js 등의 JSX/TSX 입력은 JavaScript·TypeScript 분석 경로를 사용한다. 프레임워크 이름만으로 런타임 라우팅·반응성·템플릿 실행·생성 코드 전체를 해석한다고 표시하지 않는다. CSS 자체의 관계 분석은 범위 밖이다.

## 5. 코드 해석용 보조 입력

보조 입력은 언어 개수에 포함하지 않는다. 코드의 소속·모듈 경계·명시적 파일 연결을 설명하는 데 사용한다.

| 입력군 | 대상 기능 |
|---|---|
| 구조 JSON | 인식하는 프로젝트 설정의 경로·참조·의존 정보. 임의 데이터 JSON 전체를 key별 객체 그래프로 만들지 않음 |
| 패키지 manifest | Cargo·Python·Go·Java 등의 명시적인 패키지·workspace·의존 경계 |
| .NET solution | `.sln`, `.slnx`의 프로젝트 소속과 참조 |
| .NET project | `.csproj` 등의 소스 소속과 정적 프로젝트·패키지 참조 |
| XAML | code-behind·event handler·binding·resource·확인 가능한 ViewModel 참조 |
| Delphi/Lazarus form | 지원하는 `.dfm`, `.lfm` 변형의 component 구조와 event handler 연결 |
| Lazarus package | `.lpk`의 unit·package 참조 |

`package.json`, `tsconfig`, `Cargo.toml`, `go.mod`, `go.work`, `pyproject.toml`, `pom.xml`, Composer 설정 등은 해당 언어 해석에 필요한 범위에서 읽는다. 빌드 스크립트나 프로젝트 코드를 실행해 정보를 얻지 않는다. 패키지명이 같아도 ecosystem·모듈 경계가 다르면 같은 객체로 합치지 않는다.

`.fsproj`·`.vbproj`의 정적 프로젝트 참조를 읽는 것과 F#·VB.NET 소스 함수를 분석하는 것은 별개다. 보조 입력에서 제외 언어의 경계를 발견해도 해당 언어 분석을 지원한다고 표시하지 않는다.

## 6. 범위에서 제외하는 기능

| 구분 | 현재 제품 범위 밖 |
|---|---|
| 명시적 제외 언어·DSL | SQL, Terraform/HCL, Robot Framework, Apex, Dart, COBOL, Bash/Shell, PowerShell |
| 선정하지 않은 원본 언어 | R, Solidity, Groovy, Lua, VB.NET, Objective-C, Julia, Fortran, OCaml, Verilog/SystemVerilog, BYOND DreamMaker |
| 기타 독립 언어 분석 | CSS, HTML 자체, Assembly, F# 등 선정 목록에 없는 언어 |
| 제외 보조 포맷 | PowerShell manifest, BYOND 자산, MCP 설정 그래프, Markdown 구조 링크 |
| 의미·미디어 분석 | 문서·PDF·Office·이미지·오디오·영상의 의미 추출 |
| AI 기능 | LLM 호출, 임베딩, 벡터 검색, 자연어 질문의 생성형 답변 |
| 그래프 부가 분석 | 커뮤니티 군집화, 의미 유사도 기반 객체 병합, 사용 이력 기반 관계 변경 |
| 외부 운영 | 클라우드 동기화, 원격 DB push, 원격 저장소 수집·PR 운영 도구 |
| 자동 환경 변경 | 에이전트 스킬·Git hook·에디터 설정·시스템 서비스 자동 설치 |
| 실행 기반 분석 | 대상 프로그램 실행, 매크로 평가, 빌드 실행, 완전한 런타임 호출 수집 |

선정한 웹 컴포넌트·템플릿 안에서 구조를 판별하기 위해 markup 일부를 읽을 수 있지만, 이를 독립 HTML/CSS 분석으로 집계하지 않는다.

## 7. CLI 명령어 전체

### 7.1 인덱스 생성과 갱신

| 명령 | 동작 |
|---|---|
| `kodstar build [경로]` | 프로젝트를 등록하고 최초 인덱스를 생성. 이미 인덱스가 있으면 변경분 갱신 |
| `kodstar build --full` | 선택한 프로젝트 전체를 재분석하고 정상 완료한 새 인덱스 세대를 공개 |
| `kodstar build --verify` | 모든 분석 대상의 내용 해시를 확인하고 실제 변경된 부분을 갱신 |

`build`의 기본 루트는 현재 디렉터리다. `[경로]` 또는 `--root`로 명시할 수 있다. 둘을 함께 지정할 때 서로 다른 프로젝트를 가리키면 인자 오류로 처리한다.

인덱스가 없는 조회는 `build` 실행을 안내한다. 조회 명령이 알리지 않고 프로젝트 전체 초기 분석을 시작하지 않는다.

### 7.2 파일·객체 조회

| 명령 | 동작 |
|---|---|
| `kodstar list <파일>` | 해당 파일과 직접 연결된 다른 소스 파일을 방향·관계별로 집계 |
| `kodstar symbols <파일>` | 해당 파일에 정의된 객체의 이름·종류·소유 관계·정의 위치 표시 |
| `kodstar related <파일> <객체>` | 해당 객체가 사용하는 객체와 해당 객체를 사용하는 쪽을 나누어 표시 |
| `kodstar deps <파일> <객체>` | 객체의 정방향 의존 관계 조회 |
| `kodstar refs <파일> <객체>` | 객체의 역방향 사용·참조 관계 조회 |
| `kodstar explain <파일> <객체>` | 정의 정보, 사용 위치, 해석 규칙, 후보와 미해결 이유 표시 |
| `kodstar impact <파일> [객체]` | 정적 관계를 역방향으로 탐색해 변경 영향 후보 표시 |
| `kodstar path <파일::객체> <파일::객체>` | 필터를 만족하는 두 객체 사이의 최단 관계 경로 하나 표시 |
| `kodstar search <이름>` | 정확 이름으로 객체 후보 검색 |
| `kodstar search <이름> --prefix` | 접두 이름으로 객체 후보 검색 |

### 7.3 상태·진단·언어 정보

| 명령 | 동작 |
|---|---|
| `kodstar status` | 루트·캐시 위치·인덱스 세대·분석 시각·분석 범위·직전 갱신 상태 표시 |
| `kodstar doctor` | 저장소 무결성·버전 호환·경로·지원 parser·실패 상태 진단 |
| `kodstar languages` | 사용 가능한 언어·확장자·구문 변형·관계 capability·제한 사항 표시 |
| `kodstar --help` | 명령어·옵션 도움말 |
| `kodstar --version` | CLI·엔진 버전 표시 |

`status`와 `doctor`는 기존 프로젝트를 증분 동기화한 뒤 실행한다. `languages`와 `cache list`도 선택한 등록 프로젝트가 있으면 같은 준비를 수행한다. 등록 프로젝트가 없으면 전역 보고만 제공하며 초기 분석을 시작하지 않는다.

명시적인 `--no-refresh`는 동기화를 생략하고 `not_checked` 최신성을 표시한다. 도움말·버전, 캐시 복구·정리·GC, 세션 전송·취소·종료 제어는 동기화를 우회한다. 도움말과 버전 표시를 위해 DB나 parser를 초기화하지 않는다.

### 7.4 캐시 관리

| 명령 | 동작 |
|---|---|
| `kodstar cache list` | 프로젝트 경로별 캐시 위치·용량·최근 사용 시각·사용 중 상태 표시 |
| `kodstar cache clean` | 현재 프로젝트의 사용하지 않는 분석 캐시 삭제 |
| `kodstar cache clean --all` | 모든 프로젝트의 사용하지 않는 분석 캐시 삭제 |
| `kodstar cache gc` | TTL·용량 정책에 따라 정리 가능한 캐시 삭제 |
| `kodstar cache gc --dry-run` | 삭제 예정 캐시와 이유만 표시 |

### 7.5 감시와 외부 연동

| 명령 | 동작 |
|---|---|
| `kodstar watch` | 실행 중에만 변경을 감시하고 인덱스를 갱신 |
| `kodstar serve --stdio` | 에디터·외부 도구용 구조화된 요청·응답 세션 제공 |

일반 CLI 사용에는 `watch`나 `serve`를 실행할 필요가 없다. 시스템 서비스나 상주 데몬을 자동 설치하지 않는다. 초기 인덱스 없는 `watch`·관계 요청 역시 `build` 안내 정책을 따른다.

## 8. 명령어별 결과 의미

### 8.1 파일 연결: `list`

파일 간 연결은 다음을 구분한다.

- 파일·모듈의 명시적 import·include.
- 해당 파일에 정의된 객체가 다른 파일의 객체를 사용하는 관계.
- 다른 파일의 객체가 해당 파일의 객체를 사용하는 관계.
- 템플릿·프로젝트 구조가 만드는 명시적 파일 참조.

기본 출력은 다른 파일의 경로, 방향, 관계 종류, 관계 수다. import만 존재하는 연결과 실제 객체 호출·참조가 있는 연결을 합쳐 의미를 흐리지 않는다. 같은 파일 내부의 관계는 다른 파일 목록에서 제외한다.

`list`는 직접 파일 관계 조회다. 관계 그래프 전체에 대한 재귀 탐색은 `related`나 `impact`에서 요청한다.

### 8.2 객체 선택: `symbols`와 객체 인자

객체 선택은 파일 경로와 원래 이름을 함께 사용한다. 같은 이름이 여러 개면 소유 타입·모듈이 포함된 한정 이름 또는 정의 위치로 좁힌다.

```bash
kodstar symbols src/order.rs
kodstar related src/order.rs OrderService::create_order
kodstar related src/order.rs create_order --at 24:1
```

모호한 선택을 첫 번째 검색 결과로 대체하지 않는다. 남은 후보와 정의 위치를 반환한다. overload·중첩 scope·조건부 선언·다중 정의도 이 규칙을 따른다.

### 8.3 객체 연결: `related`, `deps`, `refs`

기본 조회는 직접 관계, 깊이 1이다.

- `related`: 정방향과 역방향을 구분해 표시.
- `deps`: 선택한 객체에서 나가는 관계.
- `refs`: 선택한 객체로 들어오는 관계.
- `--sites`: 객체별 집계 대신 개별 사용 위치 표시.
- `--members`: 타입·클래스의 멤버를 탐색 시드에 포함.

```bash
kodstar related src/order.rs create_order
kodstar deps src/order.rs create_order --relation calls
kodstar refs src/order.rs create_order --sites
kodstar related src/order.rs OrderService --members
```

양방향 전이 조회는 정방향 도달 집합과 역방향 도달 집합을 각각 구한 뒤 합친다. 매 단계마다 방향을 바꾸는 무방향 확장으로 해석하지 않는다.

### 8.4 간접 연결과 전체 도달 조회

```bash
kodstar related src/order.rs create_order --depth 3
kodstar related src/order.rs create_order --transitive --all --max-nodes 0 --format jsonl
```

`--transitive`는 깊이 제한 없는 도달 탐색을 뜻한다. 출력 행 제한과 방문 객체 제한은 별도다.

**전체 연결**은 선택한 인덱스 세대, 방향, 관계 필터, 해석 상태 정책 안에서 도달 가능한 결과 전체를 뜻한다. 프로그램 실행 중 발생할 모든 관계를 뜻하지 않는다. 기본적으로 확정한 프로젝트 내부 관계를 탐색하며 후보·외부 객체는 명시적으로 포함한다.

소유 관계는 함수 호출과 다르다. `contains`·`defines`를 기본 의존성 순회에 섞어 클래스나 파일을 통해 무관한 모든 객체로 확장하지 않는다. 소유 정보는 별도로 표시하고 멤버 확장은 `--members` 등의 명시된 규칙을 적용한다.

### 8.5 변경 영향: `impact`

`impact`는 역방향으로 도달하는 정적 영향 후보를 보여준다.

- 객체를 지정하면 해당 객체를 시드로 사용한다.
- 클래스·타입은 기본적으로 해당 멤버도 영향 시드에 포함한다.
- 파일만 지정하면 파일 수준의 관계와 해당 파일의 정의들을 시드로 사용한다.
- 상속·override·구현 후보는 지원 capability와 관계 필터에 따라 구분한다.

결과는 실제 장애·테스트 실패·런타임 영향의 확정 목록이 아니다. 정적 연결을 근거로 검토할 대상을 좁히는 기능이다.

### 8.6 관계 설명: `explain`

객체 설명은 생성형 요약이 아니라 분석 근거를 제공한다.

| 정보 | 내용 |
|---|---|
| 선언 | 이름·종류·소유 scope·모듈·서명·정의 위치 |
| 연결 | 정방향·역방향 관계와 관계가 발생한 사용 위치 |
| 근거 | import·별칭·lexical binding·receiver 타입 등 적용한 해석 규칙 |
| 상태 | 확정·후보·모호함·미해결, 프로젝트 내부·외부 여부 |
| 제한 | 지원하지 않는 구문·동적 표현·조건부 후보·자원 제한 |
| 출처 | 추출기·해석기 또는 의미 정보 provider와 그 버전 |

## 9. CLI 옵션과 공통 정책

옵션은 의미가 있는 명령에만 적용한다. 예를 들어 `--sites`를 `cache` 명령에 적용하지 않는다.

| 옵션 | 의미 |
|---|---|
| `--root <경로>` | 프로젝트 루트를 명시적으로 지정 |
| `--config <파일>` | 사용자가 지정한 설정 파일 사용 |
| `--no-refresh` | 변경 확인·갱신 없이 마지막 정상 인덱스 조회 |
| `--verify` | 조회 전 전체 분석 대상의 내용 해시 확인 |
| `--direction in\|out\|both` | 관계 방향 지정. `related`와 `list` 기본값은 `both` |
| `--relation <종류>` | 관계 종류 필터. 여러 번 지정하면 합집합 |
| `--depth N` | 간접 탐색의 최대 깊이 |
| `--transitive` | 깊이 제한 없는 도달 탐색 |
| `--members` | 타입·클래스의 멤버도 시드로 포함 |
| `--sites` | 개별 사용 위치 출력 |
| `--at line:column` | 정의 위치로 객체 선택을 구분 |
| `--include-candidates` | 후보 관계를 상태 표시와 함께 포함 |
| `--include-external` | 외부 package·module·객체도 포함 |
| `--format text\|json\|jsonl\|tsv` | 출력 형식 선택 |
| `--limit N` | 출력 행 제한. 기본 제안값 200 |
| `--all` | 출력 행 제한 해제 |
| `--max-nodes N` | 탐색 방문 객체 제한. 기본 제안값 100,000, `0`은 명시적 상한 해제 |
| `--jobs N` | 분석 워커 수 조정 |
| `--memory-budget <크기>` | 분석·탐색 작업 메모리 예산 조정 |
| `--max-file-bytes N` | 파일당 분석 입력 크기 상한 조정 |

다음 조합은 인자 오류다.

- `--no-refresh`와 `--verify` 동시 지정.
- `--depth`와 `--transitive` 동시 지정.
- `deps`에 역방향, `refs`에 정방향을 강제하는 옵션.
- 알 수 없는 관계 이름이나 잘못된 숫자·크기 값.

경로에 공백이 있으면 셸 규칙에 맞게 인용한다. `path` 선택자는 파일 경로와 객체 한정 이름을 구조적으로 분리하며, Windows drive나 객체 이름의 `::`를 단순 문자열 분리로 잘못 처리하지 않는다.

## 10. 프로젝트 루트와 설정

### 10.1 루트 식별

| 상황 | 동작 |
|---|---|
| 현재 폴더에서 최초 `build` | 해당 폴더를 프로젝트 루트로 등록 |
| 명시 경로로 `build` | 지정한 경로를 루트로 등록 |
| 등록된 프로젝트의 하위 폴더에서 조회 | 현재 위치에서 가장 가까운 등록 상위 루트를 사용 |
| `--root` 지정 | 자동 탐색보다 우선 |
| 중첩 프로젝트 | 더 가까운 등록 루트가 우선 |
| 같은 이름·다른 경로의 프로젝트 | 서로 다른 캐시 사용 |
| 같은 실제 루트의 symlink | canonical 경로로 동일 프로젝트로 식별 |
| 프로젝트 이동 | 새 경로의 별도 캐시 사용. 예전 캐시는 수명 정책에 따라 정리 |

Git 루트를 근거로 사용자가 등록하지 않은 상위 범위를 자동 분석하지 않는다. 조회의 파일 경로는 현재 실행 위치에서 해석한 뒤 선택한 루트 안의 프로젝트 상대 경로로 정규화한다. 결과 경로는 프로젝트 상대 경로를 기본으로 표시한다.

### 10.2 설정 우선순위

**내장 기본값 → 전역 설정 → `--config`로 지정한 파일 → CLI 옵션** 순서다.

기본 전역 설정 위치는 `$HOME/.kodstar/config.toml`이다. 설정이 없어도 실행할 수 있다. 프로젝트 내부에 인덱스나 설정 파일을 자동 생성하지 않는다. 저장소에 직접 작성한 `kodstar.toml`을 사용하려면 `--config`로 지정한다.

설정 대상은 제외 규칙, 언어 override, 워커·메모리·파일 크기 제한, 기본 결과 한도, 캐시 수명과 용량이다. 분석 결과에 영향을 주는 설정 변경은 인덱스 재해석 또는 재분석 사유가 된다.

## 11. 자동 증분 갱신과 최신성

### 11.1 명령의 공통 증분 동기화

첫 build 이후의 일반 명령은 공통 실행 인터페이스를 통해 기본적으로 다음 동작을 포함한다. 명령별 구현은 공통 준비를 복제하지 않고 실행 책임과 필요한 정책 차이만 확장한다.

1. 프로젝트와 기존 인덱스를 확인한다.
2. 분석 범위의 파일·설정 추가·수정·삭제를 확인한다.
3. 변경이 없으면 파싱과 인덱스 쓰기를 생략한다.
4. 변경 파일을 재분석하고 영향을 받는 참조를 다시 해석한다.
5. 정상 완료한 인덱스 세대에서 해당 명령을 실행한다.

사용자는 소스를 수정할 때마다 수동 `build`를 반복할 필요가 없다.

특정 파일을 조회해도 다른 파일에서 해당 객체를 새로 참조하거나 삭제했을 수 있으므로, 기본 변경 확인은 프로젝트 전체 metadata 스캔이다. 그 비용은 파일 수에 비례하며 순수 인덱스 조회 시간과 구분한다.

### 11.2 변경 종류별 반영

| 변화 | 반영 대상 |
|---|---|
| 함수 본문 수정 | 해당 파일의 구문 사실과 참조 |
| export·타입·서명·상속 수정 | 관련 이름·타입·모듈을 조회한 참조와 재수출 경로 |
| 새 파일·새 객체 추가 | 이전 미해결 참조와 기존 유일 후보의 모호함 여부 |
| 파일·객체 삭제 | 삭제된 대상을 향하던 관계와 관련 이름 조회 |
| 파일 rename | 삭제·추가에 해당하는 변경과 모듈 경계 |
| manifest·alias·ignore 변경 | 영향을 받는 스캔 범위·모듈·해석 규칙 |
| grammar·추출기 변경 | 영향을 받는 언어의 구문 사실 |
| 해석기 변경 | 기존 사실이 충분하면 재파싱 없이 관계 재해석 |

파일 내용 해시가 같다고 해서 그 파일의 관계를 무조건 재사용하지 않는다. 상대 파일의 export가 바뀌면 자기 파일이 그대로여도 연결 대상이 달라질 수 있다.

**성공한 이름 조회뿐 아니라 실패한 조회의 의존성도 보존한다.** 나중에 정의가 추가되면 미해결이 해소되고, 새 동명 객체가 추가되면 이전 확정 연결이 모호해질 수 있다.

### 11.3 최신성 표시

| `freshness` | 의미 |
|---|---|
| `metadata_checked` | metadata 기반 변경 확인을 수행 |
| `content_verified` | 전체 분석 대상의 내용 해시 확인을 수행 |
| `not_checked` | `--no-refresh`로 기존 인덱스를 바로 조회 |

size·mtime·가능한 ctime과 최근 기록 시각을 빠른 변경 확인에 활용한다. metadata 일치만으로 내용이 절대 같다고 보장하지 않는다. 같은 크기·timestamp 충돌까지 내용으로 확인하려면 `--verify`를 사용한다.

읽는 동안 바뀐 파일은 다시 확인한다. 인덱스 세대의 원자성과 편집 중인 파일시스템 전체의 동시 snapshot은 구분한다.

## 12. 분석 관계와 객체 정체성

### 12.1 관계 종류

| 관계 | 의미 |
|---|---|
| `calls` | 확인 가능한 함수·메서드 호출 |
| `indirect_call` | 이름으로 전달·보관한 함수 값의 잠재 호출 후보. 실제 실행을 확정하지 않음 |
| `references` | 객체의 명시적인 사용·참조 |
| `uses_type` | 타입 annotation·생성·서명 등에서의 타입 사용 |
| `imports` | 파일·모듈·객체 import |
| `reexports` | 외부 이름의 재수출 |
| `includes` | 소스 include |
| `inherits` | 타입 상속 |
| `mixes_in` | 언어가 제공하는 mixin·유사 합성 관계 |
| `implements` | interface·trait·protocol 구현 |
| `overrides` | 상위 메서드 재정의 |
| `dispatch_candidate` | 정적으로 발견한 호출 구현 후보 |
| `defines` | 파일·모듈이 객체를 정의함 |
| `contains` | 타입·스코프가 멤버를 소유함 |
| `template_include` | 템플릿의 include·component 참조 |
| `template_extends` | 템플릿 layout·상속 관계 |
| `depends_on` | 프로젝트·패키지의 명시적 의존 |
| `event_handler` | UI·form에서 명시한 handler 연결 |

함수 값의 확인된 대상은 `references`로 보존하고, 잠재 실행은 `indirect_call` 후보로 분리한다. 콜백을 실행하지 않는 소비자도 있을 수 있으므로 직접 `calls`로 승격하지 않는다.

각 관계의 코드·이름·도메인은 버전 있는 registry로 관리한다. 출력 metadata의 `relation_registry_version`은 인덱스에 사용한 관계 registry를 표시한다. 모든 언어가 모든 관계를 제공하지는 않는다. Graphify의 원본 관계와 context는 의미에 맞게 정규화하며, 서로 다른 관계를 `calls` 하나로 합치지 않는다.

자기 재귀와 상호 호출을 보존한다. 같은 객체 쌍 사이의 호출·타입 사용·상속도 별도 관계로 유지한다. 같은 관계가 여러 위치에서 발생하면 개별 사용 위치를 보존하고, 기본 출력에서는 집계할 수 있다.

일반 변수·필드의 완전한 reads/writes와 alias·dataflow 분석은 기본 보장에 포함하지 않는다. 바인딩을 위해 지역 변수·필드를 추적하는 것과 프로그램 전체 데이터 흐름을 분석하는 것은 다르다.

### 12.2 객체 식별

객체의 논리 정체성은 언어, 패키지·모듈, 소유 체인, 원래 이름, 종류, 서명·조건, 필요한 동명 구분자를 보존한다.

- 표시 이름과 내부 정수 ID를 구분한다.
- 이름을 일괄 소문자로 바꾸거나 유사한 이름끼리 병합하지 않는다.
- 언어별 대소문자·이름 비교 규칙을 적용한다.
- 행 번호만으로 객체 정체성을 만들지 않는다.
- 이름·모듈·scope가 바뀌면 논리 키도 달라질 수 있다.
- 내부 DB ID가 전체 재빌드 사이에도 같다고 약속하지 않는다.

partial 선언·분리된 구현처럼 의미상 같은 객체의 여러 정의 위치는 함께 보존한다. 동명이라는 이유만으로 합치지는 않는다. 대표 정의 파일이 삭제돼도 다른 정의가 남으면 객체와 나머지 위치를 유지한다.

파일 자체도 내부 객체를 가져 top-level import·모듈 초기화 호출 등의 source가 될 수 있다.

## 13. 해석 상태와 분석 한계

### 13.1 구문 발견과 대상 확정의 구분

호출 문법을 발견했다는 사실과 호출 대상을 알아냈다는 사실은 다르다. 구문 추출 근거와 대상 해석 상태를 독립적으로 보존한다.

| 상태 | 의미 |
|---|---|
| `resolved` | 등록한 해석 규칙으로 대상을 식별 |
| `candidate` | 추가 가정이 필요한 후보 관계 |
| `ambiguous` | 여러 후보가 남아 하나로 결정할 수 없음 |
| `unresolved` | 대상을 찾거나 식별할 수 없음 |

프로젝트 내부·외부 여부는 위 상태와 별도로 표시한다. 예를 들어 외부 패키지를 확정적으로 식별할 수도 있고, 내부 후보가 여러 개여서 모호할 수도 있다.

기본 탐색은 확정한 프로젝트 내부 관계를 사용한다. 후보·외부 객체를 포함하면 해당 상태와 출처가 출력에 남는다. 설명 결과에서는 미해결 이유와 후보 정보를 숨기지 않는다.

### 13.2 해석 근거

우선 확인하는 근거는 lexical scope·shadowing·언어별 가시성, 같은 모듈의 정의, 명시적 import·별칭·재수출, 결정 가능한 receiver·소유 타입이다.

프로젝트 전체에서 같은 이름이 하나 발견되거나 가까운 폴더에 있다는 이유만으로 확정 연결을 만들지 않는다. 경로 근접도 같은 보조 정보가 있더라도 후보 정렬과 설명에 한정한다. 표준 내장 이름과 같은 사용자 정의도 먼저 해당 언어의 바인딩 규칙으로 해석한다.

임의의 신뢰도 숫자를 정확도 확률처럼 보여주지 않는다. 적용한 규칙과 근거 위치를 제공한다.

### 13.3 완전 분석을 보장하지 않는 영역

- 동적 import·동적으로 생성한 이름·property.
- reflection·eval·monkey patching·metaprogramming.
- 매크로·proc macro·언어의 임의 compile-time 실행.
- 복잡한 overload·generic 추론·완전한 alias 해석.
- 런타임 가상 dispatch·동적 DI·함수 포인터의 모든 대상.
- 빌드 옵션·전처리·조건부 선언의 모든 조합.
- 프레임워크가 런타임이나 컴파일 단계에서 생성하는 전체 관계.

조건부 선언·Rust cfg·Go build tag·C/C++ 전처리 조건은 관측한 조건을 보존한다. 여러 조건의 소스를 함께 읽은 결과를 특정 빌드 프로파일의 실행 결과처럼 표시하지 않는다.

## 14. 출력과 오류 계약

### 14.1 출력 형식

| 형식 | 목적 |
|---|---|
| text | 사람이 터미널에서 읽는 관계 목록 |
| JSON | 외부 도구가 읽는 envelope·결과 배열·요약 |
| JSONL | 큰 결과와 에디터 연동을 위한 이벤트 단위 스트리밍 |
| TSV | 셸 파이프·간단한 후처리 |

stdout에는 선택한 결과 형식만 기록한다. 자동 갱신 진행·경고·진단은 stderr로 보낸다. JSON 결과에 진행 메시지를 섞지 않는다.

```text
$ kodstar related src/order.rs create_order

대상: create_order  src/order.rs:24:1

사용하는 객체
  calls      Payment::charge  src/payment.rs:48:5  사용 src/order.rs:31:9
  uses_type  Order            src/models.rs:8:1    사용 src/order.rs:24:20

사용하는 쪽
  calls      submit_order     src/api.rs:73:1      사용 src/api.rs:81:5

미해결 참조: 1개
```

예시는 가상 결과다. 객체의 정의 위치와 그 객체를 사용한 위치를 분리해 표시한다.

### 14.2 공통 메타데이터

| 필드 | 의미 |
|---|---|
| `schema_version` | 출력 계약 버전 |
| `relation_registry_version` | 해당 인덱스의 관계 종류 registry 버전 |
| `engine_version` | 분석 엔진 버전 |
| `root` | 분석 프로젝트 루트 |
| `generation` | 조회한 인덱스 세대 |
| `indexed_at` | 인덱스 생성·갱신 시각 |
| `freshness` | metadata 확인·내용 검증·미확인 구분 |
| `complete` | 요청한 인덱스 탐색·출력을 완료했는지 |
| `truncated` | 결과·방문 한도 때문에 잘렸는지 |
| `coverage` | 분석 범위·스킵·실패·미지원·capability 정보 |
| `unresolved_count` | 미해결 참조 수 |

`complete=true`가 런타임 전체 관계의 완전성을 뜻하지 않는다. 선택한 인덱스와 필터 안의 요청 완료 여부다.

JSONL은 metadata 이벤트, result 이벤트들, 마지막 summary 이벤트 순서다. 마지막 summary가 없으면 중간 취소·오류·파이프 종료 등으로 끝났을 수 있으므로 완료 결과로 취급하지 않는다.

```jsonl
{"type":"metadata","schema_version":1,"relation_registry_version":2,"root":"/work/project","generation":7,"freshness":"metadata_checked"}
{"type":"result","relation":"calls","direction":"out","resolution":"resolved","source":{"name":"create_order","file":"src/order.rs"},"target":{"name":"Payment::charge","file":"src/payment.rs","line":48,"column":5},"site":{"file":"src/order.rs","line":31,"column":9}}
{"type":"summary","complete":true,"truncated":false,"result_count":1,"unresolved_count":0}
```

이 예시는 출력 구조 설명용이다. 실제 결과에는 선택한 명령에 따라 객체 논리 키·byte range·해석 규칙·provider·coverage를 포함한다.

### 14.3 위치·경로·정렬

내부 위치는 UTF-8 byte offset과 range를 기준으로 보존한다. text의 line·column은 1부터 시작하며 column은 UTF-8 바이트 열이다. VS Code adapter는 이를 에디터의 위치 단위로 변환한다.

비 UTF-8 경로는 표시 문자열 외에 platform과 손실 없는 OS 경로 bytes의 base64 표현을 제공한다. 표시용 문자열을 객체 정체성이나 파일 재접근의 유일한 기준으로 삼지 않는다.

기본 결과는 관계·프로젝트 상대 경로·정의 위치·한정 이름으로 결정적으로 정렬한다. 깊이 탐색은 깊이 순서를 우선하고 같은 깊이 안에서 정렬한다. 워커 완료 순서나 hash map 순서에 따라 결과가 바뀌지 않는다.

### 14.4 종료 코드

| 코드 | 의미 |
|---|---|
| `0` | 성공. 대상은 존재하지만 연결이 없는 빈 결과 포함 |
| `1` | 실행·I/O·저장소 오류 |
| `2` | 인자 오류 |
| `3` | 한도 초과·불완전 탐색·잘린 결과 |
| `4` | 인덱스·대상 없음 또는 객체 선택 모호함 |
| `130` | 사용자 취소 |

한도 때문에 잘린 결과는 데이터와 요약에 이유를 남긴다. 사용자가 pipe를 닫으면 불필요한 backtrace를 출력하지 않고 종료한다.

## 15. 캐시 위치와 수명

### 15.1 저장 위치

프로젝트명에 맞춰 기본 홈 디렉터리는 **`$HOME/.kodstar`**로 한다. Windows에서는 해당 사용자의 홈 디렉터리를 해석한다.

| 경로 | 내용 |
|---|---|
| `$HOME/.kodstar/config.toml` | 전역 설정 |
| `$HOME/.kodstar/<path-hash>/project.json` | 원래·정규화 루트, 캐시 키 버전, 최근 사용 정보 |
| `$HOME/.kodstar/<path-hash>/index.sqlite` | 객체·구문 사실·바인딩·관계·진단·세대 |
| `$HOME/.kodstar/<path-hash>/index.sqlite-wal` | SQLite의 변경 기록 보조 파일 |
| `$HOME/.kodstar/<path-hash>/index.sqlite-shm` | SQLite의 공유 상태 보조 파일 |
| `$HOME/.kodstar/<path-hash>/staging/` | 아직 공개하지 않은 분석 데이터 |
| `$HOME/.kodstar/<path-hash>/lease.lock` | 사용 중인 캐시의 삭제 방지 |
| `$HOME/.kodstar/<path-hash>/writer.lock` | 중복 갱신의 직렬화 |
| `$HOME/.kodstar/gc-state.json` | 캐시 정리 진행 정보 |

`path-hash`는 플랫폼과 canonical 절대 경로의 손실 없는 표현을 바탕으로 만든다. 내용·경로 fingerprint는 BLAKE3를 기준으로 한다. 해시와 저장한 실제 루트 정보를 함께 확인한다.

프로젝트 폴더 이름이 같아도 경로가 다르면 다른 캐시다. 프로젝트 내부에 분석 DB나 전체 graph 파일을 생성하지 않는다. 캐시에는 전체 소스 본문과 AST를 영구 보관하지 않는다.

### 15.2 수명 정책

| 항목 | 기본 설계 |
|---|---|
| 미사용 기간 | 마지막 사용 후 30일을 정리 후보 기준으로 제안 |
| 총 캐시 예산 | 2 GiB를 기본 제안값으로 사용 |
| 자동 정리 시점 | KodStar 실행 시 제한된 빈도로 수행. 기본 제안은 하루 최대 1회 |
| 사용 중인 캐시 | 조회·빌드·감시·세션이 사용 중이면 삭제하지 않음 |
| 예산 초과 | 오래 사용하지 않은 정리 가능 캐시부터 처리 |
| 정리할 후보 없음 | 활성 캐시를 억지로 삭제하지 않고 상태를 표시 |
| 수동 삭제 후 | 필요하면 `build`로 재생성 |

기간·용량·정리 빈도는 설정할 수 있다. 정확히 30일이 되는 시각에 실행되는 예약 작업은 아니다. KodStar를 실행하지 않으면 자동 정리도 수행되지 않는다.

일시적인 경로 접근 실패나 프로젝트 삭제 감지만으로 즉시 캐시를 지우지 않는다. GC는 소스, 사용자가 작성한 설정, 홈의 전역 설정을 삭제하지 않는다. 대량 캐시 순회와 용량 계산은 일반 조회의 첫 응답을 과도하게 지연시키지 않도록 작업량을 제한한다.

## 16. 저장 일관성과 장애 시 동작

정상 완료한 분석 결과만 새 인덱스 세대로 공개한다. 정의·바인딩·사실·관계·진단·세대 정보가 서로 다른 분석 상태로 섞이지 않도록 한다.

- 갱신 작성자는 프로젝트별 하나로 직렬화한다.
- 동시 갱신 요청은 먼저 끝난 작업의 세대와 변경 상태를 다시 확인한다.
- 조회는 하나의 커밋된 세대에서 일관된 결과를 읽는다.
- 중단·디스크 부족·치명적 읽기 실패는 미완료 세대를 공개하지 않는다.
- 마지막 정상 인덱스는 보존한다.
- 자동 갱신 실패 시 이전 결과를 최신 결과처럼 조용히 반환하지 않는다.
- 이전 결과를 바로 읽으려면 `--no-refresh`를 사용하고 미확인 최신성을 표시한다.
- 구문 오류·부분 추출은 정상 완료 캐시로 승격하지 않는다.
- 설정상 제외·미지원·크기 제한으로 건너뛴 파일은 실패와 구분해 coverage에 표시한다.

폴더 일부를 읽지 못한 스캔을 정상 전수 스캔으로 취급하지 않는다. 확인하지 못한 하위 파일을 삭제된 것으로 제거하지 않는다. 객체 수가 줄었다는 이유만으로 갱신을 거부하지 않고 실제 삭제·추출 완료 근거로 판단한다.

`--full`도 새 분석이 정상 완료되기 전에 기존 정상 데이터를 없애지 않는다. 손상되거나 호환되지 않는 캐시는 정상 인덱스처럼 사용하지 않고 진단과 재생성 경로를 제공한다.

## 17. 메모리·속도·용량 목표

### 17.1 핵심 자원 원칙

| 영역 | 설계 목표 |
|---|---|
| 프로젝트 전체 | 파일 목록·소스·AST·객체 그래프 전체를 동시에 RAM에 유지하지 않음 |
| 소스 읽기 | 변경 파일 위주로 읽고 가능한 한 같은 바이트 버퍼로 해시·파싱 |
| 구문 트리 | 파일 처리가 끝나면 해제. AST 전체의 장기 cache 금지 |
| parser | 필요한 언어만 지연 초기화하고 제한된 pool로 재사용 |
| 데이터 표현 | 정수 ID·원문 range·관계 enum으로 반복 문자열과 범용 속성 맵 축소 |
| 병렬 처리 | CPU 개수만이 아니라 메모리 예산으로 동시 작업 수 제한 |
| 대기열 | 항목 개수와 전체 바이트 수를 함께 제한 |
| 이름·경로 cache | 크기가 제한된 cache만 허용 |
| 직접 조회 | 필요한 방향의 인덱스에서 시작해 표시할 객체만 읽음 |
| 전이 탐색 | 방문 집합·frontier가 커지면 디스크 임시 저장 사용 |
| 출력·정렬 | 전체 결과 배열을 RAM에 만들지 않고 스트리밍·디스크 정렬 활용 |
| 시작 비용 | 도움말·버전·간단한 조회에 불필요한 grammar·분석기 초기화 금지 |

SQLite를 쓰는 이유는 전체 그래프를 메모리 객체로 복원하지 않고 필요한 관계만 디스크 인덱스에서 읽기 위해서다. 별도 DB 서버는 필요 없다.

tree-sitter는 파일 하나의 구문 트리를 여전히 만든다. 입력 callback을 사용한다고 AST 메모리가 사라지는 것은 아니다. worker·입력 버퍼·구문 트리·fact 배치·DB cache·출력 버퍼를 함께 고려한다.

### 17.2 조정 가능한 자원 기본값

| 항목 | 초기 제안값 |
|---|---|
| 분석 워커 | 2개 |
| 분석 작업 메모리 예산 | 256 MiB |
| 파일당 입력 상한 | 2 MiB |
| fact 배치 | 최대 1 MiB |
| 대기 데이터 합계 | 8 MiB |
| DB page cache | connection별 약 8 MiB, connection 수 제한 |
| 일반 결과 행 수 | 200개 |
| 탐색 방문 객체 수 | 100,000개 |

이 값은 변경 가능한 기본 설계다. 파일 상한 초과와 자원 제한은 진단·coverage에 표시하며 옵션으로 조정한다. 메모리 예산은 작업 스케줄링 목표이고 임의 입력에 대한 OS 수준의 hard RSS 보장은 아니다.

### 17.3 성능을 판단하는 기준

조회 시간은 변경 확인·파싱·해석·DB 쓰기·탐색·출력으로 구분한다. 전체 metadata 스캔을 포함한 실제 사용자 지연과 `--no-refresh`의 인덱스 조회 지연을 별도로 본다.

기존 설계의 직접 조회 p95 100 ms·peak RSS 64 MiB, 기본 인덱싱 256 MiB, 영향이 작은 변경의 갱신 p95 500 ms는 비교 환경을 고정해 검토할 초기 목표다. 이미 달성한 수치가 아니다. 작은 변경 갱신 목표는 전체 파일 metadata 스캔 비용을 제외한 조건이다.

모든 성능 수치는 OS·CPU·스토리지·빌드 조건·grammar·대상 파일과 바이트·객체·관계·미해결·스킵 수를 함께 기록해야 의미가 있다. 분석 대상을 덜 읽거나 관계를 덜 추출해서 빨라진 것을 같은 분석의 성능 개선으로 표시하지 않는다.

## 18. 엔진과 프런트엔드 계약

Rust 라이브러리 `kodstar-core`는 CLI 없이도 프로젝트 선택·분석·조회·진단을 호출할 수 있어야 한다.
CLI와 에디터의 요청에도 같은 최신성·해석 상태·한도 정책을 적용한다.

| 경계 | 계약 |
|---|---|
| 요청 | 프로젝트·설정·조회·한도·최신성 옵션을 구조화된 값으로 전달 |
| 결과 | 객체·관계·위치·metadata·진단·완료 상태를 구조화된 값으로 반환 |
| 진행·취소 | 호출자가 진행 정보를 받고 소유 작업에 취소를 전달 |
| 표시 | 호출자가 결과의 출력 형식과 UI를 결정 |

엔진은 터미널 색상·stdout/stderr·VS Code API에 의존하지 않는다.
기본 실행에는 Python·Node·언어 컴파일러·DB 서버·모델 API가 필요하지 않다.
모듈 배치와 저장 테이블은 [아키텍처 문서](docs/architecture.md)에 정의한다.

## 19. 파일 감시·에디터 연동

### 19.1 파일 감시

`watch`는 사용자가 실행한 동안만 동작한다. 변경 이벤트를 모아 갱신하고, 이벤트 누락·overflow·재시작이 있으면 metadata 재스캔으로 보완한다. 이벤트만을 진실로 간주해 삭제나 변경을 놓치지 않는다.

다른 CLI 조회와 감시 프로세스도 같은 writer·lease·인덱스 세대 규칙을 따른다.

### 19.2 VS Code와 세션

VS Code 확장은 TypeScript 프런트엔드로서 CLI의 JSON/JSONL 또는 `serve --stdio` 세션을 사용한다.

- 파일·선택 객체의 관계 목록.
- 정의·사용 위치로 이동.
- 정방향·역방향·관계 종류·깊이 필터.
- 후보·미해결·최신성·coverage 표시.
- 필요한 경우 Webview 그래프.
- 진행 표시·요청 취소.

세션 protocol은 버전·요청 ID·루트·결과·진행·취소를 구분한다. 확장은 SQLite를 직접 수정하거나 별도의 분석기를 복제하지 않는다. 원격 VS Code에서는 소스가 있는 workspace host에서 엔진을 실행한다.

adapter는 소유 CLI subprocess 하나와 최대 3개의 요청을 유지한다. 응답 프레임은 2 MiB,
요청별 표시 버퍼는 1,000행·8 MiB로 제한하며 초과 시 취소와 잘림을 표시한다.
세션 재시작·deactivate는 이전 프로세스 종료를 기다린다. 비 UTF-8 경로 이동은 현재 지원하지 않는다.

기본 분석 대상은 디스크에 저장된 파일이다. 저장하지 않은 에디터 버퍼의 overlay는 별도 확장 범위로 두고, 임시 내용이 영구 캐시를 덮어쓰지 않도록 한다. compiler·표준 SCIP 등 외부 의미 정보가 필요하면 출처를 구분하는 provider로 결합할 수 있으나 기본 실행 의존성으로 요구하지 않는다.

### 19.3 Stdio 세션 protocol 1

`kodstar serve --stdio`는 UTF-8 JSON 요청을 한 줄씩 받는다. stdout에는 protocol 이벤트만,
stderr에는 프로세스 진단만 쓴다. 요청은 `protocol_version: 1`, 문자열 또는 정수 `id`,
`method`, 선택적인 `params`를 가진다. 문자열 ID는 1~256바이트이며 활성 요청 간에 중복할 수 없다.

```jsonl
{"protocol_version":1,"id":"build-1","method":"build","params":{"project":{"root":"/work/project"}}}
{"protocol_version":1,"id":"query-1","method":"query","params":{"project":{"root":"/work/project"},"query":{"kind":"deps","file":"src/api.rs","name":"caller"},"options":{"refresh":"none","relations":["calls"]}}}
{"protocol_version":1,"id":"cancel-1","method":"cancel","params":{"request_id":"query-1"}}
{"protocol_version":1,"id":"shutdown-1","method":"shutdown"}
```

| method | params |
|---|---|
| `build` | 선택적인 `project`, `path`, `full`, `verify` |
| `query` | 필수 `query`, 선택적인 `project`, `options` |
| `status`, `doctor` | 선택적인 `project`, `options.refresh` |
| `languages` | 선택적인 `project`, `options.refresh`; 등록 프로젝트가 없으면 전역 보고 |
| `cache` | 필수 `action` (`list`, `clean`, `gc`), 선택적인 `project`, `all`, `dry_run`; list만 `options.refresh` 허용 |
| `cancel` | 필수 `request_id` |
| `shutdown` | 빈 객체 또는 생략 |

`project`는 `root`, `config`, `jobs`, `memory_budget`, `max_file_bytes`를 받는다.
크기는 정수 바이트다. 경로는 세션 시작 디렉터리 기준이며, 루트·최신성·분석·한도는
CLI와 같은 엔진 정책을 따른다. `query.kind`는 `symbols`, `list`, `related`, `deps`, `refs`,
`explain`, `impact`, `path`, `search`다. 파일·이름 인자는 CLI와 같은 뜻이고 `at`는
`[line, byte_column]`, `path`의 `from`·`to`는 `file::qualified-name` 문자열이다.
`options`는 CLI 조회 옵션의 snake_case 이름과 `relations` 배열을 사용한다.
`refresh`는 `metadata`(기본), `verify`, `none` 중 하나다.

결과·진행 이벤트는 `protocol_version`, `id`, `type`, `data`를 가진다.
`type`은 `metadata`, `result`, `progress`, `summary`다. 객체 내부 ID는 `data.id`에 있으므로
요청 ID와 충돌하지 않는다. `error`는 최상위 `code`·`message`, 마지막 `complete`는
최상위 `exit_code`를 가진다. **요청 완료는 `complete`까지 확인한다.**
CLI JSONL의 평면 행 형식과 세션의 요청 envelope를 구분한다.

입력 프레임은 64 KiB, 대기 작업은 2개, 실행 worker는 1개다. 대기열 초과 요청은 실행하지
않고 코드 3으로 응답한다. 취소·종료는 대기열을 거치지 않는다. 중복·알 수 없는 필드와
잘못된 버전·옵션은 코드 2로 거절한다. 큰 프레임은 다음 줄까지 버리고 세션을 계속한다.
EOF는 수락한 요청을 마친 뒤 종료하고, `shutdown`·SIGINT는 소유 요청을 취소한 뒤 worker를 join한다.
기본 입력은 저장된 디스크 파일이며 에디터의 미저장 버퍼 overlay는 받지 않는다.

## 20. Graphify 참고·포팅 범위

### 20.1 기준

Graphify를 분석기 설계의 주요 참고 프로젝트로 사용한다. 선정한 언어·웹 입력의 **구문 추출 규칙, 모듈·이름 해석, 파일 간 후처리, 회귀 사례**를 Rust로 포팅한다.

- 원본 저장소: [Graphify-Labs/graphify](https://github.com/Graphify-Labs/graphify)
- 조사 버전: `0.9.92`
- 조사 브랜치: `v8`
- 고정 커밋: `86c57be8204673c2a1b9849a3292a7dd3ab373d8`
- 공식 설명: [Supported inputs](https://docs.graphify.com/concepts/supported-inputs)

Python CLI를 감싸서 실행하는 방식이 아니라, 선정한 추출·해석 기능을 Rust 엔진 안에 갖는 것이 목표다. 필요한 tree-sitter grammar는 호환되는 고정 revision의 parser·scanner를 사용한다.

원본의 현재 기능과 KodStar의 새 설계를 구분한다. 위의 KodStar 기능표 전체가 이미 Graphify에 같은 형태로 구현되어 있다는 뜻은 아니다.

### 20.2 참고하는 공통 기능

| Graphify 영역 | KodStar에서 참고·포팅하는 기능 |
|---|---|
| `extractors/models.py`, `base.py` | 언어 구성·선언·import/export/alias/use 사실·공통 추출 규칙 |
| `extractors/engine.py`, `extract.py` | 공통 walker·언어별 config·extra walk·정의·호출·타입·receiver 추출 |
| `extractors/resolution.py`, `symbol_resolution.py` | import·별칭·재수출·namespace·스코프·대상 해석 |
| `resolver_registry.py`와 언어별 resolver | 파일 간 멤버·한정 호출·상속 호출 등 후처리 |
| `interface_dispatch.py`, C#/Swift dispatch | 구현·protocol·override 후보 연결 |
| `detect.py`, `rcfile.py` | 파일 판별·선정 언어의 shebang·특수 라우팅·override·제외 규칙 |
| `cache.py`, `watch.py` | fingerprint·불완전 cache 구분·삭제 대상 무효화·변경 배치·동시 갱신 규칙 |
| `affected.py`, `serve.py` | 역방향 영향·멤버 시드·이웃·BFS·경로 탐색의 개념 |
| `build.py`, `validate.py` | 방향·정확 중복·endpoint 무결성 검증 |
| `manifest_ingest.py`, 선정한 포맷 추출기 | 코드 소속·모듈 경계·명시적 프로젝트·템플릿 연결 |
| `tests/` | 동명·shadowing·receiver·import·방향·캐시 무효화 등 회귀 사례 |

### 20.3 그대로 가져오지 않는 구조와 규칙

| 원본의 구조·기법 | KodStar 설계 |
|---|---|
| Python dict·전체 결과 목록·NetworkX 그래프 | Rust의 제한된 배치·정수 ID·SQLite 인덱스 |
| 전체 graph 파일을 읽어 메모리 그래프 복원 | 필요한 관계와 표시 정보만 읽는 조회 |
| 전체 파일별 결과를 유지하는 전역 후처리 | 저장한 scope·binding·fact를 사용하는 해석 |
| slug·이름 정규화 중심 ID | 언어·모듈·scope·원래 이름을 보존한 논리 정체성 |
| 유사도 기반 객체 병합 | 정확한 정체성과 위치·관계 기준 중복 제거 |
| 전역 유일 이름·경로 근접도 추측 | 명시적 binding 우선, 근거 부족은 후보·미해결 |
| 단일 추출/추론 표식 | 구문 provenance와 대상 해석 상태를 독립 표시 |
| JSON 위주의 분석 캐시 | 사실·바인딩·조회 의존·관계를 저장한 증분 인덱스 |
| 자연어·의미 그래프·군집·클라우드 기능 | 파일·객체 기반의 명시적 로컬 CLI |

Rust 포팅 자체가 원본보다 정확한 분석을 자동으로 만들어 주지는 않는다. 메모리와 실행 구조의 개선, 지원 구문의 보존, 오연결 수정은 각각 구분해서 판단한다.

### 20.4 동작 보존과 분석 품질

선정한 언어의 원본 지원 구문·관계·framework 규칙을 기준으로 기능 대응을 확인한다. 내부 ID 문자열이나 DB 행 순서 대신 이름·scope·정의/사용 위치·방향·관계·조건·외부 여부를 정규화해 비교한다.

원본의 유용한 관계를 대량으로 누락하고 포팅 완료라고 표시하지 않는다. 동시에 원본의 오연결을 그대로 재현하는 것을 목표로 삼지도 않는다. 다른 결과에는 근거·제한·대체 규칙을 남긴다.

정상 전체 분석과 증분 갱신의 결과는 같은 입력·설정에서 정규화한 관계가 같아야 한다. 대소문자·동명·scope shadowing·별칭·재수출·타입·receiver·추가·삭제·새 후보·실패한 lookup의 해소를 포함해 일관성을 확인한다.

현재 근거는 고정 소스와 확인한 회귀 테스트 소스다. 모든 언어를 실제 프로젝트에서 실행해 정확도를 측정한 상태로 설명하지 않는다. `languages`, `status`, `doctor`, 결과 coverage가 실제 제공 기능과 제한을 드러내야 한다.

## 21. 참고 자료

- [Graphify 고정 버전의 추출 dispatch와 언어별 규칙](https://github.com/Graphify-Labs/graphify/blob/86c57be8204673c2a1b9849a3292a7dd3ab373d8/graphify/extract.py)
- [Graphify 공통 추출 엔진](https://github.com/Graphify-Labs/graphify/blob/86c57be8204673c2a1b9849a3292a7dd3ab373d8/graphify/extractors/engine.py)
- [Graphify 모듈·이름 해석](https://github.com/Graphify-Labs/graphify/blob/86c57be8204673c2a1b9849a3292a7dd3ab373d8/graphify/extractors/resolution.py)
- [Graphify 언어별 resolver registry](https://github.com/Graphify-Labs/graphify/blob/86c57be8204673c2a1b9849a3292a7dd3ab373d8/graphify/resolver_registry.py)
- [Graphify 캐시](https://github.com/Graphify-Labs/graphify/blob/86c57be8204673c2a1b9849a3292a7dd3ab373d8/graphify/cache.py)
- [Graphify 변경 영향 조회](https://github.com/Graphify-Labs/graphify/blob/86c57be8204673c2a1b9849a3292a7dd3ab373d8/graphify/affected.py)
- [Tree-sitter Rust API](https://docs.rs/tree-sitter/latest/tree_sitter/struct.Parser.html)
- [SQLite WAL](https://www.sqlite.org/wal.html)
- [rusqlite](https://github.com/rusqlite/rusqlite)
- [BLAKE3](https://github.com/BLAKE3-team/BLAKE3)
- [VS Code Extension API](https://code.visualstudio.com/api)
