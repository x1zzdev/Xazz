# Xazz product UX prototype

React workspace connecting the approved landing-to-tool experience to the real
`xazz-server` execution backend.

## Prerequisites

- Rust toolchain with `cc` (for building `xazz`, `xazz-runner`, `xazz-exec`,
  `xazz-server`).
- Built binaries available on PATH or under `target/release/`.
- `xazz-server` reachable at `http://127.0.0.1:8005` (override with
  `VITE_API_BASE_URL`).

## Run

```powershell
npm install
npm run dev
```

Open `http://127.0.0.1:5173`.

### Start the backend

```powershell
cargo build --release
target/release/xazz-server
```

`xazz-server` locates the `xazz` binary next to its own executable, under
`target/release`, or on PATH (Windows: `xazz.exe`).

## Verify

```powershell
npm run build
npm run test:contract
npm run test:contrast
npm run test:e2e
npm audit --audit-level=high
```

`npm run capture` refreshes the nine repository screenshots after a source
change. Do not use it as user-testing evidence.

## Integration scope

- **Full Run** POSTs the current `example.xzz` source (from
  `src/data.js::runnableCode`) to `POST /execute` on `xazz-server` and renders
  the returned rows, schema, stderr logs, and `[xazz:train]` training report.
- **Live Check** pings `GET /health` and reports backend reachability.
- The Compiler Canvas is structural: node/link layout comes from the
  `pipeline` fixture, while Preview, Delta, Chart, Logs, Monitor, and Receipt
  are driven by the last real Full Run response.
- Without a reachable backend, Full Run reports an honest connection failure —
  it never substitutes synthetic success.

## Truth boundary

- Per-node row/null deltas and durations are not emitted by the current
  runtime; the UI labels them "Not available" rather than inferring them.
- The `[xazz:train]` marker is emitted for both the `run ... |> train(...)`
  statement and the `|> train(Model, ...)` pipeline operator.
- Policy, DP, sLM repair, sandboxing, and durable audit remain Research or
  Planned.
- A process exit is not promoted to pipeline success without structured result
  evidence.
- Browser CSV export is an optional post-result user action; no artifact file
  is written by the backend.

Figma handoff:
<https://www.figma.com/design/WqomiXUlVze79yz3s0GRmS>
