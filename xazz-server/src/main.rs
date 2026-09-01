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
//!   GET  /security/policy                                  → view the active Policy-as-Code policy
//!   POST /security/policy/check { "code": "<xzz DSL>" }    → static guardrail inspection report
//!   POST /security/remediate    { "code": "<xzz DSL>" }    → safe code auto-remediation (deterministic + sLM)
//!
//! Port: 8005 (frontend/.env: VITE_API_BASE_URL=http://127.0.0.1:8005)

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use axum::{
    Router,
    extract::{Multipart, Path, State},
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

/// Upper bound on concurrently running `xazz run` processes — execution DoS prevention.
/// Over-limit requests are rejected immediately with 429 (no queue → prevents
/// follow-up request backlog buildup).
const MAX_CONCURRENT_EXECUTIONS: usize = 4;

/// /schema upload maximum allowed size (bytes).
const MAX_UPLOAD_BYTES: usize = 50 * 1024 * 1024;

/// AppState — execution semaphore shared across requests.
#[derive(Clone)]
struct AppState {
    exec_permits: Arc<Semaphore>,
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
        .route("/security/policy", get(handle_policy_info))
        .route("/security/policy/check", post(handle_policy_check))
        .route("/security/remediate", post(handle_remediate))
        .with_state(AppState {
            exec_permits: Arc::new(Semaphore::new(MAX_CONCURRENT_EXECUTIONS)),
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

    let addr = "127.0.0.1:8005";
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

// ── Optional Bearer token auth middleware ─────────────────────────────────────

/// When `XAZZ_SERVER_TOKEN` is set, every request requires `Authorization: Bearer <token>`.
/// When unset (or empty), all requests pass (default local-only behavior).
async fn optional_bearer_auth(
    req: axum::extract::Request,
    next: Next,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let expected = std::env::var("XAZZ_SERVER_TOKEN").unwrap_or_default();
    if expected.is_empty() {
        return Ok(next.run(req).await);
    }
    let ok = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|h: &HeaderValue| h.to_str().ok())
        .is_some_and(|h| h == format!("Bearer {expected}"));
    if ok {
        Ok(next.run(req).await)
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token".into(),
        ))
    }
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
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("web"));
            if let Some(grand) = parent.parent() {
                candidates.push(grand.join("web"));
            }
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

async fn handle_execute(
    State(state): State<AppState>,
    Json(payload): Json<ExecuteRequest>,
) -> Result<Json<ExecuteResponse>, (StatusCode, Json<ExecuteResponse>)> {
    // 0a. Concurrency semaphore — if no permit, deny execution (fail-closed, no queue).
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
                }),
            ));
        }
    };

    // 0. Policy-as-Code static guardrail (issue #2)
    //
    //    On violation this is where it ends — no temp file is created and the xazz
    //    runner is not spawned. It also denies when the policy cannot be loaded
    //    (fail-closed).
    let policy_report = match guardrail::gate(&payload.code) {
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
    let exe_path = find_xazz_exe().map_err(|e| internal_err(e))?;

    // 3. Run xazz run <tmp.xzz>
    //    Only requests that pass the gate reach this point — tests verify with the counter.
    guardrail::note_runner_invocation();
    let output = tokio::task::spawn_blocking(move || {
        Command::new(&exe_path).arg("run").arg(&tmp_path).output()
    })
    .await
    .map_err(|e| internal_err(format!("spawn_blocking 실패: {}", e)))?
    .map_err(|e| internal_err(format!("xazz.exe 실행 실패: {}", e)))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    let success = output.status.success();

    // 4. Parse stdout: extract [xazz:result], [xazz:chart], [xazz:train], [xazz:dp] markers
    let (rows, schema, logs, training, dp, diagnostics) = parse_stdout_markers(&stdout, &stderr);

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
        }))
    } else {
        let err_msg = stderr.lines().last().unwrap_or("실행 실패").to_string();
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
            error: Some(err_msg),
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
        if let Some(json_part) = trimmed.strip_prefix("[xazz:result] ") {
            if let Ok(parsed) = serde_json::from_str::<Value>(json_part) {
                if let Some(r) = parsed.get("rows") {
                    rows = r.clone();
                }
                if let Some(s) = parsed.get("schema") {
                    schema = s.clone();
                }
            }
        }
        // Burn deep-learning training result marker (JSON on the same line)
        if let Some(json_part) = trimmed.strip_prefix("[xazz:train] ") {
            if let Ok(parsed) = serde_json::from_str::<Value>(json_part) {
                training = Some(parsed);
            }
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
        if let Some(json_part) = trimmed.strip_prefix("[xazz:policy] ") {
            if serde_json::from_str::<Value>(json_part).is_err() {
                eprintln!("[xazz] ⚠️ [xazz:policy] 마커 파싱 실패");
            }
        }
        // Static semantic analysis (Type Checker) diagnostics marker
        if let Some(json_part) = trimmed.strip_prefix("[xazz:diagnostics] ") {
            if let Ok(parsed) = serde_json::from_str::<Value>(json_part) {
                diagnostics = Some(parsed);
            }
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

/// Returns the currently active Policy-as-Code policy as-is.
///
/// The frontend can use this response to show the user "which column is blocked
/// and why" in advance. If policy loading fails, it returns 500 with the reason.
async fn handle_policy_info() -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match guardrail::load_policy() {
        Ok((policy, origin)) => Ok(Json(json!({
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

// ── POST /security/policy/check ──────────────────────────────────────────────

/// Performs only a static guardrail check without executing the code.
///
/// The Visual IDE calls this endpoint while editing to show violations before the
/// run button is pressed. Even with violations it returns HTTP 200 — the check
/// itself succeeded; the verdict is in the body's `safe_to_execute`.
async fn handle_policy_check(
    Json(payload): Json<guardrail::CodeRequest>,
) -> Result<Json<guardrail::PolicyCheckResponse>, (StatusCode, Json<Value>)> {
    let (policy, origin) = guardrail::load_policy().map_err(|report| {
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
    Json(payload): Json<guardrail::CodeRequest>,
) -> Result<Json<guardrail::RemediateResponse>, (StatusCode, Json<Value>)> {
    let (policy, origin) = guardrail::load_policy().map_err(|report| {
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

// ── utilities ──────────────────────────────────────────────────────────────────

fn find_xazz_exe() -> Result<PathBuf, String> {
    // 1. Pin the path via env var (deployment hardening)
    if let Ok(pinned) = std::env::var("XAZZ_EXEC_PATH") {
        if !pinned.trim().is_empty() {
            return Ok(PathBuf::from(pinned));
        }
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
        AppState {
            exec_permits: Arc::new(Semaphore::new(64)),
        }
    }

    const UNSAFE_CODE: &str =
        "type Patient = { patient_id: string, name: string, age_band: string };
v out = load(\"data/p.csv\") :: Patient |> select([name, patient_id, age_band]);";

    /// Violating code is rejected with 422 and the report is in the body.
    #[tokio::test]
    async fn violating_code_is_rejected_with_422() {
        let result = handle_execute(
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

        let response = handle_policy_check(Json(guardrail::CodeRequest {
            code: UNSAFE_CODE.to_string(),
        }))
        .await
        .expect("policy check failed");

        assert!(!response.0.safe_to_execute);
        assert!(!response.0.policy.violations.is_empty());
        assert_eq!(guardrail::runner_invocations(), before);
    }

    /// /security/remediate returns verified safe code and the report together.
    #[tokio::test]
    async fn remediate_returns_verified_safe_code() {
        let response = handle_remediate(Json(guardrail::CodeRequest {
            code: UNSAFE_CODE.to_string(),
        }))
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
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(handle_execute(
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
}
