# kod<sup>☆</sup> 개발과 검증

이 문서는 환경 준비, 빌드·검증과 의존성 관리를 다룬다.
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

일반 빌드는 lockfile의 Rust crate와 Tree-sitter·SQLite·Zstd·BLAKE3 네이티브 소스를 컴파일한다.
BLAKE3는 지원 CPU의 네이티브 최적화를 사용하고 Zstd는 멀티스레드 압축 기능 없이 번들 빌드한다.
분석 대상 프로젝트의 코드·빌드 스크립트·의존성은 실행하거나 설치하지 않는다.

## 빌드와 실행

사용자 설치는 저장소 루트의 `./install.sh`로 실행한다. 심볼을 제거한 `dist` profile로 CLI만
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
"legacy/*.inc" = "pascal"
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

엔진 통합 테스트를 따로 실행하거나 이름으로 좁힐 수 있다.

```bash
cargo test -p kodstar-core --test engine --locked
cargo test -p kodstar-core --test engine --locked ejs
```

빌드와 검증은 개발 환경에서 수행한다. 문서 변경은 내용·참조·앵커·diff를 확인한다.
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

초기 init·변경 없는 동기화 조회·no-refresh 조회를 측정한다. coverage와 입력·객체·scope·binding·
관계·정체성·조회 의존, 보존 파일 구성·실제 바이트·체크포인트 계보의 10개 해시가 다르면 비교를 거절한다.
이진/hex 해시 표현을 정규화하고 미지원·크기 제한 파일의 분석용 해시는 비운다. 해당 파일의 실제
보존 바이트는 별도 source 해시로 전부 비교한다. init의 시각·cache 경로 같은 변동 값은 성능 지표로만 기록한다.
보고서는 실행 파일 SHA-256, 환경, 개별 시간·peak RSS와 중앙값, 이력·캐시의 파일 크기·할당 블록,
SQLite page·freelist·table 크기와 보존 payload를 포함한다. `dbstat` 미지원 환경은 table 크기를
빈 값으로 표시한다. 할당 블록은 파일시스템 보고값이며 공유 extent의 독점 사용량을 뜻하지 않는다.

이력 비교의 Python reference decoder는 설치된 `libzstd`를 사용한다. 자동 검색이 안 되면
`KODSTAR_ZSTD_LIBRARY`에 공유 라이브러리 절대 경로를 지정한다. 분석 대상의 의존성을 설치하지 않는다.

측정용 자식이 종료하고 미반영 WAL이 없을 때만 임시 저장소를 immutable로 읽는다.
실제 사용자 캐시·이력을 이 방식으로 열지 않는다. 분석 범위·스킵·미해결 수를 함께 확인하고,
변경 확인 포함 시간과 순수 조회를 구분한다. 분석 규칙 변경은 증분·전체 결과 일치도 검증한다.

`--mutations`는 임시 입력에 Go 검증 파일을 추가하고 문서·본문·정의 추가·삭제를 비교한다.
각 단계도 일곱 분석 해시와 세 소스 이력 해시·coverage가 모두 같아야 한다.

## 의존성과 grammar

널리 사용되고 유지보수되는 라이브러리를 선정하며 Rust 구현과 검증된 native 구현의 Rust 바인딩을 허용한다.
직접·전이 의존성의 버전·라이선스·보안 공지·자원 비용을 검토하고 lockfile로 고정한다.
공식 Tree-sitter와 canonical grammar 원본을 사용한다.

| 관리 대상 | 기준 파일 |
|---|---|
| Rust dependency 해석 결과 | [Cargo.lock](../Cargo.lock) |
| 저장소·검토 시점·grammar checksum·revision·ABI | [dependencies.json](../LICENSES/dependencies.json) |
| 라이선스 원문·NOTICE와 배포 고지 | [LICENSES](../LICENSES), [third-party-notices.txt](../LICENSES/third-party-notices.txt) |
| Kotlin 원본과 생성 파일 해시 | [SOURCE.json](../vendor/tree-sitter-kotlin-ng/SOURCE.json) |
| Kotlin grammar 수정 규칙 | [patch.json](../vendor/tree-sitter-kotlin-ng/patch.json) |

의존성 변경은 manifest·lockfile·라이선스/NOTICE·native 번들·grammar ABI를 함께 확인한다.
SQLite 내부 JSON 처리는 번들 SQLite의 JSONB를 사용하며 로드 가능한 외부 확장은 요구하지 않는다.
분석 인덱스 형식 15는 기존 캐시를 명시적으로 정리한 뒤 재생성한다. 영구 이력 형식 1~5는
`history upgrade`로 형식 6에 전환하며 소스 이력의 자동 삭제·재생성은 하지 않는다.
Swift grammar의 배포 원본처럼 revision만으로 배포물을 재구성할 수 없는 경우에는 crate checksum을 기준으로 한다.

```bash
cargo metadata --locked --format-version 1
python3 -B scripts/grammars/kotlin.py --check
```

Kotlin grammar의 구분자 규칙은 고정 원본에서 재생성한다. 일반 빌드는 저장된 parser를 사용한다.
원본 규칙을 변경할 때만 아래 도구를 설치하고 재생성한다. 생성 C 파일은 직접 수정하지 않는다.

```bash
cargo install tree-sitter-cli --version 0.26.10 --locked --root ./tmp/tree-sitter-cli
python3 -B scripts/grammars/kotlin.py --tree-sitter ./tmp/tree-sitter-cli/bin/tree-sitter
```
