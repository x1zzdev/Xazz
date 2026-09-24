# 대회 제출용 시각적 자료 구상 계획

> 본 문서는 오픈소스 개발자대회 결과보고서(`docs/result_report.md`) 제출용으로,
> 아직 개발이 완료되지 않은 두 이슈(#2 정적 가드레일·sLM, #3 DP·바인딩) 완성 후
> 생성할 시각적 자료(스크린샷·벤치마크·도표)의 구체적 산출물 스펙을 정의한다.

## 작성 기준(톤앤매너)

Xazz는 파이썬 생태계의 생산성을 존중하면서도, 대규모 데이터 처리 시 발생하는
런타임 타입 에러·GPU 자원 낭비·보안 검증 부재라는 구조적 한계를 극복하는 Rust 기반
차세대 AI 파이프라인 DSL이다. 모든 시각 자료는 이 스탠스를 반영하여
"파이썬과 대립이 아닌 하이퍼 퍼포먼스 모듈", "입문자도 즉시 작성 가능한
AI-Native 문법", "AI Agent 시대에 최적화된 파이프라인 플랫폼"을 전달해야 한다.

---

## A. 핵심 성능·정량 자료 (대회 정량 근거)

### A1. 전처리 성능 벤치마크 비교 차트 [최우선]
- **구성**: 가로축 데이터셋 크기(1K~10M행), 세로축 처리 시간(로그 스케일).
  pandas / Polars / Xazz 3개 라인.
- **레이블**: "Xazz vs pandas 최대 3.84배" + 측정 시점·환경 명시.
- **추천 툴**: matplotlib / Plotly. Xazz 라인은 강조색.
- **제출 활용**: 프로젝트 개요 '연산 효율성' 근거. 캡션에 환경명·데이터셋 필수.

### A2. GPU 자원 낭비 방지 비교 (선택)
- 컴파일 단계 정적 검사(`Option<T>`) 전/후의 실패 발생 시점을 시간축으로 비교.
- **구성**: "런타임 오류 → 학습 중단 → GPU 재배정" 반복 사이클 vs
  "컴파일 시점 즉시 차단" 단일 사이클.
- **레이블**: "컴파일 시점 검증으로 대규모 분산 학습 GPU 사이클 낭비 차단".

### A3. 제로카피 메모리 레이아웃 도식
- Polars DataFrame → Burn Tensor 변환 시 메모리 복사 여부 비교.
- **구성**: 메모리 블록을 화살표로 연결하되 "copy(복사)" vs "공유(Arrow zero-copy)" 대비.
- **레이블**: "언어 간 메모리 복사 오버헤드 완전 제거".

---

## B. 제출용 기능 증빙 화면 (구현 완료 시점 캡처)

### B1. Visual IDE 노드 그래프 전체 화면
- **구성**: `import → 전처리 → DP 노이즈 → Polars→Burn 변환 → train → predict → chart` 노드 체인.
- **캡션**: "노드 그래프로 보는 데이터 전처리→딥러닝 컴파일 흐름".
- **특별 포인트**: DP 노드에 Privacy Budget 사용률 게이지가 보이면 보안 R&D 강조.

### B2. sLM 보정 결과 화면 (이슈 #2 완료 증거) [최우선]
- 보안 위반 코드가 정적 가드레일에 차단된 뒤, **파인튜닝된 Qwen2.5-Coder-1.5B가
  제안한 안전 코드 + JSON 리포트** 동시 캡처.
- **구성**: 좌 = 위반 원본 코드 / 우 = 보정 코드 diff + 하단 JSON 응답(위반사유·수정이유).
- **캡션**: "정적 가드레일 → sLM 자동 보정 → JSON 리포트 전송".

### B3. SHA-256 감사 로그 테이블
- 감사 로그 화면(타임스탬프·UUID·해시·연산) 캡처. 신뢰성·compliance 어필.

---

## C. 공로 차트 (1장으로 요약)

### C1. E2E 파이프라인 데이터 플로우 다이어그램
- `.xzz` → 파서/AST → 타입·널 검증 → 가드레일 검사 → (위반 시 sLM 보정) →
  Polars 그래프 → DP 노이즈 → Burn Tensor → 학습/예측 → 결과 → 감사 로그.
- **핵심**: 이 다이어그램 하나가 이슈#2(가드레일) + #3(DP·바인딩)을 모두 연결하는 완결 흐름.

### C2. 구조화 스택 다이어그램
- 3 Layer: DSL/문법 → 컴파일러(xazz-compiler) → 실행(exec/runner/server) → UI(Visual IDE).
  하단에 sLM/Ollama, Burn, Polars 배치.

---

## D. Visual IDE 데모 자료 (2차 평가, #154)

모두 실제 xazz-server(로컬 릴리스 빌드)에 대해 `visual-ide/scripts/demo.mjs`가 자동으로 만든다.
대본은 `docs/demo/demo-scenario.md`, 슬라이드 뼈대는 `docs/demo/presentation-outline.md`.

| 자료 | 파일 | 보여주는 것 | 관련 이슈 |
|---|---|---|---|
| 데모 영상 (한국어 자막, 영어본은 스크립트로 생성) | `docs/assets/demo/xazz-demo-ko.webm` | 5분 시나리오의 자동 재현본 | #154 |
| 실행 진행 | `docs/assets/ide_run_progress.png` | 브라우저 측정 경과 시간·요청 단계, 에폭 진행률 "제공되지 않음" | #113 |
| 런 히스토리 | `docs/assets/ide_run_history.png` | 서버에 남은 실행 목록과 다시 연 영수증 | #107 |
| 감사 체인 / 변조 | `docs/assets/ide_audit_chain.png`, `ide_audit_tamper.png` | 체인 검증과 변조 레코드 위치 표시 (B3 대체) | #108 |
| 정책 팩 | `docs/assets/ide_policy_packs.png` | 테넌트 팩 설치와 변경 이력 | #109 |
| 정책 차단 | `docs/assets/ide_policy_block.png` | 식별자 노출 차단과 보정 diff (B2의 결정적 보정) | #109 |
| DP 원장 | `docs/assets/ide_dp_ledger.png` | 테넌트 ε 원장 | #110 |
| 스키마 추론 | `docs/assets/ide_schema_inference.png` | CSV 업로드 → 서버 스키마 추론 | #114 |
| 컬럼 계보 | `docs/assets/ide_lineage.png` | 집계 컬럼의 원본 추적 | #116 |
| 한국어 화면 | `docs/assets/ide_governance_ko.png` | 거버넌스 패널 한국어 | #115 |

다시 만들기: 저장소 루트에서 `./target/release/xazz-server`를 띄운 뒤
`cd visual-ide && node scripts/demo.mjs --tamper ../audit_log/audit.jsonl`.

---

## 권장 마무리 순서

1. #3(DP+바인딩) 완료 → **C1 다이어그램 + A3 메모리 도식** 확정
2. #2(가드레일+sLM) 완료 → **B2 보정 화면 + B3 감사로그** 캡처
3. 병렬 확보: **A1~A2 벤치마크**, B1 Visual IDE 전체화면
4. 최종 리포트 서술(기대효과·혁신성 섹션)에 위 근거 인용

**제출 임팩트 최대**: A1(벤치마크)과 B2(가드레일→sLM 자동 보정). 두 항목이
두 이슈(#2·#3)의 완료 증거이자 심사자의 직관적 이해를 유도한다.