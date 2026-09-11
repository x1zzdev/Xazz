/// xazz-exec/src/runtime.rs — runtime execution engine (v0.18)
///
/// Library module that runs the full compile pipeline for a .xzz source file.
///
/// ⚠️  This module exists only in the xazz-exec crate.
///     xazz-compiler has no Polars dependency, so this module is not there.
///     The CLI (xazz) does not link this module directly; it runs it
///     indirectly through the xazz-runner subprocess.
use std::collections::HashMap;
use std::fs;

use crate::chart::{build_chart_spec, df_to_json_array, write_chart_html};
use xazz_compiler::ast::{LayerKind, SaveFormat};
use xazz_compiler::ir::{ColType, MLOp, PipelineNode, Schema, SideOp, Source, Step as IrStep};
use xazz_compiler::{Lexer, Parser};
use xazz_core::i18n::{is_korean, tr};

/// Maximum number of rows to inspect for CSV schema inference.
const SCHEMA_INFERENCE_ROWS: usize = 200;

// ─────────────────────────────────────────────────────────────────────────────
// ── Top-level public entry point ──────────────────────────────────────────────
// ─────────────────────────────────────────────────────────────────────────────

/// Runs the full compile+runtime pipeline for a given .xzz source file path.
///
/// - `verbose`: if true, prints the Lexer token stream and AST to stdout.
/// - `output_csv`: if Some(path), saves the final DataFrame result to a CSV file.
pub fn run_pipeline(
    source_path: &str,
    verbose: bool,
    output_csv: Option<&str>,
    optimize: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // ── STEP 1: Read source file ────────────────────────────────────────────
    let source = fs::read_to_string(source_path).map_err(|e| {
        if is_korean() {
            format!("IO 에러: 파일 읽기 실패 '{}' — {}", source_path, e)
        } else {
            format!("IO error: failed to read file '{}' — {}", source_path, e)
        }
    })?;

    eprintln!(
        "[xazz] {}: {}  ({} bytes)",
        tr("input", "입력"),
        source_path,
        source.len()
    );

    // ── STEP 2: Lexer — tokenizing ─────────────────────────────────────────
    let mut lexer = Lexer::new(&source);
    let tokens = lexer
        .tokenize()
        .map_err(|e| format!("[xazz LEXER ERROR] {}", e))?;

    eprintln!(
        "[xazz] Lexer {}: {} {}",
        tr("complete", "완료"),
        tokens.len(),
        tr("tokens", "토큰")
    );

    if verbose {
        println!();
        println!("⚡ STEP 1. Tokenized Stream (Lexer)");
        println!("{}", "─".repeat(60));
        for token in &tokens {
            println!(
                "  [{:>4}:{:<3}] {:?}",
                token.span.line, token.span.col, token.kind
            );
        }
        println!();
    }

    // ── STEP 3: Parser — build AST ─────────────────────────────────────────
    let mut parser = Parser::new(tokens);
    let program = parser
        .parse()
        .map_err(|e| format!("[xazz PARSER ERROR] {}", e))?;

    // ── STEP 3.25: Module resolution — expand `import "path.xzz"` (issue #69) ──
    // Imported type/model/pipeline declarations are inlined into a merged AST;
    // module source texts are retained so the policy gate can scan them too.
    // Module paths resolve relative to the main file's directory.
    let source_dir = std::path::Path::new(source_path)
        .parent()
        .map(|p| {
            if p.as_os_str().is_empty() {
                std::path::PathBuf::from(".")
            } else {
                p.to_path_buf()
            }
        })
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let resolved =
        xazz_compiler::modules::resolve_imports(&program, &source_dir).map_err(|errs| {
            format!(
                "[xazz MODULE ERROR] {}\n{}",
                tr("module resolution failed", "모듈 해석 실패"),
                errs.join("\n")
            )
        })?;
    let program = resolved.program;

    eprintln!(
        "[xazz] Parser {}: {} AST {} ({} {})",
        tr("complete", "완료"),
        program.stmts.len(),
        tr("nodes", "노드"),
        resolved.modules.len(),
        tr("modules", "모듈")
    );

    // ── STEP 3.5: Static semantic analysis (Type Checker) + Typed IR generation — pre-execution defect detection ─
    // analyze_program produces diagnostics and IR in a **single pass**, eliminating double inference.
    let (check, mut ir) = xazz_compiler::analyze_program(&program);
    if !check.errors.is_empty() || !check.warnings.is_empty() {
        eprintln!(
            "[xazz] {}: {} {} / {} {}",
            tr("static analysis", "정적 분석"),
            check.errors.len(),
            tr("errors", "오류"),
            check.warnings.len(),
            tr("warnings", "경고")
        );
        for err in &check.errors {
            eprintln!("  [xazz DIAGNOSTIC ERROR] {}", err.message);
        }
        for warn in &check.warnings {
            eprintln!("  [xazz DIAGNOSTIC WARN]  {}", warn.message);
        }
        // [xazz:diagnostics] JSON marker — parseable by server/IDE
        let diag_json = serde_json::json!({
            "errors": check.errors.iter().map(|e| e.message.clone()).collect::<Vec<_>>(),
            "warnings": check.warnings.iter().map(|e| e.message.clone()).collect::<Vec<_>>(),
        });
        println!(
            "[xazz:diagnostics] {}",
            serde_json::to_string(&diag_json).unwrap_or_default()
        );
    }

    // ── STEP 3.55: Type-checker errors block execution (fail-closed) ───────
    // If semantic errors were passed through as advisory only, Polars would raise
    // more obscure panics on bad data, so execution stops on semantic errors. (Warnings remain non-blocking)
    if !check.errors.is_empty() {
        return Err(format!(
            "[xazz TYPECHECK ERROR] {} {} {} — {}. {}: {}",
            check.errors.len(),
            tr("static analysis errors", "정적 분석 오류"),
            tr("aborting execution", "건"),
            tr("execution stopped", "실행을 중단합니다"),
            tr("first error", "첫 번째 오류"),
            check.errors[0].message
        )
        .into());
    }

    // ── STEP 3.6: Policy-as-Code static guardrail — pre-execution security block (issue #2) ─
    //
    // This gate is the final checkpoint. The CLI (`xazz run`) and API server
    // (`POST /execute`) each run the same check at their front end, but this is
    // the only place that actually runs Polars, so every path must pass through here.
    //
    // If the policy cannot be loaded, execution is **refused** (fail-closed).
    let active = xazz_compiler::load_active_policy().map_err(|e| {
        let report = xazz_compiler::policy_load_failure_report(&e);
        emit_policy_marker(&report);
        format!("[xazz POLICY ERROR] {}", e)
    })?;

    let mut policy_report = xazz_compiler::check_policy_parsed(&program, &source, &active.policy);
    // Imported modules are scanned too (issue #69): their AST is already covered
    // by `check_policy_parsed` on the merged program, but literal/comment scanning
    // needs each module's own source text, so run the text gate per module.
    for (_mod_path, mod_src) in &resolved.modules {
        let module_report = xazz_compiler::check_policy(mod_src, &active.policy);
        if !module_report.safe_to_execute {
            policy_report.safe_to_execute = false;
        }
        policy_report.violations.extend(module_report.violations);
        policy_report.warnings.extend(module_report.warnings);
    }
    emit_policy_marker(&policy_report);

    eprintln!(
        "[xazz] {}: {} ({}) — {}",
        tr("static guardrail", "정적 가드레일"),
        active.policy.id,
        active.origin,
        policy_report.summary()
    );
    for warn in &policy_report.warnings {
        eprintln!("  [xazz POLICY WARN]  {} {}", warn.rule_id, warn.message);
    }

    if !policy_report.safe_to_execute {
        for v in &policy_report.violations {
            eprintln!("  [xazz POLICY BLOCK] {} {}", v.rule_id, v.message);
            eprintln!(
                "                      {}: {}",
                tr("remediation", "보정"),
                v.remediation_hint
            );
        }
        return Err(format!(
            "[xazz POLICY ERROR] {}\n{} `xazz policy <file> --fix` {}.",
            policy_report.summary(),
            tr("execution blocked", "실행이 차단되었습니다"),
            tr(
                "to review a safe alternative",
                "로 안전한 대체 코드를 확인하세요"
            )
        )
        .into());
    }

    if verbose {
        println!();
        println!("⚡ STEP 2. Abstract Syntax Tree (Parser)");
        println!("{}", "─".repeat(60));
        for (i, stmt) in program.stmts.iter().enumerate() {
            println!("  [{}] {:#?}", i, stmt);
        }
        println!();
    }

    // ── STEP 4: Codegen — generate Polars flow mapping string ─────────────────
    //
    // (With Typed IR, string codegen is no longer used in the execution path.
    //  Instead of interpreting the raw AST, we lower the IR. Only the `xazz emit` path keeps it separately.)

    // ── STEP 4.5: IR optimization (optional) — constant folding / Select merging / predicate pushdown ──
    if optimize {
        let before = ir.pipelines.iter().map(|p| p.steps.len()).sum::<usize>();
        ir = xazz_compiler::optimize_program(&ir);
        let after = ir.pipelines.iter().map(|p| p.steps.len()).sum::<usize>();
        eprintln!(
            "[xazz] IR {}: {} → {} ({} {})",
            tr("optimization applied", "최적화 적용"),
            before,
            after,
            before.saturating_sub(after),
            tr("steps reduced", "개 축소")
        );
    }

    // ── STEP 5: Runtime engine (consumes Typed IR) ─────────────────────────

    // Measures only the pipeline execution latency — excludes process boot, lexer, parser, checker.
    // Benchmarks can parse the [xazz:timing] marker to compare "pipeline execution time" alone.
    let timing_start = std::time::Instant::now();

    // 5-A: Build ModelRegistry — collect ModelDecls + log (usable regardless of declaration order)
    let mut model_registry: HashMap<String, Vec<LayerKind>> = HashMap::new();
    for m in &ir.models {
        model_registry.insert(m.name.clone(), m.layers.clone());
        handle_model_decl(&m.name, &m.layers);
    }

    // 5-B: Sequential pipeline execution + SymbolTable management
    let mut symbol_table: HashMap<String, polars::frame::DataFrame> = HashMap::new();
    let mut model_table: HashMap<String, crate::dl::TrainedModel> = HashMap::new();
    // Session privacy budget (ε-budget) — deducted per withDp call, rejects the pipeline when exceeded
    let mut dp_budget = crate::dp::PrivacyBudget::from_env();
    let mut pipeline_count = 0usize;
    let mut last_var_name: Option<String> = None;

    for node in &ir.pipelines {
        pipeline_count += 1;
        let name = node.name.as_deref().unwrap_or("<expr>");

        match execute_node(
            node,
            &symbol_table,
            &model_registry,
            &mut model_table,
            &mut dp_budget,
        ) {
            Ok(Some(df)) => {
                if let Some(vname) = &node.name {
                    eprintln!(
                        "[xazz] Pipeline #{} '{}' {}: {} × {}",
                        pipeline_count,
                        vname,
                        tr("done", "완료"),
                        df.height(),
                        df.width()
                    );
                    last_var_name = Some(vname.clone());
                    symbol_table.insert(vname.clone(), df);
                } else {
                    eprintln!(
                        "[xazz] Pipeline #{} (ExprStmt) {}: {} × {}",
                        pipeline_count,
                        tr("done", "완료"),
                        df.height(),
                        df.width()
                    );
                }
            }
            Ok(None) if node.yields_model => {
                eprintln!(
                    "[xazz] Pipeline #{} '{}' {}: {}",
                    pipeline_count,
                    name,
                    tr("done", "완료"),
                    tr("trained model created", "학습 모델 생성")
                );
                last_var_name = None;
            }
            Ok(None) => {
                eprintln!(
                    "[xazz] Pipeline #{} (TrainStmt) {}: {}",
                    pipeline_count,
                    tr("done", "완료"),
                    tr(
                        "trained model created (no binding)",
                        "학습 모델 생성 (바인딩 없음)"
                    )
                );
            }
            Err(e) => {
                eprintln!(
                    "[xazz RUNTIME ERROR] Pipeline #{} ('{}') {}: {}",
                    pipeline_count,
                    name,
                    tr("failed", "실패"),
                    e
                );
            }
        }
    }

    eprintln!(
        "[xazz] {} — {} {} / {} {} / {} {}",
        tr("done", "완료"),
        program.stmts.len(),
        tr("AST", "AST"),
        ir.types.len(),
        tr("types", "타입"),
        pipeline_count,
        tr("pipelines", "파이프라인")
    );

    // ── [xazz:timing] marker — pipeline execution latency (ms) for benchmarks/monitoring ──
    let pipeline_ms = timing_start.elapsed().as_secs_f64() * 1000.0;
    println!(
        "[xazz:timing] {}",
        serde_json::json!({ "pipeline_ms": pipeline_ms }).to_string()
    );

    // ── STEP 6: Automatic final DataFrame output (Top 5) ───────────────────
    if let Some(ref name) = last_var_name {
        if let Some(df) = symbol_table.get(name) {
            let row_count = df.height().min(5);
            let top5 = df.head(Some(row_count));
            println!();
            println!(
                "📊 [xazz Execution Result: '{}' (Top {} Rows)]",
                name, row_count
            );
            println!("{}", "─".repeat(60));
            println!("{}", top5);

            // ── [xazz:result] JSON marker ──────────────────────────────────────
            let api_limit = df.height().min(500);
            let api_df = df.head(Some(api_limit));
            let api_rows = df_to_json_array(&api_df).unwrap_or(serde_json::Value::Array(vec![]));
            let api_schema: Vec<serde_json::Value> = df
                .get_column_names()
                .iter()
                .map(|n| {
                    let dtype_str = df
                        .column(n)
                        .map(|s| format!("{}", s.dtype()))
                        .unwrap_or_default();
                    serde_json::json!({ "name": n.to_string(), "type": dtype_str })
                })
                .collect();
            let result_json = serde_json::json!({ "rows": api_rows, "schema": api_schema });
            println!(
                "[xazz:result] {}",
                serde_json::to_string(&result_json).unwrap_or_default()
            );

            // ── STEP 7: CSV Export (--output flag) ──────────────────────────
            if let Some(csv_path) = output_csv {
                match save_df_as_csv(df, csv_path) {
                    Ok(_) => {
                        println!();
                        println!("💾 [xazz] CSV {}: {}", tr("saved", "저장 완료"), csv_path);
                    }
                    Err(e) => {
                        eprintln!("[xazz] ⚠️  CSV {}: {}", tr("save failed", "저장 실패"), e);
                    }
                }
            }
        }
    }

    Ok(())
}

