# Docker: images, ports, volumes, permissions

Xazz ships a single container that runs `xazz-server` with the Visual IDE
bundled under `/app/web`. The CLI (`xazz`), IPC bridge (`xazz-runner`) and
engine (`xazz-exec`) sit next to the server in `/app` so Full Run works
without a `PATH` entry.

## Quick start (local build)

```bash
docker compose up --build
# open http://127.0.0.1:8005
```

Or without compose:

```bash
docker build -t xazz:local .
docker run --rm -p 8005:8005 -v "$PWD/data:/data" xazz:local
```

## Published images (GHCR)

Release tags (`v*`) publish multi-arch images (`linux/amd64`, `linux/arm64`)
from `.github/workflows/release.yml`:

```bash
docker pull ghcr.io/x1zzdev/xazz:latest   # or :0.3.1, :0.3
docker run --rm -p 8005:8005 -v "$PWD/data:/data" ghcr.io/x1zzdev/xazz:latest
```

Prerelease tags (`v1.0.0-rc.1`) do not move `latest`.

## Ports

| Port | Purpose |
|------|---------|
| `8005/tcp` | REST API + Visual IDE (override with `XAZZ_BIND`, compose maps `8005:8005`) |

The server binds `127.0.0.1` by default on a bare binary. Inside the container
`XAZZ_BIND=0.0.0.0:8005` is set so the published port reaches it.

## Volumes

| Container path | Compose / run flag | What lives there |
|----------------|--------------------|------------------|
| `/data` | `./data:/data` | Datasets you `load(...)`, chart HTML, and other artifacts. `WORKDIR` is `/data`, so relative paths in `.xzz` resolve here. |

The image also declares `VOLUME /data` so an anonymous volume is created if you
omit the mount.

Server state that is **not** on `/data` by default:

| Path in container | Notes |
|-------------------|-------|
| `audit_log/` (relative to CWD) | SHA-256 audit JSONL — created under `/data` when CWD is `/data`. Mount `/data` to keep it across restarts. |
| `uploads/` (relative to CWD) | CSV copies from `POST /schema` — same as above. |
| `xazz.db` (relative to CWD) | SQLite store (runs, tenant policies, DP ledger) when the server creates it under CWD. |

To pin state outside the dataset mount, bind-mount individual host paths, e.g.
`-v "$PWD/state/audit_log:/data/audit_log"`.

## Permissions (uid/gid)

The runtime user is **`xazz` → uid `10001`, gid `10001`** (non-root).

- Binaries and `/app/web` are readable/executable by others; any uid can start
  the server.
- Writes under `/data` (uploads, audit log, charts, `xazz.db`) require the host
  directory to be writable by **uid 10001**.

On a fresh host:

```bash
mkdir -p data
sudo chown -R 10001:10001 data   # match the container user
docker compose up --build
```

If your host uid already owns the folder (e.g. a dev checkout), either chown to
10001 or run the container as your uid:

```bash
docker run --rm -p 8005:8005 -u "$(id -u):$(id -g)" -v "$PWD/data:/data" xazz:local
```

With compose, add under `services.xazz`:

```yaml
    user: "${UID:-10001}:${GID:-10001}"
```

and export `UID`/`GID` before `docker compose up`.

## Environment

| Variable | Default in image | Meaning |
|----------|------------------|---------|
| `XAZZ_BIND` | `0.0.0.0:8005` | Listen address |
| `XAZZ_WEB_DIR` | `/app/web` | Static Visual IDE |
| `XAZZ_SERVER_TOKEN` | unset | If set, every request needs `Authorization: Bearer …` |
| `XAZZ_TENANT_TOKENS` | unset | `tenant=token,tenant=token` multi-tenant map |
| `XAZZ_EXEC_PATH` | unset | Absolute path to `xazz-exec` (default: next to `xazz-runner`) |
| `XAZZ_EXEC_TIMEOUT_SECS` | runner default | Hard timeout for one run |

## Smoke checklist (clean host)

1. `docker pull ghcr.io/x1zzdev/xazz:<tag>` (or `compose up --build`).
2. `curl -fsS http://127.0.0.1:8005/health` → `{"status":"ok",…}`.
3. Open `http://127.0.0.1:8005` — Visual IDE loads (no CDN requests).
4. Full Run on the seeded pipeline — Preview returns rows (needs `/data` writable).
5. History tab shows the run; Governance panels read `/dp/budget` and
   `/security/audit/chain`.
6. `docker compose down` — with a bind mount, audit log and runs survive
   `up` again.

Record the image tag and host OS/arch with the result (issue #176).
