# 2026년 오픈소스 개발자대회 결과보고서

## 표지 정보

| 항 목 | 내 용 | 항 목 | 내 용 |
|---|---|---|---|
| 팀 명 | [참가 접수 정보와 동일하게 기재] | 팀 인원 (팀장 포함) | [N명] |
| 참가부문 | [학생/일반] | 과제유형 | [자유과제/지정과제(기업명)] |

---

## 결과보고서

### 프로젝트 개요

| 항 목 | 내 용 |
|---|---|
| 프로젝트명 | **Xazz** |
| 프로젝트 등록 URL | https://github.com/xazzdev/Xazz |
| 시연영상 | [유튜브 업로드 후 URL 기재] |
| 프로젝트 소개 | 겉으로는 스크립트, 핵심은 컴파일러. Polars 초고속 데이터 전처리와 Burn 딥러닝 컴파일러, 정적 보안 가드레일을 단일 .xzz DSL로 통합하여 데이터 전처리부터 딥러닝 학습까지 엔드투엔드로 제어하는 Rust 기반 AI 파이프라인 개발 플랫폼 |

### 프로젝트 세부 내용

**개발배경 및 목적**

Python 기반 AI 파이프라인은 런타임 타입 오류와 NaN(결측치) 처리 문제, 그리고 Python↔C/C++/Rust 간 언어 경계에서 발생하는 메모리 복사 오버헤드가 반복적으로 발생한다. 이는 대규모 분산 학습 과정에서 GPU 자원 낭비와 AI 개발 생산성 저하로 이어진다.

Xazz는 Rust의 강타입 시스템으로 결측치와 타입 오류를 컴파일 단계에서 원천 검증하고, Apache Arrow 기반 제로카피 텐서 변환으로 언어 간 오버헤드를 제거하는 것을 목표로 개발되었다. 나아가 개인정보 유출 위험을 사전에 차단하는 보안 엔진과 차등 프라이버시를 통합하여, 금융·의료 등 민감 데이터 산업에서도 안심하고 AI 학습을 수행할 수 있는 오픈소스 플랫폼을 완성하는 데 목적을 둔다.

**개발환경**

- 언어: Rust 2024 Edition (stable toolchain), TypeScript/React, Python (sLM 파인튜닝)
- DL 프레임워크: Burn v0.21 (CPU 백엔드 ndarray, autodiff 지원)
- 데이터 엔진: Polars v0.53 (lazy / csv / strings / regex)
- 백엔드 서버: Axum 0.8 (REST API), Tokio, SHA-256 감사 로그
- 프론트엔드: React 18, @xyflow/react 12 (노드 기반 Visual IDE), Vite
- 보안/프라이버시: Policy-as-Code 정적 가드레일, DP 노이즈(Laplace/Gaussian Mechanism)
- sLM 서빙: Qwen2.5-Coder-1.5B, Unsloth + QLoRA 파인튜닝, GGUF 변환, llama.cpp / Ollama
- CI/CD: GitHub Actions, Playwright E2E 테스트, Cargo test / clippy
- 워크스페이스: xazz, xazz-core, xazz-compiler, xazz-exec, xazz-runner, xazz-server

**시스템 구성 및 아키텍처**

- **xazz (CLI)**: `.xzz` 스크립트의 파싱·컴파일·실행 명령 제공 (`run`, `import`, `emit rust`, `check`, `sde`, `new`). Polars/Tokio 비의존 (경량 바이너리)
- **xazz-compiler**: Lexer → Parser → AST → 정적 타입 검사 → Rust Emitter 파이프라인
- **xazz-exec**: Polars LazyFrame 전처리 및 Burn 텐서 딥러닝 실행 엔진 (무거운 의존성 격리)
- **xazz-runner**: CLI가 서브프로세스로 스폰하는 IPC 브리지 (데이터 흐름 추적 기반 샌드박스)
- **xazz-server**: REST API 서버 — 파이프라인 실행, CSV 스키마 추론, SHA-256 감사 로그·검증
- **visual-ide**: React + @xyflow/react 기반 노드형 Visual IDE (전처리·딥러닝 컴파일 흐름 시각화)

핵심 데이터 흐름: `.xzz` 스크립트 → 파서/AST → 정적 타입·널 안전성 검증 → 보안 가드레일 검사 → (위반 시 sLM 자동 보정 + 위반 사유 리포트) → Polars LazyFrame 연산 그래프 → DP 노이즈 주입 → Burn 텐서 변환 → 학습/예측 → 결과 (CSV/차트 HTML/예측) → SHA-256 감사 로그 기록

**프로젝트 주요기능**

