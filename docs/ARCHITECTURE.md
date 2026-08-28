# Xazz — Architecture Overview

This document describes the compiler pipeline, type system, IR, and execution model of Xazz.

For workspace structure and dependency graph, see [WORKSPACE.md](WORKSPACE.md).

---

## Compilation Pipeline

A `.xzz` source file goes through the following stages:

```
.xzz source
    │
    ▼
┌──────────────────────────────────────────────────────────────────────┐
│  xazz-compiler (frontend)                                            │
│                                                                      │
│  1. Lexer (lexer.rs)                                                 │
│     Source text → Token stream                                       │
│                                                                      │
│  2. Parser (parser.rs)                                               │
│     Token stream → raw AST (Program)                                 │
│                                                                      │
│  3. Semantic analysis (checker.rs) + IR generation                   │
│     raw AST → diagnostics + TypedProgram (Typed IR)                  │
│     • 진단과 IR 은 **단일 순회**로 생성 (이중 추론 없음)                │
│     • 컬럼 수준 타입 추론, 미선언 변수/컬럼, did-you-mean, Span        │
│                                                                      │
│  4. Optimization (opt.rs) — 선택 (xazz-exec --opt)                   │
│     TypedProgram → TypedProgram                                      │
│     상수 폴딩 / 연속 Select 병합 / 조건 푸시다운                       │
│                                                                      │
├─── (다음: 백엔드 lowering — xazz-exec) ───────────────────────────────┤
│                                                                      │
│  5. lower.rs     DataOp → Polars LazyFrame                           │
│  6. dl.rs        MLOp   → Burn tensors                               │
│  7. dp.rs        withDp → DP noise + composition accounting          │
│  8. chart.rs     chart  → JSON spec → HTML (Chart.js)                │
└──────────────────────────────────────────────────────────────────────┘
```

The critical property of this design: **the compiler produces a typed IR once,
and the runtime consumes that IR once.** The old architecture re-lexed and
re-parsed the source inside the runtime and interpreted the raw AST directly
against Polars/Burn — a double interpretation that made the AST the shared
source of truth for both frontend and backend concerns.

---

## Typed IR (xazz-core::ir)

`xazz-core` provides the shared kernel with zero heavy dependencies. The IR types
live in `xazz-core/src/ir.rs` and are consumed by the compiler (producer) and the
runtime (consumer). See [docs/design/ir.md](design/ir.md) for the full design.

| IR type | Purpose |
|---------|---------|
| `ColType` | `String / Int / Float / Bool / Unknown / Nullable<T>` |
| `Schema` / `SchemaField` | named column lists for pipeline input/output |
| `TypedExpr` | expression + resulting `ColType` (every expression is typed) |
| `DataOp` | data-domain operations (`Filter`, `Select`, `GroupBy`, `Aggregate`, `Join`, `WithColumn`, `Cast`, …) |
| `MLOp` | `Train` / `Predict` |
| `SideOp` | `Chart` / `WithDp` |
| `Step` | domain-tagged step preserving **pipeline order** (`filter |> withDp |> select` ≠ `filter |> select |> withDp`) |
| `PipelineNode` | one pipeline: source, input/output schema, ordered steps, yields_model |
| `ModelGraph` | `model Name { ... }` layer list (lowered to Burn) |
| `TypedProgram` | types + models + pipelines for a whole file |

### Why domain-split steps?

Data, ML and privacy/visualization operations have different evaluation models
(columnar data vs. tensor graphs vs. output perturbation). Keeping them in
separate enums lets each backend lowering know only its own domain, instead of
one giant `PipelineOp` carrying every concern.

---

## Type System

Xazz uses a structural type system with explicit null-safety.

### Column types

| Xazz type | Polars equivalent | Notes |
|---------------|-------------------|-------|
| `string` | `Utf8` | UTF-8 string column |
| `float` | `Float64` | 64-bit float |
| `int` | `Int64` | 64-bit integer |
| `bool` | `Boolean` | Boolean column |
| `Option<T>` | nullable `T` | Marks a column as potentially null |

### Null safety

`Option<T>` is the only way to declare a nullable column. A column declared as
`float` is treated as non-nullable. The `fillNull` operator on a non-Option
column is a type error.

```xzz
type Record = {
  station: string,          -- non-nullable
  pm10:    Option<float>,   -- nullable: missing values permitted
}
```

### Type annotation

The `:: TypeName` annotation on `load()` binds a schema to a data source:

```xzz
v data = load("file.csv") :: Record
```

This makes schema violations detectable before execution, and the resulting
column types flow through the pipeline as a `Schema` on every `PipelineNode`.

---

## Optimization (xazz-compiler::opt)

Optimization runs on the Typed IR, so it is backend-independent and can be
proven semantics-preserving at the language level:

- **Constant folding** — literal `BinOp` expressions are evaluated at compile
  time. Division by a literal zero is deliberately not folded (runtime
  semantics preserved).
- **Projection pruning** — consecutive `Select`s are merged (`Select(A) |> Select(B)`
  → `Select(B)`).
- **Predicate pushdown** — a `Filter` is moved before a `Select` (or a `WithColumn`)
  when the filter's columns are preserved by that boundary.

