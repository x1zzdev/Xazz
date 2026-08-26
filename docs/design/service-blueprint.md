# Xazz 서비스 블루프린트

- 대상: Python 데이터/ML 개발자, 보조 검토자/operator
- 채널: 1440 desktop workspace, 390 mobile landing
- 운영 전제: backend·auth·database 없는 local-preview-only synthetic prototype
- 검증 상태: 이 블루프린트는 승인된 설계 계약이며 실제 서비스 운영이나 사용자 테스트
  결과가 아니다.

## Landing → receipt/error

| Lane | Landing / sample | Workspace / Live Check | Preflight / Full Run | Receipt / error |
|---|---|---|---|---|
| 사용자 행동 | 가치 판단 → sample 열기 → 첫 결과 확인 | node/code 선택 → 조건 수정 → sample check | 환경·영향 확인 → 명시적 Run | 결과·근거 검토 또는 안전한 복구 선택 |
| Frontstage | light hero, proof pipeline, Core/Labs truth map | calm-dark Compiler Canvas, sources, inspector, Preview/Delta/Chart/Logs | 세 binary readiness, Local, verdict, artifact request, 별도 run confirmation, `Full Run` | table/chart, run timeline, receipt; what/where/affected/next step error |
| Backstage | synthetic air-quality fixture와 정적 sample-ready state | graph↔code mapping, 100행 capped state, downstream stale state | synthetic state machine; process와 pipeline verdict 분리 | 관측 receipt field와 unavailable future field를 분리 |
| 증거 | `100 rows sampled`, Available/Beta/Demo/Research/Planned | row/null/type delta, sampling scope, selected lineage | Process: Starting/Running/Exited; Verdict: Unknown/Partial/Failed/Succeeded | run ID/time, engine, Local, code hash computed/not persisted, artifacts, warnings |
| 지원·복구 | sample 재열기, `.xzz` guide | Explain, Open code, Apply as draft | missing binary별 도움, readiness 재확인, safe full retry | `Run full pipeline again`; partial retry/restore/cancel/resume은 Future |

## 상태 계약

```text
Maturity  : Available | Beta | Demo | Research | Planned
Process   : Starting | Running | Exited | Unavailable
Verdict   : Unknown | Partial | Failed | Succeeded
Control   : Not configured | Needs review | Approved | Rejected | Frozen
Integrity : Not computed | Computed | Verified | Mismatch
```

- `Exited`는 `Succeeded`가 아니다.
- stderr runtime error, 빈 structured result, artifact-write warning이 있으면 성공으로
  추정하지 않고 Unknown, Partial 또는 Failed를 근거와 함께 사용한다.
- 현재 Core의 hash는 `Code hash · computed`, `Not persisted`로 표기한다.
- policy approval·freeze는 `03 · Trust Flow · Future`에서만 사용한다.

## 실패/복구 블루프린트

| 실패 | Frontstage 처리 | Backstage 판정 | 안전한 다음 행동 |
|---|---|---|---|
| sample을 이해하지 못함 | 결과와 “2 issues caught” proof를 먼저 강조 | 용어·maturity 설명 state 노출 | sample 재시작 또는 `.xzz` guide |
| graph/code 추적 실패 | node, code line, lineage rail 동시 강조 | 동일 source mapping 복원 | 선택 해제 후 다시 선택 |
| binary unavailable | 해당 readiness 행과 Run을 차단 | Process `Unavailable` | 설치 도움 → readiness 다시 확인 |
| validation 실패 | failed node와 downstream stale 표시 | Verdict `Failed`, process 미시작 | Explain/Open code/Apply as draft |
| process 종료 + 근거 부족 | success 화면 금지 | Process `Exited`, Verdict `Unknown` | 로그 검토 → 전체 pipeline 다시 실행 |
| artifact 저장 경고 | 결과와 경고를 함께 유지 | Verdict `Partial` | 경로 확인 → 전체 pipeline 다시 실행 |

## 운영·측정 계획

- Hero 이해: 10초 후 4/5 설명 성공.
- First value: 4/5가 90초·3결정 이내 sample result 도달.
- Error recovery: 4/5가 45초 이내 실패 node·원인·안전한 행동 확인.
- 상태 진실성: 5/5가 execution location과 maturity, process와 verdict를 구분.
- 접근성: keyboard-only core path, visible focus, text 4.5:1, boundaries 3:1,
  status non-color-only.

위 수치는 후속 prototype test의 목표다. 아직 측정값은 없다.