// ── CSV save helper ─────────────────────────────────────────────────────────
fn save_df_as_csv(
    df: &polars::frame::DataFrame,
    path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use polars::prelude::{CsvWriter, SerWriter};

    let mut file = std::fs::File::create(path).map_err(|e| {
        if is_korean() {
            format!("CSV 파일 생성 실패 '{}' — {}", path, e)
        } else {
            format!("failed to create CSV file '{}' — {}", path, e)
        }
    })?;

    CsvWriter::new(&mut file)
        .finish(&mut df.clone())
        .map_err(|e| {
            if is_korean() {
                format!("CSV 쓰기 실패 — {}", e)
            } else {
                format!("CSV write failed — {}", e)
            }
        })?;

    Ok(())
}

/// Writes a DataFrame to an artifact file in the requested format (issue #52).
fn save_dataframe(
    df: &polars::frame::DataFrame,
    path: &str,
    format: SaveFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    match format {
        SaveFormat::Csv => save_df_as_csv(df, path),
        SaveFormat::Parquet => save_df_as_parquet(df, path),
        SaveFormat::Arrow => save_df_as_arrow(df, path),
    }
}

fn save_df_as_parquet(
    df: &polars::frame::DataFrame,
    path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use polars::prelude::ParquetWriter;

    let file = std::fs::File::create(path).map_err(|e| {
        if is_korean() {
            format!("Parquet 파일 생성 실패 '{}' — {}", path, e)
        } else {
            format!("failed to create Parquet file '{}' — {}", path, e)
        }
    })?;
    ParquetWriter::new(file)
        .finish(&mut df.clone())
        .map_err(|e| {
            if is_korean() {
                format!("Parquet 쓰기 실패 — {}", e)
            } else {
                format!("Parquet write failed — {}", e)
            }
        })?;
    Ok(())
}

