# Good first issue drafts

Working drafts for a batch of `good first issue` items. Each one is scoped to a
single crate or IDE area and grounded in the current tree (see `NEXT.md` and
`docs/ROADMAP.md`). Bodies below are the exact text to post; the matching
GitHub issues carry `good first issue` plus the labels listed under each title.

---

## 1. ci: run scripts/smoke_examples.py on pull requests
Labels: `good first issue`, `github_actions`, `chore`

`scripts/smoke_examples.py` walks `examples/` and runs `xazz check` + `xazz run`
on every pipeline, but nothing invokes it. The script only stays honest if CI
runs it.

What to do
- Add a job (or extend the existing `test` job) that builds `xazz` and
  `xazz-runner`, then runs the script.
- Keep it on the default feature set — no wgpu/cuda/onnx.
- The script already reports the failing file and stage; keep that in the CI log.

Where to start: `scripts/smoke_examples.py`, `.github/workflows/ci.yml`.

Difficulty: easy (YAML + a little Python)
Evaluation: CI, functionality testing

---

## 2. test(cli): cover project-name validation in src/project.rs
Labels: `good first issue`, `rust`, `security`

`validate_project_name` rejects path traversal before `xazz new` touches the
filesystem, and it has no tests. A regression here is a security bug, not a
cosmetic one.

What to do
- Add a `#[cfg(test)]` module in `src/project.rs` for the cases the function
  documents: empty, `.`, `..`, `/abs`, `a/b`, `a\\b`, `..\\x`, `C:\\`, and a
  URL-ish `scheme:` name.
- Include the happy path: `my-project`, `my_project`, `v2`.
- Assert on the error text, not just `is_err()`, so the invalid-name hint can't
  silently disappear.

Where to start: `src/project.rs:5`.

Difficulty: easy (basic Rust)
Evaluation: security, functionality testing

---

## 3. docs(server): fill in the REST API reference
Labels: `good first issue`, `documentation`, `scale:platform`

The HTTP API table in `docs/SECURITY_GUARDRAIL.md` covers `/security/policy*` and
`/dp/budget*`; the rest of the surface is only discoverable by reading
`xazz-server/src/main.rs`. `/execute`, `/runs`, `/runs/:id`,
`/runs/:id/resources`, `/catalog`, `/health`, `/security/audit*`,
`/security/remediate`, `/security/inference/check`, `/actor` and `/tenant` are
not in the docs.

What to do
- Add the missing routes to the existing table: method, path, one-line purpose.
- List the query params each accepts (e.g. `?cursor=` / `?limit=` / `?offset=`
  on the history routes) and the response shape at a high level.
- Point authentication behavior at `docs/SERVER.md` instead of repeating it.

Where to start: `docs/SECURITY_GUARDRAIL.md` §HTTP API, route list in
`xazz-server/src/main.rs`.

Difficulty: easy (docs)
Evaluation: community growth, presentation (clarity)

---

## 4. cli: make --tenant on single-token servers less surprising
Labels: `good first issue`, `documentation`, `rust`

When `XAZZ_SERVER_TOKEN` is set the server ignores `X-Xazz-Tenant` and runs with
an empty tenant (documented in `docs/SERVER.md`). The CLI still accepts
`--tenant` on `xazz registry`, `xazz dp` and `xazz policy-*` and sends the
header, so the flag looks like it worked when it did nothing.

What to do
- Document the no-op next to the shared `--tenant` help text, and add a row/note
  to the README CLI table.
- Keep the flag accepted — this is a clarity fix, not a removal.
- Optional: print a one-line stderr note when the server reports the
  single-token mode.

Where to start: shared flag/help helpers in `src/registry.rs`,
`src/dp_window.rs`, `src/policy_cli.rs`, `src/policy_query.rs`.

Difficulty: easy
Evaluation: community growth, usability

---

## 5. cli: let registry deploy read a policy from stdin or the default file
Labels: `good first issue`, `rust`, `enhancement`

`xazz registry deploy` takes a registry name or `--file PATH`, but not
`--file -` (stdin), and it doesn't fall back to `./xazz.policy.json` — the path
the rest of the CLI already treats as the active policy.

What to do
- Support `--file -` by reading the pack from stdin.
- When neither a name nor `--file` is given, use `./xazz.policy.json` if it
  exists; otherwise keep today's "provide a registry name or --file" error.
