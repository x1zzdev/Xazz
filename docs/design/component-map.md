# Xazz 8-state prototype component map

이 문서는 승인된 `../spec/SPEC.yaml` 0.1.1의 구현 컴포넌트 경계와 Figma
component/variant, requirement ID를 연결한다. 현재 검증 가능한 프로토타입은
`visual-ide/src/components/{Common,Landing,Workspace}.jsx`에 의도적으로
통합되어 있으며, 아래 세분화된 이름은 production handoff 시의 추출 경계다.

## Naming and state rules

- Figma component는 `04 · Components` 페이지에 만들고 slash naming을 사용한다.
- frame은 `02 · Core Experience`에 core path를, policy/Future variant는
  `03 · Trust Flow · Future`에 둔다.
- local component의 `status` prop 하나에 모든 의미를 넣지 않는다.
  `maturity`, `process`, `pipelineVerdict`, `control`, `integrity`,
  `artifactOutcome`을 분리한다.
- 모든 interactive variant는 `default`, `hover`, `focus`, `disabled`를 갖고,
  필요한 경우 `pressed` 또는 `selected`를 추가한다.
- `Live Check`는 local sample fixture만 전환한다. `Full Run`은 Preflight를 거쳐
  명시적인 action으로만 전환한다.

## Shared component crosswalk