fn save_df_as_arrow(
    df: &polars::frame::DataFrame,
    path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use polars::prelude::{IpcWriter, SerWriter};

    let file = std::fs::File::create(path).map_err(|e| {
        if is_korean() {
            format!("Arrow 파일 생성 실패 '{}' — {}", path, e)
        } else {
            format!("failed to create Arrow file '{}' — {}", path, e)
        }
    })?;
    IpcWriter::new(file).finish(&mut df.clone()).map_err(|e| {
        if is_korean() {
            format!("Arrow 쓰기 실패 — {}", e)
        } else {
            format!("Arrow write failed — {}", e)
        }
    })?;
    Ok(())
}

// ── Schema-Based Type Cast ────────────────────────────────────────────────────
fn apply_schema_cast(
    lf: polars::prelude::LazyFrame,
    schema: &Schema,
) -> polars::prelude::LazyFrame {
    use polars::prelude::col;

    let cast_exprs: Vec<polars::prelude::Expr> = schema
        .fields
        .iter()
        .filter_map(|field| {
            let dtype = ir_col_to_dtype(&field.ty);
            dtype.map(|dt| col(field.name.as_str()).cast(dt).alias(field.name.as_str()))
        })
        .collect();

    if cast_exprs.is_empty() {
        lf
    } else {
        lf.with_columns(cast_exprs)
    }
}

/// IR ColType → Polars DataType (for column casting).
fn ir_col_to_dtype(ty: &ColType) -> Option<polars::prelude::DataType> {
    use polars::prelude::DataType;
    match ty.inner() {
        ColType::String => Some(DataType::String),
        ColType::Int => Some(DataType::Int64),
        ColType::Float => Some(DataType::Float64),
        ColType::Bool => Some(DataType::Boolean),
        _ => None,
    }
}

// ── Dynamic Schema Bridge ─────────────────────────────────────────────────────
fn apply_dynamic_bridge(
    lf: polars::prelude::LazyFrame,
    csv_headers: &[String],
    schema: &Schema,
) -> polars::prelude::LazyFrame {
    let map_count = csv_headers.len().min(schema.fields.len());

    let old_names: Vec<&str> = csv_headers[..map_count]
        .iter()
        .map(String::as_str)
        .collect();
    let new_names: Vec<&str> = schema.fields[..map_count]
        .iter()
        .map(|f| f.name.as_str())
        .collect();

    let (rename_old, rename_new): (Vec<&str>, Vec<&str>) = old_names
        .iter()
        .zip(new_names.iter())
        .filter(|(o, n)| o != n)
        .map(|(o, n)| (*o, *n))
        .unzip();

    if rename_old.is_empty() {
        lf
    } else {
        lf.rename(rename_old, rename_new, false)
    }
}

// ── Type validation / Null handling ─────────────────────────────────────────
fn validate_schema_types(df: &polars::frame::DataFrame, label: &str, schema: &Schema) {
    for field in &schema.fields {
        let is_optional = field.ty.is_option();
        match df.column(&field.name) {
            Ok(series) => {
                let null_count = series.null_count();
                let dtype = series.dtype();
                if null_count > 0 && !is_optional {
                    eprintln!(
                        "[xazz WARN] Null 위반 [{}]: 필수 필드 '{}' ({:?}) 에 null {} 개 발견",
                        label, field.name, dtype, null_count
                    );
                }
            }
            Err(_) => {
                eprintln!(
                    "[xazz WARN] {} '{}' {}",
                    tr("schema field", "스키마 필드"),
                    field.name,
                    tr("not found in DataFrame", "를 DataFrame에서 찾을 수 없음")
                );
            }
        }
    }
}

// ── Loader — format dispatch, lazy/out-of-core (issue #52/#53) ─────────────
// Chooses the Polars reader by file extension and returns a **LazyFrame** so the
// scan stays out-of-core until the terminal `.collect()`:
//   .parquet/.pq  → LazyFrame::scan_parquet  (streaming, columnar)
//   .arrow/.ipc/.feather → LazyFrame::scan_ipc (streaming, columnar)
//   everything else → CSV: LazyFrame::scan_csv for UTF-8 (out-of-core),
//                     eager EUC-KR decode fallback (small Korean public data).
fn load_source_lazy(
    file_path: &str,
) -> Result<polars::prelude::LazyFrame, Box<dyn std::error::Error>> {
    // DuckDB/PostgreSQL URIs produce an eager in-memory result — wrap in LazyFrame.
    if parse_duckdb_uri(file_path).is_some() {
        let df = load_duckdb_as_df(file_path)?;
        return Ok(polars::prelude::IntoLazy::lazy(df));
    }
    if parse_postgres_uri(file_path).is_some() {
        let df = load_postgres_as_df(file_path)?;
        return Ok(polars::prelude::IntoLazy::lazy(df));
    }

    let ext = std::path::Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "parquet" | "pq" => load_parquet_lazy(file_path),
        "arrow" | "ipc" | "feather" => load_arrow_lazy(file_path),
        _ => {
            let raw_bytes = std::fs::read(file_path).map_err(|e| {
                if is_korean() {
                    format!("IO 에러: CSV 파일 읽기 실패 '{}' — {}", file_path, e)
                } else {
                    format!("IO error: failed to read CSV file '{}' — {}", file_path, e)
                }
            })?;
            // UTF-8 → true out-of-core scan_csv. Non-UTF-8 (EUC-KR/CP949) →
            // eager decode fallback (these files are small Korean public datasets).
            if String::from_utf8(raw_bytes.clone()).is_ok() {
                load_csv_lazy(file_path)
            } else {
                use polars::prelude::IntoLazy;
                let df = load_csv_as_df_from_bytes(raw_bytes)?;
                Ok(df.lazy())
            }
        }
    }
}

/// Eager format-dispatch loader — used for **small** sources where a full
/// in-memory read is cheap and avoids a second disk pass for null-validation
/// (issue #53 perf: small files load once, validate once, then wrap in lazy).
/// Also the entry point for the sanitization checks (issue #72, F3).
pub(crate) fn load_source_as_df(
    file_path: &str,
) -> Result<polars::frame::DataFrame, Box<dyn std::error::Error>> {
    // DuckDB/PostgreSQL source URIs are not file paths — handle before extension dispatch.
    if parse_duckdb_uri(file_path).is_some() {
        return load_duckdb_as_df(file_path);
    }
    if parse_postgres_uri(file_path).is_some() {
        return load_postgres_as_df(file_path);
    }

    let ext = std::path::Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "parquet" | "pq" => load_parquet_as_df(file_path),
        "arrow" | "ipc" | "feather" => load_arrow_as_df(file_path),
        _ => load_csv_as_df_from_file(file_path),
    }
}

fn load_parquet_as_df(
    file_path: &str,
) -> Result<polars::frame::DataFrame, Box<dyn std::error::Error>> {
    use polars::prelude::{ParquetReader, SerReader};

    let file = std::fs::File::open(file_path).map_err(|e| {
        if is_korean() {
            format!("IO 에러: Parquet 파일 읽기 실패 '{}' — {}", file_path, e)
        } else {
            format!(
                "IO error: failed to read Parquet file '{}' — {}",
                file_path, e
            )
        }
    })?;
    let df = ParquetReader::new(file).finish().map_err(|e| {
        if is_korean() {
            format!("Parquet 읽기 실패 '{}' — {}", file_path, e)
        } else {
            format!("Parquet read failed '{}' — {}", file_path, e)
        }
    })?;
    Ok(df)
}

