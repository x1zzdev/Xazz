# Server reference — environment variables

`xazz-server` powers the REST API and the browser Visual IDE. This page is the
canonical reference for the environment variables the server process reads.
Container-specific defaults (ports, volumes, image paths) live in
[Docker](DOCKER.md); the security endpoints and their payloads live in
[Security guardrail](SECURITY_GUARDRAIL.md).

Unset variables fall back to the defaults below. The server binds loopback by
default and only enforces authentication when at least one credential variable
is set.

## Authentication

| Variable | Default | Meaning |
|---|---|---|
| `XAZZ_SERVER_TOKEN` | unset | If set, every request needs `Authorization: Bearer <token>`. `X-Xazz-Tenant` is ignored and the tenant is empty. |
| `XAZZ_TENANT_TOKENS` | unset | `tenant=token,tenant=token` multi-tenant map. A request must send `X-Xazz-Tenant: <tenant>` plus `Authorization: Bearer <token>` matching that tenant. |
| `XAZZ_ADMIN_TOKEN` | unset | Admin Bearer token, or a comma-separated list for rotation. An admin may target any `X-Xazz-Tenant` namespace and is recorded under `X-Xazz-Actor` (default `admin`). |
| `XAZZ_ADMIN_ACTORS` | unset | `token:actor,token:actor` admin credentials with a **pinned** audit actor. A bound token ignores `X-Xazz-Actor`, so the credential cannot impersonate another actor. |

Resolution order for a request: admin credentials first, then the tenant map,
then the single token. When **none** of the four variables is set, every request
is allowed (default local-only behavior). `XAZZ_ADMIN_ACTORS` entries split on
the **last** `:` so a credential may itself contain colons; `XAZZ_ADMIN_TOKEN`
entries are comma-separated.

## Bind, paths, and execution

| Variable | Default | Meaning |
|---|---|---|
| `XAZZ_BIND` | `127.0.0.1:8005` | Listen address. Set `0.0.0.0:8005` in a container to reach the service from outside. |
| `XAZZ_WEB_DIR` | `web/` next to the executable | Directory served as the static Visual IDE (must contain `index.html`). |
| `XAZZ_EXEC_PATH` | `xazz` next to the server | Absolute path to the `xazz` CLI the server spawns to run pipelines. Pinning it is the deployment-hardening path (see [Security model](design/security-model.md)). |
| `XAZZ_EXEC_TIMEOUT_SECS` | runner default (`300`) | Hard timeout for one run, passed through to the runner. Values `<= 0` are ignored. Long `train`/sweep runs may need a higher value. |

## Differential-privacy budget

Per-tenant ε/δ envelopes are enforced by the server and injected into each run.

| Variable | Default | Meaning |
|---|---|---|
| `XAZZ_TENANT_DP_BUDGET` | `10.0` | Per-tenant total ε envelope. Non-finite or `<= 0` falls back to the default. |
| `XAZZ_TENANT_DP_DELTA_BUDGET` | `1e-4` | Per-tenant total δ envelope. Values outside `[0, 1)` fall back to the default. |
| `XAZZ_TENANT_DP_WINDOW_SECS` | `0` | Rolling budget window in seconds; `0` means cumulative (no window). Clamped to ~10 years. Overridable per tenant via `PUT /dp/budget/window`. |
| `XAZZ_DP_RESERVATION_TTL_SECS` | `3600` | How long a cross-instance DP reservation stays valid before another instance may reclaim it. Values `<= 0` fall back to the default. |
| `XAZZ_TENANT_DP_WINDOW_HISTORY_MAX` | `1000` | Per-tenant cap on retained DP-window override change-history rows. Invalid or `0` falls back to the default. |

The remaining ε/δ handed to a run is emitted as `XAZZ_DP_BUDGET` /
`XAZZ_DP_DELTA_BUDGET` in the child's environment; these are internal and not
meant to be set by operators.

## Policy-history retention

Policy-pack and retention-override histories are append-only audit trails with
per-tenant count caps and an optional time-based expiry.

| Variable | Default | Meaning |
|---|---|---|
| `XAZZ_TENANT_POLICY_HISTORY_MAX` | `1000` | Per-tenant cap on retained policy-pack change-history rows. Invalid or `0` falls back to the default. |
| `XAZZ_TENANT_POLICY_HISTORY_TTL_SECS` | `0` | Per-tenant policy-history expiry window in seconds; unset or `0` disables time-based expiry (count cap only). Overridable per tenant via `PUT /security/policy/history/ttl`. |
| `XAZZ_TENANT_POLICY_TTL_HISTORY_MAX` | `1000` | Per-tenant cap on retained retention-window override change-history rows. Independent of the policy-history cap. |
| `XAZZ_POLICY_HISTORY_SWEEP_SECS` | `3600` | Interval between periodic retention sweeps (seconds); `0` disables the sweep. Invalid values fall back to the default. |

## Execution engine (inherited)

The server passes its environment to the spawned CLI/runner, so execution-engine
variables also apply to server-initiated runs. See
[GPU & ONNX backends](GPU_BACKENDS.md) for `XAZZ_BACKEND`, `XAZZ_DEVICE`, and
`XAZZ_LOG`, [Security guardrail](SECURITY_GUARDRAIL.md) for `XAZZ_POLICY_PATH`
and the `XAZZ_SLM_*` hook, and [Data source connectors](CONNECTORS.md) for
`XAZZ_STREAMING`.