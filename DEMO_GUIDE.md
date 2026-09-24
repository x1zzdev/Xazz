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
cargo build --release -p xazz-exec
cargo build --release -p xazz-server
```

You should now have `target/release/{xazz, xazz-runner, xazz-exec, xazz-server}`. The first three binaries must stay together for `xazz run`.

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
- `xazz`, `xazz-runner`, and `xazz-exec` need to live in the same directory. A
  release build puts all three in `target/release/`.

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

## Common errors and fixes

The excerpts below were reproduced with the repository CLI/server on macOS. Run the commands from the repository root unless the example says otherwise.

### Misspelled column

Reproduce with `sed 's/mean("pm25")/mean("pm52")/' demo/preprocess_chart.xzz > /tmp/xazz-bad-column.xzz && ./target/debug/xazz check /tmp/xazz-bad-column.xzz`:

```text
❌ [error] aggregate: column 'pm52' does not exist in the schema.
💡 available columns: district, observed_at, pm25, temperature_c
  Did you mean: col("pm25")?
```

Fix the name and check again: `sed 's/mean("pm52")/mean("pm25")/' /tmp/xazz-bad-column.xzz > /tmp/xazz-fixed-column.xzz && ./target/debug/xazz check /tmp/xazz-fixed-column.xzz`.

### Nullability or type mismatch

Reproduce with `sed 's/fillNull("pm25", strategy: "mean")/fillNull("district", strategy: "mean")/' demo/preprocess_chart.xzz > /tmp/xazz-bad-type.xzz && ./target/debug/xazz check /tmp/xazz-bad-type.xzz`. `district` is declared as non-nullable `string`:

```text
❌ [error] fillNull("district", ...) : column 'district' is declared as a non-nullable type. Declare 'district' as Option<string> in the schema, or remove this operation.
⚠️  [warning] fillNull("district", <number>) : filling string column 'district' with a number may change its type.
```

Use the nullable numeric column instead: `sed 's/fillNull("district", strategy: "mean")/fillNull("pm25", strategy: "mean")/' /tmp/xazz-bad-type.xzz > /tmp/xazz-fixed-type.xzz && ./target/debug/xazz check /tmp/xazz-fixed-type.xzz`. If nulls are valid for your real column, declare `Option<T>` and choose a fill value of the same type.

### CSV encoding cannot be decoded

This minimal file contains a byte that is invalid in UTF-8 and EUC-KR. Work in a scratch directory because `xazz import` writes `main.xzz` in the current directory:

```bash
XAZZ_BIN="$PWD/target/debug/xazz"
mkdir -p /tmp/xazz-demo-encoding
python3 -c 'from pathlib import Path; Path("/tmp/xazz-demo-encoding/bad.csv").write_bytes(b"col\n\xff\n")'
(cd /tmp/xazz-demo-encoding && "$XAZZ_BIN" import bad.csv)
```

It reports:

```text
UTF-8 디코딩도 실패: invalid utf-8 sequence of 1 bytes from index 4
```

For this example the byte is Latin-1, so `iconv -f ISO-8859-1 -t UTF-8 /tmp/xazz-demo-encoding/bad.csv > /tmp/xazz-demo-encoding/fixed.csv && (cd /tmp/xazz-demo-encoding && "$XAZZ_BIN" import fixed.csv)` repairs it. For real data, identify the source encoding before choosing `iconv -f`; valid EUC-KR CSV already has an automatic fallback.

### Runner or engine binary is missing

Build only `xazz` (`cargo build -p xazz`), then run `target/debug/xazz run demo/preprocess_chart.xzz`:

```text
xazz-runner not found (PATH fallback is disabled for security). Set XAZZ_RUNNER_PATH to an absolute path or place xazz-runner next to the xazz binary.
```

Fix it with `cargo build -p xazz-runner -p xazz-exec`. Keep `xazz`, `xazz-runner`, and `xazz-exec` together in `target/debug/`. For a custom layout, set absolute `XAZZ_RUNNER_PATH` and `XAZZ_EXEC_PATH` paths instead of relying on `PATH`.

### Server port is in use

Start one listener on `127.0.0.1:8005` (for example, `python3 -m http.server 8005` in another terminal), then start `target/debug/xazz-server`. On macOS the second process reports:

```text
called `Result::unwrap()` on an `Err` value: Os { code: 48, kind: AddrInUse, message: "Address already in use" }
```

Stop the process already using the port, or use `XAZZ_BIND=127.0.0.1:8006 target/debug/xazz-server` and point the IDE to it with `VITE_API_BASE_URL=http://127.0.0.1:8006 npm run dev`. If port 5173 is occupied, Vite prints its chosen replacement port; open that address.

Other demo problems: use `visual-ide/data/seoul_air_quality.csv` rather than the LFS placeholders under `examples/data/`; declare all four `Air` CSV columns to avoid a duplicate-column error; use `XAZZ_EXEC_PATH` if the server cannot locate the CLI; and use the Vite dev server on port 5173 for local IDE development.
