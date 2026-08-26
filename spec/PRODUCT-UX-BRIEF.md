# Xazz Product UX Brief

- 상태: 2026-07-27 사용자 승인
- 근거: `INTAKE-NOTES.md`, `DISCOVERY.md`
- 산출물 목표: landing부터 첫 성공, 오류 복구, 실행 증적까지 이어지는 Figma clickable prototype

## 1. 제품 관점

Xazz가 사용자에게 팔아야 하는 것은 Rust, Polars, Burn의 나열이 아니다.

> **학습 비용을 쓰기 전에 데이터 문제를 발견하고, 데이터가 어떻게 변했는지 이해하며,
> 실행해도 되는 이유를 확인하는 경험**

이 관점에서 Xazz의 north-star는 “AI 기능이 많은 IDE”가 아니라
**inspectable, typed, local-first pipeline workbench**다.

## 2. 우선 사용자

### Primary — Python 데이터/ML 개발자

- pandas·PyTorch 중심 workflow에 익숙하다.
- Rust나 새로운 DSL은 배우기 싫지만 runtime surprise와 환경 접착 비용도 줄이고 싶다.
- 낯선 데이터의 schema/null 문제를 학습 전에 알고 싶다.
- “빠르다”는 주장보다 자신의 pipeline에서 무엇이 달라졌는지 보고 싶다.

### Secondary — ML pipeline reviewer/operator

- 본인이 작성하지 않은 pipeline이 어떤 데이터와 연산을 사용하는지 검토한다.
- 실행 위치, 변경 범위, 정책 판정, artifact와 hash가 필요하다.
- 모든 low-level log보다 “현재 무엇을 승인해야 하는가”가 먼저 보여야 한다.

보안 담당자를 첫 화면의 primary persona로 두지는 않는다. 같은 run receipt와 approval
surface를 progressive disclosure로 제공한다.

## 3. JTBD

> 낯선 데이터로 모델 학습을 시작하려 할 때, Xazz가 schema·null·lineage 위험과
> transformation impact를 먼저 보여주고 실행 가능한 pipeline과 근거를 남겨 줘서,
> GPU 비용을 쓴 뒤에 오류를 발견하지 않게 해 달라.

## 4. 사용자가 느껴야 할 감정

| 감정 | 제품 행동 |
|---|---|
| 즉시성 | sample을 열면 데이터·graph·code가 이미 연결돼 있고 한 번의 Run으로 결과가 나옴 |
| 효능감 | “완료” 대신 rows/null/type 변화, chart/table, artifact를 보여 줌 |
| 통제감 | 자동 동작은 sample check에 한정하고 full run·AI fix는 사용자가 승인 |
| 신뢰 | 실행 위치, 기능 성숙도, policy/hash 근거를 숨기지 않음 |
| 회복 가능성 | 실패 지점, 영향 범위, 마지막 성공 상태와 다음 행동을 함께 제공 |

## 5. 경험 원칙

1. **Outcome before architecture**  
   첫 화면은 “무엇을 막고 무엇을 얻게 되는가”를 말하고 기술 스택은 증거로 뒤에 둔다.

2. **First proof in one minute**  
   설치·cloud 연결보다 sample result를 먼저 보여 준다.

3. **Live check is not full execution**  
   자동 preview와 실제 data run을 시각·문구·권한 면에서 분리한다.

4. **Graph and code are one truth**  
   node를 선택하면 해당 `.xzz` line과 upstream/downstream이 동시에 강조된다.

5. **Safety is a sequence, not a badge**  
   Validate → Review → Approve → Freeze → Run → Receipt를 상태로 보여 준다.

6. **No invisible maturity**  
   Available, Beta, Demo, Research, Planned를 모든 진입점에서 동일하게 쓴다.

7. **Every failure has a recovery action**  
   오류는 원인·위치·영향·복구를 함께 말한다.

8. **Progressive evidence**  
   처음에는 결과와 다음 행동, 필요할 때 logical plan·raw log·hash를 펼친다.

9. **Exit is not success**
   process 종료, pipeline 판정, artifact 저장, policy 통제, integrity 검증을 서로 다른
   상태 축으로 표현한다. 현재 backend가 구조화하지 않은 값은 성공으로 추정하지 않는다.

## 6. Macrostructure 대안

