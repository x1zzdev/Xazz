//! xazz-server — Axum HTTP API server for the Visual IDE integration (v0.3)
//!
//! Endpoints:
//!   POST /execute          { "code": "<xzz DSL>" }         → pipeline execution, JSON result
//!   POST /schema           multipart/form-data (file)      → CSV schema inference, column types
//!   GET  /health           {}                               → server status check
//!   POST /security/audit   { "code": "<xzz DSL>" }         → SHA-256 audit log creation + persistent storage
//!   POST /security/verify  { "code": "<xzz DSL>", "hash": "<sha256>" } → audit hash verification
//!   GET  /security/audit/log                               → view all audit logs (JSONL hash chain)
//!   GET  /security/audit/log/:hash                         → look up an audit record by code hash
//!   GET  /security/audit/chain                             → verify hash-chain integrity
//!   GET  /security/policy                                  → the current Policy-as-Code policy
//!   PUT  /security/policy                                  → store the tenant's policy pack (C2)
//!   DELETE /security/policy                                → remove the tenant's policy pack (C2)
//!   GET  /security/policy/history?limit=&offset=          → tenant's policy-pack change audit + effective TTL (C2)
//!   GET  /security/policy/history/ttl                      → per-tenant effective history retention window (C2)
//!   PUT  /security/policy/history/ttl                      → per-tenant history retention window (C2)
//!   DELETE /security/policy/history/ttl                    → clear the tenant's retention override (C2)
//!   GET  /security/policy/history/ttl/history?limit=&offset= → tenant's retention-override change audit (C2)
//!   POST /security/policy/check { "code": "<xzz DSL>" }    → static guardrail inspection report
//!   POST /security/remediate    { "code": "<xzz DSL>" }    → safe code auto-remediation (deterministic + sLM)
//!   GET  /runs                                → run history (SQLite, issue C1)
//!   GET  /runs/:id                            → a single run record
//!   GET  /dp/budget                           → per-tenant DP spend/remaining + window
//!   POST /dp/budget/reset                     → reset the tenant's DP spend/window (C2)
//!
//! Port: 8005 (frontend/.env: VITE_API_BASE_URL=http://127.0.0.1:8005)

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use axum::{
    Router,
    extract::{DefaultBodyLimit, Extension, Multipart, Path, Query, State},
    http::{HeaderValue, StatusCode, header::AUTHORIZATION},
    middleware::{self, Next},
    response::{IntoResponse, Json},
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::Semaphore;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use tower_http::services::ServeDir;

mod audit_log;
mod guardrail;
mod slm;
mod store;

/// Upper bound on concurrently running `xazz run` processes — execution DoS prevention.
/// Over-limit requests are rejected immediately with 429 (no queue → prevents
/// follow-up request backlog buildup).
const MAX_CONCURRENT_EXECUTIONS: usize = 4;

/// /schema upload maximum allowed size (bytes).
const MAX_UPLOAD_BYTES: usize = 50 * 1024 * 1024;

/// Default page size for `GET /security/policy/history`.
const POLICY_HISTORY_LIMIT: usize = 100;

/// Hard cap on `?limit=` for `GET /security/policy/history`.
const POLICY_HISTORY_MAX_LIMIT: usize = 500;

/// Pagination query for `GET /security/policy/history` (issue C2).
#[derive(Debug, Deserialize)]
struct PolicyHistoryQuery {
    /// Page size; defaults to [`POLICY_HISTORY_LIMIT`], clamped to `1..=POLICY_HISTORY_MAX_LIMIT`.
    limit: Option<usize>,
    /// Rows to skip in the newest-first list; defaults to 0.
    offset: Option<usize>,
}

impl PolicyHistoryQuery {
    fn limit(&self) -> usize {
        self.limit
            .unwrap_or(POLICY_HISTORY_LIMIT)
            .clamp(1, POLICY_HISTORY_MAX_LIMIT)
    }

    fn offset(&self) -> usize {
        self.offset.unwrap_or(0)
    }
}

/// Per-tenant execution locks. Values are held weakly so locks whose owners have
/// finished can be pruned; otherwise a long-lived server would retain one entry
/// per tenant ever seen (issue C2).
type TenantLocks = Arc<
    std::sync::Mutex<std::collections::HashMap<String, std::sync::Weak<tokio::sync::Mutex<()>>>>,
>;

/// AppState shared across requests.
#[derive(Clone)]
struct AppState {
    exec_permits: Arc<Semaphore>,
    /// Persistent run-history store (issue C1)
    store: Arc<store::Store>,
    /// Per-tenant execution locks (issue C2) — serialize a tenant's
    /// precheck → run → accrue so its DP budget check is atomic across runs.
    tenant_locks: TenantLocks,
}

impl AppState {
    /// Returns the (shared) execution lock for a tenant, creating it on first use.
    ///
    /// The map stores `Weak` refs and is pruned whenever a new tenant appears, so
    /// it stays bounded by the number of tenants with in-flight executions.
    fn tenant_lock(&self, tenant: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut map = self.tenant_locks.lock().expect("tenant lock map poisoned");
        if let Some(existing) = map.get(tenant).and_then(std::sync::Weak::upgrade) {
            return existing;
        }
        map.retain(|_, lock| lock.strong_count() > 0);
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        map.insert(tenant.to_string(), Arc::downgrade(&lock));
        lock
    }
}

/// Records a run in the store, returning the new id (0 on store failure).
/// Extracts the authenticated tenant from the request extension (default "").
fn tenant_str(tenant: &str) -> &str {
    tenant
}

/// Records a run in the store, returning the new id (0 on store failure).
fn record_run_in_store(
    state: &AppState,
    code: &str,
    status: &str,
    rows: i64,
    error: Option<&str>,
    tenant: &str,
) -> i64 {
    let hash = audit_log::hash_code(code);
    match state.store.record_run(&hash, status, rows, error, tenant) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("[xazz] ⚠️ run history 저장 실패: {e}");
            0
        }
    }
}

// ── Per-tenant DP budget envelope (issue C2) ─────────────────────────────────

/// Per-tenant total ε envelope (overridable via env).
const TENANT_DP_BUDGET_ENV: &str = "XAZZ_TENANT_DP_BUDGET";
/// Per-tenant total δ envelope (overridable via env).
const TENANT_DP_DELTA_BUDGET_ENV: &str = "XAZZ_TENANT_DP_DELTA_BUDGET";
/// Per-tenant DP budget window length in seconds (0 = cumulative, no window).
const TENANT_DP_WINDOW_ENV: &str = "XAZZ_TENANT_DP_WINDOW_SECS";
/// How long a cross-instance DP reservation stays valid before it can be
/// reclaimed by another instance (guards against a crashed run blocking a tenant).
const DP_RESERVATION_TTL_ENV: &str = "XAZZ_DP_RESERVATION_TTL_SECS";
const DEFAULT_TENANT_DP_BUDGET: f64 = 10.0;
const DEFAULT_TENANT_DP_DELTA_BUDGET: f64 = 1e-4;
/// Default reservation TTL: one hour is far longer than a normal run while still
/// bounding how long a crashed instance can hold a tenant's envelope.
const DEFAULT_DP_RESERVATION_TTL_SECS: u64 = 3600;

/// Floor for the remaining budget handed to the runner. The runner's env parser
/// ignores non-positive totals, so passing 0 would silently reset to its own
/// default; this keeps "no budget left" enforceable while non-DP runs proceed.
const MIN_REMAINING_BUDGET: f64 = 1e-12;

/// Upper bound for a DP budget window (~10 years). Longer values are rejected on
/// write and clamped on read, so `anchor + window_secs` can never overflow when
/// `resets_at` is computed.
const MAX_DP_WINDOW_SECS: u64 = 10 * 365 * 24 * 60 * 60;

/// Parses the configured per-tenant (ε, δ) envelope from raw env values.
fn resolve_dp_envelope(eps_raw: Option<&str>, delta_raw: Option<&str>) -> (f64, f64) {
    let epsilon = eps_raw
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(DEFAULT_TENANT_DP_BUDGET);
    let delta = delta_raw
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite() && (0.0..1.0).contains(v))
        .unwrap_or(DEFAULT_TENANT_DP_DELTA_BUDGET);
    (epsilon, delta)
}

/// Reads the per-tenant DP envelope from the environment.
fn tenant_dp_envelope() -> (f64, f64) {
    let eps = std::env::var(TENANT_DP_BUDGET_ENV).ok();
    let delta = std::env::var(TENANT_DP_DELTA_BUDGET_ENV).ok();
    resolve_dp_envelope(eps.as_deref(), delta.as_deref())
}

/// Parses the DP budget window length. Invalid or `0` means "cumulative, no window".
/// Values above [`MAX_DP_WINDOW_SECS`] are clamped so a stale env setting cannot
/// overflow the `resets_at` computation.
fn resolve_dp_window(raw: Option<&str>) -> u64 {
    raw.and_then(|v| v.parse::<u64>().ok())
        .map(|secs| secs.min(MAX_DP_WINDOW_SECS))
        .unwrap_or(0)
}

/// Reads the per-tenant DP budget window from the environment (0 = disabled).
fn tenant_dp_window_secs() -> u64 {
    let raw = std::env::var(TENANT_DP_WINDOW_ENV).ok();
    resolve_dp_window(raw.as_deref())
}

/// Parses the cross-instance DP reservation TTL. Invalid or `0` falls back to the
/// default so a reservation can never become immediately reclaimable by accident.
fn resolve_dp_reservation_ttl(raw: Option<&str>) -> u64 {
    raw.and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_DP_RESERVATION_TTL_SECS)
}

/// Reads the DP reservation TTL from the environment.
fn dp_reservation_ttl_secs() -> u64 {
    let raw = std::env::var(DP_RESERVATION_TTL_ENV).ok();
    resolve_dp_reservation_ttl(raw.as_deref())
}

/// A tenant's effective DP budget window and where it came from — issue C2.
struct DpWindow {
    /// Window length in seconds (`0` = cumulative, no window).
    secs: u64,
    /// `"tenant"` when a stored per-tenant override is in effect, else `"global"`.
    source: &'static str,
}

/// Resolves a tenant's effective DP budget window — issue C2.
///
/// A stored per-tenant override (`tenant_dp_config`) takes precedence over the
/// global `XAZZ_TENANT_DP_WINDOW_SECS` default. An explicit stored `0` is a
/// tenant override (source `"tenant"`), distinct from having no override.
fn effective_dp_window(store: &store::Store, tenant: &str) -> Result<DpWindow, String> {
    match store.get_dp_window(tenant)? {
        Some(secs) => Ok(DpWindow {
            secs: secs.min(MAX_DP_WINDOW_SECS),
            source: "tenant",
        }),
        None => Ok(DpWindow {
            secs: tenant_dp_window_secs(),
            source: "global",
        }),
    }
}

/// Extracts the (ε, δ) a run consumed from its `[xazz:dp]` marker.
/// Returns `None` when the run used no `withDp` (nothing to bill).
fn dp_spend_from_marker(dp: &Option<Value>) -> Option<(f64, f64)> {
    let value = dp.as_ref()?;
    let epsilon = value.get("budget_spent")?.as_f64()?;
    let delta = value
        .get("budget_spent_delta")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    Some((epsilon, delta))
}

/// Releases a tenant's cross-instance DP reservation, billing only the run's
/// actual `[xazz:dp]` spend (0 when the run used no `withDp`) — issue C2.
///
/// Never fatal: a failed settle leaves the reservation in place until its TTL
/// expires, which is safer than dropping the response on an accounting hiccup.
fn settle_dp_reservation(
    store: &store::Store,
    tenant: &str,
    reservation: &store::DpReservation,
    dp: &Option<Value>,
    window_secs: u64,
) {
    let (epsilon, delta) = dp_spend_from_marker(dp).unwrap_or((0.0, 0.0));
    if let Err(e) =
        store.settle_dp_reservation(tenant, &reservation.id, epsilon, delta, window_secs)
    {
        eprintln!("[xazz] ⚠️ DP 예약 정산 실패: {e}");
    }
}

/// RAII holder for a tenant's cross-instance DP reservation (issue C2).
///
/// A run's `[xazz:dp]` marker is only known after the runner exits, but the
/// reservation must be released on *every* exit path (temp-file failure, missing
/// executable, command error). `Drop` releases it with zero spend unless
/// [`DpReservationGuard::settle`] billed the run's actual spend first.
struct DpReservationGuard<'a> {
    store: &'a store::Store,
    tenant: &'a str,
    /// `None` once the reservation has been settled or handed to a task.
    reservation: Option<store::DpReservation>,
    window_secs: u64,
    settled: bool,
}

impl<'a> DpReservationGuard<'a> {
    fn new(
        store: &'a store::Store,
        tenant: &'a str,
        reservation: store::DpReservation,
        window_secs: u64,
    ) -> Self {
        Self {
            store,
            tenant,
            reservation: Some(reservation),
            window_secs,
            settled: false,
        }
    }

    /// The held reservation (always present until settled or taken).
    fn reservation(&self) -> &store::DpReservation {
        self.reservation
            .as_ref()
            .expect("DP reservation already settled or taken")
    }

    /// Transfers reservation ownership to the caller without releasing it, so a
    /// detached task can settle it after the runner exits (GHSA-wxqx-r7f6-qq3p).
    /// Prevents `Drop` from releasing the reservation a second time.
    fn take_reservation(&mut self) -> store::DpReservation {
        self.settled = true;
        self.reservation
            .take()
            .expect("DP reservation already settled or taken")
    }
}

impl Drop for DpReservationGuard<'_> {
    fn drop(&mut self) {
        if !self.settled
            && let Some(reservation) = self.reservation.take()
        {
            // An early return: release without billing (the run never produced a
            // marker, or never started).
            settle_dp_reservation(
                self.store,
                self.tenant,
                &reservation,
                &None,
                self.window_secs,
            );
        }
    }
}

// ── request / response types ─────────────────────────────────────────────────

#[derive(Deserialize)]
struct ExecuteRequest {
    code: String,
}

#[derive(Serialize)]
struct ExecuteResponse {
    success: bool,
    rows: Value,
    schema: Value,
    logs: Vec<String>,
    stdout: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    training: Option<Value>,
    /// Differential-privacy audit report parsed from the `[xazz:dp]` marker (v0.6).
    /// None when withDp(...) is unused — the frontend shows this as "budget unconsumed".
    #[serde(skip_serializing_if = "Option::is_none")]
    dp: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<Value>,
    /// Policy-as-Code static guardrail report (v0.7 — issue #2).
    /// For blocked requests it holds the reason; for passed requests it holds warnings.
    #[serde(skip_serializing_if = "Option::is_none")]
    policy: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// Run-history id (issue C1) — absent (None) is omitted from JSON.
    #[serde(skip_serializing_if = "Option::is_none")]
    run_id: Option<i64>,
}

#[derive(Serialize)]
struct SchemaResponse {
    schema: Vec<SchemaColumn>,
    #[serde(rename = "filePath")]
    file_path: String,
}

#[derive(Serialize)]
struct SchemaColumn {
    name: String,
    #[serde(rename = "type")]
    col_type: String,
}