fn load_arrow_as_df(
    file_path: &str,
) -> Result<polars::frame::DataFrame, Box<dyn std::error::Error>> {
    use polars::prelude::{IpcReader, SerReader};

    let file = std::fs::File::open(file_path).map_err(|e| {
        if is_korean() {
            format!("IO 에러: Arrow 파일 읽기 실패 '{}' — {}", file_path, e)
        } else {
            format!(
                "IO error: failed to read Arrow file '{}' — {}",
                file_path, e
            )
        }
    })?;
    let df = IpcReader::new(file).finish().map_err(|e| {
        if is_korean() {
            format!("Arrow 읽기 실패 '{}' — {}", file_path, e)
        } else {
            format!("Arrow read failed '{}' — {}", file_path, e)
        }
    })?;
    Ok(df)
}

// ── DuckDB source connector (issue #56, Track A3) ───────────────────────────

/// Parses a `duckdb://` source URI into (db path, sql).
///
///   `load("duckdb://data.db?sql=SELECT * FROM t")`   → ("data.db", "SELECT …")
///   `load("duckdb://:memory:?sql=SELECT 1")`         → (":memory:", "SELECT 1")
///
/// Returns None when the path is not a DuckDB URI (ordinary file path).
fn parse_duckdb_uri(path: &str) -> Option<(String, String)> {
    if !path.starts_with("duckdb://") {
        return None;
    }
    let rest: &str = &path[9..]; // len("duckdb://") == 9
    // Split on the first `?sql=`.
    let Some(q) = rest.find('?') else {
        return None;
    };
    let db: &str = &rest[..q];
    let query: &str = &rest[q + 1..];
    if !query.starts_with("sql=") {
        return None;
    }
    Some((db.to_string(), query[4..].to_string()))
}

/// Loads a DuckDB query result as a Polars DataFrame (eager).
///
/// Runs the SQL against a DuckDB in-memory or file-backed database, exports the
/// result to a temporary Parquet file (`COPY (...) TO '...parquet'`), and reads
/// it back with Polars' own Parquet reader. This avoids cross-crate Arrow type
/// mismatches: DuckDB speaks its own Arrow, Polars speaks polars-arrow, and
/// Parquet is the neutral interchange format both natively support.
fn load_duckdb_as_df(uri: &str) -> Result<polars::frame::DataFrame, Box<dyn std::error::Error>> {
    let Some((db, sql)) = parse_duckdb_uri(uri) else {
        return Err(format!("invalid DuckDB URI: '{uri}'").into());
    };

    // In-memory (`:memory:`) combined with `COPY (...)` segfaulted in duckdb
    // 1.10505, so an ephemeral on-disk DB file is used for the in-memory case.
    let ephemeral_db = db == ":memory:" || db.is_empty();
    let tmp_db = std::path::PathBuf::from(std::env::temp_dir()).join(format!(
        "xazz_duckdb_{}_{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let db_str: &str = if ephemeral_db {
        tmp_db.to_str().unwrap_or("")
    } else {
        db.as_str()
    };
    let conn = duckdb::Connection::open(std::path::Path::new(&db_str)).map_err(|e| {
        if is_korean() {
            format!("DuckDB 연결 실패 '{}' — {}", db, e)
        } else {
            format!("failed to open DuckDB '{}' — {}", db, e)
        }
    })?;

    // Read the result row-by-row via ValueRef (type-tagged), then build a Polars
    // DataFrame column by column. `COPY (...)` TO parquet segfaulted in this
    // bundled duckdb build, so we avoid the file-interchange path entirely.
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        if is_korean() {
            format!("DuckDB SQL 준비 실패: {}", e)
        } else {
            format!("failed to prepare DuckDB SQL: {}", e)
        }
    })?;
    // NOTE: column_count/column_name are only valid after the statement has been
    // executed, so the query() call below must come first.
    let mut rows = stmt.query(()).map_err(|e| {
        if is_korean() {
            format!("DuckDB 쿼리 실패: {}", e)
        } else {
            format!("DuckDB query failed: {}", e)
        }
    })?;
    let stmt_ref = rows.as_ref().unwrap();
    let column_names: Vec<String> = (0..stmt_ref.column_count())
        .map(|i| {
            stmt_ref
                .column_name(i)
                .map(|s| s.as_str())
                .unwrap_or("")
                .to_string()
        })
        .collect();
    let n_cols = column_names.len();

    // Column-wise cell buffers (dynamically typed — a column may mix types).
    #[derive(Clone)]
    enum DCell {
        Int(i64),
        Float(f64),
        Str(String),
    }
    let mut cells: Vec<Vec<Option<DCell>>> = vec![Vec::new(); n_cols];

    while let Some(row) = rows.next().map_err(|e| {
        if is_korean() {
            format!("DuckDB 행 읽기 실패: {}", e)
        } else {
            format!("failed to read DuckDB row: {}", e)
        }
    })? {
        for i in 0..n_cols {
            use duckdb::types::ValueRef;
            let v = row.get_ref(i).map_err(|e| {
                if is_korean() {
                    format!("DuckDB 값 읽기 실패 (컬럼 {}): {}", i, e)
                } else {
                    format!("failed to read DuckDB value: {}", e)
                }
            })?;
            let cell = match v {
                ValueRef::Null => None,
                ValueRef::TinyInt(x) => Some(DCell::Int(x as i64)),
                ValueRef::SmallInt(x) => Some(DCell::Int(x as i64)),
                ValueRef::Int(x) => Some(DCell::Int(x as i64)),
                ValueRef::BigInt(x) => Some(DCell::Int(x)),
                ValueRef::UTinyInt(x) => Some(DCell::Int(x as i64)),
                ValueRef::USmallInt(x) => Some(DCell::Int(x as i64)),
                ValueRef::UInt(x) => Some(DCell::Int(x as i64)),
                ValueRef::UBigInt(x) => Some(DCell::Int(x as i64)),
                ValueRef::Float(x) => Some(DCell::Float(x as f64)),
                ValueRef::Double(x) => Some(DCell::Float(x)),
                ValueRef::Decimal(d) => {
                    let txt = d.to_string();
                    match txt.parse::<f64>() {
                        Ok(f) => Some(DCell::Float(f)),
                        Err(_) => Some(DCell::Str(txt)),
                    }
                }
                ValueRef::Date32(x) => Some(DCell::Int(x as i64)),
                ValueRef::Timestamp(_, x) => Some(DCell::Int(x)),
                ValueRef::Text(bytes) => {
                    Some(DCell::Str(String::from_utf8_lossy(bytes).to_string()))
                }
                _ => Some(DCell::Str("value".to_string())),
            };
            cells[i].push(cell);
        }
    }

    // Materialize each column into a typed Polars Series. The effective column
    // type is: Float if any cell is Float (ints coerce), else Str if any cell is
    // Str, else Int (or all-null → Str).
    let n_rows = cells.iter().map(|c| c.len()).max().unwrap_or(0);
    let mut df = polars::frame::DataFrame::empty();
    for i in 0..n_cols {
        let name = polars::prelude::PlSmallStr::from(column_names[i].as_str());
        let has_float = cells[i]
            .iter()
            .flatten()
            .any(|c| matches!(c, DCell::Float(_)));
        let has_str = cells[i]
            .iter()
            .flatten()
            .any(|c| matches!(c, DCell::Str(_)));

        if has_float {
            let vals: Vec<Option<f64>> = (0..n_rows)
                .map(|r| match cells[i].get(r).and_then(|c| c.as_ref()) {
                    Some(DCell::Float(x)) => Some(*x),
                    Some(DCell::Int(x)) => Some(*x as f64),
                    Some(DCell::Str(_)) => None,
                    None => None,
                })
                .collect();
            let col = polars::prelude::Column::new(name, vals);
            df.with_column(col)
                .map_err(|e| format!("failed to add DuckDB column '{}': {}", column_names[i], e))?;
        } else if has_str {
            let vals: Vec<Option<String>> = (0..n_rows)
                .map(|r| match cells[i].get(r).and_then(|c| c.as_ref()) {
                    Some(DCell::Str(x)) => Some(x.clone()),
                    Some(DCell::Int(x)) => Some(x.to_string()),
                    Some(DCell::Float(x)) => Some(x.to_string()),
                    None => None,
                })
                .collect();
            let col = polars::prelude::Column::new(name, vals);
            df.with_column(col)
                .map_err(|e| format!("failed to add DuckDB column '{}': {}", column_names[i], e))?;
        } else {
            let vals: Vec<Option<i64>> = (0..n_rows)
                .map(|r| match cells[i].get(r).and_then(|c| c.as_ref()) {
                    Some(DCell::Int(x)) => Some(*x),
                    _ => None,
                })
                .collect();
            let col = polars::prelude::Column::new(name, vals);
            df.with_column(col)
                .map_err(|e| format!("failed to add DuckDB column '{}': {}", column_names[i], e))?;
        }
    }

    if ephemeral_db {
        let _ = std::fs::remove_file(&tmp_db);
    }
    Ok(df)
}