- **[컴파일러 코어]** 독자적인 AI 스크립트 언어 파서(Parser)와 내부 구문 트리(AST) 툴체인 직접 구현
- **[데이터 가속 엔진]** Polars 엔진 융합 — 전처리 명령을 초고속 LazyFrame 연산 그래프로 변환·실행 (pandas 대비 최대 3.84배)
- **[딥러닝 컴파일]** Burn 프레임워크 연동 — 제로카피 텐서 연산 및 학습 레이어로 변환하는 컴파일 계층 구현 (`model {}` 선언 + `train()`, Adam+MSE, 체크포인트)
- **[데이터 변환 인터페이스]** Polars LazyFrame 연산 결과를 Burn 텐서(Tensor) 계층으로 전환하는 제로카피 변환 인터페이스와 파이프라인 데이터 흐름용 schema/type 표준화 구현
- **[정적 가드레일]** Policy-as-Code 기반 정적 규칙 작성 — 실행 전 단계에서 개인정보 유출·보안 컴플라이언스 위반 코드를 실시간 탐지·차단 (Definition of Done: 위반 코드 실시간 차단)
- **[프라이버시 R&D]** Rust/Python 환경에서 Laplace / Gaussian Mechanism 기반 차등 프라이버시(DP) 노이즈 주입 알고리즘 구현 — 지정된 Privacy Budget 하에서 Polars DataFrame 연산 결과에 노이즈 적용, Privacy Budget 소모 상태 모니터링
- **[내장형 sLM 엔진]** Qwen2.5-Coder-1.5B를 Unsloth + QLoRA로 보안 위반 코드 보정에 특화 파인튜닝 후 GGUF 변환, llama.cpp/Ollama 기반 온프레미스 서빙 환경 구축. 정적 가드레일에 차단된 코드를 지능적으로 자동 보정하고, 수정된 안전한 코드와 위반 사유·분석 리포트를 JSON 포맷의 API로 반환하는 지능형 AI 어시스턴트 완성
- **[비주얼 콘솔 UI]** React·@xyflow/react 기반으로 데이터 전처리·딥러닝 컴파일 파이프라인 흐름을 시각화하는 웹 IDE
- **[신뢰성 인프라]** 모든 연산 이력을 영구 보존하는 SHA-256 감사 로그 시스템과 GitHub Actions 기반 CI/CD·자동화 테스트 환경

**구동 및 시연**

```bash
cargo build --release -p xazz
cargo build --release -p xazz-runner

# 두 바이너리를 같은 디렉토리에 배치한 뒤
./xazz new my-project     # 프로젝트 + 샘플 CSV 생성
cd my-project
./xazz import data.csv    # 스키마 자동 추론 → main.xzz에 타입 블록 기록
./xazz run main.xzz       # 컴파일 + 파이프라인 실행 (chart {} → HTML 차트 렌더링)
./xazz emit rust main.xzz # .xzz → Rust 소스 변환 (Polars LazyFrame + Burn)
```

시연 예시(공기질 데이터): `type AirData` 스키마 선언 → `fillNull("pm10", strategy:"mean")` 전처리 → DP 노이즈 주입(Privacy Budget 지정) → Polars→Burn 텐서 변환 → `AirPredictor` 모델 선언 → `train()`/`predict()` → `chart {}`로 결과 시각화.

보안 가드레일 시연: 개인정보(주민등록번호·전화번호 등) 노출 규칙을 포함한 `.xzz` 코드 실행 시, 실행 전 단계에서 정적 가드레일이 즉시 차단 → sLM(파인튜닝된 Qwen2.5-Coder-1.5B)이 안전한 코드로 자동 보정하고, 위반 사유·분석 리포트를 JSON으로 반환. Visual IDE에서는 이 전 과정이 노드 그래프로 표시된다.

### 기대효과 및 활용분야

**향후 확장성 및 기대효과**

- **극대화된 연산 효율성**: Apache Arrow 기반 메모리 레이아웃과 Rust 런타임으로 기존 Python 환경 대비 파이프라인 성능·자원 효율 향상 (벤치마크 3.84배)
- **GPU 자원 낭비 원천 차단**: 컴파일 단계 정적 타입·결측치 검사로 오류를 학습 이전에 발견, 대규모 분산 학습의 불필요한 GPU 소모 방지
- **엔터프라이즈 신뢰성**: 실행 전 정적 가드레일·차등 프라이버시 노이즈·SHA-256 감사 로그로 금융·의료 등 민감 데이터 산업 적용 가능
- **지능형 보안 자동화**: 파인튜닝된 온프레미스 sLM이 차단된 보안 위반 코드를 자동 보정하고 위반 사유 리포트를 제공해 보안 준수의 개발자 부담을 경감
- **생태계 확대**: 선언형 DSL로 개발 진입 장벽을 낮추고, 글로벌 CI/CD와 기여 가이드라인을 기반으로 대한민국 주도 데이터 엔지니어링 오픈소스 생태계 활성화에 기여
- **확장 가능성**: GPU 백엔드(burn-tch / burn-wgpu) 전환, 분산 학습, 실시간 스트리밍 파이프라인, 더 풍부한 연산자·조인·스키마 진화

