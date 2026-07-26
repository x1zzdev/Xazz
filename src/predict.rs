//! predict.rs — xazz CLI Integration: NQP Prediction Handler
//!
//! Handles `xazz run <file> --predict`:
//!   1. Reads the .xzz source file
//!   2. Builds a JSON payload  {"source": "<code>"}
//!   3. Spawns Python subprocess → cli_integration/nqp_predict.py
//!   4. Pipes JSON to stdin, reads JSON from stdout
//!   5. Formats and prints the prediction result

use std::io::Write;
use std::process::{Command, Stdio};

/// Absolute path to the NQP prediction Python entry-point.
/// Resolved at compile time from the Cargo workspace root.
const NQP_SCRIPT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "\\cli_integration\\nqp_predict.py"
);

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

/// Run NQP semantic-prediction pipeline for `source_path`.
///
/// Returns `Ok(())` when the prediction has been printed to stdout.
/// Returns `Err(String)` with a human-readable error message on failure.
pub fn run_predict(source_path: &str) -> Result<(), String> {
    // ── 1. Read source file ──────────────────────────────────────────────────
    let source = std::fs::read_to_string(source_path).map_err(|e| {
        format!(
            "[xazz IO 에러]\n\
             ─────────────────────────────────────────────\n\
             Cause  : 소스 파일을 읽을 수 없습니다.\n\
             Detail : {}\n\
             → 경로와 파일 권한을 확인하세요.",
            e
        )
    })?;

    // ── 2. Build JSON payload ────────────────────────────────────────────────
    let payload = serde_json::json!({ "source": source });
    let payload_str = payload.to_string();

    println!("🔮  NQP 시맨틱 실행 예측을 시작합니다 …");
    println!("    파일   : {}", source_path);
    println!("    스크립트: {}", NQP_SCRIPT);
    println!("    모델   : C:\\checkpoint-2814  (Neural Query Planner)");
    println!();
    println!("  ※ 모델 로딩에 수 분이 소요될 수 있습니다. 잠시 기다려 주세요 …");
    println!();

    // ── 3. Spawn Python subprocess ───────────────────────────────────────────
    let mut child = Command::new("python")
        .arg(NQP_SCRIPT)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit()) // Python 로딩 진행상황은 stderr로 보여줌
        .spawn()
        .map_err(|e| {
            format!(
                "[xazz 예측 에러]\n\
                 ─────────────────────────────────────────────\n\
                 Cause  : Python 서브프로세스를 시작할 수 없습니다.\n\
                 Detail : {}\n\
                 → `python` 이 PATH에 등록되어 있는지 확인하세요.\n\
                 → 또는 `python3` 로 교체가 필요할 수 있습니다.",
                e
            )
        })?;

    // ── 4. Write JSON payload to subprocess stdin ────────────────────────────
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| "[xazz 예측 에러] stdin 파이프를 열 수 없습니다.".to_string())?;

        stdin
            .write_all(payload_str.as_bytes())
            .map_err(|e| format!("[xazz 예측 에러] stdin 쓰기 실패: {}", e))?;
        // `stdin` drops here → EOF is sent to Python process
    }

    // ── 5. Collect stdout + wait ─────────────────────────────────────────────
    let output = child
        .wait_with_output()
        .map_err(|e| format!("[xazz 예측 에러] 서브프로세스 대기 실패: {}", e))?;

    let stdout_str = String::from_utf8_lossy(&output.stdout).to_string();

    if !output.status.success() && stdout_str.trim().is_empty() {
        return Err(format!(
            "[xazz 예측 에러] Python 프로세스가 비정상 종료했습니다.\n\
             종료 코드: {}",
            output.status
        ));
    }

    // ── 6. Format and print prediction result ───────────────────────────────
    print_prediction_result(&stdout_str);
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Output formatter
// ─────────────────────────────────────────────────────────────────────────────

fn print_prediction_result(raw: &str) {
    // Locate the first JSON object in the output (skip any leading noise)
    let json_start = raw.find('{').unwrap_or(0);
    let json_str = &raw[json_start..];

    println!();
    println!("[NQP PREDICTION RESULT]");

    match serde_json::from_str::<serde_json::Value>(json_str) {
        Ok(v) => {
            let status = v["status"].as_str().unwrap_or("unknown");

            // ── Error from Python ────────────────────────────────────────────
            if status == "error" {
                let msg = v["message"].as_str().unwrap_or("알 수 없는 오류");
                println!("- Result: [ERROR]");
                println!("- Confidence: N/A");
                println!("- Warnings: {}", msg.lines().next().unwrap_or(msg));
                return;
            }

            // ── Result ──────────────────────────────────────────────────────
            let result = v["result"].as_str().unwrap_or("(결과 없음)");
            // Flatten multi-line result into single summary line for the header,
            // then print full result below
            let result_first_line = result.lines().next().unwrap_or(result);
            println!("- Result: {}", result_first_line);
            if result.lines().count() > 1 {
                for line in result.lines().skip(1) {
                    println!("           {}", line);
                }
            }

            // ── Confidence ──────────────────────────────────────────────────
            let confidence = v["confidence"].as_str().unwrap_or("unknown");
            println!("- Confidence: {}", confidence.to_uppercase());

            // ── Warnings ────────────────────────────────────────────────────
            if let Some(warns) = v["warnings"].as_array() {
                if warns.is_empty() {
                    println!("- Warnings: none");
                } else {
                    let warn_strs: Vec<&str> = warns.iter().filter_map(|w| w.as_str()).collect();
                    println!("- Warnings: {}", warn_strs.join("; "));
                }
            } else {
                println!("- Warnings: none");
            }

            // ── IR Summary (extra detail) ────────────────────────────────────
            if let Some(ir) = v.get("ir_summary") {
                let ops_count = ir["ops_count"].as_u64().unwrap_or(0);
                if let Some(ops) = ir["ops"].as_array() {
                    let op_names: Vec<&str> = ops.iter().filter_map(|o| o.as_str()).collect();
                    println!(
                        "- IR: {} op(s) parsed [{}]",
                        ops_count,
                        op_names.join(" → ")
                    );
                }
            }
        }

        Err(_) => {
            // JSON parse failed — print raw output safely
            let raw_preview = raw
                .lines()
                .next()
                .unwrap_or("(no output)")
                .trim()
                .to_string();
            println!("- Result: {}", raw_preview);
            if raw.lines().count() > 1 {
                for line in raw.lines().skip(1) {
                    let t = line.trim();
                    if !t.is_empty() {
                        println!("           {}", t);
                    }
                }
            }
            println!("- Confidence: UNKNOWN");
            println!("- Warnings: JSON parse failed — raw NQP output shown above");
        }
    }

    println!();
}