// ── PostgreSQL source connector (issue #54, Track A3) ────────────────────────

/// Parses a `postgres://` source URI into (connect-params, sql).
///
///   `load("postgres://user:pass@localhost:5432/mydb?sql=SELECT 1 AS a")`
///     → ("postgres://user:pass@localhost:5432/mydb", "SELECT 1 AS a")
///
/// The `?sql=` query parameter is the SQL text; all other query parameters are
/// left intact for the connection string. Returns None for non-postgres paths.
fn parse_postgres_uri(path: &str) -> Option<(String, String)> {
    if !path.starts_with("postgres://") {
        return None;
    }
    // Find `?sql=` (first query param) or `&sql=` (subsequent param).
    let find = path.find("?sql=").or_else(|| path.find("&sql="))?;
    let q = find;
    let conn: &str = &path[..q];
    let sql: &str = &path[q + 5..];
    Some((conn.to_string(), sql.to_string()))
}

/// Loads a PostgreSQL query result as a Polars DataFrame (eager).
///
/// Connects over TCP (NoTls by default) using the `postgres` crate, runs the
/// SQL, and converts each row's typed values into Polars columns.
fn load_postgres_as_df(uri: &str) -> Result<polars::frame::DataFrame, Box<dyn std::error::Error>> {
    use postgres::{Client, NoTls};

    let Some((conn, sql)) = parse_postgres_uri(uri) else {
        return Err(format!("invalid PostgreSQL URI: '{uri}'").into());
    };

    let mut client = Client::connect(&conn, NoTls).map_err(|e| {
        if is_korean() {
            format!("PostgreSQL 연결 실패 '{}' — {}", conn, e)
        } else {
            format!("failed to connect to PostgreSQL '{}' — {}", conn, e)
        }
    })?;

    let rows = client.query(&sql, &[]).map_err(|e| {
        if is_korean() {
            format!("PostgreSQL 쿼리 실패: {}", e)
        } else {
            format!("PostgreSQL query failed: {}", e)
        }
    })?;

    if rows.is_empty() {
        return Ok(polars::frame::DataFrame::empty());
    }

    let columns = rows[0].columns();
    let n_cols = columns.len();
    let column_names: Vec<String> = (0..n_cols).map(|i| columns[i].name().to_string()).collect();

    // Per-column cell buffers (dynamically typed).
    #[derive(Clone)]
    enum PCell {
        Int(i64),
        Float(f64),
        Str(String),
    }
    let mut cells: Vec<Vec<Option<PCell>>> = vec![Vec::new(); n_cols];

    for row in &rows {
        for i in 0..n_cols {
            // postgres crate Row::try_get — returns Result; match instead of
            // .ok() (this std has no Result::ok()).
            let cell = match row.try_get::<usize, Option<i64>>(i) {
                Ok(Some(v)) => Some(PCell::Int(v)),
                Ok(None) => None,
                Err(_) => match row.try_get::<usize, Option<f64>>(i) {
                    Ok(Some(v)) => Some(PCell::Float(v)),
                    Ok(None) => None,
                    Err(_) => match row.try_get::<usize, Option<&str>>(i) {
                        Ok(Some(v)) => Some(PCell::Str(v.to_string())),
                        Ok(None) => None,
                        Err(_) => match row.try_get::<usize, Option<bool>>(i) {
                            Ok(Some(v)) => {
                                Some(PCell::Str(if v { "true" } else { "false" }.to_string()))
                            }
                            _ => None,
                        },
                    },
                },
            };
            cells[i].push(cell);
        }
    }

    // Materialize each column into a typed Polars Series.
    let n_rows = rows.len();
    let mut df = polars::frame::DataFrame::empty();
    for i in 0..n_cols {
        let name = polars::prelude::PlSmallStr::from(column_names[i].as_str());
        let has_float = cells[i]
            .iter()
            .flatten()
            .any(|c| matches!(c, PCell::Float(_)));
        let has_str = cells[i]
            .iter()
            .flatten()
            .any(|c| matches!(c, PCell::Str(_)));

        if has_float {
            let vals: Vec<Option<f64>> = (0..n_rows)
                .map(|r| match cells[i].get(r).and_then(|c| c.as_ref()) {
                    Some(PCell::Float(x)) => Some(*x),
                    Some(PCell::Int(x)) => Some(*x as f64),
                    Some(PCell::Str(_)) => None,
                    None => None,
                })
                .collect();
            let col = polars::prelude::Column::new(name, vals);
            df.with_column(col).map_err(|e| {
                format!(
                    "failed to add PostgreSQL column '{}': {}",
                    column_names[i], e
                )
            })?;
        } else if has_str {
            let vals: Vec<Option<String>> = (0..n_rows)
                .map(|r| match cells[i].get(r).and_then(|c| c.as_ref()) {
                    Some(PCell::Str(x)) => Some(x.clone()),
                    Some(PCell::Int(x)) => Some(x.to_string()),
                    Some(PCell::Float(x)) => Some(x.to_string()),
                    None => None,
                })
                .collect();
            let col = polars::prelude::Column::new(name, vals);
            df.with_column(col).map_err(|e| {
                format!(
                    "failed to add PostgreSQL column '{}': {}",
                    column_names[i], e
                )
            })?;
        } else {
            let vals: Vec<Option<i64>> = (0..n_rows)
                .map(|r| match cells[i].get(r).and_then(|c| c.as_ref()) {
                    Some(PCell::Int(x)) => Some(*x),
                    _ => None,
                })
                .collect();
            let col = polars::prelude::Column::new(name, vals);
            df.with_column(col).map_err(|e| {
                format!(
                    "failed to add PostgreSQL column '{}': {}",
                    column_names[i], e
                )
            })?;
        }
    }
    Ok(df)
}

fn load_parquet_lazy(
    file_path: &str,
) -> Result<polars::prelude::LazyFrame, Box<dyn std::error::Error>> {
    use polars::prelude::{LazyFrame, PlRefPath, ScanArgsParquet};

    LazyFrame::scan_parquet(PlRefPath::new(file_path), ScanArgsParquet::default())
        .map_err(|e| {
            if is_korean() {
                format!("Parquet 읽기 실패 '{}' — {}", file_path, e)
            } else {
                format!("Parquet read failed '{}' — {}", file_path, e)
            }
        })
        .map_err(Into::into)
}

fn load_arrow_lazy(
    file_path: &str,
) -> Result<polars::prelude::LazyFrame, Box<dyn std::error::Error>> {
    use polars::io::ipc::IpcScanOptions;
    use polars::lazy::dsl::UnifiedScanArgs;
    use polars::prelude::{LazyFrame, PlRefPath};

    LazyFrame::scan_ipc(
        PlRefPath::new(file_path),
        IpcScanOptions::default(),
        UnifiedScanArgs::default(),
    )
    .map_err(|e| {
        if is_korean() {
            format!("Arrow 읽기 실패 '{}' — {}", file_path, e)
        } else {
            format!("Arrow read failed '{}' — {}", file_path, e)
        }
    })
    .map_err(Into::into)
}

