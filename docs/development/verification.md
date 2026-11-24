# 변경별 검증과 성능 비교

[개발 환경과 공통 검사](../development.md) · [아키텍처](../architecture.md)

## 변경 책임에 따른 테스트

[tests/engine.rs](../../crates/kodstar-core/tests/engine.rs)가
[tests/cases](../../crates/kodstar-core/tests/cases)의 통합 사례를 연결한다.
각 crate의 단위 테스트는 해당 모듈에 붙어 있고, CLI integration은
[kodstar-cli/tests](../../crates/kodstar-cli/tests)가 실제 프로세스를 실행한다.

| 변경 책임 | 먼저 확인할 경계 |
|---|---|
| 명령 파싱과 초기화 | CLI `args::tests`, `startup`, `commands`, `synchronization`에서 도움말·예시·첫 초기화·반복 실행·출력 확인 |
| 현재 조회와 잠금 | core `engine_contracts`, `consistency`, `refresh_reuse`에서 준비·취소·정체와 읽기 세대 확인 |
| 구문과 구성 | core `hybrid_recovery`, `configuration_failures`, 언어별 사례에서 부분 구문·지원 밖 입력·설정 경계와 복구 확인 |
| 해석과 증분 | core `module_incremental`, `incremental_bodies`, `append_reuse`, `return_values`에서 조회 실패 포함 소비자·ID·좌표와 전체 재분석 대조 |
| 그래프·HTTP | CLI `graph_navigation`, core `http`, `htmx`, `ejs`와 traversal 단위 테스트에서 대상·정렬·근거·탐색 한도 확인 |
| C/C++ | core `clang`, `native_context`, `native_inputs`, `native_input_budget` 및 native crate에서 VFS·진단·과거 include·자원 확인 |
| 이력 저장과 캐시 분리 | core `local_history`, `history_operations`, `history_paths`, `history_streaming`에서 bytes·경로·조건과 캐시 정리 후 분석 재생성 확인 |
| 복원·중단·영향 | core `history_recovery`, `history_watch`, `history_impact`에서 보호 저장·충돌·재개·삭제 전 사용 근거 확인 |
| 설치·세션 | CLI `installation`, `installation_data`, `session`, `watch`와 Python 프로세스 검사로 정리·종료 규칙 확인 |

```bash
cargo test -p kodstar-cli --locked --test graph_navigation
cargo test -p kodstar-core --locked --test engine incremental_bodies
cargo test -p kodstar-core --locked --test engine return_values
cargo test -p kodstar-core --locked --test engine clang
cargo test -p kodstar-clang --locked
```

설치와 출력 정체를 검사할 때는 만든 CLI의 절대 경로를 전달한다. 검사 스크립트가 격리한
프로젝트·홈·임시 경로를 사용하고 자식 프로세스 종료를 기다린 뒤 정리한다.

```bash
python3 -B scripts/checks/confirmation.py /absolute/path/to/kodstar
python3 -B scripts/checks/installation.py /absolute/path/to/kodstar
python3 -B scripts/checks/session.py /absolute/path/to/kodstar
```

복원 테스트는 임시 프로젝트의 충돌·부분 반영·복구를 관찰한다. 실제 작업 루트와 검증 프로젝트는 별도다.
장애 검증에서는 보존 성공과 분석 성공, 파일 적용과 참조 확정을 각각 확인한다.
소스 복제본·DB와 WAL·출력 spool·실행 소유 target은 종료 후 회수한다. 의도적인 소형 fixture만 추적한다.

## 범위에 맞는 추가 검사

| 도구 | 작업과 기준 |
|---|---|
| Nextest 0.9.151 | [nextest.toml](../../.config/nextest.toml)의 실행 그룹과 native 동시성 적용. doctest는 Cargo에서 별도 실행한다. |
| cargo-machete 0.9.2 | dependency 후보를 실제 import·빌드에서 생성된 코드·조건부 플랫폼으로 다시 확인 |
| cargo-deny 0.20.2 | [deny.toml](../../.config/deny.toml)의 출처·라이선스·보안 정책 확인 |
| cargo-llvm-cov 0.9.1 | Rust의 실행 범위 계측. 내장 Clang의 C++ 범위는 별도 도구로 측정한다. |
| cargo-mutants 27.1.0 | [.cargo/mutants.toml](../../.cargo/mutants.toml)의 codec 변경에서 테스트가 실패·timeout을 검출하는지 확인 |
| Loom | `kodstar_loom` 구성에서 permit 반납·경합·대기 해제 모델 점검 |
| rstest·insta·proptest | 인자별 계약, 도움말 snapshot, codec 값 범위와 손상 입력을 확인 |

