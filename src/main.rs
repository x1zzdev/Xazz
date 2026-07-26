mod cli;
mod predict;
mod project;
mod schema;
mod ux;
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

        // ── check: sLM 정적 분석 (NQP) ──────────────────────────────────────
        Commands::Check { file } => {
            println!("[Experimental] xazz check — Neural Query Planner");
            println!("  이 기능은 현재 실험적 상태입니다. 출력은 시범용 결과입니다.");
            println!();
            println!("정적 분석을 시작합니다 …  ({})", file.display());

            let spinner = ux::create_spinner("sLM Neural Query Planner 분석 중 …");
            // tokio::time::sleep 제거 → std::thread::sleep 사용
            std::thread::sleep(std::time::Duration::from_millis(1_200));
            spinner.finish_and_clear();

            let file_name = file
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");
            ux::print_mock_nqp_report(file_name);
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
