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

사용자 설치는 저장소 루트의 `./install.sh`로 실행한다. 심볼을 제거한 `dist` profile을 사용한다. 같은 명령으로 기존 설치를 교체한다.
이미 빌드한 파일은 `./install.sh --binary /absolute/path/kodstar`로 설치할 수 있다.
설치 receipt는 `$HOME/.kodstar/install.json`, 영구 프로젝트 등록부는 같은 폴더의
`projects.sqlite`다. `uninstall`은 이 등록부의 이력까지 제거하므로 임시 HOME·프로젝트로 검증한다.
PATH는 새 shell에서 적용한다. 현재 shell은 설치 출력의 명령을 적용한다.
대화형 경로 확인은 `python3 scripts/checks/confirmation.py /absolute/path/to/kodstar`로
임시 프로젝트의 실제 PTY에서 거절·동의·정리를 검증한다. 설치 통합 검사는
`cargo test -p kodstar-cli --test installation --test installation_data --locked`로 실행한다.

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
python3 scripts/checks/source.py
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

### 로컬 이력 변경 검증

아래는 변경 시 확인할 계약이다. 개별 검증의 수행 결과는 실제 테스트 실행과 구분해 보고한다.
동작 계약은 [기능 명세](../kodstar-spec.md#7-cli-명령어-전체), 배치와 상태 전이는
[통합 설계](architecture.md#공통-실행과-분석)를 따른다.

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

같은 toolchain·target으로 빌드한 두 release 실행 파일과 변경하지 않을 입력을 준비한다.
macOS에서는 [analysis.py](../scripts/benchmarks/analysis.py)로 시간·peak RSS와 결과를 비교한다.
측정 중에는 다른 빌드나 입력 편집을 진행하지 않는다.

```bash
python3 scripts/benchmarks/analysis.py --baseline /path/to/before-kodstar --candidate target/release/kodstar \
  --project /path/to/frozen-fixture --file src/api.rs --repeats 3 --output tmp/benchmark.json
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

측정용 자식이 종료하고 미반영 WAL이 없을 때만 임시 저장소를 immutable로 읽는다.
실제 사용자 캐시·이력을 이 방식으로 열지 않는다. 분석 범위·스킵·미해결 수를 함께 확인하고,
변경 확인 포함 시간과 순수 조회를 구분한다. 분석 규칙 변경은 증분·전체 결과 일치도 검증한다.

Fossil 비교는 같은 추적 파일·변경 순서·내구성 조건으로 수행하고 KodStar의 분석 비용을 별도로 제시한다.
복원한 바이트·경로·속성을 비교해야 하며 서로 다른 저장 형식의 내부 해시를 직접 비교하지 않는다.
Fossil 실행 비교는 아직 수행하지 않았다.

### 앞선 청크·AST·메타데이터 최적화

2026-10-21에 남은 다섯 항목을 구현했다. 처리 순서·소스 구성과 소유권은
[아키텍처 문서](architecture.md), 전환·보존 계약은 [기능 명세](../kodstar-spec.md#153-영구-이력-저장소)에서 관리한다.

| 항목 | 구현과 경계 |
|---|---|
| 내용 청크 공유 | 영구 이력 **형식 2**. 64KiB 고정 경계의 청크를 내용 주소로 공유한다. 전체 blob과 청크·줄 metadata를 검증하며 이력을 삭제하지 않는다. |
| 설정 snapshot 재사용 | 한 설정 분석 안에서 snapshot connection과 준비 statement를 공유한다. 파일별 추가 connection·파일 구성 생성을 제거했다. |
| 분석 저장 중복 제거 | 분석 **형식 9**. 단일 이진 내용 해시로 파일을 식별하며 중복 timestamp 해시를 제거했다. 사실·환경 해시도 32바이트로 저장하고 scope·lookup 복합 키에 `WITHOUT ROWID`를 적용했다. |
| 파일 내 AST 재사용 | 한 AST로 끝난 예비 추출은 같은 트리로 다음 순회를 수행한다. 완료·오류·파일 전환에서 해제한다. 복수 AST 입력은 이전 트리를 해제한 뒤 다음 트리를 생성하는 경로를 유지한다. |
| 텍스트 diff 스트리밍 | 전체 내용의 무결성·UTF-8을 먼저 검증한다. 검증한 청크 주소·개행 metadata로 같은 구간을 건너뛰며 고정 버퍼와 출력 행 하나만 보유한다. 긴 행·원본 행 번호·CRLF를 보존한다. |

이 앞선 실험의 추출 규칙은 **43**, JSON schema·stdio protocol은 **2**다. 현재 구현의 버전은 아래 공통 개선 절에서 구분한다. 새 의존성은 추가하지 않았다.
매 명령의 소스 변경 확인과 기존 정상 분석 재사용 경로도 유지한다.

기존 이력과 캐시는 다음처럼 명시적으로 처리한다. 일반 실행은 형식 1을 자동 변경하지 않는다.
전환 중 손상·취소·알 수 없는 schema 객체·chunk 열을 발견하면 원형을 보존한다.
전환 후 압축 정리 실패는 이미 완료된 형식 전환을 취소하지 않으며 같은 명령으로 다시 시도한다.

```bash
kodstar history upgrade --root /work/project
kodstar cache clean --root /work/project
kodstar init /work/project
```

#### 시간·peak RSS

직전 실행 파일과 최종 release 실행 파일을 Rust 1.99.1·macOS 27.0.1 ARM64에서 비교했다.
`jobs=1`, `memory_budget=128MiB`, 5회 교차 실행의 중앙값이며 같은 canonical 프로젝트 경로에서
HOME·이력·캐시를 매번 새로 준비했다. 출력 행과 분석·보존 바이트·파일 구성·계보 해시는 일치했다.
대표성 있는 실제 프로젝트 전체의 속도나 OS 메모리 상한을 뜻하지 않는다.

| 입력·작업 | 시간: 전 → 후 | 최대 RSS: 전 → 후 |
|---|---|---|
| 설정 64개 참조·미지원 binary 1,500개·Rust 1개 최초 분석 | 268.79 → 248.27ms, 약 8% 단축 | 14.73 → 14.47MiB |
| 정수 literal 120,000개를 포함한 240,068바이트 Rust 파일의 호출 대상 변경 | 977.35 → 842.55ms, 약 14% 단축 | 69.14 → 68.94MiB |
| 약 1.89MB 텍스트에서 가운데 한 행 변경 diff | 18.98 → 21.40ms | 13.77 → 10.23MiB, 약 26% 감소 |
| 1.80MB 한 행의 끝부분 변경 diff | 21.51 → 23.06ms | 15.36 → 11.89MiB, 약 23% 감소 |

설정 입력의 coverage는 scanned 1,566·analyzed 1·auxiliary 65·unsupported 1,500·symbols 67·
relationships 64·unresolved 0이다. 큰 AST 입력은 analyzed 1·symbols 4·relationships 1·unresolved 0이며
skipped·unsupported는 0이다. 이력 전용 diff는 분석 cache를 필수로 열지 않으며 source 결과를 비교한다.

새 diff는 메모리 사용을 줄이는 대가로 위 입력에서 **1.55~2.41ms, 약 7~13%** 느렸다.
이 비용을 속도 개선으로 설명하지 않는다. 개행 없는 청크의 불필요한 탐색과 같은 잠금 안에서
이미 전체 검증한 내용의 반복 해시 계산은 제거했다. 내용·범위·행을 줄이지 않았다.

일반 Rust fixture는 482개 입력·분석 소스 36,150바이트, analyzed 481·symbols 1,922·relationships 480·
unresolved 480·skipped/unsupported 0이다. 최초 분석 375.17 → 381.33ms,
변경 확인 포함 조회 15.27 → 15.42ms, no-refresh 조회 9.60 → 9.60ms였다.
이 작은 시간 차이는 새 속도 개선으로 해석하지 않는다.

#### 저장량·이전 형식 검증

같은 482 입력에서 초기 분석 cache는 **2.25 → 1.93MiB, 약 14% 감소**했다.
작은 고유 파일의 이력 DB는 청크 주소·줄 metadata 비용으로 **260 → 308KiB**로 늘었다.
두 저장소의 합계는 **2.50 → 2.23MiB**였다. 공유 청크가 적은 입력의 공간 비용도 함께 고려한다.

별도 실험은 8MiB 의사 난수 binary와 작은 Rust 파일에 같은 64KiB 영역의 한 바이트를 세 번 수정하고
최초 상태를 복원했다. 보존하는 논리 내용은 전후 약 32MiB로 동일하며 과거 버전을 생략하지 않았다.
아래 파일 크기는 한 번의 저장 실험이다.

| 단계 | 이력 DB 파일: 전 → 후 |
|---|---|
| 최초 저장 | 8.13 → 8.14MiB |
| 변경 없는 verify | 8.13 → 8.14MiB |
| 한 바이트 변경 3회 | 32.34 → 8.35MiB |
| 최초 상태 복원 | 32.34 → 8.35MiB |

실제 이전 실행 파일이 만든 형식 1 저장소의 명시적 업그레이드도 확인했다.
DB 파일은 **32.34 → 8.34MiB**였으며 모든 보존 blob의 실제 바이트·ID·구성·계보 해시가 일치했다.
이전 분석 cache가 자동 대체되지 않는 것, 명시적 정리 후 같은 분석을 재생성하는 것,
최초·최신 버전의 실제 파일 복원도 확인했다.

회귀 검사는 전환 중 취소·손상·알 수 없는 추가 데이터 보존, 압축 정리 실패 후 재시도,
SQLite page 한도로 재현한 저장 공간 부족, 청크 경계의 UTF-8·CRLF·긴 행·빈 내용·개행 metadata 손상을 포함한다.
동시 실행의 기존 lease·복원 중단·rollback·마지막 정상 세대 검사도 수행했다.
transient WAL·임시 파일의 최대 디스크 사용량은 측정하지 않았다. 전환·압축 정리는 추가 디스크가
필요할 수 있으며 자동 이력 삭제·내용 압축·보존 기간 변경은 도입하지 않았다.

AST·allocator의 peak는 소스 바이트 크기로 예측하지 않는다. 설정 예산은 OS hard RSS limit이 아니다.
검증한 실행 플랫폼은 macOS ARM64이며 Linux 실행은 이번 환경에서 확인하지 않았다.

### 공통 비용 축소 구현

현재 추출 버전은 **44**, 분석 cache는 **11**, 영구 이력은 **4**이다. JSON schema·stdio protocol **2**는
유지한다. 새 의존성은 추가하지 않았다. 구현 책임과 안전한 부분 갱신 범위는
[아키텍처 문서](architecture.md#실프로젝트-비용-축소-구현과-후속-설계)가 정의한다.

기존 이력 형식 1·2·3과 분석 cache 10 이하는 자동 변경하지 않는다. 별도 실행 파일 교체 후 해당 프로젝트에서
다음 명령으로 명시적으로 전환·재생성한다. 이력 전환은 원문·ID·계보·이름·브랜치를 보존한다.

```bash
kodstar history upgrade --root /work/project
kodstar cache clean --root /work/project
kodstar init /work/project
```

회귀 검사는 README 수정·삭제(Python·JS·Rust·Go), 분석 입력의 제거·형식 전이, Go 본문과 interface
호출 변경, 성공·실패 lookup의 새 대상·동명 후보·rename·삭제, 타입 경계의 전체 fallback을 포함한다.
부분 갱신의 snapshot은 **full을 실행하기 전에** 수집해 동일 입력의 전체 결과와 비교한다.
무관한 관계 행의 ID를 표시해 실제 재사용도 확인한다.

statement 풀은 재귀 중 같은 SQL의 별도 활성 cursor, named parameter 정리, 유휴·바이트 예산,
쿼리 실패 후 회수를 검증한다. 이력 전환은 형식 1·2·3의 실제 내용·FK·무결성, 원문 손상·취소·
알 수 없는 schema·공간 부족 rollback, 전환 후 정리 실패 재시도를 확인한다. uninstall은 같은 형식 인식을 사용한다.

분석 비교에서 lookup key의 내부 파일 ID는 경로로 정규화한다. 공유 검증 SQL인
[lookups.sql](../scripts/benchmarks/lookups.sql)을 Rust snapshot과 Python benchmark가 함께 읽는다.
이 정규화는 ID만 다른 같은 lookup을 비교하며, 성공 여부를 바꾸면 여전히 차이를 검출한다.

`analysis.py --mutations`는 복사본에 고유 Go probe와 문서를 추가하고 초기 분석·동일 상태 조회에 더해
문서 변경·함수 본문·정의 추가·삭제를 비교한다. 원본은 변경하지 않으며 매 단계 coverage·일곱 분석
해시·원문/구성/계보 세 해시를 대조한다. 이 옵션은 Go fixture의 검증이며 다른 언어의 속도를 추정하지 않는다.

#### 압축 사전·관계 블록과 기존 이력 재압축 — 2026-10-22

현재 분석 저장 형식은 **12**, 이력 형식은 **4**, 추출 규칙은 **44**, JSON·stdio 계약은 **2**다.
사전·관계 저장은 `index/dictionary`·`index/relations`, SQLite cursor 경계는 `index/packed`에 둔다.
이력은 `content`·`checkout`·`inputs`·`maintenance`로, 스크립트는 `benchmarks`·`checks`·`grammars`로 나눴다.
source/header 검사·grammar 생성·benchmark 명령과 Rust의 공유 검증 SQL 참조도 새 경로를 사용한다.

Rust 1.99.1·macOS 27.0.1 ARM64에서 앞선 형식 11 실행 파일과 최종 형식 12 실행 파일을
**3회 교차 실행**했다. 빌드·다른 성능 검사를 동시에 실행하지 않았다. 동일한 canonical 경로에서
고정한 v-code 추적 파일 657개와 Go probe·문서를 사용했다. jobs=1·memory_budget=128MiB다.
최초 coverage는 scanned 659·analyzed 640·unsupported 18·skipped 0, source_bytes 1,364,762,
symbols 3,353·resolved relationships 13,480·unresolved 4,472다. 모든 변경 단계에서 coverage,
일곱 분석 의미 해시와 원문·파일 구성·계보의 세 해시가 일치했다.

| 항목 | 형식 11 | 형식 12 |
|---|---:|---:|
| 초기 분석 캐시 파일 합계 | 16,782,107B | 13,386,523B |
| 초기 분석 peak RSS | 25.53MiB | 24.25MiB |
| 초기 분석 | 3.833초 | 4.478초 |
| 변경 없는 동기화 조회 | 20.98ms | 21.42ms |
| no-refresh 조회 | 10.77ms | 10.83ms |
| 문서 변경 | 41.94ms | 63.86ms |
| Go 본문 변경 | 73.37ms | 88.21ms |
| 함수 정의 추가 | 2.641초 | 3.300초 |
| 함수 정의 삭제 | 2.646초 | 3.294초 |

저장 공간은 **20.2%**, 초기 peak RSS는 **5.0%** 줄었다. 초기 분석은 **16.8%**, 정의 추가·삭제는
약 **24~25%** 느려졌다. 저장·메모리 절감과 압축 공개·검색 비용의 교환이며 속도 개선으로 설명하지 않는다.
초기 분석의 새 실행 파일 3회는 5.217·4.478·4.471초였다. 조회의 작은 차이는 이 반복 수로
일반적인 속도 차이라고 단정하지 않는다. 본문 변경 RSS는 17.47→16.41MiB, 정의 추가는
23.30→22.17MiB였다. 문서 변경 RSS는 14.58→15.02MiB였다.

초기 분석 DB의 사전과 검색 경로는 4,448,256→2,060,288B, 관계와 검색 경로는
3,907,584→2,916,352B였다. 초기 DB freelist는 양쪽 모두 0이었다. key ID·역방향 경로가
payload 외의 주요 비용으로 남는다. 큰 단일 key는 원문을 중복한 sparse 경계 없이 보존하며,
256KiB SQLite 행 한도에서 120KB Unicode key의 정확 검색·재공개·doctor도 확인했다.

추가 일반 압축은 별도로 비교했다. 선언 추가 상태의 13,443,072B DB에서 사전·관계 payload
합계 1,187,429B를 Python 표준 zlib의 DEFLATE level 6으로 블록별 비교했다. 32B 이상의
이득이 있는 블록만 선택하면 payload에서 475,134B를 더 줄일 수 있었다. DB 페이지·인덱스의
실제 회수량을 뜻하지 않는다. DB의 약 3.5%에 해당하는 이 추가 가능성보다 현재 SQL 공개·조회
비용과 의존성 조건을 우선해 제품에 별도 codec을 추가하지 않았다. Cargo 의존성은 기존
rusqlite의 `vtab` feature만 확장했으며 새 라이브러리는 추가하지 않았다.

기존 형식 4의 독립 raw 청크를 재압축하는 별도 CLI 검증도 했다. 8KiB 바이너리 한 개를
4바이트씩 1,000회 바꾼 1,001개 체크포인트에서 DB는 **9,084,928→450,560B**, 약 **95.0%** 줄었다.
이 fixture는 독립 저장된 기존 내용을 모사한 것으로 현재 자동 델타 수집 결과의 추가 절감률이 아니다.
`history upgrade`는 0.229초·peak RSS 14.94MiB였다. 모든 blob 원문, 보존 metadata·ID·chunk refs의
해시가 같았고 c1·c1001을 실제 복원한 바이트와 doctor가 일치했다. 기준의 순서·깊이 위반은 0개였다.
이미 형식 전환을 commit한 뒤 재압축이 실패하면 형식 4를 보존하고 해당 단계 재시도를 안내한다.

검증은 Rust 테스트 **454개**, formatting, workspace all-target Clippy `-D warnings`,
debug·release·dist build, 469개 소스의 크기·헤더·협력 경로 검사, 고정 grammar 파일 14개의
출처 검사와 실제 터미널 확인 검사를 포함한다. 손상·공간 부족·취소·부분 공개 rollback,
증분·전체 결과 일치와 실제 복원을 검증했다. 사용자 원본 v-code·이력·전역 설치는 변경하지 않았다.
Linux 실행·Fossil 자체 비교·unlink된 임시 파일을 포함한 순간 최대 디스크 사용량은 측정하지 않았다.
최종 측정 JSON과 dist 실행 파일만 의도적인 결과로 남기고 소스 복사본·DB·별도 HOME·이전 바이너리·빌드 캐시는 정리했다.

#### 앞선 저장 표현 검증 — 형식 11·4, 2026-10-22

Rust **1.99.1**과 macOS 27.0.1 ARM64에서 형식 11·4를 검증했다. Homebrew의 직접 실행 파일이
rustup shim보다 앞서는 환경에서는 `rustup show`의 활성 버전과 실제 `rustc --version`이 다를 수 있다.
이번 최종 검사는 지정 toolchain의 bin을 자식 프로세스 PATH 앞에 두고 cargo·rustc·Clippy·rustfmt를
모두 같은 버전으로 실행했다. 사용자 shell 설정은 변경하지 않았다.

workspace 테스트 **444개**(core 단위 35·엔진 379·CLI 30), formatting, all-target Clippy
`-D warnings`, debug·release·dist build를 통과했다. 직접 작성한 소스·스크립트 445개는 모두
350줄 미만이며 헤더·참조 검사를 통과했다. 새 의존성은 없으며 기존 rusqlite의 functions feature를 사용한다.

정수 블록의 최대 ID·성공 bit·손상·초과 길이, 공개 중 실패 rollback과 파일별 부분 갱신을 확인한다.
블록 안의 fact는 개별 SQLite FK로 표현하지 않으므로 doctor가 복원한 ID의 존재·파일 소유권·중복을
별도로 검사한다. 구간 복원은 가변 청크의 경계 전후·파일 끝에서 원문과 대조한다.

바이트 인코딩은 결정적 바이너리·반복 입력의 round trip, 잘못된 literal/copy·가변 정수·길이·base,
모든 2바이트 payload의 오류 처리와 출력 한도를 검증한다. 작은 파일 1,000회 수정의 모든 원문을
다시 읽으며 델타 base가 다른 델타를 참조하지 않는지 확인한다. 내용 기반 경계는 input read 크기와
독립이며 큰 앞부분 삽입 뒤의 재공유를 확인한다. 형식 1·2·3의 내용·metadata·FK와 취소·손상·
알 수 없는 schema·디스크 부족 rollback, 정상 미참조 내용 주소의 보존도 확인한다.

분석·이력의 논리 SQL view를 직접 읽는 도구에는 제한된 decoder 함수 등록이 필요하다.
[postings.py](../scripts/benchmarks/postings.py)와 [content_codec.py](../scripts/benchmarks/content_codec.py)는
제품 decoder와 독립적인 비교 구현이다. [history.py](../scripts/benchmarks/history.py)는
작은 수정 1,000회와 큰 삽입 20회의 모든 blob 바이트·구성·계보를 비교하고 초기·최신 체크포인트를
실제로 복원한다. 같은 원문을 복원하는지와 저장소 전체 크기를 함께 확인한다.

최종 배포 실행 파일과 앞선 형식 10·3 실행 파일을 **같은 Rust 1.99.1**, 고정 canonical root에서
5회 교차 비교했다. v-code 추적 파일 657개에 Go probe·문서를 더한 입력이며 jobs=1·128MiB다.
scanned 659·analyzed 640·unsupported 18·skipped 0, source_bytes 1,364,762·symbols 3,353·
resolved relationships 13,480·unresolved 4,472를 유지했다. 모든 작업에서 coverage·일곱 분석 해시와
원문·구성·계보 세 해시가 일치했다. 다음 값은 중앙값이며 파일 크기는 미반영 WAL이 없는 합계다.

| 항목 | 앞선 형식 | 현재 형식 |
|---|---:|---:|
| 초기 분석 cache | 18.86MiB | 15.99MiB |
| 초기 영구 이력 | 2.27MiB | 1.56MiB |
| 초기 분석 시간 / peak RSS | 3.768초 / 25.81MiB | 3.849초 / 25.50MiB |
| 문서 변경 / peak RSS | 42.40ms / 16.31MiB | 41.85ms / 14.52MiB |
| Go 본문 변경 / peak RSS | 66.69ms / 17.73MiB | 73.36ms / 17.47MiB |
| 함수 추가 / peak RSS | 2.621초 / 24.27MiB | 2.641초 / 23.22MiB |
| 함수 삭제 / peak RSS | 2.611초 / 23.64MiB | 2.652초 / 22.59MiB |
| 변경 없는 동기화 조회 | 20.71ms | 21.36ms |
| no-refresh 조회 | 10.11ms | 11.09ms |

분석 cache는 약 15.2%, 초기 이력은 약 31.4% 줄었다. 초기 분석 약 2.2%·본문 약 6.7ms의
추가 비용이 있으므로 모든 작업의 속도 개선으로 설명하지 않는다. 원래 문자열·관계·조회 근거를
보존한 결과다. 쓰기 page cache는 설정 예산의 1/4로 분배해 인코딩·공개 중 보유량을 낮췄다.
동일 형식의 앞선 조정본에서 관측한 초기 RSS 약 29.5MiB 증가는 최종 실행 파일에서 남지 않았다.

장기 이력은 고정 seed의 바이너리 fixture를 같은 경로에서 각 실행 파일로 별도 생성했다.
작은 변경은 8KiB 원문의 4바이트를 버전마다 바꾸고, 큰 변경은 1MiB 원문 앞에 매번 16바이트를
추가한다. 실제 프로젝트 수정 이력의 압축률로 일반화하지 않는다.

| 변경 trace | 앞선 이력 | 현재 이력 | 갱신 시간 중앙값 | 초기 상태 복원 |
|---|---:|---:|---:|---:|
| 작은 파일 1,000회 수정 | 8.70MiB | 0.45MiB | 11.81 → 11.85ms | 24.76 → 30.21ms |
| 큰 파일 앞부분 20회 삽입 | 21.28MiB | 1.30MiB | 19.94 → 19.80ms | 32.98 → 39.71ms |

두 trace의 전체 DB는 각각 약 94.9%·93.9% 줄었다. 모든 blob의 원문·현재 파일 구성·계보가 같으며
초기·최신 상태의 실제 복원과 doctor를 통과했다. 작은 trace의 갱신 최대 RSS는 11.17 → 11.36MiB,
큰 trace는 12.45 → 12.92MiB였다. 복원에는 인코딩 해석과 주소 검증 비용이 있으므로 저장 절감과
읽기·CPU 비용을 구분한다.

앞선 실행 파일로 만든 실제 v-code 입력 복사본의 형식 3도 전환했다. 이력은
2,396,160 → 1,605,632바이트였고 모든 metadata·원문·계보를 보존했다. 캐시 10은 전환 뒤에도
명시적 정리 전에는 거절했고, 정리·init 뒤 형식 11의 일곱 의미 해시가 같았다. c1·c2의 문서를 실제
복원했다. 사용자의 실제 프로젝트·이력·cache·설치는 변경하지 않았다.
기존 이력 전환에서는 원래 청크를 독립 인코딩한다. 과거 모든 버전의 CDC 재분할이나 델타 기준
재선정까지 수행한 결과로 해석하지 않는다. 새 수집부터 해당 선택을 적용한다.

최종 dist 실행 파일은 41,452,272바이트이며 전체 grammar를 유지한다. 비교 중 발생한 소스 복사본·
별도 HOME·이전 실행 파일은 검증 뒤 정리했다. Linux 실행, Fossil 자체와의 실행 비교,
WAL·unlink된 임시 파일까지 포함한 순간 최대 디스크 사용량은 이번 검증에 포함하지 않았다.

#### 앞선 공통 개선 실측 — 2026-10-22

다음은 분석 형식 10·이력 형식 3 단계의 기록이다. 이후 저장 표현 변경의 결과는 별도로 기록한다.

Rust 1.99.1·macOS 27.0.1 ARM64, jobs=1·memory_budget=128MiB, 고정한 v-code 원문에
Go probe 1개·문서 1개를 더했다. 최초 상태는 scanned 659·analyzed 640·unsupported 18·skipped 0,
source_bytes 1,364,762·symbols 3,353·resolved relationships 13,480·unresolved 4,472다.

검증은 두 단계다. 기존 실행 파일↔공통 개선본을 5회 교차 비교한 뒤, 개선본↔최종 메모리 조정본을
별도 5회 교차 비교했다. **각 비교 안**에서는 canonical 경로를 고정했으며 두 비교의 절대 경로는 다르다.
원문 manifest는 두 단계가 같고, 각 단계의 모든 작업에서 coverage와 열 개 결과 해시가 일치했다.
다음 표는 1차의 기존본과 2차의 최종본 중앙값이다. 한 차례의 직접 비교 결과로 섞어 설명하지 않는다.

| 작업 | 시간 | peak RSS |
|---|---|---|
| 초기 분석 | 15.89초 → 3.76초 | 27.73 → 25.81MiB |
| 문서 변경 | 15.10초 → 42.14ms | 26.33 → 16.33MiB |
| Go 본문 변경 | 15.07초 → 66.48ms | 27.03 → 17.77MiB |
| 함수 정의 추가 | 15.08초 → 2.63초 | 26.92 → 24.25MiB |
| 함수 정의 삭제 | 15.09초 → 2.62초 | 26.30 → 23.61MiB |

메모리 조정은 쓰기 연결의 page cache를 설정 예산의 절반으로 분배하고 유휴 statement 보유량을
읽기 예산의 1/4·최대 1MiB로 제한한 변경이다. 활성 작업의 읽기 한도와 예산 초과 오류는 유지한다.
2차 직접 비교에서 초기 분석은 3.737 → 3.761초, peak RSS는 30.08 → 25.81MiB였다.
정의 삭제는 2.593 → 2.616초, 32.14 → 23.61MiB였다. 소폭의 시간 비용으로 보유 메모리를 줄였다.
최종 변경 없는 동기화 조회는 20.48ms,
no-refresh 조회는 10.11ms였다.

같은 초기 상태의 분석 cache 파일 합계는 약 25.35 → 18.86MiB,
이력 파일 합계는 3.19 → 2.27MiB였다.
새 실행 파일의 배포 크기는 약 39.55MiB이며 전체 grammar를 유지한다. 실행 시간 단축·용량·메모리
효과는 이 입력에 대한 결과이고 다른 언어의 실프로젝트 성능 보장은 아니다.

실제 v-code 형식 2 DB의 **복사본**도 전환했다. 보존 테이블·모든 청크 실제 내용의 해시는 같았고,
DB는 3,391,488 → 2,400,256바이트였다. 과거 c1·c2의 SYSTEM.md를 각각 실제 복원해 바이트를 대조했다.
기존 실행 파일이 만든 이력 2·cache 9 조합에서 명시적 업그레이드, cache 정리, init도 확인했다.
이전 cache는 정리 전에는 계속 거절되며 자동 재생성하지 않는다.

임시 HOME에서 이전 실행 파일 설치 → 실제 install.sh의 dist 빌드·교체 → 초기화 → clean uninstall을
검사했다. 사용자의 shell 본문과 프로젝트 원문을 보존했다. Rust 테스트 430개, formatting,
workspace all-target Clippy -D warnings, debug·release·dist 빌드와 source/header·문서 검사도 수행했다.
실제 사용자 이력·cache·설치는 변경하지 않았다.

WAL·unlink된 임시 파일까지 포함한 순간 최대 디스크 사용량, 장기 수정 trace·Fossil 비교와 Linux 실행은
이번 검증에 포함하지 않았다. 이력 전환은 추가 임시 디스크가 필요하며, 압축과 자동 이력 삭제는 도입하지 않았다.

### 실프로젝트 병목 분석과 후속 검증

2026-10-21의 `v-code` Go 프로젝트 측정이다. 다음 수치는 구현 완료 항목의 추가 성능 보장이 아니라
[구현·후속 설계](architecture.md#실프로젝트-비용-축소-구현과-후속-설계)의 근거다. 당시 설계 단계의 분석은 제품 코드·설정·
실제 저장 형식을 변경하지 않았다. 소스와 DB 복사본·심볼 제거 복사본은 실험 후 정리했다.

| 기준 | 관측 결과 |
|---|---|
| 입력 | 추적 657개, Go 639개·약 1.30MiB. Go module 설정 1개, 분석 미지원 17개, 한도 스킵 0개 |
| 분석 상태 | 심볼 3,349, 확정 관계 13,479, 미해결 fact 3,625·모호 fact 847 |
| 기존 실제 실행 | 최초 분석 15.64초·peak RSS 28.22MiB. 변경 확인 검색 17.39ms, no-refresh 검색 7.39ms: 각 5회 중앙값 |
| 고정 복사본의 변경 | 파일 추가·대상 추가·rename/수정·삭제 각각 약 15.1~15.3초. 전체 분석과 7종 의미 결과 해시 일치 |
| 추가 profile 실행 | 최초 16.16초, resolve 진행 표시 시작 643.89ms. 본문 1개 변경 15.98초, resolve 표시 시작 34.72ms |
| 본문 변경 추출 | parsed 1, reused 656. 전체 파일을 다시 파싱한 것이 아니라 관계 해석·후속 저장 범위가 넓음 |
| 메모리 근거 | 28MiB 안팎의 관측 RSS는 작은 Go 입력의 결과다. 큰 파일·다른 언어·병렬도나 hard RSS 상한을 검증한 수치가 아님 |

profile은 동일한 release 실행 파일, 별도 HOME·소스 이력·분석 cache에서 수행했다. stderr PTY의
진행 시각을 기록하고 resolve 시작 뒤 5초 동안 macOS `sample`을 실행했다. 표본에서 SQLite VM 실행,
SQL parser·query planner, allocation·복사가 반복됐다. 표본의 상위 함수 빈도를 전체 실행 비율로
해석하지 않는다. `resolve` 표시는 commit·coverage 계산과 구분되지 않으므로 세부 계측이 더 필요하다.

개선 전 소스 확인상 부분 본문 재해석은 `incremental::reusable_bodies`의 단순 Rust 조건에 한정된다.
Go 변경은 `Resolution::All`로 관계·영구 lookup 의존성을 지운 뒤 다시 생성한다.
당시 영구 lookup 기록을 영향 fact 선택에 사용하는 경로는 없었으며, 해석 중 임시 lookup 기록의
Python 의미 판정 용도와 구분해야 한다.

#### 저장 구조 실험

실제 DB는 읽기 또는 설치·이력 잠금 아래 미반영 WAL이 없음을 확인한 복사에만 사용했다.
변형·VACUUM은 소유한 SQLite 복사본에 수행했다. 최종 파일 크기는 복사본을 DELETE journal로
정리한 뒤 page count × page size와 일치하는지 확인했다. WAL이 남은 본체 크기만으로 비교하지 않는다.

| 대상 | 관측·복사본 실험 | 해석 |
|---|---|---|
| 분석 DB | 25.32MiB 중 `lookup_dependencies`와 역조회 index가 13.77MiB, 약 54.4% | 같은 긴 문자열 key와 kind가 fact마다, 인덱스에도 반복됨 |
| lookup 정규화 | 130,662행·53,184개 고유 key. 원본 25.32MiB → 단순 VACUUM 23.28MiB → key ID 공유 17.77MiB | 정리 효과와 분리한 구조 효과는 약 23.7%. 양방향 행 차집합 0과 integrity 확인. 실행 성능은 미측정 |
| 관계 문자열 | 36,725행에서 rule 약 0.66MiB, reason 약 1.20MiB의 문자열 payload | 반복 값 공유의 후속 후보. 전체 페이지 절감량이나 속도 이득을 뜻하지 않음 |
| 이력 DB | 원본 3.23MiB, 청크 본문 table 2.88MiB 중 unused 약 1.34MiB | BLOB과 페이지 배치가 우선 검토 대상 |
| 청크 rowid 배치 | VACUUM 기준 3.13MiB → 2.29MiB, 약 26.8% 감소 | 같은 662개 청크·1,572,454바이트 원문, FK·integrity 확인. 실제 업그레이드·복원 구현 검증은 별도 필요 |
| 압축 가능성 | 기존 청크별 zlib level 3·raw fallback의 payload 1,572,454 → 665,783바이트 | Python 기본 도구의 가능성 조사만 수행. 코덱 선택·전체 DB 크기·실행 시간·새 의존성 채택 근거로 확정하지 않음 |
| 실행 파일 | 40.96MiB, `__TEXT.__const` 약 31.24MiB. 큰 symbol 주소 구간은 grammar parse table | Rust 코드 최적화만으로 큰 폭 축소를 기대하기 어려움 |
| 심볼 제거 복사본 | 40.96 → 39.47MiB, 약 3.6% 감소. `--version` 성공 | 언어 전체·설치·오류 처리 회귀 검증은 배포 설정을 실제 적용할 때 수행 |
| 개발 target | 약 466.8MiB, `deps/`와 native build 출력이 대부분 | 설치 실행 파일 용량과 별도 지표. 제거 시 다음 빌드 비용 증가 |

영구 이력 662개 청크는 모두 64KiB보다 작고 최대 25,284바이트였다. 따라서 현재 고정 청크 공유가
이 입력의 작은 파일 내부 변경을 분할해 공유해 주지는 않는다. 장기 증가량은 최초 DB 크기와 별도로
100·1,000회 변경 trace에서 파일 앞부분 삽입·본문 수정·rename·동일 내용 복원으로 측정해야 한다.

#### 설계 당시 순서와 합격 기준

1. 세부 단계와 SQL 준비·step·scan·sort·임시 디스크 최대치를 계측하고 반복 SQL 준비를 줄인다.
   변경 없는 검색의 현재 경로는 유지하며, 먼저 전체 분석 비용을 줄이는 저위험 변경으로 검증한다.
2. lookup key 공유와 역방향 의존성 소비를 같은 설계로 연결한다. Go의 일반 본문 변경부터
   `SelectedFacts`를 허용하고 package·정의 변화·method set으로 범위를 확장한다.
3. 저장 정규화와 관계 문자열·인덱스를 개별 측정한다. 영구 이력 rowid 배치는 별도 형식 전환으로 검증한다.
4. 배포 심볼 제거와 LTO 조합을 비교한다. 압축·내용 기반 청크 분할은 장기 이력 trace 뒤에 결정한다.

초기 성능 목표는 동일한 고정 Go 입력의 일반 본문 변경 **1초 미만**, 최초 분석 **5초 미만**이다.
이는 설계 당시의 목표다. 구현 후 측정 결과는 공통 비용 축소 구현 절에서 구분한다. 변경 없는 검색은 기존 중앙값 대비 10% 초과 회귀를 피하고,
peak RSS도 같은 입력·작업에서 기준 대비 10% 초과 증가 시 재검토한다. 서로 다른 작업의 RSS를 섞지 않는다.

부분 갱신은 정상 전체 분석과 입력·정의·scope·binding·관계·사용 위치·후보·lookup·진단을 정규화해
비교한다. 성공한 조회와 실패한 조회, 새 동명 후보·삭제·rename·shadowing·import/export·receiver·
interface·설정 변경을 포함한다. 생성 ID가 바뀔 수 있는 비교는 의미 식별자와 위치로 정규화한다.
실패한 동등성 검사는 성능 결과와 별도로 실패로 보고하며 분석 범위 축소로 통과시키지 않는다.

저장 변경은 rollback·취소·공간 부족·동시 reader/writer·GC·마지막 정상 세대를 검증한다.
이력 형식 전환은 모든 원문·체크포인트 계보·이름·브랜치를 대조하고 과거·최신 파일을 실제 복원한다.
측정은 5회 이상 교차 실행하고 CPU 시간·RSS·WAL/임시 파일 최대치·최종 DB 크기를 함께 기록한다.
Linux 실측과 Fossil의 동일 입력·내구성 조건 비교는 아직 수행하지 않았다.

#### 언어·맥락별 공통 개선 검증

개선 전 같은 날 별도의 작은 Python·JavaScript·Rust 프로젝트에서 **README.md만 수정**했다.
세 경우 모두 parsed 0·reused 1이고 심볼·관계 개수는 같았지만 `resolve` 진행 단계에 진입했다.
이력 checkpoint가 추가되는 동작은 필요하며, 분석 의존성이 없는 내용 변경까지 전체 관계 해석으로
연결되는 부분이 공통 개선 대상이다. 이는 경로 재현이며 다른 언어의 실프로젝트 실행 시간 측정은 아니다.
세 임시 프로젝트와 전용 HOME은 자동 정리했다.

공통 엔진 변경은 한 언어의 시간만으로 검증하지 않는다. 기존 언어별 회귀 사례를 활용하고 아래의
대표 경계에 필요한 fixture를 더한다. 모든 언어·변경·규모의 무차별 조합은 만들지 않는다.

| 대표 입력 | 확인할 고유 무효화 경계 |
|---|---|
| Go | package 이름, 새 동명 정의, receiver·embedding·interface method set |
| Rust 단순·복잡·혼합 프로젝트 | 기존 빠른 본문 경로, trait·impl·macro, 관계없는 JS 파일이 함께 있는 경우 |
| Python | lexical binding·shadowing, 명시/별표 import, export 변경, MRO·property |
| JS/TS monorepo | 재수출, package exports, workspace 연결, tsconfig 상속·path alias |
| C/C++ | header/include 전파, 선언·정의 병합, 조건부 선언과 overload |
| 혼합 포맷 | Vue·Svelte 등의 script/template 연결, 원본 위치 mapping, 외부 component 변경 |

각 사례에서 이력 전용 문서 변경, 위치만 이동, 함수 본문, 정의 추가·삭제·rename,
실패 lookup의 새 대상, 설정 변화와 지원/제외 상태 전이를 필요한 범위로 검증한다.
파일 수·원문 바이트·사실 수·후보 수·영향 범위·이력 길이를 별도 축으로 보고,
작은 파일 다수·대형 파일 소수·밀집 참조·순환 import·장기 이력에서 자원 상한과 결과 일치를 확인한다.

공통 계측은 변경 입력·파싱·재사용·무효화·재해석 fact·갱신 관계의 개수, 범위 확대 이유,
단계 시간·SQL 준비/step·RSS·영구/WAL/임시 디스크를 구분한다. 언어별 결과·후보·진단을 보존한
동등성 확인을 먼저 통과해야 하며, 절감률은 실제 측정한 입력과 작업에만 적용한다.

## 의존성과 grammar

주요 라이브러리는 GitHub 3,000 stars 이상을 기준으로 선정한다.
언어 grammar·전이 의존성은 승인된 예외로서 고정 버전과 라이선스를 검토한다.
공식 Tree-sitter와 canonical grammar 원본을 사용한다.

| 관리 대상 | 기준 파일 |
|---|---|
| Rust dependency 해석 결과 | [Cargo.lock](../Cargo.lock) |
| 저장소·검토 시점·grammar checksum·revision·ABI | [dependencies.json](../LICENSES/dependencies.json) |
| 라이선스 원문·NOTICE와 배포 고지 | [LICENSES](../LICENSES), [third-party-notices.txt](../LICENSES/third-party-notices.txt) |
| Kotlin 원본과 생성 파일 해시 | [SOURCE.json](../vendor/tree-sitter-kotlin-ng/SOURCE.json) |
| Kotlin grammar 수정 규칙 | [patch.json](../vendor/tree-sitter-kotlin-ng/patch.json) |

의존성 변경은 manifest·lockfile·라이선스/NOTICE·native 번들·grammar ABI를 함께 확인한다.
Swift grammar의 배포 원본처럼 revision만으로 배포물을 재구성할 수 없는 경우에는 crate checksum을 기준으로 한다.

```bash
cargo metadata --locked --format-version 1
python3 scripts/grammars/kotlin.py --check
```

Kotlin grammar의 구분자 규칙은 고정 원본에서 재생성한다. 일반 빌드는 저장된 parser를 사용한다.
원본 규칙을 변경할 때만 아래 도구를 설치하고 재생성한다. 생성 C 파일은 직접 수정하지 않는다.

```bash
cargo install tree-sitter-cli --version 0.26.10 --locked --root ./tmp/tree-sitter-cli
python3 scripts/grammars/kotlin.py --tree-sitter ./tmp/tree-sitter-cli/bin/tree-sitter
```

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