| Production handoff target | Source/adaptation | Figma component and variants | Requirement IDs |
|---|---|---|---|
| `prototype/src/components/actions/Button.jsx` | 신규; sibling `ToolPalette.jsx`의 command grouping만 참고 | `Action/Button` · `intent=primary|secondary|tertiary|danger`, `state=default|hover|focus|disabled|pressed` | R-002, R-008, R-011, R-013, R-014 |
| `prototype/src/components/status/StatusBadge.jsx` | 신규 | `Status/Badge` · `axis=maturity|process|pipeline|control|integrity|artifact`; axis별 승인 label variant | R-003, R-008, R-009, R-012, R-013, R-018 |
| `prototype/src/components/status/MaturityLabel.jsx` | 신규 | `Status/Maturity` · `value=Available|Beta|Demo|Research|Planned` | R-003, R-012, R-013 |
| `prototype/src/components/navigation/FocusSkipLink.jsx` | 신규 | `Navigation/Skip link` · `state=hidden|focus` | R-013, R-014 |
| `prototype/src/components/landing/XazzMark.jsx` | 신규; old screenshot/logo 재사용 금지 | `Brand/Xazz mark` · `theme=light|dark`, `.xzz` compiler-rail motif | R-004, R-014, R-016 |
| `prototype/src/components/landing/ProofPipeline.jsx` | sibling Canvas macrostructure의 정적·semantic 재구성 | `Proof/Pipeline` · `viewport=desktop|mobile`, `state=issues|result` | R-001, R-002, R-003, R-004, R-013, R-015 |
| `prototype/src/components/landing/CapabilityTruthMap.jsx` | 신규 | `Capability/Truth map` · maturity label variants | R-003, R-013, R-014 |
| `prototype/src/components/start/StartChoiceCard.jsx` | 신규 | `Project start/Choice` · `type=sample|csv|existing`, `state=default|hover|focus|selected` | R-002, R-004, R-013, R-014 |
| `prototype/src/components/start/SchemaImportReview.jsx` | sibling `ConfigWindow.jsx`의 local parsing/schema row 구조를 adaptation | `Import/Schema review` · `state=review|warning|confirmed`, `row=default|renamed|cast|nullable` | R-005, R-013, R-014, R-016 |
| `prototype/src/components/workspace/WorkspaceShell.jsx` | sibling `App.jsx` split/result layout을 adaptation | `Shell/Workspace` · `state=sample|blocked|running|success|error` | R-004, R-006, R-007, R-008, R-013, R-014 |
| `prototype/src/components/workspace/CommandBar.jsx` | sibling `ToolPalette.jsx` 정보 구조만 adaptation | `Workspace/Command bar` · state별 primary action slot | R-007, R-008, R-009, R-013, R-014 |
| `prototype/src/components/workspace/CompilerCanvas.jsx` | sibling `Canvas.jsx`의 React Flow foundation 재사용 | `Canvas/Compiler` · `state=sample|selected|stale|blocked|running` | R-006, R-007, R-010, R-013, R-014 |
| `prototype/src/components/workspace/PipelineNode.jsx` | sibling `CustomNode.jsx` foundation 재사용 | `Canvas/Pipeline node` · `state=default|selected|running|warning|failed|success|stale` | R-006, R-007, R-010, R-013, R-014, R-018 |
| `prototype/src/components/workspace/SemanticNodeList.jsx` | 신규; Canvas의 keyboard mirror | `Canvas/Node list item` · PipelineNode와 동일 state + focus/selected | R-006, R-013, R-014 |
| `prototype/src/components/workspace/CodePanel.jsx` | sibling `App.jsx`의 line-numbered panel을 adaptation | `Code/Panel` · `state=default|line-selected|line-error|stale` | R-006, R-007, R-011, R-013, R-014 |
| `prototype/src/components/workspace/Inspector.jsx` | sibling `ConfigWindow.jsx` structure를 adaptation | `Inspector/Panel` · `tab=setup|schema|impact`, `state=empty|ready|warning` | R-005, R-006, R-010, R-013, R-014 |
| `prototype/src/components/workspace/LineageRail.jsx` | 신규 | `Canvas/Lineage rail` · `state=default|upstream|selected|downstream|stale` | R-006, R-007, R-013, R-014 |
| `prototype/src/components/workspace/LiveCheckPanel.jsx` | 신규; old Auto-Run 재사용 금지 | `Check/Live check` · `state=idle|checking|ready|stale|issue`, 항상 `100-row sample` label | R-007, R-010, R-013, R-014, R-016 |
| `prototype/src/components/results/ResultDock.jsx` | sibling `ResultsWindow.jsx` layout을 adaptation | `Results/Dock` · `tab=preview|delta|chart|logs|receipt`, `state=empty|ready|partial|failed` | R-010, R-011, R-012, R-013, R-014, R-017, R-018 |
| `prototype/src/components/results/DataTable.jsx` | sibling ResultsWindow table behavior 재사용 | `Data/Table` · `state=sample|sorted|row-selected|empty`, scope/unit header variants | R-002, R-010, R-013, R-014, R-016 |
| `prototype/src/components/results/ChartCard.jsx` | sibling ResultsWindow local SVG chart code 재사용 | `Data/Chart` · `type=bar|line|pie|scatter`, `state=readable|over-limit|empty`, `view=chart|table` | R-002, R-010, R-013, R-014, R-017 |
| `prototype/src/components/results/ImpactDelta.jsx` | 신규 | `Evidence/Impact delta` · `metric=rows|nulls|schema|type|duration|artifact`, `state=observed|unavailable` | R-010, R-013, R-014 |
| `prototype/src/components/run/RuntimeReadiness.jsx` | 신규; Xazz crate names만 static fixture에 사용 | `Run/Readiness row` · `runtime=xazz|xazz-runner|xazz-exec`, 현재 `state=not-verified`; 실제 계약 이후 `verified|missing|unknown` | R-008, R-013, R-014 |
| `prototype/src/components/run/PreflightDialog.jsx` | 신규 | `Run/Preflight` · `pipeline=not-evaluated`, `control=not-configured`, `confirmation=required|confirmed`; policy approval은 Future only | R-008, R-009, R-013, R-014 |
| `prototype/src/components/run/RunProgress.jsx` | 신규 | `Run/Progress` · `stage=starting|validating|executing|collecting|exited`, `aria-live=polite` | R-008, R-009, R-013, R-014, R-018 |
| `prototype/src/components/run/OutcomeSummary.jsx` | 신규 | `Run/Outcome` · `process=running|exited|unavailable`, `pipeline=unknown|partial|failed|succeeded`, `artifact=unknown|not-requested|warning|written|failed` | R-008, R-012, R-013, R-014, R-018 |
| `prototype/src/components/run/RunReceipt.jsx` | 신규 | `Run/Receipt` · `state=success|partial|failed`, `field=observed|unavailable|future` | R-003, R-009, R-012, R-013, R-014, R-018 |
| `prototype/src/components/errors/ErrorCard.jsx` | sibling ResultsWindow error surface는 concept only | `Error/Card` · `kind=validation|runtime|artifact`, `severity=warning|partial|failed`; fixed what/where/affected/next order | R-011, R-012, R-013, R-014, R-018 |
| `prototype/src/components/errors/RecoveryActions.jsx` | 신규 | `Error/Recovery actions` · core `explain|open-code|apply-draft|full-retry`; `partial-retry|restore`는 `maturity=Planned` | R-003, R-011, R-013, R-014, R-018 |

