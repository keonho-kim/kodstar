# 의존성과 생성 소스

[개발 환경](../development.md) · [native 리소스](../architecture/native.md) · [저장소 개발 원칙](../../AGENTS.md)

## 변경의 기록 위치

| 항목 | 소유 파일 |
|---|---|
| 직접 사용하는 의존성과 Cargo 기능 | [Cargo.toml](../../Cargo.toml)과 각 crate manifest |
| 해석된 버전, Git revision, registry checksum | [Cargo.lock](../../Cargo.lock) |
| 의존성 출처·검토·grammar revision·ABI | [LICENSES/dependencies.json](../../LICENSES/dependencies.json) |
| 배포 고지와 개발 전용 도구 고지 | [third-party-notices.txt](../../LICENSES/third-party-notices.txt), [development-notices.txt](../../LICENSES/development-notices.txt) |
| native compiler와 내장 헤더 출처 | [kodstar-clang/native/SOURCE.json](../../crates/kodstar-clang/native/SOURCE.json) |
| 원본 Tree-sitter grammar와 로컬 보완 | [vendor/](../../vendor)의 각 `SOURCE.json`과 `patch.json` |

의존성을 바꾸는 단위는 manifest와 lockfile, 역할·라이선스·바이너리 비용, 영향을 받는 native 입력과
parser ABI, 재현 가능한 checksum을 포함한다. [deny.toml](../../.config/deny.toml)의 허용 목록은 외부
의존성 출처와 라이선스를 검사한다. `publish = false`인 workspace crate에는 도구의 private-package 정책이 적용된다.

## 어디에서 어떤 라이브러리를 사용하는가

| 영역 | 사용 목적 |
|---|---|
| CLI·역직렬화 | clap은 인자와 도움말, serde/serde_json은 공개 결과·stdio와 구성 값의 형식 처리 |
| 입력과 watcher | ignore·globset은 프로젝트 파일 선택, notify는 변경 신호, config는 TOML 설정 병합 |
| 분석·저장 | Tree-sitter는 구문, rusqlite와 내장 SQLite는 물리 DB와 연결 한도, BLAKE3는 내용 주소와 fingerprint |
| 이력과 비교 | FastCDC·Zstd는 청크와 저장 표현, similar는 제한된 변경 창의 줄 정렬 |
| 탐색 | petgraph의 BFS와 강연결 요소, Roaring의 방문 집합 |
| 원문 처리 | memchr·simdutf8는 원문 탐색과 UTF-8 검사, encoding_rs는 명시 인코딩 변환, roxmltree는 원문 위치를 보존하는 XML 문서 |
| 제한된 재사용 | smallvec·smol_str은 짧은 배열·이름, string-interner는 파일 단위 native 문자 사전 |
| 관측 | tracing과 tracing-subscriber는 CLI 단계 계측과 stdout에서 분리된 stderr 진행 |

실행과 저장에서 실제 사용하는 기능은 각 manifest의 features와 호출 경로를 확인한다.
`--licenses`는 CLI의 [build.rs](../../crates/kodstar-cli/build.rs)가 압축한 고지, Clang build가 수집한
헤더 고지를 출력한다. 원본 고지는 [LICENSES](../../LICENSES)에 보관한다.

## 파서 원본 확인과 재생성

일반 빌드는 Cargo의 고정 버전과 `vendor/`에 있는 parser를 사용한다. 소스 변경이 없는 상태의 검사는
path dependency·registry checksum 및 기록한 grammar 생성 입력을 대조한다.

```bash
cargo metadata --locked --format-version 1
python3 -B scripts/grammars/generate.py --check
```

[scripts/grammars/generate.py](../../scripts/grammars/generate.py)는 한 grammar의 보존 원본과 패치,
고정 생성기와 결과의 hash를 다룬다. grammar 동작을 변경할 때 원본·패치 변경을 먼저 기록하고
생성 C·JSON을 해당 절차로 갱신한다. 예를 들어 Kotlin 보완 grammar는 고정 Tree-sitter CLI 0.26.10으로
재생성할 수 있다.

```bash
cargo install tree-sitter-cli --version 0.26.10 --locked --root /path/to/owned-temp/tree-sitter-cli
python3 -B scripts/grammars/generate.py --grammar tree-sitter-kotlin-ng \
  --tree-sitter /path/to/owned-temp/tree-sitter-cli/bin/tree-sitter
python3 -B scripts/grammars/generate.py --check
```

C++ 생성은 C grammar를 참조한다. C 쪽 입력이 바뀌면 C를 먼저 생성하고 C++의 `SOURCE.json` 입력 해시를
맞춘 뒤 C++를 생성한다. 마무리로 원본·ABI·라이선스 기록, parser 회귀 테스트와 실제 corpus의 범위를 확인한다.
생성기·다운로드·컴파일 부산물은 작업 소유 디렉터리에서 회수한다.
