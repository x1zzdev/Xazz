# Xazz 경험 구조 대안

## 결정

Core는 **Compiler Canvas**를 사용한다. graph를 중심으로 `.xzz` code, lineage, inspector,
result dock을 같은 source에 연결해 “무엇이 어떻게 변했는가”를 보여 주기 때문이다.
Notebook Ledger는 비교·fallback 후보, Guard Command Center는 Future trust flow로 남긴다.

## 비교

| 기준 | A. Compiler Canvas | B. Notebook Ledger | C. Guard Command Center |
|---|---|---|---|
| 핵심 과업 | 전체 pipeline 추적, node impact 확인 | 단계별 작성과 inline result | policy verdict와 승인·receipt 검토 |
| Frontstage | graph + code split + inspector + bottom result dock | 순차 cell + 각 cell output | rule/verdict + approval timeline |
| Backstage 요구 | graph↔code mapping, lineage, selected-node delta | cell dependency와 partial execution 계약 | policy engine, approver, freeze, persistent receipt |
| 첫 가치 | sample graph에서 결과와 변화까지 한 화면 | 첫 cell 결과가 매우 가까움 | 위험 판정이 먼저라 결과가 늦음 |
| 목표 감정 | 이해, 효능감, 통제 | 친숙함, 빠른 학습 | 신뢰, 감시 가능성 |
| 현재 자산 적합성 | React Flow와 `.xzz` pipeline mental model에 가장 높음 | 현재 자산과 거리가 있고 notebook과 차별 약함 | backend가 없는 현재 Core와 불일치 |
| 주요 실패 | 빈 canvas 공포, 공간 과밀, graph와 code 분리 인식 | 전체 lineage를 놓치고 cell 실행을 실제 partial run으로 오인 | 미구현 Guard/승인을 Available로 오인 |
| 복구/완화 | sample preloaded, node/code 동시 강조, 한 primary action, result dock | dependency overview와 Full Run 경계 명시 | `03 · Trust Flow · Future`, maturity label, Core와 시각적 분리 |
| 판정 | **Core 채택** | 보류·비교 후보 | Future |

## Compiler Canvas flow

```text
Landing proof
  → Sample-ready Canvas
  → Result first
  → Select node: graph ↔ code ↔ impact
  → Live Check · 100-row sample
  → Preflight · three-binary readiness
  → Explicit Full Run
  → Receipt or evidence-led Error
```

Canvas는 frontstage에서 한 화면의 trace를 제공하지만 backstage는 synthetic prototype
state만 사용한다. process exit, pipeline verdict, artifact outcome을 분리하고 backend가
제공하지 않는 partial retry·cancel·restore는 만들지 않는다.

## 실패 시 전환 조건

Compiler Canvas 선택은 승인됐지만 사용자 검증은 아직 수행하지 않았다. 다음이 반복되면
Notebook Ledger concept을 제한적으로 비교한다.

- 5명 중 2명 이상이 sample result에 90초 안에 도달하지 못함.
- 5명 중 2명 이상이 선택 node의 code·upstream/downstream·delta를 연결하지 못함.
- 사용자가 canvas를 편집 도구로만 보고 결과/증거 표면을 찾지 못함.
- keyboard path에서 graph가 핵심 과업을 막음.

비교 시 동일 synthetic sample과 동일 copy를 사용하고 task success, first-value time,
trace 정확도, recovery 정확도를 측정한다. 미감 선호만으로 구조를 바꾸지 않는다.

## 성공 측정

- Hero 이해 4/5, first value 4/5, error recovery 4/5.
- execution location·maturity·process/verdict 구분 5/5.
- keyboard-only core path 완주, contrast/overflow/CDN gate 통과.
- 빈 canvas, unsupported capability, 자동 AI fix, color-only status 오인 0건.

위 수치는 테스트 목표이지 달성 결과가 아니다.
