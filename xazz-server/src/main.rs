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
    extract::{Extension, Multipart, Path, State},
    http::{HeaderValue, StatusCode, header::AUTHORIZATION},
    middleware::{self, Next},
    response::{IntoResponse, Json},
    routing::{get, post},
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

/// AppState shared across requests.
#[derive(Clone)]
struct AppState {
    exec_permits: Arc<Semaphore>,
    /// Persistent run-history store (issue C1)
    store: Arc<store::Store>,
    /// Per-tenant execution locks (issue C2) — serialize a tenant's
    /// precheck → run → accrue so its DP budget check is atomic across runs.
    tenant_locks:
        Arc<std::sync::Mutex<std::collections::HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
}

impl AppState {
    /// Returns the (shared) execution lock for a tenant, creating it on first use.
    fn tenant_lock(&self, tenant: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut map = self.tenant_locks.lock().expect("tenant lock map poisoned");
        map.entry(tenant.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
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
const DEFAULT_TENANT_DP_BUDGET: f64 = 10.0;
const DEFAULT_TENANT_DP_DELTA_BUDGET: f64 = 1e-4;

/// Floor for the remaining budget handed to the runner. The runner's env parser
/// ignores non-positive totals, so passing 0 would silently reset to its own
/// default; this keeps "no budget left" enforceable while non-DP runs proceed.
const MIN_REMAINING_BUDGET: f64 = 1e-12;

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
fn resolve_dp_window(raw: Option<&str>) -> u64 {
    raw.and_then(|v| v.parse::<u64>().ok()).unwrap_or(0)
}

/// Reads the per-tenant DP budget window from the environment (0 = disabled).
fn tenant_dp_window_secs() -> u64 {
    let raw = std::env::var(TENANT_DP_WINDOW_ENV).ok();
    resolve_dp_window(raw.as_deref())
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

    let app = Router::new()
        .route("/execute", post(handle_execute))
        .route("/schema", post(handle_schema))
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
        .route("/security/remediate", post(handle_remediate))
        .route("/security/inference/check", post(handle_inference_check))
        .route("/runs", get(handle_runs_list))
        .route("/runs/{id}", get(handle_run_by_id))
        .route("/dp/budget", get(handle_dp_budget))
        .route("/dp/budget/reset", post(handle_dp_budget_reset))
        .route("/catalog", post(handle_catalog))
        .with_state(AppState {
            exec_permits: Arc::new(Semaphore::new(MAX_CONCURRENT_EXECUTIONS)),
            store: Arc::new(store::Store::new()),
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

/// When `XAZZ_SERVER_TOKEN` is set, every request requires `Authorization: Bearer <token>`.
/// When `XAZZ_TENANT_TOKENS` is set (format `tenant1=token1,tenant2=token2`), a request
/// must present `X-Xazz-Tenant: <tenant>` + `Authorization: Bearer <token>` for that tenant.
/// When neither is set, all requests pass (default local-only behavior).
async fn optional_bearer_auth(
    mut req: axum::extract::Request,
    next: Next,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let single_token = std::env::var("XAZZ_SERVER_TOKEN").unwrap_or_default();
    let tenant_map = parse_tenant_tokens(&std::env::var("XAZZ_TENANT_TOKENS").unwrap_or_default());

    // Both auth modes unset → allow all (local loopback tool).
    if single_token.is_empty() && tenant_map.is_empty() {
        req.extensions_mut().insert(String::new());
        return Ok(next.run(req).await);
    }

    let auth = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|h: &HeaderValue| h.to_str().ok())
        .unwrap_or("")
        .to_string();

    // Multi-tenant mode: tenant header + per-tenant token.
    if !tenant_map.is_empty() {
        let tenant = req
            .headers()
            .get(TENANT_HEADER)
            .and_then(|h: &HeaderValue| h.to_str().ok())
            .unwrap_or("")
            .to_string();
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

    // 1. Save the DSL code to a temp .xzz file
    let tmp = tempfile::Builder::new()
        .suffix(".xzz")
        .tempfile()
        .map_err(|e| internal_err(format!("임시파일 생성 실패: {}", e)))?;

    let tmp_path = tmp.path().to_path_buf();
    {
        let mut f = tmp.as_file();
        f.write_all(payload.code.as_bytes())
            .map_err(|e| internal_err(format!("임시파일 쓰기 실패: {}", e)))?;
        f.flush().ok();
    }

    // 2. Locate the xazz.exe executable path
    let exe_path = find_xazz_exe().map_err(internal_err)?;

    // 2b. Per-tenant DP budget isolation (issue C2): a tenant may only consume up
    //     to its own cumulative envelope. The runner receives the *remaining*
    //     budget, so cross-run composition is enforced by the existing DP
    //     accounting (the runner refuses a `withDp` that would exceed it).
    let (total_eps, total_delta) = tenant_dp_envelope();
    let window_secs = tenant_dp_window_secs();
    let (spent_eps, spent_delta) = state
        .store
        .dp_spent(tenant_str(tenant.as_str()), window_secs)
        .map_err(|e| internal_err(format!("DP 원장 조회 실패: {e}")))?;
    let remaining_eps = (total_eps - spent_eps).max(MIN_REMAINING_BUDGET);
    let remaining_delta = (total_delta - spent_delta).max(MIN_REMAINING_BUDGET);

    // 3. Run xazz run <tmp.xzz>
    //    Only requests that pass the gate reach this point — tests verify with the counter.
    guardrail::note_runner_invocation();
    let output = tokio::task::spawn_blocking(move || {
        Command::new(&exe_path)
            .arg("run")
            .arg(&tmp_path)
            .env("XAZZ_DP_BUDGET", remaining_eps.to_string())
            .env("XAZZ_DP_DELTA_BUDGET", remaining_delta.to_string())
            .output()
    })
    .await
    .map_err(|e| internal_err(format!("spawn_blocking 실패: {}", e)))?
    .map_err(|e| internal_err(format!("xazz.exe 실행 실패: {}", e)))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    let success = output.status.success();

    // 4. Parse stdout: extract [xazz:result], [xazz:chart], [xazz:train], [xazz:dp] markers
    let (rows, schema, logs, training, dp, diagnostics) = parse_stdout_markers(&stdout, &stderr);

    // 4b. Bill this run's DP consumption to the tenant's ledger (issue C2).
    //     Only the marker's cumulative spend is billed; no withDp → nothing billed.
    if let Some((eps, delta)) = dp_spend_from_marker(&dp)
        && let Err(e) =
            state
                .store
                .add_dp_spend(tenant_str(tenant.as_str()), eps, delta, window_secs)
    {
        eprintln!("[xazz] ⚠️ DP 원장 갱신 실패: {e}");
    }

    // 5. Auto-audit the execution history (trust infrastructure — persist all operation history)
    //    Even on failure, return the execution, logging only the audit-record failure as a warning.
    match audit_log::append_with_outcome(
        &payload.code,
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
        &payload.code,
        if success { "success" } else { "failed" },
        rows_count,
        err_msg_opt.as_deref(),
        tenant_str(tenant.as_str()),
    );

    if success {
        Ok(Json(ExecuteResponse {
            success: true,
            rows,
            schema,
            logs,
            stdout,
            training,
            dp,
            diagnostics,
            policy: serde_json::to_value(&policy_report).ok(),
            error: None,
            run_id: Some(run_id),
        }))
    } else {
        Ok(Json(ExecuteResponse {
            success: false,
            rows: json!([]),
            schema: json!([]),
            logs,
            stdout,
            training,
            dp,
            diagnostics,
            policy: serde_json::to_value(&policy_report).ok(),
            error: err_msg_opt,
            run_id: Some(run_id),
        }))
    }
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

async fn handle_schema(
    mut multipart: Multipart,
) -> Result<Json<SchemaResponse>, (StatusCode, String)> {
    // Extract the file field from the multipart
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut original_name = "upload.csv".to_string();

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            format!("multipart 파싱 실패: {}", e),
        )
    })? {
        if field.name() == Some("file") {
            original_name = field.file_name().unwrap_or("upload.csv").to_string();
            let data = field
                .bytes()
                .await
                .map_err(|e| (StatusCode::BAD_REQUEST, format!("파일 읽기 실패: {}", e)))?;
            // Upload size upper bound — reject if exceeded (disk DoS prevention).
            if data.len() > MAX_UPLOAD_BYTES {
                return Err((
                    StatusCode::PAYLOAD_TOO_LARGE,
                    format!(
                        "파일이 너무 큽니다. 최대 {} MB 까지 허용됩니다.",
                        MAX_UPLOAD_BYTES / (1024 * 1024)
                    ),
                ));
            }
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
    let computed = format!("{:x}", hasher.finalize());
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

/// Stores (or replaces) the authenticated tenant's policy pack — issue C2.
///
/// `self-service`: the tenant writes only its own namespace. The body is validated
/// with the same parser used at load time, so an unparseable pack is rejected here
/// instead of becoming a later fail-closed denial.
async fn handle_policy_set(
    Extension(tenant): Extension<String>,
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let tenant = tenant_str(tenant.as_str());
    let text = payload.to_string();
    let policy = xazz_compiler::Policy::from_json_str(&text)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e.message }))))?;

    state.store.set_tenant_policy(tenant, &text).map_err(|e| {
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
async fn handle_policy_delete(
    Extension(tenant): Extension<String>,
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let tenant = tenant_str(tenant.as_str());
    let deleted = state.store.delete_tenant_policy(tenant).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e })),
        )
    })?;
    Ok(Json(json!({ "tenant": tenant, "deleted": deleted })))
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
    let window_secs = tenant_dp_window_secs();
    let (spent_eps, spent_delta) = state
        .store
        .dp_spent(tenant, window_secs)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(dp_budget_view(
        &state,
        tenant,
        total_eps,
        total_delta,
        window_secs,
        spent_eps,
        spent_delta,
    )?))
}

