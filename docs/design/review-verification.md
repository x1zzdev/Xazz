# Independent critique verification

- Review input: nine rendered prototype screenshots, approved `SPEC.yaml`,
  `MASTER.md`, `state-contract.md`, and `component-map.md`
- Reviewer role: independent design critic; no file edits
- Verification rule: one cheapest observable check per finding before acting
- User testing: **not run**

## Finding receipts

### Preflight appeared to be a dead end

- **Claim:** the user could not complete the required review and enable Full Run.
- **Test:** inspected `PreflightDialog` and ran the existing keyboard E2E path.
- **Result:** **REFUTED as a functional dead end** — the native checkbox was focusable,
  changed state, and enabled `Start full run`; the E2E path passed. **CONFIRMED as
  a visual-affordance defect** — CSS hid the input and the rendered warning showed no
  checkbox control.
- **Action:** preserved the native checkbox and added a visible unchecked/checked
  control with explicit `Check to confirm` copy. The frame is named
  `Preflight · Needs Review`; `Blocked` remains a separate contract state.

### Maturity vocabulary and Future Labs grouping

- **Claim:** `Core available` was not a valid Maturity value, and Burn, DP, and
  monitoring were grouped under one `Research` label.
- **Test:** compared rendered strings and JSX against the five allowed maturity values
  in `state-contract.md`.
- **Result:** **CONFIRMED** — two Maturity badges used `Core available`; Burn and DP
  shared one Research row. The critic's separate claim that
  `Prototype unavailable` was a Maturity value was **REFUTED**; it is option
  availability metadata, not a status-axis badge.
- **Action:** Maturity badges now use `Available`; Future Labs uses per-capability
  `Research` or `Planned` badges.

### Live Check looked like an implemented Core backend contract

- **Claim:** Live Check lacked the mandatory Future-contract disclosure.
- **Test:** searched all product copy for `Future contract` and compared it with
  state-contract invariant 7.
- **Result:** **CONFIRMED** — zero component occurrences before the change.
- **Action:** every primary Live Check surface now states `Demo`, `Future contract`,
  synthetic 100-row scope, and no backend call. Running labels the previous check
  `stale`.

### Selected lineage did not identify upstream and downstream nodes

- **Claim:** only the selected node/code line was distinguished.
- **Test:** inspected graph node-state derivation, semantic list classes, edges, and
  code-line classes.
- **Result:** **CONFIRMED** — no upstream/downstream relation was rendered; only counts
  appeared in the inspector.
- **Action:** graph nodes, edges, exact operation code lines, and the keyboard mirror
  now carry explicit `Upstream`, `Selected`, or `Downstream` text/classes. E2E asserts
  the expected 2/2 relation counts for Fill null.

### Receipt invented unavailable execution fields

- **Claim:** a synthetic Run ID and artifact filename were presented as run evidence.
- **Test:** compared receipt fields with `state-contract.md` §6 and the current
  `/execute` response contract.
- **Result:** **CONFIRMED** — `DEMO-0727-001` occupied the Run ID field and the artifact
  filename had no structured run outcome.
- **Action:** renamed the deterministic identifier to `Fixture ID`; execution time is
  `Not available in this version`; artifact is a separate `Not written` axis and the
  optional browser download is not described as a run artifact.

### Failed-run inspector reused sample impact values

- **Claim:** the failed Fill null step showed successful sample deltas without a source
  label, while artifact language conflicted.
- **Test:** rendered the error route and traced inspector values to `pipeline.detail`.
- **Result:** **CONFIRMED** — the same fixture deltas were used in ready and error
  states, and the inspector said `None` while recovery copy said untrusted.
- **Action:** failed-run rows/nulls/schema are `Not available in failed run`; the UI
  explicitly says stale Live Check values were not substituted; artifact language is
  consistently `Not written · output untrusted`.

### Prototype branching controls competed with product UI

- **Claim:** `Complete with evidence` and `Show runtime error` looked like user-facing
  run choices.
- **Test:** inspected the running screenshot's reading order and button emphasis.
- **Result:** **CONFIRMED** — the success branch was the strongest action inside the
  run-status panel.
- **Action:** product UI now exposes only `View logs` and truthfully disabled cancel.
  Test branching moved to a dashed `Prototype navigator · not product UI` panel in the
  otherwise empty lower source rail.

### Error recovery over-emphasized immediate rerun

- **Claim:** Full Run was primary even though the safe next step was to inspect code.
- **Test:** compared action styles and copy with state-contract S-10.
- **Result:** **CONFIRMED**.
- **Action:** `Open code` is primary. Rerun is secondary and routes through
  `Review preflight to rerun`.

## Second-pass critique receipts

### Preflight mixed Control, verdict, and synthetic readiness

- **Claim:** `Control · Not configured` conflicted with `Control · Needs review`;
  pre-run `Pipeline · Unknown` and three unverified `Ready` rows overstated evidence.
- **Test:** compared the rendered labels and JSX with state-contract axes and current
  backend endpoints.
- **Result:** **CONFIRMED** — the same Control axis carried two values, the run had not
  started, and the browser prototype made no readiness call.
- **Action:** Control stays `Not configured`; Pipeline is `Not evaluated`; the user
  decision is a separate `Run confirmation · Required/Confirmed` axis. All three
  runtime rows say `Future contract · not verified`.

### Success and artifact outcome contradicted the requested side effect

- **Claim:** preflight promised a CSV write and timeline said `Demo CSV ready`, while
  the successful receipt said `Artifact · Not written`.
- **Test:** traced the only CSV creation path to the post-result Download button and
  compared it with R-018.
