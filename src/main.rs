// `CompileError` is intentionally kept unboxed (see xazz-core/src/error.rs).
#![allow(clippy::result_large_err)]

mod cli;
mod http;
mod policy_cli;
mod project;
mod registry;
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
            if let Some(ref out) = output
                && let Some(out_str) = out.to_str()
            {
                cmd.arg("--output").arg(out_str);
            }
            if opt {
                cmd.arg("--opt");
            }

            // ── --json: print structured JSON execution result ──────────────────────
            // Capture xazz-runner's stdout, parse the [xazz:result] / [xazz:diagnostics] /
            // [xazz:train] / [xazz:dp] markers, and reassemble into a single JSON object.
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

                let markers = parse_run_markers(&stdout);

                let exit_code = output.status.code().unwrap_or(1);
                let summary = serde_json::json!({
                    "success": success,
                    "exit_code": exit_code,
                    "source": source_path,
                    "rows": markers.rows,
                    "schema": markers.schema,
                    "diagnostics": markers.diagnostics,
                    "training": markers.training,
                    "dp": markers.dp,
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

            let (parse_result, result) = {
                // Parse, then resolve `import "path.xzz"` relative to the file's dir (issue #69).
                use xazz_compiler::{Lexer, Parser};
                let parsed = Lexer::new(&source).tokenize().and_then(|tokens| {
                    Parser::new(tokens.clone())
                        .parse()
                        .map(|program| (tokens, program))
                });
                match parsed {
                    Err(e) => (Err(e), xazz_compiler::CheckResult::default()),
                    Ok((tokens, program)) => {
                        let src_dir = std::path::Path::new(&file)
                            .parent()
                            .map(|p| {
                                if p.as_os_str().is_empty() {
                                    std::path::PathBuf::from(".")
                                } else {
                                    p.to_path_buf()
                                }
                            })
                            .unwrap_or_else(|| std::path::PathBuf::from("."));
                        match xazz_compiler::modules::resolve_imports(&program, &src_dir) {
                            Err(module_errs) => {
                                let combined = module_errs.join("\n");
                                (
                                    Err(xazz_compiler::CompileError::new(
                                        xazz_compiler::ErrorKind::Other(format!(
                                            "module resolution failed: {combined}"
                                        )),
                                        xazz_compiler::Span::new(1, 1),
                                        combined,
                                    )),
                                    xazz_compiler::CheckResult::default(),
                                )
                            }
                            Ok(resolved) => {
                                let (check, _ir) = if resolved.modules.is_empty() {
                                    xazz_compiler::checker::analyze_program_with_tokens(
                                        &resolved.program,
                                        &tokens,
                                    )
                                } else {
                                    xazz_compiler::analyze_program(&resolved.program)
                                };
                                (Ok(resolved.program), check)
                            }
                        }
                    }
                }
            };

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

        // ── import: auto-generate schema → xazz type definition + load statement ──
        Commands::Import { file } => {
            if let Err(e) = schema::import_file(&file) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }

        // ── sanitize: fine-tuning data sanitization (issue #72, F3) ───────────
        // Delegates to xazz-exec --sanitize via the runner IPC bridge, exactly
        // like columnar schema inference (`xazz import *.parquet`).
        Commands::Sanitize { file, json } => {
            let runner = crate::find_runner().map_err(|e| anyhow::anyhow!(e))?;
            let mut cmd = std::process::Command::new(&runner);
            cmd.arg("--sanitize").arg(&file);
            if json {
                cmd.arg("--json");
            }
            let status = cmd
                .status()
                .map_err(|e| anyhow::anyhow!("xazz-exec --sanitize '{}' 실행 실패: {}", file, e))?;
            if !status.success() {
                std::process::exit(status.code().unwrap_or(1));
            }
        }

        // ── registry: policy packs + stdlib modules (issue #68, E4) ───────────────
        Commands::Registry { action } => {
            let code = match action {
                cli::RegistryAction::List => registry::list(),
                cli::RegistryAction::Show { name } => registry::show(&name),
                cli::RegistryAction::Install { name, out, force } => {
                    registry::install(&name, out.as_deref(), force)
                }
                cli::RegistryAction::Deploy {
                    name,
                    server,
                    tenant,
                    token,
                    actor,
                } => registry::deploy(&name, &server, &tenant, token.as_deref(), actor.as_deref()),
            };
            if code != 0 {
                std::process::exit(code);
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
pub(crate) fn find_runner() -> Result<std::path::PathBuf, String> {
    // 1. Pin the path via environment variable (deployment hardening)
    if let Ok(pinned) = std::env::var("XAZZ_RUNNER_PATH")
        && !pinned.trim().is_empty()
    {
        return Ok(std::path::PathBuf::from(pinned));
    }

    // 2. Search next to the current executable
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        #[cfg(windows)]
        let candidate = dir.join("xazz-runner.exe");
        #[cfg(not(windows))]
        let candidate = dir.join("xazz-runner");

        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(
        "xazz-runner not found (PATH fallback is disabled for security). \
         Set XAZZ_RUNNER_PATH to an absolute path or place xazz-runner next to the xazz binary."
            .to_string(),
    )
}

// ── `xazz run --json`: stdout marker extraction ───────────────────────────────
//
// The execution engine reports structured results as single-line stdout markers
// (`[xazz:<kind>] <JSON>`). The CLI never links Polars, so it only reassembles
// those markers; the shapes are owned by xazz-exec (see `runtime.rs`).

/// Markers collected from one `xazz-runner` run.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct RunMarkers {
    /// `[xazz:result]` → `rows` (the last one wins, as in the server).
    pub rows: serde_json::Value,
    /// `[xazz:result]` → `schema`.
    pub schema: serde_json::Value,
    /// `[xazz:diagnostics]` — type-checker diagnostics.
    pub diagnostics: Option<serde_json::Value>,
    /// `[xazz:train]` — Burn training report (last `train` in the script).
    pub training: Option<serde_json::Value>,
    /// `[xazz:dp]` — one entry per `withDp` step, in execution order (issue #117).
    /// Each carries the DpReport plus the session budget after that step
    /// (`budget_spent`, `budget_total`, `budget_remaining`, the `_delta` twins, `query_count`).
    pub dp: Vec<serde_json::Value>,
}

/// Parses the `[xazz:result]`, `[xazz:diagnostics]`, `[xazz:train]` and `[xazz:dp]`
/// markers out of the runner's stdout.
///
/// `[xazz:dp]` accepts both the current single-line form and the legacy form where
/// the JSON follows on the next line. Markers emitted by an older engine without
/// `budget_remaining*` get those fields derived from `budget_total − budget_spent`,
/// so consumers can rely on them either way.
pub(crate) fn parse_run_markers(stdout: &str) -> RunMarkers {
    let mut markers = RunMarkers {
        rows: serde_json::Value::Array(vec![]),
        schema: serde_json::Value::Array(vec![]),
        ..RunMarkers::default()
    };

    let lines: Vec<&str> = stdout.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if let Some(json_part) = trimmed.strip_prefix("[xazz:result] ")
            && let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_part)
        {
            if let Some(r) = parsed.get("rows") {
                markers.rows = r.clone();
            }
            if let Some(s) = parsed.get("schema") {
                markers.schema = s.clone();
            }
        }
        if let Some(json_part) = trimmed.strip_prefix("[xazz:diagnostics] ")
            && let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_part)
        {
            markers.diagnostics = Some(parsed);
        }
        if let Some(json_part) = trimmed.strip_prefix("[xazz:train] ")
            && let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_part)
        {
            markers.training = Some(parsed);
        }
        if let Some(json_part) = trimmed.strip_prefix("[xazz:dp] ") {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_part) {
                markers.dp.push(with_dp_remaining(parsed));
            }
        } else if trimmed == "[xazz:dp]" {
            // Legacy two-line form: JSON on the following line.
            let next = lines.get(i + 1).map(|l| l.trim()).unwrap_or("");
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(next) {
                markers.dp.push(with_dp_remaining(parsed));
                i += 1;
            }
        }
        i += 1;
    }

    markers
}

