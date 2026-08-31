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
use std::time::{Duration, Instant};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 실행 엔진(xazz-exec)의 기본 최대 실행 시간(초).
/// `XAZZ_EXEC_TIMEOUT_SECS` 환경변수로 재정의 가능 (0 이하 무시).
const DEFAULT_EXEC_TIMEOUT_SECS: u64 = 300;

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
            "[xazz-runner] usage: xazz-runner <file.xzz|file.csv> [--verbose] [--output <path.csv>]"
        );
        eprintln!("[xazz-runner] helpers: --version | --help | --check-engine");
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
    // 실행 시간 제한(타임아웃)은 DoS 방지용 경량 하드닝이다.
    // (참고: 이는 프로세스 격리이지 OS 샌드박스가 아니다 — seccomp/landlock 은 별도 마일스톤)
    let mut child = Command::new(&exec_path)
        .args(&args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap_or_else(|e| {
            eprintln!(
                "[xazz-runner] ERROR: could not start the xazz-exec execution engine."
            );
            eprintln!("[xazz-runner] path  : {}", exec_path.display());
            eprintln!("[xazz-runner] cause : {}", e);
            eprintln!(
                "[xazz-runner] check that the xazz-exec binary is in the same directory as xazz-runner."
            );
            std::process::exit(1);
        });

    let timeout_secs = std::env::var("XAZZ_EXEC_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_EXEC_TIMEOUT_SECS);

    let started = Instant::now();
    let code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code().unwrap_or(1),
            Ok(None) => {
                if started.elapsed().as_secs() >= timeout_secs {
                    let _ = child.kill();
                    let _ = child.wait();
                    eprintln!(
                        "[xazz-runner] ERROR: execution exceeded {timeout_secs}s — engine terminated. \
                         (adjustable via the XAZZ_EXEC_TIMEOUT_SECS environment variable)"
                    );
                    std::process::exit(1);
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                eprintln!(
                    "[xazz-runner] ERROR: failed to check execution engine status: {e}"
                );
                std::process::exit(1);
            }
        }
    };

    // xazz-exec 종료 코드를 그대로 전파
    std::process::exit(code);
}

fn print_usage() {
    println!("xazz-runner {} — Xazz execution-engine IPC bridge", VERSION);
    println!();
    println!("usage:");
    println!("  xazz-runner <file.xzz|file.csv> [--verbose] [--output <path.csv>]");
    println!();
    println!("  --version, -V        print version");
    println!("  --help, -h           show this help");
    println!("  --check-engine       diagnose availability of the execution engine (xazz-exec)");
}

/// xazz-exec 실행 엔진의 존재와 실행 가능 여부를 진단한다.
///
/// 종료 코드: 0 = 정상, 1 = 엔진 누락/실행 불가
fn check_engine() {
    let mut ok = true;
    println!("[xazz-runner] execution-engine diagnostics");

    let exec_path = match resolve_exec_binary() {
        Ok(p) => {
            println!("  engine path : {}", p.display());
            p
        }
        Err(msg) => {
            eprintln!("  error      : {}", msg);
            eprintln!(
                "[xazz-runner] diagnostics failed — place xazz-exec in the same directory as xazz-runner, or set XAZZ_EXEC_PATH."
            );
            std::process::exit(1);
        }
    };

    if exec_path.exists() {
        println!("  exists     : yes");
    } else {
        println!("  exists     : no");
        ok = false;
    }

    // 실행 가능 여부는 실제로 --version 호출로 확인
    if ok {
        match Command::new(&exec_path).arg("--version").output() {
            Ok(out) => {
                let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
                println!(
                    "  runnable   : yes ({}{})",
                    ver,
                    if out.status.success() {
                        ""
                    } else {
                        " [abnormal exit]"
                    }
                );
                if !out.status.success() {
                    ok = false;
                }
            }
            Err(e) => {
                println!("  runnable   : no ({})", e);
                ok = false;
            }
        }
    }

    if ok {
        println!("[xazz-runner] diagnostics passed — execution engine available");
    } else {
        eprintln!(
            "[xazz-runner] diagnostics failed — place xazz-exec in the same directory as xazz-runner, or set XAZZ_EXEC_PATH."
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
