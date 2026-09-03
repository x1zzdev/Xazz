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

### A2. Out-of-core / streaming execution + large-scale benchmark
- [ ] Switch CSV/Parquet load to `LazyFrame::scan_*` + Polars `streaming` feature
- [ ] Extend `benches/` scale suite from 4.09M rows to 10M / 50M / 200M rows (synthetic)
- [ ] Document peak-RSS vs latency tradeoff already noted in README
- Depends on: A1. Acceptance: benchmark chart shows sub-linear latency growth.

### A3. External source connectors
- [ ] Embedded DuckDB source (`load("duckdb://...")`), SQL text in `.xzz`
- [ ] PostgreSQL read connector behind a `load("postgres://...")` source attribute
- Depends on: A2. Acceptance: one `.xzz` file mixing CSV + DB sources in a single pipeline.

### A4. `xazz import` extension — Parquet/Arrow/DB schema inference
- [ ] `xazz import data.parquet` writes the same inferred `type` block today's CSV path does
- [ ] Interactive column → field mapping surfaced (feeds R-005 in the IDE)
- Depends on: A1. Acceptance: import round-trips into a runnable pipeline.

---

## Track B — Language scale (reuse units)

### B1. Module system
- [ ] `import "./preprocess.xzz"` — split pipelines across files
- [ ] Named reusable pipelines (`fn` / named pipeline definitions) callable from any file
- [ ] Module-level `type` declarations shared across files
- Acceptance: a 2-file project where `main.xzz` imports a data-prep module and type-checks against it.

### B2. Standard library crate (`xazz-stdlib`)
- [ ] New workspace member with date/string/statistics helpers implemented in `.xzz`-visible operators
- [ ] Versioned with the workspace; docs generated
- Depends on: B1. Acceptance: `import "std/math"` usable in demos.

### B3. LSP server (`xazz-lsp`)
- [ ] New crate (`tower-lsp`) exposing diagnostics, hover, go-to-def, rename by **reusing the checker**
- [ ] The Typed IR schema is already available to drive column-aware hover/autocomplete
- Acceptance: diagnostics in VS Code match `xazz check` line:col output exactly.

---

## Track C — Platform scale (team / org reach)

### C1. Server persistence + run history
- [ ] SQLite storage in `xazz-server`: projects, run history, receipts (SHA-256 chain today is append-only JSONL)
- [ ] `GET /runs` list + `GET /runs/:id` receipt replay
- Depends on: none (server is already independent). Acceptance: run history survives restart and is queryable via API.

### C2. Auth / multi-tenant
- [ ] Token-based auth beyond loopback (`XAZZ_SERVER_TOKEN` exists as a skeleton)
- [ ] Per-tenant policy packs + DP budgets isolated by namespace
- Depends on: C1. Acceptance: two tenants cannot see each other's runs or budgets.

### C3. Pipeline catalog + lineage
- [ ] Dataset registration and column-level lineage derived from the IR's flowing `Schema`
- [ ] The architecture already models schema on every `PipelineNode` — lineage is a query over it
- Depends on: C1. Acceptance: a reviewer can trace `groupBy → agg → chart` output columns back to source columns.

### C4. Python bindings (PyO3)
- [ ] `pip install xazz` — expose compile/check/run as Python functions (replace the ad-hoc `python/xazz_dp.py`)
- [ ] Numpy/Pandas in → Arrow out handoff on the Python boundary
- Depends on: B3 (shared checker ergonomics), C1 optional. Acceptance: `xazz.check(src)` returns the same diagnostics as the CLI.

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

- **E1. VS Code extension** — LSP client + runner integration (depends on B3)
- **E2. Official Docker image** — `xazz-server` + runner + IDE bundled, mountable data dir
- **E3. GitHub Actions official action** — `xazz check`/`run` in CI with policy gate
- **E4. Package/registry for policy packs + stdlib modules** (starts from the 3 existing domain policy packs)

---

## Recommended execution order

Efficiency rule: **value-per-effort first, then dependency chain.** Do not start C2 before C1, or B3 before B2.

| Step | Issue | Rationale |
| :--- | :--- | :--- |
| 1 | ~~A1 — save + Parquet/Arrow load~~ | ✅ Done — issue #52 |
| 2 | A2 — out-of-core + big benchmark | Proves "scale" with numbers; extends existing bench infra |
| 3 | A4 — import extension | Reuses A1's loader; completes the load→schema→run loop for new formats |
| 4 | B1 — module system | Unlocks reuse; prerequisite for stdlib |
| 5 | B3 — LSP | Reuses checker; biggest DX/visibility win, enables E1 |
| 6 | A3 — connectors | Independent of language work; adds DuckDB/Postgres sources |
| 7 | C1 — server persistence | Foundation for C2/C3; independent of A/B |
| 8 | B2 — stdlib | Needs B1 |
| 9 | C4 — Python bindings | Big adoption lever; best after LSP/B1 ergonomics |
| 10 | C2/C3 — auth + lineage | Both depend on C1 |
| 11 | D1/D2/D3 — ML | Phase 6; mostly independent, GPU hardware availability gates timing |
| 12 | E1–E4 — ecosystem | Everything downstream of B3/C1/A3 |

Legend: 🔴 no external dependency | 🟠 depends on an earlier step | 🟢 parallel-friendly

---

## Status tracking

- README roadmap Phase 5/6 rows remain the public status surface.
- Each issue carries a `scale:*` label matching its track.
- Update this file and the README table when a milestone's acceptance criteria are met.