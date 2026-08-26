mod cli;
mod predict;
mod project;
mod schema;
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
            predict,
            output,
            json,
        } => {
            let source_path = match file.to_str() {
                Some(p) => p.to_owned(),
                None => {
                    eprintln!(
                        "IO 에러: 파일 경로를 UTF-8 문자열로 변환할 수 없습니다.\n\
                         경로에 유효하지 않은 문자가 포함되어 있는지 확인하세요."
                    );
                    std::process::exit(1);
                }
            };

            if !file.exists() {
                eprintln!(
                    "[xazz IO 에러]\n\
                     ─────────────────────────────────────────────\n\
                     Cause   : 소스 파일을 찾을 수 없습니다.\n\
                     Detail  : '{}' 경로에 파일이 존재하지 않습니다.\n\
                     → 경로를 다시 확인하거나 .xzz 파일을 먼저 생성하세요.",
                    source_path
                );
                std::process::exit(1);
            }

            // ── --predict 분기: NQP 시맨틱 예측 모드 ───────────────────────
            // predict는 Polars를 사용하지 않으므로 CLI에서 직접 처리한다.
            if predict {
                if let Err(e) = predict::run_predict(&source_path) {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
                return Ok(());
            }

            if release {
                println!("🚀  릴리즈 모드 (Polars 최적화 플래그 활성화)");
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
                        "xazz-runner 실행 실패: {}\n\
                         → 'xazz-runner' 바이너리가 PATH 또는 xazz 실행 파일과 같은 디렉토리에 있는지 확인하세요.",
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
                    "xazz-runner 실행 실패: {}\n\
                     → 'xazz-runner' 바이너리가 PATH 또는 xazz 실행 파일과 같은 디렉토리에 있는지 확인하세요.",
                    e
                )
            })?;

            if !status.success() {
                std::process::exit(status.code().unwrap_or(1));
            }
        }

        // ── emit: .xzz → 타겟 언어 변환 출력 ──────────────────────────────
        // emit은 Polars 없이 컴파일러만 사용하므로 CLI에서 직접 처리한다.
        Commands::Emit { format, file, out } => {
            let source_path = match file.to_str() {
                Some(p) => p.to_owned(),
                None => {
                    eprintln!("IO 에러: 파일 경로를 UTF-8 문자열로 변환할 수 없습니다.");
                    std::process::exit(1);
                }
            };

            if !file.exists() {
                eprintln!(
                    "[xazz IO 에러]\n\
                     ─────────────────────────────────────────────\n\
                     Cause   : 소스 파일을 찾을 수 없습니다.\n\
                     Detail  : '{}' 경로에 파일이 존재하지 않습니다.\n\
                     → 경로를 다시 확인하거나 .xzz 파일을 먼저 생성하세요.",
                    source_path
                );
                std::process::exit(1);
            }

            match format.to_lowercase().as_str() {
                "rust" => {
                    let out_path = out.as_ref().and_then(|p| p.to_str()).map(String::from);

                    println!(
                        "⚙  xazz emit rust  │  소스: {}  │  출력: {}",
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
                        "[xazz emit 에러]\n\
                         ─────────────────────────────────────────────\n\
                         Cause   : 지원하지 않는 출력 형식입니다.\n\
                         Detail  : '{}' 는 유효한 emit 형식이 아닙니다.\n\
                         Available: rust\n\
                         → Did you mean: xazz emit rust {}",
                        unknown, source_path
                    );
                    std::process::exit(1);
                }
            }
        }

        // ── check: 정적 의미 분석 (Type Checker) ────────────────────────────
        Commands::Check { file } => {
            let source = match std::fs::read_to_string(&file) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("IO 에러: 파일 읽기 실패 '{}' — {}", file.display(), e);
                    std::process::exit(1);
                }
            };

            let (parse_result, result) = xazz_compiler::check_source(&source);

            // 파싱 단계 에러가 있으면 그대로 출력 후 실패 종료
            if let Err(e) = &parse_result {
                eprintln!("{}", e);
                std::process::exit(1);
            }

            println!("═══ xazz check: 정적 의미 분석 ═══");
            println!("파일     : {}", file.display());
            println!("오류     : {}건", result.errors.len());
            println!("경고     : {}건", result.warnings.len());
            println!();

            for err in &result.errors {
                let loc = if err.span.line > 0 {
                    format!(" [{}행:{}열]", err.span.line, err.span.col)
                } else {
                    String::new()
                };
                println!("❌ [오류{}] {}", loc, err.message);
                if let Some(s) = &err.ai_suggestion {
                    println!("   💡 {}", s);
                }
                println!();
            }
            for warn in &result.warnings {
                let loc = if warn.span.line > 0 {
                    format!(" [{}행:{}열]", warn.span.line, warn.span.col)
                } else {
                    String::new()
                };
                println!("⚠️  [경고{}] {}", loc, warn.message);
            }

            if result.is_err() {
                println!();
                eprintln!(
                    "[xazz check] 정적 분석에서 {}건의 오류를 발견했습니다.",
                    result.errors.len()
                );
                std::process::exit(1);
            }
            println!();
            println!("✅ 정적 분석 통과 — 실행 전 결함 없음");
        }

        // ── sde: 합성 데이터 생성 ────────────────────────────────────────────
        Commands::Sde { rows, output } => {
            println!("[Preview] xazz sde — Synthetic Data Engine");
            println!("  이 기능은 현재 Preview 상태입니다. CLI 통합이 진행 중입니다.");
            println!();
            println!("  rows: {}  │  output: {}", rows, output.display());
            println!("  xazz-sde 엔진 연동 예정.");
        }

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
//   1. 현재 xazz 실행 파일과 같은 디렉토리
//   2. PATH에서 찾기 (OS가 Command::new에서 자동 처리)
fn find_runner() -> Result<std::path::PathBuf, String> {
    // 1. 현재 실행 파일 옆에서 탐색
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

    // 2. PATH에서 탐색 (OS 위임)
    Ok(std::path::PathBuf::from("xazz-runner"))
}
