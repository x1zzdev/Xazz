# Xazz — Architecture Overview

This document describes the compiler pipeline, type system, and execution model of Xazz.

For workspace structure and dependency graph, see [WORKSPACE.md](WORKSPACE.md).

---

## Compilation Pipeline

A `.xzz` source file goes through the following stages:

```
.xzz source
    │
    ▼
┌──────────────────────────────────────────────────────────────────┐
│  xazz-compiler                                                   │
│                                                                  │
│  1. Lexer (lexer.rs)                                             │
│     Source text → Token stream                                   │
│     Handles: keywords, identifiers, operators, string literals   │
│                                                                  │
│  2. Parser (parser.rs)                                           │
│     Token stream → AST                                           │
│     Produces: TypeDecl, VarDecl, PipelineExpr, ChartBlock        │
│                                                                  │
│  3. Codegen (codegen.rs)                                         │
│     AST → IR (intermediate representation)                       │
│     Resolves: type bindings, pipeline chain structure            │
│                                                                  │
│  4. Emitter (emitter.rs)                                         │
│     IR → Rust source (Polars LazyFrame API calls)                │
│     Output: compilable Rust file                                 │
└──────────────────────────────────────────────────────────────────┘
    │
    ▼  (xazz run path)
┌──────────────────────────────────────────────────────────────────┐
│  xazz-exec (via xazz-runner subprocess)                         │
│                                                                  │
│  run_pipeline() — interprets compiled IR with Polars             │
│  LazyFrame execution: filter, groupBy, join, sort, ...           │
│  Chart rendering: HTML output via charting library               │
└──────────────────────────────────────────────────────────────────┘
    │
    ▼
Result: terminal output / CSV export / HTML chart
```

---

## AST Structure

Core AST nodes (`xazz-core/src/ast.rs`):

| Node | Description |
|------|-------------|
| `TypeDecl` | `type Name = { field: Type, ... }` — struct-like schema declaration |
| `VarDecl` | `v name = expr` — pipeline variable binding |
| `ExprStmt` | Expression statement (result discarded) |
| `PipelineOp` | Individual operator: `filter`, `groupBy`, `join`, `sort`, `select`, `cast`, `withColumn`, `rename`, `mean`, `fillNull`, `sample`, `median`, `variance`, `std` |
| `ChartConfig` | `chart { type: bar, x: ..., y: ... }` — visualization configuration |
| `Expr` | Expression: column reference, binary op, literal, function call |
| **v0.3 Deep Learning** | |
| `ModelDecl` | `model Name { Dense(64) -> ReLU() -> Dense(1) }` — neural network declaration |
| `TrainStmt` | `run data \|> train(Model, target: "col", epochs: 10)` — training statement |
| `LayerKind` | `Dense(usize)`, `ReLU`, `Sigmoid`, `Tanh`, `Softmax`, `Dropout(f64)`, `BatchNorm` |
| `TrainConfig` | Hyperparameters: `target`, `epochs`, `learning_rate`, `batch_size`, `validation_split` |

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

`Option<T>` is the only way to declare a nullable column. A column declared as `float` is treated as non-nullable. The `fillNull` operator on a non-Option column is a type error.

Example:
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

This makes schema violations detectable before execution.

---

## Execution Model

`xazz run` does not execute the pipeline directly. It delegates to `xazz-runner` via subprocess:

```
xazz run file.xzz
  │
  ├── compile .xzz → IR (in-process, xazz-compiler)
  │
  └── spawn xazz-runner file.xzz [--verbose] [--output path]
       │
       └── xazz-exec: run_pipeline(ir, data_path, output_path)
            │
            └── Polars LazyFrame: scan_csv → filter → groupBy → collect
                 │
                 └── chart rendering → HTML output
```

**Why subprocess?**  
Polars adds ~28 MB to a binary. Isolating it to `xazz-runner` keeps the `xazz` CLI binary at ~2–5 MB. The CLI stays fast to start and install. The tradeoff is that `xazz-runner` must exist alongside `xazz` in the same directory.

---

## Neural Query Planner (NQP) — Experimental

`xazz check` invokes the Neural Query Planner, a planned static analysis layer that is currently in stub/experimental state.

The intended design:
- Semantic analysis of pipeline structure
- Column-level type inference across pipeline steps
- Null-flow tracking: detecting unhandled `Option<T>` columns at consumption sites
- Query plan suggestions

Current status: experimental stub. The check command outputs a mock report for demonstration. Full NQP implementation is a Phase 5 goal.

---

## Synthetic Data Engine (SDE) — Preview

`Xazz-sde` is a standalone crate (`xazz-sde/`) for generating synthetic CSV datasets conforming to a given schema.

It is not part of the main CLI dependency graph. The `xazz sde` CLI subcommand currently prints a preview notice — full integration is planned.

Intended features:
- Schema-driven row generation
- Statistical distribution parameters (range, null rate, cardinality)
- Output: CSV compatible with `xazz import`

---

## Chart Output

The `chart {}` block in a pipeline triggers chart rendering at the end of pipeline execution:

```xzz
v result = load("data.csv") :: T
  |> filter(pm10 > 50)
  |> groupBy("station")
  |> mean("pm10")

chart {
  kind:  bar,
  x:     station,
  y:     pm10,
  title: "PM10 by Station",
}
```

Output: an HTML file containing an interactive chart. The chart renderer runs inside `xazz-exec` after the Polars pipeline completes.

---

## Deep Learning DSL (v0.3)

Xazz supports declarative neural network definition and training via the `model` and `run |> train()` constructs.

### Model Declaration

```xzz
model AirQualityNet {
  Dense(64) -> ReLU() -> Dense(32) -> ReLU() -> Dense(1)
}
```

This produces a `Stmt::ModelDecl` AST node containing a `Vec<LayerKind>`. The codegen generates Burn-compatible layer descriptions.

### Training Statement

```xzz
v data = load("air.csv") :: AirQuality
  |> dropNull("pm10")
  |> select(["pm10", "pm25", "temperature"])

run data |> train(AirQualityNet, target: "pm10", epochs: 50, lr: 0.001)
```

This produces a `Stmt::TrainStmt` AST node with a `TrainConfig` struct.

### Execution

The runtime (`xazz-exec`) currently logs model architecture and training config as placeholders. Full Burn integration (Autodiff + DataLoader) is planned for a future release.

### Layer Types

| Xazz Layer | Burn Equivalent |
|------------|-----------------|
| `Dense(n)` | `nn::LinearConfig::new(n, n)` |
| `ReLU()` | `nn::ReLU::new()` |
| `Sigmoid()` | `nn::Sigmoid::new()` |
| `Tanh()` | `nn::Tanh::new()` |
| `Softmax()` | `nn::Softmax::new()` |
| `Dropout(r)` | `nn::DropoutConfig::new(r)` |
| `BatchNorm()` | `nn::BatchNormConfig::new()` |

---

## xazz emit rust

`xazz emit rust file.xzz` transpiles a `.xzz` script to Rust source code that directly calls the Polars LazyFrame API. This output is primarily useful for:

- Inspecting how Xazz maps DSL constructs to Polars operations
- Embedding pipeline logic into a larger Rust project
- Debugging codegen output

The emitted Rust code can be compiled with `cargo` independently of Xazz.

---

## Security & Audit (v0.3)

The `xazz-server` provides security endpoints for code integrity verification:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Server health check |
| `/security/audit` | POST | Generate SHA-256 hash of DSL code |
| `/security/verify` | POST | Verify SHA-256 hash of DSL code |

These endpoints enable audit logging and tamper detection for pipeline scripts.