/// Out-of-core CSV scan for UTF-8 files. Streams row batches from disk instead
/// of materializing the whole file; null normalization mirrors load_csv_as_df.
fn load_csv_lazy(
    file_path: &str,
) -> Result<polars::prelude::LazyFrame, Box<dyn std::error::Error>> {
    use polars::prelude::{LazyCsvReader, LazyFileListReader, NullValues, PlRefPath, PlSmallStr};

    let null_strings: Vec<PlSmallStr> = vec![
        "".into(),
        " ".into(),
        "-".into(),
        "점검중".into(),
        "N/A".into(),
    ];

    LazyCsvReader::new(PlRefPath::new(file_path))
        .with_infer_schema_length(Some(SCHEMA_INFERENCE_ROWS))
        .map_parse_options(move |p| {
            p.with_null_values(Some(NullValues::AllColumns(null_strings.clone())))
        })
        .finish()
        .map_err(|e| {
            if is_korean() {
                format!("CSV 읽기 실패 '{}' — {}", file_path, e)
            } else {
                format!("CSV read failed '{}' — {}", file_path, e)
            }
        })
        .map_err(Into::into)
}

/// Threshold (bytes) above which the eager schema null-validation collect is
/// skipped, keeping large sources out-of-core (issue #53).
const SMALL_SOURCE_BYTES: u64 = 32 * 1024 * 1024;

fn source_is_small(file_path: &str) -> bool {
    // DuckDB/PostgreSQL URIs are not files — treat as eager (in-memory result).
    if parse_duckdb_uri(file_path).is_some() {
        return true;
    }
    if parse_postgres_uri(file_path).is_some() {
        return true;
    }
    std::fs::metadata(file_path)
        .map(|m| m.len() <= SMALL_SOURCE_BYTES)
        .unwrap_or(true)
}

/// Collects a LazyFrame with the streaming engine when the source is large,
/// falling back to the in-memory engine for unsupported plans (issue #53).
/// Small pipelines (tests/ML/DP) use the in-memory engine to avoid scheduler
/// overhead on trivial queries.
fn collect_lazy(
    lf: polars::prelude::LazyFrame,
    source_is_large: bool,
) -> Result<polars::frame::DataFrame, Box<dyn std::error::Error>> {
    use polars::lazy::dsl::Engine;

    let use_streaming = match std::env::var("XAZZ_STREAMING") {
        Ok(v) => v == "1" || v.eq_ignore_ascii_case("true"),
        Err(_) => source_is_large,
    };

    if use_streaming {
        match lf.clone().collect_with_engine(Engine::Streaming) {
            Ok(df) => Ok(df),
            // Unsupported nodes fall back to the in-memory engine.
            Err(_) => lf.collect().map_err(Into::into),
        }
    } else {
        lf.collect().map_err(Into::into)
    }
}

// ── CSV loader (automatic encoding handling + dirty-data null normalization) ──
fn load_csv_as_df_from_bytes(
    raw_bytes: Vec<u8>,
) -> Result<polars::frame::DataFrame, Box<dyn std::error::Error>> {
    use polars::prelude::{CsvParseOptions, CsvReadOptions, NullValues, SerReader};
    use std::io::Cursor;

    let utf8_string = match String::from_utf8(raw_bytes.clone()) {
        Ok(s) => s,
        Err(_) => {
            use encoding_rs::EUC_KR;
            let (cow, _encoding_used, _had_errors) = EUC_KR.decode(&raw_bytes);
            cow.into_owned()
        }
    };

    let null_vals = NullValues::AllColumns(vec![
        "".into(),
        " ".into(),
        "-".into(),
        "점검중".into(),
        "N/A".into(),
    ]);

    let cursor = Cursor::new(utf8_string.into_bytes());
    let df = CsvReadOptions::default()
        .with_infer_schema_length(Some(SCHEMA_INFERENCE_ROWS))
        .with_parse_options(CsvParseOptions::default().with_null_values(Some(null_vals)))
        .into_reader_with_file_handle(cursor)
        .finish()?;

    Ok(df)
}

fn load_csv_as_df_from_file(
    file_path: &str,
) -> Result<polars::frame::DataFrame, Box<dyn std::error::Error>> {
    let raw_bytes = std::fs::read(file_path).map_err(|e| {
        if is_korean() {
            format!("IO 에러: CSV 파일 읽기 실패 '{}' — {}", file_path, e)
        } else {
            format!("IO error: failed to read CSV file '{}' — {}", file_path, e)
        }
    })?;
    load_csv_as_df_from_bytes(raw_bytes)
}

