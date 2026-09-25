# Xazz — Scale Roadmap

> 최신 우선순위와 이슈 링크의 단일 소스. 각 작업 항목은 GitHub issue 로 트래킹된다.
> This document is the single source for the scale roadmap: each work item maps to a
> GitHub issue and to a track in the README's Phase 5/6 plan.

---

## Why scale?

Xazz is already a **correct, secure, documented** compiler+runtime: Typed IR (single-pass,
backend-independent), 3-gate policy guardrails, (ε,δ) DP accounting, SHA-256 audit chain,
and a 1.95×-vs-pandas benchmark (at 912K rows; 1.39× at 228K and 4.09M rows). What it is not yet is **scalable** — in four distinct senses:

| Sense of scale | Current ceiling | To grow past it |
| :--- | :--- | :--- |
| **Data volume** | CSV-only, in-core (whole file in memory) | Parquet/Arrow, out-of-core streaming, DB connectors, write path |
| **Program size** | Single-file DSL, no reuse unit | Module system, stdlib, LSP |
| **Team/org reach** | Local binary + local stateless server | Persistence, auth, lineage, Python bindings |
| **ML depth** | CPU-only Burn, Dense/MLP-only | GPU backends, ONNX interop, richer model graphs |
| **GenAI governance** | No prompt/output gates, no fine-tuning data policy | Input/output gates, fine-tuning data sanitization, model provenance |

Everything below is deliberately ordered: **each track's first issue is the highest
value-per-effort step**, and later issues depend on earlier ones.

---

## Track A — Data scale (out-of-core + sources)

A data-pipeline language that can only read CSV into memory is bounded to laptop-scale
datasets. This track makes Xazz handle real workloads.

### A1. `save()` output operator + Parquet/Arrow load — ✅ done (issue #52)
- [x] `load("x.parquet")` / `load("x.arrow")` → extension-dispatched loader (schema flows into the existing `:: Type` annotation path)
- [x] `|> save("out.parquet", format: "parquet")` and `save("out.csv")` — artifacts written directly from the pipeline
- [ ] `xazz check`/`xazz run` handle compressed + multi-file glob sources
- Acceptance: `xazz run` a script that loads Parquet and writes a Parquet artifact, with `[xazz:result]` unchanged. ✅

### A2. Out-of-core / streaming execution + large-scale benchmark — ✅ done (issue #53)
- [x] Switch CSV/Parquet load to `LazyFrame::scan_*` + Polars `streaming` feature (adaptive: eager ≤32MB, lazy/streaming above)
- [x] Extend `benches/` scale suite to 200M rows (synthetic, `--xlarge` opt-in)
- [x] Document peak-RSS vs latency tradeoff already noted in README
- Measured 2026-09-04: 1.39×/1.95×/1.39× vs pandas at 228K/912K/4.09M rows; lower peak RSS at scale.

### A3. External source connectors — issue #54
- [x] Embedded DuckDB source (`load("duckdb://...")`), SQL text in `.xzz` — `duckdb://:memory:?sql=...`
      and `duckdb://data.db?sql=...`; results flow into the normal Polars pipeline
      (filter/groupBy/mean etc.) and the schema `:: Type` annotation. Works with
      `xazz run`, `xazz check`, and `xazz sanitize`.
- [x] PostgreSQL read connector behind a `load("postgres://...")` source attribute —
      `postgres://user:pass@host:port/db?sql=...` (NoTls TCP; other query params pass through).
      Results flow into the same Polars pipeline; works with `xazz run`/`check`/`sanitize`.
- Depends on: A2. Acceptance: one `.xzz` file mixing CSV + DB sources in a single pipeline.
  ✅ **Done 2026-09-08** (DuckDB + PostgreSQL): both connectors implemented and tested.
  Demos in `examples/duckdb/`. `xazz check` passes on both URI schemes.

### A4. `xazz import` extension — Parquet/Arrow schema inference — ✅ done (issue #55)
- [x] `xazz import data.parquet` writes the same inferred `type` block today's CSV path does (via `xazz-exec --schema`)
- [ ] Interactive column → field mapping surfaced (feeds R-005 in the IDE)
- Depends on: A1. Acceptance: import round-trips into a runnable pipeline. ✅

