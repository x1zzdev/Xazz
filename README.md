<div align="center">

```text
 ██╗  ██╗ █████╗ ███████╗███████╗
 ╚██╗██╔╝██╔══██╗╚══███╔╝╚══███╔╝
  ╚███╔╝ ███████║  ███╔╝   ███╔╝ 
  ██╔██╗ ██╔══██║ ███╔╝   ███╔╝  
 ██╔╝ ██╗██║  ██║███████╗███████╗
 ╚═╝  ╚═╝╚═╝  ╚═╝╚══════╝╚══════╝
```

# Xazz

**A next-generation Rust-based AI pipeline platform unifying Polars preprocessing, Burn deep learning compilation, and static security guardrails into a single DSL.**

*Scripting on the surface. Compiled at its core.*

[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Language: .xzz](https://img.shields.io/badge/Language-.xzz-orange.svg)]()
[![Backend: Polars](https://img.shields.io/badge/Backend-Polars-red.svg)]()
[![DL Engine: Burn](https://img.shields.io/badge/DL%20Engine-Burn-purple.svg)]()
[![Version: v0.2.8](https://img.shields.io/badge/Version-v0.2.8-green.svg)](https://github.com/xazzdev/Xazz/releases)
[![CI](https://github.com/xazzdev/Xazz/actions/workflows/ci.yml/badge.svg)](https://github.com/xazzdev/Xazz/actions/workflows/ci.yml)

[한국어 README](README_kr.md)

</div>

---

## Overview

### Development Purpose

Xazz was developed to structurally resolve runtime type errors and cross-language overhead inherent in Python-based AI pipelines. Built on Rust's strong type system, it validates missing values and type mismatches at the compilation stage, integrates real-time security integrity verification, and aims to be a next-generation open-source AI development environment — where a single DSL script controls everything from data preprocessing to deep learning training, end-to-end.

### About the Project

Xazz is a Rust-based end-to-end DSL platform combining the **Polars** engine and the **Burn** deep learning framework. It preserves the development productivity of a scripting language while providing zero-copy tensor conversion and static-analysis-based null safety. To minimize abnormal input and memory access risks during large-scale data processing, a data-flow-tracking sandbox has been implemented. This project is not a simple library wrapper — it is an independent open-source platform where the parser, AST, data preprocessing, security runtime, and deep learning compilation engine have all been designed and implemented from the ground up.

---

## Key Features

### 1. Compiler Core & Acceleration
- **Compiler Core**: Independent AI scripting language parser and abstract syntax tree (AST) toolchain implementation
- **Deep Learning Compilation Layer**: Integration with **Burn**, a high-performance Rust AI framework, to compile zero-copy tensor operations into deep learning training layers
- **Data Acceleration Engine**: Fusing with the **Polars** engine to transform user preprocessing commands into ultra-fast LazyFrame operation graphs and execute them

### 2. Security & Privacy Guardrails
- **Static Guardrails (Policy-as-Code)**: Real-time detection and blocking of personal information leaks and security compliance violations at the pre-execution stage
- **Privacy R&D**: Research and validation of **Differential Privacy (DP)** algorithms that guarantee mathematical safety through statistical noise injection
- **Built-in sLM Security Assistant**: An on-premise small Language Model (sLM) that automatically corrects blocked code without external leakage and provides violation reason reports

### 3. Visual Console & Monitoring
- **Visual Console UI**: A node-based web IDE built with React and `@xyflow/react` that visualizes data preprocessing and deep learning compilation flows
- **Real-Time Monitoring**: A statistical dashboard for monitoring the privacy budget consumption status of Differential Privacy and computational resource efficiency

### 4. 💎 Reliability Infrastructure
- **Reliability Infrastructure**: Design of a **SHA-256-based audit log** system that permanently preserves all operation histories
- **Global CI/CD**: Automated testing and high-reliability verification environment built on GitHub Actions

---

## Architecture & Crate Structure

Xazz is organized as a modularized Rust workspace.

| Crate | Role & Function |
| :--- | :--- |
| **`xazz`** | CLI binary entry point (`xazz run`, `xazz emit`, etc.) |
| **`xazz-core`** | AST (Abstract Syntax Tree), static type checker, common data types, and tensor definitions |
| **`xazz-compiler`** | `.xzz` DSL script parsing, AST generation, Burn/Polars operation compiler |
| **`xazz-runner`** | Data-flow-tracking security sandboxing and subprocess-isolated runtime |
| **`xazz-exec`** | Polars LazyFrame preprocessing and Burn tensor deep learning execution engine |
| **`xazz-server`** | sLM security correction engine, web console backend, and SHA-256 audit log server |

---

## Expected Effects

- **Maximized Computational Efficiency**: Improved pipeline computation and resource efficiency over existing Python data processing environments through Apache Arrow-based memory layout and Rust runtime
- **Elimination of GPU Resource Waste**: Statically inspecting missing values and types at the compilation stage to pre-detect errors, preventing unnecessary GPU resource loss during large-scale distributed training
- **Enterprise Data Reliability**: Providing a secure infrastructure that allows industries handling sensitive data — such as finance and healthcare — to perform AI training with confidence through zero-overhead security engine and differential privacy
- **Ecosystem Expansion**: Lowering the development entry barrier with a declarative data pipeline DSL, and contributing to activating a Korea-led data engineering open-source ecosystem based on systematic contribution guidelines

---

## Quick Example

**Scenario:** Load air quality CSV data, apply null-safe preprocessing via Polars, and train a deep learning prediction model using Burn with zero-copy tensor conversion.

### Python (pandas + PyTorch)

```python
import pandas as pd
import torch
import torch.nn as nn

# 1. Preprocessing (No static type check, runtime NaN risk)
df = pd.read_csv("air_data.csv")
df["pm10"] = df["pm10"].fillna(df["pm10"].mean())
X = torch.tensor(df[["temp", "humidity"]].values, dtype=torch.float32)
y = torch.tensor(df[["pm10"]].values, dtype=torch.float32)

# 2. PyTorch Model & Training (Involves memory copy & verbose boilerplate)
class Predictor(nn.Module):
    def __init__(self):
        super().__init__()
        self.net = nn.Sequential(nn.Linear(2, 64), nn.ReLU(), nn.Linear(64, 1))
    def forward(self, x): return self.net(x)

model = Predictor()
optimizer = torch.optim.Adam(model.parameters(), lr=0.01)
criterion = nn.MSELoss()
# ... (Verbose training loop required)
```

### Xazz (.xzz)

```xzz
// 1. Schema declaration (Compile-time Null Safety)
type AirData = {
    temp:     float,
    humidity: float,
    pm10:     Option<float>,
}

// 2. Ultra-fast Lazy preprocessing via Polars
v dataset = load("air_data.csv") :: AirData
    |> fillNull("pm10", strategy: "mean")
    |> select(["temp", "humidity", "pm10"])

// 3. Declarative Burn Deep Learning Model
model AirPredictor {
    Dense(64) -> ReLU() -> Dense(1)
}

// 4. Integrated execution with Zero-Copy Tensor conversion
run dataset 
    |> train(AirPredictor, target: "pm10", epochs: 10)
```

| Feature | Python (pandas + PyTorch) | Xazz (.xzz) |
|---------|--------------------------|-------------|
| Pipeline Scope | Fragmented (pandas + PyTorch separately) | Unified End-to-End DSL (Preprocessing to DL) |
| Tensor Conversion | Memory copy overhead (CPU/GPU) | Zero-Copy Tensor Integration (Burn backend) |
| Type & Null Safety | Runtime exception risk (NaN / Type error) | Compile-time Static Guard (`Option<T>`) |
| Model Boilerplate | Manual tensor layout & dimension wiring | Auto-inferred feature dimensions & loss function |

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
| `xazz emit rust` | Transpile `.xzz` → Rust source (Polars LazyFrame + Burn) | Stable |
| `model {}` + `train()` | Burn deep-learning model declaration & training (Adam + MSE, CPU backend) | Stable |
| `xazz check` | Static analysis via Neural Query Planner | Experimental |
| `xazz sde` | Synthetic data generation engine integration | Preview |
| Built-in `chart {}` | Render pipeline results as bar / line / pie / scatter | Stable |
| `Option<T>` type system | Null-safe column declarations, `fillNull` operator | Stable |
| `fillNull(strategy:)` | Mean / median / zero fill strategies (`strategy: "mean"`) | Stable |
| EUC-KR CSV support | Auto-detect and decode CP949-encoded Korean CSV files | Stable |
| Visual IDE | Graphical pipeline editor (separate repository) | Stable |

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

![Xazz Benchmark](benches/x1zzLang_benchmark2.png)

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
| Phase 5 — AI Expansion | Burn deep-learning layer (model declaration, training, checkpoints), NQP | DL complete / NQP Experimental |

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
