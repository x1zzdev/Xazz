//! xazz-server — Visual IDE 연동 Axum HTTP API 서버 (v0.3)
//!
//! 엔드포인트:
//!   POST /execute          { "code": "<xzz DSL>" }         → 파이프라인 실행, JSON 결과 반환
//!   POST /schema           multipart/form-data (file)      → CSV 스키마 추론, 컬럼 타입 반환
//!   GET  /health           {}                               → 서버 상태 확인
//!   POST /security/audit   { "code": "<xzz DSL>" }         → SHA-256 감사 로그 생성
//!   POST /security/verify  { "code": "<xzz DSL>", "hash": "<sha256>" } → 감사 해시 검증
//!
//! 포트: 8005 (frontend/.env: VITE_API_BASE_URL=http://127.0.0.1:8005)

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use axum::{
    extract::Multipart,
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tower_http::cors::{Any, CorsLayer};

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

    let app = Router::new()
        .route("/execute", post(handle_execute))
        .route("/schema", post(handle_schema))
        .route("/health", get(handle_health))
        .route("/security/audit", post(handle_security_audit))
        .route("/security/verify", post(handle_security_verify))
        .layer(cors);

    let addr = "127.0.0.1:8005";
    println!("[xazz-server] 🚀 Listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// ── POST /execute ─────────────────────────────────────────────────────────────

async fn handle_execute(
    Json(payload): Json<ExecuteRequest>,
) -> Result<Json<ExecuteResponse>, (StatusCode, Json<ExecuteResponse>)> {
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
    let output = tokio::task::spawn_blocking(move || {
        Command::new(&exe_path).arg("run").arg(&tmp_path).output()
    })
    .await
    .map_err(|e| internal_err(format!("spawn_blocking 실패: {}", e)))?
    .map_err(|e| internal_err(format!("xazz.exe 실행 실패: {}", e)))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    let success = output.status.success();

    // 4. stdout 파싱: [xazz:result], [xazz:chart], [xazz:train] 마커 추출
    let (rows, schema, logs, training) = parse_stdout_markers(&stdout, &stderr);

    if success {
        Ok(Json(ExecuteResponse {
            success: true,
            rows,
            schema,
            logs,
            stdout,
            training,
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
            error: Some(err_msg),
        }))
    }
}

/// stdout 에서 [xazz:result], [xazz:chart], [xazz:train] 마커를 파싱한다.
fn parse_stdout_markers(stdout: &str, stderr: &str) -> (Value, Value, Vec<String>, Option<Value>) {
    let mut rows = json!([]);
    let mut schema = json!([]);
    let mut training: Option<Value> = None;
    let logs: Vec<String> = stderr.lines().map(|l| l.to_string()).collect();

    for line in stdout.lines() {
        let trimmed = line.trim();
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
        // Burn 딥러닝 학습 결과 마커
        if let Some(json_part) = trimmed.strip_prefix("[xazz:train] ") {
            if let Ok(parsed) = serde_json::from_str::<Value>(json_part) {
                training = Some(parsed);
            }
        }
    }

    (rows, schema, logs, training)
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
}

async fn handle_security_audit(Json(payload): Json<AuditRequest>) -> Json<AuditResponse> {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(payload.code.as_bytes());
    let hash = format!("{:x}", hasher.finalize());

    Json(AuditResponse {
        hash,
        algorithm: "SHA-256".to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        code_length: payload.code.len(),
    })
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
}

async fn handle_security_verify(Json(payload): Json<VerifyRequest>) -> Json<VerifyResponse> {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(payload.code.as_bytes());
    let computed = format!("{:x}", hasher.finalize());
    let valid = computed == payload.hash;

    Json(VerifyResponse {
        valid,
        computed_hash: computed,
        provided_hash: payload.hash,
        algorithm: "SHA-256".to_string(),
    })
}

// ── 유틸리티 ──────────────────────────────────────────────────────────────────

fn find_xazz_exe() -> PathBuf {
    // 1. 현재 실행파일과 같은 디렉터리
    if let Ok(current_exe) = std::env::current_exe() {
        let sibling = current_exe
            .parent()
            .unwrap_or(&current_exe)
            .join("xazz.exe");
        if sibling.exists() {
            return sibling;
        }
    }
    // 2. target/release/xazz.exe (CWD 기준)
    let candidate = PathBuf::from("target/release/xazz.exe");
    if candidate.exists() {
        return candidate;
    }
    // 3. PATH fallback
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
            error: Some(msg),
        }),
    )
}
