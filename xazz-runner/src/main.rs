// xazz-runner/src/main.rs
//
// xazz IPC 브리지 — CLI와 실행 엔진 사이의 경량 릴레이
//
// ✅  이 바이너리는 Polars/tokio/rayon/reqwest/hyper를 링크하지 않는다.
// ✅  허용 의존성: serde, serde_json, std 만
//
// 아키텍처:
//   xazz CLI (NO Polars)
//     ↓ std::process::Command (spawn)
//   xazz-runner  (NO Polars — this binary)
//     ↓ std::process::Command (spawn)
//   xazz-exec    (Polars + tokio + rayon 격리)
//
// 통신 프로토콜:
//   - xazz CLI → xazz-runner : CLI args 전달
//   - xazz-runner → xazz-exec : args 그대로 전달, stdout/stderr 상속
//   - 종료 코드: xazz-exec 종료 코드를 그대로 전파

use std::path::PathBuf;
use std::process::{Command, Stdio};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() {
        eprintln!(
            "[xazz-runner] 사용법: xazz-runner <file.xzz|file.csv> [--verbose] [--output <path.csv>]"
        );
        std::process::exit(1);
    }

    // ── xazz-exec 바이너리 경로 해석 ──────────────────────────────────────
    // 1순위: 현재 실행 파일과 동일 디렉터리의 xazz-exec[.exe]
    // 2순위: PATH에서 검색
    let exec_path = resolve_exec_binary();

    // ── xazz-exec 서브프로세스 스폰 ───────────────────────────────────────
    // stdin/stdout/stderr 상속 → 투명한 IPC relay
    let status = Command::new(&exec_path)
        .args(&args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .unwrap_or_else(|e| {
            eprintln!(
                "[xazz-runner] ERROR: xazz-exec 실행 엔진을 시작할 수 없습니다."
            );
            eprintln!("[xazz-runner] 경로: {}", exec_path.display());
            eprintln!("[xazz-runner] 원인: {}", e);
            eprintln!(
                "[xazz-runner] xazz-exec 바이너리가 xazz-runner와 같은 디렉터리에 있는지 확인하세요."
            );
            std::process::exit(1);
        });

    // xazz-exec 종료 코드를 그대로 전파
    std::process::exit(status.code().unwrap_or(1));
}

/// xazz-exec 바이너리 경로를 해석한다.
///
/// 우선순위:
/// 1. 현재 실행 파일(xazz-runner)과 같은 디렉터리
/// 2. PATH 검색 폴백
fn resolve_exec_binary() -> PathBuf {
    #[cfg(target_os = "windows")]
    let exec_name = "xazz-exec.exe";
    #[cfg(not(target_os = "windows"))]
    let exec_name = "xazz-exec";

    // 현재 실행 파일 옆에서 찾기
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(dir) = current_exe.parent() {
            let candidate = dir.join(exec_name);
            if candidate.exists() {
                return candidate;
            }
        }
    }

    // PATH 폴백
    PathBuf::from(exec_name)
}
