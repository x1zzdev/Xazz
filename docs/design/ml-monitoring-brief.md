# ML compile + monitoring — design brief

- Scope slug: `ml-monitoring`
- Workflow mode: `0TO1` (new canvas view and new pipeline stage band inside an accepted master)
- Risk profile: `STRICT` (inherits `../design-evidence/xazz-product-ux.json` rationale — this scope
  adds privacy-capability representation, the sharpest case of that risk)
- Journey scope: `touchpoint_only` (workspace surface; no new service handoff, owner, or channel)
- Source issue: https://github.com/xazzdev/Xazz/issues/1
- Master constraint: `../design-system/xazz/MASTER.md`, page override
  `../design-system/xazz/pages/workspace.md`

## Actor and context

A Python data or ML developer (persona `P-001`, `../spec/SPEC.yaml`) has written an `.xzz` pipeline
that no longer stops at preprocessing. Since `cbb2577`/`ff5e47f`/`a4b543d` the language has real
`train(...)` and `predict(...)` pipeline operators backed by a working Burn engine in
`xazz-exec/src/dl.rs`.

## Job

Understand, before and after a run, what the ML half of the pipeline did — which stages exist,
what the trained model actually is, and what the run cost — with the same inspectability the
preprocessing half already has.

## Observed problem

1. `visual-ide/src/data.js` models five preprocessing stages only
   (`load → schema → fill → filter → result`). `PipelineCanvas` renders exactly that array, so a
   pipeline containing `train`/`predict` has no visual representation. The compiler canvas is
   silent about the half of the language the backend just gained.
2. `README.md` §2 and §"Real-Time Monitoring" promise differential-privacy budget tracking and
   resource-efficiency monitoring. `state-contract.md` §3.1 correctly classifies both as
   `Research` and `Planned`. There is no designed surface for either, so the first person to
   build one has no contract telling them which numbers are real — the failure mode is a
   dashboard of invented telemetry that reads as measured.

## Desired user outcome

The developer can name, for every stage and every monitored quantity, whether the number in front
of them was measured by this run, is a fixed synthetic fixture, or does not exist yet.

## Desired service outcome

A reviewable UI contract that tells the backend team the exact response shape the DP accountant
and resource monitor must return, without any shipped screen claiming those systems exist.

## Success criteria

- Every ML stage node carries an evidence line traceable to a field of `TrainReport`
  (`xazz-exec/src/dl.rs:130`) or to `[xazz:model]` (`xazz-exec/src/runtime.rs:968`).
- No quantity absent from `TrainReport` is rendered as a measured value; per-epoch loss history
  and wall-clock duration in particular render as `Not available in this version`.
- The DP and resource panels carry a persistent `Research` / `Planned` maturity badge and a
  scope line naming them synthetic, in every state, at every viewport.
- `npm run test:contract` gains assertions that fail if a forbidden claim string
  (`budget safe`, `policy passed`, measured-privacy phrasing) reappears.
- Existing 18 traced requirements and the 100→41 fixture invariant keep passing.

## Constraints

- `state-contract.md` §7: Burn, DP, policy, sLM, monitoring values must not be filled with
  realistic invented numbers.
- `../design-system/xazz/pages/workspace.md`: dark token column, no glow/blur/gradient/glass, canvas
  stays the largest region, status never color-only, graph selection mirrored in a semantic list.
- `MASTER.md` §6: normal text ≥ 4.5:1, essential boundaries ≥ 3:1, no CDN font/chart/icon.
- Charts must carry title, unit, series label, text summary, and a table alternative.
- Mobile IDE remains out of scope; laptop 1280 and desktop 1440 are the responsive targets.
