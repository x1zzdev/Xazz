# Audit HMAC (#304): Windows verification

Date: 2026-10-22 (Asia/Seoul). Base: `b347688` (`origin/main`).
Environment: Windows, `stable-x86_64-pc-windows-gnu`, rustc/cargo 1.98.0;
NVIDIA GeForce RTX 4070 Laptop GPU, driver 591.44 detected.

## Functional checks

PowerShell settings used for the workspace gates:

```powershell
$env:CARGO_PROFILE_DEV_DEBUG = '0'
$env:CARGO_PROFILE_TEST_DEBUG = '0'
$env:CARGO_BUILD_JOBS = '4'
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
git diff --check
```

All four checks passed. Tests: **759 passed, 0 failed, 0 ignored** across
18 test-result summaries (including empty doc-test suites). Server coverage:
**132 unit tests + 3 real-process HTTP tests**. Default Cargo features were used;
CUDA/wgpu/ONNX acceptance tests were not enabled. Audit authentication uses CPU
cryptography and does not change GPU backend code. Debug information was disabled
to avoid the documented Windows GNU executable-size limitation.

The HMAC tests cover RFC 4231's SHA-256 vector, a record payload golden value
independently calculated with Python's standard `hmac`/`json`, recomputed unkeyed
forgeries, field-boundary ambiguity, MAC stripping, bad/unknown keys and malformed
tags, explicit legacy migration, rotation with retained verification keys, and
refusal to append to forged history. Actual server processes exercise production
environment loading, startup rejection, HTTP 500 on failed audit persistence,
authentication status, and two writers sharing one authenticated log on Windows.
All test logs/databases are temporary; the operator's runtime log is not used.

An initial HTTP run in the tool's network-isolated environment timed out on
loopback connections. The final complete suite ran on the Windows host and
passed. No test was skipped or weakened to work around that restriction.

## Dependency policy: existing workspace failures

Used cargo-deny 0.20.2 installed under the ignored `target/qa-304-tools/` directory.

| Scope / command | Result |
|---|---|
| `cargo deny --all-features check` | Passed; this non-virtual root manifest selects the CLI dependency graph only |
| `cargo deny --workspace --all-features check` | Failed: advisories and licenses; bans and sources passed |
| Same workspace check against a separate source snapshot of `b347688` | Same 37 error diagnostic counts and same 19 RustSec IDs |

Workspace errors: 16 license-policy rejections, 14 vulnerability diagnostics,
3 unsoundness diagnostics, 2 unmaintained-package diagnostics and 2 yanked
versions (`chacha20 0.10.0`, `spin 0.9.8`). For example, `spin` is reached through
the existing `multer -> axum -> xazz-server` dependency path. The PR changes no
locked package version; it only adds the server's edge to the already-locked
`hmac 0.13.0`. Dependency upgrades and license-policy decisions are separate work.

The existing CI license job uses the root manifest without `--workspace`, so its
success must not be presented as coverage of all workspace packages. This PR
does not weaken policy or ignore advisories to obtain a passing result.

Local raw evidence (ignored build output): `target/qa-304-workspace-tests.log`,
`target/qa-304-clippy-final.log`, `target/qa-304-deny.log`,
`target/qa-304-deny-workspace.log`, and `target/qa-304-deny-baseline.log`.

## Remaining protection boundary

HMAC protects against a log-file writer without the server's keys/configuration.
Valid-prefix rollback, tail truncation and external checkpoints remain #305.
See [server configuration](../SERVER.md#audit-record-authentication-304) and
[the threat model](../design/security-model.md#audit-log-authentication-304).
