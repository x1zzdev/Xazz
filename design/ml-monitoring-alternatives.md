# ML compile + monitoring — structural alternatives

Three structurally different placements were compared, not colour or card variants. All three
assume the same ML stage nodes exist; they differ in where monitoring lives and therefore in what
the monitoring numbers appear to be evidence *of*.

## ALT-001 — Monitoring as a sixth result-dock tab

Pipeline gains `train`/`predict` nodes in the canvas. DP and resource panels become a `Monitor`
tab beside Preview / Delta / Chart / Logs / Receipt.

- Benefit: zero new navigation; inherits the dock's existing `Synthetic fixture` scope strip.
- Cost: the dock is 232px tall per `MASTER.md` §3. Two charts plus a per-operation budget ledger
  do not fit without either shrinking the canvas (violates "canvas retains visual dominance") or
  scrolling a chart, which the responsive gate treats as a small-screen strategy failure.
- Accessibility/misuse risk: low.
- Uncertainty: whether users would find a Research-grade surface inside a dock whose other five
  tabs are all run evidence — proximity implies the same evidence status.

## ALT-002 — Monitoring as a separate screen route

`?screen=monitor` with its own dashboard grid, independent of the workspace.

- Benefit: unlimited layout room; matches the issue wording "대시보드 구축" most literally.
- Cost: splits the work context. More seriously, detaching the panels from a run removes the
  natural anchor for "no evidence yet" — a standalone dashboard has no run to be empty *about*,
  which is precisely the condition under which a builder fills it with plausible numbers.
- Accessibility/misuse risk: **high**. This is the structure that most invites the failure mode
  the brief exists to prevent.
- Uncertainty: none worth resolving; the misuse risk is disqualifying under the design-method
  priority order (safety/privacy first).

## ALT-003 — Monitoring as a fourth canvas view mode *(selected)*

The existing `Graph / Split / Code` segmented control in `CanvasToolbar` gains `Monitor`. The
canvas region — the largest area in the frame — switches to the monitoring dashboard while the
topbar, source rail, inspector, and result dock keep their run context.

- Benefit: full canvas area for charts without shrinking anything; monitoring stays bound to the
  current run state, so `runState === 'ready'` yields a truthful "no run has produced telemetry"
  empty state instead of a decorative dashboard; reuses a control the user already learned.
- Cost: graph and monitor cannot be seen at once. Accepted — the two answer different questions
  ("what are the stages" vs "what did the run cost") and the inspector still shows the selected
  node in both.
- Accessibility/misuse risk: low. The segmented control is already keyboard-reachable and
  `aria-pressed`-labelled.
- Uncertainty: whether developers look for monitoring under a canvas view switch rather than a
  dock tab. Recorded as the open question for the user test.

## Selection

Selected `ALT-003`. Criteria in the design-method priority order:

1. **Privacy/misuse safety** — ALT-003 and ALT-001 keep monitoring anchored to a run; ALT-002 does
   not. ALT-002 rejected here.
2. **State truth** — ALT-003's empty state is meaningful (`no run yet`); ALT-001 inherits a dock
   strip that says `Synthetic fixture`, conflating a fixture with an unimplemented capability.
3. **Layout integrity** — only ALT-003 fits two charts and a ledger without violating the master's
   canvas-dominance and no-chart-scroll rules.

Remaining uncertainty: discoverability of `Monitor` inside the view segmented control. Test task
T-02 in `design/ml-monitoring-test-plan.md` targets exactly this.
