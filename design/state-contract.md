# Xazz UI State Contract

- 기준 SPEC: `spec/SPEC.yaml` v0.1.1, approved 2026-07-27
- 기준 구현: 현재 저장소의 `xazz`, `xazz-runner`, `xazz-exec`, `xazz-server`
- 적용 범위: landing-to-tool 정적·합성 Figma prototype과 향후 UI 구현
- 비적용 범위: 이 문서는 존재하지 않는 backend 계약을 새로 정의하거나 구현 완료로 간주하지 않는다.

## 1. 절대 불변식

1. `Maturity`, `Process`, `Pipeline verdict`, `Control`, `Integrity`는 서로 다른 축이다. 하나의
   badge나 색으로 합치지 않는다.
2. **process exit code `0`은 `Process = Exited` 증거일 뿐 `Pipeline verdict = Succeeded`
   증거가 아니다.**
3. `Succeeded`는 다음 증거가 모두 있을 때만 허용한다.
   - raw stdout에 `[xazz:result]` marker가 실제로 존재한다.
   - 해당 marker의 JSON이 파싱된다.
   - stderr/logs에 `[xazz RUNTIME ERROR]`가 없다.
   - 요청한 artifact가 있다면 write-failure warning이 없다.
4. exit `0`이어도 runtime error, 빈 structured result, artifact-write warning이 있으면
   `Unknown`, `Partial`, `Failed` 중 증거에 맞는 verdict를 사용한다.
5. backend가 반환하지 않는 값은 추정하거나 `0`, `∞`, `PASS`로 채우지 않는다.
   사용자 표기는 `Not available in this version`, capability 표기는 `Research` 또는
   `Planned`를 사용한다.
6. `Control`의 `Approved`와 `Frozen`, `Integrity`의 `Verified`는 서로 대체할 수 없다.
   현재 SHA-256 비교 성공은 승인·동결·영구 감사 증거가 아니다.
7. 자동 동작은 side-effect-free capped sample인 `Live Check`로만 표현한다. 현재 backend에는
   별도 Live Check API가 없으므로 prototype에서는 `Future contract`를 함께 표시한다.
8. 실행이 artifact를 요청하지 않았다면 성공 verdict와 `Artifact = Not requested`는
   양립한다. 결과 이후의 browser export는 별도 사용자 동작이며 run artifact로 소급
   표기하지 않는다.
9. synthetic readiness는 실제 runtime 준비 증거가 아니다. 세 binary 모두
   `Future contract · not verified`로 표시하며 실제 `Ready`와 혼용하지 않는다.
8. 오류 수정안은 항상 draft다. `Apply as draft` 뒤 diff 검토 없이 실행 상태로 전환하지 않는다.

## 2. 근거가 되는 현재 backend 계약

| 근거 | 실제 계약 | UI 한계 |
|---|---|---|
| `xazz-server/src/main.rs::handle_execute` | `POST /execute`가 `{success, rows, schema, logs, stdout, error?}` 반환 | `success`는 child process status이며 전체 pipeline 성공을 보장하지 않음 |
| `xazz-server/src/main.rs::parse_stdout_markers` | `[xazz:result]`만 `rows`와 `schema`로 승격 | chart/model/train, node timing, delta, artifact는 구조화되지 않음 |
| `xazz-exec/src/runtime.rs::run_pipeline` | pipeline별 성공·실패를 stderr에 기록하고 다음 statement로 진행 | 개별 runtime 실패가 있어도 최종 `Ok(())`와 exit `0` 가능 |
| `xazz-exec/src/runtime.rs::validate_schema_types` | non-optional null과 누락 field를 warning으로 기록 | 차단 verdict가 아니며 Rename/Select가 있으면 검증 생략 |
| `xazz-exec/src/runtime.rs::handle_model_decl` | `[xazz:model]` marker와 “Burn 미구현” 경고 출력 | model 실행·compile 완료 증거가 아님 |
| `xazz-exec/src/runtime.rs::handle_train_stmt` | `[xazz:train]` marker와 “학습 루프 미구현” 경고 출력 | training progress, metric, checkpoint, success 없음 |
| `xazz-server/src/main.rs::handle_security_audit` | raw code bytes의 SHA-256과 응답 시각 계산 | 저장, audit ID, hash chain, 서명, policy 연결 없음 |
| `xazz-server/src/main.rs::handle_security_verify` | 입력 code의 computed hash와 provided hash를 문자열 비교 | `valid: true`는 exact-code hash match일 뿐 승인·감사 아님 |
| `xazz-server/src/main.rs::handle_health` | API process의 `status`, server version, timestamp 반환 | xazz, xazz-runner, xazz-exec 개별 readiness 아님 |
| `xazz-server/src/main.rs::handle_schema` | upload 저장 후 최대 100행으로 `{name,type}` 추론 | encoding, nullable, actual sample count, retention은 응답하지 않음 |

