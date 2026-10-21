# KodStar

Rust로 작성한 로컬 소스코드 관계 분석 엔진과 CLI입니다. 공식 Tree-sitter로 구문 사실을 추출하고,
SQLite에 저장한 관계를 조회합니다.

실행 지원 대상은 **macOS와 Linux**입니다. **Windows는 지원하지 않습니다.**

## 시작

[rust-toolchain.toml](rust-toolchain.toml)의 toolchain과 네이티브 C 빌드 도구를 준비한 뒤
저장소 루트에서 실행합니다. toolchain 선택 방법은 [개발 환경](docs/development.md#개발-환경)을 참고합니다.

```bash
cargo build --workspace --locked
./target/debug/kodstar build /work/project
./target/debug/kodstar symbols /work/project/src/api.rs --root /work/project
./target/debug/kodstar deps /work/project/src/api.rs caller --root /work/project --format json
./target/debug/kodstar explain /work/project/src/api.rs caller --root /work/project
```

프로젝트 안에서는 등록된 상위 루트를 선택하므로 상대 파일 경로를 사용할 수 있습니다.
조회와 `watch`를 사용하기 전에 `build`로 초기 인덱스를 만듭니다.

## 구현 범위

| 기능 | 명령 |
|---|---|
| 분석·갱신 | `build` |
| 객체·관계 조회 | `symbols`, `list`, `related`, `deps`, `refs`, `explain`, `impact`, `path`, `search` |
| 상태·진단·캐시 | `status`, `doctor`, `languages`, `cache list/clean/gc` |
| 연동 | `watch`, `serve --stdio`, VS Code adapter |

19개 언어와 Vue·Svelte·Astro·EJS·Blade·Razor의 선정된 정적 규칙을 구현합니다.
**명세와 Graphify의 전체 언어·framework 규칙 포팅은 진행 중입니다.**
언어별 기능과 제한은 `languages --format json`, 포팅 근거와 미완료 범위는
[포팅 검증](docs/development.md#포팅-검증)에서 확인합니다.

첫 build 이후 일반 명령은 변경을 확인하고 증분 동기화합니다.
후보·모호함·미해결과 최신성은 결과에 표시합니다.
명령 옵션·출력·캐시 정책은 [기능 명세](kodstar-spec.md)에 정의합니다.

## 문서와 저장소

| 문서 | 내용 |
|---|---|
| [기능 명세](kodstar-spec.md) | 제품 범위·명령·출력·최신성·stdio 계약 |
| [아키텍처](docs/architecture.md) | 레이어·주요 소스 구성·실행 흐름·상태와 자원 소유권 |
| [개발 문서](docs/development.md) | 환경·빌드·설정 예제·검증·의존성·포팅 근거 |
| [개발 원칙](AGENTS.md) | 설계 검토·코드·주석·변경 절차 |

엔진은 `crates/kodstar-core`, CLI는 `crates/kodstar-cli`, 에디터는 `editors/vscode`에 있습니다.
언어별 규칙은 레이어 안의 언어 모듈에, 혼합 입력 처리는 포맷별 모듈에 둡니다.

고정 의존성과 grammar 출처는 [의존성 목록](LICENSES/dependencies.json),
라이선스 원문과 고지는 [LICENSES](LICENSES)에 있습니다.
