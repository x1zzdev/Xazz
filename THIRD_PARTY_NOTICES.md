# Third-party notices

Xazz is licensed under the Apache License, Version 2.0. It depends on open-source
software from the Rust (crates.io) and JavaScript (npm) ecosystems. This file
summarizes the components that are most relevant to redistribution; the complete,
authoritative list is `Cargo.lock`, `visual-ide/package-lock.json`, and the SBOM
in [docs/result_report.md](docs/result_report.md) (붙임1).

Licenses are checked in CI by [`cargo-deny`](deny.toml) (`cargo deny check`), which
fails the build on a disallowed license, an unknown source, or a security advisory.
The allowed license set is permissive only; the single weak-copyleft dependency
(MPL-2.0, `colored`) is scoped to that crate via an explicit exception.

## Rust (engine and server)

| Component | License | Purpose |
|---|---|---|
| Polars | MIT | LazyFrame preprocessing engine |
| Burn, burn-ndarray | MIT | Deep-learning compile/train/infer |
| Apache Arrow (arrow2/arrow-*) | Apache-2.0 | Columnar buffers / zero-copy tensor handoff |
| axum, tokio, tower-http, tower-lsp | MIT | HTTP server, async runtime, LSP |
| serde, serde_json | MIT/Apache-2.0 | Serialization |
| csv, encoding_rs | MIT/Apache-2.0 | CSV parsing, EUC-KR/CP949 decoding |
| sha2 | MIT/Apache-2.0 | SHA-256 audit chain |
| rusqlite (bundled SQLite) | MIT | Run history / policy / DP ledger |
| duckdb, postgres | MIT/Apache-2.0 | DuckDB and PostgreSQL source connectors |
| ort (ONNX Runtime bindings) | MIT/Apache-2.0 | ONNX export/inference (optional) |
| wgpu | MIT/Apache-2.0 | WebGPU backend (optional) |
| tch (LibTorch bindings) | MIT/Apache-2.0 | CUDA backend (optional) |
| colored | MPL-2.0 | CLI colour output (MPL-2.0 exception) |

## JavaScript (Visual IDE)

| Component | License | Purpose |
|---|---|---|
| React, react-dom | MIT | UI runtime |
| @xyflow/react | MIT | Node-based DAG editor |
| lucide-react | ISC | Icons |
| Vite | MIT | Build tool (dev dependency) |
| @playwright/test | Apache-2.0 | E2E/contrast tests (dev dependency) |

## Optional model / serving (not redistributed)

| Component | License | Note |
|---|---|---|
| Qwen2.5-Coder-1.5B | Apache-2.0 | Fetched by the operator's Ollama; no weights bundled |
| Ollama, llama.cpp | MIT | On-premise sLM serving (operator-installed) |
| Unsloth | Apache-2.0 | QLoRA fine-tuning scaffold (R&D, not shipped) |

If you redistribute Xazz, keep the `LICENSE` and this `NOTICE`, and preserve the
upstream license texts of the bundled dependencies as required by their terms.
