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

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // ── 도우미 플래그: --version / --help / --check-engine ──────────────────
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("xazz-runner {}", VERSION);
        return;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return;
    }

    // 실행 엔진 존재/가용성 진단 (변경 없이 확인만)
    if args
        .iter()
        .any(|a| a == "--check-engine" || a == "--doctor")
    {
        check_engine();
        return;
    }

    if args.is_empty() {
        eprintln!(
            "[xazz-runner] 사용법: xazz-runner <file.xzz|file.csv> [--verbose] [--output <path.csv>]"
        );
        eprintln!("[xazz-runner] 도우미: --version | --help | --check-engine");
        std::process::exit(1);
    }

    // ── xazz-exec 바이너리 경로 해석 ──────────────────────────────────────
    let exec_path = match resolve_exec_binary() {
        Ok(p) => p,
        Err(msg) => {
            eprintln!("[xazz-runner] ERROR: {}", msg);
            std::process::exit(1);
        }
    };

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

fn print_usage() {
    println!("xazz-runner {} — Xazz 실행 엔진 IPC 브리지", VERSION);
    println!();
    println!("사용법:");
    println!("  xazz-runner <file.xzz|file.csv> [--verbose] [--output <path.csv>]");
    println!();
    println!("  --version, -V        버전 출력");
    println!("  --help, -h           이 도움말");
    println!("  --check-engine       실행 엔진(xazz-exec) 가용성 진단");
}

/// xazz-exec 실행 엔진의 존재와 실행 가능 여부를 진단한다.
///
/// 종료 코드: 0 = 정상, 1 = 엔진 누락/실행 불가
fn check_engine() {
    let mut ok = true;
    println!("[xazz-runner] 실행 엔진 진단");

    let exec_path = match resolve_exec_binary() {
        Ok(p) => {
            println!("  엔진 경로 : {}", p.display());
            p
        }
        Err(msg) => {
            eprintln!("  오류     : {}", msg);
            eprintln!(
                "[xazz-runner] 진단 실패 — xazz-exec 를 xazz-runner 와 같은 디렉터리에 배치하거나 XAZZ_EXEC_PATH 를 설정하세요."
            );
            std::process::exit(1);
        }
    };

    if exec_path.exists() {
        println!("  존재     : 예");
    } else {
        println!("  존재     : 아니오");
        ok = false;
    }

    // 실행 가능 여부는 실제로 --version 호출로 확인
    if ok {
        match Command::new(&exec_path).arg("--version").output() {
            Ok(out) => {
                let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
                println!(
                    "  실행     : 예 ({}{})",
                    ver,
                    if out.status.success() {
                        ""
                    } else {
                        " [비정상 종료]"
                    }
                );
                if !out.status.success() {
                    ok = false;
                }
            }
            Err(e) => {
                println!("  실행     : 아니오 ({})", e);
                ok = false;
            }
        }
    }

    if ok {
        println!("[xazz-runner] 진단 통과 — 실행 엔진 사용 가능");
    } else {
        eprintln!(
            "[xazz-runner] 진단 실패 — xazz-exec 를 xazz-runner 와 같은 디렉터리에 배치하거나 XAZZ_EXEC_PATH 를 설정하세요."
        );
    }
    std::process::exit(if ok { 0 } else { 1 });
}

/// xazz-exec 바이너리 경로를 해석한다.
///
/// 우선순위:
/// 1. `XAZZ_EXEC_PATH` 환경변수 (배포 하드닝)
/// 2. 현재 실행 파일(xazz-runner)과 같은 디렉터리
///
/// PATH 폴백은 수행하지 않는다 (PATH 셰도잉으로 임의 코드 실행되는 것을 방지).
/// 찾지 못하면 `Err`를 반환한다 (fail-closed).
fn resolve_exec_binary() -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    let exec_name = "xazz-exec.exe";
    #[cfg(not(target_os = "windows"))]
    let exec_name = "xazz-exec";

    // 1. 환경변수로 경로 고정 (배포 하드닝)
    if let Ok(pinned) = std::env::var("XAZZ_EXEC_PATH") {
        if !pinned.trim().is_empty() {
            return Ok(PathBuf::from(pinned));
        }
    }

    // 2. 현재 실행 파일 옆에서 찾기
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(dir) = current_exe.parent() {
            let candidate = dir.join(exec_name);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    Err(format!(
        "xazz-exec 실행 엔진을 찾을 수 없습니다 (PATH 폴백은 보안상 비활성화됨). \
         XAZZ_EXEC_PATH 로 절대 경로를 지정하거나 xazz-exec 를 xazz-runner 와 같은 디렉터리에 배치하세요."
    ))
}