Each pass is covered by structural unit tests, and an execution-equivalence test
in `xazz-exec` proves the reordered pipeline yields the same DataFrame.

Polars' `LazyFrame` also performs internal predicate pushdown / column pruning;
the IR optimizer defines these transformations explicitly at the language layer
for backend independence (future backends benefit without re-deriving them).

---

## Execution Model

`xazz run` does not execute the pipeline directly. It delegates to `xazz-runner`
via subprocess:

```
xazz run file.xzz
  │
  ├── compile .xzz → Typed Program (in-process, xazz-compiler)
  │
  └── spawn xazz-runner file.xzz [--verbose] [--opt]
       │
       └── xazz-exec: analyze_program() → ir → execute_node()
            │
            ├── resolve source (CSV → schema cast/bridge)
            ├── Step::Data  → lower::lower_data (Polars LazyFrame)
            ├── Step::ML    → dl::train / dl::predict (Burn)
            ├── Step::Side  → chart / DP (with ε/δ composition accounting)
            └── collect → result markers ([xazz:result], [xazz:train], …)
```

**Why subprocess?** Polars adds ~28 MB to a binary. Isolating it to `xazz-exec`
keeps the `xazz` CLI binary at ~2–5 MB. The tradeoff is that `xazz-runner` must
exist alongside `xazz` in the same directory (or `XAZZ_RUNNER_PATH`).

> **Security terminology:** the subprocess boundary is **process isolation**, not
> an OS sandbox. `xazz-runner` enforces a configurable execution timeout
> (`XAZZ_EXEC_TIMEOUT_SECS`, default 300s) as lightweight hardening, but OS-level
> sandboxing (seccomp/landlock) is tracked as a separate milestone. See
> [docs/design/security-model.md](design/security-model.md).

---

## Memory Model (Polars ↔ Burn)

Data crossing from Polars into Burn goes through `xazz-exec/src/tensor_bridge.rs`:

```
Polars (Arrow columnar)                 Burn TensorData
   Float64/Float32 contiguous ─────────► direct raw-buffer read (cont_slice)
   Int / has-nulls ────────────────────► cast + null→NaN path
          │                                     │
          +────────────── columnar → row-major [n, d] ────────────+
```

The unavoidable copy boundaries are the f64→f32 precision downgrade (the Burn
CPU backend is f32), the columnar→row-major rearrangement (Burn `TensorData`
owns a row-major `Vec`), and host→device transfer (owned by `Tensor::from_data`).
For already-contiguous `Float64/Float32` columns, `cont_slice()` reads the raw
Arrow buffer and skips the intermediate cast + per-column `Vec`. See
[docs/design/memory-model.md](design/memory-model.md).

---

## Differential Privacy

`withDp(epsilon:, mechanism:, ...)` applies output perturbation (Laplace /
Gaussian). Privacy accounting uses **sequential composition**: each query
records `(εᵢ, δᵢ)`; `PrivacyBudget` accumulates `Σεᵢ` and `Σδᵢ` and refuses any
query that would exceed either the total ε budget (`XAZZ_DP_BUDGET`, default
10.0) or the total δ budget (`XAZZ_DP_DELTA_BUDGET`, default 1e-4). Laplace is
pure ε-DP (δ contribution 0); Gaussian is (ε, δ)-DP and consumes both. See
[docs/design/dp-spec.md](design/dp-spec.md).

---

## Neural Query Planner (NQP) — Experimental

`xazz check` runs the real static semantic analyzer. The Neural Query Planner
remains a planned research layer (Phase 5): semantic analysis of pipeline
structure, deeper column-level inference, null-flow tracking, and query-plan
suggestions on top of the Typed IR.

---

## Security & Guardrails

Policy-as-Code guardrails scan the pipeline before execution — PII patterns
(RRN, phone, card numbers with Luhn validation), secrets, custom rules. The
guardrail is enforced at three gates: CLI (`xazz run`), execution engine
(`xazz-exec` STEP 3.6, the final gate), and API (`POST /execute`). See
[docs/SECURITY_GUARDRAIL.md](SECURITY_GUARDRAIL.md).

---

## xazz emit rust

`xazz emit rust file.xzz` transpiles a `.xzz` script to Rust source that
directly calls the Polars LazyFrame API (plus Burn for `model {}`/`train()`).
This is a standalone introspection/embedding path — it is independent of the
runtime execution path, which now consumes the Typed IR.

---

## Audit & Results Markers

`xazz-exec` emits line-prefixed JSON markers on stdout that the CLI, server, and
Visual IDE parse:

| Marker | Payload |
|--------|---------|
| `[xazz:result]` | final DataFrame rows + schema |
| `[xazz:diagnostics]` | type-checker errors/warnings |
| `[xazz:policy]` | guardrail report (always emitted) |
| `[xazz:train]` | Burn training report |
| `[xazz:model]` | model declaration metadata |
| `[xazz:dp]` | DP noise report + ε/δ budget/composition state |
| `[xazz:chart]` | chart JSON spec |

`xazz-server` records an append-only SHA-256 hash-chained audit log and exposes
`/security/audit` and `/security/verify`.