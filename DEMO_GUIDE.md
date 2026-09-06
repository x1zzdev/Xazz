# Xazz — Demo Guide

This guide walks through the Xazz demo end to end: what to install, what to launch, and what each example actually does. All commands assume you are running from the repository root.

---

## What Xazz is

Xazz is a Rust-based DSL for building AI and data pipelines. You write a pipeline in a small script (`.xzz`), and before anything runs, the compiler statically checks types and missing values. At runtime the pipeline does the actual work:

- Data preparation with Polars (lazy frames, null handling, grouping)
- Chart rendering
- Deep learning training through the Burn engine
- Differential privacy noise on aggregate results
- An audit log (SHA-256 hashes) of every run

"Looks like a script, compiles like a program" is the short version.

---

## What you need before starting

- A Rust toolchain (`cargo`) if the release binaries are not built yet
- Node.js + npm for the Visual IDE frontend
- Ports `8005` (backend) and `5173` (Vite dev server) free

The three demo pipelines live in `demo/`:

| File | What it does | Runtime |
|---|---|---|
| `demo/preprocess_chart.xzz` | Polars preprocessing + bar chart | ~1s |
| `demo/deep_learning.xzz` | Burn DL training, 5 epochs | ~1s |
| `demo/dp.xzz` | Differential privacy (Laplace) noise | <1s |

---

## Step 1 — Build the binaries

Only needed once (skip if `target/release/xazz` and friends already exist):

```bash
cargo build --release -p xazz
cargo build --release -p xazz-runner
cargo build --release -p xazz-server
```

You should now have `target/release/{xazz, xazz-runner, xazz-server}`.

Install the IDE frontend dependencies (one time):

```bash
cd visual-ide && npm install && cd ..
```

---

## Step 2 — Smoke-test the examples

Run each example once to make sure everything works before going further:

```bash
./target/release/xazz run demo/preprocess_chart.xzz
./target/release/xazz run demo/deep_learning.xzz
./target/release/xazz run demo/dp.xzz
```

Tips:

- The examples load `visual-ide/data/seoul_air_quality.csv`. Please keep that
  path — the files under `examples/data/` are Git LFS placeholders and will not
  run.
- The `Air` type must declare all four CSV columns
  (`observed_at, district, pm25, temperature_c`) or the runtime will fail with a
  duplicate-column error.
- `xazz` and `xazz-runner` need to live in the same directory. After a release
  build they are both in `target/release/`, so this is handled.

---

## Step 3 — Start the servers

You need two terminals.

```bash
# Terminal A — backend
./target/release/xazz-server
```

```bash
# Terminal B — Visual IDE (Vite dev server)
cd visual-ide
VITE_API_BASE_URL=http://127.0.0.1:8005 npm run dev
```

Then open:

- Backend health check: http://127.0.0.1:8005/health → `{"status":"ok"}`
- Visual IDE: http://127.0.0.1:5173

If `xazz-server` can't find the `xazz` binary, point it at the right path:

```bash
XAZZ_EXEC_PATH=/abs/path/to/target/release/xazz ./target/release/xazz-server
```

---

## Step 4 — Walk through the demo

### 1. Static check inside out

```bash
./target/release/xazz check demo/deep_learning.xzz
```

`check` runs the compiler's static analysis: undeclared columns, type mismatches,
and null-safety checks. A clean output means the pipeline is safe to execute.

### 2. See the compiler at work

```bash
./target/release/xazz emit rust demo/deep_learning.xzz | head -40
```

The same script compiles to real Rust source that drives the Polars LazyFrame
and Burn tensor operations. The entire toolchain — parser, AST, type checker,
code generator — is implemented in this repo.

### 3. Preprocessing and charts

```bash
./target/release/xazz run demo/preprocess_chart.xzz
```

This loads the Seoul air quality data, fills missing `pm25` values with the
column mean, averages by district, sorts, and renders a top-5 bar chart. The
chart is written as `result_chart_chart.html` — open it in a browser to see it.

### 4. Deep learning

```bash
./target/release/xazz run demo/deep_learning.xzz
```

`model AirPredictor` declares a small Dense→ReLU→Dense network, and a single
`train()` call runs real training on the Burn engine. You'll see the loss
decrease across 5 epochs. The preprocessed tensors flow straight into training
with a zero-copy conversion staple.

### 5. Differential privacy

```bash
./target/release/xazz run demo/dp.xzz
```

`withDp(epsilon: 1.0, laplace)` adds Laplace noise to the aggregate so a single
record can't be identified from the output. The run prints a privacy budget
report, and every execution is written to an SHA-256 audit log.

### 6. Visual IDE

Open http://127.0.0.1:5173, create a workspace, and run the bundled example.
The pipeline (preprocess → compile → train → predict) renders as a node graph,
with the result and a code hash (audit integrity) displayed in the side panel.
This is the entry point for people who prefer a UI and for agents driving the
pipeline programmatically.

---

## Troubleshooting

| Problem | Fix |
|---|---|
| Pipeline fails loading data | Use `visual-ide/data/seoul_air_quality.csv`, not `examples/data/` (LFS placeholders) |
| `duplicate column` error | All four CSV columns must be declared in the `Air` type |
| Can't find `xazz-runner` | Put `xazz` and `xazz-runner` in the same directory |
| Port 8005 already taken | Stop the existing `xazz-server` first |
| Port 5173 taken | Vite picks another port automatically — use that address |
| Visual IDE returns 404 from the backend | Use the Vite dev server (5173); same-origin serving is only shipped in the release package |
| IDE binary not found by the server | Set `XAZZ_EXEC_PATH` (see Step 3) |