## 3. 독립 상태축

| 축 | 허용 상태 | 현재 증거 | 증거가 없을 때 |
|---|---|---|---|
| **Maturity** | `Available`, `Beta`, `Demo`, `Research`, `Planned` | SPEC R-003과 capability별 소스 | badge를 숨기지 말고 `Research` 또는 `Planned` |
| **Process** | `Idle`, `Requesting`, `Running`, `Exited`, `Exit failed`, `Termination unknown` | client request lifecycle, child exit status | `Termination unknown`; pipeline 성공으로 승격 금지 |
| **Pipeline verdict** | `Not evaluated`, `Unknown`, `Partial`, `Succeeded`, `Failed`, `Cancelled` | raw marker, parse result, runtime-error lines, artifact warnings | `Unknown` |
| **Control** | `Not configured`, `Validating`, `Needs review`, `Rejected`, `Approved`, `Frozen` | 현재 실제 증거는 `Not configured`뿐 | `Not configured`; `PASS` 금지 |
| **Integrity** | `Not computed`, `Computed`, `Verified`, `Mismatch`, `Not available` | `/security/audit`, `/security/verify` 응답 | `Not computed` 또는 `Not available` |

### 3.1 Maturity capability mapping

| Capability | Maturity | 허용 claim | 금지 claim |
|---|---|---|---|
| Local Polars pipeline run | `Available` | `Runs locally with Polars` | `Sandboxed`, `all nodes validated` |
| `xazz import` / `/schema` inference | `Beta` | `100-row inferred schema · review required` | `full-file verified`, 응답에 없는 encoding/nullable 확정 |
| `xazz emit rust` schema-column check | `Available` | `Static column check during Rust emit` | `same guard runs before every Full Run` |
| `xazz check` fixed NQP report | `Demo` | `Demonstration output` | `local sLM analysis completed`, `98.2% measured` |
| SHA-256 compute/compare | `Available` | `Code hash computed/verified` | `audit log persisted`, `pipeline approved` |
| Policy-as-Code, approval, freeze | `Research` | `Future trust flow` | `Policy passed`, `Approved`, `Frozen` |
| Differential privacy budget | `Research` | `Not available in this version` | epsilon/delta 잔량, `budget safe` |
| Burn model execution/training | `Planned` | `Model/training syntax parsed` | `model compiled`, `training completed` |
| sLM automatic remediation | `Research` | `Suggested future capability` | `AI fixed`, `local model generated this fix` |
| Run monitoring/resource telemetry | `Planned` | `Not available in this version` | live CPU/GPU, privacy spend, node duration |
| Security sandbox | `Research` | `Separate local subprocesses` | `isolated sandbox`, filesystem/network restricted |

### 3.2 Pipeline verdict derivation

| Process/result evidence | Pipeline verdict |
|---|---|
| Request not run, validation not performed | `Not evaluated` |
| exit status unknown, or exit `0` with no actual `[xazz:result]` marker and no definitive error | `Unknown` |
| parsed result exists, but any pipeline runtime error or artifact-write warning exists | `Partial` |
| parsed result exists, no runtime error, and every requested artifact has no write warning | `Succeeded` |
| non-zero exit, parse/lexer failure, or runtime error with no usable structured result | `Failed` |
| verified termination after an explicit cancel request | `Cancelled` |

`rows: []`만으로 marker의 부재와 정상적인 0-row 결과를 구분할 수 없다. 반드시 raw `stdout`에서
`[xazz:result]` marker 존재를 함께 확인한다.

## 4. 필수 UI 상태 11개

아래 상태는 status text와 icon을 항상 함께 사용하며 색만으로 구분하지 않는다.