| 대안 | 구조 | 장점 | 위험 | 판정 |
|---|---|---|---|---|
| A. Compiler Canvas | graph 중심, code/inspector 동기화, bottom result dock | 기존 React Flow 자산과 맞고 전체 pipeline 이해가 빠름 | 빈 canvas 공포를 sample로 해소해야 함 | **추천** |
| B. Notebook Ledger | 단계별 cell과 inline output | 첫 학습이 쉽고 결과가 가까움 | 현재 자산·DSL mental model과 멀고 Hex/Jupyter와 차별이 약함 | 보류 |
| C. Guard Command Center | policy verdict와 audit 중심 | 보안 서사가 강함 | 아직 구현되지 않은 기능이 제품 전체를 지배하고 개발자 첫 성공이 늦음 | Future |

추천안은 A를 Core로 사용하고, C의 policy verdict·receipt 패턴만 실행 전후에 삽입한다.

## 7. Golden path

```text
Landing
  → Open sample pipeline
  → Sample workspace가 data + graph + .xzz를 연결해 표시
  → Run sample
  → 결과 table/chart + rows/null/type delta
  → node 선택으로 graph ↔ code ↔ data impact 확인
  → 한 조건을 바꾸고 Live Check
  → Preflight에서 문제·영향·수정안을 검토
  → 명시적 Full Run
  → Run receipt와 artifact 확인
```

복구 path:

```text
Preflight/Run failure
  → 실패 node + code line
  → 무엇이 / 왜 / downstream 어디까지 영향인지 확인
  → Explain 또는 Apply as draft
  → diff 검토
  → 여기부터 다시 실행 또는 마지막 성공 상태 복원
```

## 8. Landing 정보 구조

### Hero

초안 메시지:

> **Catch data errors before training starts.**  
> Build one typed `.xzz` pipeline, inspect every transformation, and run it locally with Polars.

Primary CTA: `Open a sample pipeline`  
Secondary CTA: `Read the .xzz guide`

Hero의 시각 증거는 추상 3D artwork가 아니라 실제 작은 pipeline이다.

```text
CSV  →  Schema  →  Fill null  →  Filter  →  Result
         2 issues caught                    42 rows
```

### Proof sequence

1. **See the result first** — sample table/chart
2. **Understand what changed** — node별 rows/null/type delta
3. **Run with evidence** — local indicator, maturity badge, hash receipt
4. **Know what is real** — Core와 Labs capability map

벤치마크를 쓸 때는 workload, row count, 비교 조건 링크를 같이 표시하고 “Rust라서 무조건
빠르다”는 카피를 쓰지 않는다.

## 9. Project start

빈 canvas 대신 세 선택지만 둔다.

1. `Run the air-quality sample` — 추천
2. `Import a CSV`
3. `Open an existing .xzz project`

CSV import review에는 다음을 반드시 보여 준다.

- `100 rows sampled`
- 감지 encoding
- column name → `.xzz` field mapping
- inferred type과 nullable
- rename/cast 경고
- 생성될 `main.xzz` preview

사용자가 Confirm 하기 전 project source를 변경하지 않는다.

## 10. Workspace 구조

```text
┌ Project / environment ─────────────── Live Check ─── Run ┐
├ Data & operations ┬──────── Compiler Canvas ───────┬ Inspector ┤
│ files             │ graph / split / code           │ setup     │
│ schema            │ node + edge                    │ schema    │
│ transforms        │ upstream/downstream highlight  │ impact    │
│ models · Labs     │                                │           │
├───────────────────┴──── Result / Run detail dock ──┴───────────┤
│ Preview | Delta | Chart | Logs | Receipt                       │
└────────────────────────────────────────────────────────────────┘
```

시각 우선순위:

1. 현재 사용자가 조작 중인 node 또는 code
2. 다음 primary action
3. 실행·검증 상태
4. 결과와 영향
5. 고급 trace

모든 영역을 독립 card로 감싸지 않고 spacing, alignment, surface level로 그룹화한다.

## 11. 핵심 상호작용

### Live Check

- 100행 이하 synthetic/sample preview
- 전체 데이터 실행과 다른 icon·label
- 변경된 downstream만 stale로 표시
- side effect 없음
- 예상 비용과 sample 범위를 표시

### Full Run

- `xazz`, `xazz-runner`, `xazz-exec` 세 실행 파일의 준비 상태
- local/remote 위치
- 입력·출력 artifact
- preflight verdict
- consequential action이면 explicit confirmation
- Stop이 실제 중단 상태와 resume/retry 기준을 남김