// ── main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // create the uploads/ directory upfront
    let _ = std::fs::create_dir_all("uploads");

    // ── Security: only allow loopback origins via CORS ──────────────────────────
    // This server receives pipeline code to execute and can read arbitrary local
    // files, so it blocks cross-origin requests from arbitrary webpages (remote
    // origins). Both the Vite dev (5173) and same-origin (release web/) scenarios
    // are loopback and thus pass.
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _parts| {
            let b = origin.as_bytes();
            b.starts_with(b"http://localhost")
                || b.starts_with(b"http://127.0.0.1")
                || b.starts_with(b"http://[::1]")
        }))
        .allow_methods(Any)
        .allow_headers(Any);

    let web_root = resolve_web_dir();
    if web_root.is_some() {
        println!("[xazz-server] 📁 Serving IDE from {:?}", web_root);
    }

    let store = Arc::new(store::Store::new());
    let app = Router::new()
        .route("/execute", post(handle_execute))
        // axum caps request bodies at 2 MB by default, which rejected uploads long
        // before handle_schema's own MAX_UPLOAD_BYTES check could answer 413. The
        // extra 1 MB covers multipart framing so that check stays the one that decides.
        .route(
            "/schema",
            post(handle_schema).layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES + 1024 * 1024)),
        )
        .route("/health", get(handle_health))
        .route("/security/audit", post(handle_security_audit))
        .route("/security/verify", post(handle_security_verify))
        .route("/security/audit/log", get(handle_audit_log))
        .route("/security/audit/log/{hash}", get(handle_audit_lookup))
        .route("/security/audit/chain", get(handle_audit_chain))
        .route(
            "/security/policy",
            get(handle_policy_info)
                .put(handle_policy_set)
                .delete(handle_policy_delete),
        )
        .route("/security/policy/check", post(handle_policy_check))
        .route("/security/policy/history", get(handle_policy_history))
        .route(
            "/security/policy/history/ttl",
            get(handle_policy_history_ttl_get)
                .put(handle_policy_history_ttl_set)
                .delete(handle_policy_history_ttl_clear),
        )
        .route(
            "/security/policy/history/ttl/history",
            get(handle_policy_history_ttl_history),
        )
        .route("/security/remediate", post(handle_remediate))
        .route("/security/inference/check", post(handle_inference_check))
        .route("/runs", get(handle_runs_list))
        .route("/runs/{id}", get(handle_run_by_id))
        .route("/dp/budget", get(handle_dp_budget))
        .route("/dp/budget/reset", post(handle_dp_budget_reset))
        .route("/dp/budget/history", get(handle_dp_reset_history))
        .route(
            "/dp/budget/window",
            put(handle_dp_window_set).delete(handle_dp_window_clear),
        )
        .route("/catalog", post(handle_catalog))
        .with_state(AppState {
            exec_permits: Arc::new(Semaphore::new(MAX_CONCURRENT_EXECUTIONS)),
            store: store.clone(),
            tenant_locks: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        })
        .layer(cors);

    // ── Optional Bearer token auth ─────────────────────────────────────────────
    // When `XAZZ_SERVER_TOKEN` is set, every request requires `Authorization: Bearer <token>`.
    // When unset, it operates as a local-only tool (loopback binding + loopback CORS
    // provide the first line of defense).
    let app = app.layer(middleware::from_fn(optional_bearer_auth));

    let app = match web_root {
        Some(root) => app.fallback_service(ServeDir::new(root).not_found_service(serve_index())),
        None => app,
    };

    // Bind address — defaults to loopback (local-only tool). Set XAZZ_BIND to
    // 0.0.0.0:8005 in a container so the service is reachable from outside.
    let addr = std::env::var("XAZZ_BIND").unwrap_or_else(|_| "127.0.0.1:8005".to_string());
    println!("[xazz-server] 🚀 Listening on http://{}", addr);

    // ── Periodic uploads/ cleanup — schema-inference upload files need no long-term retention. ──
    // Every 24 hours, delete files older than 1 hour (disk DoS prevention).
    tokio::spawn(async {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(24 * 60 * 60)).await;
            clean_stale_uploads();
        }
    });

    // ── Periodic policy-history retention sweep (issue C2) ─────────────────────
    // Expired rows are normally pruned inside a pack change, so an idle tenant's
    // rows would otherwise linger on disk (hidden only by the read filter). Sweep
    // every tenant on an interval (`XAZZ_POLICY_HISTORY_SWEEP_SECS`, default 1h;
    // `0` disables).
    let sweep_secs = store::resolve_policy_history_sweep(
        std::env::var(store::POLICY_HISTORY_SWEEP_ENV)
            .ok()
            .as_deref(),
    );
    if sweep_secs > 0 {
        let store = store.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(sweep_secs)).await;
                match store.sweep_expired_policy_history() {
                    Ok(0) => {}
                    Ok(n) => {
                        println!("[xazz-server] 🧹 policy-history sweep removed {n} stale row(s)")
                    }
                    Err(e) => eprintln!("[xazz-server] policy-history sweep failed: {e}"),
                }
            }
        });
    }

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

/// Deletes files older than 1 hour from uploads/.
fn clean_stale_uploads() {
    const MAX_AGE: std::time::Duration = std::time::Duration::from_secs(60 * 60);
    let Ok(entries) = std::fs::read_dir("uploads") else {
        return;
    };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            continue;
        }
        let stale = meta
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age > MAX_AGE);
        if stale {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

// ── Optional Bearer token auth middleware (issue C2) ─────────────────────────

/// Tenant header — the authenticated tenant is passed to handlers via a request
/// extension so `/runs` can scope its queries per tenant.
const TENANT_HEADER: &str = "x-xazz-tenant";

/// Actor header — an administrator may name the identity recorded as
/// `changed_by` when delegating a policy change to a tenant (issue C2).
const ACTOR_HEADER: &str = "x-xazz-actor";

/// Admin credential env var — when set, a request bearing this Bearer token may
/// act on any tenant's policy and is recorded under the actor identity.
const ADMIN_TOKEN_ENV: &str = "XAZZ_ADMIN_TOKEN";

/// Default actor identity when an admin request omits `X-Xazz-Actor`.
const DEFAULT_ACTOR: &str = "admin";

/// Identity of an administrator acting on behalf of a tenant (issue C2).
///
/// Inserted into the request extensions only when the request authenticated with
/// `XAZZ_ADMIN_TOKEN`; absent for self-service tenant requests. Policy-change
/// handlers use it as `changed_by`, so a delegated change is attributed to the
/// administrator rather than the tenant whose namespace was edited.
#[derive(Debug, Clone)]
struct Actor(String);

/// When `XAZZ_SERVER_TOKEN` is set, every request requires `Authorization: Bearer <token>`.
/// When `XAZZ_TENANT_TOKENS` is set (format `tenant1=token1,tenant2=token2`), a request
/// must present `X-Xazz-Tenant: <tenant>` + `Authorization: Bearer <token>` for that tenant.
/// When `XAZZ_ADMIN_TOKEN` is set, a request bearing that token is an administrator:
/// it may target any `X-Xazz-Tenant` namespace and is recorded under `X-Xazz-Actor`
/// (default `admin`) as the policy-change actor.
/// When none is set, all requests pass (default local-only behavior).
async fn optional_bearer_auth(
    mut req: axum::extract::Request,
    next: Next,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let single_token = std::env::var("XAZZ_SERVER_TOKEN").unwrap_or_default();
    let tenant_map = parse_tenant_tokens(&std::env::var("XAZZ_TENANT_TOKENS").unwrap_or_default());
    let admin_token = std::env::var(ADMIN_TOKEN_ENV).unwrap_or_default();

    // No auth mode configured → allow all (local loopback tool).
    if single_token.is_empty() && tenant_map.is_empty() && admin_token.is_empty() {
        req.extensions_mut().insert(String::new());
        return Ok(next.run(req).await);
    }

    let auth = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|h: &HeaderValue| h.to_str().ok())
        .unwrap_or("")
        .to_string();

    // Admin mode: cross-tenant, recorded under the actor identity.
    if !admin_token.is_empty() && auth == format!("Bearer {admin_token}") {
        let tenant = header_str(&req, TENANT_HEADER);
        let actor = header_str(&req, ACTOR_HEADER);
        req.extensions_mut().insert(tenant);
        req.extensions_mut().insert(Actor(resolve_actor(&actor)));
        return Ok(next.run(req).await);
    }

    // Multi-tenant mode: tenant header + per-tenant token.
    if !tenant_map.is_empty() {
        let tenant = header_str(&req, TENANT_HEADER);
        if tenant.is_empty() {
            return Err((
                StatusCode::UNAUTHORIZED,
                "missing X-Xazz-Tenant header".into(),
            ));
        }
        let expected = tenant_map.get(&tenant);
        let ok = expected.is_some_and(|t| auth == format!("Bearer {t}"));
        if !ok {
            return Err((StatusCode::UNAUTHORIZED, "invalid tenant token".into()));
        }
        req.extensions_mut().insert(tenant);
        return Ok(next.run(req).await);
    }

    // Single-token mode.
    if auth == format!("Bearer {single_token}") {
        req.extensions_mut().insert(String::new());
        Ok(next.run(req).await)
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token".into(),
        ))
    }
}

/// Reads a request header as a trimmed owned string (empty when absent/invalid).
fn header_str(req: &axum::extract::Request, name: &str) -> String {
    req.headers()
        .get(name)
        .and_then(|h: &HeaderValue| h.to_str().ok())
        .map(str::trim)
        .unwrap_or("")
        .to_string()
}

/// Resolves the admin actor name from an `X-Xazz-Actor` value, defaulting to
/// [`DEFAULT_ACTOR`] when blank.
fn resolve_actor(raw: &str) -> String {
    if raw.is_empty() {
        DEFAULT_ACTOR.to_string()
    } else {
        raw.to_string()
    }
}

/// Parses `XAZZ_TENANT_TOKENS` (`tenant1=token1,tenant2=token2`) into a map.
fn parse_tenant_tokens(raw: &str) -> std::collections::HashMap<String, String> {
    raw.split(',')
        .filter(|pair| !pair.is_empty())
        .filter_map(|pair| {
            let mut it = pair.splitn(2, '=');
            match (it.next(), it.next()) {
                (Some(k), Some(v)) if !k.is_empty() && !v.is_empty() => {
                    Some((k.trim().to_string(), v.trim().to_string()))
                }
                _ => None,
            }
        })
        .collect()
}

// ── Static IDE serving ───────────────────────────────────────────────────────

/// Finds the built Visual IDE static-asset directory. None if absent.
///
/// Priority:
///   1. `XAZZ_WEB_DIR` environment variable (explicit)
///   2. `web/` next to the executable
///   3. `web/` one level above the executable (when pkg places bin/ and web/ side by side)
///   4. `web/` in the current working directory
fn resolve_web_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XAZZ_WEB_DIR") {
        let p = PathBuf::from(dir);
        if p.join("index.html").exists() {
            return Some(p);
        }
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        candidates.push(parent.join("web"));
        if let Some(grand) = parent.parent() {
            candidates.push(grand.join("web"));
        }
    }
    candidates.push(PathBuf::from("web"));

    candidates
        .into_iter()
        .find(|p| p.join("index.html").exists())
}

/// SPA fallback — returns index.html when ServeDir finds no matching file.
/// Lets the Vite SPA router (/editor, /monitor, etc.) handle routing client-side.
fn serve_index() -> tower_http::services::ServeFile {
    let web_root = resolve_web_dir().unwrap_or_else(|| PathBuf::from("web"));
    let index = web_root.join("index.html");
    tower_http::services::ServeFile::new(index)
}

// ── POST /execute ─────────────────────────────────────────────────────────────

// `ExecuteResponse` is a large, flat wire struct; axum's `IntoResponse` needs the
// concrete `(StatusCode, Json<ExecuteResponse>)` error type, so boxing it would
// break the handler signature. Allow the size lint at this boundary.
#[allow(clippy::result_large_err)]
async fn handle_execute(
    Extension(tenant): Extension<String>,
    State(state): State<AppState>,
    Json(payload): Json<ExecuteRequest>,
) -> Result<Json<ExecuteResponse>, (StatusCode, Json<ExecuteResponse>)> {
    // 0a. Per-tenant serialization (issue C2): a tenant's precheck → run → accrue
    //     must be atomic. Otherwise two concurrent runs both read the same
    //     remaining DP budget and can jointly exceed the envelope. Each tenant has
    //     its own lock, so cross-tenant throughput is unaffected.
    let tenant_mutex = state.tenant_lock(tenant_str(tenant.as_str()));
    let _tenant_guard = tenant_mutex.lock().await;

    // 0b. Concurrency semaphore — if no permit, deny execution (fail-closed, no queue).
    let _permit = match state.exec_permits.try_acquire() {
        Ok(p) => p,
        Err(_) => {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                Json(ExecuteResponse {
                    success: false,
                    rows: json!([]),
                    schema: json!([]),
                    logs: vec![],
                    stdout: String::new(),
                    training: None,
                    dp: None,
                    diagnostics: None,
                    policy: None,
                    error: Some("server is at capacity; try again shortly".to_string()),
                    run_id: None,
                }),
            ));
        }
    };

    // 0. Policy-as-Code static guardrail (issue #2)
    //
    //    On violation this is where it ends — no temp file is created and the xazz
    //    runner is not spawned. It also denies when the policy cannot be loaded
    //    (fail-closed).
    let policy_report =
        match guardrail::gate_for(&state.store, tenant_str(tenant.as_str()), &payload.code) {
            guardrail::Decision::Reject { report } => {
                // Blocks are audit-worthy too — record what was blocked and why.
                if let Err(e) = audit_log::append_with_outcome(&payload.code, Some("blocked")) {
                    eprintln!("[xazz] ⚠️ failed to record block in audit log: {}", e);
                }
                let logs = report
                    .violations
                    .iter()
                    .map(|v| format!("{} {}: {}", v.rule_id, v.rule_name, v.message))
                    .collect::<Vec<_>>();
                return Err((
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(ExecuteResponse {
                        success: false,
                        rows: json!([]),
                        schema: json!([]),
                        logs,
                        stdout: String::new(),
                        training: None,
                        dp: None,
                        diagnostics: None,
                        policy: serde_json::to_value(&report).ok(),
                        error: Some(report.summary()),
                        run_id: None,
                    }),
                ));
            }
            guardrail::Decision::Allow { report, .. } => report,
        };

    // 0c. Cross-instance DP budget reservation (issue C2): the in-process tenant
    //     lock above only serializes runs inside this server. The DB reservation
    //     atomically claims the tenant's whole remaining envelope so a second
    //     instance cannot hand the same budget to a concurrent run. It is released
    //     on every exit path by the guard's Drop; actual spend is billed after the
    //     run produces its `[xazz:dp]` marker.
    let (total_eps, total_delta) = tenant_dp_envelope();
    let window = effective_dp_window(&state.store, tenant_str(tenant.as_str()))
        .map_err(|e| internal_err(format!("DP window 조회 실패: {e}")))?;
    let reservation = state
        .store
        .reserve_dp_budget(
            tenant_str(tenant.as_str()),
            total_eps,
            total_delta,
            window.secs,
            dp_reservation_ttl_secs(),
        )
        .map_err(|e| internal_err(format!("DP 예약 실패: {e}")))?;
    let Some(reservation) = reservation else {
        // Another instance holds this tenant's reservation → fail-closed, no queue.
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(ExecuteResponse {
                success: false,
                rows: json!([]),
                schema: json!([]),
                logs: vec![],
                stdout: String::new(),
                training: None,
                dp: None,
                diagnostics: None,
                policy: None,
                error: Some(
                    "tenant already has an execution in flight; try again shortly".to_string(),
                ),
                run_id: None,
            }),
        ));
    };
    let mut dp_reservation = DpReservationGuard::new(
        &state.store,
        tenant_str(tenant.as_str()),
        reservation,
        window.secs,
    );

    // 1. Save the DSL code to a temp .xzz file
    let tmp = tempfile::Builder::new()
        .suffix(".xzz")
        .tempfile()
        .map_err(|e| internal_err(format!("임시파일 생성 실패: {}", e)))?;

    {
        let mut f = tmp.as_file();
        f.write_all(payload.code.as_bytes())
            .map_err(|e| internal_err(format!("임시파일 쓰기 실패: {}", e)))?;
        f.flush().ok();
    }

    // 2. Locate the xazz.exe executable path
    let exe_path = find_xazz_exe().map_err(internal_err)?;

    // The runner receives the reserved remaining budget, so cross-run composition
    // is enforced by the existing DP accounting (the runner refuses a `withDp`
    // that would exceed it).
    let remaining_eps = dp_reservation
        .reservation()
        .epsilon
        .max(MIN_REMAINING_BUDGET);
    let remaining_delta = dp_reservation.reservation().delta.max(MIN_REMAINING_BUDGET);

    // 3. Run xazz run <tmp.xzz> and finish *all* bookkeeping inside the same
    //    blocking task (GHSA-wxqx-r7f6-qq3p). If the client disconnects, axum
    //    drops this handler future, but the blocking task keeps running to
    //    completion — so the executed pipeline is still accounted for even when
    //    nobody is left to receive the response. Only requests that pass the gate
    //    reach this point — tests verify with the counter.
    guardrail::note_runner_invocation();
    // Hand reservation ownership to the task; disarm the guard so its Drop does
    // not also release it.
    let reservation = dp_reservation.take_reservation();
    drop(dp_reservation);

    Ok(Json(
        run_execution_job(
            state.clone(),
            payload.code.clone(),
            tenant.clone(),
            exe_path,
            tmp,
            reservation,
            window.secs,
            remaining_eps,
            remaining_delta,
            serde_json::to_value(&policy_report).ok(),
        )
        .await,
    ))
}