| ID / 상태 | 5축 projection | Trigger | Required evidence | Forbidden claim | Primary action | Safe recovery | Future? |
|---|---|---|---|---|---|---|---|
| S-01 `Loading` | Maturity 유지 · Process `Requesting` · Verdict `Not evaluated` · Control 유지 · Integrity 유지 | sample/project/schema/run surface를 요청했으나 응답 전 | client request가 pending이고 이전 결과는 stale로 표시됨 | `Running pipeline`, `Validated`, 예전 결과가 최신이라는 표현 | `Wait` 또는 제공 가능한 경우 `Back` | timeout 시 `Retry request`; 이전 성공 결과는 `Last known result`로만 유지 | No, client-observed |
| S-02 `Empty` | Process `Idle` · Verdict `Not evaluated` 또는 증명된 0-row result · Control `Not configured` · Integrity `Not computed` | project/data가 없거나 실제 result marker가 0 rows를 담음 | empty reason; 0-row이면 raw marker와 parsed schema | marker 없이 `0 rows succeeded`, `No issues` | `Open sample pipeline` 또는 `Import CSV` | sample로 시작; import는 confirm 전 source를 변경하지 않음 | No |
| S-03 `Draft` | Process `Idle` · Verdict `Not evaluated` · Control `Not configured` · Integrity `Not computed` | code/graph/schema 변경이 아직 실행·승인되지 않음 | unsaved/pending diff와 affected downstream 범위 | `Ready`, `Approved`, `Applied AI fix` | `Review changes` 또는 `Live Check` | draft 폐기 또는 마지막 저장본으로 되돌리기 | No, UI state |
| S-04 `Validating` | Process `Running`인 sample check · Verdict `Unknown` · Control `Not configured` · Integrity `Not computed` | capped sample의 side-effect-free Live Check 시작 | sample cap, synthetic/sample source, no-side-effect label | `Full Run`, `Policy validating`, 전체 데이터 검사 | `View check scope` | check 취소는 local UI draft만 보존; Full Run 자동 전환 금지 | **Yes: 별도 backend 계약 없음** |
| S-05 `Blocked` | Process `Idle` · Verdict `Failed` 또는 `Not evaluated` · Control `Not configured`/Future `Rejected` · Integrity 상태 유지 | xazz/xazz-runner/xazz-exec readiness 누락, parse error, unsafe preflight, Future policy reject | 무엇이, 어디서, downstream 어디까지 영향인지; 누락된 readiness를 개별 표시 | `Policy rejected`(실제 policy evidence 없을 때), `Safe to run` | `Open code` 또는 `Resolve requirement` | 수정은 draft로만 적용하고 preflight를 다시 수행 | Mixed: core error는 No, policy/readiness aggregate는 Future |
| S-06 `Ready` | Process `Idle` · Verdict `Not evaluated` · Control 현재 `Not configured` · Integrity 선택적 `Computed` | environment와 preflight가 필요한 증거를 충족하고 사용자가 Full Run 전 단계에 도달 | xazz/xazz-runner/xazz-exec 개별 readiness, execution location, input/output path, side effects | `/health` 하나로 `All engines ready`, `Approved`, `Sandboxed` | `Full Run` | 조건이 바뀌면 즉시 `Draft` 또는 `Blocked`; 자동 실행 금지 | **Yes: 현재 readiness 계약 불충분** |
| S-07 `Running` | Process `Running` · Verdict `Unknown` · Control 현재 `Not configured` · Integrity run 전 값 | 명시적 Full Run request가 pending | 시작 시각, local execution label, known input/output; progress는 측정값이 있을 때만 | percent progress, current node, ETA, cancellation 가능 여부를 근거 없이 표시 | `View logs`; 실제 cancel 계약 전에는 `Stop` 비활성/미노출 | request 단절 시 `Termination unknown`; 결과를 success로 가정하지 않고 full retry만 제안 | No for pending request; progress/cancel은 Future |
| S-08 `Partial` | Process `Exited` 가능 · Verdict `Partial` · Control 유지 · Integrity 결과별 표시 | usable `[xazz:result]`와 runtime error 또는 artifact-write warning이 함께 존재 | parsed rows/schema, exact warning/error lines, 누락 artifact, affected downstream | `Succeeded`, `All artifacts saved`, `Receipt complete` | `Inspect affected step` | `Open code` 후 전체 재실행; `Retry from here`는 backend 전까지 미제공 | No, 현재 로그에서 파생 가능 |
| S-09 `Success` | Process `Exited` · Verdict `Succeeded` · Control 현재 `Not configured` · Integrity `Not computed`/`Computed`/`Verified` | §1의 Succeeded 증거를 모두 충족 | actual result marker, parse success, runtime-error 부재, 요청 artifact write 성공 | exit `0`만으로 성공, `Policy passed`, `Audited`, node timings가 측정됐다는 표현 | `View result` 또는 `View receipt` | 결과와 receipt를 read-only로 유지; 변경 시 즉시 `Draft/Stale` | No, 단 엄격한 파생 필요 |
| S-10 `Error` | Process `Exit failed` 또는 `Exited` · Verdict `Failed` · Control 유지 · Integrity 유지 | lexer/parser/server spawn/runtime failure로 usable result가 없음 | What happened, where, affected downstream, raw error evidence | `No data changed`(side effect 확인 전), 자동 원인 확정, `AI fixed` | `Open code` 또는 `Explain` | `Apply as draft`; 현재 구현은 전체 재실행만 허용 | No |
| S-11 `Cancelled` | Process `Exited` · Verdict `Cancelled` · Control 유지 · Integrity 결과별 표시 | explicit cancel 요청 뒤 실제 process termination이 확인됨 | cancel request ID/time과 termination acknowledgement; 남은 artifact 상태 | client가 화면을 닫았다는 이유로 `Cancelled`, `No side effects`, `Can resume` | `Review partial artifacts` | cleanup 확인 후 처음부터 재실행; resume/retry-from-here 금지 | **Yes: cancel/termination API 없음** |