## Delivered reusable Figma source

`04 · Components`에는 단순 specimen과 별도로 실제 reusable source를 둔다.

| Component set | Node ID | Variant properties |
|---|---|---|
| `Button` | `40:27` | `Surface=Light|Dark`, `Tone=Primary|Secondary`, `State=Default|Hover|Focus|Pressed|Disabled` — 20 variants |
| `Status badge` | `40:76` | `Axis=Maturity|Process|Pipeline|Control|Run confirmation|Integrity|Artifact`, axis별 승인 상태 — 22 variants |
| `Pipeline node` | `40:125` | `State=Default|Ready|Selected|Running|Warning|Failed|Success|Stale` — 8 variants |

- local variable collection `29:145`에 color, space, radius token 19개를 둔다.
- reusable source에는 122개 color paint binding과 250개 space/radius binding을
  적용했다.
- `Typography/Action`, `Typography/Meta axis`, `Typography/Meta value`,
  `Typography/Node title`, `Typography/Body small` local text style을 사용한다.
  reusable source의 96개 text layer가 모두 이 style 중 하나를 참조한다.
- Core 화면은 로컬 React prototype을 editable layer로 가져온 구현 기준 캡처다.
  위 component set은 후속 production screen 정리와 handoff의 canonical source이며,
  캡처 내부의 모든 하위 layer가 instance라고 과장하지 않는다.

## Frame-to-component map

| Approved frame | Required local composition | Figma frame / key variants | Requirement IDs |
|---|---|---|---|
| **Landing · Desktop 1440** | `XazzMark`, `LandingHero`, `ProofPipeline`, `CapabilityTruthMap`, primary/secondary `Button` | `02 · Core Experience / Landing · Desktop 1440`; `Proof/Pipeline viewport=desktop`; light theme | R-001, R-002, R-003, R-004, R-013, R-014, R-016 |
| **Landing · Mobile 390** | desktop landing components in single-column order; no workspace chrome | `02 · Core Experience / Landing · Mobile 390`; `Proof/Pipeline viewport=mobile`; 20px gutters | R-001, R-002, R-003, R-004, R-013, R-014, R-015, R-016 |
| **Project Start** | `StartChoiceCard×3`, sample recommendation, `SchemaImportReview` review overlay/state | `02 · Core Experience / Project Start`; `Choice type=sample state=selected`; `Import/Schema review state=review` | R-002, R-004, R-005, R-013, R-014, R-016 |
| **Workspace · Sample Ready** | `WorkspaceShell`, `CommandBar`, `CompilerCanvas`, `SemanticNodeList`, `PipelineNode`, `LineageRail`, `CodePanel`, `Inspector`, `LiveCheckPanel`, `ResultDock`, `DataTable`, `ImpactDelta` | `02 · Core Experience / Workspace · Sample Ready`; node `selected`; code `line-selected`; Live Check `ready`; result `ready` | R-003, R-004, R-006, R-007, R-010, R-013, R-014, R-016, R-017 |
| **Preflight · Needs Review** | inert Workspace context + focus-trapped `PreflightDialog`, synthetic `RuntimeReadiness×3`, explicit native run-scope confirmation, disabled Full Run until confirmation | `02 · Core Experience / Preflight · Needs Review`; `Pipeline=Not evaluated`; `Control=Not configured`; `Run confirmation=Required→Confirmed`; three `Future contract · not verified` rows; `Action/Button state=disabled→enabled` | R-003, R-008, R-009, R-013, R-014, R-016, R-018 |
| **Run · In Progress** | Workspace read-only context + `RunProgress`, separated status axes, interruptible synthetic transition | `02 · Core Experience / Run · In Progress`; `process=running`; `pipeline=unknown`; current stage in `aria-live` | R-008, R-009, R-013, R-014, R-016, R-018 |
| **Run · Success + Receipt** | `OutcomeSummary`, `ResultDock`, `DataTable`/`ChartCard`, `ImpactDelta`, `RunReceipt` | `02 · Core Experience / Run · Success + Receipt`; `process=exited`, `pipeline=succeeded`, `artifact=not-requested`; receipt `success`; optional browser export is post-result | R-003, R-008, R-009, R-010, R-012, R-013, R-014, R-016, R-017, R-018 |
| **Error Recovery** | selected failed node/code line, `ErrorCard`, `RecoveryActions`, partial/failed `RunReceipt` evidence | `02 · Core Experience / Error Recovery`; `Pipeline node=failed`; `Code=line-error`; `Receipt=partial|failed`; core full retry enabled | R-003, R-008, R-009, R-011, R-012, R-013, R-014, R-016, R-018 |

