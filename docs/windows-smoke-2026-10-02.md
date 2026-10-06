# Windows smoke results and macOS handoff — 2026-10-02

Related: #167 (OS acceptance), #128 (resource telemetry), #262 (release gates).
Base: `7f8e987` on `main`. Work branch: `fix/windows-smoke-167`.

## Findings addressed locally

1. **Quick Start import generated a reserved variable name.**
   `xazz new demo`, followed by `xazz import data/sample.csv` inside the project,
   emitted `v sample = ...`; `xazz check main.xzz --json` failed at line 11.
   The CLI now checks generated variable names with the DSL lexer and uses
   `data_sample` for this filename. Regression coverage runs the real CLI
   through project creation, import, and checking. Existing CSV golden tests
   were updated; ordinary variable names remain unchanged.
2. **Concurrent audit-log reads could fail on Windows.**
   The earlier workspace run failed `inference_check_blocks_leaked_secret`
   because `chain_valid` was false. Writers held an exclusive file lock, while
   readers opened the file without a shared lock. A new regression test holds
   a writer lock over an incomplete JSON record and checks that the reader
   waits for completion. It failed before the change and passed after readers
   were changed to use a shared lock on the same handle used for reading.
   Hashing and tamper verification are unchanged.
3. **v0.3.1 bundled Git LFS pointers as CSV files.**
   The existing Windows ZIP was checked against its published checksum:
   `fa8af9583be6d4bd2ffd01e44bdfeb7b29854573c1e8a25a5b0567825c6af952`.
   The new archive checker rejects it at
   `xazz/examples/data/adm_code_to_station.csv`. Release checkout now enables
   LFS; a pre-upload check rejects unresolved pointers or missing example CSVs.
   The checker has ZIP and tar.gz tests for valid data, pointers, missing data,
   and mixed valid/pointer entries. No release was created or replaced.

## Validation status at handoff

| Check | Result |
|---|---|
| New Quick Start CLI regression before fix | Failed with the reserved-name parse error |
| New concurrent audit reader regression before fix | Failed: reader did not wait for the writer lock |
| `cargo test -p xazz -p xazz-server` after fixes | Passed: CLI 64 unit tests, 2 integration tests, server 115 tests |
| `python -m unittest discover -s scripts -p test_check_release_data.py -v` | Passed: 8 archive cases |
| `cargo fmt --all -- --check` | Passed |
| `git diff --check` | Passed |
| Full `cargo test --workspace` after fixes | Started; stopped during dependency compilation for handoff, no final result |
| Full workspace Clippy | Not completed; was queued after the workspace test |
| `cargo deny check --all-features` | Not run; local cargo-deny command unavailable |
| Updated binary end-to-end run / HTTP smoke | Not run before handoff |
| Updated release workflow on hosted runners | Not run; next release must verify the packaged artifacts |

Earlier evidence from the unchanged frontend on this same base: Visual IDE
build, contract and stdout checks passed; Playwright passed **70/70** tests.
These are prior-run results, not a new frontend run for this patch.
The earlier full Rust run had the single audit test failure described above.

Windows host: Windows 11, Intel Core Ultra 9 185H, RTX 4070 Laptop and Intel Arc;
Rust 1.98.0, `stable-x86_64-pc-windows-gnu`. Rust's MSVC toolchain is listed as
installed, but `link.exe` and Visual Studio Build Tools were not found in the
locations checked. Windows ONNX acceptance remains pending.

## #128 status

The code already persists resource telemetry and exposes
`GET /runs/{id}/resources`, including tenant isolation and an explicit
`available:false` state for records without telemetry. The focused server
tests covering these paths passed. The CLI reports **wall-clock only** on
Windows; Unix uses process-tree CPU time and peak RSS. There are no measured
per-stage or GPU counters. Do not close #128 as full per-stage instrumentation
based on the existing aggregate implementation. A real server/CLI smoke is
still useful on macOS.

## Continue on macOS

After fetching the work branch, complete the pending verification:

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python3 -m unittest discover -s scripts -p test_check_release_data.py -v
cargo deny check --all-features  # when cargo-deny is available
cargo build -p xazz -p xazz-runner -p xazz-exec -p xazz-server
```

Then test `new -> import -> check -> run` in a fresh project, and run the IDE
checks from `visual-ide/`. The bundled macOS v0.3.1 examples may also contain
LFS pointers; validate the actual archive rather than assuming the new
workflow has already fixed an old release. #167 stays open until the required
OS evidence and a corrected release archive have been verified.

Independent weekend development, with no NVIDIA GPU requirement:

- **#162 / draft PR #248**, `scaffold/162-stratified-timeseries-split`: decide
  single-target versus group stratification and the rare-class minimum policy;
  finish and test the existing scaffold. Inspect current main and reconcile
  branch conflicts first. The draft was still open at handoff.
- **#163 / draft PR #249**, `scaffold/163-classification-metrics`: implement
  macro precision/recall/F1, binary AUC, confusion matrix, classification
  loss/output, and sweep metric integration. Small CPU datasets are sufficient.
- **#166**: 200M-row measurements need memory, disk and time, not a GPU.
- **#127 and #129**: remain deferred by maintainer direction; do not resume.
- **#236**: Windows ONNX/CUDA verification stays on the Windows machine after
  MSVC setup. Existing burn-cuda and wgpu hardware checks already passed.

No new issues, review comments, PRs, merges, or releases were posted for this
handoff. Local raw logs remain under `target/qa-167/` (ignored by Git); earlier
logs are in the Windows machine's `Desktop/xazz-gpu-logs/` folder.
