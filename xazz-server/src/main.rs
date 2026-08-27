//! xazz-server — Visual IDE 연동 Axum HTTP API 서버 (v0.3)
//!
//! 엔드포인트:
//!   POST /execute          { "code": "<xzz DSL>" }         → 파이프라인 실행, JSON 결과 반환
//!   POST /schema           multipart/form-data (file)      → CSV 스키마 추론, 컬럼 타입 반환
//!   GET  /health           {}                               → 서버 상태 확인
//!   POST /security/audit   { "code": "<xzz DSL>" }         → SHA-256 감사 로그 생성 + 영구 저장
//!   POST /security/verify  { "code": "<xzz DSL>", "hash": "<sha256>" } → 감사 해시 검증
//!   GET  /security/audit/log                               → 전체 감사 로그 조회 (JSONL 해시 체인)
//!   GET  /security/audit/log/:hash                         → 코드 해시로 감사 레코드 조회
//!   GET  /security/audit/chain                             → 해시 체인 무결성 검증
//!   GET  /security/policy                                  → 활성 Policy-as-Code 정책 조회
//!   POST /security/policy/check { "code": "<xzz DSL>" }    → 정적 가드레일 검사 리포트
//!   POST /security/remediate    { "code": "<xzz DSL>" }    → 안전 코드 자동 보정 (결정적 + sLM)
//!
//! 포트: 8005 (frontend/.env: VITE_API_BASE_URL=http://127.0.0.1:8005)

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use axum::{
    extract::{Multipart, Path},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

mod audit_log;
mod guardrail;
mod slm;

// ── 요청 / 응답 타입 ────────────────────────────────────────────────────────

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
    /// `[xazz:dp]` 마커에서 파싱한 차등 프라이버시 감사 리포트 (v0.6).
    /// withDp(...) 미사용 시 None — 프론트엔드는 이를 "예산 미소모"로 표시한다.
    #[serde(skip_serializing_if = "Option::is_none")]
    dp: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<Value>,
    /// Policy-as-Code 정적 가드레일 리포트 (v0.7 — issue #2).
    /// 차단된 요청에서는 차단 사유가, 통과한 요청에서는 경고가 담긴다.
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
    // uploads/ 디렉터리 미리 생성
    let _ = std::fs::create_dir_all("uploads");

    let cors = CorsLayer::new()
        .allow_origin(Any)
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
        .layer(cors);

    let app = match web_root {
        Some(root) => app.fallback_service(ServeDir::new(root).not_found_service(serve_index())),
        None => app,
    };

    let addr = "127.0.0.1:8005";
    println!("[xazz-server] 🚀 Listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// ── Static IDE serving ───────────────────────────────────────────────────────

/// 빌드된 Visual IDE 정적 자산 디렉터리를 찾는다. 없으면 None.
///
/// 우선순위:
///   1. 환경변수 `XAZZ_WEB_DIR` (명시적 지정)
///   2. 실행 바이너리 옆의 `web/`
///   3. 실행 바이너리 상위의 `web/` (pkg에서 bin/ 과 web/ 를 나란히 둘 경우)
///   4. 현재 작업 디렉터리의 `web/`
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

/// SPA 폴백 — ServeDir 에서 매치되는 파일이 없으면 index.html 을 반환한다.
/// Vite SPA 라우터(/editor, /monitor 등)가 클라이언트 사이드에서 처리하도록 한다.
fn serve_index() -> tower_http::services::ServeFile {
    let web_root = resolve_web_dir().unwrap_or_else(|| PathBuf::from("web"));
    let index = web_root.join("index.html");
    tower_http::services::ServeFile::new(index)
}

// ── POST /execute ─────────────────────────────────────────────────────────────

async fn handle_execute(
    Json(payload): Json<ExecuteRequest>,
) -> Result<Json<ExecuteResponse>, (StatusCode, Json<ExecuteResponse>)> {
    // 0. Policy-as-Code 정적 가드레일 (issue #2)
    //
    //    위반이면 여기서 끝난다 — 임시 파일도 만들지 않고 xazz 실행기도 스폰하지 않는다.
    //    정책을 불러오지 못한 경우에도 마찬가지로 거부한다 (fail-closed).
    let policy_report = match guardrail::gate(&payload.code) {
        guardrail::Decision::Reject { report } => {
            // 차단 역시 감사 대상이다 — 무엇이 왜 막혔는지 영구 기록에 남긴다.
            if let Err(e) = audit_log::append_with_outcome(&payload.code, Some("blocked")) {
                eprintln!("[xazz] ⚠️ 차단 감사 기록 실패: {}", e);
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

    // 1. DSL 코드를 임시 .xzz 파일에 저장
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

    // 2. xazz.exe 실행 파일 경로 탐색
    let exe_path = find_xazz_exe();

    // 3. xazz run <tmp.xzz> 실행
    //    게이트를 통과한 요청만 이 지점에 도달한다 — 테스트가 카운터로 검증한다.
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

    // 4. stdout 파싱: [xazz:result], [xazz:chart], [xazz:train], [xazz:dp] 마커 추출
    let (rows, schema, logs, training, dp, diagnostics) = parse_stdout_markers(&stdout, &stderr);

    // 5. 실행 이력 자동 감사 기록 (신뢰성 인프라 — 모든 연산 이력 영구 보존)
    //    실패해도 실행은 반환하되, 감사 기록 실패만 로그에 경고로 남긴다.
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

/// stdout 에서 [xazz:result], [xazz:chart], [xazz:train], [xazz:diagnostics], [xazz:dp] 마커를 파싱한다.
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
        // Burn 딥러닝 학습 결과 마커 (같은 줄에 JSON)
        if let Some(json_part) = trimmed.strip_prefix("[xazz:train] ") {
            if let Ok(parsed) = serde_json::from_str::<Value>(json_part) {
                training = Some(parsed);
            }
        }
        // 차등 프라이버시 감사 마커 — 두 줄: "[xazz:dp]" 다음 줄에 JSON 리포트.
        if trimmed == "[xazz:dp]" {
            let next = lines.get(i + 1).map(|l| l.trim()).unwrap_or("");
            if let Ok(parsed) = serde_json::from_str::<Value>(next) {
                dp = Some(parsed);
                i += 1; // JSON 줄은 소비
            }
        }
        // Policy-as-Code 가드레일 마커 — 실행 엔진이 내보낸 정책 리포트.
        // 서버는 앞단에서 이미 같은 검사를 했지만, 실행 엔진의 판정을 그대로
        // 신뢰할 수 있도록 마커도 로그로 남긴다.
        if let Some(json_part) = trimmed.strip_prefix("[xazz:policy] ") {
            if serde_json::from_str::<Value>(json_part).is_err() {
                eprintln!("[xazz] ⚠️ [xazz:policy] 마커 파싱 실패");
            }
        }
        // 정적 의미 분석(Type Checker) 진단 마커
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
    // multipart 에서 파일 필드 추출
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
            file_bytes = Some(data.to_vec());
        }
    }

    let bytes = file_bytes.ok_or((StatusCode::BAD_REQUEST, "파일 필드 없음".to_string()))?;

    // 저장 경로 생성 (uploads/<uuid>_<name>)
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

    // 인코딩 감지 및 CSV 파싱
    let text = decode_bytes(&bytes);
    let schema = infer_csv_schema_from_text(&text);

    Ok(Json(SchemaResponse { schema, file_path }))
}

// ── CSV 스키마 추론 (xazz import 와 동일 로직) ────────────────────────────────

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

    // 컬럼별로 샘플 값을 수집
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

    // append-only 감사 로그에 영구 저장 (실패해도 해시는 반환)
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
}

async fn handle_security_verify(Json(payload): Json<VerifyRequest>) -> Json<VerifyResponse> {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(payload.code.as_bytes());
    let computed = format!("{:x}", hasher.finalize());
    let valid = computed == payload.hash;
    // 로그에 존재하는지 여부도 함께 반환
    let logged = audit_log::lookup_by_hash(&payload.hash)
        .ok()
        .map(|r| !r.is_empty());

    Json(VerifyResponse {
        valid,
        computed_hash: computed,
        provided_hash: payload.hash,
        algorithm: "SHA-256".to_string(),
        logged,
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

/// 현재 적용 중인 Policy-as-Code 정책을 그대로 돌려준다.
///
/// 프런트엔드는 이 응답으로 "어떤 컬럼이 왜 막히는지"를 사용자에게 미리
/// 보여줄 수 있다. 정책 로딩에 실패하면 500 과 함께 사유를 돌려준다.
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

/// 코드를 실행하지 않고 정적 가드레일 검사만 수행한다.
///
/// Visual IDE 는 편집 중에 이 엔드포인트를 호출해 실행 버튼을 누르기 전에
/// 위반을 표시한다. 위반이 있어도 HTTP 200 이다 — 검사 자체는 성공했으며,
/// 판정은 본문의 `safe_to_execute` 에 담긴다.
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

/// 차단된 코드를 안전한 대체 코드로 보정하고 위반 리포트를 함께 반환한다.
///
/// 보정 전략은 두 단계다.
///   1. 결정적 보정 — AST 를 직접 고쳐 항상 동작하는 안전 코드를 만든다.
///   2. 온프레미스 sLM(Qwen2.5-Coder) — 켜져 있으면 더 자연스러운 재작성을
///      제안하되, **같은 정책 엔진으로 재검증**을 통과할 때만 채택된다.
///
/// 응답의 `remediation.verified` 가 false 면 사람이 처리해야 할 위반이
/// 남아 있다는 뜻이다. 이 경우 보정 코드를 "안전하다"고 표시하면 안 된다.
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

// ── 유틸리티 ──────────────────────────────────────────────────────────────────

fn find_xazz_exe() -> PathBuf {
    // 0. 환경변수로 경로 고정 (배포 하드닝) — 지정되면 PATH 폴백을 절대 수행하지 않는다
    if let Ok(pinned) = std::env::var("XAZZ_EXEC_PATH") {
        if !pinned.trim().is_empty() {
            return PathBuf::from(pinned);
        }
    }

    // 플랫폼별 실행 파일명
    let names: &[&str] = if cfg!(windows) {
        &["xazz.exe"]
    } else {
        &["xazz", "xazz.exe"]
    };

    // 1. 현재 실행파일과 같은 디렉터리
    if let Ok(current_exe) = std::env::current_exe() {
        let dir = current_exe.parent().unwrap_or(&current_exe);
        for name in names {
            let sibling = dir.join(name);
            if sibling.exists() {
                return sibling;
            }
        }
    }
    // 2. target/release (CWD 기준)
    for name in names {
        let candidate = PathBuf::from("target/release").join(name);
        if candidate.exists() {
            return candidate;
        }
    }
    // 3. PATH fallback
    eprintln!(
        "[xazz WARN] 실행기 xazz 를 PATH 에서 찾았습니다 (운영 환경에서는 XAZZ_EXEC_PATH 로 절대 경로를 고정하세요)"
    );
    PathBuf::from("xazz")
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

// ── 통합 테스트 — 실행 게이트 (issue #2) ─────────────────────────────────────
//
// 여기서 증명하려는 것은 "위반 코드가 거부된다"가 아니라
// **"위반 코드에서는 실행기가 아예 호출되지 않는다"** 이다.
// 앞의 것은 분석 결과일 뿐이고, 뒤의 것이 실제 보안 속성이다.

#[cfg(test)]
mod tests {
    use super::*;

    const UNSAFE_CODE: &str =
        "type Patient = { patient_id: string, name: string, age_band: string };
v out = load(\"data/p.csv\") :: Patient |> select([name, patient_id, age_band]);";

    /// 위반 코드는 422 로 거부되고, 리포트가 본문에 실린다.
    #[tokio::test]
    async fn violating_code_is_rejected_with_422() {
        let result = handle_execute(Json(ExecuteRequest {
            code: UNSAFE_CODE.to_string(),
        }))
        .await;

        let (status, body) = result.err().expect("위반 코드가 거부되지 않았다");
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(!body.0.success);
        let policy = body.0.policy.as_ref().expect("정책 리포트가 없다");
        assert_eq!(policy["safe_to_execute"], serde_json::Value::Bool(false));
        assert!(
            policy["violations"]
                .as_array()
                .map(|v| !v.is_empty())
                .unwrap_or(false),
            "위반 목록이 비어 있다: {:?}",
            policy
        );
    }

    /// 거부된 요청에서는 xazz 실행기가 단 한 번도 스폰되지 않는다.
    #[tokio::test]
    async fn rejected_request_never_invokes_runner() {
        let before = guardrail::runner_invocations();

        let _ = handle_execute(Json(ExecuteRequest {
            code: UNSAFE_CODE.to_string(),
        }))
        .await;

        assert_eq!(
            guardrail::runner_invocations(),
            before,
            "차단된 요청인데 실행기가 호출되었다"
        );
    }

    /// 파싱조차 되지 않는 코드도 fail-closed 로 거부되며 실행기를 부르지 않는다.
    #[tokio::test]
    async fn unparseable_code_is_rejected_without_running() {
        let before = guardrail::runner_invocations();

        let result = handle_execute(Json(ExecuteRequest {
            code: "v x = |> |> ???".to_string(),
        }))
        .await;

        let (status, _) = result.err().expect("파싱 불가 코드가 거부되지 않았다");
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(guardrail::runner_invocations(), before);
    }

    /// 하드코딩된 비밀키가 있으면 거부된다.
    #[tokio::test]
    async fn hardcoded_secret_is_rejected() {
        let code = "// AKIAIOSFODNN7EXAMPLE\n\
                    type P = { age_band: string };\n\
                    v x = load(\"d.csv\") :: P |> select([age_band]);";
        let result = handle_execute(Json(ExecuteRequest {
            code: code.to_string(),
        }))
        .await;
        let (status, body) = result.err().expect("비밀키가 있는데 통과했다");
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        // 리포트에 원본 키가 실려서는 안 된다.
        let serialized = serde_json::to_string(&body.0.policy).unwrap_or_default();
        assert!(
            !serialized.contains("AKIAIOSFODNN7EXAMPLE"),
            "리포트에 원본 비밀키가 노출되었다"
        );
    }

    /// /security/policy/check 는 위반이 있어도 200 이며 판정은 본문에 담긴다.
    #[tokio::test]
    async fn policy_check_returns_report_without_executing() {
        let before = guardrail::runner_invocations();

        let response = handle_policy_check(Json(guardrail::CodeRequest {
            code: UNSAFE_CODE.to_string(),
        }))
        .await
        .expect("정책 검사 실패");

        assert!(!response.0.safe_to_execute);
        assert!(!response.0.policy.violations.is_empty());
        assert_eq!(guardrail::runner_invocations(), before);
    }

    /// /security/remediate 는 검증된 안전 코드와 리포트를 함께 돌려준다.
    #[tokio::test]
    async fn remediate_returns_verified_safe_code() {
        let response = handle_remediate(Json(guardrail::CodeRequest {
            code: UNSAFE_CODE.to_string(),
        }))
        .await
        .expect("보정 실패");

        assert!(!response.0.safe_to_execute, "원본은 위반이어야 한다");
        let rem = &response.0.remediation;
        // 이 코드는 남길 컬럼이 age_band 하나 있으므로 보정이 가능하다.
        assert!(
            rem.verified,
            "보정 코드가 검증되지 않았다: {}",
            rem.report_after.render()
        );
        // 보정된 코드는 실제로 정책을 통과해야 한다 — 말이 아니라 재검증으로.
        let policy = xazz_compiler::Policy::builtin();
        let recheck = xazz_compiler::check_policy(&rem.code, &policy);
        assert!(
            recheck.safe_to_execute,
            "보정 코드가 여전히 위반이다: {}",
            recheck.render()
        );
    }

    /// 안전한 코드는 게이트를 통과한다 (오탐 회귀 방지).
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