- Validate the JSON locally with `Policy::from_json_str` before sending, as the
  `--file` path already does.
- Add tests for stdin, the default-file fallback, and the missing-everything
  error.

Where to start: `src/registry.rs:202` (`run_registry_deploy`).

Difficulty: easy to medium (basic Rust)
Evaluation: functionality, community growth

---

## 6. docs(emitter): document the files generated predict code needs
Labels: `good first issue`, `documentation`, `rust`

`xazz emit rust` for a pipeline with `|> predict(...)` generates code that reads
`checkpoints/<Model>.stats.json` and adds a `serde_json` line to the header, but
the emitter docs/README don't say either is required to compile and run the
output.

What to do
- Document the extra `serde_json` dependency and the `.stats.json` sidecar in
  the emit section of the README (and/or an emitter doc).
- Call out that running the generated file on its own, without a checkpoint,
  fails — and what to do about it.
- Keep it short; link to the emit tests for the exact shape.

Where to start: `xazz-compiler/src/emitter.rs:134` (header) and the predict emit
around `emitter.rs:1411`.

Difficulty: easy (docs)
Evaluation: community growth, presentation (clarity)

---

## 7. docs(bench): account for runner startup in the benchmark method
Labels: `good first issue`, `documentation`, `scale:data`

`[xazz:timing]` measures the pipeline only. At the small scale the end-to-end
wall time is dominated by spawning `xazz-runner` (and `xazz-exec`), which the
README numbers don't mention — so "Xazz is 0.76× pandas at 228K" reads worse
than the pipeline itself and nobody knows why.

What to do
- Add a short "how it's measured" note to the benchmark docs/README: pipeline
  time vs process spawn + boot.
- If easy, record the spawn/boot delta in `benches/run_readme_benchmark.py` so a
  rerun captures it.
- Don't change the headline numbers in this issue.

Where to start: `benches/run_readme_benchmark.py`,
`benches/benchmark_results.json`, README performance section.

Difficulty: easy (docs; Python optional)
Evaluation: presentation (honesty), community growth

---

## 8. fix(visual-ide): tell "resource lookup failed" apart from "no record"
Labels: `good first issue`, `javascript`, `scale:ecosystem`

`ResourcePanel` renders the same empty state whether the server said
`available: false` (a run with no resource record) or the fetch failed (404,
network, wrong tenant). Users can't tell a missing feature from a broken
request.

What to do
- Pass the fetch error through to the panel and show a distinct "couldn't load"
  state with the reason.
- Keep the existing empty state for a successful `available: false`.
- Add an e2e/mock case for the failed fetch.

Where to start: `visual-ide/src/components/Monitor.jsx:428` (`ResourcePanel`),
`getRunResources` in `visual-ide/src/api.js`.

Difficulty: easy (React)
Evaluation: usability, community growth

---

## 9. refactor(visual-ide): share the history pagination control
Labels: `good first issue`, `javascript`, `scale:ecosystem`

Four panels hand-roll the same Previous / "Page n" / Next markup and the same
cursor-stack logic: policy history, policy-history TTL, DP window history and DP
reset history. The labels (`ttlPage` / `historyPage` / `windowPage` /
`gov.dp.historyPage`) are duplicated too.

What to do
- Extract one small pagination component (props: page, onPrevious, onNext,
  hasPrev/hasNext, label) and use it in all four places.
- Collapse the duplicate i18n keys, or keep them if the panels need different
  wording.
- Existing e2e tests must keep passing unchanged.

Where to start: `visual-ide/src/components/Governance.jsx`.

Difficulty: easy to medium (React)
Evaluation: maintainability, community growth

---

## 10. fix(visual-ide): show when the policy TTL failed to load
Labels: `good first issue`, `javascript`, `scale:ecosystem`

After the TTL load was split out of the policy-history load, `PolicyPackPanel`
renders the TTL form behind `{ttl && ...}`. If `getPolicyTtl` errors or the
server is offline, the form just vanishes with no explanation.

What to do
- Render an explicit "couldn't load" state for the TTL area when the fetch
  fails, instead of nothing.
- Keep the normal form for a successful load.
- Add a mock/e2e case for the failure.

Where to start: `PolicyPackPanel` in `visual-ide/src/components/Governance.jsx`,
`getPolicyTtl` in `visual-ide/src/api.js`.

