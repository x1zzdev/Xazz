mod cli;
mod policy_cli;
mod project;
mod schema;
mod sde;
mod whoami;

use clap::Parser;
use cli::{Cli, Commands};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        // ── run: .xzz 데이터 분석 코드 실행 ──────────────────────────────────
        //
        // ⚠️  아키텍처 원칙 (바이너리 크기 최소화):
        //   CLI 바이너리는 Polars/Tokio를 링크하지 않는다.
        //   run 명령어는 xazz-runner 서브프로세스를 스폰해 실행을 위임한다.
        //   통신: CLI args만 사용 (별도 IPC 불필요)
        Commands::Run {
            file,
            release,
            verbose,
            output,
            json,
        } => {
            let source_path = match file.to_str() {
                Some(p) => p.to_owned(),
                None => {
                    eprintln!(
                        "IO error: file path is not valid UTF-8.\n\
                         Check that the path does not contain invalid characters."
                    );
                    std::process::exit(1);
                }
            };

            if !file.exists() {
                eprintln!(
                    "[xazz IO error]\n\
                     ─────────────────────────────────────────────\n\
                     Cause   : source file not found.\n\
                     Detail  : no file exists at '{}'.\n\
                     → check the path or create the .xzz file first.",
                    source_path
                );
                std::process::exit(1);
            }

            // ── 보안 가드레일 (issue #2) ──────────────────────────────────
            // 서브프로세스를 띄우기 전에 Policy-as-Code 정적 검사를 통과해야 한다.
            // 위반이 있으면 여기서 종료한다. (실행 엔진에도 동일한 게이트가 있다)
            policy_cli::gate_before_run(&source_path, json);

            if release {
                println!("🚀  release mode (Polars optimization flags enabled)");
                println!();
            }

            // ── xazz-runner 서브프로세스 스폰 ────────────────────────────────
            // Polars/Tokio는 xazz-runner 바이너리에만 링크되며,
            // 이 CLI 바이너리의 크기에 영향을 주지 않는다.
            let runner = find_runner()?;
            let mut cmd = std::process::Command::new(&runner);
            cmd.arg(&source_path);
            if verbose {
                cmd.arg("--verbose");
            }
            if let Some(ref out) = output {
                if let Some(out_str) = out.to_str() {
                    cmd.arg("--output").arg(out_str);
                }
            }

            // ── --json: 구조화된 JSON 실행 결과 출력 ────────────────────────
            // xazz-runner 의 stdout 을 캡처해 [xazz:result] / [xazz:diagnostics]
            // 마커를 파싱한 뒤 단일 JSON 객체로 재조립한다.
            if json {
                let output = cmd.output().map_err(|e| {
                    format!(
                        "failed to run xazz-runner: {}\n\
                         → check that the xazz-runner binary is on PATH or in the same directory as the xazz executable.",
                        e
                    )
                })?;
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let success = output.status.success();

                let mut rows = serde_json::Value::Array(vec![]);
                let mut schema = serde_json::Value::Array(vec![]);
                let mut diagnostics: Option<serde_json::Value> = None;

                for line in stdout.lines() {
                    let trimmed = line.trim();
                    if let Some(json_part) = trimmed.strip_prefix("[xazz:result] ") {
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_part) {
                            if let Some(r) = parsed.get("rows") {
                                rows = r.clone();
                            }
                            if let Some(s) = parsed.get("schema") {
                                schema = s.clone();
                            }
                        }
                    }
                    if let Some(json_part) = trimmed.strip_prefix("[xazz:diagnostics] ") {
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_part) {
                            diagnostics = Some(parsed);
                        }
                    }
                }

                let exit_code = output.status.code().unwrap_or(1);
                let summary = serde_json::json!({
                    "success": success,
                    "exit_code": exit_code,
                    "source": source_path,
                    "rows": rows,
                    "schema": schema,
                    "diagnostics": diagnostics,
                    "error": if success { None } else { Some(stderr.trim()) },
                    "logs": stderr.lines().map(|l| l.to_string()).collect::<Vec<_>>(),
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&summary).unwrap_or_default()
                );
                if !success {
                    std::process::exit(exit_code);
                }
                return Ok(());
            }

            let status = cmd.status().map_err(|e| {
                format!(
                    "failed to run xazz-runner: {}\n\
                     → check that the xazz-runner binary is on PATH or in the same directory as the xazz executable.",
                    e
                )
            })?;

            if !status.success() {
                std::process::exit(status.code().unwrap_or(1));
            }
        }

        // ── policy: Policy-as-Code 보안 가드레일 검사 (issue #2) ────────────
        // 정적 분석만 사용하므로 Polars 없이 CLI에서 직접 처리한다.
        Commands::Policy {
            file,
            json,
            fix,
            out,
        } => {
            if !file.exists() {
                eprintln!(
                    "[xazz IO error]\n\
                     ─────────────────────────────────────────────\n\
                     Cause   : source file not found.\n\
                     Detail  : no file exists at '{}'.",
                    file.display()
                );
                std::process::exit(1);
            }
            let code = policy_cli::run_policy_command(&file, json, fix, out.as_deref());
            if code != 0 {
                std::process::exit(code);
            }
        }

        // ── emit: .xzz → 타겟 언어 변환 출력 ──────────────────────────────
        // emit은 Polars 없이 컴파일러만 사용하므로 CLI에서 직접 처리한다.
        Commands::Emit { format, file, out } => {
            let source_path = match file.to_str() {
                Some(p) => p.to_owned(),
                None => {
                    eprintln!("IO error: file path is not valid UTF-8.");
                    std::process::exit(1);
                }
            };

            if !file.exists() {
                eprintln!(
                    "[xazz IO error]\n\
                     ─────────────────────────────────────────────\n\
                     Cause   : source file not found.\n\
                     Detail  : no file exists at '{}'.\n\
                     → check the path or create the .xzz file first.",
                    source_path
                );
                std::process::exit(1);
            }

            match format.to_lowercase().as_str() {
                "rust" => {
                    let out_path = out.as_ref().and_then(|p| p.to_str()).map(String::from);

                    println!(
                        "⚙  xazz emit rust  │  source: {}  │  output: {}",
                        source_path,
                        out_path.as_deref().unwrap_or("stdout")
                    );
                    println!();

                    if let Err(e) =
                        xazz_compiler::emitter::emit_rust(&source_path, out_path.as_deref())
                    {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                }
                unknown => {
                    eprintln!(
                        "[xazz emit error]\n\
                         ─────────────────────────────────────────────\n\
                         Cause   : unsupported output format.\n\
                         Detail  : '{}' is not a valid emit format.\n\
                         Available: rust\n\
                         → Did you mean: xazz emit rust {}",
                        unknown, source_path
                    );
                    std::process::exit(1);
                }
            }
        }

        // ── check: 정적 의미 분석 (Type Checker) ────────────────────────────
        Commands::Check { file, json } => {
            let source = match std::fs::read_to_string(&file) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("IO error: failed to read file '{}' — {}", file.display(), e);
                    std::process::exit(1);
                }
            };

            let (parse_result, result) = xazz_compiler::check_source(&source);

            // 파싱 단계 에러가 있으면 그대로 출력 후 실패 종료
            if let Err(e) = &parse_result {
                if json {
                    let out = serde_json::json!({
                        "success": false,
                        "parse_error": e.to_string(),
                        "errors": [],
                        "warnings": [],
                    });
                    println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
                } else {
                    eprintln!("{}", e);
                }
                std::process::exit(1);
            }

            // ── --json: 구조화된 진단 결과 출력 ─────────────────────────────
            if json {
                let diag = |e: &xazz_compiler::CompileError| {
                    serde_json::json!({
                        "line": if e.span.line > 0 { e.span.line } else { 0 },
                        "col": if e.span.line > 0 { e.span.col } else { 0 },
                        "category": e.kind.category(),
                        "message": e.message,
                        "suggestion": e.ai_suggestion,
                    })
                };
                let errors: Vec<_> = result.errors.iter().map(&diag).collect();
                let warnings: Vec<_> = result.warnings.iter().map(&diag).collect();
                let out = serde_json::json!({
                    "success": result.is_ok(),
                    "source": file.display().to_string(),
                    "error_count": errors.len(),
                    "warning_count": warnings.len(),
                    "errors": errors,
                    "warnings": warnings,
                });
                println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
                if result.is_err() {
                    std::process::exit(1);
                }
                return Ok(());
            }

            println!("═══ xazz check: static semantic analysis ═══");
            println!("file      : {}", file.display());
            println!("errors    : {}", result.errors.len());
            println!("warnings  : {}", result.warnings.len());
            println!();

            for err in &result.errors {
                let loc = if err.span.line > 0 {
                    format!(" [line {}: col {}]", err.span.line, err.span.col)
                } else {
                    String::new()
                };
                println!("❌ [error{}] {}", loc, err.message);
                if let Some(s) = &err.ai_suggestion {
                    println!("   💡 {}", s);
                }
                println!();
            }
            for warn in &result.warnings {
                let loc = if warn.span.line > 0 {
                    format!(" [line {}: col {}]", warn.span.line, warn.span.col)
                } else {
                    String::new()
                };
                println!("⚠️  [warning{}] {}", loc, warn.message);
            }

            if result.is_err() {
                println!();
                eprintln!(
                    "[xazz check] found {} error(s) in the static analysis.",
                    result.errors.len()
                );
                std::process::exit(1);
            }
            println!();
            println!("✅ static analysis passed — no defects before execution");
        }

        // ── sde: 합성 데이터 생성 ────────────────────────────────────────────
        Commands::Sde { rows, output } => match sde::generate(rows, &output) {
            Ok(()) => {
                println!(
                    "✅ synthetic data generated: {} rows → {}",
                    rows,
                    output.display()
                );
            }
            Err(e) => {
                eprintln!("[xazz sde] error: {}", e);
                std::process::exit(1);
            }
        },

        // ── new: 새 프로젝트 생성 ─────────────────────────────────────────────
        Commands::New { name } => {
            if let Err(e) = project::create_project(&name) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }

        // ── import: CSV → xazz 타입 정의 + load 문 자동 생성 ─────────────────
        Commands::Import { file } => {
            if let Err(e) = schema::import_csv(&file) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }

        // ── whoami: 사용자 아이덴티티 출력 ──────────────────────────────────
        Commands::Whoami => {
            whoami::run_whoami()?;
        }
    }

    Ok(())
}

// ── xazz-runner 바이너리 탐색 ────────────────────────────────────────────────
//
// 탐색 순서:
//   1. XAZZ_RUNNER_PATH 환경변수 (배포 하드닝)
//   2. 현재 xazz 실행 파일과 같은 디렉토리
// PATH 폴백은 수행하지 않는다 (PATH 셰도잉으로 임의 코드 실행 방지, fail-closed)
fn find_runner() -> Result<std::path::PathBuf, String> {
    // 1. 환경변수로 경로 고정 (배포 하드닝)
    if let Ok(pinned) = std::env::var("XAZZ_RUNNER_PATH") {
        if !pinned.trim().is_empty() {
            return Ok(std::path::PathBuf::from(pinned));
        }
    }

    // 2. 현재 실행 파일 옆에서 탐색
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            #[cfg(windows)]
            let candidate = dir.join("xazz-runner.exe");
            #[cfg(not(windows))]
            let candidate = dir.join("xazz-runner");

            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    Err(
        "xazz-runner not found (PATH fallback is disabled for security). \
         Set XAZZ_RUNNER_PATH to an absolute path or place xazz-runner next to the xazz binary."
            .to_string(),
    )
}
