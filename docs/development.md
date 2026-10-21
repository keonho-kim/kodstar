# 개발과 검증

이 문서는 환경 준비, 빌드·검증, 의존성 관리와 포팅 근거를 다룬다.
제품 계약은 [기능 명세](../kodstar-spec.md), 코드 구조는 [아키텍처](architecture.md),
변경 절차와 작성 원칙은 [AGENTS.md](../AGENTS.md)를 따른다.

## 개발 환경

고정 toolchain과 component는 [rust-toolchain.toml](../rust-toolchain.toml),
edition과 최소 Rust 버전은 [Cargo.toml](../Cargo.toml)에 정의한다.
[rustup](https://rust-lang.github.io/rustup/installation/index.html)과 네이티브 C 빌드 도구가 필요하다.
실행 지원 대상은 macOS와 Linux이며, Windows는 지원하지 않는다.
macOS는 Xcode Command Line Tools, Linux는 C toolchain을 준비한다.

저장소 루트에서 고정 toolchain을 설치하고 선택을 확인한다.

```bash
rustup toolchain install 1.99.1 --profile minimal --component rustfmt --component clippy --component llvm-tools
rustup show active-toolchain
cargo --version
rustc --version
```

Homebrew 등의 독립 `cargo`·`rustc`가 PATH에서 먼저 선택되면 toolchain 파일이 적용되지 않을 수 있다.
이 경우 아래처럼 toolchain의 실행 경로를 해당 명령에 지정한다. 이후의 Cargo 검사에도 같은 방법을 적용한다.

```bash
kodstar_toolchain_bin="$(dirname "$(rustup which --toolchain 1.99.1 rustc)")"
PATH="$kodstar_toolchain_bin:$PATH" cargo build --workspace --locked
```

일반 빌드는 lockfile의 Rust crate와 저장된 Tree-sitter·SQLite 네이티브 소스를 컴파일한다.
분석 대상 프로젝트의 코드·빌드 스크립트·의존성은 실행하거나 설치하지 않는다.

## 빌드와 실행

```bash
cargo build --workspace --locked
./target/debug/kodstar --help
./target/debug/kodstar build /work/project --format json
./target/debug/kodstar symbols /work/project/src/api.rs --root /work/project
cargo build --workspace --release --locked
```

명령·옵션·종료 코드·최신성은
[CLI 계약](../kodstar-spec.md#7-cli-명령어-전체)에서 확인한다.

## 설정 예제

아래는 개발용 설정 예제다. TOML의 크기 값은 정수 바이트다.
병합 순서와 기본값은 [설정 우선순위](../kodstar-spec.md#102-설정-우선순위)와
[자원 기본값](../kodstar-spec.md#172-조정-가능한-자원-기본값)에 정의한다.

```toml
jobs = 2
memory_budget = 134217728
max_file_bytes = 2097152
exclude = ["generated/**"]

[language_overrides]
"legacy/*.inc" = "pascal"
```

파일을 지정해 적용한다.

```bash
./target/debug/kodstar build /work/project --config /work/kodstar.toml
```

저장·추출 버전은 [index/mod.rs](../crates/kodstar-core/src/infrastructure/index/mod.rs),
관계 registry는 [relations.rs](../crates/kodstar-core/src/domain/relations.rs)에서 관리한다.
호환성 오류와 캐시 재생성 절차는 [저장 일관성](../kodstar-spec.md#16-저장-일관성과-장애-시-동작)을 따른다.

## 변경 검증

변경한 책임의 테스트를 먼저 실행한다. Rust 변경의 공통 검사는 저장소 루트에서 수행한다.

```bash
python3 scripts/check_source.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked
```

엔진 통합 테스트를 따로 실행하거나 이름으로 좁힐 수 있다.

```bash
cargo test -p kodstar-core --test engine --locked
cargo test -p kodstar-core --test engine --locked ejs
```

빌드와 검증은 개발 환경에서 수행한다. GitHub Actions CI는 사용하지 않는다.
문서 변경은 내용·참조·앵커·diff를 확인한다.
산출물은 `target/`, 임시 검증 결과는 `tmp/` 또는 `artifacts/`에 두며,
소형 회귀 fixture는 이를 소비하는 테스트와 함께 관리한다.

## 성능 검증

같은 toolchain·target으로 빌드한 두 release 실행 파일과 변경하지 않을 입력을 준비한다.
macOS에서는 [benchmark.py](../scripts/benchmark.py)로 시간·peak RSS와 결과를 비교한다.
측정 중에는 다른 빌드나 입력 편집을 진행하지 않는다.

```bash
python3 scripts/benchmark.py --baseline /path/to/before-kodstar --candidate target/release/kodstar \
  --project /path/to/frozen-fixture --file src/api.rs --repeats 3 --output tmp/benchmark.json
```

각 실행은 전용 임시 캐시에서 초기 build·변경 없는 동기화 조회·no-refresh 조회를 수행한다.
coverage와 입력·심볼·scope·binding·관계·정체성·조회 의존의 정규화 해시가 다르면 비교를 실패시킨다.
보고서에는 실행 파일 SHA-256, 실행 환경, 개별 시간·RSS와 중앙값을 기록한다.
캐시는 자식 프로세스가 종료하고 미반영 WAL이 없는 상태에서만 immutable로 읽는다.

보고 시에는 재현 입력, 설정, coverage, 미해결·스킵·미지원 수, 결과 동등성을 함께 남긴다.
갱신 포함 시간과 순수 조회 시간을 구분한다. 작업 메모리 예산과 OS가 강제하는 RSS 한도도 구분한다.
구조 변경은 같은 고정 입력의 결과 보존을 확인하며, 언어 의미 변경은 증분·전체 재분석 결과를 비교한다.

## 의존성과 grammar

주요 라이브러리는 GitHub 3,000 stars 이상을 기준으로 선정한다.
언어 grammar·전이 의존성은 승인된 예외로서 고정 버전과 라이선스를 검토한다.
공식 Tree-sitter와 canonical grammar 원본을 사용한다.

| 관리 대상 | 기준 파일 |
|---|---|
| Rust dependency 해석 결과 | [Cargo.lock](../Cargo.lock) |
| 에디터 dependency 해석 결과 | [package-lock.json](../editors/vscode/package-lock.json) |
| 저장소·검토 시점·grammar checksum·revision·ABI | [dependencies.json](../LICENSES/dependencies.json) |
| 라이선스 원문·NOTICE와 배포 고지 | [LICENSES](../LICENSES), [third-party-notices.txt](../LICENSES/third-party-notices.txt) |
| Kotlin 원본과 생성 파일 해시 | [SOURCE.json](../vendor/tree-sitter-kotlin-ng/SOURCE.json) |
| Kotlin grammar 수정 규칙 | [patch.json](../vendor/tree-sitter-kotlin-ng/patch.json) |

의존성 변경은 manifest·lockfile·라이선스/NOTICE·native 번들·grammar ABI를 함께 확인한다.
Swift grammar의 배포 원본처럼 revision만으로 배포물을 재구성할 수 없는 경우에는 crate checksum을 기준으로 한다.

```bash
cargo metadata --locked --format-version 1
python3 scripts/generate_kotlin.py --check
```

Kotlin grammar의 구분자 규칙은 고정 원본에서 재생성한다. 일반 빌드는 저장된 parser를 사용한다.
원본 규칙을 변경할 때만 아래 도구를 설치하고 재생성한다. 생성 C 파일은 직접 수정하지 않는다.

```bash
cargo install tree-sitter-cli --version 0.26.10 --locked --root ./tmp/tree-sitter-cli
python3 scripts/generate_kotlin.py --tree-sitter ./tmp/tree-sitter-cli/bin/tree-sitter
```

## VS Code adapter 개발

에디터 개발에는 Node.js 22와 npm을 사용한다. VS Code API 범위와 개발 의존성은
[package.json](../editors/vscode/package.json)에 정의한다.
Rust 실행 파일을 먼저 빌드한 뒤 저장소 루트에서 실행한다.

```bash
npm ci --prefix editors/vscode --ignore-scripts --no-audit --no-fund
npm run format:check --prefix editors/vscode
npm test --prefix editors/vscode
```

테스트는 `target/debug/kodstar`를 사용하며, 다른 실행 파일은 `KODSTAR_TEST_BINARY`로 지정한다.
세션 검증은 임시 사용자 홈과 프로젝트에서 수행한다.
실제 UI는 `editors/vscode`를 VS Code Extension Development Host로 열고
`kodstar.executable`에 빌드한 실행 파일의 절대 경로를 지정해 확인한다.
원격 환경에서는 workspace host의 실행 파일을 사용한다.

[Stdio protocol](../kodstar-spec.md#193-stdio-세션-protocol-1)은 기능 명세에 정의한다.
adapter의 요청·표시 버퍼와 subprocess 수명은 `client.ts`, 프레임과 자료형은 `protocol.ts`에서 관리한다.
미저장 버퍼와 비 UTF-8 경로 이동은 현재 지원하지 않는다. 자동 세션 테스트와 Extension Host UI 검증을 구분한다.

## 포팅 검증

포팅 기준 원본의 버전·revision은 [dependencies.json](../LICENSES/dependencies.json)의 Graphify 항목에,
라이선스와 원문 고지는 [LICENSES](../LICENSES)에 고정한다.
아래 표는 선정 범위의 규칙군과 직접 소유 모듈을 연결한다. 부분 구현을 완료로 계산하지 않는다.
원본의 전역 유일 이름 추측·유사도 병합은 명시적 근거·후보·미해결로 대체하며, 원본에 없는 기능은 추가 요구로 구분한다.

| 규칙군 | 고정 원본 근거 | KodStar 소유 경로 (`src/` 기준) | 현재 미검증·부분 범위 |
|---|---|---|---|
| 공통 이름·binding | `extractors/models.py`, `resolution.py` | `domain/facts`, `parse`, `analysis/resolve` | namespace·가시성·선언/정의 대응 |
| Python | `engine.py::_python_*`, `_resolve_python_member_calls` | `parse/python`, `resolve/python`, `returns` | fixture yield local·동적 callable·동적 decorator·완전한 추론 |
| JS/TS | `engine.py::_js_*`, `_ts_*`, `resolution.py` | `parse/javascript/imports`, `commonjs`, `resolve/javascript` | 타입/값 공간·동적 decorator/module·receiver |
| Rust | `extractors/rust.py`, `_resolve_rust_self_member_calls` | `parse/rust`, `parse/rust/imports`, `resolve/rust` | visibility·cfg_attr export·macro_use·trait/generic 선택 |
| Go | `extractors/go.py`, `_resolve_go_member_calls` | `parse/go`, `resolve/go`, `auxiliary/go*` | 완전한 서명·pointer method set·MVS/version 선택·vendor |
| Java | `engine.py::_java_*`, `_resolve_java_member_calls` | `parse/fields`, `type_uses`, `return_values`, `resolve/returns` | overload·접근 제한 |
| C/C++ | `engine.py::_cpp_*`, `_resolve_cpp_member_calls`, `_resolve_c_include_edges` | `parse/cxx`, `conditions`, `resolve/cxx` | nested forward·조건부 include·compiler module·macro·template·overload·alias 타입 등가 |
| C# | `extractors/csharp.py`, `engine.py::_csharp_*`, `csharp_dispatch.py` | `parse/csharp`, `return_values`, `resolve/identities`, `dotnet_projects` | generic/partial method·조건부/import/custom build 소속·복수 프로젝트 source instance·dispatch |
| PHP | `engine.py::_php_*`, `_resolve_php_member_calls` | `parse/php`, `parse/php/imports`, `resolve/php` | union/intersection·property 접근 제한·magic/late binding·동적 callable·route closure |
| Kotlin | `engine.py::_kotlin_*`, `_resolve_kotlin_*` | `parse/kotlin`, `call_expressions`, `parse/kotlin/imports`, `resolve` | extension 선택·type alias·정밀 generic/접근·Gradle module |
| Swift | `engine.py::_swift_*`, `_merge_swift_extensions`, `swift_dispatch.py` | `parse/swift`, `resolve/swift`, `swift_projects`, `ownership`, `dispatch` | generic extension·정밀 protocol/접근·testable·로컬 package product·동적 manifest |
| Ruby | `engine.py::_ruby_*`, lookup barrier | `parse/ruby`, `parse/ruby/imports`, `resolve/ruby` | runtime lookup·정밀 refinement/접근·reopen 순서·load path |
| Scala | `engine.py::_scala_*` | `parse/scala`, `parse/scala/imports`, `resolve/scala` | implicit/given 검색·singleton/path-dependent 타입·generic 대입·정밀 접근/override |
| Zig | `extractors/zig.py` | `parse/zig`, `members`, `parse/zig/imports` | C import·build module map·comptime·quoted identifier·값 추론 |
| Elixir | `extractors/elixir.py`, `_resolve_elixir_*` | `parse/elixir`, `parse/elixir/imports`, `resolve/identities` | protocol 런타임 dispatch·다중/동적 구현 target·alias 기반 구현 module 이름·동적 macro·전체 binding |
| Erlang | `extractors/erlang.py` | `parse/erlang`, `resolve/identities` | spec의 함수 소유 대응·전체 record/type 의미·전처리·behaviour/callback 검사 |
| Common Lisp | `extractors/commonlisp.py` | `domain/resolve/lisp`, `parse/lisp`, `resolve/lisp` | 런타임 package 변경·CLOS 적용성·custom definer 평가·custom readtable |
| Pascal | `extractors/pascal.py`, `pascal_forms.py` | `parse/pascal`, `parse/pascal/imports`, `resolve/pascal_declarations`, `formats` | overload 선택·generic 의미 등가·include 문맥·compiler directive |
| 간접 호출·후보 | dispatch-table/call-argument, `interface_dispatch.py` | `parse/callable_values`, `resolve/callable_values`, `dispatch` | 동적 callable·언어별 구현/override 근거 |
| 웹 입력·framework | 선정 component·Blade·Razor, decorator/fixture/route 규칙 | `formats/components`, `parse`, `resolve` | 정적 component·bare event 이외의 template 표현식·동적 component·framework 연결 |
| Blade·Livewire | `extractors/blade.py`의 extends/include/component/action | `components/blade`, `components/blade/livewire`, `resolve/inputs` | 동적 view·등록 namespace·component 등록/소유·전체 template 평가 |
| EJS | `extract.py::extract_ejs`와 공식 tag/include 계약 | `components/ejs`, `parse/javascript/ejs`, `resolve/ejs` | runtime root/views/includer·생성 JavaScript·실행 순서 |
| Astro | `extract.py::extract_astro`와 공식 script 문맥 | `components/astro`, `components/script_facts` | define:vars·bundling·page 실행 순서·전체 template 표현식 |
| 보조 입력 | `manifest_ingest.py`, 선정 project/form 추출기 | `formats/auxiliary`, `formats/xml` | 코드 소속·경로/프로젝트 참조의 해석 적용 |

원본 파일 경로는 `graphify/` 또는 `graphify/extractors/`에 속한다. Rust 경로의 `parse`는
`infrastructure/parse`, `resolve`는 `application/analysis/resolve`를 뜻한다.
완료는 원본의 선정 구문·관계·회귀 분기를 확인하고 차이에 근거를 남긴 뒤 판정한다.
비교 값은 이름·scope·정의/사용 위치·방향·관계·조건·내부/외부·해석 상태다.
증분 추가/삭제/rename/설정 변경은 전체 재빌드와 비교하고, 실패·취소는 마지막 정상 세대를 보존해야 한다.
같은 분석 범위에서 시간·peak RSS·정리 경로도 확인한다. 이 표나 테스트 개수만으로 포팅률을 주장하지 않는다.

Rust 이름 공간과 macro textual scope의 추가 근거는 공식 [Namespaces](https://doc.rust-lang.org/reference/names/namespaces.html)와
[Macros by example](https://doc.rust-lang.org/reference/macros-by-example.html#scoping-exporting-and-importing)이다.
Raw identifier의 원문과 조회 키 분리는 [Identifiers](https://doc.rust-lang.org/reference/identifiers.html)를 따른다. Unicode NFC 등가 정규화는 아직 제공하지 않는다.
Lisp의 compiler macro와 setf expander는 [DEFINE-COMPILER-MACRO](https://www.lispworks.com/documentation/HyperSpec/Body/m_define.htm)와 [DEFINE-SETF-EXPANDER](https://www.lispworks.com/documentation/HyperSpec/Body/m_defi_3.htm)에 따라 일반 함수 binding과 구분한다. 표준 definer를 명시적으로 인식하며 원본의 임의 `def*` 이름 추측은 적용하지 않는다.
Lisp 정적 package 옵션은 [DEFPACKAGE](https://www.lispworks.com/documentation/HyperSpec/Body/m_defpkg.htm)를 근거로 한다. 여러 `in-package` 구간·함수/값 공간·공개/내부 이름·nickname·정적 재수출을 구분한다. 런타임 package 조작과 custom reader는 실행하지 않는다.
PHP의 정적 이름 규칙은 [namespace fallback](https://www.php.net/manual/en/language.namespaces.fallback.php), trait 선택과 별칭은 [Traits](https://www.php.net/manual/en/language.oop5.traits.php)를 추가 근거로 한다. 대상 PHP 코드나 autoloader를 실행하지 않으며 동적 callable과 runtime late binding의 확정은 제공하지 않는다.
Python comprehension의 내부/바깥 iterable scope는 공식 [Expressions](https://docs.python.org/3/reference/expressions.html#displays-for-lists-sets-and-dictionaries), walrus의 바깥 binding은 [PEP 572](https://peps.python.org/pep-0572/#scope-of-the-target)를 따른다. `__all__`은 정적 list/tuple·같은 종류 결합·`+=`·마지막 정적 대입을 읽고 wildcard에만 적용한다. 동적/조건부 목록과 평가하지 못한 변경은 후보로 남기며 대상 Python 코드를 실행하지 않는다.
C/C++ 선언은 명시적 include 도달·같은 문맥·서명·반환 타입을 연결 근거로 삼는다. C++ [Program and linkage](https://eel.is/c++draft/basic.link)를 추가 근거로 사용한다. static/익명 namespace는 파일 밖과 합치지 않고, 여러 정의는 후보로 보존한다. 파일 전체를 감싸며 같은 macro를 먼저 정의하는 정형 `#ifndef`/`#if !defined(...)` guard는 정체성 비교에서만 구분하며 원래 조건은 저장한다. 내부 feature 조건은 비교에서 보존한다. translation unit 전처리·빌드 target·C++ module·alias 타입 등가의 전체 계산은 미지원 범위다.
Scala의 given/context bound는 공식 [Context Bounds](https://docs.scala-lang.org/scala3/reference/contextual/context-bounds.html)와 [Given Imports](https://docs.scala-lang.org/scala3/reference/contextual/given-imports.html)를 추가 근거로 한다. named/anonymous given의 구문 소유와 실제 implicit 검색은 구분한다. self-type 전제는 `depends_on`, 명시 상속과 mixin은 `inherits`/`mixes_in`으로 보존한다. 컴파일러 implicit 검색은 실행하지 않는다.
SwiftPM target·sources·exclude의 정적 소속은 공식 [PackageDescription](https://docs.swift.org/package-manager/PackageDescription/PackageDescription.html)을 따른다. Package.swift를 실행하거나 package를 내려받지 않는다. 조건부 dependency는 후보로 남기며, 전처리 directive가 있는 manifest와 동적 source/path는 소속 확정을 제공하지 않는다. 명시적 public/open의 target 경계는 적용하지만 전체 Swift 접근 제어와 testable import를 구현한 것으로 표시하지 않는다.
Go의 `use`·`replace` 범위와 우선순위는 [Modules Reference](https://go.dev/ref/mod)를 따른다. 인덱스에 포함된 로컬 manifest만 연결하며 `go` 명령·module 다운로드·MVS는 실행하지 않는다. 버전 조건이 있는 replacement는 선언된 require와 대응한 후보로 표시한다.
Ruby의 instance/singleton 구분과 `extend` 방향은 공식 [Object](https://docs.ruby-lang.org/en/3.4/Object.html) 문서를 추가 근거로 한다. 정적 class factory를 인식해도 런타임 lookup 안전성을 함께 확정하지 않는다.
Zig `@import`·tagged union·error set·같은 파일의 private 선언 의미는 공식 [0.15.2 문서](https://ziglang.org/documentation/0.15.2/)를 추가 근거로 한다. C preprocessor와 comptime은 실행하지 않는다.
Lisp 기본 reader의 ASCII 이름 키·이스케이프·quote/backquote 처리는 HyperSpec의
[reader case](https://www.lispworks.com/documentation/HyperSpec/Body/23_ab.htm),
[symbol tokens](https://www.lispworks.com/documentation/HyperSpec/Body/02_cd.htm),
[backquote](https://www.lispworks.com/documentation/HyperSpec/Body/02_df.htm)를 추가 근거로 삼는다.