## 5. Recovery와 approval 경계

| Action | 현재 허용 여부 | 계약 |
|---|---|---|
| `Explain` | 허용 | raw error를 설명하되 원인 확정과 실행 성공을 보장하지 않음 |
| `Open code` | 허용 | error가 line/column을 제공하면 이동; runtime 문자열만 있으면 pipeline/variable 수준으로 이동 |
| `Apply as draft` | 허용 | diff를 표시하고 `Draft`로 전환; 자동 Full Run 금지 |
| `Retry full run` | 허용 | 환경·입출력·side effect를 다시 확인한 뒤 명시적 실행 |
| `Retry from here` | 미지원 | checkpoint/node-resume backend가 생길 때까지 `Not available in this version` |
| `Restore last success` | 미지원 | persisted snapshot 계약이 생길 때까지 `Not available in this version` |
| `Cancel/Resume` | 미지원 | process control과 termination acknowledgement 전까지 Future |
| `Approve` / `Freeze` | Research | policy version, reviewer identity, immutable plan/input hash가 모두 있어야 활성화 |

## 6. 값 가용성

| Field | 현재 UI 값 | 이유 |
|---|---|---|
| Run ID | `Not available in this version` | `/execute` 응답에 없음 |
| Engine version | `Not available in this version` | `/health`는 xazz-server version만 반환 |
| Execution location | `Local process · not sandboxed` | 서버가 loopback에 bind되지만 실행 응답의 구조화 field는 아님 |
| Node duration / row delta / null delta | `Not available in this version` | 구조화 runtime event 없음 |
| Artifact list/status | stdout/log evidence가 있을 때만 표시 | `/execute`에 artifact field 없음 |
| Code hash | `/security/audit` 성공 시 `Computed` | 영구 저장되지 않음 |
| Dataset hash | `Not available in this version` | backend 계약 없음 |
| Policy version/verdict/approver/freeze hash | `Research · Not configured` | policy/approval backend 없음 |
| Privacy epsilon/delta/budget | `Research · Not available in this version` | DP accountant 없음 |
| CPU/GPU/memory metrics | `Planned · Not available in this version` | monitoring endpoint 없음 |
| Encoding / nullable from `/schema` | `Not available in this response` | endpoint가 `{name,type}`만 반환 |

## 7. Prototype 표시 규칙

- Core frame은 실제 계약만 사용한다.
- `Validate → Needs Review/Rejected → Approve → Freeze → Run → Receipt`는
  `Trust Flow · Future`에서만 사용하고 모든 frame에 `Research / Synthetic state`를 표시한다.
- Burn, DP, policy, sLM, monitoring 값은 현실적인 숫자를 만들어 채우지 않는다.
- synthetic sample 결과는 `Synthetic sample`과 sample cap을 항상 함께 표시한다.
- receipt에 없는 field는 빈 칸이나 dash 대신 `Not available in this version`을 쓴다.
