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

## Audit record authentication (#304)

| Variable | Default | Meaning |
|---|---|---|
| `XAZZ_AUDIT_HMAC_KEYS` | unset | Secret JSON object mapping key IDs to hex-encoded keys of at least 32 random bytes. IDs contain 1–64 ASCII letters, digits, `_` or `-`. Keep outside the log directory and source control. |
| `XAZZ_AUDIT_HMAC_ACTIVE_KEY_ID` | unset | ID used to authenticate new records; must exist in `KEYS`. Other keys verify older records. |
| `XAZZ_AUDIT_HMAC_REQUIRED_FROM` | `0` when configured | First record index requiring HMAC. Set once to the existing record count when migrating a trusted legacy log. Never derive this value from an untrusted log on startup. |

The server loads keys and policy once at startup. It rejects incomplete/invalid
configuration and an existing chain that fails verification, before listening.
All three variables are removed from execution child environments. The HTTP API
has no key setter and cannot choose the signing key or migration boundary.

For a new log, provision `KEYS` through the service's secret environment, select
the active ID, and leave `REQUIRED_FROM` at zero. Generate keys using a
cryptographically secure random generator; the 32-byte length check does not
measure entropy. Do not put live keys in command lines, repository files, or logs.

For migration, stop all writers, verify and back up the old log in a trusted
environment, record its count `N`, and configure `REQUIRED_FROM=N` in protected
service configuration. Restart with keys enabled. Existing bytes are preserved;
new entries are signed. The unsigned prefix remains explicitly reported as
legacy, not retroactively authenticated. A boundary beyond the current count is
rejected. Do not increase the boundary to work around a verification failure.

For rotation, stop writers, add a fresh key under a new ID, switch the active ID,
and restart all instances with the same key ring and boundary. Retain every old
verification key for as long as its records are retained. Missing/wrong keys,
partial authentication fields, malformed tags, and missing mandatory tags fail
verification. Removing keys does not silently downgrade an existing signed log.

If all three settings are absent, legacy unsigned operation remains available
with an explicit startup warning. `GET /security/audit/chain` returns one locked
snapshot with `{intact, records, authenticated, hmac_configured, legacy_records,
warning}`. `intact` checks the SHA chain and the configured HMAC policy;
`authenticated` is true only for a nonempty valid chain with every record signed.
No-key or mixed legacy chains never report `authenticated: true`. A failed append
returns HTTP 500 from `POST /security/audit`; no record is added. Log I/O or parse
failures also return 500 from the chain endpoint.

See [the threat model](design/security-model.md#audit-log-authentication-304)
for the file-writer boundary and the external checkpoint work remaining in #305.

## Bind, paths, and execution

| Variable | Default | Meaning |
|---|---|---|
| `XAZZ_BIND` | `127.0.0.1:8005` | Listen address. Set `0.0.0.0:8005` in a container to reach the service from outside. |
| `XAZZ_WEB_DIR` | `web/` next to the executable | Directory served as the static Visual IDE (must contain `index.html`). |
| `XAZZ_EXEC_PATH` | `xazz` next to the server | Absolute path to the `xazz` CLI the server spawns to run pipelines. 이 서버 전용 경로 설정은 자식 프로세스 환경에서 제거된다. 러너가 같은 디렉터리의 `xazz-exec`를 찾으므로 CLI·러너·엔진을 함께 배치해야 한다. Pinning it is the deployment-hardening path (see [Security model](design/security-model.md)). |
| `XAZZ_EXEC_TIMEOUT_SECS` | runner default (`300`) | Hard timeout for one run, passed through to the runner. Values `<= 0` are ignored. Long `train`/sweep runs may need a higher value. |

## Differential-privacy budget

Per-tenant ε/δ envelopes are enforced by the server and injected into each run.

| Variable | Default | Meaning |
|---|---|---|
| `XAZZ_TENANT_DP_BUDGET` | `10.0` | Per-tenant total ε envelope. Non-finite or `<= 0` falls back to the default. |
| `XAZZ_TENANT_DP_DELTA_BUDGET` | `1e-4` | Per-tenant total δ envelope. Values outside `[0, 1)` fall back to the default. |
| `XAZZ_TENANT_DP_WINDOW_SECS` | `0` | Rolling budget window in seconds; `0` means cumulative (no window). Clamped to ~10 years. Overridable per tenant via `PUT /dp/budget/window`. |
| `XAZZ_DP_RESERVATION_TTL_SECS` | `3600` | How long a cross-instance DP reservation stays valid before another instance may reclaim it. Values `<= 0` fall back to the default. |
| `XAZZ_DP_MAX_EPSILON` | unset | Opt-in **per-query** ε cap (issue #118), independent of the policy pack's `max_epsilon`. `/execute` rejects with `422` before reserving budget when any `withDp` ε exceeds it (the error names the step and ε); the runner inherits the variable and re-checks it in `apply_dp`. Invalid or `<= 0` disables the cap. |
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