- **Result:** **CONFIRMED** — no artifact is created by the simulated run.
- **Action:** the run requests no artifact. Preflight and receipt now say
  `Artifact · Not requested`; optional browser export is a separate post-result user
  action. Failed runs retain `Not written · output untrusted`.

### Modal keyboard focus escaped into the workspace

- **Claim:** opening preflight left focus on `BODY`, allowed Tab into background UI,
  and did not restore the Full Run trigger.
- **Test:** opened Full Run with Enter in headless Chrome and inspected the active
  element and `inert` state. The active element was `BODY`; the first Tab reached
  `Skip to Compiler Canvas`; no inert ancestor existed.
- **Result:** **CONFIRMED**.
- **Action:** preflight now focuses the native checkbox, traps Tab/Shift+Tab, supports
  Escape, marks the top bar, skip link, and workspace shell inert/hidden to assistive
  technology, and restores focus to Full Run when dismissed. E2E now follows this real
  sequence without manually focusing dialog controls.

## Independent code-review receipts

### Pre-run tabs invented completed execution evidence

- **Claim:** ready-state Logs and Receipt rendered `Exited`, `Succeeded`, and
  `Integrity · Computed` because success was the fallback branch.
- **Test:** opened `/?screen=workspace`, selected Logs and Receipt, and captured their
  rendered text before any run.
- **Result:** **CONFIRMED** — both surfaces presented completed-run evidence.
- **Action:** success is now an explicit branch. Ready shows `Not started` and
  `Not evaluated`; running shows a pending receipt. A Playwright scenario forbids
  pre-run success evidence.

### Fixture, code-line, artifact, and partial-result drift

- **Claim:** the landing showed a row absent from `resultRows`, Check schema selected
  code line 2 instead of line 4, Result implied a run-produced CSV, and the error
  surface mixed `Failed`, `Partial`, and unlabeled stale counts.
- **Test:** compared rendered values with `src/data.js`, selected Check schema in the
  browser, and opened the error route.
- **Result:** **CONFIRMED**.
- **Action:** landing rows derive from `resultRows`; schema maps to line 4; Result says
  `Not requested · optional browser export`; the error verdict is consistently
  `Partial` and every retained preview scope says `Last Live Check · stale`.

### Token-only contrast gate produced a false green

- **Claim:** the landing badge axis and selected code line numbers failed 4.5:1 even
  while the token-pair test passed.
- **Test:** measured computed foreground and effective background in Chromium.
- **Result:** **CONFIRMED** — 4.28:1 and 3.28:1 respectively.
- **Action:** corrected the foreground tokens and added rendered-selector contrast
  scenarios for landing/workspace badges and ready/selected/error code rows.

### Capture and browser assumptions were not portable or deterministic

- **Claim:** capture could delete prior evidence before failure, trust another server
  on port 4173, require branded Chrome, and produce animation-dependent hashes.
- **Test:** inspected the build/capture lifecycle and compared two running-state
  captures.
- **Result:** **CONFIRMED**.
- **Action:** capture now builds first, uses an in-process random-port Vite preview,
  validates the Xazz title, writes to a temporary directory, swaps only after all nine
  frames succeed, uses bundled Chromium with reduced motion, and disables screenshot
  animations.

### Fixed synthetic fill value was mislabelled as a median

- **Claim:** the prototype described `31.0` as the fixture median even though the
  94 non-null PM2.5 values have a median of `43.0`.
- **Test:** sorted the source fixture values and compared the two middle values with
  the implemented null transform and visible inspector copy.
- **Result:** **CONFIRMED** — the middle values are `43.0` and `43.0`; `31.0` is an
  intentional fixed synthetic demo value, not an inferred statistic.
- **Action:** `demoFillValue` is now the transform's single source of truth; contract
  tests pin the fixed-value invariant, and local plus Figma error-recovery copy names
  it without claiming a median.

## Verification after changes

- Production build: passed.
- Contract test: `fixture=100→41; requirements=18/18`.
- Playwright: 10/10 passed, including real Tab-only entry navigation, keyboard
  confirmation, pre-run receipt protection, lineage relations, unavailable failed-run
  impact, rendered contrast, artifact status, and Korean validation.
- Contrast: 11 semantic token pairs and two rendered-selector scenarios passed; normal
  text ≥ 4.5:1 and essential boundaries ≥ 3:1.
- Capture determinism: two independent running-state captures produced the same
  SHA-256 `19B1FA4633D55448A10D9C8892B9157AA4BB794E14A1F98E1BB178FC557F949B`.
- Figma Core audit: 11 top-level frames, zero frame overlaps, and 10/10 intended
  click transitions connected. Final success frame `36:2` is on
  `02 · Core Experience`, not a documentation page.
- Figma design-system audit: component sets `Button` (`40:27`), `Status badge`
  (`40:76`), and `Pipeline node` (`40:125`) expose 50 variants in total.
  Reusable sources use 19 local variables, five local text styles, 122 bound color
  paints, and 250 bound spacing/radius properties; all 96 text layers reference a
  local text style.
- Figma visual inspection: final success and reusable-component source render without
  clipping; the exact 390 px mobile QA reference remains separate from the 512 px
  editable import.
- Figma post-review sync: success receipt exposes 12 disclosure fields, error verdict
  is `Partial`, stale scope is explicit, the landing preview row matches `resultRows`,
  the fill intent names `31.0` as a fixed synthetic demo value, and the 390×5469 QA
  frame uses the final capture image.
- Final delta review: local source, contract invariant, rendered error evidence, and
  Figma node `23:482` were cross-checked; CRITICAL/HIGH findings: 0.
- Dependency audit: `npm audit --audit-level=high` reported zero vulnerabilities.
