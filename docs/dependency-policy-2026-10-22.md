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
| 2 | Maintenance notices without patched versions | Owner-approved temporary exceptions for bincode 2.0.1 and paste 1.0.15, expiring **2026-11-21 UTC**. These packages remain unmaintained; they have not been represented as fixed. |

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

- Original PR head f8100e9: all CI checks completed successfully, but its old
  dependency job had the incomplete graph described above.
- Updated full workspace dependency check: **0 errors**, with the two explicitly
  approved temporary maintenance exceptions.
- Exception enforcement: **7 tests passed**.
- Windows workspace, ONNX/CUDA hardware and final commit CI: **in progress**.
  Draft remains enabled until these complete successfully. Previous hardware
  evidence describes the earlier revision and does not substitute for this run.
