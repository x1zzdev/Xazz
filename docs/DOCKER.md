# Docker: images, ports, volumes, permissions

Xazz ships a single container that runs `xazz-server` with the Visual IDE
bundled under `/app/web`. The CLI (`xazz`), IPC bridge (`xazz-runner`) and
engine (`xazz-exec`) sit next to the server in `/app` so Full Run works
without a `PATH` entry.

## Quick start (local build)

```bash
docker compose up
# open http://127.0.0.1:8005
```

Compose builds `xazz:local` when it is missing. It uses a Docker-managed
`xazz-data` volume so a fresh checkout needs no host-directory permission setup.
The image includes the synthetic air-quality sample at
`/data/visual-ide/data/seoul_air_quality.csv`. In the IDE, open Monitor and use
**Check safe example** or **Check unsafe example** to compare static policy
verdicts. Neither button executes code or changes the current pipeline.

Or without compose:

```bash
docker build -t xazz:local .
docker run --rm -p 8005:8005 -v xazz-data:/data xazz:local
```

## Release images (GHCR)

The next release tag (`v*`) will trigger a multi-arch image build
(`linux/amd64`, `linux/arm64`) in `.github/workflows/release.yml`. No GHCR image
was available when this guide was updated; use the local Compose build above
until a published tag is verified. After publication:

```bash
docker pull ghcr.io/x1zzdev/xazz:latest   # or a verified release tag
docker run --rm -p 8005:8005 -v xazz-data:/data ghcr.io/x1zzdev/xazz:latest
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
| `/data` | `xazz-data:/data` | Synthetic sample, datasets you `load(...)`, chart HTML, and other artifacts. `WORKDIR` is `/data`, so relative paths in `.xzz` resolve here. |

The image also declares `VOLUME /data` so an anonymous volume is created if you
omit the mount.
The Compose named volume persists after `docker compose down`; removing it with
`docker compose down -v` deletes runs, audit records, uploads, and artifacts.
To use host files instead, replace `xazz-data:/data` with `./data:/data` and
follow the permissions instructions below. A bind mount hides the image's seeded
sample; copy `visual-ide/data/seoul_air_quality.csv` to
`./data/visual-ide/data/seoul_air_quality.csv` before trying the default Full Run.

### Upgrading from the earlier `./data` bind mount

The default mount changed to a named volume for the one-command demo. Docker does
not move existing `./data` files automatically. **Before the first `docker compose
up` with this version**, either keep the old `./data:/data` line in your local
Compose file, or copy the existing state into the new volume:

```bash
docker compose build
docker compose run --rm --no-deps -v "$PWD/data:/legacy:ro" --entrypoint sh xazz \
  -c 'cp -nR /legacy/. /data/'
docker compose up
```

The copy leaves `./data` untouched and does not overwrite files already in the
new volume. Check History and the audit chain before using the new volume for
other runs. If the copy reports a permission error, stop and use the original
`./data:/data` mount until file ownership is resolved.

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
sudo chown -R 10001:10001 data   # only for a host bind mount
docker compose up
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

1. `docker pull ghcr.io/x1zzdev/xazz:<tag>` after publication (or `docker compose up` for the local image).
2. `curl -fsS http://127.0.0.1:8005/health` → `{"status":"ok",…}`.
3. Open `http://127.0.0.1:8005` — Visual IDE loads (no CDN requests).
4. Full Run on the seeded pipeline — Preview returns rows (needs `/data` writable).
5. History tab shows the run; Governance panels read `/dp/budget` and
   `/security/audit/chain`.
6. `docker compose down` — the named volume keeps the audit log and runs for
   the next `up`.

Record the image tag and host OS/arch with the result (issue #176).