/// Runs the pipeline and performs *all* post-run bookkeeping — DP settle, audit
/// append, and run-history record — inside a single blocking task
/// (GHSA-wxqx-r7f6-qq3p).
///
/// The work is deliberately self-contained: if the caller's future is dropped
/// (client disconnect), the blocking task still runs to completion and the run
/// remains accounted for. Returns the wire response.
#[allow(clippy::too_many_arguments)]
async fn run_execution_job(
    state: AppState,
    code: String,
    tenant: String,
    exe_path: PathBuf,
    tmp: tempfile::NamedTempFile,
    reservation: store::DpReservation,
    window_secs: u64,
    remaining_eps: f64,
    remaining_delta: f64,
    policy_value: Option<Value>,
) -> ExecuteResponse {
    let policy_for_err = policy_value.clone();
    tokio::task::spawn_blocking(move || {
        let tmp_path = tmp.path().to_path_buf();
        let output = Command::new(&exe_path)
            .arg("run")
            .arg(&tmp_path)
            .env("XAZZ_DP_BUDGET", remaining_eps.to_string())
            .env("XAZZ_DP_DELTA_BUDGET", remaining_delta.to_string())
            .output();

        let (success, stdout, stderr) = match output {
            Ok(output) => (
                output.status.success(),
                String::from_utf8_lossy(&output.stdout).to_string(),
                String::from_utf8_lossy(&output.stderr).to_string(),
            ),
            Err(e) => (false, String::new(), format!("xazz.exe 실행 실패: {}", e)),
        };

        // 4. Parse stdout: extract [xazz:result], [xazz:chart], [xazz:train], [xazz:dp] markers
        let (rows, schema, logs, training, dp, diagnostics) =
            parse_stdout_markers(&stdout, &stderr);

        // 4b. Release the reservation, billing only this run's actual DP consumption
        //     (issue C2). No withDp → nothing billed but the reservation is freed.
        settle_dp_reservation(&state.store, &tenant, &reservation, &dp, window_secs);

        // 5. Auto-audit the execution history (trust infrastructure — persist all
        //    operation history). Even on failure, log only the audit-record failure
        //    as a warning.
        match audit_log::append_with_outcome(
            &code,
            Some(if success { "success" } else { "failed" }),
        ) {
            Ok(rec) => eprintln!(
                "[xazz] 감사 기록 #{} 저장: outcome={}, hash={}",
                rec.index,
                rec.outcome.as_deref().unwrap_or("unknown"),
                &rec.hash[..rec.hash.len().min(12)]
            ),
            Err(e) => eprintln!("[xazz] ⚠️ 감사 로그 저장 실패: {}", e),
        }

        // 5b. Persist the run to history (issue C1) — survives restart.
        let rows_count = rows.as_array().map(|a| a.len() as i64).unwrap_or(0);
        let err_msg_opt = if success {
            None
        } else {
            Some(stderr.lines().last().unwrap_or("실행 실패").to_string())
        };
        let run_id = record_run_in_store(
            &state,
            &code,
            if success { "success" } else { "failed" },
            rows_count,
            err_msg_opt.as_deref(),
            &tenant,
        );

        if success {
            ExecuteResponse {
                success: true,
                rows,
                schema,
                logs,
                stdout,
                training,
                dp,
                diagnostics,
                policy: policy_value,
                error: None,
                run_id: Some(run_id),
            }
        } else {
            ExecuteResponse {
                success: false,
                rows: json!([]),
                schema: json!([]),
                logs,
                stdout,
                training,
                dp,
                diagnostics,
                policy: policy_value,
                error: err_msg_opt,
                run_id: Some(run_id),
            }
        }
    })
    .await
    .unwrap_or_else(|e| ExecuteResponse {
        success: false,
        rows: json!([]),
        schema: json!([]),
        logs: vec![],
        stdout: String::new(),
        training: None,
        dp: None,
        diagnostics: None,
        policy: policy_for_err,
        error: Some(format!("spawn_blocking 실패: {}", e)),
        run_id: None,
    })
}

/// Parses the [xazz:result], [xazz:chart], [xazz:train], [xazz:diagnostics], [xazz:dp] markers from stdout.
fn parse_stdout_markers(
    stdout: &str,
    stderr: &str,
) -> (
    Value,
    Value,
    Vec<String>,
    Option<Value>,
    Option<Value>,
    Option<Value>,
) {
    let mut rows = json!([]);
    let mut schema = json!([]);
    let mut training: Option<Value> = None;
    let mut dp: Option<Value> = None;
    let mut diagnostics: Option<Value> = None;
    let logs: Vec<String> = stderr.lines().map(|l| l.to_string()).collect();

    let lines: Vec<&str> = stdout.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if let Some(json_part) = trimmed.strip_prefix("[xazz:result] ")
            && let Ok(parsed) = serde_json::from_str::<Value>(json_part)
        {
            if let Some(r) = parsed.get("rows") {
                rows = r.clone();
            }
            if let Some(s) = parsed.get("schema") {
                schema = s.clone();
            }
        }
        // Burn deep-learning training result marker (JSON on the same line)
        if let Some(json_part) = trimmed.strip_prefix("[xazz:train] ")
            && let Ok(parsed) = serde_json::from_str::<Value>(json_part)
        {
            training = Some(parsed);
        }
        // Differential-privacy audit marker — single-line self-contained:
        //   [xazz:dp] <JSON>            (new form — safe even if broken by newlines/emojis)
        //   [xazz:dp]\n<JSON>           (legacy form — JSON on the next line, kept for compatibility)
        if let Some(json_part) = trimmed.strip_prefix("[xazz:dp] ") {
            if let Ok(parsed) = serde_json::from_str::<Value>(json_part) {
                dp = Some(parsed);
            }
        } else if trimmed == "[xazz:dp]" {
            let next = lines.get(i + 1).map(|l| l.trim()).unwrap_or("");
            if let Ok(parsed) = serde_json::from_str::<Value>(next) {
                dp = Some(parsed);
                i += 1; // JSON line is consumed
            }
        }
        // Policy-as-Code guardrail marker — policy report emitted by the execution engine.
        // The server already ran the same check upstream, but logs the marker as-is so
        // the execution engine's verdict can be trusted.
        if let Some(json_part) = trimmed.strip_prefix("[xazz:policy] ")
            && serde_json::from_str::<Value>(json_part).is_err()
        {
            eprintln!("[xazz] ⚠️ [xazz:policy] 마커 파싱 실패");
        }
        // Static semantic analysis (Type Checker) diagnostics marker
        if let Some(json_part) = trimmed.strip_prefix("[xazz:diagnostics] ")
            && let Ok(parsed) = serde_json::from_str::<Value>(json_part)
        {
            diagnostics = Some(parsed);
        }
        i += 1;
    }

    (rows, schema, logs, training, dp, diagnostics)
}

// ── POST /schema ──────────────────────────────────────────────────────────────

fn validate_upload_size(len: usize) -> Result<(), StatusCode> {
    if len > MAX_UPLOAD_BYTES {
        Err(StatusCode::PAYLOAD_TOO_LARGE)
    } else {
        Ok(())
    }
}

async fn handle_schema(
    mut multipart: Multipart,
) -> Result<Json<SchemaResponse>, (StatusCode, String)> {
    // Extract the file field from the multipart
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut original_name = "upload.csv".to_string();
    let too_large = || {
        (
            StatusCode::PAYLOAD_TOO_LARGE,
            format!(
                "파일이 너무 큽니다. 최대 {} MB 까지 허용됩니다.",
                MAX_UPLOAD_BYTES / (1024 * 1024)
            ),
        )
    };
    // The route's body limit surfaces here as a multipart error whose status is 413;
    // answer it with the same message as the explicit size check below.
    let read_err = |e: axum::extract::multipart::MultipartError, what: &str| {
        if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
            too_large()
        } else {
            (StatusCode::BAD_REQUEST, format!("{what}: {e}"))
        }
    };

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| read_err(e, "multipart 파싱 실패"))?
    {
        if field.name() == Some("file") {
            original_name = field.file_name().unwrap_or("upload.csv").to_string();
            let data = field
                .bytes()
                .await
                .map_err(|e| read_err(e, "파일 읽기 실패"))?;
            // Upload size upper bound — reject if exceeded (disk DoS prevention).
            validate_upload_size(data.len()).map_err(|_| too_large())?;
            file_bytes = Some(data.to_vec());
        }
    }

    let bytes = file_bytes.ok_or((StatusCode::BAD_REQUEST, "파일 필드 없음".to_string()))?;

    // Build the save path (uploads/<uuid>_<name>)
    let uid = uuid::Uuid::new_v4().to_string();
    let safe_name: String = original_name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let file_path = format!("uploads/{}_{}", uid, safe_name);
    std::fs::write(&file_path, &bytes).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("파일 저장 실패: {}", e),
        )
    })?;

    // Encoding detection and CSV parsing
    let text = decode_bytes(&bytes);
    let schema = infer_csv_schema_from_text(&text);

    Ok(Json(SchemaResponse { schema, file_path }))
}

// ── CSV schema inference (same logic as xazz import) ──────────────────────────

fn decode_bytes(bytes: &[u8]) -> String {
    match String::from_utf8(bytes.to_vec()) {
        Ok(s) => s,
        Err(_) => {
            let (cow, _, _) = encoding_rs::EUC_KR.decode(bytes);
            cow.into_owned()
        }
    }
}

fn infer_csv_schema_from_text(text: &str) -> Vec<SchemaColumn> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(text.as_bytes());

    let headers: Vec<String> = match rdr.headers() {
        Ok(h) => h.iter().map(|s| s.to_string()).collect(),
        Err(_) => return vec![],
    };

    // Collect sample values per column
    let col_count = headers.len();
    let mut samples: Vec<Vec<String>> = vec![Vec::new(); col_count];

    for (i, result) in rdr.records().enumerate() {
        if i >= 100 {
            break;
        }
        if let Ok(record) = result {
            for (j, val) in record.iter().enumerate() {
                if j < col_count && !val.trim().is_empty() {
                    samples[j].push(val.trim().to_string());
                }
            }
        }
    }

    headers
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let col_type = infer_type(&samples[i]);
            SchemaColumn {
                name: name.clone(),
                col_type,
            }
        })
        .collect()
}

fn infer_type(values: &[String]) -> String {
    if values.is_empty() {
        return "string".to_string();
    }

    let mut all_bool = true;
    let mut all_int = true;
    let mut all_float = true;

    for v in values {
        let lower = v.to_lowercase();
        if lower != "true" && lower != "false" && lower != "1" && lower != "0" {
            all_bool = false;
        }
        if v.parse::<i64>().is_err() {
            all_int = false;
        }
        if v.parse::<f64>().is_err() {
            all_float = false;
        }
    }

    if all_bool
        && values
            .iter()
            .all(|v| matches!(v.to_lowercase().as_str(), "true" | "false"))
    {
        "bool".to_string()
    } else if all_int {
        "int".to_string()
    } else if all_float {
        "float".to_string()
    } else {
        "string".to_string()
    }
}

// ── GET /health ────────────────────────────────────────────────────────────────

async fn handle_health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "timestamp": chrono::Utc::now().to_rfc3339(),
    }))
}

// ── POST /security/audit ─────────────────────────────────────────────────────

#[derive(Deserialize)]
struct AuditRequest {
    code: String,
}

#[derive(Serialize)]
struct AuditResponse {
    hash: String,
    algorithm: String,
    timestamp: String,
    code_length: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    index: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    record_hash: Option<String>,
}

async fn handle_security_audit(Json(payload): Json<AuditRequest>) -> Json<AuditResponse> {
    let hash = audit_log::hash_code(&payload.code);

    // Persist to the append-only audit log (the hash is returned even on failure)
    let stored = audit_log::append(&payload.code);

    match stored {
        Ok(record) => Json(AuditResponse {
            hash,
            algorithm: "SHA-256".to_string(),
            timestamp: record.timestamp.clone(),
            code_length: payload.code.len(),
            index: Some(record.index),
            record_hash: Some(record.record_hash.clone()),
        }),
        Err(_e) => Json(AuditResponse {
            hash,
            algorithm: "SHA-256".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            code_length: payload.code.len(),
            index: None,
            record_hash: None,
        }),
    }
}

// ── POST /security/verify ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct VerifyRequest {
    code: String,
    hash: String,
}

#[derive(Serialize)]
struct VerifyResponse {
    valid: bool,
    computed_hash: String,
    provided_hash: String,
    algorithm: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    logged: Option<bool>,
    /// States the semantic limit of verification — `valid` only proves "input hash == hash
    /// recorded in the audit log", not that the code was actually executed.
    /// Execution status can only be inferred from the record's `outcome` field.
    note: String,
}