/// Resets the authenticated tenant's accumulated DP spend and window — issue C2.
///
/// Tenant-scoped (self-service): only the caller's ledger is cleared. Other tenants'
/// spends are untouched.
async fn handle_dp_budget_reset(
    State(state): State<AppState>,
    Extension(tenant): Extension<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let tenant = tenant_str(tenant.as_str());
    state
        .store
        .reset_dp_budget(tenant)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let (total_eps, total_delta) = tenant_dp_envelope();
    let window_secs = tenant_dp_window_secs();
    Ok(Json(dp_budget_view(
        &state,
        tenant,
        total_eps,
        total_delta,
        window_secs,
        0.0,
        0.0,
    )?))
}

/// Builds the `GET /dp/budget` response body from the tenant's ledger state.
fn dp_budget_view(
    state: &AppState,
    tenant: &str,
    total_eps: f64,
    total_delta: f64,
    window_secs: u64,
    spent_eps: f64,
    spent_delta: f64,
) -> Result<Value, (StatusCode, String)> {
    let anchor = state
        .store
        .dp_window_started_at(tenant)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?
        .unwrap_or(0);
    let resets_at = if window_secs > 0 && anchor > 0 {
        anchor + window_secs as i64
    } else {
        0
    };
    Ok(json!({
        "tenant": tenant,
        "spent_epsilon": spent_eps,
        "spent_delta": spent_delta,
        "total_epsilon": total_eps,
        "total_delta": total_delta,
        "remaining_epsilon": (total_eps - spent_eps).max(0.0),
        "remaining_delta": (total_delta - spent_delta).max(0.0),
        "window_secs": window_secs,
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

    /// Test AppState — a permit count large enough that the execution semaphore does not
    /// impose test concurrency limits.
    fn test_state() -> AppState {
        let tmp_db =
            std::env::temp_dir().join(format!("xazz_server_test_{}.db", std::process::id()));
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

        let body = handle_dp_budget(State(state), Extension(tenant.clone()))
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
        let other = handle_dp_budget(State(test_state()), Extension("dp-endpoint-other".into()))
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

        let body = handle_dp_budget_reset(State(state.clone()), Extension(tenant.clone()))
            .await
            .expect("reset endpoint")
            .0;
        assert_eq!(body["tenant"], json!(tenant));
        assert_eq!(body["spent_epsilon"], json!(0.0));
        assert_eq!(body["spent_delta"], json!(0.0));
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
        let del = handle_policy_delete(Extension("tenant-a".to_string()), State(state.clone()))
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
            State(state),
            Json(json!({ "not": "a policy" })),
        )
        .await;
        let (status, body) = result.expect_err("invalid pack was accepted");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.0["error"].as_str().is_some());
    }

    /// /execute applies the authenticated tenant's pack and never invokes the runner on block.
    #[tokio::test]
    async fn execute_applies_tenant_policy() {
        let state = unique_state("exec");
        let mut pack = xazz_compiler::Policy::builtin();
        pack.direct_identifiers.push("region".to_string());
        let _ = handle_policy_set(
            Extension("tenant-a".to_string()),
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
}
