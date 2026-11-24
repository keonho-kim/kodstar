# 개발 환경과 빌드

[문서 지도](README.md) · [개발 원칙](../AGENTS.md) · [변경별 검증](development/verification.md) · [의존성과 생성 소스](development/dependencies.md)

## 개발 환경

| 준비할 입력 | 확인할 값과 경로 |
|---|---|
| Rust | [rust-toolchain.toml](../rust-toolchain.toml)의 1.99.1과 rustfmt·Clippy·LLVM 도구 |
| 네이티브 C/C++ 도구 | macOS는 Xcode Command Line Tools, Linux는 해당 환경의 C/C++ 빌드 도구 |
| LLVM/Clang 개발 입력 | 23.1.4의 헤더와 정적 라이브러리. 기본 탐색을 바꾸려면 `LLVM_CONFIG_PATH`로 해당 `llvm-config` 지정 |
| macOS용 표준 헤더 | 같은 LLVM의 libc++와 해당 target의 `__config_site`, 그리고 Zig 0.17.0 배포본의 Darwin C 헤더 |

macOS에서 기본 탐색에 사용하는 Homebrew 경로는 `/opt/homebrew/opt/llvm/bin/llvm-config`와
`/opt/homebrew/Cellar/zig/0.17.0/lib/zig`다. Zig 표준 헤더 위치는 `KODSTAR_ZIG_LIB_PATH`로 지정할 수 있다.
이 Darwin 헤더 조합은 macOS ARM64에서 실행 검증했다. 다른 target은 설치 환경에서 빌드와 실행을 각각 확인한다.
프로젝트의 native 빌드 입력과 출처는 [Clang 리소스 명세](../crates/kodstar-clang/native/SOURCE.json)에 있다.

```bash
rustup toolchain install 1.99.1 --profile minimal --component rustfmt --component clippy --component llvm-tools
rustup show active-toolchain
cargo --version
rustc --version
```

독립 설치한 Cargo가 먼저 선택되는 셸에서는 고정 toolchain의 실행 경로를 명시할 수 있다.

```bash
kodstar_toolchain_bin="$(dirname "$(rustup which --toolchain 1.99.1 rustc)")"
PATH="$kodstar_toolchain_bin:$PATH" cargo build --workspace --locked
```

## 빌드하고 사용해 보기

```bash
cargo build --workspace --locked
./target/debug/kodstar --help
./target/debug/kodstar init /work/project --yes --format json
./target/debug/kodstar symbols /work/project/src/api.rs --root /work/project
cargo build --release -p kodstar-cli --locked
cargo build --profile dist -p kodstar-cli --locked
```

lockfile의 Rust 의존성과 Tree-sitter·SQLite·Zstd·BLAKE3 등 번들 소스가 컴파일된다.
`kodstar-clang`의 build script가 LLVM/Clang archive와 헤더를 준비한다.
`dist`는 symbol strip, ThinLTO와 단일 codegen unit을 사용한다.
개발 프로파일의 바이너리는 `target/debug/kodstar`, 일반 최적화 빌드는 `target/release/kodstar`에 있다.

사용자 계정에 설치할 때는 `./install.sh`, 이미 준비한 바이너리를 설치할 때는
`./install.sh --binary /path/to/kodstar`를 사용한다. 설치·제거가 다루는 저장 상태는
[명령 참조](reference/cli.md#설치와-시작)에 정의한다.

### 설정 파일 예제

```toml
jobs = 2
memory_budget = 134217728
max_file_bytes = 2097152
exclude = ["generated/**"]

[language_overrides]
"headers/*.h" = "cpp"
```

```bash
./target/debug/kodstar init /work/project --config /work/kodstar.toml
```

병합 순서와 기본값은 [설정 계약](reference/cli.md#루트와-설정), 동시 작업의 예산은
[자원 수명](architecture/storage.md#자원-수명)에서 확인한다.

## 변경 검증

운영 코드의 공통 검사는 저장소 루트에서 실행한다. 변경에서 중요한 경계는
[변경별 테스트](development/verification.md#변경-책임에-따른-테스트)로 먼저 확인한다.

```bash
python3 -B scripts/checks/source.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked
```

문서의 검증 범위는 내용, 실제 코드·명령 옵션, 상대 경로·앵커, 예제의 의도와 `git diff --check`다.
검증용 예제는 독립한 프로젝트에서 실행한다. 테스트 구조·임시 자원 정리와 성능 비교 방법은
[검증 문서](development/verification.md), parser 재생성과 원본 기록은
[의존성 문서](development/dependencies.md)에 있다.

## GitHub 릴리스

[Release 워크플로](../.github/workflows/release.yml)는 macOS ARM64와 Ubuntu 24.04 x86-64 러너에서
각각 같은 커밋의 `dist` 바이너리를 빌드한다. [도구 준비 스크립트](../scripts/release/toolchain.sh)는
공식 LLVM 23.1.4와 macOS용 Zig 0.17.0 배포 파일의 SHA-256을 확인한다.
macOS 빌드의 배포 대상은 14.0이며, 공식 아카이브의 bitcode와 같은 버전의 `libLTO` 및 시스템 libc++를 사용한다.
[패키징 검사](../scripts/release/package.py)는 격리한 홈에서 설치하고 내장 Clang 호출이 해석되는지 확인한 뒤
바이너리·설치 스크립트·라이선스 고지를 아카이브에 담는다.

`main`에서 **Release**를 실행하고 `publish`를 켜면 두 빌드가 성공한 뒤 `Cargo.toml`의 workspace
버전으로 태그와 GitHub Release를 만들고 아카이브 두 개와 `SHA256SUMS`를 첨부한다.
`publish`의 기본 실행은 검증한 패키지를 워크플로 산출물로 올린다. 같은 이름의 태그가 이미 있으면 게시를 중단한다.

```bash
gh workflow run release.yml --ref main -f publish=true
```
