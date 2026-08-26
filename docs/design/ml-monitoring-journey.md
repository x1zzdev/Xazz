# ML compile + monitoring — journey

Scope: `touchpoint_only`. One actor (`P-001` Python data/ML developer), one touchpoint (Xazz
workspace). No new backstage owner, support process, or channel is introduced, so no service
blueprint record changes; see `service-blueprint.md` for the unchanged baseline.

| Step | Action | Goal | Touchpoint | Friction | Evidence | Failure | Recovery |
|---|---|---|---|---|---|---|---|
| J-1 | Open a pipeline that contains `train`/`predict` | See the whole program | Compiler canvas | Today the ML half is invisible: `data.js` stops at `result` | E-101 | User assumes the ML stages failed to parse | Stage band labels every node's stage; ML band is present even before a run |
| J-2 | Select an ML stage node | Learn what the stage will do and to which columns | Canvas → inspector | Preprocessing nodes carry row/null deltas; ML stages have no comparable pre-run evidence | E-102 | Inspector shows blanks, reading as broken | Pre-run ML nodes state configuration (target, epochs, lr) from source, and mark outcome fields `Not evaluated` |
| J-3 | Run Full Run | Get a trained model | Preflight → run overlay | Training has no progress contract — no epoch events reach stdout | E-103 | A fabricated progress bar implies measured progress | Run overlay keeps the existing truthful "no measured progress" treatment; ML nodes go `Unknown`, not a percentage |
| J-4 | Read the training outcome | Judge whether the model is usable | Monitor view · Burn panel | `TrainReport` has final losses but no epoch history | E-104 | A drawn loss curve invents the epochs between | Two-point train/val comparison plus explicit `Per-epoch history: Not available in this version` |
| J-5 | Check what the run cost in privacy budget | Decide whether another run is safe | Monitor view · DP panel | No DP accountant exists anywhere in the codebase | E-105 | A green "budget safe" badge asserts a control that does not exist | Panel is `Research`, values render from a `contract: proposed` mock and are stamped synthetic in the chart, the summary, and the table alternative |
| J-6 | Check resource efficiency | Decide whether to scale the job | Monitor view · Resource panel | No monitoring endpoint; `/health` returns server version only | E-105 | Live-looking CPU/GPU gauges | Panel is `Planned`, same synthetic stamping, and names the absent endpoint |
| J-7 | Return to the graph | Continue editing | View segmented control | View switch could lose node selection | E-102 | Selection resets, breaking code lineage | Selected node id is owned by `Workspace`, not by the view, so it survives the switch |

## Abandonment and return

- If the user leaves at J-5 without trusting the numbers, the intended outcome is still met: the
  panels are *supposed* to tell them not to trust the numbers yet.
- Returning to the workspace re-enters at the last `?state=` route value; the Monitor view is not
  persisted in the URL, so a returning user lands on the graph — the run-truth surface — first.

## First value and efficacy

- **First value:** the ML stage band appears in the canvas the moment the pipeline is opened,
  before any run, so the program is legible immediately.
- **Efficacy moment:** after a Full Run, the Burn panel changes from `Not evaluated` to real
  `TrainReport` fields (`num_params`, `final_train_loss`), which is a visible effect that could
  only have come from the user's run.