## Prototype transition contract

```text
Landing · Desktop 1440 ─┐
                        ├─ Open a sample pipeline → Project Start
Landing · Mobile 390 ───┘
Project Start → Run the air-quality sample → Workspace · Sample Ready
Workspace · Sample Ready → Full Run → Preflight · Needs Review
Preflight · Needs Review → explicit synthetic run-scope confirmation → Run · In Progress
Run · In Progress → verified fixture → Run · Success + Receipt
Run · In Progress → runtime/artifact warning fixture → Error Recovery
Preflight · Needs Review → close without confirmation → Workspace · Sample Ready
Error Recovery → review preflight to rerun → Preflight · Needs Review
```

- Landing desktop/mobile은 같은 entry state의 responsive variants이며 서로를 순차
  단계로 세지 않는다.
- prototype link는 native focus order를 따라야 하며 pointer-only hotspot을 만들지 않는다.
- success transition은 process exit만으로 발생하지 않는다. structured result가
  success evidence를 제공하고 requested artifact failure가 없을 때만
  `pipeline=succeeded`를 사용한다. 현재 fixture는 artifact를 요청하지 않는다.
- runtime error, empty structured result, artifact warning은 evidence에 따라
  `unknown`, `partial`, `failed` 중 하나로 이동한다.
- partial retry, restore, resume, durable audit, policy/DP/sLM/Burn은 core transition을
  실행하지 않는다. 필요하면 `03 · Trust Flow · Future`에서 maturity label과 함께만
  보여 준다.

## Fixture ownership

| Fixture | Required fields | Consumer |
|---|---|---|
| `sample-air-quality` | synthetic rows, 100-row scope, schema, encoding, nullable map, graph, verified `.xzz` lines | Project Start, Workspace |
| `preflight-blocked` | independent `xazz`, `xazz-runner`, `xazz-exec` readiness and blocking reason | Preflight |
| `run-progress` | stage sequence, elapsed synthetic time, process=`Running`, pipeline=`Unknown` | Run Progress |
| `run-success` | process exit, non-empty structured result, artifact requested=`false`, artifact=`Not requested`, node deltas, warnings, code hash | Success + Receipt |
| `run-error` | what, where, downstream scope, safe next step, process/pipeline/artifact evidence | Error Recovery |

Fixture는 repository-relative local module로 제공하고 network, filesystem upload,
backend process, CDN에 의존하지 않는다. 값이 fixture에 없으면 UI에는
`Not available`을 표시한다.