async fn handle_security_verify(Json(payload): Json<VerifyRequest>) -> Json<VerifyResponse> {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(payload.code.as_bytes());
    let computed = hex::encode(hasher.finalize());
    let valid = computed == payload.hash;
    // Also return whether it exists in the log
    let logged = audit_log::lookup_by_hash(&payload.hash)
        .ok()
        .map(|r| !r.is_empty());

    Json(VerifyResponse {
        valid,
        computed_hash: computed,
        provided_hash: payload.hash,
        algorithm: "SHA-256".to_string(),
        logged,
        note: "sha256(input) == recorded hash only proves the code was audited; it does not prove the code was executed. Check the record's outcome for execution status.".to_string(),
    })
}

// ── GET /security/audit/log ──────────────────────────────────────────────────

async fn handle_audit_log() -> Result<Json<Value>, (StatusCode, String)> {
    let records = audit_log::all().map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(json!({ "count": records.len(), "records": records })))
}

// ── GET /security/audit/log/:hash ────────────────────────────────────────────

async fn handle_audit_lookup(
    Path(hash): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let records =
        audit_log::lookup_by_hash(&hash).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if records.is_empty() {
        return Err((
            StatusCode::NOT_FOUND,
            format!("해시 '{}' 에 해당하는 감사 레코드가 없습니다.", hash),
        ));
    }
    Ok(Json(
        json!({ "hash": hash, "matches": records.len(), "records": records }),
    ))
}

// ── GET /security/audit/chain ────────────────────────────────────────────────

async fn handle_audit_chain() -> Result<Json<Value>, (StatusCode, String)> {
    let valid = audit_log::verify_chain().map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let count = audit_log::all().map(|r| r.len()).unwrap_or(0);
    Ok(Json(json!({ "intact": valid, "records": count })))
}

// ── GET /security/policy ─────────────────────────────────────────────────────

/// Returns the currently active Policy-as-Code policy for the authenticated tenant.
///
/// The frontend can use this response to show the user "which column is blocked
/// and why" in advance. If policy loading fails, it returns 500 with the reason.
async fn handle_policy_info(
    Extension(tenant): Extension<String>,
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match guardrail::load_policy_for(&state.store, tenant_str(tenant.as_str())) {
        Ok((policy, origin)) => Ok(Json(json!({
            "tenant": tenant_str(tenant.as_str()),
            "origin": origin,
            "policy": policy,
            "slm": guardrail::SlmStatus::from_config(&slm::SlmConfig::from_env()),
        }))),
        Err(report) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": report.summary(), "policy": report })),
        )),
    }
}

// ── PUT /security/policy ─────────────────────────────────────────────────────

/// Resolves the audit actor for a policy change (issue C2).
///
/// A delegated admin change is attributed to the [`Actor`]; a self-service
/// tenant change is attributed to the tenant itself. An admin request must name
/// a target tenant via `X-Xazz-Tenant`, so a delegated change can never land in
/// the empty/global namespace by accident.
fn policy_change_actor(
    tenant: &str,
    actor: Option<&Extension<Actor>>,
) -> Result<String, (StatusCode, Json<Value>)> {
    match actor {
        Some(_) if tenant.is_empty() => Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "admin policy change requires X-Xazz-Tenant" })),
        )),
        Some(Extension(a)) => Ok(a.0.clone()),
        None => Ok(tenant.to_string()),
    }
}

/// Stores (or replaces) the authenticated tenant's policy pack — issue C2.
///
/// `self-service`: the tenant writes only its own namespace. An administrator
/// authenticated with `XAZZ_ADMIN_TOKEN` may instead target the namespace named
/// by `X-Xazz-Tenant`; the change is then recorded under `X-Xazz-Actor`
/// (default `admin`) rather than the tenant. The body is validated with the same
/// parser used at load time, so an unparseable pack is rejected here instead of
/// becoming a later fail-closed denial.
async fn handle_policy_set(
    Extension(tenant): Extension<String>,
    actor: Option<Extension<Actor>>,
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let tenant = tenant_str(tenant.as_str());
    let changed_by = policy_change_actor(tenant, actor.as_ref())?;
    let text = payload.to_string();
    let policy = xazz_compiler::Policy::from_json_str(&text)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e.message }))))?;

    state
        .store
        .set_tenant_policy(tenant, &text, &changed_by)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e })),
            )
        })?;

    Ok(Json(json!({
        "tenant": tenant,
        "origin": guardrail::tenant_origin(tenant),
        "policy": policy,
    })))
}

// ── DELETE /security/policy ──────────────────────────────────────────────────

/// Removes the authenticated tenant's stored policy pack — issue C2.
///
/// After deletion the tenant falls back to the global policy / builtin baseline.
/// As with [`handle_policy_set`], an admin may delete another tenant's pack and
/// is recorded as the change actor.
async fn handle_policy_delete(
    Extension(tenant): Extension<String>,
    actor: Option<Extension<Actor>>,
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let tenant = tenant_str(tenant.as_str());
    let changed_by = policy_change_actor(tenant, actor.as_ref())?;
    let deleted = state
        .store
        .delete_tenant_policy(tenant, &changed_by)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e })),
            )
        })?;
    Ok(Json(json!({ "tenant": tenant, "deleted": deleted })))
}

// ── GET /security/policy/history ─────────────────────────────────────────────

/// Returns the authenticated tenant's append-only policy-pack change history.
///
/// Each entry records the action (`set`/`delete`), the previous and new pack JSON,
/// who changed it, and when — so a pack replacement or removal is auditable even
/// though `tenant_policies` only keeps the latest state (issue C2). Stored packs
/// are returned as embedded JSON (falling back to a string if a legacy row is not
/// parseable) rather than escaped text. `?limit=&offset=` page the newest-first
/// list; the response echoes the effective page plus the tenant's effective
/// retention window (`ttl_secs`/`ttl_source`) so a caller can tell how much of
/// the history has already expired (issue C2).
async fn handle_policy_history(
    Extension(tenant): Extension<String>,
    State(state): State<AppState>,
    Query(page): Query<PolicyHistoryQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let tenant = tenant_str(tenant.as_str());
    let limit = page.limit();
    let offset = page.offset();
    let ttl = effective_policy_history_ttl(&state.store, tenant).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e })),
        )
    })?;
    let records = state
        .store
        .list_policy_history(tenant, limit, offset)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e })),
            )
        })?;
    let history: Vec<Value> = records
        .into_iter()
        .map(|r| {
            json!({
                "id": r.id,
                "tenant": r.tenant,
                "action": r.action,
                "old_policy_json": embed_policy_json(r.old_policy_json),
                "new_policy_json": embed_policy_json(r.new_policy_json),
                "changed_by": r.changed_by,
                "changed_at": r.changed_at,
            })
        })
        .collect();
    Ok(Json(json!({
        "tenant": tenant,
        "limit": limit,
        "offset": offset,
        "ttl_secs": ttl.secs,
        "ttl_source": ttl.source,
        "history": history,
    })))
}

/// Parses a stored policy-pack JSON text for embedding; unparseable legacy rows
/// are returned as a plain string so the endpoint never fails on history.
fn embed_policy_json(raw: Option<String>) -> Value {
    match raw {
        Some(s) => serde_json::from_str(&s).unwrap_or(Value::String(s)),
        None => Value::Null,
    }
}

// ── Per-tenant policy-history retention window (issue C2) ─────────────────────

/// A tenant's effective policy-history retention window and where it came from.
struct PolicyHistoryTtl {
    /// Window length in seconds (`0` = no time-based expiry).
    secs: u64,
    /// `"tenant"` when a stored per-tenant override is in effect, else `"global"`.
    source: &'static str,
}

/// Resolves a tenant's effective policy-history retention window — issue C2.
///
/// A stored per-tenant override (`tenant_policy_history_config`) takes precedence
/// over the global `XAZZ_TENANT_POLICY_HISTORY_TTL_SECS` default. An explicit
/// stored `0` is a tenant override (source `"tenant"`), distinct from having no
/// override.
fn effective_policy_history_ttl(
    store: &store::Store,
    tenant: &str,
) -> Result<PolicyHistoryTtl, String> {
    match store.get_policy_history_ttl(tenant)? {
        Some(secs) => Ok(PolicyHistoryTtl {
            secs,
            source: "tenant",
        }),
        None => Ok(PolicyHistoryTtl {
            secs: store.policy_history_ttl_default(),
            source: "global",
        }),
    }
}

/// Reports the authenticated tenant's effective policy-history retention window — issue C2.
///
/// Returns the stored per-tenant override when present (`ttl_source: "tenant"`),
/// otherwise the global `XAZZ_TENANT_POLICY_HISTORY_TTL_SECS` default
/// (`ttl_source: "global"`). Read-only and tenant-scoped.
async fn handle_policy_history_ttl_get(
    State(state): State<AppState>,
    Extension(tenant): Extension<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let tenant = tenant_str(tenant.as_str());
    let ttl = effective_policy_history_ttl(&state.store, tenant)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(json!({
        "tenant": tenant,
        "ttl_secs": ttl.secs,
        "ttl_source": ttl.source,
    })))
}

/// Request body for `PUT /security/policy/history/ttl`.
#[derive(Deserialize)]
struct PolicyHistoryTtlRequest {
    /// Retention window in seconds; `0` disables time-based expiry for the tenant.
    ttl_secs: u64,
}

/// Sets the authenticated tenant's policy-history retention-window override — issue C2.
///
/// Self-service and tenant-scoped: only the caller's override is written; the
/// global `XAZZ_TENANT_POLICY_HISTORY_TTL_SECS` remains the fallback for tenants
/// without one. `ttl_secs: 0` stores an explicit "no time-based expiry" override,
/// which differs from deleting the override (which restores the global default).
async fn handle_policy_history_ttl_set(
    State(state): State<AppState>,
    Extension(tenant): Extension<String>,
    Json(payload): Json<PolicyHistoryTtlRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let tenant = tenant_str(tenant.as_str());
    state
        .store
        .set_policy_history_ttl(tenant, payload.ttl_secs, tenant)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(json!({
        "tenant": tenant,
        "ttl_secs": payload.ttl_secs,
        "ttl_source": "tenant",
    })))
}

/// Removes the authenticated tenant's policy-history retention-window override — issue C2.
///
/// After removal the tenant falls back to the global
/// `XAZZ_TENANT_POLICY_HISTORY_TTL_SECS` default. Tenant-scoped: other tenants'
/// overrides are untouched.
async fn handle_policy_history_ttl_clear(
    State(state): State<AppState>,
    Extension(tenant): Extension<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let tenant = tenant_str(tenant.as_str());
    state
        .store
        .clear_policy_history_ttl(tenant, tenant)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let ttl = effective_policy_history_ttl(&state.store, tenant)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(json!({
        "tenant": tenant,
        "ttl_secs": ttl.secs,
        "ttl_source": ttl.source,
    })))
}

/// Returns the authenticated tenant's append-only retention-override change
/// history — issue C2.
///
/// Each entry records the action (`set`/`clear`), the previous and new override
/// (in seconds), who changed it, and when — so a change to the tenant's
/// policy-history retention window is auditable even though
/// `tenant_policy_history_config` only keeps the latest state. `?limit=&offset=`
/// page the newest-first list.
async fn handle_policy_history_ttl_history(
    Extension(tenant): Extension<String>,
    State(state): State<AppState>,
    Query(page): Query<PolicyHistoryQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let tenant = tenant_str(tenant.as_str());
    let limit = page.limit();
    let offset = page.offset();
    let records = state
        .store
        .list_policy_history_ttl_history(tenant, limit, offset)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e })),
            )
        })?;
    let history: Vec<Value> = records
        .into_iter()
        .map(|r| {
            json!({
                "id": r.id,
                "tenant": r.tenant,
                "action": r.action,
                "old_ttl_secs": r.old_ttl_secs,
                "new_ttl_secs": r.new_ttl_secs,
                "changed_by": r.changed_by,
                "changed_at": r.changed_at,
            })
        })
        .collect();
    Ok(Json(json!({
        "tenant": tenant,
        "limit": limit,
        "offset": offset,
        "history": history,
    })))
}

// ── POST /security/policy/check ──────────────────────────────────────────────

/// Performs only a static guardrail check without executing the code.
///
/// The Visual IDE calls this endpoint while editing to show violations before the
/// run button is pressed. Even with violations it returns HTTP 200 — the check
/// itself succeeded; the verdict is in the body's `safe_to_execute`.
async fn handle_policy_check(
    Extension(tenant): Extension<String>,
    State(state): State<AppState>,
    Json(payload): Json<guardrail::CodeRequest>,
) -> Result<Json<guardrail::PolicyCheckResponse>, (StatusCode, Json<Value>)> {
    let (policy, origin) = guardrail::load_policy_for(&state.store, tenant_str(tenant.as_str()))
        .map_err(|report| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": report.summary(), "policy": report })),
            )
        })?;

    let report = xazz_compiler::check_policy(&payload.code, &policy);
    Ok(Json(guardrail::PolicyCheckResponse {
        safe_to_execute: report.safe_to_execute,
        policy_origin: origin,
        policy: report,
    }))
}

// ── POST /security/remediate ─────────────────────────────────────────────────

/// Remediates blocked code into a safe replacement and returns the violation report.
///
/// The remediation strategy has two stages.
///   1. Deterministic remediation — directly edits the AST to produce safe code that
///      always works.
///   2. On-premise sLM (Qwen2.5-Coder) — if enabled, suggests a more natural rewrite,
///      but it is adopted **only when it passes re-verification by the same policy engine**.
///
/// If the response's `remediation.verified` is false, human-handled violations remain.
/// In that case, the remediated code must not be marked "safe".
async fn handle_remediate(
    Extension(tenant): Extension<String>,
    State(state): State<AppState>,
    Json(payload): Json<guardrail::CodeRequest>,
) -> Result<Json<guardrail::RemediateResponse>, (StatusCode, Json<Value>)> {
    let (policy, origin) = guardrail::load_policy_for(&state.store, tenant_str(tenant.as_str()))
        .map_err(|report| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": report.summary(), "policy": report })),
            )
        })?;

    let cfg = slm::SlmConfig::from_env();
    let report = xazz_compiler::check_policy(&payload.code, &policy);
    let remediation = guardrail::remediate_with_slm(&payload.code, &policy, &cfg).await;

    Ok(Json(guardrail::RemediateResponse {
        safe_to_execute: report.safe_to_execute,
        policy_origin: origin,
        policy: report,
        remediation,
        slm: guardrail::SlmStatus::from_config(&cfg),
    }))
}

// ── POST /security/inference/check ───────────────────────────────────────────

/// Inference-call request: the code that produced the call + the prompt/response pair.
#[derive(Deserialize)]
struct InferenceCheckRequest {
    /// The `.xzz` source that issued the inference call
    code: String,
    /// The prompt sent to the model
    prompt: String,
    /// The model's response — re-scanned at runtime for PII/secrets
    response: String,
    /// Optional SHA-256 fingerprint of the model weights that produced the
    /// response (F5 cohort, F4 #73). Recorded in the audit chain when present.
    #[serde(default)]
    model_fingerprint: Option<String>,
}