/// Backfills `budget_remaining` / `budget_remaining_delta` on a `[xazz:dp]` payload
/// that predates those fields. Leaves payloads that already carry them untouched.
fn with_dp_remaining(mut dp: serde_json::Value) -> serde_json::Value {
    if let Some(obj) = dp.as_object_mut() {
        for (remaining, total, spent) in [
            ("budget_remaining", "budget_total", "budget_spent"),
            (
                "budget_remaining_delta",
                "budget_total_delta",
                "budget_spent_delta",
            ),
        ] {
            if obj.contains_key(remaining) {
                continue;
            }
            if let (Some(t), Some(s)) = (
                obj.get(total).and_then(|v| v.as_f64()),
                obj.get(spent).and_then(|v| v.as_f64()),
            ) {
                obj.insert(remaining.into(), serde_json::json!((t - s).max(0.0)));
            }
        }
    }
    dp
}

#[cfg(test)]
mod run_marker_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dp_markers_are_collected_in_order_with_budget() {
        let stdout = "\
[xazz] Pipeline #1 'station_counts' done: 25 × 2
[xazz:dp] {\"mechanism\":\"laplace\",\"epsilon\":1.0,\"delta\":null,\"noised_columns\":[\"pm10\"],\"budget_spent\":1.0,\"budget_total\":10.0,\"budget_spent_delta\":0.0,\"budget_total_delta\":0.0001,\"query_count\":1,\"budget_remaining\":9.0,\"budget_remaining_delta\":0.0001}
[xazz:dp] {\"mechanism\":\"gaussian\",\"epsilon\":0.5,\"delta\":1e-5,\"noised_columns\":[\"pm10\"],\"budget_spent\":1.5,\"budget_total\":10.0,\"budget_spent_delta\":1e-5,\"budget_total_delta\":0.0001,\"query_count\":2,\"budget_remaining\":8.5,\"budget_remaining_delta\":9e-5}
[xazz:result] {\"rows\":[{\"station\":\"강남구\",\"pm10\":21.3}],\"schema\":[{\"name\":\"station\",\"type\":\"str\"}]}
";
        let m = parse_run_markers(stdout);
        assert_eq!(m.dp.len(), 2);
        assert_eq!(m.dp[0]["mechanism"], "laplace");
        assert_eq!(m.dp[0]["budget_remaining"], 9.0);
        assert_eq!(m.dp[1]["mechanism"], "gaussian");
        assert_eq!(m.dp[1]["budget_spent"], 1.5);
        assert_eq!(m.dp[1]["query_count"], 2);
        // Engine-provided remaining values are passed through, not recomputed.
        assert_eq!(m.dp[1]["budget_remaining"], 8.5);
        assert_eq!(m.rows[0]["station"], "강남구");
        assert_eq!(m.schema[0]["name"], "station");
    }

    #[test]
    fn dp_remaining_is_derived_for_engines_without_the_field() {
        let stdout = "[xazz:dp] {\"mechanism\":\"laplace\",\"epsilon\":2.0,\"budget_spent\":2.0,\"budget_total\":10.0,\"budget_spent_delta\":0.0,\"budget_total_delta\":0.0}\n";
        let m = parse_run_markers(stdout);
        assert_eq!(m.dp.len(), 1);
        assert_eq!(m.dp[0]["budget_remaining"], 8.0);
        assert_eq!(m.dp[0]["budget_remaining_delta"], 0.0);
    }

    #[test]
    fn dp_remaining_floors_at_zero_when_overspent_marker_is_replayed() {
        let stdout = "[xazz:dp] {\"budget_spent\":11.0,\"budget_total\":10.0}\n";
        let m = parse_run_markers(stdout);
        assert_eq!(m.dp[0]["budget_remaining"], 0.0);
        // No δ totals in the payload → no δ remainder is invented.
        assert!(m.dp[0].get("budget_remaining_delta").is_none());
    }

    #[test]
    fn legacy_two_line_dp_marker_is_still_parsed() {
        let stdout = "[xazz:dp]\n{\"mechanism\":\"laplace\",\"budget_spent\":1.0,\"budget_total\":4.0}\n[xazz:result] {\"rows\":[],\"schema\":[]}\n";
        let m = parse_run_markers(stdout);
        assert_eq!(m.dp.len(), 1);
        assert_eq!(m.dp[0]["budget_remaining"], 3.0);
        assert_eq!(m.rows, json!([]));
    }

    #[test]
    fn training_and_diagnostics_markers_are_captured() {
        let stdout = "\
[xazz:diagnostics] {\"error_count\":0,\"warning_count\":1,\"errors\":[],\"warnings\":[]}
[xazz:train] {\"model_name\":\"AirPredictor\",\"report\":{\"epochs\":10,\"final_train_loss\":1513.07},\"success\":true,\"type\":\"train_stmt\"}
";
        let m = parse_run_markers(stdout);
        assert_eq!(m.diagnostics.as_ref().unwrap()["warning_count"], 1);
        assert_eq!(m.training.as_ref().unwrap()["model_name"], "AirPredictor");
        assert!(m.dp.is_empty());
    }

    #[test]
    fn malformed_markers_are_ignored_without_panicking() {
        let stdout = "[xazz:dp] {not json\n[xazz:dp]\nalso not json\n[xazz:result] nope\n";
        let m = parse_run_markers(stdout);
        assert!(m.dp.is_empty());
        assert_eq!(m.rows, json!([]));
        assert!(m.training.is_none());
    }
}