설치가 필요하면 도구를 `cargo install NAME --version VERSION --locked`로 준비한다.
실행별 빌드와 보고서는 작업 소유 경로를 사용한다. coverage의
`CARGO_LLVM_COV_TARGET_DIR`와 advisory DB의 `KODSTAR_ADVISORY_DB`도 검증용 경로로 지정할 수 있다.

```bash
cargo machete
cargo deny --config .config/deny.toml --locked check
cargo nextest run --workspace --locked
cargo test --workspace --doc --locked
cargo llvm-cov nextest --workspace --locked --summary-only
cargo mutants --output /path/to/owned-temp/mutants
```

```bash
RUSTFLAGS='--cfg=kodstar_loom' CARGO_TARGET_DIR=/path/to/owned-temp/loom-target \
  cargo test -p kodstar-core --lib --locked application::analysis::budget::model:: -- --test-threads=1
```

[insta.yaml](../../.config/insta.yaml)의 기본 검사는 승인된 snapshot을 읽는다. 도움말을 의도적으로 바꾸면
관련 snapshot의 예상 출력을 확인해 갱신한다. Loom은 Mutex·Condvar의 실행 순서를 검증하며,
취소 polling·신호·native FFI는 실제 실행 테스트에서 별도로 확인한다.

## 성능 비교

측정 전 도구·profile·target이 같은 두 바이너리와 고정 입력을 준비한다.
복사와 최초 실행 준비를 마친 뒤 고정한 입력에서 비교 명령을 실행한다. 별도 빌드와 입력 편집은 측정 전에 마친다.
같은 canonical 복제 경로에서 실행 순서를 교대하고, 임시 홈·이력·캐시로 각 실행을 격리한다.

| 스크립트 | 비교하는 것 |
|---|---|
| [analysis.py](../../scripts/benchmarks/analysis.py) | 초기화, 변경 없는 동기화, 기존 인덱스 조회, 선택한 편집. 분석 범위·논리 결과·이력과 peak RSS·시간을 비교한다. |
| [graph.py](../../scripts/benchmarks/graph.py) | 최단 경로, 넓은 in/out frontier, 제한 탐색, 전체 조회의 행·순서·잘림·방문 수와 비용을 비교한다. |
| [history.py](../../scripts/benchmarks/history.py) | 반복 저장·원문 복원·이력 크기와 내용 해시를 비교한다. |
| [engine.rs benchmark](../../crates/kodstar-core/benches/engine.rs) | Criterion으로 저장 그래프 조회·텍스트 diff·무변경 동기화를 반복 측정한다. |

```bash
python3 -B scripts/benchmarks/analysis.py --baseline /path/to/before --candidate target/dist/kodstar \
  --project /path/to/frozen-project --file src/api.rs --repeats 5 --output /path/to/analysis-report.json
python3 -B scripts/benchmarks/graph.py --baseline /path/to/before --candidate target/dist/kodstar \
  --independent-indexes --repeats 5 --output /path/to/graph-report.json
CARGO_TARGET_DIR=/path/to/owned-temp/bench-target CARGO_INCREMENTAL=0 \
  cargo bench -p kodstar-core --bench engine --locked -- --noplot
```

macOS 비교 스크립트의 RSS는 `/usr/bin/time -l`로 읽는다. 이력 decoder는 libzstd를 사용한다.
공유 라이브러리 자동 탐색이 어려운 환경에서는 `KODSTAR_ZSTD_LIBRARY`로 위치를 지정한다. 자식이 종료하고 pending WAL이 없는
소유 임시 DB를 immutable 모드로 읽는다. 결과 보고에는 환경·바이너리 SHA-256·개별 실행과 중앙값,
coverage·이력/캐시 크기와 가능한 경우 SQLite 세부 사용량을 기록한다.

분석 스크립트의 `--newline-edit`, `--body-replacement OLD NEW`, `--declaration-edit`, `--mutations`는
편집을 별도 복제에서 적용한다. 전체 재분석 비교가 있는 시나리오에서 증분 결과의 입력·관계·이력을 대조한다.
lookup 계약의 의도적인 변경은 해당 편집 시나리오와 `--lookup-contract-change`를 함께 지정해 검증한다.
native 환경 ID가 달라질 때는 `--native-engine-change`를 명시한다. 두 옵션은 바이너리 간 허용하는 비교 차이를
기록하며 같은 바이너리의 증분·전체 대조에는 전체 계약을 적용한다.

Criterion은 같은 임시 target에서 `--save-baseline before`로 저장하고 변경 후 `--baseline before`로 비교할 수 있다.
측정 결과로 개선을 설명할 때에는 처리 범위·누락·실행 환경과 시간·자원 사용량을 함께 적는다.
