# Workspace override

These rules override `design-system/xazz/MASTER.md` only for the Compiler Canvas.

## Density and hierarchy

- Use the 4/8px rhythm inside tool chrome and 12/16px between control groups.
- Canvas remains the largest visual region; inspector and result dock are
  evidence surfaces, not competing dashboards.
- One selected pipeline node controls code highlight, lineage highlight,
  inspector evidence, and result context.
- Keep raw logs behind progressive disclosure.

## Theme

- Use the dark token column.
- No glow, blur, gradient, transparent glass, or heavy shadow.
- Accent mint marks active trace and primary action, not generic decoration.
- Semantic status colors appear only in the affected component and always with text.

## Interaction

- `Live Check` and `Full Run` have different labels, icons, copy, and surfaces.
- The Full Run button opens preflight; it never starts from a graph edit.
- Preflight moves focus to the run-scope confirmation, traps keyboard focus, makes the
  workspace inert, and returns focus to Full Run when dismissed.
- Prototype readiness rows say `Future contract · not verified`; they never imply that
  a backend runtime was checked.
- Keyboard order: project → view mode → node list → canvas controls → inspector →
  result tabs → run action.
- Graph selection is mirrored in a semantic list so drag/pan is never required.
- During the synthetic demo, state transitions remain interruptible and expose
  the current stage through `aria-live`.

## Trust contract

- Show `xazz`, `xazz-runner`, and `xazz-exec` readiness independently.
- Show process state separately from pipeline verdict and artifact outcome.
- A run with no requested artifact may be `Succeeded · Artifact: Not requested`.
  Optional browser export happens after the run and is not receipt evidence.
- Runtime error text or artifact warnings produce Partial/Failed/Unknown, not success.
- Policy, DP budget, sLM correction, Burn training, partial retry, and durable
  audit are Future/Research surfaces.