---

## Track B — Language scale (reuse units)

### B1. Module system — ✅ done (issue #69)
- [x] `import "./preprocess.xzz"` — split pipelines across files (type/model/v pipelines inlined at import site)
- [x] Named reusable pipelines callable from any file (module `v` declarations referenced like locals)
- [x] Module-level `type` declarations shared across files
- [x] Cyclic-import detection (fail-closed)
- Acceptance: a 2-file project where `main.xzz` imports a data-prep module and type-checks against it. ✅
- Semantics: module = plain `.xzz`; imports resolve relative to importing file; each module is inlined exactly once (repeated/diamond imports do not duplicate declarations); checker runs on the merged AST so duplicate/missing-reference validation spans modules; policy gate scans module sources' literals too.

### B2. Standard library (`xazz-stdlib`) — issue #56
- [x] `xazz-stdlib/` with reusable `.xzz` modules (`common`, `math`, `models`),
      embedded into the compiler via `include_str!` so `import "std/<name>"` resolves
      everywhere with no filesystem config. Versioned with the workspace; documented
      in `xazz-stdlib/README.md`.
- [x] Includes type declarations (schemas) and reusable `model {}` architectures
- [ ] Richer date/string/statistics **operators** — future work (needs language-level
      operator additions; the stdlib currently reuses existing operators)
- Depends on: B1. Acceptance: `import "std/math"` usable in demos. ✅ **Done 2026-09-11**:
  `examples/stdlib_import.xzz` imports `std/common` + `std/models` and trains `MLPSmall`
  end-to-end.

### B3. LSP server (`xazz-lsp`) — issue #75
- [x] New crate (`tower-lsp`) exposing diagnostics, hover, go-to-def by **reusing the checker**
- [x] Diagnostics on open/change/save — byte-for-byte match `xazz check` line:col output
- [x] `import "mod.xzz"` resolved relative to the script file before checking (reuses B1)
- [x] hover / goto-def over the token-level **symbol table** (`xazz-compiler::symbols`:
      variable/type/model definitions + references) — verified via stdio smoke test
- [x] rename — symbol-table-driven file-wide rename (`prepare_rename` + `rename`), verified
      over stdio: `mydata` → `my_data` produces edits for the definition and every reference
- Acceptance: diagnostics in VS Code match `xazz check` line:col output exactly.
  ✅ **Done 2026-09-10**: diagnostics + hover + goto-def + rename all verified end-to-end over stdio.

---

## Track C — Platform scale (team / org reach)

### C1. Server persistence + run history
- [x] SQLite storage in `xazz-server` (`rusqlite`, `xazz.db`): `runs` table with id, code_hash,
      status, rows, error, created_at. The audit SHA-256 chain stays JSONL (append-only).
- [x] `GET /runs` list + `GET /runs/:id` receipt replay; `POST /execute` returns `run_id`
- Depends on: none. Acceptance: run history survives restart and is queryable via API.
  ✅ **Done 2026-09-09**: verified — execute → run_id, /runs lists, /runs/1 replays, data
  persists across a server restart.

### C2. Auth / multi-tenant — issue #59
- [x] Token-based auth beyond loopback — `XAZZ_SERVER_TOKEN` (single) + `XAZZ_TENANT_TOKENS`
      (`tenant1=token1,tenant2=token2`) with `X-Xazz-Tenant` header; 401 on missing/invalid
- [x] Per-tenant run isolation — `runs.tenant` column; `/runs` + `/runs/:id` scoped to the
      authenticated tenant (cross-tenant read → 404)
- [x] Per-tenant policy packs isolated by namespace — `tenant_policies` table keyed by tenant;
      `PUT`/`DELETE /security/policy` (self-service) store/remove the tenant's pack; execution
      and policy endpoints resolve the tenant's pack first (global/builtin fallback, fail-closed)
- [x] Per-tenant DP budgets isolated by namespace — server-side cumulative ε/δ ledger
      (`dp_budget` table); each run receives the tenant's *remaining* budget via
      `XAZZ_DP_BUDGET`/`XAZZ_DP_DELTA_BUDGET`, and `GET /dp/budget` reports spent/remaining.
      One tenant's spend never affects another's.
