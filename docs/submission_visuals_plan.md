# 대회 제출용 시각 자료 계획

> 결과보고서(`docs/result_report.md`) 제출용 근거 자료 목록. 초기의 "구상" 단계를 지나
> **실제 산출물 기준**으로 갱신했다. 각 항목에 파일 경로와 재생성 방법을 둔다.
> 표에 없는 추가 캡처는 `docs/assets/`, 도식 원본은 `docs/figures/`에 있다.

## 작성 기준(톤앤매너)

Xazz는 파이썬 생태계의 생산성을 존중하면서도, 대규모 데이터 처리 시 발생하는
런타임 타입 에러·GPU 자원 낭비·보안 검증 부재라는 구조적 한계를 극복하는 Rust 기반
차세대 AI 파이프라인 DSL이다. 모든 시각 자료는 이 스탠스를 반영하여
"파이썬과 대립이 아닌 하이퍼 퍼포먼스 모듈", "입문자도 즉시 작성 가능한
AI-Native 문법", "AI Agent 시대에 최적화된 파이프라인 플랫폼"을 전달해야 한다.

---

## A. 핵심 성능·정량 자료

### A1. 전처리 성능 벤치마크 비교 차트 — `docs/assets/benchmark_chart.png` ✅
- **구성**: 가로축 데이터셋 크기(228K/912K/4.09M 행), 세로축 처리 시간 비교 +
  speedup 막대. pandas / Polars / Xazz.
- **레이블**: README Performance 기준 — pandas 대비 **228K 1.39×, 912K 1.95×,
  4.09M 1.39×**, 최대 규모 peak RSS 570 MB vs 656 MB.
  (초기 초안의 "최대 3.84배"는 실제 측정과 달라 정정)
- **재생성**: `benches/`의 README 벤치마크 스크립트로 재측정 (측정 환경·데이터셋 캡션 필수).

### A2. 컴파일 시점 검증으로 GPU 낭비 차단 — `docs/figures/compile-time-safety[-kr].svg` ✅
- 정적 검사(`Option<T>`) 전/후의 실패 시점 비교: "런타임 오류 → 학습 중단 → GPU 재할당"
  반복 vs "컴파일 시점 즉시 차단".

### A3. 제로카피 메모리 레이아웃 도식 — `docs/figures/zero-copy[-kr].svg` ✅
- Polars DataFrame → Burn Tensor 변환에서 남는 복사 경계(f64→f32, columnar→row-major,
  host→device)를 명시. `docs/design/memory-model.md`와 연계.

---

## B. 기능 증빙 화면 (Visual IDE, 실제 서버 응답 캡처)

| 항목 | 파일 | 보여주는 것 |
|---|---|---|
| B1. 노드 그래프 전체 화면 | `docs/assets/ide_workspace.png`, `ide_monitor.png` | `import → 전처리 → DP → Burn 변환 → train/predict → chart` 노드 체인과 모니터 |
| B2. 가드레일 차단·보정 | `docs/assets/ide_policy_block.png` | 위반 코드 차단 + 결정적 자동 보정 diff + JSON 리포트 |
| B3. 감사 로그/체인 | `docs/assets/ide_audit_chain.png`, `ide_audit_tamper.png` | SHA-256 해시 체인 검증과 변조 레코드 위치 |

B2의 **sLM(Qwen2.5-Coder-1.5B) 자동 보정**은 선택 기능이며, QLoRA 파인튜닝 학습은
아직 실행되지 않았다(유형 1, Ollama 서빙). "파인튜닝된 모델이 제안한 코드" 캡처는
학습 완료 시점으로 보류하고, 현재는 **결정적 규칙 기반 보정**을 캡처한다.

---

## C. 도식 (한 장 요약)

| 항목 | 파일 | 내용 |
|---|---|---|
| C1. E2E 데이터 플로우 | `docs/figures/pipeline-flow[-kr].svg` | `.xzz` → 파서/AST → 타입·널 검증 → 가드레일 → Polars → DP → Burn → 학습/예측 → 감사 로그 |
| C2. 구조화 스택 | `docs/figures/workspace-stack[-kr].svg` | DSL → 컴파일러 → 실행(exec/runner/server) → UI, 하단 sLM/Ollama·Burn·Polars |

도식 원본(편집 가능)과 재생성 절차는 [`docs/figures/README.md`](figures/README.md) 참고.

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

## 현황과 남은 작업

- A1~A3, B1~B3, C1~C2 모두 실제 산출물 확보 (A1 벤치마크는 README Performance 수치로 정정).
- D(데모 영상·스크린샷)는 `demo.mjs`로 실제 서버에 대해 자동 생성.

남은 작업:
1. sLM QLoRA 실학습 완료 시 B2의 "파인튜닝 모델 보정" 캡처로 교체 (현재는 결정적 보정 캡처).
2. 결과보고서 서술(기대효과·혁신성)에 위 근거를 인용하고, 표지 정보를 채운 뒤 제출.

**제출 임팩트 최대**: A1(벤치마크)과 B2(가드레일→보정). 두 항목이 성능과 보안
차별점을 한눈에 전달한다.