// ── Single pipeline node execution (consumes Typed IR) ──────────────────
//
// Consumes the type-checker's PipelineNode once instead of interpreting the raw AST,
// branching into data (lower::lower_data)/ML (dl::train/predict)/side (chart/dp).
fn execute_node(
    node: &PipelineNode,
    symbol_table: &HashMap<String, polars::frame::DataFrame>,
    model_registry: &HashMap<String, Vec<LayerKind>>,
    model_table: &mut HashMap<String, crate::dl::TrainedModel>,
    dp_budget: &mut crate::dp::PrivacyBudget,
) -> Result<Option<polars::frame::DataFrame>, Box<dyn std::error::Error>> {
    use polars::prelude::IntoLazy;

    let (mut lf, _schema_opt, source_is_large): (
        polars::prelude::LazyFrame,
        Option<&Schema>,
        bool,
    ) = match &node.source {
        Source::Load { file_path, schema } => {
            let is_large = !source_is_small(file_path);

            if !is_large {
                // ── Small source → eager fast path (issue #53) ──────────────────
                // Read the file once via the format-dispatched eager loader, validate
                // in-memory, then wrap in LazyFrame. Avoids the lazy-scan plus a
                // second disk pass for null-validation (which regressed small files).
                let df_raw = load_source_as_df(file_path)?;
                let src_headers: Vec<String> = df_raw
                    .get_column_names()
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                let lf_bridged = match schema {
                    Some(fields) => {
                        let lf_renamed = apply_dynamic_bridge(df_raw.lazy(), &src_headers, fields);
                        apply_schema_cast(lf_renamed, fields)
                    }
                    None => df_raw.lazy(),
                };
                if let Some(fields) = schema {
                    let df_loaded = lf_bridged.clone().collect()?;
                    validate_schema_types(&df_loaded, file_path, fields);
                }
                (lf_bridged, schema.as_ref(), false)
            } else {
                // ── Large source → lazy out-of-core scan (issue #53) ────────────
                // Source stays a LazyFrame until the terminal collect; large files
                // are never fully materialized upfront. Schema null-validation's
                // eager collect is skipped (it would defeat out-of-core).
                let mut lf_raw = load_source_lazy(file_path)?;
                let src_headers: Vec<String> = lf_raw
                    .collect_schema()
                    .map_err(|e| format!("schema inference failed for '{}' — {}", file_path, e))?
                    .iter_names()
                    .map(|n| n.to_string())
                    .collect();
                let lf_bridged = match schema {
                    Some(fields) => {
                        let lf_renamed = apply_dynamic_bridge(lf_raw, &src_headers, fields);
                        apply_schema_cast(lf_renamed, fields)
                    }
                    None => lf_raw,
                };
                (lf_bridged, schema.as_ref(), true)
            }
        }
        Source::Ref { var } => match symbol_table.get(var.as_str()) {
            Some(df) => (df.clone().lazy(), None, false),
            None => {
                return Err(format!(
                    "변수 에러: 미선언 변수 '{}' 참조. 이전 파이프라인에서 먼저 선언하세요.",
                    var
                )
                .into());
            }
        },
    };

    let mut pending_group: Option<String> = None;

    for step in &node.steps {
        match step {
            IrStep::Data(op) => {
                crate::lower::lower_data(op, &mut lf, symbol_table, &mut pending_group)?;
            }
            IrStep::ML(MLOp::Train { model, config }) => {
                let snapshot = lf.clone().collect()?;
                let layers = model_registry.get(model.as_str()).cloned().ok_or_else(|| {
                    if is_korean() {
                        format!(
                            "모델 블록 '{model}' 을 찾을 수 없습니다. 먼저 `model {model} {{ ... }}` 로 선언하세요."
                        )
                    } else {
                        format!(
                            "model block '{model}' was not found. Declare it first with `model {model} {{ ... }}`."
                        )
                    }
                })?;
                let trained = crate::dl::train(&snapshot, model, &layers, config)
                    .map_err(|e| format!("{}: {e}", tr("training failed", "학습 실패")))?;
                print_train_report(&trained);

                let source_var = node.name.clone().unwrap_or_else(|| match &node.source {
                    Source::Ref { var } => var.clone(),
                    Source::Load { .. } => String::new(),
                });
                let train_json = serde_json::json!({
                    "type": "train_stmt",
                    "success": true,
                    "source_var": source_var,
                    "model_name": model,
                    "report": serde_json::to_value(&trained.report).unwrap_or_default(),
                });
                println!(
                    "[xazz:train] {}",
                    serde_json::to_string(&train_json).unwrap_or_default()
                );

                if node.yields_model
                    && let Some(vname) = &node.name
                {
                    model_table.insert(vname.clone(), trained);
                }
                return Ok(None);
            }
            IrStep::ML(MLOp::Predict { model, as_col }) => {
                let trained = model_table.get(model.as_str()).ok_or_else(|| {
                    if is_korean() {
                        format!(
                            "학습된 모델 변수 '{model}' 이 없습니다. 먼저 `v {model} = ... |> train(...)` 로 학습하세요."
                        )
                    } else {
                        format!(
                            "trained model variable '{model}' was not found. Train it first with `v {model} = ... |> train(...)`."
                        )
                    }
                })?;
                let snapshot = lf.clone().collect()?;
                let out = crate::dl::predict(trained, &snapshot, as_col.as_deref())
                    .map_err(|e| format!("{}: {e}", tr("prediction failed", "예측 실패")))?;
                eprintln!(
                    "[xazz] Predict '{}' {}: {} ({} {})",
                    model,
                    tr("done", "완료"),
                    tr("prediction column added", "예측 컬럼 추가"),
                    out.height(),
                    tr("rows", "행")
                );
                lf = out.lazy();
            }
            IrStep::Side(SideOp::Chart(config)) => {
                let snapshot = lf.clone().collect()?;
                let spec = build_chart_spec(config, &snapshot)?;
                // Single-line self-contained marker — keeps the parser intact even if
                // newlines/emoji would break the JSON. (Previously: marker and JSON spanned two lines)
                println!("[xazz:chart] {}", serde_json::to_string(&spec)?);

                let safe_base = node.name.clone().unwrap_or_else(|| match &node.source {
                    Source::Ref { var } => var.clone(),
                    Source::Load { .. } => format!("chart_{}", node.id + 1),
                });
                let safe_name: String = safe_base
                    .chars()
                    .map(|c| {
                        if c.is_alphanumeric() || c == '_' || c == '-' {
                            c
                        } else {
                            '_'
                        }
                    })
                    .collect();
                let html_path = format!("{}_chart.html", safe_name);
                match write_chart_html(&spec, &html_path) {
                    Ok(_) => {
                        println!(
                            "[xazz] 📊 {} HTML: {}",
                            tr("chart generated", "차트 HTML 생성"),
                            html_path
                        );
                        #[cfg(target_os = "windows")]
                        let _ = std::process::Command::new("cmd")
                            .args(["/c", "start", "", &html_path])
                            .spawn();
                        #[cfg(target_os = "macos")]
                        let _ = std::process::Command::new("open").arg(&html_path).spawn();
                        #[cfg(target_os = "linux")]
                        let _ = std::process::Command::new("xdg-open")
                            .arg(&html_path)
                            .spawn();
                    }
                    Err(e) => {
                        eprintln!(
                            "[xazz] ⚠️  {}: {}",
                            tr("chart HTML generation failed", "차트 HTML 생성 실패"),
                            e
                        );
                    }
                }

                eprintln!(
                    "[xazz] Chart '{}' done: {} {}",
                    config.chart_type.as_str(),
                    snapshot.height(),
                    tr("rows", "행")
                );
                lf = snapshot.lazy();
            }
            IrStep::Side(SideOp::WithDp(args)) => {
                // 1) Collect the pipeline so far, then apply output perturbation
                let snapshot = lf.clone().collect()?;
                let (noised, report) = crate::dp::apply_dp(&snapshot, args)?;

                // 2) Deduct budget only after noise injection succeeds (atomic composition accounting).
                //    - Injecting noise into k columns is k independent mechanisms, so bill as k·ε.
                //    - If apply_dp fails, the budget is untouched (no ε spent on failure).
                let delta = args.delta.unwrap_or(crate::dp::DEFAULT_DELTA);
                dp_budget.spend_n(
                    args.mechanism,
                    args.epsilon,
                    delta,
                    report.noised_columns.len(),
                )?;

                let mut dp_json =
                    serde_json::to_value(&report).unwrap_or_else(|_| serde_json::json!({}));
                if let Some(obj) = dp_json.as_object_mut() {
                    obj.insert("budget_spent".into(), serde_json::json!(dp_budget.spent()));
                    obj.insert("budget_total".into(), serde_json::json!(dp_budget.total()));
                    obj.insert(
                        "budget_spent_delta".into(),
                        serde_json::json!(dp_budget.spent_delta()),
                    );
                    obj.insert(
                        "budget_total_delta".into(),
                        serde_json::json!(dp_budget.total_delta()),
                    );
                    obj.insert(
                        "query_count".into(),
                        serde_json::json!(dp_budget.query_count()),
                    );
                }
                // Single-line self-contained marker (parse-safe even if broken by newlines/emoji).
                println!("[xazz:dp] {}", dp_json);
                eprintln!(
                    "[xazz] DP {}: {} (ε={}, δ={}, Δf={}, {}={:.4}) — {} {:?} | {} ε {:.2}/{:.2} · δ {:.2e}/{:.2e}",
                    tr("applied", "적용"),
                    report.mechanism,
                    report.epsilon,
                    report
                        .delta
                        .map(|d| format!("{d:.2e}"))
                        .unwrap_or_else(|| "0".to_string()),
                    report.sensitivity,
                    tr("noise parameter", "노이즈 파라미터"),
                    report.noise_param,
                    tr("columns", "컬럼"),
                    report.noised_columns,
                    tr("budget", "예산"),
                    dp_budget.spent(),
                    dp_budget.total(),
                    dp_budget.spent_delta(),
                    dp_budget.total_delta(),
                );

                lf = noised.lazy();
            }
            IrStep::Side(SideOp::Save { path, format }) => {
                let snapshot = lf.clone().collect()?;
                save_dataframe(&snapshot, path, *format)?;
                eprintln!(
                    "[xazz] save('{}') {}: {} {} × {}",
                    path,
                    tr("done", "완료"),
                    format.as_str(),
                    snapshot.height(),
                    snapshot.width()
                );
                lf = snapshot.lazy();
            }
        }
    }

    let df = collect_lazy(lf, source_is_large)?;
    Ok(Some(df))
}

// ── Burn deep-learning execution (v0.4) ───────────────────────────────────

/// ModelDecl handling — logs the model definition and registers layer info for training.
fn handle_model_decl(name: &str, layers: &[LayerKind]) {
    println!();
    println!("🧠 [xazz Model Declaration: {}]", name);
    println!("{}", "─".repeat(60));
    println!(
        "  {} ({}):",
        tr("layer configuration", "레이어 구성"),
        layers.len()
    );
    for (i, layer) in layers.iter().enumerate() {
        let layer_desc = match layer {
            LayerKind::Dense(n) => format!("Dense({})", n),
            LayerKind::ReLU => "ReLU()".to_string(),
            LayerKind::Sigmoid => "Sigmoid()".to_string(),
            LayerKind::Tanh => "Tanh()".to_string(),
            LayerKind::Softmax => "Softmax()".to_string(),
            LayerKind::Dropout(r) => format!("Dropout({})", r),
            LayerKind::BatchNorm => "BatchNorm()".to_string(),
        };
        println!("    [{}] {}  →  {}", i, layer_desc, layer.to_burn_str());
    }
    println!();

    // [xazz:model] JSON marker — emitted so the server/IDE can parse model info
    let model_json = serde_json::json!({
        "type": "model_decl",
        "name": name,
        "layers": layers.iter().map(|l| format!("{:?}", l)).collect::<Vec<_>>(),
        "burn_code": layers.iter().map(|l| l.to_burn_str()).collect::<Vec<_>>(),
    });
    println!(
        "[xazz:model] {}",
        serde_json::to_string(&model_json).unwrap_or_default()
    );
}