#[derive(Serialize, Debug)]
struct InferenceFinding {
    kind: String,
    line: usize,
    col: usize,
    /// Masked value — the raw value is never returned
    redacted: String,
}

#[derive(Serialize)]
struct InferenceCheckResponse {
    /// true when the response is safe — no PII/secret literals
    safe_to_emit: bool,
    /// Runtime re-scan findings (LLM output gate, issue #71 / F2)
    findings: Vec<InferenceFinding>,
    /// Audit-chain index of the per-call evidence record
    audit_index: u64,
    /// Audit chain still valid after appending
    chain_valid: bool,
    /// SHA-256 of the prompt (evidence, not the prompt itself)
    prompt_hash: String,
    /// SHA-256 of the response (evidence, not the response itself)
    response_hash: String,
    /// SHA-256 of the model weights, echoed when the caller supplied it (F4 #73)
    #[serde(skip_serializing_if = "Option::is_none")]
    model_fingerprint: Option<String>,
}

/// Runtime output gate: re-scans a generated response against the same policy
/// literals and records the prompt/response pair in the audit chain as per-call
/// evidence. The response text is **never stored** — only its SHA-256 hash.
///
/// Static analysis (F1) cannot cover generative output, so this is the runtime
/// complement: a flagged response must be blocked/filtered before it is emitted.
async fn handle_inference_check(
    Json(payload): Json<InferenceCheckRequest>,
) -> Result<Json<InferenceCheckResponse>, (StatusCode, String)> {
    use xazz_compiler::policy::patterns::{SecretKind, scan_output_text};

    // 1. Runtime re-scan of the generated response (free-form text, not code).
    let findings: Vec<InferenceFinding> = scan_output_text(&payload.response)
        .into_iter()
        .map(|f| InferenceFinding {
            kind: match f.kind {
                SecretKind::ResidentRegistrationNumber => {
                    "resident_registration_number".to_string()
                }
                SecretKind::PhoneNumber => "phone_number".to_string(),
                SecretKind::Email => "email".to_string(),
                SecretKind::CreditCard => "credit_card".to_string(),
                SecretKind::ApiKey => "api_key".to_string(),
                SecretKind::PrivateKey => "private_key".to_string(),
                SecretKind::GenericSecret => "generic_secret".to_string(),
            },
            line: f.line,
            col: f.col,
            redacted: f.redacted,
        })
        .collect();
    let safe_to_emit = findings.is_empty();

    // 2. Record the call in the audit chain as per-call evidence.
    let record = audit_log::append_inference_call(
        &payload.code,
        &payload.prompt,
        &payload.response,
        payload.model_fingerprint.as_deref(),
        if safe_to_emit {
            Some("safe")
        } else {
            Some("blocked")
        },
    )
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let chain_valid = audit_log::verify_chain().unwrap_or(false);

    Ok(Json(InferenceCheckResponse {
        safe_to_emit,
        findings,
        audit_index: record.index,
        chain_valid,
        prompt_hash: record.prompt_hash.unwrap_or_default(),
        response_hash: record.response_hash.unwrap_or_default(),
        model_fingerprint: record.model_fingerprint,
    }))
}

// ── Run history (issue C1) ────────────────────────────────────────────────────

/// Lists runs newest-first, scoped to the authenticated tenant.
async fn handle_runs_list(
    State(state): State<AppState>,
    Extension(tenant): Extension<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let tenant = tenant_str(tenant.as_str());
    let runs = state
        .store
        .list_runs(50, tenant)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(json!({ "tenant": tenant, "runs": runs })))
}

/// Fetches a single run by id, scoped to the authenticated tenant.
async fn handle_run_by_id(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    Extension(tenant): Extension<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let tenant = tenant_str(tenant.as_str());
    match state
        .store
        .get_run(id, tenant)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?
    {
        Some(run) => Ok(Json(json!(run))),
        None => Err((
            StatusCode::NOT_FOUND,
            format!("run {id} not found (or not in tenant '{tenant}')"),
        )),
    }
}

// ── Per-tenant DP budget (issue C2) ──────────────────────────────────────────

/// Reports the authenticated tenant's cumulative DP spend and remaining envelope.
///
/// When a budget window is configured (`XAZZ_TENANT_DP_WINDOW_SECS > 0`) the spend
/// reflects the current window only, and `resets_at` gives the epoch second at which
/// the next automatic roll happens (`0` when no window is configured).
async fn handle_dp_budget(
    State(state): State<AppState>,
    Extension(tenant): Extension<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let tenant = tenant_str(tenant.as_str());
    let (total_eps, total_delta) = tenant_dp_envelope();
    let window = effective_dp_window(&state.store, tenant)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let (spent_eps, spent_delta) = state
        .store
        .dp_spent(tenant, window.secs)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(dp_budget_view(
        &state,
        tenant,
        total_eps,
        total_delta,
        window,
        spent_eps,
        spent_delta,
    )?))
}

/// Resolves the audit actor for a DP budget reset (issue #124).
///
/// A delegated admin reset is attributed to the [`Actor`] and must name a target
/// tenant via `X-Xazz-Tenant`; a self-service reset is attributed to the tenant.
fn dp_reset_actor(
    tenant: &str,
    actor: Option<&Extension<Actor>>,
) -> Result<String, (StatusCode, String)> {
    match actor {
        Some(_) if tenant.is_empty() => Err((
            StatusCode::BAD_REQUEST,
            "admin DP reset requires X-Xazz-Tenant".to_string(),
        )),
        Some(Extension(a)) => Ok(a.0.clone()),
        None => Ok(tenant.to_string()),
    }
}

/// Resets the authenticated tenant's accumulated DP spend and window — issue C2.
///
/// Tenant-scoped (self-service): only the caller's ledger is cleared. An admin
/// authenticated with `XAZZ_ADMIN_TOKEN` may instead target the namespace named
/// by `X-Xazz-Tenant`, and the reset is recorded under `X-Xazz-Actor` (issue
/// #124). Every reset appends to `dp_reset_history`, so it stays auditable even
/// though `dp_budget` only keeps the current value.
async fn handle_dp_budget_reset(
    State(state): State<AppState>,
    Extension(tenant): Extension<String>,
    actor: Option<Extension<Actor>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let tenant = tenant_str(tenant.as_str());
    let actor = dp_reset_actor(tenant, actor.as_ref())?;
    let record = state
        .store
        .reset_dp_budget_audited(tenant, &actor)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let (total_eps, total_delta) = tenant_dp_envelope();
    let window = effective_dp_window(&state.store, tenant)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let mut view = dp_budget_view(&state, tenant, total_eps, total_delta, window, 0.0, 0.0)?;
    if let Some(obj) = view.as_object_mut() {
        obj.insert("reset_by".to_string(), json!(record.actor));
        obj.insert("reset_at".to_string(), json!(record.reset_at));
        obj.insert(
            "spent_epsilon_before".to_string(),
            json!(record.spent_epsilon_before),
        );
        obj.insert(
            "spent_delta_before".to_string(),
            json!(record.spent_delta_before),
        );
    }
    Ok(Json(view))
}

/// Returns the tenant's append-only DP budget reset history — issue #124.
async fn handle_dp_reset_history(
    State(state): State<AppState>,
    Extension(tenant): Extension<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let tenant = tenant_str(tenant.as_str());
    let resets = state
        .store
        .list_dp_resets(tenant, 50)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(json!({ "tenant": tenant, "resets": resets })))
}

/// Request body for `PUT /dp/budget/window`.
#[derive(Deserialize)]
struct DpWindowRequest {
    /// Window length in seconds; `0` disables the window (cumulative budget).
    window_secs: u64,
}

/// Sets the authenticated tenant's DP budget window override — issue C2.
///
/// Self-service and tenant-scoped: only the caller's override is written; the
/// global `XAZZ_TENANT_DP_WINDOW_SECS` remains the fallback for tenants without
/// one. `window_secs: 0` stores an explicit "cumulative" override, which differs
/// from deleting the override (which restores the global default).
async fn handle_dp_window_set(
    State(state): State<AppState>,
    Extension(tenant): Extension<String>,
    Json(payload): Json<DpWindowRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let tenant = tenant_str(tenant.as_str());
    if payload.window_secs > MAX_DP_WINDOW_SECS {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("window_secs must be at most {MAX_DP_WINDOW_SECS} seconds (~10 years)"),
        ));
    }
    state
        .store
        .set_dp_window(tenant, payload.window_secs)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let (total_eps, total_delta) = tenant_dp_envelope();
    let (spent_eps, spent_delta) = state
        .store
        .dp_spent(tenant, payload.window_secs)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(dp_budget_view(
        &state,
        tenant,
        total_eps,
        total_delta,
        DpWindow {
            secs: payload.window_secs,
            source: "tenant",
        },
        spent_eps,
        spent_delta,
    )?))
}

/// Removes the authenticated tenant's DP budget window override — issue C2.
///
/// After removal the tenant falls back to the global `XAZZ_TENANT_DP_WINDOW_SECS`
/// default. Tenant-scoped: other tenants' overrides are untouched.
async fn handle_dp_window_clear(
    State(state): State<AppState>,
    Extension(tenant): Extension<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let tenant = tenant_str(tenant.as_str());
    state
        .store
        .clear_dp_window(tenant)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let (total_eps, total_delta) = tenant_dp_envelope();
    let window = effective_dp_window(&state.store, tenant)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let (spent_eps, spent_delta) = state
        .store
        .dp_spent(tenant, window.secs)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(dp_budget_view(
        &state,
        tenant,
        total_eps,
        total_delta,
        window,
        spent_eps,
        spent_delta,
    )?))
}

/// Builds the `GET /dp/budget` response body from the tenant's ledger state.
fn dp_budget_view(
    state: &AppState,
    tenant: &str,
    total_eps: f64,
    total_delta: f64,
    window: DpWindow,
    spent_eps: f64,
    spent_delta: f64,
) -> Result<Value, (StatusCode, String)> {
    let anchor = state
        .store
        .dp_window_started_at(tenant)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?
        .unwrap_or(0);
    let resets_at = if window.secs > 0 && anchor > 0 {
        anchor + window.secs as i64
    } else {
        0
    };
    // An in-flight run holds the tenant's remaining envelope in `dp_reservation`.
    // Subtract it here so `remaining_*` never overstates what a new run could
    // actually claim before that reservation is settled (issue #123).
    let reservation = state
        .store
        .live_dp_reservation(tenant)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let (reserved_eps, reserved_delta, reservation_expires_at) = match &reservation {
        Some((r, expires_at)) => (r.epsilon, r.delta, *expires_at),
        None => (0.0, 0.0, 0),
    };
    Ok(json!({
        "tenant": tenant,
        "spent_epsilon": spent_eps,
        "spent_delta": spent_delta,
        "reserved_epsilon": reserved_eps,
        "reserved_delta": reserved_delta,
        "total_epsilon": total_eps,
        "total_delta": total_delta,
        "remaining_epsilon": (total_eps - spent_eps - reserved_eps).max(0.0),
        "remaining_delta": (total_delta - spent_delta - reserved_delta).max(0.0),
        "in_flight": reservation.is_some(),
        "reservation_expires_at": reservation_expires_at,
        "window_secs": window.secs,
        "window_source": window.source,
        "window_started_at": anchor,
        "resets_at": resets_at,
    }))
}

// ── Pipeline catalog / column lineage (issue C3) ─────────────────────────────

/// Compiles the given code and returns the pipeline catalog + column lineage.
///
/// The Rust compiler is the single source of truth: the same Typed IR that
/// drives execution also produces this catalog, so a reviewer can trace any
/// pipeline's output columns back to their source columns.
async fn handle_catalog(
    Json(payload): Json<ExecuteRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let (parse, check) = xazz_compiler::compile_ir(&payload.code);
    let Ok((_program, ir)) = parse else {
        return Err((StatusCode::BAD_REQUEST, "failed to parse code".into()));
    };
    if !check.errors.is_empty() {
        let first = check.errors[0].message.clone();
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("compile error: {first}"),
        ));
    }
    let catalog = xazz_compiler::catalog::build_catalog(&ir);
    Ok(Json(json!({ "catalog": catalog })))
}

// ── utilities ──────────────────────────────────────────────────────────────────

fn find_xazz_exe() -> Result<PathBuf, String> {
    // 1. Pin the path via env var (deployment hardening)
    if let Ok(pinned) = std::env::var("XAZZ_EXEC_PATH")
        && !pinned.trim().is_empty()
    {
        return Ok(PathBuf::from(pinned));
    }

    // platform-specific executable name
    let names: &[&str] = if cfg!(windows) {
        &["xazz.exe"]
    } else {
        &["xazz", "xazz.exe"]
    };

    // 2. Same directory as the current executable
    if let Ok(current_exe) = std::env::current_exe() {
        let dir = current_exe.parent().unwrap_or(&current_exe);
        for name in names {
            let sibling = dir.join(name);
            if sibling.exists() {
                return Ok(sibling);
            }
        }
    }
    // 3. target/release (CWD-based, project-local) — allowed only when CWD looks like
    //    the Xazz repo root (a directory with Cargo.toml). Prevents disguising an
    //    executable via relative-path shadowing from an arbitrary CWD.
    if std::path::Path::new("Cargo.toml").is_file() {
        for name in names {
            let candidate = PathBuf::from("target/release").join(name);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }
    // No PATH fallback is performed (PATH-shadowing prevention, fail-closed)
    Err(
        "xazz 실행 파일을 찾을 수 없습니다 (PATH 폴백은 보안상 비활성화됨). \
         XAZZ_EXEC_PATH 로 절대 경로를 지정하거나 xazz 를 xazz-server 와 같은 디렉터리에 배치하세요."
            .to_string(),
    )
}

fn internal_err(msg: String) -> (StatusCode, Json<ExecuteResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ExecuteResponse {
            success: false,
            rows: json!([]),
            schema: json!([]),
            logs: vec![],
            stdout: String::new(),
            training: None,
            dp: None,
            diagnostics: None,
            policy: None,
            error: Some(msg),
            run_id: None,
        }),
    )
}

