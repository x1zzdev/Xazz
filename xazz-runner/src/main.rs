// xazz-runner/src/main.rs
//
// xazz IPC bridge — a lightweight relay between the CLI and the execution engine
//
// ✅  This binary does not link Polars/tokio/rayon/reqwest/hyper.
// ✅  Allowed dependencies: serde, serde_json, std only
//
// Architecture:
//   xazz CLI (NO Polars)
//     ↓ std::process::Command (spawn)
//   xazz-runner  (NO Polars — this binary)
//     ↓ std::process::Command (spawn)
//   xazz-exec    (Polars + tokio + rayon isolated)
//
// Communication protocol:
//   - xazz CLI → xazz-runner : passes CLI args through
//   - xazz-runner → xazz-exec : passes args through as-is, inherits stdout/stderr
//   - exit code: propagates the xazz-exec exit code as-is

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default maximum execution time (seconds) for the execution engine (xazz-exec).
/// Overridable via the `XAZZ_EXEC_TIMEOUT_SECS` environment variable (values ≤ 0 ignored).
const DEFAULT_EXEC_TIMEOUT_SECS: u64 = 300;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // ── helper flags: --version / --help / --check-engine ───────────────────
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("xazz-runner {}", VERSION);
        return;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return;
    }

    // Diagnose execution-engine presence/availability (check only, no changes)
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

    // ── resolve the xazz-exec binary path ─────────────────────────────────────
    let exec_path = match resolve_exec_binary() {
        Ok(p) => p,
        Err(msg) => {
            eprintln!("[xazz-runner] ERROR: {}", msg);
            std::process::exit(1);
        }
    };

    // ── spawn the xazz-exec subprocess ───────────────────────────────────────
    // stdin/stdout/stderr inherited → transparent IPC relay
    // The execution time limit (timeout) is lightweight hardening against DoS.
    // (Note: this is process isolation, not an OS sandbox — seccomp/landlock
    //  are a separate milestone)
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
                eprintln!("[xazz-runner] ERROR: failed to check execution engine status: {e}");
                std::process::exit(1);
            }
        }
    };

    // propagate the xazz-exec exit code as-is
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

/// Diagnoses the existence and runnability of the xazz-exec execution engine.
///
/// Exit code: 0 = ok, 1 = engine missing/not runnable
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

    // runnability is verified by actually invoking --version
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

/// Resolves the xazz-exec binary path.
///
/// Priority:
/// 1. `XAZZ_EXEC_PATH` environment variable (deployment hardening)
/// 2. Same directory as the current executable (xazz-runner)
///
/// No PATH fallback is performed (prevents arbitrary code execution via PATH shadowing).
/// Returns `Err` if not found (fail-closed).
fn resolve_exec_binary() -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    let exec_name = "xazz-exec.exe";
    #[cfg(not(target_os = "windows"))]
    let exec_name = "xazz-exec";

    // 1. Pin the path via env var (deployment hardening)
    if let Ok(pinned) = std::env::var("XAZZ_EXEC_PATH") {
        if !pinned.trim().is_empty() {
            return Ok(PathBuf::from(pinned));
        }
    }

    // 2. Look next to the current executable
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
