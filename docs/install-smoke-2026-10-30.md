# Install & container smoke — 2026-10-30

Release-audit record for `v0.4.0` / `v0.4.1` (issues #167, #176, #262). Every
command below was run against the **published** archives/image, not a local
build. All runs happened on the same physical machine; "clean" means a fresh
user profile / container filesystem, not a second host.

## Environment

| Field | Value |
|-------|-------|
| Host OS | Windows 11 26200.9711 (x86_64) |
| Linux env | WSL2 Ubuntu, kernel shared with the host |
| macOS env | none available |
| Docker | Docker Desktop 29.4.1, engine `linux/x86_64`, Compose v5.1.3 |
| Date | 2026-10-30 |

## Artifact integrity

- `v0.4.0/checksums.sha256` verified for all three archives
  (`linux-x64`, `macos-arm64`, `windows-x64`).
- `v0.4.1` linux archive verified against its published `checksums.sha256`;
  `xazz --version` → `xazz 0.4.1`.
- No archive contains a Git LFS pointer: the bundled
  `examples/data/seoul_air_2023.csv` is real data (EUC-KR) in both the Windows
  and macOS archives.
- macOS archives contain `xazz`, `xazz-runner`, `xazz-exec`, `xazz-server`
  (all arm64 Mach-O) and a prebuilt `xazz/web`.

## Linux (clean HOME)

```bash
xazz --version                 # xazz 0.4.0
xazz new my-project            # data/sample.csv, example.xzz, main.xzz, xazz.toml
cd my-project
xazz import data/sample.csv    # writes the `type Sample` block
xazz run main.xzz              # 10 × 4 rows, exit 0
xazz run main.xzz --json       # exit_code 0, resources source=runner-process-tree
```

## Windows (clean profile)

Same loop via `xazz.exe` from the `windows-x64` archive:

```
xazz new proj → import data/sample.csv → run main.xzz   # 10 × 4 rows, exit 0
```

`--json` reports `duration_ms`, `cpu_user_ms`, `max_rss_kb` with
`source: runner-process-tree`.

## Container (`ghcr.io/x1zzdev/xazz:0.4.1`, anonymous pull)

```bash
docker pull ghcr.io/x1zzdev/xazz:0.4.1      # digest sha256:9d862d7e…
docker run -d --name xazz-smoke -p 8010:8005 -v xazz-smoke-data:/data \
  ghcr.io/x1zzdev/xazz:0.4.1
```

| Check | Result |
|-------|--------|
| `GET /health` | `{"status":"ok","version":"0.4.1"}` |
| IDE `GET /` | 200, no CDN references |
| `POST /security/policy/check` safe example | `safe_to_execute: true` |
| same endpoint, unsafe example | `safe_to_execute: false`, `XZP001` |
| `POST /execute` (sample CSV, cast/fillNull/select/take) | 200, `success: true`, 5 rows, `run_id: 1` |
| `GET /runs` | lists run 1 (`status: success`, 5 rows) |
| `GET /dp/budget` | ε 10.0 / δ 1e-4, spent 0 |
| `GET /security/audit/chain` | `intact: true`, 1 record |
| container user | `uid=10001(xazz)` |
| re-create container on the same named volume | run 1 and the audit record still present |

Multi-arch index for `0.4.1` (and for `0.4.0`) contains `linux/amd64` and
`linux/arm64` manifests; the arm64 manifest was not executed (no arm64 host).

## Known gaps

- **macOS binaries were not executed** (no macOS host). Only file type, archive
  contents, and the absence of LFS pointers were checked.
- Archives and the image are **not code-signed/notarized**: macOS Gatekeeper
  needs `xattr -dr com.apple.quarantine`, Windows SmartScreen may prompt.
- `linux/arm64` image is published but unrun.
- A second physical host was not used.