### 기타

**프로젝트의 혁신성 및 차별성**

- 단순 라이브러리 래퍼가 아닌, 파서·AST·타입 검사·코드 생성·보안 런타임·딥러닝 컴파일 엔진까지 핵심 툴체인을 바닥부터 직접 설계·구현한 독립 컴파일러 플랫폼
- 스크립트 언어의 생산성과 Rust 강타입 컴파일러의 안전성을 단일 DSL로 통합한 "겉으로는 스크립트, 핵심은 컴파일러" 설계
- 제로카피 텐서 변환과 정적 널 안전성(`Option<T>`)으로 Python 런타임 오류·오버헤드의 근본 문제를 구조적으로 해결
- 데이터 전처리부터 딥러닝 학습까지 End-to-End로 잇는 파이프라인 DSL + 보안 가드레일 + 감사 로그 결합

**한계점 및 향후 발전 로드맵**

| Phase | 목표 | 상태 |
|---|---|---|
| Phase 1 | DSL 문법·타입 시스템·컴파일러 파이프라인 | 완료 |
| Phase 2 | Polars 연동·CLI 도구·차트 출력 | 완료 |
| Phase 3 | Visual IDE·그래픽 파이프라인 편집기 | 완료 |
| Phase 4 | 연산자 확장·join 개선·스키마 진화 | 진행 중 |
| Phase 5 | Burn 딥러닝 계층(모델·학습·체크포인트), NQP | 딥러닝 완료 / NQP Experimental |
| Phase 6 | DP 노이즈 주입 모듈 + Polars→Burn 데이터 변환 인터페이스 | 완료 |
| Phase 7 | 정적 가드레일 + 파인튜닝 sLM 자동 보정 모듈 (GGUF/Ollama 서빙) | 완료 |

향후 계획: Phase 4(연산자·조인·스키마 진화) 및 Phase 5(NQP 쿼리 플래너 고도화) 완성, GPU 백엔드 및 분산 학습 지원, sLM 파인튜닝 데이터·보정 정확도 고도화 및 다양한 언어 모델 확장, 커뮤니티 기여·유지보수 체계 지속 강화.

**소감 및 후기**

[팀워크, 기술적 한계 극복 사례 등 프로젝트 개발을 통해 느낀점 기재]

---

## 붙임1 — SBOM(소프트웨어 자재명세서)

| 번호 | 라이브러리명 | 버전 | 라이선스 | 공식 저장소 URL | 사용 목적 및 주요 기능 |
|---|---|---|---|---|---|
| 1 | polars | 0.53 | MIT | https://github.com/pola-rs/polars | 데이터 전처리 LazyFrame 연산 그래프 엔진 |
| 2 | burn | 0.21 | MIT | https://github.com/tracel-ai/burn | 딥러닝 모델 학습·추론 컴파일 엔진 |
| 3 | burn-ndarray | 0.21 | MIT | https://github.com/tracel-ai/burn | CPU 백엔드 텐서 연산 (autodiff) |
| 4 | clap | 4.4 | MIT/Apache-2.0 | https://github.com/clap-rs/clap | CLI 인자 파싱 |
| 5 | serde / serde_json | 1.0 | MIT/Apache-2.0 | https://github.com/serde-rs/serde | 직렬화·JSON 파싱 |
| 6 | csv | 1.3 | MIT/Apache-2.0 | https://github.com/BurntSushi/rust-csv | CSV 파싱·스키마 추론 |
| 7 | encoding_rs | 0.8 | MIT/Apache-2.0 | https://github.com/hsivonen/encoding_rs | EUC-KR(CP949) 한글 CSV 디코딩 |
| 8 | anyhow | 1.0 | MIT/Apache-2.0 | https://github.com/dtolnay/anyhow | 오류 처리 |
| 9 | indicatif | 0.17 | MIT | https://github.com/console-rs/indicatif | CLI 진행률 표시 |
| 10 | colored | 2.1 | MPL-2.0 | https://github.com/mackwic/colored | CLI 색상 출력 |
| 11 | axum | 0.8 | MIT | https://github.com/tokio-rs/axum | REST API 서버 프레임워크 |
| 12 | tokio | 1 | MIT | https://github.com/tokio-rs/tokio | 비동기 런타임 |
| 13 | tower-http | 0.6 | MIT | https://github.com/tower-rs/tower-http | CORS 미들웨어 |
| 14 | uuid | 1 | Apache-2.0/MIT | https://github.com/uuid-rs/uuid | 감사 로그 UUID |
| 15 | sha2 | 0.10 | MIT/Apache-2.0 | https://github.com/RustCrypto/hashes | SHA-256 감사 로그 해시 |
| 16 | chrono | 0.4 | MIT/Apache-2.0 | https://github.com/chronotope/chrono | 감사 로그 타임스탬프 |
| 17 | tempfile | 3 | MIT/Apache-2.0 | https://github.com/Stebalien/tempfile | 임시 파일 처리 |
| 18 | react | 18.3.1 | MIT | https://github.com/facebook/react | 프론트엔드 UI 렌더링 |
| 19 | react-dom | 18.3.1 | MIT | https://github.com/facebook/react | DOM 렌더링 |
| 20 | @xyflow/react | 12.10.2 | MIT | https://github.com/xyflow/xyflow | 노드 기반 Visual IDE 플로우 |
| 21 | lucide-react | 1.16.0 | ISC | https://github.com/lucide-icons/lucide | 아이콘 |
| 22 | vite | 7.3.6 | MIT | https://github.com/vitejs/vite | 프론트엔드 빌드 도구 |
| 23 | @playwright/test | 1.55.1 | Apache-2.0 | https://github.com/microsoft/playwright | E2E·대비 테스트 |
| 24 | Qwen2.5-Coder-1.5B | 1.5B | Apache-2.0 | https://github.com/QwenLM/Qwen2.5-Coder | 보안 위반 코드 자동 보정 sLM (파인튜닝 기반) |
| 25 | Unsloth | - | Apache-2.0 | https://github.com/unslothai/unsloth | sLM 파인튜닝 최적화 (QLoRA) |
| 26 | llama.cpp | - | MIT | https://github.com/ggml-org/llama.cpp | GGUF 모델 추론·서빙 |
| 27 | Ollama | - | MIT | https://github.com/ollama/ollama | 온프레미스 sLM 모델 서빙/실행 |

