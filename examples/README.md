# Xazz examples

Every pipeline here is checked and run by `scripts/smoke_examples.py` (wired into
CI as the **Example smoke** job, #266). Paths below are relative to the repository
root. Build the CLI and its execution binaries first:

```bash
cargo build -p xazz -p xazz-runner -p xazz-exec
```

Some examples read data that is **not committed** — it is either Git LFS data or
produced by a generator. See the *Data* column.

| Example | What it shows | Run | Data / prerequisites |
|---|---|---|---|
| `examples/hello/main.xzz` | Smallest pipeline: load → filter → `groupBy` → bar chart | `cd examples/hello && ../../target/debug/xazz run main.xzz` | `sales.csv` (in the directory) |
| `examples/preprocessing/poc_*.xzz` | Preprocessing operators (`cast`, `filter`, `select`, `count`, `fillNull`) and the Dynamic Schema Bridge | `target/debug/xazz run examples/preprocessing/poc_script.xzz` (from root) | `examples/data/seoul_air_*.csv` (Git LFS) |
| `examples/privacy/dp_aggregation.xzz` | Differential privacy: `withDp(epsilon: …)` noise on aggregates | `target/debug/xazz run examples/privacy/dp_aggregation.xzz` | `examples/data/seoul_air_2026.csv` (Git LFS) |
| `examples/deep_learning/air_pipeline.xzz` | Declarative Burn model + `train`/`predict` pipeline operators | `target/debug/xazz run examples/deep_learning/air_pipeline.xzz` | `examples/data/seoul_air_2024.csv` (Git LFS). Slow: trains a 228K-row dataset |
| `examples/deep_learning/air_quality_predictor.xzz` | Dense/ReLU model, training diagnostics | `target/debug/xazz run examples/deep_learning/air_quality_predictor.xzz` | same as above. Slow |
| `examples/end_to_end/air_quality_full.xzz` | Load → clean → aggregate → chart → train end to end | `target/debug/xazz run examples/end_to_end/air_quality_full.xzz` | `examples/data/seoul_air_2024.csv` (Git LFS). Slow |
| `examples/visualization/viz_bar_chart.xzz` | Bar chart | `target/debug/xazz run examples/visualization/viz_bar_chart.xzz` | none — inline `duckdb://:memory:` source |
| `examples/visualization/viz_line_chart.xzz` | Line chart | `target/debug/xazz run examples/visualization/viz_line_chart.xzz` | none — inline DuckDB |
| `examples/visualization/viz_pie_chart.xzz` | Pie chart | `target/debug/xazz run examples/visualization/viz_pie_chart.xzz` | none — inline DuckDB |
| `examples/visualization/viz_scatter_chart.xzz` | Scatter chart (no direct identifiers) | `target/debug/xazz run examples/visualization/viz_scatter_chart.xzz` | none — inline DuckDB |
| `examples/duckdb/hello_duckdb.xzz` | DuckDB source connector (`duckdb://…?sql=…`) | `target/debug/xazz run examples/duckdb/hello_duckdb.xzz` | none — in-memory DuckDB |
| `examples/duckdb/postgres_demo.xzz` | PostgreSQL source connector | `target/debug/xazz run examples/duckdb/postgres_demo.xzz` | **Needs a local PostgreSQL** (`createdb mydb`). Expected to fail in the smoke harness |
| `examples/semiconductor/main.xzz` | Etch SPC: clean → X-bar chart → control-limit count | `cd examples/semiconductor && python3 generate_wafer_data.py && xazz run main.xzz` | Generate `data/wafer_metrology.csv` first |
| `examples/security/patient_safe.xzz` | Guardrail demo that **passes** the policy check | `target/debug/xazz run examples/security/patient_safe.xzz` | `examples/security/data/patients.csv` (generate with `python3 examples/security/generate_patients.py`) |
| `examples/security/patient_unsafe.xzz` | Guardrail demo **blocked** by policy (`xazz policy` / `xazz run` exit 1) | `target/debug/xazz policy examples/security/patient_unsafe.xzz` | `patients.csv` (generator). Expected to fail on purpose |
| `examples/security/patient_secret_leak.xzz` | Hardcoded-secret violation (`XZP010`/`XZP011`) | `target/debug/xazz policy examples/security/patient_secret_leak.xzz --fix` | none. Expected to fail on purpose |
| `examples/security/prompt_safe.xzz` | GenAI input gate: a safe prompt passes | `target/debug/xazz run examples/security/prompt_safe.xzz` | none — inline DuckDB |
| `examples/security/prompt_unsafe.xzz` | GenAI input gate: injection/jailbreak/exfiltration blocked | `target/debug/xazz policy examples/security/prompt_unsafe.xzz` | none. Expected to fail on purpose |
| `examples/stdlib_import.xzz` | Importing embedded stdlib modules (`import "std/…"`) | `target/debug/xazz run examples/stdlib_import.xzz` | `visual-ide/data/seoul_air_quality.csv` (Git LFS) |

## Data and prerequisites

- **Git LFS.** `*.csv` is tracked with Git LFS (`.gitattributes`). Run `git lfs pull`
  after cloning, or the example data will be pointer files.
- **Generators.** `examples/security/generate_patients.py` and
  `examples/semiconductor/generate_wafer_data.py` write synthetic CSVs that are
  intentionally not committed. The CI smoke job runs both before checking examples.
- **Server / database.** `examples/duckdb/postgres_demo.xzz` needs a local
  PostgreSQL; it is listed as an expected failure in the smoke harness.
- **GPU / ONNX features.** None of these examples need `--features wgpu/cuda/onnx`;
  they run on the default CPU backend.

## Running the smoke check locally

```bash
python3 examples/security/generate_patients.py
python3 examples/semiconductor/generate_wafer_data.py
python3 scripts/smoke_examples.py --xazz target/debug/xazz --timeout 60
```

Training-heavy examples (`deep_learning/`, `end_to_end/`) are skipped by default;
add `--include-heavy` to run them. Examples that are *supposed* to fail are
reported as `XFAIL` and only fail the run if they unexpectedly pass (`XPASS`).
