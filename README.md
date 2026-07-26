<div align="center">

```text
 ██╗  ██╗ ██╗ ███████╗███████╗██╗      █████╗ ███╗   ██╗ ██████╗ 
 ╚██╗██╔╝███║ ╚══███╔╝╚══███╔╝██║     ██╔══██╗████╗  ██║██╔════╝ 
  ╚███╔╝ ╚██║   ███╔╝   ███╔╝ ██║     ███████║██╔██╗ ██║██║  ███╗
  ██╔██╗  ██║  ███╔╝   ███╔╝  ██║     ██╔══██║██║╚██╗██║██║   ██║
 ██╔╝ ██╗ ██║ ███████╗███████╗███████╗██║  ██║██║ ╚████║╚██████╔╝
 ╚═╝  ╚═╝ ╚═╝ ╚══════╝╚══════╝╚══════╝╚═╝  ╚═╝╚═╝  ╚═══╝ ╚═════╝ 
```

# Xazz

**A Rust-based DSL platform exploring data analysis accessibility.**  
*Scripting on the surface. Compiled at its core.*

[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Language: .xzz](https://img.shields.io/badge/Language-.xzz-orange.svg)]()
[![Backend: Polars](https://img.shields.io/badge/Backend-Polars-red.svg)]()
[![Version: v0.2.8](https://img.shields.io/badge/Version-v0.2.8-green.svg)](https://github.com/xazzdev/Xazz/releases)
[![CI](https://github.com/xazzdev/Xazz/actions/workflows/ci.yml/badge.svg)](https://github.com/xazzdev/Xazz/actions/workflows/ci.yml)

[한국어 README](README_kr.md)

</div>

---

## Project Overview

Xazz is a domain-specific language (DSL) designed to explore how data analysis tooling can be made more accessible. The language compiles `.xzz` scripts into optimized [Polars](https://github.com/pola-rs/polars) LazyFrame execution plans via a Rust-based compiler pipeline.

This project is primarily an exercise in **language design**, **compiler engineering**, and **type system research** — not a production-ready replacement for existing data analysis tools.

**What this project demonstrates:**
- A declarative pipeline DSL with a null-safe type system (`Option<T>`)
- A multi-crate Rust workspace that isolates heavy dependencies (Polars) from the CLI binary
- An auto schema inference tool (`xazz import`) that generates type definitions from CSV files
- A Visual IDE for graphical pipeline editing

---

## Core Idea

Most data analysis workflows require environment setup before touching a single row of data: install Python, install libraries, configure a virtual environment, infer column types by hand, handle nulls explicitly.

Xazz explores a different approach: schema is declared upfront in the type system, null-safety is enforced at the type level, and the pipeline is expressed as a composition of named operations.

```
Type declaration → Pipeline composition → Compiled execution
```

The goal is not to replace existing tools but to investigate what a purpose-built, type-safe data pipeline language looks like and how far that design can go.

---

## Quick Example

**Scenario:** Filter and aggregate air quality data from a CSV file.

### Python (pandas)

```python
import pandas as pd

df = pd.read_csv("data.csv")
df = df[df["pm10"] > 50]
result = df.groupby("station")["pm10"].mean()
print(result)
```

*Requires library installation. Type errors surface at runtime. Null handling is manual.*

### Xazz

```xzz
type AirQuality = {
  station: string,
  pm10:    Option<float>,
}

v data = load("data.csv") :: AirQuality
  |> cast("pm10", "float")
  |> filter(pm10 > 50)
  |> groupBy("station")
  |> mean("pm10")
```

*No imports. Schema declared upfront. Null-safe via `Option<T>`.*

| | Python (pandas) | Xazz |
|--|-----------------|----------|
| Library dependencies | `pandas`, `numpy` | None (built-in) |
| Type validation | Runtime | Schema declaration time |
| Null handling | Manual NaN checks | `Option<T>` in type definition |

**From `xazz import` to running pipeline:**

```bash
xazz new my-project    # scaffold project + sample CSV
cd my-project
xazz import data.csv   # auto-infer schema → write type block to main.xzz
xazz run main.xzz      # compile + execute pipeline
```

---

## Result Preview

After running a pipeline with a `chart {}` block, Xazz renders the result as an HTML chart:

![Xazz chart](screenshot_result_chart.png)

> *Example: pipeline execution result rendered as a bar chart. Chart output is written to an HTML file.*

---

## Visual IDE

[![Xazz Visual IDE](screenshot_visual_ide.png)](https://github.com/xazzdev/Xazz-visual-ide)

A graphical editing and execution environment for `.xzz` pipelines.  
→ [Xazz Visual IDE repository](https://github.com/xazzdev/Xazz-visual-ide)

---

## Features

| Feature | Description | Status |
|---------|-------------|--------|
| `xazz run` | Compile and execute `.xzz` pipeline | Stable |
| `xazz import` | Auto-infer CSV schema → generate type block | Stable |
| `xazz new` | Scaffold project with sample CSV and runnable example | Stable |
| `xazz emit rust` | Transpile `.xzz` → Rust source (Polars LazyFrame calls) | Stable |
| `xazz check` | Static analysis via Neural Query Planner | Experimental |
| `xazz sde` | Synthetic data generation engine integration | Preview |
| Built-in `chart {}` | Render pipeline results as bar / line / pie / scatter | Stable |
| `Option<T>` type system | Null-safe column declarations, `fillNull` operator | Stable |
| EUC-KR CSV support | Auto-detect and decode CP949-encoded Korean CSV files | Stable |
| Visual IDE | Graphical pipeline editor (separate repository) | Stable |

---

## Architecture

Xazz is structured as a Cargo workspace with intentional dependency isolation. The CLI binary does not link Polars or Tokio — those are isolated to the execution engine binary (`xazz-runner` / `xazz-exec`).

```
xazz (CLI binary)
│  clap + indicatif + colored + csv + anyhow + encoding_rs
│  NO Polars  ·  NO Tokio
│
├── xazz-compiler          Lexer → Parser → Codegen → Emitter
│   └── xazz-core          Shared AST / Token / Error types (serde only)
│
└── [subprocess spawn] ──► xazz-runner
                           │
                           └── xazz-exec       Polars LazyFrame runtime
```

**Crate responsibilities:**

| Crate | Role | Heavy deps |
|-------|------|------------|
| `xazz` (CLI) | Argument parsing, import, new, emit, check | None |
| `xazz-core` | Shared AST, Token, Error types | serde only |
| `xazz-compiler` | Lexer / Parser / Codegen / Emitter | None |
| `xazz-exec` | Polars execution engine | **Polars, encoding_rs** |
| `xazz-runner` | Execution binary (spawned by CLI) | via xazz-exec |
| `xazz-sde` | Synthetic data generation (standalone) | polars, rayon |
| `xazz-server` | REST API server (standalone) | axum, tokio |

**Why this structure?**  
The CLI binary stays small (~2–5 MB) because it never links Polars. When `xazz run` is called, it spawns `xazz-runner` as a subprocess — the runner carries all the heavy Polars dependencies independently. Communication between them uses only CLI arguments (no IPC protocol).

**Binary size trade-off:**

| Binary | Approx. Size | Contains |
|--------|-------------|----------|
| `xazz` (CLI) | ~2–5 MB | Compiler, schema inference, project scaffolding |
| `xazz-runner` | ~30+ MB | Polars execution engine |

For more detail, see [docs/WORKSPACE.md](docs/WORKSPACE.md).

---

## Installation

### Option A — Pre-built release (recommended)

1. Download the latest release archive for your platform from [Releases](https://github.com/xazzdev/Xazz/releases).

   | Platform | Archive |
   |----------|---------|
   | Windows x64 | `xazz-<version>-windows-x64.zip` |
   | Linux x64 | `xazz-<version>-linux-x64.tar.gz` |
   | macOS arm64 | `xazz-<version>-macos-arm64.tar.gz` |

2. Extract the archive. You will find `xazz` and `xazz-runner` in the same directory.

   > **Important:** Both binaries must remain in the same directory. `xazz run` spawns `xazz-runner` as a subprocess — if `xazz-runner` is missing, pipeline execution will fail.

3. Add the extracted directory to your `PATH`.

4. Verify:

   ```bash
   xazz --help
   ```

### Option B — Build from source

Requires Rust stable toolchain.

```bash
git clone https://github.com/xazzdev/Xazz.git
cd Xazz

# Build CLI
cargo build --release -p xazz

# Build execution engine
cargo build --release -p xazz-runner

# Both binaries land in target/release/
```

Place both `xazz` and `xazz-runner` in the same directory before use.

---

## Benchmark

![Xazz Benchmark](benches/Xazz_benchmark2.png)

The benchmark compares Xazz against an equivalent pandas pipeline on a 3.4M-row Seoul air quality dataset.

> Xazz achieved up to **3.84× faster** execution than the pandas baseline on this workload.

This performance comes primarily from the Polars LazyFrame backend, which applies query optimization before execution. The benchmark is measuring end-to-end pipeline throughput, not compiler overhead.

Benchmark source: [`benches/run_benchmark.py`](benches/run_benchmark.py) / [`benches/benchmark_pipeline.xzz`](benches/benchmark_pipeline.xzz)

---

## Roadmap

| Phase | Goal | Status |
|-------|------|--------|
| Phase 1 — Core Language | DSL syntax, type system, compiler pipeline | Complete |
| Phase 2 — Execution Layer | Polars integration, CLI tooling, chart output | Complete |
| Phase 3 — IDE Integration | Visual IDE, graphical pipeline editor | Complete |
| Phase 4 — Expanded Language | More operators, join improvements, schema evolution | In progress |
| Phase 5 — AI Expansion | Natural language query interface (NQP), AI-augmented analysis | Experimental |

---

## Contributing

Xazz is an open-source project. Bug reports, ideas, and discussions via GitHub Issues are always welcome.

**Note on code contributions (Pull Requests):**  
To maintain authorship integrity during the 8th Korea CodeFair 2026 evaluation period, code contributions (Pull Requests) are temporarily suspended until October 2026. PRs will reopen after the competition concludes.

- Issues (bug reports, ideas, discussion): Open
- Pull Requests: Suspended until October 2026

See [CONTRIBUTING.md](CONTRIBUTING.md) for local build instructions and contribution guidelines.

---

## License

Apache-2.0 — see [LICENSE](LICENSE) for details.

---

<div align="center">

**Xazz — 2026**

</div>
