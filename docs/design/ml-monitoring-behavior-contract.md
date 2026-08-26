# ML compile + monitoring — behavior contract

Plain-language state, transition, and permission rules. Written before implementation per the
design-method behavior-contract requirement. These rules — not a screenshot — are what any
generator or implementer must satisfy.

## Canvas · ML stage band

- The pipeline graph always shows both stage bands. The ML band exists even when no run has
  happened; it is never hidden to "keep the graph clean".
- Every node states its band (`PREPROCESS` or `ML COMPILE`) in text, not by colour alone.
- Before any run, ML nodes show configuration read from the program (target column, epochs,
  learning rate, layer count) and their outcome line reads `Not evaluated`.
- While `runState === 'running'`, ML nodes are `Unknown`. They never show a percentage, an ETA, or
  a current-epoch counter, because no epoch event reaches stdout.
- After a successful run, an ML node's evidence line may show only fields present in
  `TrainReport`. Any other quantity is `Not available in this version`.
- When `runState === 'error'`, ML nodes downstream of the failure are `stale`, matching the
  existing preprocessing rule.
- Selecting any node updates the inspector and code highlight identically in both bands.

## Canvas · view switching

- `Monitor` is a fourth option in the existing view segmented control and follows its existing
  `aria-pressed` and keyboard behaviour.
- Switching views never changes the selected node, the run state, or the result-dock tab.
- The Monitor view is not written to the URL; a reloaded or shared link opens on the graph.

## Monitor view · panel permissions

Three panels, three different evidence statuses. The difference is the point of the screen and
must never be flattened into one look.

| Panel | Maturity | May render measured values? | Source |
|---|---|---|---|
| Burn compile & training | `Beta` | Yes, but only fields of `TrainReport` | `[xazz:train]` marker → `ExecuteResponse.training` |
| Differential privacy budget | `Research` | **No** | none — proposed contract only |
| Resource efficiency | `Planned` | **No** | none — no monitoring endpoint exists |

- The `Research` and `Planned` badges are persistent. They are present in the empty state, the
  populated state, the error state, and the table alternative — not only on first render.
- Every chart in a `Research` or `Planned` panel carries a scope line reading
  `Synthetic structure · not measured · proposed contract` in the same position the canvas uses
  for its `Live Check demo · Future contract` line.
- The accessible text summary of each such chart repeats the synthetic status, so a screen-reader
  user never receives the numbers without the caveat.
- No panel may use `--success` colour. `budget safe`, `policy passed`, `audited`, and `sandboxed`
  remain forbidden strings and are asserted against in `tests/contract.mjs`.

## Monitor view · states

- **No run yet** (`ready`): Burn panel is empty with the reason "No Full Run has produced a
  training report"; DP and resource panels still render their proposed structure, because their
  status does not depend on a run — they are unimplemented either way.
- **Running**: Burn panel shows `Unknown`; it does not show the previous run's report as if
  current.
- **Error**: Burn panel shows the `[xazz:train]` failure payload (`success: false`, `error`) and
  marks the report fields `Not available in failed run`.
- **Success**: Burn panel populates from the report.

## What is intentionally impossible

- There is no control to "refresh telemetry", because no endpoint exists to refresh from.
- There is no privacy-budget threshold input, because a budget that cannot be measured cannot be
  enforced, and an input would imply it can.
- Per-epoch loss cannot be charted. The component takes two points and refuses more.
