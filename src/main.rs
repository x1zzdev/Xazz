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
        // ── run: execute .xzz data analysis code ────────────────────────────────
        //
        // ⚠️  Architecture principle (binary size minimization):
        //   The CLI binary does not link Polars/Tokio.
        //   The run command spawns the xazz-runner subprocess to delegate execution.
        //   Communication: CLI args only (no separate IPC needed)
        Commands::Run {
            file,
            release,
            verbose,
            output,
            json,
            opt,
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

            // ── security guardrail (issue #2) ──────────────────────────────
            // Must pass the Policy-as-Code static check before spawning the subprocess.
            // If there are violations, exit here. (The execution engine has the same gate)
            policy_cli::gate_before_run(&source_path, json);

            if release {
                println!("🚀  release mode (Polars optimization flags enabled)");
                println!();
            }

            // ── spawn the xazz-runner subprocess ────────────────────────────────
            // Polars/Tokio are only linked into the xazz-runner binary,
            // so they do not affect this CLI binary's size.
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
            if opt {
                cmd.arg("--opt");
            }

            // ── --json: print structured JSON execution result ──────────────────────
            // Capture xazz-runner's stdout, parse the [xazz:result] / [xazz:diagnostics]
            // markers, and reassemble into a single JSON object.
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

        // ── policy: Policy-as-Code security guardrail check (issue #2) ──────────
        // Uses only static analysis, so it is handled directly in the CLI without Polars.
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

        // ── emit: convert .xzz → target language ────────────────────────────────
        // emit uses only the compiler without Polars, so it is handled directly in the CLI.
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

        // ── check: static semantic analysis (type checker) ──────────────────────
        Commands::Check { file, json } => {
            let source = match std::fs::read_to_string(&file) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("IO error: failed to read file '{}' — {}", file.display(), e);
                    std::process::exit(1);
                }
            };

            let (parse_result, result) = xazz_compiler::check_source(&source);

            // If there are parsing errors, print them and exit with failure
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

            // ── --json: print structured diagnostics result ──────────────────────────
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

        // ── sde: generate synthetic data ──────────────────────────────────────────
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

        // ── new: create a new project ─────────────────────────────────────────────
        Commands::New { name } => {
            if let Err(e) = project::create_project(&name) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }

        // ── import: auto-generate CSV → xazz type definition + load statement ─────
        Commands::Import { file } => {
            if let Err(e) = schema::import_csv(&file) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }

        // ── whoami: print user identity ──────────────────────────────────────────
        Commands::Whoami => {
            whoami::run_whoami()?;
        }
    }

    Ok(())
}

// ── locate the xazz-runner binary ──────────────────────────────────────────────
//
// Search order:
//   1. XAZZ_RUNNER_PATH environment variable (deployment hardening)
//   2. Same directory as the current xazz executable
// No PATH fallback (prevents arbitrary code execution via PATH shadowing, fail-closed)
fn find_runner() -> Result<std::path::PathBuf, String> {
    // 1. Pin the path via environment variable (deployment hardening)
    if let Ok(pinned) = std::env::var("XAZZ_RUNNER_PATH") {
        if !pinned.trim().is_empty() {
            return Ok(std::path::PathBuf::from(pinned));
        }
    }

    // 2. Search next to the current executable
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