- [x] DP budget reset / window API — `POST /dp/budget/reset` zeroes a tenant's spend and
      re-anchors its window (self-service, tenant-scoped); `XAZZ_TENANT_DP_WINDOW_SECS`
      enables an automatic sliding window (`GET /dp/budget` reports `window_secs`/`resets_at`).
- [x] Per-tenant DP window length — a stored override (`tenant_dp_config`) takes precedence
      over the global `XAZZ_TENANT_DP_WINDOW_SECS`; `PUT`/`DELETE /dp/budget/window` manage
      it self-service and `GET /dp/budget` reports `window_source` (`tenant`/`global`).
- [x] Same-tenant DP precheck is atomic — per-tenant execution lock serializes a tenant's
      precheck → run → accrue; concurrent runs can no longer read the same `remaining` and
      jointly exceed the envelope. Different tenants run in parallel.
- Depends on: C1. Acceptance: two tenants cannot see each other's runs.
  ✅ **Run isolation done 2026-09-09**: verified end-to-end — tenant-a and tenant-b each see only
  their own runs; cross-tenant GET /runs/:id → 404.
  ✅ **DP budget isolation done 2026-09-11** (issue #59/C2): the `dp_budget` ledger accrues each
  run's `[xazz:dp]` spend per tenant; remaining budget is injected per run.
  ✅ **Policy pack isolation done 2026-09-14** (issue #59/C2): `tenant_policies` stores each
  tenant's pack by namespace; policy endpoints and `/execute` resolve it first (global/builtin fallback).
  ✅ **DP reset/window done 2026-09-14** (issue #59/C2): `POST /dp/budget/reset` + optional
  `XAZZ_TENANT_DP_WINDOW_SECS` sliding window (rolls the ledger when elapsed).
  ✅ **Same-tenant atomicity done 2026-09-14** (issue #59/C2): in-process per-tenant execution
  locks serialize the DP precheck/run/accrue sequence; cross-tenant runs stay parallel.

### C3. Pipeline catalog + lineage — issue #60
- [x] Column-level lineage derived from the IR's flowing `Schema` — `xazz-compiler::catalog`
      walks each pipeline's ordered steps, tracking output-column provenance
      (select/rename/groupBy+agg/withColumn; filter/drop-null/cast/sort are pass-through)
- [x] Pipeline catalog entry per `PipelineNode` (id, variable name, input/output columns, lineage)
- [x] `POST /catalog {code}` — compile the code and return the catalog (same Typed IR as execution)
- Depends on: C1. Acceptance: a reviewer can trace `groupBy → agg → chart` output columns back
  to source columns. ✅ **Done 2026-09-09**: verified — `by_station` output
  `pm10_rank ← pm10`, `pm10 ← pm10`, `station ← station`.

### C4. Python bindings — issue #61
- [x] `xazz.check(src)` / `xazz.run(src)` / `xazz.policy(src)` — pure-Python adapter over the CLI
      (`python/xazz/`), same diagnostics as the CLI byte-for-byte
- [ ] PyO3 native extension — deferred: no python3-dev headers in this build env (no sudo);
      the subprocess bridge already delivers the same contract
- [ ] Numpy/Pandas in → Arrow out handoff — deferred (native extension path)
- Depends on: B3 (shared checker ergonomics), C1 optional. Acceptance: `xazz.check(src)` returns
  the same diagnostics as the CLI. ✅ **Done 2026-09-09** (subprocess adapter + 5 tests).

---

## Track D — ML scale (Phase 6)

### D1. GPU backends — issue #62
- [x] `ComputeBackend` trait at the `MLOp` boundary + `XAZZ_BACKEND` selection with an
      explicit CPU fallback warning (`xazz-exec/src/backend.rs`); GPU parity test is
      `#[ignore]`d behind each feature
- [x] `burn-wgpu` (cross-vendor) provider behind `--features wgpu` — trains and
      predicts on the probed device with a portable checkpoint handoff; lavapipe
      software acceptance passed (2026-09-16)
- [x] `burn-tch` (CUDA) provider behind `--features cuda` (2026-09-22) — `LibTorch`
      device selection via `XAZZ_CUDA_DEVICE` (default 0), fails closed with a clear
      message when the linked LibTorch has no CUDA runtime
- [x] `burn-wgpu` real-hardware acceptance — passed on Windows 11 / RTX 4070 Laptop +
      Intel Arc iGPU (2026-09-25); `XAZZ_DEVICE=dgpu:0` / `igpu:0` verified to select the
      intended adapter via per-process GPU engine counters
      (`docs/design/gpu-backend-acceptance.md`)
- [ ] Real-hardware acceptance on a CUDA host — `cargo test -p xazz-exec --features cuda -- --ignored`
      ⚠️ Attempted 2026-09-25 on the RTX 4070 host with the `x86_64-pc-windows-gnu` toolchain:
      LibTorch 2.9.0+cu128 downloads, but torch-sys' C++ shim does not compile under g++
      (MSVC-only flags/ABI). **Requires the MSVC toolchain on Windows** — retry pending.
- Depends on: none (Burn API is backend-agnostic). Acceptance: same `.xzz` trains on CPU and CUDA with identical reported losses.
  ⏳ **Provider landed 2026-09-22**: `burn-tch` wired through the trait + `train_on_device`/
  `predict_on_device` device threading; the gated acceptance test needs a CUDA host with a
  CUDA-built LibTorch (`TORCH_CUDA_VERSION`).

### D2. ONNX export/import — issue #63
- [x] ONNX provider slot in `ComputeBackend` (`--features onnx`)
- [x] `TrainedModel` → ONNX export — `dl::onnx_export` emits a standard `ModelProto`
      for Dense/Conv1d/Embedding + ReLU/Sigmoid/Tanh/Softmax (2026-09-22)
- [x] ONNX → inference without re-training — `OnnxBackend::predict` runs the exported
      graph through ONNX Runtime (`ort`, binaries auto-downloaded) (2026-09-22)
- [ ] Real ONNX Runtime acceptance on a standard toolchain — `cargo test -p xazz-exec --features onnx -- --ignored`
      ⚠️ Attempted 2026-09-25 on Windows with the `x86_64-pc-windows-gnu` toolchain:
      `ort-sys` ships prebuilt binaries only for `*-windows-msvc`
      (`no prebuilt binaries available for target x86_64-pc-windows-gnu`). **Requires the MSVC
      toolchain on Windows**; macOS `coreml` run still pending (`docs/design/gpu-backend-acceptance.md`).
- Unlocks ecosystem interop and model serving
- Depends on: D1 (device mapping). Acceptance: exported ONNX runs in onnxruntime with same prediction.
  ⏳ **Provider landed 2026-09-22**: export + `ort` runtime + parity test wired; the local
  WSL zig C++ linker cannot link ort's prebuilt C++ static library, so the gated acceptance
  must run on a standard toolchain (Windows/CI).

### D3. Model graph expansion — issue #64
- [x] **Early stopping** — `train(..., validation_split: 0.3, patience: N)` stops when validation
      loss does not improve for N epochs; report gains `stopped_early` + `best_epoch`
- [x] **Conv1d layer** — `Conv1d(out_channels, kernel_size)` in the `model {}`
      declaration (`Same` padding, length-preserving), wired through
      parser/checker/emitter/dl; CPU E2E train/predict test added (2026-09-14)
- [x] **Embedding layer** — `Embedding(vocab, embed_dim)` in the `model {}`
      declaration (first layer only; raw category indices, no z-score). A scalar
      vocab is shared by every input column; a list `Embedding([4, 7], 3)` gives
      each input column its own vocabulary (per-column, 2026-09-21). wired through
      parser/checker/emitter/dl; CPU E2E train/predict test added (2026-09-18)
- [x] **Hyperparameter sweep** — list-valued `epochs`/`lr`/`batch_size` in `train()`
      run a cartesian grid search; the best combination (validation loss when a
      split is set, else training loss) is reported and kept for `predict()`.
      `metric: "mae" | "r2"` selects the winner by MAE or R² instead of MSE
      (2026-09-18, metric 2026-09-21). `sort: "metric" | "epochs" | "lr" | "batch"`
      orders the reported table and `top: N` keeps only the N best-by-metric
      combinations (2026-09-21); `tiebreak: "epochs" | "lr" | "batch"` or an
      ordered list (`tiebreak: [lr, batch]`) sets the axis(es) compared first
      among `sort:` ties (2026-09-23); a
      `metric:`/`sort:`/`tiebreak:`/`top:` without a sweep grid
      is a no-op and the checker warns — including explicitly written defaults
      like `metric: "mse"`/`sort: "metric"` (2026-09-22)
- [x] **Checkpoint versioning** — a `<model>.meta.json` sidecar manifest records the Xazz
      checkpoint format version, xazz version, model shape/provenance; loads fail closed when
      the manifest declares a newer format, while legacy checkpoints without a manifest still
      load (2026-09-21)
- Depends on: D1. Acceptance: a CNN pipeline trains end-to-end on image-style tabular data.
  ⏳ **Partial 2026-09-11 / updated 2026-09-18**: early stopping, the Conv1d layer,
  the Embedding layer, and the hyperparameter sweep are shipped and verified end-to-end
  on CPU (early stopping + `best_epoch`; Conv1d/Embedding CPU train/predict; sweep grid
  selection). Checkpoint versioning shipped 2026-09-21.

---

## Track E — Ecosystem scale

- **E1. VS Code extension** — ✅ `vscode-xazz/` (issue #65): LSP client (diagnostics, hover,
  go-to-def, rename via `xazz-lsp`) + `Xazz: Run`/`Check` commands + `.xzz` syntax highlighting.
  Binary auto-discovery (config / env / `target/{debug,release}` / PATH). Compiles cleanly; F5 or `vsce package`.
- **E2. Official Docker image** — ✅ (issue #66) `Dockerfile` + `docker-compose.yml`: 3 stages
  (Rust binaries → Visual IDE → slim runtime), bundles `xazz`/`xazz-runner`/`xazz-server` + `/app/web`,
  non-root user, `VOLUME /data`, `EXPOSE 8005`. `XAZZ_BIND` env lets the container listen on `0.0.0.0`.
- **E3. GitHub Actions official action** — ✅ (issue #67) `.github/actions/xazz` composite action
  (builds the CLI, runs `check`/`policy`/`run` over a file or directory, fails on non-zero exit)
  + `.github/workflows/policy.yml` gate: check demo, safe pipeline passes, unsafe pipeline is
  asserted blocked, demo pipeline runs end-to-end.
- **E4. Package/registry for policy packs + stdlib modules** — ✅ (issue #68) offline-first embedded
  registry: `xazz registry list/show/install`. Packs (healthcare/finance/public-sector) install to
  `xazz.policy.json` (auto-loaded); stdlib modules install to `std/<name>.xzz`. `--out`/`--force` supported.

---

## Track F — GenAI governance

Positioning: engines (Burn, burn-engine) are becoming embedded commodities; **governance is the
layer Xazz owns**. This track extends the same three gate points to GenAI and is timed to pair
with the burn-engine release (Burn 0.22 era) — LoRA/QLoRA fine-tuning and on-device inference
are exactly where "safe data in, auditable output out" becomes a real requirement.

### F1. Prompt input gate — static scan of prompt literals — issue #70
- [x] New rule family `XZP020`–`XZP022` in `xazz-compiler/src/policy`: prompt-injection / jailbreak /
      exfiltration patterns detected at **compile time** against prompt literals (same fail-closed, line:col story as today's PII rules)
- [x] A prompt is just another typed literal — `prompt("...")` or a `model.prompt` literal gets the same
      policy treatment as an RRN literal (source-text scan, comments included; context-limited to
      `prompt(...)` shapes so ordinary data strings are not flagged)
- [ ] `xazz check` surface: a `--policy` flag or policy-in-check reporting (today the gate surfaces are
      `xazz policy` and the `xazz run` gate)
- Depends on: none (pure policy-engine extension). Acceptance: `xazz check` blocks a jailbreak prompt
  with `line:col` diagnostics, `xazz policy --fix` proposes a sanitized prompt.
  ✅ **Done 2026-09-07**: `xazz policy`/`xazz run` block XZP020/021/022 with line:col; prompts are left
  as residual by `--fix` (never auto-rewritten). Demo: `examples/security/prompt_{unsafe,safe}.xzz`.

### F2. LLM output gate — runtime re-scan + audit chaining — issue #71
- [x] Runtime re-scan of model outputs (free-form text) against the same policy rules — `scan_output_text()`
      (PII/API key/private key; `GenericSecret` excluded to avoid false positives on credential *examples*)
- [x] Prompt + response SHA-256 hashes appended to the existing append-only audit chain
      (`append_inference_call`; the response is **never stored** — only its hash)
- [x] `POST /security/inference/check` — runtime output gate + per-call audit evidence
- Depends on: F1 (shared rule catalog). Acceptance: a demo where an LLM call emitting a masked secret
  is flagged and the prompt/response pair is verifiable in `/security/audit`.
  ✅ **Done 2026-09-07**: `curl /security/inference/check` flags a leaked API key (masked `AK******`),
  records `prompt_hash`/`response_hash` to the audit chain, and `GET /security/audit/chain` verifies it.

### F3. Fine-tuning data sanitization — safe LoRA/QLoRA prep — issue #72
- [x] First-class op `sanitize(...)` — `xazz sanitize <file>` runs PII / near-duplicate / bias checks
      on training data (CSV/Parquet/Arrow) and emits a structured sanitization report
      (the fine-tune intake artifact). Raw PII is never reported — only masked samples
- [ ] Pair with burn-engine LoRA/QLoRA: the sanitization report feeds the fine-tuning call
      (deferred to F4 — the engine isn't released yet)
- Depends on: F1 (rules reused). Acceptance: a `.xzz` pipeline that sanitizes a CSV, emits a structured
  sanitization report, then hands the cleaned data to a fine-tuning call.
  ✅ **Done 2026-09-07**: `xazz sanitize examples/security/data/patients.csv` flags 500 masked phone
  findings + near-duplicate/bias sections; `--json` emits the structured artifact. 345 tests pass.

### F4. burn-engine integration — embed or deploy, both — issue #73
- [x] Backend trait at the `MLOp` lowering boundary so Burn stays the first provider but
      burn-engine / ONNX Runtime are swappable behind the same Typed IR
      (`xazz-exec/src/backend.rs`, `XAZZ_BACKEND` selection + CPU fallback)
- [ ] burn-engine / remote-server provider impls (embedded vs remote, mirroring the
      `xazz-runner` + `xazz-server` split)
- Depends on: D2 (ONNX) partially, F1–F3 (the guardrails must exist before inference calls are first-class). 
  Acceptance: the same `.xzz` runs inference via embedded burn-engine and via ONNX with identical outputs.
  ⏳ **Interface landed 2026-09-11**: dispatch is trait-based and verified on CPU; provider impls are
  blocked on the Burn 0.22 / burn-engine release.

### F5. Model provenance — weights & license metadata guard — issue #74
- [x] `model {}` declarations and `load("hf://...")`-style sources carry license/weights metadata;
      policy can block restricted-license or fingerprinted-unknown weights
      (policy registry `allowed_models` + `denied_licenses` + `require_model_provenance` fail-closed;
      XZP030 MODEL_LICENSE_BLOCKED · XZP031 MODEL_PROVENANCE_UNKNOWN; local `model {}` graphs are
      code, never judged)
- [x] Model fingerprint joins the audit chain alongside code and output hashes —
      `AuditRecord.model_fingerprint` + `POST /security/inference/check` accepts an
      optional `model_fingerprint`; covered by the tamper-evident `record_hash`
- Depends on: C3 (lineage) optional, F2 (audit chain extension). Acceptance: a policy that rejects a
  non-commercial-license model at compile time.
  ✅ **Done 2026-09-07**: policy rejects `Llama3-License` at compile time (XZP030) and unregistered
  models fail-closed (XZP031); verified via CLI. 351 workspace tests pass.
  ✅ **Fingerprint-in-audit 2026-09-11** (issue #73): fingerprint recorded and chain-verified;
  tampering with it breaks `verify()`.

---

## Track G — Community scale (docs, releases, playground)

Community expansion is planned and tracked in [COMMUNITY.md](COMMUNITY.md) as
Track G. The steps run in order — docs lower the entry barrier, release and
contribution rules keep the flow steady, then an install-free playground makes
sense. Tracking issue: #155.

### G1. Multi-language docs + Discussions — issues #156 / #157
- [x] GitHub Discussions (Q&A, Ideas, Show and tell, Announcements) opened and
      linked from README, CONTRIBUTING, and the issue chooser (#156)
- [x] `README_ja` / `README_zh` plus a translation guide, English kept as the
      single source of truth (#157)

### G2. Release & contribution guide — issue #158
- [ ] `docs/RELEASING.md` — release cadence, checklist, and a release-note
      template tied to the `CHANGELOG.md` sections
- [ ] Contributor ladder and first-PR / `good first issue` rules in
      `CONTRIBUTING.md`

### G3. Web playground / hosted demo — issue #159
- [x] Local one-command demo (`docker compose up`) with safe/unsafe policy
      examples; no public remote server, abuse limits out of scope (#159)

### G4. Reflect the track in governance — issue #160
- [ ] Track G in this roadmap and the README roadmap table, every item linked
      to its issue

Depends on: nothing hard; G3 builds on E2 (Docker). Acceptance: the public
roadmap and README both show Track G with live issue links, and a newcomer can
go from the README to a merged first PR using only the linked docs.

---

## Recommended execution order

Efficiency rule: **value-per-effort first, then dependency chain.** Do not start C2 before C1, or B3 before B2.

| Step | Issue | Rationale |
| :--- | :--- | :--- |
| 1 | ~~A1 — save + Parquet/Arrow load~~ | ✅ Done — issue #52 |
| 2 | A2 — out-of-core + big benchmark | Proves "scale" with numbers; extends existing bench infra |
| 3 | ~~A4 — import extension~~ | ✅ Done — issue #55 |
| 4 | ~~B1 — module system~~ | ✅ Done — issue #69 |
| 5 | B3 — LSP (#75, diagnostics done) | Reuses checker; biggest DX/visibility win, enables E1 |
| 6 | ~~A3 — connectors (#54)~~ | ✅ Done — DuckDB + PostgreSQL sources |
| 7 | ~~C1 — server persistence~~ | ✅ Done — SQLite run history + /runs API |
| 8 | ~~B2 — stdlib (#56)~~ | ✅ Done — `xazz-stdlib/` embedded modules (`std/common`, `std/math`, `std/models`) |
| 9 | ~~C4 — Python bindings (#61, subprocess adapter)~~ | ✅ Done — `xazz.check/run/policy` from Python; PyO3 deferred |
| 10 | ~~C2 — auth + multi-tenant (#59)~~ | ✅ Done — tenant token auth + scoped /runs + per-tenant DP budget ledger + per-tenant policy packs |
| 11 | D1/D2/D3 — ML | Phase 6; backend trait + `XAZZ_BACKEND` landed (#62/#63); D3 early stopping done (#64). GPU/ONNX providers hardware-gated |
| 12 | ~~F1 — prompt input gate~~ | ✅ Issue #70 open — pure policy-engine extension; biggest GenAI governance win per effort |
| 13 | F2 — output gate (#71) | Depends on F1; completes the request/response audit story |
| 14 | F3 — fine-tuning sanitization (#72) | Depends on F1; pairs with burn-engine LoRA/QLoRA launch |
| 15 | F4 — burn-engine / ONNX interop (#73) | Backend trait landed; provider impls timed to burn-engine release |
| 16 | E1–E4 — ecosystem | Everything downstream of B3/C1/A3 |
| 17 | ~~F5 — model provenance (#74)~~ | ✅ Done — policy registry gate (XZP030/031); fingerprint-in-audit deferred to F4 |

Legend: 🔴 no external dependency | 🟠 depends on an earlier step | 🟢 parallel-friendly

---

## Status tracking

- README roadmap Phase 5/6 rows remain the public status surface.
- Each issue carries a `scale:*` label matching its track (`genai:*` for Track F).
- Update this file and the README table when a milestone's acceptance criteria are met.
- Community expansion (docs, Discussions, releases, playground) is Track G
  above; [COMMUNITY.md](COMMUNITY.md) holds the plan behind it and its issues
  mirror the steps.