현재 backend에는 progress·cancel·부분 재실행 계약이 없고 runtime 오류가 process
exit 0으로 끝날 수 있다. 따라서 Core prototype은 `Process exited`와
`Pipeline succeeded`를 분리하고, stderr runtime error나 artifact warning이 있으면
`Unknown/Partial`과 안전한 전체 재실행만 제공한다. `Retry from here`, cancel/resume,
last-success restore는 Future로 표시한다.

### Error card

항상 다음 순서:

1. What happened
2. Where it happened
3. What is affected
4. Safe next step

Actions:

- `Explain`
- `Open code`
- `Apply as draft`
- `Retry from here`
- `Restore last success`

AI 수정안은 자동 반영하지 않는다.

### Run receipt

Core:

- run ID와 시간
- engine/version
- execution location
- code hash
- input/output path와 row count
- node별 duration·delta
- warnings와 feature maturity

Future/조건부:

- policy version·verdict·approver·freeze hash
- dataset hash
- privacy budget
- cache/remote resource usage

구현되지 않은 field는 빈 값 대신 `Not available in this version`으로 표시한다.

## 12. Figma 산출물 범위

### Pages

1. `00 · Brief & Flow`
2. `01 · Foundations`
3. `02 · Core Experience`
4. `03 · Trust Flow · Future`
5. `04 · Components`

### Core prototype frames

1. Landing · Desktop 1440
2. Landing · Mobile 390
3. Project Start
4. Workspace · Sample Ready
5. Preflight · Blocked / Needs Review
6. Run · In Progress
7. Run · Success + Receipt
8. Error Recovery

### Required component families

- Button, IconButton, SegmentedControl
- StatusBadge · runtime / control / maturity 축 분리
- PipelineNode · default / selected / running / warning / failed / success / stale
- InspectorSection
- DataGrid header/cell states
- ErrorCard / ViolationCard
- RunTimeline item
- Receipt row
- Empty / Loading / Partial / Success / Error / Cancelled states

모든 반복 요소는 component/variant, 색·간격·radius·type은 Figma variable/style로 만든다.

## 13. 미감 방향

- Landing: light, generous whitespace, actual pipeline proof
- Workspace: calm dark neutral, 낮은 surface 대비, 선명한 type hierarchy
- Accent: 한 가지 primary action 색
- Success/warning/error는 semantic 전용
- 제품 고유 motif: `.xzz` type bracket, data-flow rail, compiler stage marker
- 금지: 보라색 AI gradient 남용, glass card 반복, 이유 없는 glow, 과도한 radius,
  작은 대문자 label, 모든 섹션 card화, cyber-security cliché

## 14. 접근성·품질 gate

- small text 대비 `>= 4.5:1`, UI boundary/icon `>= 3:1`
- body line-height `>= 1.5`
- keyboard-only로 sample open → run → result → receipt 가능
- 모든 icon-only control에 accessible name과 44px에 준하는 hit area
- focus visible
- status를 색만으로 표현하지 않음
- loading, empty, partial, success, error, cancelled 상태 포함
- 1440 desktop과 390 mobile landing에서 overflow 없음
- Squint Test에서 primary action이 먼저 보임
- section screenshot에서 clipping, placeholder, wrong font 0건

## 15. 성공 기준 초안

1. 첫 방문자 5명 중 4명 이상이 10초 노출 뒤 “무엇을, 왜 쓰는지”를 설명한다.
2. Primary persona 5명 중 4명 이상이 별도 도움 없이 90초, 3개 이하 의사결정으로 sample
   result에 도달한다.
3. 오류 prototype에서 5명 중 4명 이상이 45초 안에 실패 node, 원인, 안전한 다음 행동을
   찾는다.
4. 5명 모두 실행 위치와 기능 maturity를 올바르게 구분한다.
5. 위 접근성·상태·시각 게이트를 모두 통과한다.

숫자는 프로토타입 검증 목표로 승인됐다. 실제 사용자 테스트를 수행하기 전에는 달성값으로
표현하지 않는다.

## 16. Non-goals

- 이 단계에서 production frontend 또는 backend 구현
- 실제 개인/의료/금융 데이터 사용
- 구현되지 않은 Burn/DP/sLM/policy 기능을 Available로 시연
- 전체 monitoring dashboard와 조직 관리
- 모바일 IDE
- 기존 Visual IDE의 색만 바꾸는 reskin