// ── Integration tests — execution gate (issue #2) ─────────────────────────────
//
// What we aim to prove here is not "violating code is rejected" but
// **"for violating code, the runner is never invoked at all"**.
// The former is only an analysis result; the latter is the actual security property.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_upload_is_rejected() {
        assert_eq!(validate_upload_size(MAX_UPLOAD_BYTES), Ok(()));
        assert_eq!(
            validate_upload_size(MAX_UPLOAD_BYTES + 1),
            Err(StatusCode::PAYLOAD_TOO_LARGE)
        );
    }

    /// Test AppState — a permit count large enough that the execution semaphore does not
    /// impose test concurrency limits.
    fn test_state() -> AppState {
        // Each call gets its own SQLite file. Sharing one file across the
        // parallel test threads made concurrent writers fail with
        // "database is locked" even with a busy timeout (deadlock on the
        // shared→reserved lock upgrade).
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp_db = std::env::temp_dir().join(format!(
            "xazz_server_test_{}_{}.db",
            std::process::id(),
            seq
        ));
        AppState {
            exec_permits: Arc::new(Semaphore::new(64)),
            store: Arc::new(store::Store::open_at(&tmp_db)),
            tenant_locks: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }

    const UNSAFE_CODE: &str =
        "type Patient = { patient_id: string, name: string, age_band: string };
v out = load(\"data/p.csv\") :: Patient |> select([name, patient_id, age_band]);";

    /// Violating code is rejected with 422 and the report is in the body.
    #[tokio::test]
    async fn violating_code_is_rejected_with_422() {
        let result = handle_execute(
            Extension(String::new()),
            State(test_state()),
            Json(ExecuteRequest {
                code: UNSAFE_CODE.to_string(),
            }),
        )
        .await;

        let (status, body) = result.err().expect("violating code was not rejected");
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(!body.0.success);
        let policy = body.0.policy.as_ref().expect("no policy report");
        assert_eq!(policy["safe_to_execute"], serde_json::Value::Bool(false));
        assert!(
            policy["violations"]
                .as_array()
                .map(|v| !v.is_empty())
                .unwrap_or(false),
            "violation list is empty: {:?}",
            policy
        );
    }

    /// The xazz runner is never spawned even once for a rejected request.
    #[tokio::test]
    async fn rejected_request_never_invokes_runner() {
        let before = guardrail::runner_invocations();

        let _ = handle_execute(
            Extension(String::new()),
            State(test_state()),
            Json(ExecuteRequest {
                code: UNSAFE_CODE.to_string(),
            }),
        )
        .await;

        assert_eq!(
            guardrail::runner_invocations(),
            before,
            "runner was invoked for a blocked request"
        );
    }

    /// Even code that does not parse is rejected fail-closed without invoking the runner.
    #[tokio::test]
    async fn unparseable_code_is_rejected_without_running() {
        let before = guardrail::runner_invocations();

        let result = handle_execute(
            Extension(String::new()),
            State(test_state()),
            Json(ExecuteRequest {
                code: "v x = |> |> ???".to_string(),
            }),
        )
        .await;

        let (status, _) = result.err().expect("unparseable code was not rejected");
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(guardrail::runner_invocations(), before);
    }

    /// A hardcoded secret key is rejected.
    #[tokio::test]
    async fn hardcoded_secret_is_rejected() {
        let code = "// AKIAIOSFODNN7EXAMPLE\n\
                    type P = { age_band: string };\n\
                    v x = load(\"d.csv\") :: P |> select([age_band]);";
        let result = handle_execute(
            Extension(String::new()),
            State(test_state()),
            Json(ExecuteRequest {
                code: code.to_string(),
            }),
        )
        .await;
        let (status, body) = result.err().expect("secret key passed the gate");
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        // The report must not contain the original key.
        let serialized = serde_json::to_string(&body.0.policy).unwrap_or_default();
        assert!(
            !serialized.contains("AKIAIOSFODNN7EXAMPLE"),
            "original secret key leaked in the report"
        );
    }

    /// /security/policy/check returns 200 even with violations; the verdict is in the body.
    #[tokio::test]
    async fn policy_check_returns_report_without_executing() {
        let before = guardrail::runner_invocations();

        let response = handle_policy_check(
            Extension(String::new()),
            State(test_state()),
            Json(guardrail::CodeRequest {
                code: UNSAFE_CODE.to_string(),
            }),
        )
        .await
        .expect("policy check failed");

        assert!(!response.0.safe_to_execute);
        assert!(!response.0.policy.violations.is_empty());
        assert_eq!(guardrail::runner_invocations(), before);
    }

    /// /security/remediate returns verified safe code and the report together.
    #[tokio::test]
    async fn remediate_returns_verified_safe_code() {
        let response = handle_remediate(
            Extension(String::new()),
            State(test_state()),
            Json(guardrail::CodeRequest {
                code: UNSAFE_CODE.to_string(),
            }),
        )
        .await
        .expect("remediation failed");

        assert!(
            !response.0.safe_to_execute,
            "the original must be a violation"
        );
        let rem = &response.0.remediation;
        // This code has one remaining column (age_band), so it is fixable.
        assert!(
            rem.verified,
            "remediated code was not verified: {}",
            rem.report_after.render()
        );
        // The remediated code must actually pass the policy — via re-verification, not assertion.
        let policy = xazz_compiler::Policy::builtin();
        let recheck = xazz_compiler::check_policy(&rem.code, &policy);
        assert!(
            recheck.safe_to_execute,
            "remediated code still violates: {}",
            recheck.render()
        );
    }

    /// When the execution semaphore is fully exhausted, /execute rejects with 429 (no queue).
    #[test]
    fn execute_rejects_when_semaphore_exhausted() {
        let state = AppState {
            exec_permits: Arc::new(Semaphore::new(0)),
            store: Arc::new(store::Store::new()),
            tenant_locks: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(handle_execute(
            Extension(String::new()),
            State(state),
            Json(ExecuteRequest {
                code: "type P = { a: string }; v x = load(\"data/a.csv\") :: P;".to_string(),
            }),
        ));
        let (status, body) = result.err().expect("execution allowed without a permit");
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert!(
            body.0.error.as_deref().unwrap_or("").contains("capacity"),
            "no error message: {:?}",
            body.0.error
        );
    }

    /// A tenant whose DP reservation is held by another instance is rejected with
    /// 429 before the runner is invoked (issue C2, multi-instance).
    #[tokio::test]
    async fn execute_rejects_when_tenant_reservation_is_held() {
        let state = test_state();
        let tenant = format!("dp-res-{}", std::process::id());
        // Model another server instance holding this tenant's reservation.
        let held = state
            .store
            .reserve_dp_budget(&tenant, 10.0, 1e-4, 0, 3600)
            .expect("reserve")
            .expect("granted");

        let before = guardrail::runner_invocations();
        let result = handle_execute(
            Extension(tenant.clone()),
            State(state.clone()),
            Json(ExecuteRequest {
                code: "type P = { a: string }; v x = load(\"data/a.csv\") :: P;".to_string(),
            }),
        )
        .await;

        let (status, body) = result
            .err()
            .expect("execution proceeded despite a held reservation");
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert!(
            body.0.error.as_deref().unwrap_or("").contains("in flight"),
            "no error message: {:?}",
            body.0.error
        );
        assert_eq!(
            guardrail::runner_invocations(),
            before,
            "runner must not run while the reservation is held"
        );

        // Releasing the held reservation unblocks the tenant.
        state
            .store
            .settle_dp_reservation(&tenant, &held.id, 0.0, 0.0, 0)
            .expect("release");
        assert!(
            state
                .store
                .reserve_dp_budget(&tenant, 10.0, 1e-4, 0, 3600)
                .expect("reserve after release")
                .is_some()
        );
    }

    /// Same-tenant executions share one lock; different tenants do not (issue C2).
    #[tokio::test]
    async fn tenant_execution_locks_are_per_tenant() {
        let state = test_state();
        let a1 = state.tenant_lock("a");
        let a2 = state.tenant_lock("a");
        let b = state.tenant_lock("b");
        assert!(Arc::ptr_eq(&a1, &a2), "same tenant must share one lock");
        assert!(
            !Arc::ptr_eq(&a1, &b),
            "different tenants must not share a lock"
        );

        // Holding the tenant lock serializes the same tenant but not others.
        let guard = a1.lock().await;
        assert!(
            a2.try_lock().is_err(),
            "same tenant's executions must be serialized"
        );
        assert!(
            b.try_lock().is_ok(),
            "a different tenant must not be blocked"
        );
        drop(guard);
    }

    /// Idle tenants are pruned from the lock map so it cannot grow unbounded.
    #[tokio::test]
    async fn tenant_lock_map_prunes_idle_tenants() {
        let state = test_state();
        {
            let _a = state.tenant_lock("idle-a");
            let _b = state.tenant_lock("idle-b");
            assert_eq!(state.tenant_locks.lock().unwrap().len(), 2);
        }

        // Both locks are dropped; the next new tenant prunes them before inserting.
        let _c = state.tenant_lock("active-c");
        let map = state.tenant_locks.lock().unwrap();
        assert_eq!(map.len(), 1, "idle tenants must be pruned");
        assert!(map.contains_key("active-c"));
    }

    /// Safe code passes the gate (false-positive regression prevention).
    #[test]
    fn safe_code_passes_the_gate() {
        let safe = "type AQ = { station: string, pm10: Option<float> };
v x = load(\"examples/data/seoul_air_2024.csv\") :: AQ
    |> groupBy(\"station\")
    |> mean(\"pm10\");";
        assert!(matches!(
            guardrail::gate(safe),
            guardrail::Decision::Allow { .. }
        ));
    }

    // ── Inference output gate (issue #71, F2) ─────────────────────────────────

    /// A response leaking an API key is flagged and the call is audited.
    #[tokio::test]
    async fn inference_check_blocks_leaked_secret() {
        let result = handle_inference_check(Json(InferenceCheckRequest {
            code: "v x = load(\"d.csv\") :: S;".to_string(),
            prompt: "summarize the incident".to_string(),
            response: "The key AKIAIOSFODNN7EXAMPLE was exposed. Please rotate it.".to_string(),
            model_fingerprint: None,
        }))
        .await;

        let body = result.expect("inference check should succeed");
        assert!(!body.safe_to_emit);
        assert!(
            body.findings.iter().any(|f| f.kind == "api_key"),
            "API 키 미탐지: {:?}",
            body.findings
        );
        assert!(
            body.findings.iter().all(|f| !f.redacted.contains("AKIA")),
            "원본 값 노출: {:?}",
            body.findings
        );
        assert!(body.chain_valid, "감사 체인이 깨졌습니다");
    }

    /// A clean response is safe to emit, has no findings, and is audited as such.
    #[tokio::test]
    async fn inference_check_passes_clean_response() {
        let result = handle_inference_check(Json(InferenceCheckRequest {
            code: "v x = load(\"d.csv\") :: S;".to_string(),
            prompt: "summarize the quarterly report".to_string(),
            response: "Revenue grew 12%. Good quarter.".to_string(),
            model_fingerprint: None,
        }))
        .await;

        let body = result.expect("inference check should succeed");
        assert!(body.safe_to_emit);
        assert!(body.findings.is_empty());
        assert!(body.chain_valid);
        // The prompt/response texts are never stored — only their hashes.
        assert!(!body.response_hash.contains("Revenue"));
        assert!(body.response_hash.len() == 64);
    }

    /// A supplied model fingerprint is echoed and recorded in the audit chain
    /// alongside the code/prompt/response evidence (F5 cohort, F4 #73).
    #[tokio::test]
    async fn inference_check_records_model_fingerprint() {
        let fp = audit_log::hash_code("Qwen/Qwen2.5-1.5B-Instruct@rev1");
        let result = handle_inference_check(Json(InferenceCheckRequest {
            code: "v x = load(\"d.csv\") :: S;".to_string(),
            prompt: "summarize the quarterly report".to_string(),
            response: "Revenue grew 12%. Good quarter.".to_string(),
            model_fingerprint: Some(fp.clone()),
        }))
        .await;

        let body = result.expect("inference check should succeed");
        assert_eq!(body.model_fingerprint.as_deref(), Some(fp.as_str()));
        assert!(body.chain_valid);

        // The evidence record binds the fingerprint.
        let recorded =
            audit_log::lookup_by_hash(&audit_log::hash_code("v x = load(\"d.csv\") :: S;"))
                .expect("audit lookup");
        assert!(
            recorded
                .iter()
                .any(|r| r.model_fingerprint.as_deref() == Some(fp.as_str())),
            "모델 지문이 감사 체인에 기록되어야 함"
        );
    }

    // ── Per-tenant DP budget isolation (issue C2) ─────────────────────────────

    #[test]
    fn resolve_dp_envelope_defaults_and_overrides() {
        assert_eq!(resolve_dp_envelope(None, None), (10.0, 1e-4));
        assert_eq!(resolve_dp_envelope(Some("2.5"), Some("0.01")), (2.5, 0.01));
        // Invalid values fall back to the defaults.
        assert_eq!(resolve_dp_envelope(Some("0"), Some("7")), (10.0, 1e-4));
        assert_eq!(resolve_dp_envelope(Some("-1"), Some("-1")), (10.0, 1e-4));
        assert_eq!(resolve_dp_envelope(Some("abc"), Some("xyz")), (10.0, 1e-4));
    }

    #[test]
    fn resolve_dp_window_parses_and_defaults_to_disabled() {
        assert_eq!(resolve_dp_window(None), 0);
        assert_eq!(resolve_dp_window(Some("3600")), 3600);
        assert_eq!(resolve_dp_window(Some("0")), 0);
        assert_eq!(resolve_dp_window(Some("abc")), 0);
        assert_eq!(resolve_dp_window(Some("-5")), 0);
    }

    #[test]
    fn resolve_dp_reservation_ttl_parses_and_defaults() {
        assert_eq!(
            resolve_dp_reservation_ttl(None),
            DEFAULT_DP_RESERVATION_TTL_SECS
        );
        assert_eq!(resolve_dp_reservation_ttl(Some("120")), 120);
        // 0/invalid would make a reservation instantly reclaimable → default.
        assert_eq!(
            resolve_dp_reservation_ttl(Some("0")),
            DEFAULT_DP_RESERVATION_TTL_SECS
        );
        assert_eq!(
            resolve_dp_reservation_ttl(Some("abc")),
            DEFAULT_DP_RESERVATION_TTL_SECS
        );
    }

    #[test]
    fn dp_marker_spend_is_extracted() {
        assert!(dp_spend_from_marker(&None::<Value>).is_none());
        assert!(dp_spend_from_marker(&Some(json!({}))).is_none());
        let marker = Some(json!({ "budget_spent": 1.25, "budget_spent_delta": 2e-5 }));
        assert_eq!(dp_spend_from_marker(&marker), Some((1.25, 2e-5)));
        // A missing delta field is treated as 0 (Laplace is pure ε-DP).
        let marker = Some(json!({ "budget_spent": 3.0 }));
        assert_eq!(dp_spend_from_marker(&marker), Some((3.0, 0.0)));
    }

    #[tokio::test]
    async fn dp_budget_endpoint_is_tenant_scoped() {
        let state = test_state();
        let tenant = format!("dp-endpoint-{}", std::process::id());
        state
            .store
            .add_dp_spend(&tenant, 1.0, 5e-5, 0)
            .expect("seed spend");

        let body = handle_dp_budget(State(state.clone()), Extension(tenant.clone()))
            .await
            .expect("budget endpoint")
            .0;
        assert_eq!(body["tenant"], json!(tenant));
        assert!((body["spent_epsilon"].as_f64().unwrap() - 1.0).abs() < 1e-12);
        assert!((body["spent_delta"].as_f64().unwrap() - 5e-5).abs() < 1e-15);
        assert!((body["remaining_epsilon"].as_f64().unwrap() - 9.0).abs() < 1e-12);
        // No window configured in tests → cumulative, never auto-resets.
        assert_eq!(body["window_secs"], json!(0));
        assert_eq!(body["resets_at"], json!(0));

        // Another tenant is unaffected by this tenant's spend.
        let other = handle_dp_budget(State(state), Extension("dp-endpoint-other".into()))
            .await
            .expect("budget endpoint")
            .0;
        assert_eq!(other["spent_epsilon"], json!(0.0));
        assert_eq!(other["remaining_epsilon"], json!(10.0));
    }

    /// POST /dp/budget/reset clears only the authenticated tenant's ledger.
    #[tokio::test]
    async fn dp_budget_reset_is_tenant_scoped() {
        let state = test_state();
        let tenant = format!("dp-reset-{}", std::process::id());
        let other = format!("dp-reset-other-{}", std::process::id());
        state
            .store
            .add_dp_spend(&tenant, 2.0, 1e-5, 0)
            .expect("seed tenant");
        state
            .store
            .add_dp_spend(&other, 1.0, 0.0, 0)
            .expect("seed other");

        let body = handle_dp_budget_reset(State(state.clone()), Extension(tenant.clone()), None)
            .await
            .expect("reset endpoint")
            .0;
        assert_eq!(body["tenant"], json!(tenant));
        assert_eq!(body["spent_epsilon"], json!(0.0));
        assert_eq!(body["spent_delta"], json!(0.0));
        assert_eq!(body["reset_by"], json!(tenant));
        assert!((body["remaining_epsilon"].as_f64().unwrap() - 10.0).abs() < 1e-12);
        // The reset re-anchors the window.
        assert!(body["window_started_at"].as_i64().unwrap() > 0);

        // The other tenant keeps its spend.
        let other_body = handle_dp_budget(State(state), Extension(other))
            .await
            .expect("budget endpoint")
            .0;
        assert!((other_body["spent_epsilon"].as_f64().unwrap() - 1.0).abs() < 1e-12);
    }

    /// A reset appends to an append-only history with the acting identity; an
    /// admin-delegated reset is attributed to the actor, not the tenant (#124).
    #[tokio::test]
    async fn dp_budget_reset_records_actor_and_history() {
        let state = test_state();
        let tenant = format!("dp-audit-{}", std::process::id());
        state
            .store
            .add_dp_spend(&tenant, 4.0, 2e-5, 0)
            .expect("seed spend");

        // Self-service reset is attributed to the tenant.
        let self_body =
            handle_dp_budget_reset(State(state.clone()), Extension(tenant.clone()), None)
                .await
                .expect("self reset")
                .0;
        assert_eq!(self_body["reset_by"], json!(tenant));
        assert!((self_body["spent_epsilon_before"].as_f64().unwrap() - 4.0).abs() < 1e-12);

        // A delegated admin reset is attributed to the actor.
        state
            .store
            .add_dp_spend(&tenant, 1.0, 0.0, 0)
            .expect("reseed spend");
        let admin_body = handle_dp_budget_reset(
            State(state.clone()),
            Extension(tenant.clone()),
            Some(Extension(Actor("root".to_string()))),
        )
        .await
        .expect("admin reset")
        .0;
        assert_eq!(admin_body["reset_by"], json!("root"));
        assert!((admin_body["spent_epsilon_before"].as_f64().unwrap() - 1.0).abs() < 1e-12);

        // History is newest-first and tenant-scoped.
        let history = handle_dp_reset_history(State(state.clone()), Extension(tenant.clone()))
            .await
            .expect("history")
            .0;
        assert_eq!(history["tenant"], json!(tenant));
        let resets = history["resets"].as_array().unwrap();
        assert_eq!(resets.len(), 2);
        assert_eq!(resets[0]["actor"], json!("root"));
        assert_eq!(resets[1]["actor"], json!(tenant));

        let other = handle_dp_reset_history(State(state), Extension("dp-audit-other".into()))
            .await
            .expect("other history")
            .0;
        assert_eq!(other["resets"].as_array().unwrap().len(), 0);
    }

    /// An admin reset without a target tenant is rejected (issue #124).
    #[tokio::test]
    async fn dp_budget_reset_admin_requires_target_tenant() {
        let state = test_state();
        let err = handle_dp_budget_reset(
            State(state),
            Extension(String::new()),
            Some(Extension(Actor("root".to_string()))),
        )
        .await
        .expect_err("admin reset without a target was accepted");
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
    }

    /// A tenant can override its own DP window; the global default is the fallback.
    #[tokio::test]
    async fn dp_window_endpoint_sets_and_clears_tenant_override() {
        let state = test_state();
        let tenant = format!("dp-window-{}", std::process::id());
        let other = format!("dp-window-other-{}", std::process::id());

        // Seed a budget row so setting the window exercises the re-anchor path.
        state
            .store
            .add_dp_spend(&tenant, 1.0, 0.0, 0)
            .expect("seed spend");

        // No override: the effective window is the global default.
        let window = effective_dp_window(&state.store, &tenant).expect("resolve");
        assert_eq!(window.source, "global");
        assert_eq!(window.secs, tenant_dp_window_secs());

        // PUT stores a tenant-scoped override.
        let body = handle_dp_window_set(
            State(state.clone()),
            Extension(tenant.clone()),
            Json(DpWindowRequest { window_secs: 7200 }),
        )
        .await
        .expect("set window")
        .0;
        assert_eq!(body["window_secs"], json!(7200));
        assert_eq!(body["window_source"], json!("tenant"));
        let anchor = body["window_started_at"].as_i64().unwrap();
        assert!(anchor > 0, "set re-anchors the existing budget row");
        assert_eq!(body["resets_at"].as_i64().unwrap(), anchor + 7200);
        // Re-anchoring does not wipe the accrued spend.
        assert!((body["spent_epsilon"].as_f64().unwrap() - 1.0).abs() < 1e-12);

        // Another tenant still resolves to the global default.
        let other_window = effective_dp_window(&state.store, &other).expect("resolve other");
        assert_eq!(other_window.source, "global");

        // GET reflects the tenant override.
        let got = handle_dp_budget(State(state.clone()), Extension(tenant.clone()))
            .await
            .expect("budget")
            .0;
        assert_eq!(got["window_secs"], json!(7200));
        assert_eq!(got["window_source"], json!("tenant"));

        // DELETE clears it and restores the global default.
        let cleared = handle_dp_window_clear(State(state.clone()), Extension(tenant.clone()))
            .await
            .expect("clear window")
            .0;
        assert_eq!(cleared["window_source"], json!("global"));
        assert_eq!(cleared["window_secs"], json!(tenant_dp_window_secs()));
        assert!(state.store.get_dp_window(&tenant).expect("read").is_none());
    }

    /// A window above the cap is rejected; the cap itself is accepted (issue #125).
    #[tokio::test]
    async fn dp_window_rejects_above_cap() {
        let state = test_state();
        let tenant = format!("dp-window-cap-{}", std::process::id());
        // Seed a budget row so the accepted override also re-anchors a window.
        state
            .store
            .add_dp_spend(&tenant, 1.0, 0.0, 0)
            .expect("seed spend");

        let err = handle_dp_window_set(
            State(state.clone()),
            Extension(tenant.clone()),
            Json(DpWindowRequest {
                window_secs: MAX_DP_WINDOW_SECS + 1,
            }),
        )
        .await
        .expect_err("window above cap was accepted");
        assert_eq!(err.0, StatusCode::BAD_REQUEST);

        let body = handle_dp_window_set(
            State(state.clone()),
            Extension(tenant.clone()),
            Json(DpWindowRequest {
                window_secs: MAX_DP_WINDOW_SECS,
            }),
        )
        .await
        .expect("window at cap")
        .0;
        assert_eq!(body["window_secs"], json!(MAX_DP_WINDOW_SECS));
        assert!(body["resets_at"].as_i64().unwrap() > 0);
    }

    /// Env/stored windows above the cap are clamped so `resets_at` cannot overflow.
    #[test]
    fn resolve_dp_window_clamps_above_cap() {
        let at_cap = MAX_DP_WINDOW_SECS.to_string();
        assert_eq!(resolve_dp_window(Some(&at_cap)), MAX_DP_WINDOW_SECS);
        assert_eq!(resolve_dp_window(Some("999999999")), MAX_DP_WINDOW_SECS);
        assert_eq!(resolve_dp_window(Some("0")), 0);
        assert_eq!(resolve_dp_window(Some("not-a-number")), 0);
    }

    /// GET /dp/budget reflects an in-flight reservation: `remaining_*` is reduced
    /// and `in_flight` stays set until the reservation is settled (issue #123).
    #[tokio::test]
    async fn dp_budget_reflects_in_flight_reservation() {
        let state = test_state();
        let tenant = format!("dp-inflight-{}", std::process::id());
        state
            .store
            .add_dp_spend(&tenant, 1.0, 0.0, 0)
            .expect("seed spend");

        // Baseline: no reservation, remaining is total minus spend.
        let before = handle_dp_budget(State(state.clone()), Extension(tenant.clone()))
            .await
            .expect("budget")
            .0;
        assert_eq!(before["in_flight"], json!(false));
        assert!((before["remaining_epsilon"].as_f64().unwrap() - 9.0).abs() < 1e-12);

        // A run claims the tenant's remaining envelope.
        let held = state
            .store
            .reserve_dp_budget(&tenant, 10.0, 1e-4, 0, 3600)
            .expect("reserve")
            .expect("granted");
        let during = handle_dp_budget(State(state.clone()), Extension(tenant.clone()))
            .await
            .expect("budget")
            .0;
        assert_eq!(during["in_flight"], json!(true));
        assert!((during["reserved_epsilon"].as_f64().unwrap() - 9.0).abs() < 1e-12);
        assert_eq!(during["remaining_epsilon"], json!(0.0));

        // Settling releases it; the billed spend is then reflected.
        state
            .store
            .settle_dp_reservation(&tenant, &held.id, 2.0, 0.0, 0)
            .expect("settle");
        let after = handle_dp_budget(State(state), Extension(tenant))
            .await
            .expect("budget")
            .0;
        assert_eq!(after["in_flight"], json!(false));
        assert!((after["spent_epsilon"].as_f64().unwrap() - 3.0).abs() < 1e-12);
        assert!((after["remaining_epsilon"].as_f64().unwrap() - 7.0).abs() < 1e-12);
    }

    /// A tenant can override its own policy-history retention window; the global
    /// default is the fallback (issue C2).
    #[tokio::test]
    async fn policy_history_ttl_endpoint_sets_and_clears_tenant_override() {
        let state = unique_state("hist_ttl_endpoint");
        let tenant = format!("hist-ttl-{}", std::process::id());
        let other = format!("hist-ttl-other-{}", std::process::id());

        // No override: the effective window is the global default.
        let before = effective_policy_history_ttl(&state.store, &tenant).expect("resolve");
        assert_eq!(before.source, "global");
        assert_eq!(before.secs, state.store.policy_history_ttl_default());

        // PUT stores a tenant-scoped override.
        let body = handle_policy_history_ttl_set(
            State(state.clone()),
            Extension(tenant.clone()),
            Json(PolicyHistoryTtlRequest { ttl_secs: 7200 }),
        )
        .await
        .expect("set ttl")
        .0;
        assert_eq!(body["tenant"], json!(tenant));
        assert_eq!(body["ttl_secs"], json!(7200));
        assert_eq!(body["ttl_source"], json!("tenant"));
        assert_eq!(
            state.store.get_policy_history_ttl(&tenant).expect("read"),
            Some(7200)
        );

        // Another tenant still resolves to the global default.
        let other_ttl = effective_policy_history_ttl(&state.store, &other).expect("resolve other");
        assert_eq!(other_ttl.source, "global");

        // DELETE clears it and restores the global default.
        let cleared =
            handle_policy_history_ttl_clear(State(state.clone()), Extension(tenant.clone()))
                .await
                .expect("clear ttl")
                .0;
        assert_eq!(cleared["ttl_source"], json!("global"));
        assert_eq!(
            cleared["ttl_secs"],
            json!(state.store.policy_history_ttl_default())
        );
        assert!(
            state
                .store
                .get_policy_history_ttl(&tenant)
                .expect("read")
                .is_none()
        );
    }

    /// `GET /security/policy/history/ttl` reports the effective window and its source
    /// so clients can observe the fallback without mutating state (issue C2).
    #[tokio::test]
    async fn policy_history_ttl_endpoint_reports_effective_window() {
        let state = unique_state("hist_ttl_get");
        let tenant = format!("hist-ttl-get-{}", std::process::id());

        // No override: vends the global default and its source.
        let global = handle_policy_history_ttl_get(State(state.clone()), Extension(tenant.clone()))
            .await
            .expect("get ttl")
            .0;
        assert_eq!(global["tenant"], json!(tenant));
        assert_eq!(global["ttl_source"], json!("global"));
        assert_eq!(
            global["ttl_secs"],
            json!(state.store.policy_history_ttl_default())
        );

        // With an override: reports the tenant value and source.
        state
            .store
            .set_policy_history_ttl(&tenant, 1800, &tenant)
            .expect("set");
        let tenant_view =
            handle_policy_history_ttl_get(State(state.clone()), Extension(tenant.clone()))
                .await
                .expect("get ttl")
                .0;
        assert_eq!(tenant_view["ttl_source"], json!("tenant"));
        assert_eq!(tenant_view["ttl_secs"], json!(1800));
    }

    /// `GET /security/policy/history/ttl/history` exposes the tenant's
    /// retention-override change audit, newest-first (issue C2).
    #[tokio::test]
    async fn policy_history_ttl_change_history_endpoint_lists_changes() {
        let state = unique_state("hist_ttl_hist_endpoint");
        let tenant = format!("hist-ttl-hist-{}", std::process::id());

        let empty = handle_policy_history_ttl_history(
            Extension(tenant.clone()),
            State(state.clone()),
            Query(PolicyHistoryQuery {
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect("history")
        .0;
        assert_eq!(empty["tenant"], json!(tenant));
        assert_eq!(empty["history"], json!([]));

        let _ = handle_policy_history_ttl_set(
            State(state.clone()),
            Extension(tenant.clone()),
            Json(PolicyHistoryTtlRequest { ttl_secs: 3600 }),
        )
        .await
        .expect("set ttl");

        let listed = handle_policy_history_ttl_history(
            Extension(tenant.clone()),
            State(state.clone()),
            Query(PolicyHistoryQuery {
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect("history")
        .0;
        let history = listed["history"].as_array().expect("history array");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0]["action"], json!("set"));
        assert_eq!(history[0]["old_ttl_secs"], Value::Null);
        assert_eq!(history[0]["new_ttl_secs"], json!(3600));
        assert_eq!(history[0]["changed_by"], json!(tenant));
    }

    /// `GET /security/policy/history` exposes the tenant's effective retention
    /// window alongside the page so clients can see how far back the history is
    /// retained without a second call (issue C2).
    #[tokio::test]
    async fn policy_history_reports_effective_ttl() {
        let state = unique_state("hist_ttl_view");
        let tenant = format!("hist-ttl-view-{}", std::process::id());

        // No override: global default + source.
        let global = handle_policy_history(
            Extension(tenant.clone()),
            State(state.clone()),
            Query(PolicyHistoryQuery {
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect("history")
        .0;
        assert_eq!(global["ttl_source"], json!("global"));
        assert_eq!(
            global["ttl_secs"],
            json!(state.store.policy_history_ttl_default())
        );

        // Tenant override is reflected in the same response.
        state
            .store
            .set_policy_history_ttl(&tenant, 900, &tenant)
            .expect("set");
        let overridden = handle_policy_history(
            Extension(tenant.clone()),
            State(state.clone()),
            Query(PolicyHistoryQuery {
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect("history")
        .0;
        assert_eq!(overridden["ttl_source"], json!("tenant"));
        assert_eq!(overridden["ttl_secs"], json!(900));
    }

    // ── Per-tenant policy packs (issue C2) ────────────────────────────────────

    /// Isolated AppState for policy tests (each tenant-policy write gets its own DB).
    fn unique_state(tag: &str) -> AppState {
        let tmp_db =
            std::env::temp_dir().join(format!("xazz_server_pol_{}_{}.db", std::process::id(), tag));
        AppState {
            exec_permits: Arc::new(Semaphore::new(64)),
            store: Arc::new(store::Store::open_at(&tmp_db)),
            tenant_locks: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// A stored pack is namespaced by tenant: only its owner reads/applies it.
    #[tokio::test]
    async fn tenant_policy_endpoints_are_namespaced() {
        let state = unique_state("ns");
        let code = "type P = { region: string };\n\
                    v x = load(\"d.csv\") :: P |> select([region]);";

        // tenant-a writes a pack that classifies `region` as a direct identifier.
        let mut pack = xazz_compiler::Policy::builtin();
        pack.id = "tenant-a-pack".to_string();
        pack.direct_identifiers.push("region".to_string());
        let set = handle_policy_set(
            Extension("tenant-a".to_string()),
            None,
            State(state.clone()),
            Json(serde_json::to_value(&pack).unwrap()),
        )
        .await
        .expect("set policy")
        .0;
        assert_eq!(set["origin"], json!("tenant:tenant-a"));
        assert_eq!(set["policy"]["id"], json!("tenant-a-pack"));

        // tenant-a reads its own pack.
        let info_a = handle_policy_info(Extension("tenant-a".to_string()), State(state.clone()))
            .await
            .expect("info a")
            .0;
        assert_eq!(info_a["origin"], json!("tenant:tenant-a"));
        assert_eq!(info_a["policy"]["id"], json!("tenant-a-pack"));

        // tenant-b is unaffected — it gets the builtin baseline.
        let info_b = handle_policy_info(Extension("tenant-b".to_string()), State(state.clone()))
            .await
            .expect("info b")
            .0;
        assert_eq!(info_b["policy"]["id"], json!("xazz-builtin-pii"));

        // The verdict differs by tenant for identical code.
        let check_a = handle_policy_check(
            Extension("tenant-a".to_string()),
            State(state.clone()),
            Json(guardrail::CodeRequest {
                code: code.to_string(),
            }),
        )
        .await
        .expect("policy check a")
        .0;
        assert!(!check_a.safe_to_execute);
        let check_b = handle_policy_check(
            Extension("tenant-b".to_string()),
            State(state.clone()),
            Json(guardrail::CodeRequest {
                code: code.to_string(),
            }),
        )
        .await
        .expect("policy check b")
        .0;
        assert!(check_b.safe_to_execute);

        // Deleting tenant-a's pack reverts it to the builtin baseline.
        let del = handle_policy_delete(
            Extension("tenant-a".to_string()),
            None,
            State(state.clone()),
        )
        .await
        .expect("delete")
        .0;
        assert_eq!(del["deleted"], json!(true));
        let info_a2 = handle_policy_info(Extension("tenant-a".to_string()), State(state))
            .await
            .expect("info a2")
            .0;
        assert_eq!(info_a2["policy"]["id"], json!("xazz-builtin-pii"));
    }

    /// An invalid pack is rejected at write time (never stored).
    #[tokio::test]
    async fn tenant_policy_set_rejects_invalid_pack() {
        let state = unique_state("bad");
        let result = handle_policy_set(
            Extension("t".to_string()),
            None,
            State(state),
            Json(json!({ "not": "a policy" })),
        )
        .await;
        let (status, body) = result.expect_err("invalid pack was accepted");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.0["error"].as_str().is_some());
    }

    /// An administrator may change another tenant's pack; the audit attributes the
    /// change to the actor while the namespace stays the target tenant (issue C2).
    #[tokio::test]
    async fn admin_delegated_policy_change_records_actor() {
        let state = unique_state("admin");

        let mut pack = xazz_compiler::Policy::builtin();
        pack.id = "admin-set".to_string();
        let set = handle_policy_set(
            Extension("tenant-a".to_string()),
            Some(Extension(Actor("root".to_string()))),
            State(state.clone()),
            Json(serde_json::to_value(&pack).unwrap()),
        )
        .await
        .expect("admin set")
        .0;
        assert_eq!(set["tenant"], json!("tenant-a"));
        assert_eq!(set["origin"], json!("tenant:tenant-a"));

        let _ = handle_policy_delete(
            Extension("tenant-a".to_string()),
            Some(Extension(Actor("root".to_string()))),
            State(state.clone()),
        )
        .await
        .expect("admin delete");

        let body = handle_policy_history(
            Extension("tenant-a".to_string()),
            State(state.clone()),
            Query(PolicyHistoryQuery {
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect("history")
        .0;
        let history = body["history"].as_array().expect("history array");
        assert_eq!(history.len(), 2, "{body}");
        assert_eq!(history[0]["action"], json!("delete"));
        assert_eq!(history[0]["changed_by"], json!("root"));
        assert_eq!(history[1]["action"], json!("set"));
        assert_eq!(history[1]["changed_by"], json!("root"));
    }

    /// An admin request must name the target tenant — it cannot land in the
    /// empty/global namespace (issue C2).
    #[tokio::test]
    async fn admin_policy_change_requires_target_tenant() {
        let state = unique_state("admin_target");
        let pack = xazz_compiler::Policy::builtin();

        let set = handle_policy_set(
            Extension(String::new()),
            Some(Extension(Actor("admin".to_string()))),
            State(state.clone()),
            Json(serde_json::to_value(&pack).unwrap()),
        )
        .await;
        let (status, _) = set.expect_err("admin set without target was accepted");
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let del = handle_policy_delete(
            Extension(String::new()),
            Some(Extension(Actor("admin".to_string()))),
            State(state),
        )
        .await;
        let (status, _) = del.expect_err("admin delete without target was accepted");
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    /// A blank `X-Xazz-Actor` falls back to the default admin identity (issue C2).
    #[test]
    fn actor_header_defaults_to_admin() {
        assert_eq!(resolve_actor(""), "admin");
        assert_eq!(resolve_actor("root"), "root");
    }

    /// Policy-pack changes are recorded append-only with who/when/previous (issue C2).
    #[tokio::test]
    async fn policy_history_records_who_when_and_previous_pack() {
        let state = unique_state("hist");

        let mut v1 = xazz_compiler::Policy::builtin();
        v1.id = "v1".to_string();
        let mut v2 = xazz_compiler::Policy::builtin();
        v2.id = "v2".to_string();

        let _ = handle_policy_set(
            Extension("tenant-a".to_string()),
            None,
            State(state.clone()),
            Json(serde_json::to_value(&v1).unwrap()),
        )
        .await
        .expect("set v1");
        let _ = handle_policy_set(
            Extension("tenant-a".to_string()),
            None,
            State(state.clone()),
            Json(serde_json::to_value(&v2).unwrap()),
        )
        .await
        .expect("set v2");
        let _ = handle_policy_delete(
            Extension("tenant-a".to_string()),
            None,
            State(state.clone()),
        )
        .await
        .expect("delete");

        let body = handle_policy_history(
            Extension("tenant-a".to_string()),
            State(state.clone()),
            Query(PolicyHistoryQuery {
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect("history")
        .0;
        let history = body["history"].as_array().expect("history array");
        assert_eq!(history.len(), 3, "{body}");
        assert_eq!(body["limit"], json!(POLICY_HISTORY_LIMIT));
        assert_eq!(body["offset"], json!(0));
        // Newest-first: delete, set(v2), set(v1).
        assert_eq!(history[0]["action"], json!("delete"));
        assert_eq!(history[0]["changed_by"], json!("tenant-a"));
        assert_eq!(history[0]["old_policy_json"]["id"], json!("v2"));
        assert!(history[0]["new_policy_json"].is_null());
        assert_eq!(history[1]["action"], json!("set"));
        assert_eq!(history[1]["old_policy_json"]["id"], json!("v1"));
        assert_eq!(history[1]["new_policy_json"]["id"], json!("v2"));
        assert_eq!(history[2]["old_policy_json"], Value::Null);
        assert!(history[0]["changed_at"].as_i64().unwrap() > 0);

        // A different tenant's history is empty (namespaced).
        let other = handle_policy_history(
            Extension("tenant-b".to_string()),
            State(state),
            Query(PolicyHistoryQuery {
                limit: None,
                offset: None,
            }),
        )
        .await
        .expect("history b")
        .0;
        assert_eq!(other["history"].as_array().unwrap().len(), 0);
    }

    /// `?limit=`/`?offset=` page the newest-first history (issue C2).
    #[tokio::test]
    async fn policy_history_supports_pagination() {
        let state = unique_state("hist_page");
        for i in 1..=3 {
            let mut pack = xazz_compiler::Policy::builtin();
            pack.id = format!("v{i}");
            let _ = handle_policy_set(
                Extension("tenant-a".to_string()),
                None,
                State(state.clone()),
                Json(serde_json::to_value(&pack).unwrap()),
            )
            .await
            .expect("set");
        }

        let page = |limit: usize, offset: usize| {
            let state = state.clone();
            async move {
                handle_policy_history(
                    Extension("tenant-a".to_string()),
                    State(state),
                    Query(PolicyHistoryQuery {
                        limit: Some(limit),
                        offset: Some(offset),
                    }),
                )
                .await
                .expect("history")
                .0
            }
        };

        let first = page(2, 0).await;
        let first_ids: Vec<&str> = first["history"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h["new_policy_json"]["id"].as_str().unwrap())
            .collect();
        assert_eq!(first_ids, vec!["v3", "v2"]);
        assert_eq!(first["limit"], json!(2));
        assert_eq!(first["offset"], json!(0));

        let second = page(2, 2).await;
        let second_ids: Vec<&str> = second["history"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h["new_policy_json"]["id"].as_str().unwrap())
            .collect();
        assert_eq!(second_ids, vec!["v1"]);
    }

    /// `?limit=` is clamped to a sane page range (issue C2).
    #[test]
    fn policy_history_limit_is_clamped() {
        let clamp = |limit: Option<usize>| {
            PolicyHistoryQuery {
                limit,
                offset: None,
            }
            .limit()
        };
        assert_eq!(clamp(None), POLICY_HISTORY_LIMIT);
        assert_eq!(clamp(Some(0)), 1);
        assert_eq!(clamp(Some(25)), 25);
        assert_eq!(clamp(Some(usize::MAX)), POLICY_HISTORY_MAX_LIMIT);
    }

    /// /execute applies the authenticated tenant's pack and never invokes the runner on block.
    #[tokio::test]
    async fn execute_applies_tenant_policy() {
        let state = unique_state("exec");
        let mut pack = xazz_compiler::Policy::builtin();
        pack.direct_identifiers.push("region".to_string());
        let _ = handle_policy_set(
            Extension("tenant-a".to_string()),
            None,
            State(state.clone()),
            Json(serde_json::to_value(&pack).unwrap()),
        )
        .await
        .expect("set policy");

        let code = "type P = { region: string };\n\
                    v x = load(\"d.csv\") :: P |> select([region]);";
        let before = guardrail::runner_invocations();
        let result = handle_execute(
            Extension("tenant-a".to_string()),
            State(state),
            Json(ExecuteRequest {
                code: code.to_string(),
            }),
        )
        .await;
        let (status, _) = result.err().expect("tenant policy was not applied");
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(guardrail::runner_invocations(), before);
    }

    /// A client disconnect (axum dropping the caller's future) must not lose the
    /// run's bookkeeping: the detached blocking task still settles DP, appends the
    /// audit record, and persists the run (GHSA-wxqx-r7f6-qq3p).
    ///
    /// This drives `run_execution_job` directly rather than `handle_execute` so the
    /// global runner-invocation counter stays untouched for the gate tests.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn disconnected_execute_still_audits_and_records() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        // Fake runner: writes a start marker, then sleeps so the job is still in
        // flight when we disconnect.
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let script = std::env::temp_dir().join(format!(
            "xazz_fake_runner_{}_{}.sh",
            std::process::id(),
            nonce
        ));
        let started = std::env::temp_dir().join(format!(
            "xazz_fake_runner_{}_{}.started",
            std::process::id(),
            nonce
        ));
        {
            let mut f = std::fs::File::create(&script).expect("create fake runner");
            writeln!(f, "#!/bin/sh").unwrap();
            writeln!(f, "touch '{}'", started.display()).unwrap();
            writeln!(f, "sleep 0.3").unwrap();
            writeln!(f, "echo '[xazz:result] {{\"rows\":[1],\"schema\":[]}}'").unwrap();
            f.flush().unwrap();
        }
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake runner");

        let state = unique_state("disconnect");
        let tenant = String::new();
        let code = format!(
            "// disconnect-nonce {nonce}\n\
             type P = {{ a: string }}; v x = load(\"data/a.csv\") :: P;"
        );

        // A temp .xzz file and a real DP reservation, exactly as handle_execute sets up.
        let tmp = tempfile::Builder::new()
            .suffix(".xzz")
            .tempfile()
            .expect("temp file");
        {
            let mut f = tmp.as_file();
            f.write_all(code.as_bytes()).expect("write code");
            f.flush().ok();
        }
        let reservation = state
            .store
            .reserve_dp_budget(&tenant, 10.0, 1e-4, 0, 3600)
            .expect("reserve")
            .expect("granted");

        let before = state.store.list_runs(50, &tenant).expect("list runs").len();

        let task = tokio::spawn(run_execution_job(
            state.clone(),
            code.clone(),
            tenant.clone(),
            script.clone(),
            tmp,
            reservation,
            0,
            10.0,
            1e-4,
            None,
        ));

        // Wait until the runner has actually started, then drop the future — exactly
        // what axum does when the client disconnects. Waiting on the marker makes the
        // race deterministic: the job is provably suspended at the blocking `.await`.
        for _ in 0..50 {
            if started.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(started.exists(), "fake runner never started");
        task.abort();
        let _ = task.await;

        // The detached blocking task must finish its bookkeeping on its own.
        let mut recorded = before;
        for _ in 0..50 {
            recorded = state.store.list_runs(50, &tenant).expect("list runs").len();
            if recorded > before {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        let _ = std::fs::remove_file(&script);
        let _ = std::fs::remove_file(&started);

        assert!(
            recorded > before,
            "disconnected run was not persisted to history"
        );
        let audited =
            audit_log::lookup_by_hash(&audit_log::hash_code(&code)).expect("audit lookup");
        assert!(
            !audited.is_empty(),
            "disconnected run was not appended to the audit chain"
        );
    }
}