---

## 붙임2 — AI 모델 활용 및 라이선스 기술 명세서

### 1. AI 모델 활용 유형 (해당하는 항목에 ▣ 표시)

- □ 유형 1: 외부 모델 그대로 활용
- ▣ 유형 2: 외부 모델 파인튜닝 (기존 공개 모델 Qwen2.5-Coder-1.5B를 가져와 보안 위반 코드 보정용 데이터셋으로 QLoRA 미세조정)
- □ 유형 3: 자체 개발 모델

※ 개발 과정에서 코딩·디버깅 보조용으로 상용 AI를 단순 활용한 경우는 유형에 체크하지 않음 (4번 항목에 기재)

### 2. 기반(베이스) 모델 정보 (유형 1, 2 작성 필수 / 유형 3은 '해당 없음')

| 항 목 | 내 용 |
|---|---|
| 기반 모델명 및 개발사 | Qwen2.5-Coder-1.5B (Alibaba Qwen team) |
| 기반 모델 라이선스 | Apache 2.0 |

### 3. 데이터셋 정보 및 가중치 배포 명세 (유형 2, 3 작성 필수)

| 항 목 | 내 용 |
|---|---|
| 학습 데이터셋 정보 | 정적 가드레일로 탐지되는 개인정보 노출·보안 위반 코드 쌍을 수집·합성하여 구성한 보안 위반→안전 코드 대조 데이터셋 (보정 특화) |
| 데이터 정제/가공 방법 요약 | 개인정보 비식별화(마스킹) 조치, 오픈소스 출품을 위한 프롬프트 포맷 변환 및 필터링, 보안 위반 코드와 안전한 보정 코드의 instruction/response 구조로 정제 |
| 새로 생성된 가중치 공개 저장소 URL | [Hugging Face 모델 리포지토리 URL 기재 — 승인 절차 없이 누구나 접근 가능한 공개 주소] |
| 가중치 파일 정보 및 배포방식 | LoRA 어댑터(QLoRA) 형태 배포 후 GGUF 양자화 변환, llama.cpp/Ollama로 온프레미스 서빙 |

### 4. 소스코드 라이선스 및 개발 환경 정보 (모든 유형 필수 작성)

| 항 목 | 내 용 |
|---|---|
| 직접 작성한 코드의 오픈소스 라이선스 | Apache License 2.0 |
| 학습/추론 소스코드 공개 저장소 URL | https://github.com/xazzdev/Xazz |
| 상용 AI 보조도구 활용 여부 및 범위 | 코드 작성 및 디버깅 보조용으로 상용 AI(예: ChatGPT/Claude)를 활용하였으며, 전체 코드의 일부 수준으로 보조적 활용만 함 |

---

※ 붙임2 작성 안내: 본 프로젝트는 외부 공개 모델(Qwen2.5-Coder-1.5B, Apache-2.0)을 보안 위반 코드 보정에 특화하여 QLoRA 방식으로 파인튜닝한 **유형 2**에 해당합니다. 새로 생성된 가중치의 공개 저장소 URL은 모델 배포 전 확정하여 기재해 주시기 바랍니다.