# Dependency policy repair — 2026-10-22

PR: [#329](https://github.com/x1zzdev/Xazz/pull/329)

## Why the green CI missed 37 errors

The repository root is both a workspace and the lightweight CLI package.
`cargo deny --all-features check` used only that package as its graph root.
It therefore omitted the execution engine, server and optional GPU dependencies.
All entry points now use `cargo deny --workspace --all-features --locked check`.
The standalone license inventory also includes all optional features.

## Disposition of the original 37 errors

| Count | Cause | Change |
| --- | --- | --- |
| 16 | License allowlist omitted engine/GPU licenses | Add the permissive Apache LLVM exception, BSD-2-Clause, ISC, Zlib, CC0 and CDLA licenses; limit MPL-2.0 to the existing colored exception and option-ext 0.2.0. Preserve upstream notices. |
| 10 | Vulnerability/unsoundness advisories with compatible patched releases | Update crossbeam-epoch, event-listener, h2, memmap2, rustls (four notices), rustls-webpki and stable-vec. |
| 1 | Distrusted root certificate | Update webpki-roots to 1.0.10. |
| 3 | ZIP extraction vulnerabilities | Update duckdb/libduckdb-sys to 1.10506.1, selecting zip 9.0.5. |
| 2 | Yanked releases | Update chacha20 to 0.10.2 and spin to 0.9.9. |
| 1 | Unbounded protobuf recursion | Remove rlx-onnx-proto/protobuf 1.x; generate the official ONNX schema with protobuf-codegen and runtime 3.7.2. Add a deeply nested type rejection test. |
| 2 | XML parsing denial of service | Patch object_store 0.13.2's sole quick-xml dependency to 0.41.0; keep the Polars API and upstream source unchanged. |
| 2 | Maintenance notices without patched versions | Temporary exceptions approved by the requesting user for bincode 2.0.1 and paste 1.0.15, expiring **2026-11-21 UTC**. These packages remain unmaintained; they have not been represented as fixed. |

The maintenance exception register is [security/dependency-exceptions.json](../security/dependency-exceptions.json).
CI fails on expiry, a version change, a missing reason/removal plan, or a mismatch
with deny.toml. Vulnerabilities and unsoundness have **no exceptions**. Duplicate
version warnings remain warnings under the existing policy.

## Source and compatibility

- The ONNX schema and license come from [ONNX v1.16.2](https://github.com/onnx/onnx/tree/v1.16.2).
  Generation uses the pure Rust parser, with matching pinned codegen/runtime.
  The exporter still emits IR version 8 / opset 13 and existing graph operators.
- [object_store patch record](../vendor/object_store/PATCH.md): all original
  source files and upstream licenses are preserved; only the normalized
  quick-xml version requirement changes. Remove the patch after an upstream
  Polars-compatible release carries the fix.
- [bincode maintenance notice](https://rustsec.org/advisories/RUSTSEC-2025-0141.html)
  and [paste maintenance notice](https://rustsec.org/advisories/RUSTSEC-2024-0436.html)
  explain why a normal patch update cannot resolve those two records.

## Validation status

Code commit `3dc1c2414a3df0af4cec5bb01a23652dbb3a76e1`; later commits on the PR
change only documentation, comments, the exception register wording and the
evidence archive. Windows 11, NVIDIA GeForce RTX 4070 Laptop GPU, driver 591.44,
Rust 1.98.0 (GNU toolchain for CUDA, MSVC for ONNX), ORT 1.31.0 CUDA 13
distribution, 2026-10-22 KST.

| Check | Result |
| --- | --- |
| Full workspace dependency policy (`--workspace --all-features --locked`) | 0 errors; 2 recorded maintenance exceptions; 41 duplicate-version warnings |
| Exception enforcement (`scripts/test_dependency_exceptions.py`) | 7 passed |
| Loopback HTTP/XML regression (`http_xml_security`) | 2 passed locally, Linux and Windows CI |
| ONNX schema and recursion-limit tests | 4 passed |
| CUDA training and inference (GNU, `cuda`) | 1 passed, 0 failed, 0 ignored; `training_backend=cuda`; finite loss; same-checkpoint max difference 0 |
| ONNX CPU inference (MSVC, `onnx`) | 1 passed; 2 CPU compute nodes; max difference 0 |
| ONNX CUDA inference (MSVC, `onnx-cuda`) | 1 passed; 3 CUDA compute nodes; max difference 0 |
| CUDA request on a binary built without CUDA | expected rejection: exit 101, exactly one failure, `ONNX execution provider is not compiled into this binary` |
| `cuda:999` request on the CUDA build | expected rejection: exit 101, exactly one failure, `Invalid device ID: 999` |
| `cuda` library tests | 138 passed, 0 failed, 1 ignored (the hardware test, run separately above) |
| `onnx-cuda` library tests | 141 passed, 0 failed, 1 ignored (same) |
| Workspace `cargo clippy --all-targets -D warnings` | passed (0 warnings, exit 0) |
| Workspace `cargo test --workspace --locked` | 746 passed, 0 failed, 0 ignored across 17 test binaries (GPU features excluded) |
| PR CI for the code commit | all jobs passed (Linux/macOS/Windows tests, optional feature compiles, policy, docs). The CodeQL scan reports one alert in unmodified vendored `object_store` code: the upstream `allow_insecure` client option, which Xazz never enables. |

The ONNX CUDA hardware run was executed with a working tree that differed from
the code commit only in four documentation/comment/wording files; the exact diff
and its hash are preserved in the evidence archive (`working-tree-diff.patch`).
ONNX training remains on CPU. The hardware checks are small-model correctness
gates, not performance or operator-coverage claims.

Evidence archive: [dependency-gpu-windows-2026-10-22.zip](evidence/dependency-gpu-windows-2026-10-22.zip),
35 files plus `manifest.json`, 281,352 bytes, SHA-256
`c4a45043fd6c5b8b879b3d9bd1f53146b044e302ff92177f16e6b9618cebe23d`. Personal absolute paths and ANSI sequences were normalized; no
binaries, DLLs or model checkpoints are included. The earlier archive
`gpu-windows-2026-10-22.zip` describes the previous revision and is kept unchanged.
See also the [hardware revalidation follow-up](gpu-revalidation-2026-10-22.md).