/// Prints the trained model's (TrainedModel) report to the console.
fn print_train_report(trained: &crate::dl::TrainedModel) {
    let report = &trained.report;
    println!("{}", "─".repeat(60));
    println!("✅  {}", tr("training complete", "학습 완료"));
    println!("  {}  : {}", tr("input dim", "입력 차원"), report.input_dim);
    println!(
        "  {}  : {}",
        tr("output dim", "출력 차원"),
        report.output_dim
    );
    println!(
        "  {} : {}",
        tr("parameters", "파라미터 수"),
        report.num_params
    );
    println!(
        "  {} : {:.6}",
        tr("final loss (MSE)", "최종 손실(MSE)"),
        report.final_train_loss
    );
    if let Some(v) = report.final_val_loss {
        println!(
            "  {} : {:.6}",
            tr("validation loss (MSE)", "검증 손실(MSE)"),
            v
        );
    }
    println!(
        "  {}  : {:?}",
        tr("feature columns", "특성 컬럼"),
        report.feature_names
    );
    if !report.predictions.is_empty() {
        println!("  {}  :", tr("sample predictions", "샘플 예측"));
        for i in 0..report.predictions.len().min(5) {
            println!(
                "    [{i}] {} = {:.4}   {} = {:.4}",
                tr("predicted", "예측"),
                report.predictions[i],
                tr("actual", "실제"),
                report.targets[i]
            );
        }
    }
    println!(
        "  {} : {}",
        tr("checkpoint", "체크포인트"),
        report.checkpoint_path
    );
    println!();
}

/// Exports the policy report to stdout via the `[xazz:policy]` marker.
///
/// The server/IDE parses this marker to show the block reason and remediation hint.
/// It is always emitted regardless of block/pass, so the frontend can trust that
/// "the check ran".
fn emit_policy_marker(report: &xazz_compiler::PolicyReport) {
    match serde_json::to_string(report) {
        Ok(json) => println!("[xazz:policy] {}", json),
        Err(e) => eprintln!(
            "[xazz] ⚠️ {}: {}",
            tr(
                "policy report serialization failed",
                "정책 리포트 직렬화 실패"
            ),
            e
        ),
    }
}

// ── Streaming engine support tests (issue #53) ───────────────────────────

#[cfg(test)]
mod streaming_tests {
    use polars::lazy::dsl::Engine;
    use polars::prelude::{IntoLazy, df};

    /// Proves the streaming engine (Engine::Streaming) natively supports the
    /// benchmark operator set — filter/group_by/agg/sort/limit — WITHOUT the
    /// in-memory fallback. This is what makes the 200M-row out-of-core bench valid.
    #[test]
    fn streaming_engine_supports_benchmark_operators() {
        let frame = df!(
            "g" => ["a", "a", "b", "b", "c"],
            "v" => [1i64, 5, 10, 20, 30],
        )
        .unwrap();

        let lf = frame
            .clone()
            .lazy()
            .filter(polars::prelude::col("v").gt(0))
            .group_by([polars::prelude::col("g")])
            .agg([polars::prelude::col("v").sum().alias("s")])
            .sort(
                ["s"],
                polars::prelude::SortMultipleOptions::default().with_order_descending(true),
            )
            .limit(2);

        let out = lf
            .collect_with_engine(Engine::Streaming)
            .expect("streaming 엔진이 벤치 연산(filter/group_by/sum/sort/limit)을 지원해야 함");
        assert_eq!(out.height(), 2);
    }
}

// ── DuckDB source connector tests (issue #56, Track A3) ───────────────────

#[cfg(test)]
mod duckdb_tests {
    use super::*;

    #[test]
    fn parses_duckdb_uri() {
        let (db, sql) =
            parse_duckdb_uri("duckdb://:memory:?sql=SELECT 1 AS a").expect("should parse");
        assert_eq!(db, ":memory:");
        assert_eq!(sql, "SELECT 1 AS a");

        let (db, sql) =
            parse_duckdb_uri("duckdb://data.db?sql=SELECT * FROM t").expect("should parse");
        assert_eq!(db, "data.db");
        assert_eq!(sql, "SELECT * FROM t");
    }

    #[test]
    fn non_duckdb_path_is_not_a_uri() {
        assert!(parse_duckdb_uri("data.csv").is_none());
        assert!(parse_duckdb_uri("duckdb://noproto").is_none());
    }

    #[test]
    fn loads_query_result_as_df() {
        let df = load_duckdb_as_df(
            "duckdb://:memory:?sql=SELECT 1 AS id, 'alpha' AS label, 3.14 AS val UNION ALL SELECT 2, 'beta', 2.71",
        )
        .expect("duckdb query should run");
        assert_eq!(df.height(), 2);
        assert_eq!(df.width(), 3);
        // id is i64
        let id_vals: Vec<i64> = df
            .column("id")
            .unwrap()
            .as_materialized_series()
            .i64()
            .unwrap()
            .iter()
            .flatten()
            .collect();
        assert_eq!(id_vals, vec![1, 2]);
        // label is str
        let label_vals: Vec<&str> = df
            .column("label")
            .unwrap()
            .as_materialized_series()
            .str()
            .unwrap()
            .iter()
            .flatten()
            .collect();
        assert_eq!(label_vals, vec!["alpha", "beta"]);
        // val is f64 (Decimal coerced)
        let val_vals: Vec<f64> = df
            .column("val")
            .unwrap()
            .as_materialized_series()
            .f64()
            .unwrap()
            .iter()
            .flatten()
            .collect();
        assert!((val_vals[0] - 3.14).abs() < 1e-9, "val[0]={}", val_vals[0]);
        assert!((val_vals[1] - 2.71).abs() < 1e-9, "val[1]={}", val_vals[1]);
    }

    #[test]
    fn empty_result_is_empty_df() {
        let df = load_duckdb_as_df("duckdb://:memory:?sql=SELECT 1 AS a WHERE 1=0")
            .expect("empty query should run");
        assert_eq!(df.height(), 0);
    }
}

// ── PostgreSQL source connector tests (issue #54, Track A3) ───────────────

#[cfg(test)]
mod postgres_tests {
    use super::*;

    #[test]
    fn parses_postgres_uri() {
        let (conn, sql) =
            parse_postgres_uri("postgres://user:pass@localhost:5432/mydb?sql=SELECT 1 AS a")
                .expect("should parse");
        assert_eq!(conn, "postgres://user:pass@localhost:5432/mydb");
        assert_eq!(sql, "SELECT 1 AS a");

        // Other query parameters survive (only ?sql= is consumed).
        let (conn, _sql) =
            parse_postgres_uri("postgres://user@host/db?sslmode=disable&sql=SELECT 2")
                .expect("should parse");
        assert_eq!(conn, "postgres://user@host/db?sslmode=disable");
    }

    #[test]
    fn non_postgres_path_is_not_a_uri() {
        assert!(parse_postgres_uri("data.csv").is_none());
        assert!(parse_postgres_uri("duckdb://x?sql=SELECT 1").is_none());
        // No ?sql= → not loadable.
        assert!(parse_postgres_uri("postgres://user@host/db").is_none());
    }

    #[test]
    fn postgres_without_server_errors_gracefully() {
        // No local Postgres in CI/dev by default — connecting to a closed port
        // must return a clear Err, never panic/segfault.
        let err = load_postgres_as_df("postgres://user:pass@127.0.0.1:1/x?sql=SELECT 1");
        assert!(err.is_err(), "should fail to connect on port 1");
        let msg = err.unwrap_err().to_string();
        assert!(
            msg.contains("PostgreSQL") || msg.contains("postgres"),
            "unexpected error: {msg}"
        );
    }
}