Difficulty: easy (React)
Evaluation: usability

---

## 11. docs(examples): add an index README for examples/
Labels: `good first issue`, `documentation`

`examples/` has ten directories and no top-level README, so there's no way to
see what each pipeline demonstrates or which command to run it. Only `hello/`
and `semiconductor/` have their own README.

What to do
- Add `examples/README.md` with a table: directory, one-line description, the
  command to run it, and any data prerequisites.
- Call out which examples need the server, GPU features, or the DuckDB/Postgres
  connectors.
- Link it from the root README's examples section.

Where to start: `examples/`, `scripts/smoke_examples.py` (lists what runs).

Difficulty: easy (docs)
Evaluation: community growth, presentation (clarity)

---

## 12. docs: refresh the Japanese and Chinese README translations
Labels: `help wanted`, `documentation`

`README_ja.md` and `README_zh.md` both record source commit `e8db438`
(2026-09-24). The English README has changed since — it gained the limitations
section and the DuckDB/Polars comparison. Per `docs/TRANSLATING.md`, a
translation that can't be reviewed should not silently advance its source
commit.

What to do
- Compare the English README against the translated entry pages and update
  anything that drifted (commands, feature claims, links).
- Update the source commit/date in each status note, or mark it
  "⚠ Translation may be outdated" and link the change if you can't verify it.
- Keep code identifiers, flags and paths untranslated.

Where to start: `README_ja.md`, `README_zh.md`, `docs/TRANSLATING.md`.

Difficulty: easy to medium (needs a fluent reviewer)
Evaluation: community growth, presentation

---

## 13. emitter: fail compilation on a per-column vocab mismatch instead of panicking
Labels: `good first issue`, `rust`, `scale:ml`

For `Embedding([v0, v1, ...], d)` the generated Rust asserts
`embed_vocab.len() == input_dim` at runtime and panics on mismatch. The checker
now validates the common cases, but the emit path still compiles and only dies
after the program starts.

What to do
- Turn the mismatch into an emit-time error (or warning) using the input
  dimension the emitter already knows, rather than emitting the `assert_eq!`.
- Keep the generated `assert_eq!` as a last-resort guard if you can't prove the
  dimension statically, but stop treating it as the only check.
- Add a regression test next to the existing emitter tests.

Where to start: `xazz-compiler/src/emitter.rs:944`.

Difficulty: medium (compiler internals)
Evaluation: functionality testing, robustness

---

## 14. docs(security): document what happens to a stale DP reservation settle
Labels: `good first issue`, `documentation`, `security`

Cross-instance DP reservations expire after `XAZZ_DP_RESERVATION_TTL_SECS` and
another instance can reclaim the budget. If the original run finishes after that
and settles late, its spend is charged but the reclaimed reservation is not
released. That's intended, but it isn't written down anywhere a reviewer would
find it.

What to do
- Add a short note to the DP budget docs explaining the stale-settle case and
  why it's safe (spend is still charged; the reclaimed reservation belongs to
  the new holder).
- Mention the reservation TTL env var and its default.
- If reservation ids can be recorded in the audit trail cheaply, note that as a
  follow-up rather than doing it here.

Where to start: `docs/SECURITY_GUARDRAIL.md` (DP section),
`xazz-server/src/main.rs:190` (reservation TTL).

Difficulty: easy (docs)
Evaluation: security, presentation

---

## 15. ci: compile-check the wgpu backend on pull requests
Labels: `good first issue`, `github_actions`, `rust`

`cargo clippy --workspace` and `cargo test --workspace` build the default
feature set only. The `wgpu` (and `cuda`) features are exercised by hand, so a
change that breaks a gated backend can merge green — a dependency bump has
already threatened the ONNX path without CI catching it.

What to do
- Add a job that runs
  `cargo check --release -p xazz-exec --features wgpu --all-targets` (a check,
  not the ignored GPU tests).
- Keep it on one OS to bound cost; a compile failure is enough of a signal.
- Don't add cuda/onnx here — they need a toolchain and are tracked separately.

Where to start: `.github/workflows/ci.yml`, feature list in
`xazz-exec/Cargo.toml`.

Difficulty: easy (YAML + Rust feature flags)
Evaluation: CI, functionality testing
