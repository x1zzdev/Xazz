# Xazz — Scale Roadmap

> 최신 우선순위와 이슈 링크의 단일 소스. 각 작업 항목은 GitHub issue 로 트래킹된다.
> This document is the single source for the scale roadmap: each work item maps to a
> GitHub issue and to a track in the README's Phase 5/6 plan.

---

## Why scale?

Xazz is already a **correct, secure, documented** compiler+runtime: Typed IR (single-pass,
backend-independent), 3-gate policy guardrails, (ε,δ) DP accounting, SHA-256 audit chain,
and a 2.6×-vs-pandas benchmark. What it is not yet is **scalable** — in four distinct senses:

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
- Semantics: module = plain `.xzz`; imports resolve relative to importing file; checker runs on the merged AST so duplicate/missing-reference validation spans modules; policy gate scans module sources' literals too.

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
- [ ] Per-tenant policy packs + DP budgets isolated by namespace (extends the tenant scoping to
      policy/budget state — runs isolation is done)
- Depends on: C1. Acceptance: two tenants cannot see each other's runs.
  ✅ **Run isolation done 2026-09-09**: verified end-to-end — tenant-a and tenant-b each see only
  their own runs; cross-tenant GET /runs/:id → 404. Budget isolation remains.

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

### D1. GPU backends
- [ ] `burn-tch` (CUDA) then `burn-wgpu` (cross-vendor) behind a feature flag / `XAZZ_BACKEND`
- [ ] Detect at runtime, fall back to CPU with an explicit warning
- Depends on: none (Burn API is backend-agnostic). Acceptance: same `.xzz` trains on CPU and CUDA with identical reported losses.

### D2. ONNX export/import
- [ ] `TrainedModel` → ONNX export; ONNX → inference without re-training
- [ ] Unlocks ecosystem interop and model serving
- Depends on: D1 (device mapping). Acceptance: exported ONNX runs in onnxruntime with same prediction.

### D3. Model graph expansion
- [ ] CNN/embedding layers, hyperparameter sweep, early stopping, checkpoint versioning
- Depends on: D1. Acceptance: a CNN pipeline trains end-to-end on image-style tabular data.

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
- **E4. Package/registry for policy packs + stdlib modules** (starts from the 3 existing domain policy packs)

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
- [ ] Backend trait at the `MLOp` lowering boundary so Burn stays the first provider but burn-engine /
      ONNX Runtime are swappable behind the same Typed IR (de-risks Burn's pivot toward inference)
- [ ] Runner subprocess keeps both modes: embedded engine (in-process) or remote server
      (mirrors today's `xazz-runner` + `xazz-server` split)
- Depends on: D2 (ONNX) partially, F1–F3 (the guardrails must exist before inference calls are first-class). 
  Acceptance: the same `.xzz` runs inference via embedded burn-engine and via ONNX with identical outputs.

### F5. Model provenance — weights & license metadata guard — issue #74
- [x] `model {}` declarations and `load("hf://...")`-style sources carry license/weights metadata;
      policy can block restricted-license or fingerprinted-unknown weights
      (policy registry `allowed_models` + `denied_licenses` + `require_model_provenance` fail-closed;
      XZP030 MODEL_LICENSE_BLOCKED · XZP031 MODEL_PROVENANCE_UNKNOWN; local `model {}` graphs are
      code, never judged)
- [ ] Model fingerprint joins the audit chain alongside code and output hashes
      (deferred to F4 — happens when the engine records real inference calls)
- Depends on: C3 (lineage) optional, F2 (audit chain extension). Acceptance: a policy that rejects a
  non-commercial-license model at compile time.
  ✅ **Done 2026-09-07**: policy rejects `Llama3-License` at compile time (XZP030) and unregistered
  models fail-closed (XZP031); verified via CLI. 351 workspace tests pass.

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
| 10 | ~~C2 — auth + multi-tenant (#59, run isolation)~~ | ✅ Done — tenant token auth + scoped /runs; budget isolation remains |
| 11 | D1/D2/D3 — ML | Phase 6; mostly independent, GPU hardware availability gates timing |
| 12 | ~~F1 — prompt input gate~~ | ✅ Issue #70 open — pure policy-engine extension; biggest GenAI governance win per effort |
| 13 | F2 — output gate (#71) | Depends on F1; completes the request/response audit story |
| 14 | F3 — fine-tuning sanitization (#72) | Depends on F1; pairs with burn-engine LoRA/QLoRA launch |
| 15 | F4 — burn-engine / ONNX interop (#73) | Depends on D2 + F1–F3; timed to burn-engine release |
| 16 | E1–E4 — ecosystem | Everything downstream of B3/C1/A3 |
| 17 | ~~F5 — model provenance (#74)~~ | ✅ Done — policy registry gate (XZP030/031); fingerprint-in-audit deferred to F4 |

Legend: 🔴 no external dependency | 🟠 depends on an earlier step | 🟢 parallel-friendly

---

## Status tracking

- README roadmap Phase 5/6 rows remain the public status surface.
- Each issue carries a `scale:*` label matching its track (`genai:*` for Track F).
- Update this file and the README table when a milestone's acceptance criteria are met.