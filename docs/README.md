# KodStar 문서

코드를 처음 읽는다면 [아키텍처의 소스 폴더 지도](architecture.md#소스-폴더-지도)부터 본다.
기본 조회가 파일 변경을 발견한 뒤 결과를 출력하기까지의 흐름은 [명령 실행](architecture/execution.md)에 있다.

## 문서 폴더 구조

```text
kodstar/
├── README.md                         설치와 첫 CLI 조회
├── kodstar-spec.md                   제품 계약과 명세 목차
├── AGENTS.md                         저장소 개발 원칙
└── docs/
    ├── README.md                     문서 지도
    ├── architecture.md               레이어, 소스 지도, 공통 실행 흐름
    ├── architecture/
    │   ├── execution.md              CLI, 공통 실행기, watch, stdio
    │   ├── inputs.md                 파일 분류, 구문 추출, 혼합 입력
    │   ├── native.md                 Clang 빌드, FFI, 체크포인트 VFS
    │   ├── resolution.md             이름 해석, 관계 공개, 증분 무효화
    │   ├── queries.md                검색, 탐색, 개요, 영향 조회
    │   ├── storage.md                SQLite 인덱스, 캐시, 잠금과 예산
    │   └── history.md                소스 수집, 비교, 복원과 복구
    ├── reference/
    │   ├── cli.md                    명령과 설정
    │   ├── analysis.md               언어, 관계, 한계
    │   ├── output.md                 결과 형식, Rust API, stdio 계약
    │   └── storage.md                동기화, 이력, 디스크 상태와 수명
    ├── development.md                도구 준비, 빌드, 기본 검사
    ├── development/
    │   ├── verification.md           변경별 테스트, 품질 도구, 벤치마크
    │   └── dependencies.md           외부 소스, 파서 생성, 라이선스
    └── assets/                       README 로고와 제작 자료
```

## 읽는 순서

| 하려는 일 | 읽을 곳 |
|---|---|
| 설치하고 소스에서 관계를 찾기 | [빠른 시작](../README.md#quick-start) → [CLI 명령과 설정](reference/cli.md) |
| 명령이나 결과 타입 수정하기 | [공통 실행과 출력](architecture/execution.md) → [조회 코드](architecture/queries.md) |
| 언어 또는 프레임워크 해석 고치기 | [입력과 구문 추출](architecture/inputs.md) → [이름 해석과 증분 갱신](architecture/resolution.md) |
| C/C++ 근거 또는 빌드 바꾸기 | [내장 Clang](architecture/native.md) → [의존성과 생성 경로](development/dependencies.md) |
| 과거 분석·소스 복구 추적하기 | [이력 구현](architecture/history.md) → [인덱스와 캐시](architecture/storage.md) |
| 구조화된 출력을 읽는 도구 작성하기 | [출력과 도구 연동](reference/output.md) |
| 변경을 검증하고 비교하기 | [개발 환경](development.md) → [변경별 검증](development/verification.md) |

사용자가 관찰하는 동작은 [기능 명세](../kodstar-spec.md)의 장에서, 모듈 책임과 실행 순서는
[아키텍처](architecture.md)의 장에서 확인한다. 실행 가능한 검증 명령은 [개발 문서](development.md)에 모인다.
