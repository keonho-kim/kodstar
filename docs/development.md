# kod<sup>☆</sup> 개발과 검증

이 문서는 환경 준비, 빌드·검증과 의존성 관리를 다룬다.
제품 계약은 [기능 명세](../kodstar-spec.md), 코드 구조는 [아키텍처](architecture.md),
변경 절차와 작성 원칙은 [AGENTS.md](../AGENTS.md)를 따른다.

## 개발 환경

고정 toolchain과 component는 [rust-toolchain.toml](../rust-toolchain.toml),
edition과 최소 Rust 버전은 [Cargo.toml](../Cargo.toml)에 정의한다.
[rustup](https://rust-lang.github.io/rustup/installation/index.html)과 네이티브 C/C++ 빌드 도구와 LLVM/Clang **23.1.4**의 개발 header·정적 archive가 필요하다.
실행 지원 대상은 macOS와 Linux이며, Windows는 지원하지 않는다.
macOS는 Xcode Command Line Tools, Linux는 C/C++ toolchain을 준비한다.
`LLVM_CONFIG_PATH`로 해당 버전의 `llvm-config`를 지정할 수 있다. macOS Homebrew의
`/opt/homebrew/opt/llvm/bin/llvm-config`도 탐색한다. 실행 바이너리에 링크한 frontend와
builtin header가 들어가므로 설치된 CLI는 LLVM 설치나 실행 시 추출을 요구하지 않는다.
macOS 빌드는 Zig **0.17.0** 배포본의 `lib/zig`도 필요하다. `KODSTAR_ZIG_LIB_PATH`로 지정하거나
Homebrew의 `/opt/homebrew/Cellar/zig/0.17.0/lib/zig`를 사용한다. Zig 실행 파일은 링크하거나
배포하지 않으며 Darwin C header 원문만 읽는다. 같은 LLVM 설치의 `include/c++/v1`과
대상별 `__config_site`를 압축해 포함한다. 이 표준 헤더 조합의 실행 검증 대상은 macOS ARM64다.

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

일반 빌드는 lockfile의 Rust crate와 Tree-sitter·SQLite·Zstd·BLAKE3 소스를 컴파일하고,
`kodstar-clang`이 동일 버전의 Clang/LLVM 정적 archive에서 도달 가능한 frontend 코드를 연결한다.
builtin·libc++·Darwin C header는 빌드에서 학습한 공용 dictionary와 파일별 Zstd frame으로 생성하며 내용 ID·target과 native bridge·정적 archive의
내용 ID를 분석 fingerprint에 포함한다.
같은 Cargo `OUT_DIR`에서 Clang 빌드 스크립트를 다시 실행하면 정적 archive의 경로·파일 식별자·
크기·수정 및 변경 시각을 대조한다. 메타데이터가 같은 archive에는 저장한 내용 ID를 사용하고,
새 출력 디렉터리나 변경된 archive는 내용을 읽어 해시한다.
BLAKE3는 지원 CPU의 네이티브 최적화를 사용하고 Zstd는 멀티스레드 압축 기능 없이 번들 빌드한다.
분석 대상 프로젝트의 코드·빌드 스크립트·의존성은 실행하거나 설치하지 않는다.

진행 이벤트는 CLI의 stderr에 출력하며 비대화형 리다이렉션에서도 수집·파싱·native 파일·해석·공개
단계를 확인할 수 있다. stdio session은 구조화된 progress frame으로 전달한다. 자원 예산·worker의 소유권과
작업 한도는 [아키텍처](architecture.md#웹-요청과-native-의미-분석), native 한도의 값과 제한 진단은
[기능 명세](../kodstar-spec.md#134-내장-cc-의미-근거)에서 관리한다.

## 빌드와 실행

사용자 설치는 저장소 루트의 `./install.sh`로 실행한다. ThinLTO·단일 codegen unit·심볼 제거를 적용한 `dist` profile로 CLI만
전용 임시 target에 빌드하고 종료 시 정리한다. 같은 명령으로 기존 설치를 교체한다.
이미 빌드한 파일은 `./install.sh --binary /path/to/kodstar`로 설치한다. 상대 경로는 호출 위치 기준이다.
설치 시 `~/.kodstar`에는 설치 기록과 잠금만 준비하며 프로젝트 DB는 `init`에서 생성한다.
Rust toolchain과 Cargo 공유 다운로드 캐시는 개발 환경이 관리한다.
설치 receipt는 `$HOME/.kodstar/install.json`, 영구 프로젝트 등록부는 같은 폴더의
`projects.sqlite`다. `uninstall`은 이 등록부의 이력까지 제거하므로 임시 HOME·프로젝트로 검증한다.
PATH는 새 shell에서 적용한다. 현재 shell은 설치 출력의 명령을 적용한다.
대화형 경로 확인은 `python3 -B scripts/checks/confirmation.py /absolute/path/to/kodstar`로
임시 프로젝트의 실제 PTY에서 거절·동의·정리를 검증한다. 설치 통합 검사는
`cargo test -p kodstar-cli --test installation --test installation_data --locked`로 실행한다.
스크립트 자체의 임시 빌드 정리는 `python3 -B scripts/checks/installation.py /absolute/path/to/kodstar`로
확인한다. 실제 CLI 바이너리와 통제된 빌드 결과를 사용해 성공·빌드/설치 실패·중단·상대 경로를 검증한다.

초기화는 `init`으로 이력 저장소와 분석을 함께 준비한다. `cargo build`는 개발용 실행 파일 빌드다.

```bash
cargo build --workspace --locked
./target/debug/kodstar --help
./target/debug/kodstar init /work/project --yes --format json
./target/debug/kodstar symbols /work/project/src/api.rs --root /work/project
cargo build --workspace --release --locked
cargo build --workspace --profile dist --locked
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
"headers/*.h" = "cpp"
```

파일을 지정해 적용한다.

```bash
./target/debug/kodstar init /work/project --config /work/kodstar.toml
```

저장·추출 버전은 [index/mod.rs](../crates/kodstar-core/src/infrastructure/index/mod.rs),
관계 registry는 [relations.rs](../crates/kodstar-core/src/domain/relations.rs)에서 관리한다.
호환성 오류와 캐시 재생성 절차는 [저장 일관성](../kodstar-spec.md#16-저장-일관성과-장애-시-동작)을 따른다.

## 변경 검증

변경한 책임의 테스트를 먼저 실행한다. Rust 변경의 공통 검사는 저장소 루트에서 수행한다.

```bash
python3 -B scripts/checks/source.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked
```

그래프 조회는 `cargo test -p kodstar-cli --locked --test graph_navigation`으로
확인한다. Java 메서드 호출과 반환 타입 해석은
`cargo test -p kodstar-core --locked --test engine return_values`에서 확인한다.

### 개발 품질 도구

개발 도구는 Cargo 하위 명령으로 직접 사용하며 배포 CLI에 포함하지 않는다.
`$CARGO_HOME/bin` 또는 기본 `$HOME/.cargo/bin`을 PATH에 추가한다.

| 도구 | 용도 |
|---|---|
| [cargo-nextest](https://github.com/nextest-rs/nextest) | 격리한 테스트 실행과 병렬 수 제한 |
| [cargo-machete](https://github.com/bnjbvr/cargo-machete) | 미사용 의존성 탐지 |
| [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) | 보안 권고·라이선스·의존성 출처 검사 |
| [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) | Rust 테스트 커버리지 측정 |
| [cargo-mutants](https://github.com/sourcefrog/cargo-mutants) | 변이 검사로 테스트의 검출 능력 확인 |

다음 명령은 설치한 도구를 유지하고 실행별 빌드·보안 DB·임시 출력을 종료 시 회수한다.
설치 명령은 지정 버전이 설치되어 있으면 건너뛴다. 선택 검사도 같은 subshell 안에서 실행한다.

```bash
(
    set -eu
    kodstar_check_dir="$(mktemp -d "${TMPDIR:-/tmp}/kodstar-checks.XXXXXXXX")"
    trap 'rm -rf -- "$kodstar_check_dir"' EXIT
    export CARGO_TARGET_DIR="$kodstar_check_dir/target"
    export CARGO_BUILD_BUILD_DIR="$kodstar_check_dir/build"
    export CARGO_LLVM_COV_TARGET_DIR="$kodstar_check_dir/coverage"
    export KODSTAR_ADVISORY_DB="$kodstar_check_dir/advisories"
    export TMPDIR="$kodstar_check_dir/tmp"
    mkdir -p "$TMPDIR"

    cargo install cargo-nextest --version 0.9.151 --locked --target-dir "$kodstar_check_dir/install"
    cargo install cargo-machete --version 0.9.2 --locked --target-dir "$kodstar_check_dir/install"
    cargo install cargo-deny --version 0.20.2 --locked --target-dir "$kodstar_check_dir/install"
    cargo install cargo-llvm-cov --version 0.9.1 --locked --target-dir "$kodstar_check_dir/install"
    cargo install cargo-mutants --version 27.1.0 --locked --target-dir "$kodstar_check_dir/install"

    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo machete
    cargo deny --config .config/deny.toml --locked check
    cargo nextest run --workspace --locked
    cargo test --workspace --doc --locked

    # 선택 검사: 필요한 명령의 주석을 해제한다.
    # cargo llvm-cov nextest --workspace --locked --summary-only
    # cargo mutants --output "$kodstar_check_dir/mutants"
    # RUSTFLAGS='--cfg=kodstar_loom' CARGO_TARGET_DIR="$kodstar_check_dir/loom-target" \
    #     CARGO_BUILD_BUILD_DIR="$kodstar_check_dir/loom-build" \
    #     cargo test -p kodstar-core --lib --locked application::analysis::budget::model:: -- --test-threads=1
)
```

공유 Cargo 다운로드 캐시는 유지한다. 명시적으로 보존할 LCOV 결과는 `--lcov --output-path`
옵션으로 임시 경로 밖의 새 파일에 출력한다. 중단 시 하위 프로세스가 끝났는지 확인한 뒤
남은 임시 경로를 제거한다.

`cargo-machete`의 결과는 실제 import와 생성 코드 사용을 확인해 판단한다. `cargo-deny` 정책은
[deny.toml](../.config/deny.toml)에서 관리하며 보안 데이터베이스도 실행 소유 임시 경로에서 읽는다.
라이선스 허용 목록은 외부 의존성에 적용한다. 아직 프로젝트 라이선스를 지정하지 않은
`publish = false`인 workspace crate에는 Cargo의 private-package 예외를 적용하며 새 라이선스를 부여하지 않는다.
공개 API와 다른 플랫폼의 조건부 코드는 컴파일러의 `dead_code` 결과만으로 삭제하지 않는다.

Nextest로 unit·통합 테스트를 실행하고 doctest는 `cargo test --doc`으로 별도 확인한다.
[nextest.toml](../.config/nextest.toml)은 전체 실행 수와 native 그룹을 제한하고 실패한 테스트를
자동 재시도하지 않는다. 프로세스별 격리 비용도 있으므로 실행 속도는 실제 측정으로 판단한다.
`cargo llvm-cov`는 Rust 테스트의 도달 범위를 보고하며 미실행 코드가 곧 데드코드라는 뜻은 아니다.
내장 Clang 정적 archive의 C++ 구현까지 Rust 커버리지에 포함되지는 않는다.

`cargo mutants`는 [고정 범위](../.cargo/mutants.toml)의 정수 codec을 바꿔 테스트가 오류를 검출하는지
확인한다. 일반 검사보다 비싸므로 해당 계약을 바꿀 때 선택해 실행한다.
변이가 반복문을 비종료 상태로 만들면 시간 초과로 별도 진단하며 검출 성공으로 바꾸지 않는다.
`kodstar_loom` 설정은 실제 바이트 예산의 Mutex·Condvar를 테스트에 한해 계측한다. 모델마다 최대 10,000개
실행 순서와 두 번의 선점을 탐색하며 permit 총량·반납·대기 해제를 확인한다.
Loom의 조건 변수는 시간 경과를 모델링하지 않으므로 취소 polling·신호·native FFI는 기존 테스트가 검증한다.

rstest는 동일한 인자 계약의 사례를 개별 선택 가능한 테스트로 만들고, insta는 기존 도움말 검사의
출력을 스냅샷으로 확인한다. 승인된 `.snap` 파일은 테스트 fixture로 추적한다.
[insta.yaml](../.config/insta.yaml)은 자동 갱신을 끄므로 일반 검사 실패 시 `.snap.new`를 만들지 않는다.
의도한 계약 변경은 해당 스냅샷만 명시적으로 갱신하고 diff를 검토한다.

엔진 통합 테스트를 따로 실행하거나 이름으로 좁힐 수 있다.
`tests/engine.rs`는 사례 모듈을 연결하고, 기본 조회·갱신·실패·경로 계약은
`tests/cases/engine_contracts.rs`가 소유한다.
초기 색인 차단은 `consistency`, 무변경·검증 갱신의 재사용은 `refresh_reuse`,
제공 파일의 추가·변경·삭제와 모듈 lookup 무효화는 `module_incremental`이 확인한다.
`hybrid_recovery`는 부분 구문 복구·소유 범위·후보 관계와 Clang 보강의 독립성을,
`configuration_failures`는 잘못된 설정·입력 한도·참조 제거와 설정 복구를 확인한다.
같은 입력·상태 전이·출력 계약의 검사는 해당 소유 모듈에 모은다. 언어·구문·실패 시점이
다른 회귀는 구분하며, 저장 형식의 손상 거절·업그레이드 검사는 현재 지원 계약에 맞춰 유지한다.

```bash
cargo test -p kodstar-core --test engine --locked
cargo test -p kodstar-core --test engine --locked ejs
cargo test -p kodstar-core --test engine --locked http
cargo test -p kodstar-core --test engine --locked htmx
cargo test -p kodstar-core --test engine --locked clang
cargo test -p kodstar-clang --locked
```

빌드와 검증은 개발 환경에서 수행한다. 문서 변경은 내용·참조·앵커·diff를 확인한다.

`proptest`는 고정 seed와 제한된 사례 수로 정수·문자열·lookup codec의 전체 값 범위,
정규 표현, 손상·overflow·후행 바이트 거절을 검사한다. 테스트가 별도 회귀 파일을 만들지 않으며,
실패 seed는 출력으로 확인하고 필요한 최소 사례를 소스 테스트에 추가한다.

`criterion`은 개발 의존성으로 실제 엔진의 저장된 그래프 조회·분리된 텍스트 diff·무변경
동기화를 측정한다. 배포 코드에 benchmark 실행기를 포함하지 않는다.

```bash
CARGO_TARGET_DIR=/path/to/owned-target CARGO_INCREMENTAL=0 cargo bench -p kodstar-core --bench engine --locked -- --noplot
```

같은 임시 target에서 변경 전 `--save-baseline before`, 변경 후 `--baseline before`를
Criterion 인자에 추가해 비교한다. 작은 조회와 넓은 그래프를 함께 확인하며 결과·정렬·
잘림·저장 내용이 같은 경우에만 시간·RSS를 비교한다. 최초 빌드와 다른 컴파일 작업은
측정 구간에서 제외한다. 실행이 끝나면 기준선·프로파일·복사본을 포함한 임시 target을 회수한다.
해석 진행 로그의 `expression_rows`, `expression_statements`, `expression_steps`는
최종 expression 의존성의 배치 기록 비용이며 scope 의존성의 즉시 기록과 구분한다.

Criterion 결과는 해당 target 아래에 저장된다. 검증용 target·임시 프로젝트는 작업 종료 시
제거하고, 비교 근거가 필요하면 작은 측정 보고서만 보존한다. 성능 측정 중 다른 빌드·테스트를
동시에 실행하지 않는다.

일반 진행·실행 단계의 시간·결과 개수는 기본적으로 stderr에 표시한다.
전역 옵션 `--verbose false`로 끄고 `--verbose true`로 명시적으로 켠다. 옵션은 명령 앞이나
하위 명령 뒤에 지정하며 boolean 값은 필수다. 오류·분석 경고와 stdio progress frame은 유지한다.
subscriber는 CLI 실행 수명을 따르고 상세 출력이 켜진 경우 stderr에만 출력한다.
이 계측은 소스 원문·검색어·프로젝트 경로를 기록하거나 로그 파일을 만들지 않는다.
CLI 도움말 변경은 `args::tests`에서 공개 명령·인자 설명과 예시의 인자 파싱을 확인하고,
`startup` 통합 검사에서 전체 공개 명령의 `-h`·`--help` 출력과 홈·프로젝트 무변경을 확인한다.
stdio 변경은 `python3 -B scripts/checks/session.py /absolute/path/to/kodstar`로 읽히지 않는
stdout에서 shutdown·요청 취소 후 프로젝트 잠금 해제·SIGINT와 프레임 임시 파일 회수를 확인한다.
테스트·측정은 격리한 `TMPDIR`·HOME·프로젝트를 사용하고 자식 종료를 확인한 뒤 회수한다.
검증용 Cargo 출력도 작업 소유 `CARGO_TARGET_DIR`에 두며 더 이상 재사용하지 않으면 제거한다.
실패·취소에도 소스 복사본·DB·WAL·프레임 spool·Python bytecode를 남기지 않는다.
소형 회귀 fixture는 실제 소비 테스트와 함께 관리한다.

### 로컬 이력 변경 검증

변경 책임에 따라 다음 계약을 확인한다.
동작 계약은 [기능 명세](../kodstar-spec.md#7-cli-명령어-전체), 배치와 상태 전이는
[실행 구조](architecture.md#실행과-상태-소유권)를 따른다.

| 변경 책임 | 확인할 계약 |
|---|---|
| 초기화 | 최초 `init`의 저장소·체크포인트·분석 연결, 반복 초기화의 이력 보존, 알 수 없는 기존 데이터 보호 |
| 설치·제거 | 임시 HOME 설치·재설치·PATH 중복 방지·경로 확인/취소·루트 차단·활성 세션 차단·캐시 정리 후 이력 제거·사용자 파일 보존 |
| 공개 계약 | CLI·Rust API·stdio의 초기화·이력·결과 계약 일치 |
| 자동 저장 | 추가·수정·삭제·rename 반영, 변경 없는 반복 명령·중복 watch 이벤트에서 체크포인트 증가 없음 |
| 입력 일치 | 읽는 중 편집·불완전 스캔·취소에서 부분 체크포인트 미공개, 저장된 바이트와 분석 입력 일치 |
| 상태·실패 | 체크포인트 저장 후 분석 실패 시 이력 보존, 마지막 정상 세대 구분, 재시도 시 이력 중복 없음 |
| 캐시 분리 | cache clean/GC 후 이력·이름·브랜치 유지, 다음 분석에서 재생성, no-refresh는 쓰기·재생성 없음 |
| 이력·diff | 분석 실패·캐시 부재에서도 조회, 이름만 부여할 때 내용 중복 없음, 기본/명시 비교 기준의 실제 ID 표시 |
| 복원 | 전체·선택 파일 복원, 사전 보존 실패 시 작업 파일 불변, 미추적 파일 충돌, 복원 결과의 새 체크포인트 |
| 브랜치 | 생성과 전환 구분, 전환 전 변경 보존, 현재 브랜치 삭제 거부, 참조 삭제 후 이력 보존 |
| 중단 복구 | 파일 쓰기 단계별 장애·취소, 부분 효과 식별, 정상 참조·이력 보존, 임시 자원과 잠금 정리 |
| 과거 분석 | checkout 없는 `--rev`, 보존 manifest로 package 경계 판정, 체크포인트·분석 환경의 캐시 격리, 증분/전체 결과 일치 |
| 변경 영향 | `--since`의 변경 전후 정의·관계 사용, 삭제된 정의의 기존 사용처 포함, coverage·환경 차이 표시 |
| 출력·연동 | 체크포인트와 분석 세대의 연결, schema/protocol 버전, 출력 완료·잘림, CLI/watch/stdio 정책 일치 |

임시 프로젝트와 전용 이력·캐시 디렉터리를 사용하고 실제 사용자 파일을 복원 대상으로 삼지 않는다.
회귀 사례는 `history_operations`, `history_paths`, `history_recovery`, `history_watch`, `history_impact`,
`local_history`에 있으며 CLI·stdio 검사도 함께 실행한다. 소스 복원은 임시 프로젝트만 사용한다.

## 성능 검증

같은 toolchain·profile·target으로 빌드한 비교 실행 파일과 고정 입력을 준비한다.
macOS에서는 [analysis.py](../scripts/benchmarks/analysis.py)로 시간·peak RSS와 결과를 비교한다.
측정 중에는 다른 빌드나 입력 편집을 진행하지 않는다.

```bash
python3 -B scripts/benchmarks/analysis.py --baseline /path/to/baseline-kodstar --candidate target/dist/kodstar \
  --project /path/to/frozen-project --file src/api.rs --repeats 5 --output /path/to/benchmark.json
```

각 실행은 같은 canonical 경로에 새 임시 복제본을 만들고 이력·HOME·분석 캐시를 분리한다.
원본과 원본의 `.kodstar`는 수정하지 않는다. package·객체 식별에 루트 경로가 포함되므로 비교 대상의
경로를 고정하며, 복사·측정 준비 시간은 명령 시간에서 제외한다. 최초 분석은 앱 저장소가 빈 상태이며
OS 디스크 캐시가 비어 있다는 의미는 아니다. baseline/candidate 순서는 반복마다 바꾼다.
각 바이너리의 `--version` 첫 실행 시간을 별도로 기록한 뒤 프로젝트를 측정한다. macOS의 최초
실행 검증 비용이 한쪽의 최초 분석 시간에만 섞이지 않도록 하며 이력·분석 캐시는 여전히 빈 상태다.

초기 init·변경 없는 동기화 조회·no-refresh 조회를 측정한다. coverage와 입력·객체·scope·binding·
관계·정체성·조회 의존·native 분석/입력 관측, 보존 파일 구성·실제 바이트·체크포인트 계보의 해시가 다르면 비교를 거절한다.
이진/hex 해시 표현을 정규화하고 미지원·크기 제한 파일의 분석용 해시는 비운다. 해당 파일의 실제
보존 바이트는 별도 source 해시로 전부 비교한다. init의 시각·cache 경로 같은 변동 값은 성능 지표로만 기록한다.
보고서는 실행 파일 SHA-256, 환경, 개별 시간·peak RSS와 중앙값, 이력·캐시의 파일 크기·할당 블록,
SQLite page·freelist·table 크기와 보존 payload를 포함한다. `dbstat` 미지원 환경은 table 크기를
빈 값으로 표시한다. 할당 블록은 파일시스템 보고값이며 공유 extent의 독점 사용량을 뜻하지 않는다.

그래프 전용 비교는 같은 저장소에서 출력 행의 순서·방문 수·잘림을 포함해 비교한다.
저장 형식이 다르면 `--independent-indexes`로 같은 원본 경로에 각 바이너리의 인덱스를 따로 만들고
정규화한 의미 결과와 조회 전후의 불변성을 대조한다.
생성한 Go 입력은 실행하지 않으며 내부 임시 디렉터리에서만 초기화·조회한다.
연결이 많은 최단 경로, 전체 관계 출력, 방문 제한 조회와 넓은 정방향·역방향 대기열을 별도로 측정한다.

```bash
python3 -B scripts/benchmarks/graph.py --baseline /path/to/baseline-kodstar --candidate target/dist/kodstar \
  --repeats 5 --output /path/to/graph-benchmark.json
```

이력 비교의 Python reference decoder는 설치된 `libzstd`를 사용한다. 자동 검색이 안 되면
`KODSTAR_ZSTD_LIBRARY`에 공유 라이브러리 절대 경로를 지정한다. 분석 대상의 의존성을 설치하지 않는다.

측정용 자식이 종료하고 미반영 WAL이 없을 때만 임시 저장소를 immutable로 읽는다.
실제 사용자 캐시·이력을 이 방식으로 열지 않는다. 분석 범위·스킵·미해결 수를 함께 확인하고,
변경 확인 포함 시간과 순수 조회를 구분한다. 분석 규칙 변경은 증분·전체 결과 일치도 검증한다.

`--newline-edit`는 지정한 파일의 임시 복제본에 개행을 추가하고 증분·전체 분석의 전체 분석·이력 해시와 coverage를 비교한다. 복제본 밖으로 이어지는 경로는 거절한다.
`--mutations`는 임시 입력에 Go 검증 파일을 추가하고 문서·본문·정의 추가·삭제를 비교한다.
각 단계도 전체 분석·소스 이력 해시와 coverage가 모두 같아야 한다.
`--body-replacement OLD NEW`는 지정 파일의 첫 일치 구간을 바꾸고, `--declaration-edit`는
지원하는 언어의 함수 하나를 추가한다. 두 방식 모두 임시 복제본에서 증분·전체 분석을
실행하며 전체 해시와 coverage를 대조한다. 일치하는 본문 구간이 없으면 측정을 거절한다.

lookup key의 계약을 의도적으로 변경한 바이너리 비교는 변경 시나리오와 `--lookup-contract-change`를
명시한다. 바이너리 사이에서는 lookup을 제외한 나머지 해시와 coverage를 비교하며, 각 바이너리의
변경 후 증분·전체 분석은 lookup과 native 관측까지 포함한 전체 해시가 같아야 한다. 보고서에는 비교 옵션과
비교에서 제외한 원래 lookup 해시도 남긴다. 일반 성능 변경의 기본 비교는 모든 해시를 사용한다.

native bridge가 달라진 바이너리는 `--native-engine-change`를 명시한다. 환경 fingerprint는
보고서에 각각 기록하고 바이너리 사이에서는 의미 결과·입력 관측·진단을 비교한다. 같은 바이너리의
증분·전체 비교에서는 환경 fingerprint도 반드시 일치해야 한다.

## 의존성과 grammar

널리 사용되고 유지보수되는 라이브러리를 선정하며 Rust 구현과 검증된 native 구현의 Rust 바인딩을 허용한다.
직접·전이 의존성의 버전·라이선스·보안 공지·자원 비용을 검토하고 lockfile로 고정한다.
공식 Tree-sitter와 canonical grammar 원본을 사용한다.

분석 중 짧은 이름과 배열은 `smol_str`·`smallvec`, native 파일의 반복 문자열은
`string-interner`, 제한된 그래프 방문 집합은 `roaring`을 사용한다. 탐색 대기열은
`petgraph::Bfs`를 사용하며 실제 I/O·오류·취소는 이웃 목록을 만드는 경계에서 처리한다.
petgraph는 필요한 `std` 기능만 활성화하고 병렬 분석·직렬화·추가 그래프 표현은 활성화하지 않는다.
이력의 제한된 변경 창은 `similar`의 행 정렬을 사용한다. `tracing`·`tracing-subscriber`는
동기 실행 단계의 CLI 상세 출력을 담당하며 비동기 runtime·로그 파일을 추가하지 않는다.
큰 UTF-8 입력의 검증은 `simdutf8`로 수행하며 실패 위치가 필요한 경계는 `compat` API를 사용한다.
Python·Ruby의 명시적 인코딩은 `encoding_rs`의 대체 없는 decoder를 사용한다. Latin-1은
언어의 원래 byte-to-codepoint 규칙으로 처리하며 지원 범위와 원문 위치 계약은 기능 명세에 둔다.
XML 보조 입력은 `roxmltree`의 읽기 전용 문서와 원문 위치를 사용한다.
XML의 속성 개수·ASCII 값 검증 수정을 포함한 공식 upstream revision을 manifest와
lockfile에 고정하며 별도의 로컬 parser 패치를 유지하지 않는다.

| 관리 대상 | 기준 파일 |
|---|---|
| Rust dependency 해석 결과 | [Cargo.lock](../Cargo.lock) |
| 저장소·검토 시점·grammar checksum·revision·ABI | [dependencies.json](../LICENSES/dependencies.json) |
| 라이선스 원문·NOTICE와 배포 고지 | [LICENSES](../LICENSES), [third-party-notices.txt](../LICENSES/third-party-notices.txt) |
| 개발 전용 dependency의 라이선스 원문 | [development-notices.txt](../LICENSES/development-notices.txt) |
| 보완한 grammar의 원본·패치·생성 파일 해시 | `vendor/tree-sitter-*/SOURCE.json`과 `patch.json` |

의존성 변경은 manifest·lockfile·라이선스/NOTICE·native 번들·grammar ABI를 함께 확인한다.
LLVM 출처·버전·라이선스는 `crates/kodstar-clang/native/SOURCE.json`과 dependency inventory의
`native_components`에 기록한다. `kodstar --licenses`는 정적 링크한 라이브러리와 builtin header의
고지도 함께 출력한다. Windows와 외부 SDK 전체 포함을 주장하지 않으며 Linux 실행은 실제 환경에서 검증한다.
SQLite 내부 JSON 처리는 번들 SQLite의 JSONB를 사용하며 로드 가능한 외부 확장은 요구하지 않는다.
분석 캐시의 호환성과 재생성은 [저장 일관성](../kodstar-spec.md#16-저장-일관성과-장애-시-동작),
영구 이력의 형식 전환은 [이력 정책](../kodstar-spec.md#153-영구-이력-저장소)을 따른다.
저장소에 보존한 grammar는 canonical upstream revision·입력 SHA-256·ABI와 재현 가능한 패치를 함께 고정한다.
변경 없는 registry grammar는 Cargo.lock의 배포 checksum을 기준으로 한다.

```bash
cargo metadata --locked --format-version 1
python3 -B scripts/grammars/generate.py --check
```

C/C++·C#·JavaScript/TypeScript·Kotlin·Swift·Common Lisp 보완 규칙은 고정 원본에서
재생성한다. 일반 빌드는 저장된 parser를 사용한다.
원본 규칙을 변경할 때만 아래 도구를 설치하고 재생성한다. 생성 C 파일은 직접 수정하지 않는다.

```bash
cargo install tree-sitter-cli --version 0.26.10 --locked --root ./tmp/tree-sitter-cli
python3 -B scripts/grammars/generate.py --grammar tree-sitter-kotlin-ng --tree-sitter ./tmp/tree-sitter-cli/bin/tree-sitter
```

C++ 생성은 C grammar를 입력으로 사용한다. C를 바꾸면 먼저 재생성하고 C++의 `SOURCE.json`에
입력 해시를 맞춘 뒤 C++를 생성한다. 생성 후 `--check`와 parser 회귀·실제 corpus 검사를 실행한다.
임시 generator·다운로드·컴파일 출력은 작업 소유 디렉터리에 두고 검사 종료 후 제거한다.
