# Xazz 핵심 사용자 여정

- 기준: `../spec/SPEC.yaml` v0.1.1 승인본, `../spec/PRODUCT-UX-BRIEF.md`,
  `../design-system/xazz/MASTER.md`
- 범위: production 기능이 아닌 landing-to-tool Figma clickable prototype
- 검증 상태: 아래 감정·시간·성공률은 **사용자 테스트 전 가설**이며 달성 결과가 아니다.

## 첫 가치

Python 데이터/ML 개발자가 설치나 인증 없이 `Open a sample pipeline`을 선택하고, synthetic
air-quality sample의 data·graph·`.xzz`가 연결된 상태에서 table/chart와 rows/null/type
변화를 확인하는 순간이다. 목표는 90초 이내, 3개 이하의 의사결정이다.

## Golden path

| 단계 | 사용자 과업 | Frontstage | Backstage / prototype state | 목표 감정 | 실패와 복구 | 검증할 성공 신호 |
|---|---|---|---|---|---|---|
| Landing | Xazz가 왜 필요한지 판단 | 실제 작은 pipeline, 결과 중심 hero, `Open a sample pipeline` | synthetic sample과 Core/Labs maturity map을 정적 상태로 준비 | 명료함, 기대 | 기술 나열로 가치가 불명확하면 proof pipeline과 결과를 먼저 제시 | 10초 뒤 5명 중 4명이 “학습 전 typed pipeline 문제를 찾는 도구”라고 설명 |
| Sample | 설치 없이 첫 결과 열기 | data + graph + `.xzz`, `100 rows sampled`, Local, Available | 외부 CDN 없는 local asset; 미리 만든 sample state 로드 | 즉시성 | 빈 canvas·로딩 막힘 없이 sample 재열기 제공 | 5명 중 4명이 90초·3결정 이내 table/chart 도달 |
| Workspace | 변환과 결과의 관계 이해 | Compiler Canvas, code split, inspector, result dock | node·code·upstream/downstream의 동일 source mapping | 효능감 | 선택 위치를 잃으면 breadcrumb와 `Back to result` 제공 | 선택 node의 code·lineage·rows/null/type delta를 올바르게 찾음 |
| Live Check | 조건을 바꾸고 안전하게 미리보기 | `Live Check · 100-row sample · no side effects`, downstream stale 표시 | capped synthetic sample의 정적 before/after 상태 | 안전한 탐색, 통제감 | 검증 실패 시 what/where/affected/next step과 `Open code` | Full Run과 다른 범위·권한임을 전원이 구분 |
| Preflight | 실제 실행 전 준비·영향 검토 | `xazz`/`xazz-runner`/`xazz-exec` readiness, Local, verdict, artifact request | prototype에서는 세 바이너리를 `Future contract · not verified`로 명시하고, Control과 run confirmation을 별도 축으로 표현 | 신뢰, 책임감 | 하나라도 실제 unavailable이면 Run 차단, 설치 도움과 다시 확인 제공 | 사용자가 synthetic readiness와 실제 readiness를 구분하고 필요한 다음 행동을 설명 |
| Full Run | 명시적으로 전체 실행 | 한 개의 primary `Full Run`, process와 pipeline 상태를 분리 | production 실행이 아닌 synthetic `Starting → Running → Exited` prototype | 긴장 후 확신 | process exit만으로 success 처리하지 않음; runtime error/빈 result/artifact warning은 Unknown·Partial·Failed | 5명 모두 process exit와 pipeline verdict를 구분 |
| Receipt | 결과와 근거 검토 | result, artifact, row count, duration, warning, maturity, `Code hash · computed` | 관측값과 `Not available in this version` 필드를 구분; hash는 미보존 | 성취, 신뢰 | artifact 경고는 Partial로 유지하고 `Run full pipeline again` 제공 | 사용자가 실행 위치·산출물·경고·기능 성숙도를 찾음 |
| Error | 실패 지점에서 안전하게 회복 | failed node + code line + downstream impact; Explain/Open code/Apply as draft | 오류·영향·draft diff의 정적 state | 회복 가능성 | 자동 수정 금지; partial retry/restore/cancel/resume은 Future 표기 | 5명 중 4명이 45초 안에 실패 지점·원인·안전한 다음 행동을 찾음 |

## Truth boundary

- Core는 현재 확인된 typed pipeline·Polars 실행·result/hash 계산 범위만 `Available`로 쓴다.
- CSV import는 `Beta · Review required`, NQP는 `Demo`, Guard/DP/Burn/sLM은
  `Research` 또는 `Planned`다.
- policy approval, partial retry, restore, cancel/resume은 Future flow이며 Core 동작처럼
  연결하지 않는다.
- success 색은 검증된 pipeline 성공에만 사용하고 process, verdict, control, integrity,
  maturity를 각각 텍스트로 표시한다.
