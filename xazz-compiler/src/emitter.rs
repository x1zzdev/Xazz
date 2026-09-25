/// xazz-compiler/src/emitter.rs — Rust code emission (Transpiler Layer) v0.16
///
/// .xzz script → standalone runnable Rust + Polars source code generator.
///
/// [v0.16 changes]
///   - BoolLit support (to_typed_polars_expr)
///   - Count(None) / Count(Some(col)) distinction
///   - code generation for new operators:
///     GroupBy + aggregation operator → .group_by([...]).agg([...]) pairing
///     OrderBy → .sort([...], SortMultipleOptions)
///     Take → .limit(n)
///     DropNull → .drop_nulls(Some(vec![...]))
///     FillNull → .with_columns([...fill_null(...)])
///   - validate_op_columns: schema validation added for the new column-argument operators
///   - Join operator code generation: .join(..., JoinArgs::new(JoinType::...))
///   - WithColumn operator code generation: .with_columns([expr.alias("name")])
///   - arithmetic operator to_typed_polars_expr: add/sub/mul/div
///
/// deduplication (single location):
///   - the expression → Polars string mapping uses the single implementation in
///     `crate::polars_text` (shared by this module and codegen.rs).
///   - the runtime op→Polars mapping exists only in xazz-exec/src/lower.rs (Typed IR).
use std::collections::HashMap;
use std::fs;

use crate::ast::{
    EmbeddingVocab, Expr, LayerKind, LoadOptions, PipelineOp, PipelineSource, Program, Stmt,
    TrainConfig,
};
use crate::policy::printer::escape;
use crate::{Codegen, Lexer, Parser, StructField};
use xazz_core::i18n::is_korean;

/// default training batch size (the default in generated code).
const DEFAULT_BATCH_SIZE: usize = 32;
/// validation split ratio upper bound (the clamp in generated code).
const MAX_VALIDATION_SPLIT: f64 = 0.9;

/// Human-readable `load()` option suffix for comments, e.g. `, sep: ";", header: false`.
fn load_options_comment(options: &LoadOptions) -> String {
    let mut parts = Vec::new();
    if let Some(sep) = options.separator {
        parts.push(format!("sep: \"{}\"", sep as char));
    }
    if let Some(header) = options.has_header {
        parts.push(format!("header: {}", header));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(", {}", parts.join(", "))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ── public entry points ───────────────────────────────────────────────────────────────
// ─────────────────────────────────────────────────────────────────────────────

/// Converts a .xzz file into standalone runnable Rust source code.
pub fn emit_rust(
    source_path: &str,
    out_path: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    // ── Step 1: read the source file ───────────────────────────────────────────────
    let source = fs::read_to_string(source_path)
        .map_err(|e| format!("IO 에러: 파일 읽기 실패 '{}' — {}", source_path, e))?;

    // ── Step 2: lexer ────────────────────────────────────────────────────────
    let mut lexer = Lexer::new(&source);
    let tokens = lexer.tokenize().map_err(|e| format!("{}", e))?;

    // ── Step 3: parser ────────────────────────────────────────────────────────
    let mut parser = Parser::new(tokens);
    let program = parser.parse().map_err(|e| format!("{}", e))?;

    // ── Step 4: schema validation + Rust code generation ────────────────────────────────
    let rust_code = generate_rust_src(&program, source_path)?;

    // ── Step 5: output ────────────────────────────────────────────────────────
    match out_path {
        Some(path) => {
            fs::write(path, &rust_code)
                .map_err(|e| format!("IO 에러: 출력 파일 쓰기 실패 '{}' — {}", path, e))?;

            println!("✅  emit rust 완료 → {}", path);
            println!();
            println!("─── 실행 방법 ──────────────────────────────────────────────────");
            println!("  1) 새 Rust 프로젝트 생성:");
            println!("       cargo new my_analysis && cd my_analysis");
            println!("  2) Cargo.toml 에 의존성 추가:");
            println!(
                "       polars      = {{ version = \"0.53\", features = [\"lazy\", \"csv\"] }}"
            );
            println!("       encoding_rs = \"0.8\"");
            println!("  3) {} 를 src/main.rs 로 복사 후 실행:", path);
            println!("       cargo run");
            println!("────────────────────────────────────────────────────────────────");
        }
        None => println!("{}", rust_code),
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// ── internal: AST → Rust source string generation ─────────────────────────────────────────
// ─────────────────────────────────────────────────────────────────────────────

fn generate_rust_src(
    program: &Program,
    source_path: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // ── build the global schema map ──────────────────────────────────────────────────
    let mut schema_map: HashMap<String, Vec<StructField>> = HashMap::new();
    for stmt in &program.stmts {
        if let Stmt::TypeDecl { name, fields } = stmt {
            schema_map.insert(name.clone(), fields.clone());
        }
    }

    let mut out = String::new();

    // file header
    out.push_str("// ═══════════════════════════════════════════════════════════════\n");
    out.push_str("// Auto-generated by xazzLang `emit rust`\n");
    out.push_str(&format!("// Source: {}\n", source_path));
    out.push_str("//\n");
    out.push_str("// [required Cargo.toml dependencies]\n");
    out.push_str("//   polars      = { version = \"0.53\", features = [\"lazy\", \"csv\"] }\n");
    out.push_str("//   encoding_rs = \"0.8\"\n");
    if program_has_dl(program) {
        out.push_str("//   burn        = { version = \"0.21\", default-features = false, features = [\"std\", \"autodiff\"] }\n");
        out.push_str("//   burn-ndarray = { version = \"0.21\", default-features = false, features = [\"std\", \"burn-autodiff\"] }\n");
        out.push_str("//   serde_json   = \"1.0\"\n");
    }
    out.push_str("// ═══════════════════════════════════════════════════════════════\n\n");

    // imports
    out.push_str("use polars::prelude::*;\n");
    out.push_str("use encoding_rs::EUC_KR;\n");
    out.push_str("use std::io::Cursor;\n\n");

    if program_has_dl(program) {
        out.push_str(&emit_burn_imports());
        out.push('\n');
    }

    // CSV loader helper
    out.push_str(&emit_csv_loader_fn());
    out.push('\n');

    // deep-learning (Burn) model definition + training helper
    if program_has_dl(program) {
        for stmt in &program.stmts {
            if let Stmt::ModelDecl { name, layers } = stmt {
                out.push_str(&emit_dl_model_struct(name, layers));
            }
        }
        out.push_str(&emit_extract_xy_fn());
        out.push('\n');
        out.push_str(&emit_dl_metrics_fn());
        out.push('\n');
        out.push_str(&emit_dl_sweep_helpers_fn());
        out.push('\n');
    }

    // fn main()
    out.push_str("fn main() -> Result<(), Box<dyn std::error::Error>> {\n");

    // TypeDecl → schema comment block
    let has_schemas = program
        .stmts
        .iter()
        .any(|s| matches!(s, Stmt::TypeDecl { .. }));
    if has_schemas {
        out.push_str(
            "    // ── xazzLang Schema Definitions (reference comments) ─────────────────────\n",
        );
        for stmt in &program.stmts {
            if let Stmt::TypeDecl { name, fields } = stmt {
                out.push_str(&format!("    // type {} = {{\n", name));
                for f in fields {
                    out.push_str(&format!("    //   {:<14}: {}\n", f.name, f.field_type));
                }
                out.push_str("    // }\n");
            }
        }
        out.push_str(
            "    // ──────────────────────────────────────────────────────────────────\n\n",
        );
    }

    // per-variable schema tracking
    let mut var_col_types: HashMap<String, HashMap<String, String>> = HashMap::new();

    // VarDecl → LazyFrame pipeline
    for stmt in &program.stmts {
        if let Stmt::VarDecl {
            var_name,
            is_mut,
            source,
            ops,
        } = stmt
        {
            let mut_kw = if *is_mut { "mut " } else { "" };

            // determine active column type map
            let col_types: HashMap<String, String> = match source {
                PipelineSource::Load { schema_name, .. } => schema_map
                    .get(schema_name)
                    .map(|fields| {
                        fields
                            .iter()
                            .map(|f| (f.name.clone(), f.field_type.clone()))
                            .collect()
                    })
                    .unwrap_or_default(),
                PipelineSource::VarRef(src_var) => var_col_types
                    .get(src_var.as_str())
                    .cloned()
                    .unwrap_or_default(),
            };

            // schema validation
            if !col_types.is_empty() {
                for op in ops {
                    validate_op_columns(op, &col_types, &var_col_types, source_path)?;
                }
            }

            // code generation header
            out.push_str(&format!(
                "    // ── Pipeline: {}v {} ──────────────────────────────────────────\n",
                mut_kw, var_name
            ));

            // determine source
            match source {
                PipelineSource::Load {
                    file_path,
                    schema_name,
                    options,
                } => {
                    out.push_str(&format!(
                        "    // load(\"{}\"{}) :: {}\n",
                        escape(file_path),
                        load_options_comment(options),
                        schema_name
                    ));
                    let separator = match options.separator {
                        Some(sep) => format!("Some(b'\\x{:02x}')", sep),
                        None => "None".to_string(),
                    };
                    out.push_str(&format!(
                        "    let {}{} = load_csv(\"{}\", {}, {})? // :: {}\n",
                        mut_kw,
                        var_name,
                        escape(file_path),
                        options.has_header.unwrap_or(true),
                        separator,
                        schema_name
                    ));
                    out.push_str("        .lazy()\n");
                }
                PipelineSource::VarRef(src_var) => {
                    out.push_str(&format!(
                        "    let {}{} = {}.clone().lazy()\n",
                        mut_kw, var_name, src_var
                    ));
                }
            }

            // ── pipeline operator chaining ─────────────────────────────────────
            // GroupBy+agg pattern: save GroupBy, then merge with the next aggregate operator for output
            let mut has_count = false;
            let mut pending_group_col: Option<String> = None;
            // `v m = ... |> train(...)` — train is terminal (runtime returns the
            // trained model and ignores later ops), so capture it and emit the
            // real Burn training block instead of a placeholder comment.
            let mut train_terminal: Option<(&str, &TrainConfig)> = None;
            // Whether the lazy chain for this variable is currently open (i.e. the
            // last emitted line is a `.lazy()`/method call awaiting `.collect()?;`).
            // `predict()` must break the lazy chain to run Burn inference eagerly,
            // then the following ops restart a fresh lazy chain (`needs_relazy`).
            let mut chain_open = true;
            let mut needs_relazy = false;

            for op in ops {
                if needs_relazy {
                    out.push_str(&format!(
                        "    let {}{} = {}.clone().lazy()\n",
                        mut_kw, var_name, var_name
                    ));
                    chain_open = true;
                    needs_relazy = false;
                }
                match op {
                    PipelineOp::Filter(expr) => {
                        let polars_expr = to_typed_polars_expr(expr, &col_types);
                        out.push_str(&format!(
                            "        .filter({})  // |> filter({})\n",
                            polars_expr,
                            Codegen::expr_to_xzz(expr)
                        ));
                    }
                    PipelineOp::Select(cols) => {
                        let polars_cols: Vec<String> = cols
                            .iter()
                            .map(|c| format!("col(\"{}\")", escape(c)))
                            .collect();
                        out.push_str(&format!(
                            "        .select([{}])  // |> select([{}])\n",
                            polars_cols.join(", "),
                            cols.join(", ")
                        ));
                    }
                    PipelineOp::Count(None) => {
                        // count (no arg): only set the flag for row-count output
                        has_count = true;
                    }

                    // ── GroupBy storage ───────────────────────────────────────────
                    PipelineOp::GroupBy(group_col) => {
                        pending_group_col = Some(group_col.clone());
                    }

                    // ── aggregate operators: with or without GroupBy ─────────────────
                    PipelineOp::Count(Some(agg_col)) => {
                        let agg = crate::polars_text::agg_expr_to_polars(
                            crate::ir::AggKind::Count,
                            agg_col,
                        );
                        if let Some(gc) = pending_group_col.take() {
                            out.push_str(&format!(
                                "        .group_by([col(\"{}\")])\n        .agg([{}])  // |> groupBy(\"{}\") |> count(\"{}\")\n",
                                escape(&gc), agg, gc, agg_col
                            ));
                        } else {
                            out.push_str(&format!(
                                "        .select([{}])  // |> count(\"{}\")\n",
                                agg, agg_col
                            ));
                        }
                    }
                    PipelineOp::Sum(agg_col) => {
                        let agg = crate::polars_text::agg_expr_to_polars(
                            crate::ir::AggKind::Sum,
                            agg_col,
                        );
                        if let Some(gc) = pending_group_col.take() {
                            out.push_str(&format!(
                                "        .group_by([col(\"{}\")])\n        .agg([{}])  // |> groupBy(\"{}\") |> sum(\"{}\")\n",
                                escape(&gc), agg, gc, agg_col
                            ));
                        } else {
                            out.push_str(&format!(
                                "        .select([{}])  // |> sum(\"{}\")\n",
                                agg, agg_col
                            ));
                        }
                    }
                    PipelineOp::Mean(agg_col) => {
                        let agg = crate::polars_text::agg_expr_to_polars(
                            crate::ir::AggKind::Mean,
                            agg_col,
                        );
                        if let Some(gc) = pending_group_col.take() {
                            out.push_str(&format!(
                                "        .group_by([col(\"{}\")])\n        .agg([{}])  // |> groupBy(\"{}\") |> mean(\"{}\")\n",
                                escape(&gc), agg, gc, agg_col
                            ));
                        } else {
                            out.push_str(&format!(
                                "        .select([{}])  // |> mean(\"{}\")\n",
                                agg, agg_col
                            ));
                        }
                    }
                    PipelineOp::Min(agg_col) => {
                        let agg = crate::polars_text::agg_expr_to_polars(
                            crate::ir::AggKind::Min,
                            agg_col,
                        );
                        if let Some(gc) = pending_group_col.take() {
                            out.push_str(&format!(
                                "        .group_by([col(\"{}\")])\n        .agg([{}])  // |> groupBy(\"{}\") |> min(\"{}\")\n",
                                escape(&gc), agg, gc, agg_col
                            ));
                        } else {
                            out.push_str(&format!(
                                "        .select([{}])  // |> min(\"{}\")\n",
                                agg, agg_col
                            ));
                        }
                    }
                    PipelineOp::Max(agg_col) => {
                        let agg = crate::polars_text::agg_expr_to_polars(
                            crate::ir::AggKind::Max,
                            agg_col,
                        );
                        if let Some(gc) = pending_group_col.take() {
                            out.push_str(&format!(
                                "        .group_by([col(\"{}\")])\n        .agg([{}])  // |> groupBy(\"{}\") |> max(\"{}\")\n",
                                escape(&gc), agg, gc, agg_col
                            ));
                        } else {
                            out.push_str(&format!(
                                "        .select([{}])  // |> max(\"{}\")\n",
                                agg, agg_col
                            ));
                        }
                    }

                    // ── sorting / slicing ────────────────────────────────────────
                    PipelineOp::OrderBy {
                        col: sort_col,
                        desc,
                    } => {
                        out.push_str(&format!(
                            "        .sort([\"{}\"], SortMultipleOptions::default().with_order_descending({}))  // |> orderBy(\"{}\", desc: {})\n",
                            escape(sort_col), desc, sort_col, desc
                        ));
                    }
                    PipelineOp::Take(n) => {
                        out.push_str(&format!("        .limit({})  // |> take({})\n", n, n));
                    }

                    // ── Null handling ──────────────────────────────────────────────
                    PipelineOp::DropNull(drop_col) => {
                        out.push_str(&format!(
                            "        .drop_nulls(Some(vec![col(\"{}\")]))  // |> dropNull(\"{}\")\n",
                            escape(drop_col), drop_col
                        ));
                    }
                    PipelineOp::FillNull {
                        col: fill_col,
                        value,
                    } => {
                        let lit_str = crate::polars_text::fill_value_to_polars(value, fill_col);
                        out.push_str(&format!(
                            "        .with_columns([col(\"{}\").fill_null({})])  // |> fillNull(\"{}\", ...)\n",
                            escape(fill_col), lit_str, fill_col
                        ));
                    }

                    // ── v0.16+ / v0.21 Join ────────────────────────────────────
                    PipelineOp::Join {
                        other,
                        left_on,
                        right_on,
                        how,
                    } => {
                        let left_cols: Vec<String> = left_on
                            .iter()
                            .map(|k| format!("col(\"{}\")", escape(k)))
                            .collect();
                        let right_cols: Vec<String> = right_on
                            .iter()
                            .map(|k| format!("col(\"{}\")", escape(k)))
                            .collect();
                        out.push_str(&format!(
                            "        .join(\n            {}.clone().lazy(),\n            [{}],\n            [{}],\n            JoinArgs::new({}),\n        )  // |> join({}, left_on: {:?}, right_on: {:?})\n",
                            other,
                            left_cols.join(", "),
                            right_cols.join(", "),
                            how.as_polars_str(),
                            other,
                            left_on,
                            right_on
                        ));
                    }

                    // ── v0.16+ WithColumn ──────────────────────────────────────
                    PipelineOp::WithColumn {
                        name: col_name,
                        expr,
                    } => {
                        let polars_expr = to_typed_polars_expr(expr, &col_types);
                        out.push_str(&format!(
                            "        .with_columns([{}.alias(\"{}\")])  // |> withColumn(\"{}\", {})\n",
                            polars_expr,
                            escape(col_name),
                            col_name,
                            Codegen::expr_to_xzz(expr)
                        ));
                    }

                    // ── Chart: unsupported by emitter (runtime only) ──────────────
                    PipelineOp::Chart(config) => {
                        out.push_str(&format!(
                            "        // |> chart {{ type: {} }}  →  [xazz:chart] JSON output\n",
                            config.chart_type.as_str()
                        ));
                    }

                    // ── v0.20 Cast ─────────────────────────────────────────────
                    PipelineOp::Cast {
                        col: cast_col,
                        to_type,
                    } => {
                        let polars_type = crate::polars_text::cast_dtype_to_polars(to_type);
                        out.push_str(&format!(
                            "        .with_columns([col(\"{}\").cast({})])  // |> cast(\"{}\", \"{}\")\n",
                            escape(cast_col), polars_type, cast_col, to_type
                        ));
                    }

                    // ── Rename ─────────────────────────────────────────────────
                    PipelineOp::Rename { old_name, new_name } => {
                        out.push_str(&format!(
                            "        .rename([\"{}\"], [\"{}\"], false)  // |> rename(\"{}\", \"{}\")\n",
                            old_name, new_name, old_name, new_name
                        ));
                    }

                    // ── Replace ────────────────────────────────────────────────
                    PipelineOp::Replace {
                        col: rep_col,
                        from,
                        to,
                    } => {
                        out.push_str(&format!(
                            "        .with_columns([col(\"{}\").str().replace(lit(\"{}\"), lit(\"{}\"), false).alias(\"{}\")])  // |> replace(\"{}\", \"{}\", \"{}\")\n",
                            escape(rep_col), escape(from), escape(to), escape(rep_col), escape(rep_col), escape(from), escape(to)
                        ));
                    }

                    // ── v0.22 sample(n) / sample(n, seed: 42) ──────────────────
                    PipelineOp::Sample { n, seed } => match seed {
                        Some(s) => {
                            out.push_str(&format!(
                                "        .collect()?.sample_n_literal({}, false, false, Some({}))?.lazy()  // |> sample({}, seed: {})\n",
                                n, s, n, s
                            ));
                        }
                        None => {
                            out.push_str(&format!(
                                "        .collect()?.sample_n_literal({}, false, false, None)?.lazy()  // |> sample({})\n",
                                n, n
                            ));
                        }
                    },

                    // ── v0.22 median / variance / std aggregates ─────────────────────
                    PipelineOp::Median(agg_col) => {
                        let agg = crate::polars_text::agg_expr_to_polars(
                            crate::ir::AggKind::Median,
                            agg_col,
                        );
                        if let Some(gc) = pending_group_col.take() {
                            out.push_str(&format!(
                                "        .group_by([col(\"{}\")])\n        .agg([{}])  // |> groupBy(\"{}\") |> median(\"{}\")\n",
                                escape(&gc), agg, gc, agg_col
                            ));
                        } else {
                            out.push_str(&format!(
                                "        .select([{}])  // |> median(\"{}\")\n",
                                agg, agg_col
                            ));
                        }
                    }
                    PipelineOp::Variance(agg_col) => {
                        let agg = crate::polars_text::agg_expr_to_polars(
                            crate::ir::AggKind::Variance,
                            agg_col,
                        );
                        if let Some(gc) = pending_group_col.take() {
                            out.push_str(&format!(
                                "        .group_by([col(\"{}\")])\n        .agg([{}])  // |> groupBy(\"{}\") |> variance(\"{}\")\n",
                                escape(&gc), agg, gc, agg_col
                            ));
                        } else {
                            out.push_str(&format!(
                                "        .select([{}])  // |> variance(\"{}\")\n",
                                agg, agg_col
                            ));
                        }
                    }
                    PipelineOp::Std(agg_col) => {
                        let agg = crate::polars_text::agg_expr_to_polars(
                            crate::ir::AggKind::Std,
                            agg_col,
                        );
                        if let Some(gc) = pending_group_col.take() {
                            out.push_str(&format!(
                                "        .group_by([col(\"{}\")])\n        .agg([{}])  // |> groupBy(\"{}\") |> std(\"{}\")\n",
                                escape(&gc), agg, gc, agg_col
                            ));
                        } else {
                            out.push_str(&format!(
                                "        .select([{}])  // |> std(\"{}\")\n",
                                agg, agg_col
                            ));
                        }
                    }
                    // ── v0.23 agg([...]) — multi-aggregation in one pass ─────────────
                    PipelineOp::Agg(specs) => {
                        let aggs: Vec<String> = specs
                            .iter()
                            .map(|s| {
                                crate::polars_text::agg_expr_to_polars_aliased(
                                    crate::ir::AggKind::from(s.func),
                                    &s.col,
                                )
                            })
                            .collect();
                        let joined = aggs.join(", ");
                        if let Some(gc) = pending_group_col.take() {
                            out.push_str(&format!(
                                "        .group_by([col(\"{}\")])\n        .agg([{}])  // |> groupBy(\"{}\") |> agg([...])\n",
                                escape(&gc), joined, gc
                            ));
                        } else {
                            out.push_str(&format!(
                                "        .select([{}])  // |> agg([...])\n",
                                joined
                            ));
                        }
                    }
                    // ── v0.5 deep-learning operator: terminal train() emits a real Burn block ──
                    PipelineOp::Train { model_name, config } => {
                        train_terminal = Some((model_name.as_str(), config));
                        break;
                    }
                    PipelineOp::Predict { model_var, as_col } => {
                        // Break the pending lazy chain so the frame is materialized
                        // and the trained checkpoint can be run through Burn.
                        if chain_open {
                            out.push_str("        .collect()?;\n");
                            chain_open = false;
                        }
                        match find_model_binding(program, model_var) {
                            Some(binding) if !binding.config.is_sweep() => {
                                out.push_str(&emit_dl_predict_call(
                                    var_name,
                                    binding.model_name,
                                    &binding.config.target,
                                    as_col.as_deref(),
                                    binding.embedding_input,
                                ));
                            }
                            Some(_) => {
                                out.push_str(&format!(
                                    "    // |> predict({model_var}, as: ...) — 스윕 모델은 예측 대상 체크포인트가 모호해 emit 미지원 (xazz 실행 시 반영)\n"
                                ));
                            }
                            None => {
                                out.push_str(&format!(
                                    "    // |> predict({model_var}, as: ...) — 같은 프로그램에서 학습된 모델 변수를 찾을 수 없어 emit 미지원 (xazz 실행 시 반영)\n"
                                ));
                            }
                        }
                        needs_relazy = true;
                    }
                    // ── v0.6 withDp — in emit rust, delegate DP injection to runtime ──
                    PipelineOp::WithDp(args) => {
                        out.push_str(&format!(
                            "        // |> withDp(epsilon: {}, mechanism: {})  → DP noise injected at xazz execution\n",
                            args.epsilon,
                            args.mechanism.as_str()
                        ));
                    }
                    // ── v0.3.2 save — writes the result artifact at xazz execution (issue #52) ──
                    PipelineOp::Save { path, format } => {
                        out.push_str(&format!(
                            "        // |> save(\"{}\", format: \"{}\")  → result artifact written at xazz execution\n",
                            path, format.as_str()
                        ));
                    }
                }
            }

            // collect (predict may have already closed the chain and produced the
            // frame eagerly, in which case there is no open lazy chain to close)
            if chain_open {
                out.push_str("        .collect()?;\n");
            }

            // terminal train(): the collected frame is the training data and the
            // variable itself denotes the trained model (mirrors the runtime).
            if let Some((model_name, config)) = train_terminal {
                let embedding_input = program.stmts.iter().any(|s| {
                    matches!(s, Stmt::ModelDecl { name, layers }
                        if name == model_name
                            && matches!(layers.first(), Some(LayerKind::Embedding { .. })))
                });
                warn_emit_output_dim(program, model_name);
                out.push_str(&emit_dl_train_call(
                    var_name,
                    model_name,
                    config,
                    embedding_input,
                ));
                out.push('\n');
                continue;
            }

            // result output
            if has_count {
                out.push_str(&format!(
                    "    println!(\"[{}] count = {{}}\", {}.height());\n",
                    var_name, var_name
                ));
            } else {
                out.push_str(&format!(
                    "    println!(\"[{}]\\n{{}}\", {});\n",
                    var_name, var_name
                ));
            }
            out.push('\n');

            if !col_types.is_empty() {
                var_col_types.insert(var_name.clone(), col_types);
            }
        }
    }

    // deep-learning (Burn) training statement execution
    for stmt in &program.stmts {
        if let Stmt::TrainStmt {
            source_var,
            model_name,
            config,
        } = stmt
        {
            let embedding_input = program.stmts.iter().any(|s| {
                matches!(s, Stmt::ModelDecl { name, layers }
                    if name == model_name
                        && matches!(layers.first(), Some(LayerKind::Embedding { .. })))
            });
            warn_emit_output_dim(program, model_name);
            out.push_str(&emit_dl_train_call(
                source_var,
                model_name,
                config,
                embedding_input,
            ));
        }
    }

    out.push_str("    Ok(())\n");
    out.push_str("}\n");

    Ok(out)
}

// ─────────────────────────────────────────────────────────────────────────────
// ── deep-learning (Burn) code generation (v0.4) ─────────────────────────────
// ─────────────────────────────────────────────────────────────────────────────

/// Returns whether the script contains DL statements (ModelDecl / TrainStmt /
/// a `v m = ... |> train(...)` VarDecl whose ops include a train op).
fn program_has_dl(program: &Program) -> bool {
    program.stmts.iter().any(|s| match s {
        Stmt::ModelDecl { .. } | Stmt::TrainStmt { .. } => true,
        Stmt::VarDecl { ops, .. } => ops.iter().any(|op| matches!(op, PipelineOp::Train { .. })),
        _ => false,
    })
}

/// Burn import block.
fn emit_burn_imports() -> String {
    r#"use burn::{
    backend::Autodiff,
    module::Module,
    nn::{Linear, LinearConfig, PaddingConfig1d, conv::{Conv1d, Conv1dConfig}, Embedding, EmbeddingConfig},
    optim::{AdamConfig, GradientsParams, Optimizer},
    record::{FullPrecisionSettings, PrettyJsonFileRecorder},
    tensor::{
        Device, Tensor, TensorData,
        activation::{relu, sigmoid, softmax, tanh},
        backend::Backend,
    },
};
use burn_ndarray::NdArray;

type TrainBackend = Autodiff<NdArray<f32>>;
"#
    .to_string()
}

/// One compiled forward op from the DSL layer chain — activation calls attach to
/// the immediately preceding Dense/Conv1d layer (same semantics as `dl.rs`).
enum DlLayer {
    /// Dense(units, activation)
    Dense(usize, String),
    /// Conv1d(out_channels, kernel_size, activation)
    Conv1d(usize, usize, String),
    /// Embedding(vocab, embed_dim, activation)
    Embedding(EmbeddingVocab, usize, String),
}

/// Normalizes a DSL layer chain into ordered Dense/Conv1d specs.
fn dl_layer_specs(layers: &[LayerKind]) -> Vec<DlLayer> {
    let mut specs: Vec<DlLayer> = Vec::new();
    for layer in layers {
        match layer {
            LayerKind::Dense(n) if *n > 0 => specs.push(DlLayer::Dense(*n, String::from("None"))),
            LayerKind::Dense(_) => {}
            LayerKind::Conv1d {
                out_channels,
                kernel_size,
            } if *out_channels > 0 && *kernel_size > 0 => specs.push(DlLayer::Conv1d(
                *out_channels,
                *kernel_size,
                String::from("None"),
            )),
            LayerKind::Conv1d { .. } => {}
            LayerKind::Embedding { vocab, embed_dim } if vocab.is_valid() && *embed_dim > 0 => {
                specs.push(DlLayer::Embedding(
                    vocab.clone(),
                    *embed_dim,
                    String::from("None"),
                ))
            }
            LayerKind::Embedding { .. } => {}
            LayerKind::ReLU => set_dl_act(&mut specs, "relu"),
            LayerKind::Sigmoid => set_dl_act(&mut specs, "sigmoid"),
            LayerKind::Tanh => set_dl_act(&mut specs, "tanh"),
            LayerKind::Softmax => set_dl_act(&mut specs, "softmax"),
            LayerKind::Dropout(_) | LayerKind::BatchNorm => {}
        }
    }
    specs
}

fn set_dl_act(specs: &mut [DlLayer], act: &str) {
    if let Some(last) = specs.last_mut() {
        match last {
            DlLayer::Dense(_, a) | DlLayer::Conv1d(_, _, a) | DlLayer::Embedding(_, _, a) => {
                *a = act.to_string()
            }
        }
    }
}

/// Returns the declared layers of a model, if any.
fn find_model_layers<'a>(program: &'a Program, model_name: &str) -> Option<&'a [LayerKind]> {
    program.stmts.iter().find_map(|stmt| match stmt {
        Stmt::ModelDecl { name, layers } if name == model_name => Some(layers.as_slice()),
        _ => None,
    })
}

/// Emit-stage counterpart to `checker::warn_model_output_dim`.
///
/// A scalar regression target needs the model's final layer to be `Dense(1)`;
/// a chain ending in Conv1d/Embedding (or `Dense(n)` with `n != 1`) makes the
/// generated `forward` return several values, so training broadcasts the target
/// and `predict()` later fails. The runtime is already fail-closed; this surfaces
/// the same problem when `xazz emit rust` runs, before the generated program is
/// compiled. `None` means the chain ends in `Dense(1)` (or has no usable layer).
fn emit_output_dim_warning(model_name: &str, layers: &[LayerKind]) -> Option<String> {
    let specs = dl_layer_specs(layers);
    if specs.is_empty() || matches!(specs.last(), Some(DlLayer::Dense(1, _))) {
        return None;
    }
    Some(if is_korean() {
        format!(
            "emit rust : 모델 '{model_name}' 의 마지막 레이어가 Dense(1) 이 아닙니다. 스칼라 타겟 회귀에서는 학습이 타겟을 브로드캐스트하고 predict() 가 실패합니다. 마지막에 Dense(1) 을 추가하세요."
        )
    } else {
        format!(
            "emit rust : model '{model_name}' does not end with Dense(1); a single scalar target will be broadcast and predict() will fail. Add a final Dense(1)."
        )
    })
}

/// Looks up a model's layers and prints the emit-stage output-dimension warning
/// (if any) to stderr. Shared by the `TrainStmt` and VarDecl `train()` paths.
fn warn_emit_output_dim(program: &Program, model_name: &str) {
    if let Some(layers) = find_model_layers(program, model_name)
        && let Some(msg) = emit_output_dim_warning(model_name, layers)
    {
        eprintln!("[xazz] {msg}");
    }
}

/// `model <Name> { ... }` → Burn nn module struct + new() + forward().
fn emit_dl_model_struct(name: &str, layers: &[LayerKind]) -> String {
    let specs = dl_layer_specs(layers);
    if specs.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    out.push_str(&format!(
        "#[derive(Module, Debug)]\nstruct {}<B: Backend> {{\n",
        name
    ));
    for (i, spec) in specs.iter().enumerate() {
        match spec {
            DlLayer::Dense(units, _) => {
                out.push_str(&format!("    layer{i}: Linear<B>,  // Dense({units})\n"))
            }
            DlLayer::Conv1d(c, k, _) => {
                out.push_str(&format!("    conv{i}: Conv1d<B>,  // Conv1d({c}, {k})\n"))
            }
            DlLayer::Embedding(v, d, _) => out.push_str(&format!(
                "    embed{i}: Embedding<B>,  // Embedding({}, {d})\n    #[module(skip)]\n    embed_vocab{i}: Vec<usize>,\n",
                v.display()
            )),
        }
    }
    out.push_str("}\n\n");

    out.push_str(&format!(
        "impl<B: Backend> {name}<B> {{\n    fn new(device: &Device<B>, input_dim: usize) -> Self {{\n"
    ));
    // Per-column vocabularies need `input_dim` to replicate a shared vocab; the
    // generated module keeps the expanded per-column list alongside the single
    // combined embedding table (disjoint row ranges, one per column).
    for (i, spec) in specs.iter().enumerate() {
        if let DlLayer::Embedding(v, _, _) = spec {
            match v {
                EmbeddingVocab::Shared(size) => out.push_str(&format!(
                    "        let embed_vocab{i}: Vec<usize> = vec![{size}; input_dim];\n"
                )),
                EmbeddingVocab::PerColumn(sizes) => {
                    let list = sizes
                        .iter()
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    out.push_str(&format!(
                        "        let embed_vocab{i}: Vec<usize> = vec![{list}];\n        assert_eq!(embed_vocab{i}.len(), input_dim, \"Embedding per-column vocab length must match the feature count\");\n"
                    ));
                }
            }
        }
    }
    out.push_str("        Self {\n");
    let mut cur = "input_dim".to_string();
    for (i, spec) in specs.iter().enumerate() {
        match spec {
            DlLayer::Dense(units, _) => {
                out.push_str(&format!(
                    "            layer{i}: LinearConfig::new({cur}, {units}).init(device),\n"
                ));
                cur = units.to_string();
            }
            DlLayer::Conv1d(c, k, _) => {
                out.push_str(&format!(
                    "            conv{i}: Conv1dConfig::new(1, {c}, {k}).with_padding(PaddingConfig1d::Same).init(device),\n"
                ));
                cur = format!("{c} * {cur}");
            }
            DlLayer::Embedding(_, d, _) => {
                out.push_str(&format!(
                    "            embed{i}: EmbeddingConfig::new(embed_vocab{i}.iter().sum::<usize>(), {d}).init(device),\n            embed_vocab{i},\n"
                ));
                cur = format!("{d} * {cur}");
            }
        }
    }
    out.push_str("        }\n    }\n\n");

    out.push_str("    fn forward(&self, x: Tensor<B, 2>) -> Tensor<B, 2> {\n");
    for (i, spec) in specs.iter().enumerate() {
        match spec {
            DlLayer::Dense(_, act) => {
                out.push_str(&format!("        let x = self.layer{i}.forward(x);\n"));
                emit_dl_act(&mut out, act);
            }
            DlLayer::Conv1d(_, _, act) => {
                out.push_str("        let [batch, len] = x.dims();\n");
                out.push_str(&format!(
                    "        let x = self.conv{i}.forward(x.reshape([batch, 1, len]));\n"
                ));
                out.push_str("        let [b, c, l] = x.dims();\n");
                out.push_str("        let x = x.reshape([b, c * l]);\n");
                emit_dl_act(&mut out, act);
            }
            DlLayer::Embedding(_, _, act) => {
                // Each column j uses its own row range in the combined table:
                // index = clamp(value, 0, vocab_j - 1) + offset_j.
                out.push_str("        let [_, len] = x.dims();\n");
                out.push_str(&format!("        let vocabs = &self.embed_vocab{i};\n"));
                out.push_str("        let mut offset = 0usize;\n");
                out.push_str(
                    "        let mut cols: Vec<Tensor<B, 2>> = Vec::with_capacity(len);\n",
                );
                out.push_str("        for (j, &v) in vocabs.iter().enumerate() {\n");
                out.push_str("            let idx = x.clone().narrow(1, j, 1).clamp(0.0, v.saturating_sub(1) as f32) + offset as f32;\n");
                out.push_str("            cols.push(idx);\n");
                out.push_str("            offset += v;\n");
                out.push_str("        }\n");
                out.push_str("        let idx = Tensor::cat(cols, 1).int();\n");
                out.push_str(&format!("        let x = self.embed{i}.forward(idx);\n"));
                out.push_str("        let [b, l, d] = x.dims();\n");
                out.push_str("        let x = x.reshape([b, l * d]);\n");
                emit_dl_act(&mut out, act);
            }
        }
    }
    out.push_str("        x\n    }\n}\n\n");

    out
}

fn emit_dl_act(out: &mut String, act: &str) {
    match act {
        "relu" => out.push_str("        let x = relu(x);\n"),
        "sigmoid" => out.push_str("        let x = sigmoid(x);\n"),
        "tanh" => out.push_str("        let x = tanh(x);\n"),
        "softmax" => out.push_str("        let x = softmax(x, 1);\n"),
        _ => {}
    }
}

/// Column → f32 tensor data extraction helper
/// (Polars DataFrame → (x_flat, y_flat, feature_count, feature_names)).
fn emit_extract_xy_fn() -> String {
    r#"/// Extracts numeric columns (excluding the target) as features and the target column as the label.
/// The feature names are returned so `predict()` can persist and later replay the
/// exact training feature order and standardization.
fn extract_xy(
    df: &DataFrame,
    target: &str,
) -> Result<(Vec<f32>, Vec<f32>, usize, Vec<String>), Box<dyn std::error::Error>> {
    let names = df.get_column_names();
    let mut features: Vec<String> = Vec::new();
    for name in names {
        if name.as_str() == target {
            continue;
        }
        if let Ok(col) = df.column(name.as_str()) {
            if matches!(
                col.dtype(),
                DataType::Float64
                    | DataType::Float32
                    | DataType::Int64
                    | DataType::Int32
                    | DataType::Int16
                    | DataType::Int8
                    | DataType::UInt64
                    | DataType::UInt32
                    | DataType::UInt16
                    | DataType::UInt8
            ) {
                features.push(name.to_string());
            }
        }
    }
    if features.is_empty() {
        return Err("학습 가능한 숫자형 특성 컬럼이 없습니다.".into());
    }
    let n = df.height();
    let mut xs = Vec::with_capacity(n * features.len());
    let mut ys = Vec::with_capacity(n);
    for i in 0..n {
        for f in &features {
            let v = df.column(f)?.get(i).unwrap_or(AnyValue::Float64(f64::NAN));
            xs.push(xz_anyvalue_f32(v));
        }
        let v = df.column(target)?.get(i).unwrap_or(AnyValue::Float64(f64::NAN));
        ys.push(xz_anyvalue_f32(v));
    }
    Ok((xs, ys, features.len(), features))
}

/// AnyValue → f32 (non-numeric is NaN).
fn xz_anyvalue_f32(v: AnyValue) -> f32 {
    match v {
        AnyValue::Float64(x) => x as f32,
        AnyValue::Float32(x) => x,
        AnyValue::Int64(x) => x as f32,
        AnyValue::Int32(x) => x as f32,
        AnyValue::Int16(x) => x as f32,
        AnyValue::Int8(x) => x as f32,
        AnyValue::UInt64(x) => x as f32,
        AnyValue::UInt32(x) => x as f32,
        AnyValue::UInt16(x) => x as f32,
        AnyValue::UInt8(x) => x as f32,
        _ => f32::NAN,
    }
}

/// Slices a batch into (feature tensor, label tensor).
fn make_both_tensors(
    xs: &[f32],
    ys: &[f32],
    feature_count: usize,
    start: usize,
    end: usize,
    device: &Device<TrainBackend>,
) -> (Tensor<TrainBackend, 2>, Tensor<TrainBackend, 2>) {
    let b = end - start;
    let mut xv = Vec::with_capacity(b * feature_count);
    let mut yv = Vec::with_capacity(b);
    for i in start..end {
        for j in 0..feature_count {
            xv.push(xs[i * feature_count + j]);
        }
        yv.push(ys[i]);
    }
    let x = Tensor::<TrainBackend, 2>::from_data(TensorData::new(xv, [b, feature_count]), device);
    let y = Tensor::<TrainBackend, 2>::from_data(TensorData::new(yv, [b, 1]), device);
    (x, y)
}
"#
    .to_string()
}

/// Emitted regression-metric helper used by the sweep emitter to select the best
/// combination by `metric:` (D3), mirroring `xazz-exec`'s `regression_metrics`.
fn emit_dl_metrics_fn() -> String {
    r#"/// Mean absolute error and R² over a prediction/target slice (D3 sweep metrics).
///
/// R² is `1 - SS_res / SS_tot`; zero-variance targets report `0.0` (undefined)
/// rather than NaN so sweep ranking stays well-defined.
fn xz_regression_metrics(preds: &[f32], targets: &[f32]) -> (f64, f64) {
    let n = preds.len().min(targets.len());
    if n == 0 {
        return (f64::NAN, f64::NAN);
    }
    let mut abs_sum = 0.0f64;
    let mut mean_t = 0.0f64;
    for i in 0..n {
        abs_sum += (preds[i] as f64 - targets[i] as f64).abs();
        mean_t += targets[i] as f64;
    }
    let mae = abs_sum / n as f64;
    mean_t /= n as f64;

    let mut ss_res = 0.0f64;
    let mut ss_tot = 0.0f64;
    for i in 0..n {
        let p = preds[i] as f64;
        let t = targets[i] as f64;
        ss_res += (t - p).powi(2);
        ss_tot += (t - mean_t).powi(2);
    }
    let r2 = if ss_tot > 0.0 {
        1.0 - ss_res / ss_tot
    } else {
        0.0
    };
    (mae, r2)
}
"#
    .to_string()
}

/// Emitted sweep-result row + comparator used by the sweep emitter to order and
/// filter the reported combinations by `sort:`/`tiebreak:`/`top:` (D3), mirroring
/// `xazz-exec`'s `SweepReport::compare`.
fn emit_dl_sweep_helpers_fn() -> String {
    r#"/// A single hyperparameter-sweep result (D3), used for report ordering.
#[derive(Clone)]
struct XzSweepRow {
    epochs: usize,
    lr: f64,
    batch_size: usize,
    score: f64,
    final_train_loss: f64,
    final_val_loss: Option<f64>,
    metric_value: f64,
    selected: bool,
}

/// Compares two rows on one sweep axis (D3); `metric` is a pseudo-axis handled
/// by the caller, so it compares equal here.
fn xz_sweep_axis(ax: &str, a: &XzSweepRow, b: &XzSweepRow) -> std::cmp::Ordering {
    match ax {
        "epochs" => a.epochs.cmp(&b.epochs),
        "lr" => a.lr.partial_cmp(&b.lr).unwrap_or(std::cmp::Ordering::Equal),
        "batch" => a.batch_size.cmp(&b.batch_size),
        _ => std::cmp::Ordering::Equal,
    }
}

/// Orders sweep rows by `sort` then the `tiebreak` axes (D3). `metric` sorts
/// best-first (ascending score); axis sorts ascend. Explicit tiebreak axes are
/// tried first (deduped, excluding the sort axis), then the canonical remaining
/// axes — mirroring `SweepReport::compare`.
fn xz_sweep_compare(
    a: &XzSweepRow,
    b: &XzSweepRow,
    sort: &str,
    tiebreak: &[&str],
) -> std::cmp::Ordering {
    let primary = if sort == "metric" {
        a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal)
    } else {
        xz_sweep_axis(sort, a, b)
    };
    let mut fallback: Vec<&str> = Vec::with_capacity(3);
    for &t in tiebreak {
        if t != "metric" && t != sort && !fallback.contains(&t) {
            fallback.push(t);
        }
    }
    for ax in ["epochs", "lr", "batch"] {
        if ax != sort && !fallback.contains(&ax) {
            fallback.push(ax);
        }
    }
    let mut ord = primary;
    for ax in fallback {
        ord = ord.then_with(|| xz_sweep_axis(ax, a, b));
    }
    ord
}
"#
    .to_string()
}

/// `run <src> |> train(<Model>, ...)` → main() training code block.
fn emit_dl_train_call(
    source_var: &str,
    model_name: &str,
    config: &TrainConfig,
    embedding_input: bool,
) -> String {
    let target = &config.target;
    let epochs = config.epochs.max(1);
    let lr = config.learning_rate;
    let batch_size = config.batch_size.unwrap_or(DEFAULT_BATCH_SIZE).max(1);
    let val_split = config
        .validation_split
        .unwrap_or(0.0)
        .clamp(0.0, MAX_VALIDATION_SPLIT);

    // Embedding-first models consume raw category indices; skip z-score normalization.
    let xs_norm = if embedding_input {
        r#"        // Embedding input: keep raw category indices (no z-score standardization)
        let xs: Vec<f32> = xs
            .iter()
            .map(|&v| if v.is_finite() { v } else { 0.0 })
            .collect();"#
            .to_string()
    } else {
        r#"        let xs: Vec<f32> = xs
            .iter()
            .enumerate()
            .map(|(idx, v)| {
                let col = idx % feature_count;
                let v = v as f64;
                let v = if !v.is_finite() { fmean[col] } else { v };
                ((v - fmean[col]) / fstd[col]) as f32
            })
            .collect();"#
            .to_string()
    };

    if config.is_sweep() {
        return emit_dl_sweep_call(source_var, model_name, config, &xs_norm, val_split);
    }

    format!(
        r#"    // ── Deep Learning: run {source_var} |> train({model_name}, target: "{target}") ──────
    {{
        let (xs, ys, feature_count, feature_names) = extract_xy(&{source_var}, "{target}")?;
        let n = ys.len();
        if n == 0 {{
            return Err("학습 데이터가 비어 있습니다.".into());
        }}

        // NaN → mean imputation + standardization (per-feature z-score)
        let mut fmean = vec![0f64; feature_count];
        let mut fstd = vec![1f64; feature_count];
        for j in 0..feature_count {{
            let (mut s, mut c) = (0f64, 0usize);
            for i in 0..n {{
                let v = xs[i * feature_count + j] as f64;
                if v.is_finite() {{ s += v; c += 1; }}
            }}
            fmean[j] = if c > 0 {{ s / c as f64 }} else {{ 0.0 }};
        }}
        for j in 0..feature_count {{
            let (mut s, mut c) = (0f64, 0usize);
            for i in 0..n {{
                let d = xs[i * feature_count + j] as f64 - fmean[j];
                if d.is_finite() {{ s += d * d; c += 1; }}
            }}
            fstd[j] = if c > 1 {{ (s / (c - 1) as f64).max(1e-8).sqrt() }} else {{ 1.0 }};
        }}
{xs_norm}
        let tmean: f64 = {{
            let (mut s, mut c) = (0f64, 0usize);
            for &t in &ys {{ if t.is_finite() {{ s += t as f64; c += 1; }} }}
            if c > 0 {{ s / c as f64 }} else {{ 0.0 }}
        }};
        let ys: Vec<f32> = ys
            .iter()
            .map(|&t| if t.is_finite() {{ t as f32 }} else {{ tmean as f32 }})
            .collect();

        let device: Device<TrainBackend> = Default::default();
        let mut model = {model_name}::<TrainBackend>::new(&device, feature_count);
        let mut optim = AdamConfig::new().init::<TrainBackend, _>();

        let train_n = n - ((n as f64 * {val_split}) as usize);
        for epoch in 0..{epochs} {{
            let mut loss_sum = 0f32;
            let mut steps = 0usize;
            for b in 0..((train_n + {batch_size} - 1) / {batch_size}) {{
                let start = b * {batch_size};
                let end = ((b + 1) * {batch_size}).min(train_n);
                let (x, y) = make_both_tensors(&xs, &ys, feature_count, start, end, &device);
                let out = model.forward(x);
                let loss = ((out - y).powf_scalar(2.0)).mean();
                let lv = loss.clone().into_data().to_vec::<f32>().unwrap_or_default()[0];
                let grads = loss.backward();
                let grads = GradientsParams::from_grads(grads, &model);
                model = optim.step({lr}, model, grads);
                loss_sum += lv;
                steps += 1;
            }}
            println!(
                "[Epoch {{epoch:>3}}/{{}}]  train_loss(MSE) = {{:.6}}",
                {epochs},
                if steps > 0 {{ loss_sum / steps as f32 }} else {{ 0.0 }}
            );
        }}

        let valid = model.valid();
        let recorder = PrettyJsonFileRecorder::<FullPrecisionSettings>::new();
        std::fs::create_dir_all("checkpoints")?;
        valid.save_file(&format!("checkpoints/{model_name}"), &recorder)?;
        println!("[xazz] ✅ 체크포인트 저장 → checkpoints/{model_name}.json");

        // Persist the feature order + z-score statistics predict() must replay
        // so standalone emitted code reproduces the training preprocessing.
        let stats = serde_json::json!({{
            "feature_names": feature_names,
            "fmean": fmean,
            "fstd": fstd,
        }});
        std::fs::write(
            &format!("checkpoints/{model_name}.stats.json"),
            serde_json::to_string_pretty(&stats)?,
        )?;
    }}
"#
    )
}

/// A model variable bound by `v <var> = ... |> train(<Model>, ...)`.
struct ModelBinding<'a> {
    model_name: &'a str,
    config: &'a TrainConfig,
    /// Whether the model starts with an `Embedding` layer (raw category input).
    embedding_input: bool,
}

/// Resolves the trained-model variable referenced by `predict(model_var, ...)`
/// to the model declaration and training config that produced it.
fn find_model_binding<'a>(program: &'a Program, var: &str) -> Option<ModelBinding<'a>> {
    for stmt in &program.stmts {
        if let Stmt::VarDecl { var_name, ops, .. } = stmt
            && var_name == var
        {
            for op in ops {
                if let PipelineOp::Train { model_name, config } = op {
                    let embedding_input = program.stmts.iter().any(|s| {
                        matches!(s, Stmt::ModelDecl { name, layers }
                            if name == model_name
                                && matches!(layers.first(), Some(LayerKind::Embedding { .. })))
                    });
                    return Some(ModelBinding {
                        model_name,
                        config,
                        embedding_input,
                    });
                }
            }
        }
    }
    None
}

/// `data |> predict(<model_var>, as: "col")` → eager inference block that loads
/// the checkpoint + normalization sidecar written by the training emit, rebuilds
/// the model, runs the forward pass, and appends the prediction column.
fn emit_dl_predict_call(
    frame_var: &str,
    model_name: &str,
    target: &str,
    as_col: Option<&str>,
    embedding_input: bool,
) -> String {
    let out_col = as_col
        .map(str::to_string)
        .unwrap_or_else(|| format!("{target}_pred"));
    let raw_input = if embedding_input { "true" } else { "false" };

    format!(
        r#"    // ── Deep Learning (predict): {frame_var} |> predict({model_name}, as: "{out_col}") ──────
    let {frame_var} = {{
        let stats_raw = std::fs::read_to_string("checkpoints/{model_name}.stats.json")?;
        let stats: serde_json::Value = serde_json::from_str(&stats_raw)?;
        let fmean: Vec<f64> = stats["fmean"]
            .as_array()
            .map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.0)).collect())
            .unwrap_or_default();
        let fstd: Vec<f64> = stats["fstd"]
            .as_array()
            .map(|a| a.iter().map(|v| v.as_f64().filter(|s| *s != 0.0).unwrap_or(1.0)).collect())
            .unwrap_or_default();
        let feature_names: Vec<String> = stats["feature_names"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let feature_count = feature_names.len();
        if feature_count == 0 {{
            return Err("모델에 특성 정보가 없습니다. 먼저 train()으로 학습하세요.".into());
        }}
        let n = {frame_var}.height();
        if n == 0 {{
            return Err("예측할 데이터가 비어 있습니다.".into());
        }}

        // Same preprocessing as training: feature order from the stats sidecar,
        // z-score for dense models, raw category indices for Embedding-first models.
        let raw_input = {raw_input};
        let mut xs: Vec<f32> = Vec::with_capacity(n * feature_count);
        for i in 0..n {{
            for (j, name) in feature_names.iter().enumerate() {{
                let col = {frame_var}.column(name.as_str())?;
                let v = xz_anyvalue_f32(col.get(i).unwrap_or(AnyValue::Float64(f64::NAN))) as f64;
                if raw_input {{
                    xs.push(if v.is_finite() {{ v as f32 }} else {{ 0.0 }});
                }} else {{
                    let mean = fmean.get(j).copied().unwrap_or(0.0);
                    let std = fstd.get(j).copied().unwrap_or(1.0);
                    let v = if v.is_finite() {{ v }} else {{ mean }};
                    xs.push(((v - mean) / std) as f32);
                }}
            }}
        }}

        let device: Device<TrainBackend> = Default::default();
        let recorder = PrettyJsonFileRecorder::<FullPrecisionSettings>::new();
        let template = {model_name}::<TrainBackend>::new(&device, feature_count);
        let model = template
            .load_file("checkpoints/{model_name}.json", &recorder, &device)
            .map_err(|e| format!("체크포인트 로드 실패: {{e}}"))?;
        let x = Tensor::<TrainBackend, 2>::from_data(TensorData::new(xs, [n, feature_count]), &device);
        let preds = model.forward(x).into_data().to_vec::<f32>().unwrap_or_default();
        let mut out = {frame_var}.clone();
        out.with_column(Column::new(
            "{out_col}".into(),
            preds.into_iter().map(|v| v as f64).collect::<Vec<f64>>(),
        ))?;
        out
    }};
"#
    )
}

/// `run <src> |> train(<Model>, epochs: [..], lr: [..], ...)` → grid-search code block.
///
/// Emits the same normalization preamble as the single-run path, then a loop over
/// the cartesian-product combinations. Each combo is scored on the training (and,
/// when `validation_split` is set, validation) split by the
/// [`TrainConfig::sweep_metric`] (`mse`/`mae`/`r2`), early stopping is applied on
/// the validation loss, and the best-scoring combo is reported.
fn emit_dl_sweep_call(
    source_var: &str,
    model_name: &str,
    config: &TrainConfig,
    xs_norm: &str,
    val_split: f64,
) -> String {
    let target = &config.target;
    let combos = config.expand_sweep();
    let combo_list = combos
        .iter()
        .map(|c| {
            format!(
                "({}, {:.10}f64, {})",
                c.epochs,
                c.learning_rate,
                c.batch_size.unwrap_or(DEFAULT_BATCH_SIZE).max(1)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let metric_id = config.sweep_metric.id();
    let sort_id = config.sweep_sort.id();
    let tiebreak_list = config
        .sweep_tiebreak
        .iter()
        .map(|t| format!("\"{}\"", t.id()))
        .collect::<Vec<_>>()
        .join(", ");
    let top_expr = match config.sweep_top {
        Some(n) => format!("Some({})", n.max(1)),
        None => "None".to_string(),
    };
    let patience_expr = match config.early_stopping_patience {
        Some(p) if p > 0 => format!("Some({p})"),
        _ => "None".to_string(),
    };

    format!(
        r#"    // ── Deep Learning (sweep): run {source_var} |> train({model_name}, target: "{target}") ──────
    {{
        let (xs, ys, feature_count, _feature_names) = extract_xy(&{source_var}, "{target}")?;
        let n = ys.len();
        if n == 0 {{
            return Err("학습 데이터가 비어 있습니다.".into());
        }}

        // NaN → mean imputation + standardization (per-feature z-score)
        let mut fmean = vec![0f64; feature_count];
        let mut fstd = vec![1f64; feature_count];
        for j in 0..feature_count {{
            let (mut s, mut c) = (0f64, 0usize);
            for i in 0..n {{
                let v = xs[i * feature_count + j] as f64;
                if v.is_finite() {{ s += v; c += 1; }}
            }}
            fmean[j] = if c > 0 {{ s / c as f64 }} else {{ 0.0 }};
        }}
        for j in 0..feature_count {{
            let (mut s, mut c) = (0f64, 0usize);
            for i in 0..n {{
                let d = xs[i * feature_count + j] as f64 - fmean[j];
                if d.is_finite() {{ s += d * d; c += 1; }}
            }}
            fstd[j] = if c > 1 {{ (s / (c - 1) as f64).max(1e-8).sqrt() }} else {{ 1.0 }};
        }}
{xs_norm}
        let tmean: f64 = {{
            let (mut s, mut c) = (0f64, 0usize);
            for &t in &ys {{ if t.is_finite() {{ s += t as f64; c += 1; }} }}
            if c > 0 {{ s / c as f64 }} else {{ 0.0 }}
        }};
        let ys: Vec<f32> = ys
            .iter()
            .map(|&t| if t.is_finite() {{ t as f32 }} else {{ tmean as f32 }})
            .collect();

        // Validation split drives metric selection + early stopping (D3).
        let val_n = (n as f64 * {val_split}) as usize;
        let train_n = n - val_n;

        let combos: Vec<(usize, f64, usize)> = vec![{combo_list}];
        let metric = "{metric_id}";
        let patience: Option<usize> = {patience_expr};
        let mut rows: Vec<XzSweepRow> = Vec::new();
        for (epochs, lr, batch_size) in combos {{
            let device: Device<TrainBackend> = Default::default();
            let mut model = {model_name}::<TrainBackend>::new(&device, feature_count);
            let mut optim = AdamConfig::new().init::<TrainBackend, _>();
            let mut final_train_loss = f64::NAN;
            let mut final_val_loss: Option<f64> = None;
            let mut best_val_loss = f64::INFINITY;
            let mut epochs_no_improve = 0usize;
            for epoch in 0..epochs {{
                let mut loss_sum = 0f32;
                let mut steps = 0usize;
                for b in 0..((train_n + batch_size - 1) / batch_size) {{
                    let start = b * batch_size;
                    let end = ((b + 1) * batch_size).min(train_n);
                    let (x, y) = make_both_tensors(&xs, &ys, feature_count, start, end, &device);
                    let out = model.forward(x);
                    let loss = ((out - y).powf_scalar(2.0)).mean();
                    let lv = loss.clone().into_data().to_vec::<f32>().unwrap_or_default()[0];
                    let grads = loss.backward();
                    let grads = GradientsParams::from_grads(grads, &model);
                    model = optim.step(lr, model, grads);
                    loss_sum += lv;
                    steps += 1;
                }}
                final_train_loss = if steps > 0 {{ (loss_sum / steps as f32) as f64 }} else {{ f64::NAN }};

                // Validation loss for MSE selection / early stopping (D3).
                if val_n > 0 {{
                    let (x, y) = make_both_tensors(&xs, &ys, feature_count, train_n, n, &device);
                    let vout = model.forward(x);
                    let vloss = ((vout - y).powf_scalar(2.0)).mean();
                    final_val_loss = vloss.into_data().to_vec::<f32>().map(|v| v[0] as f64).ok();
                }}
                if let Some(v) = final_val_loss {{
                    if v < best_val_loss {{
                        best_val_loss = v;
                        epochs_no_improve = 0;
                    }} else {{
                        epochs_no_improve += 1;
                    }}
                }}
                let val_line = final_val_loss
                    .map(|v| format!("  val_loss = {{v:.6}}"))
                    .unwrap_or_default();
                println!(
                    "[Epoch {{epoch:>3}}/{{}}]  train_loss = {{:.6}}{{}}",
                    epochs, final_train_loss, val_line
                );
                if let Some(p) = patience {{
                    if val_n > 0 && epochs_no_improve >= p {{
                        println!(
                            "[xazz] early stop (epochs={{}} lr={{}} batch={{}}): no val_loss improvement for {{}} epoch(s)",
                            epochs, lr, batch_size, p
                        );
                        break;
                    }}
                }}
            }}

            // Full-set regression metrics for metric-based selection (D3).
            let (xall, _) = make_both_tensors(&xs, &ys, feature_count, 0, n, &device);
            let preds = model.forward(xall).into_data().to_vec::<f32>().unwrap_or_default();
            let (train_mae, train_r2, val_mae, val_r2) = if preds.len() == n {{
                let (train_mae, train_r2) = xz_regression_metrics(&preds[..train_n], &ys[..train_n]);
                if val_n > 0 {{
                    let (val_mae, val_r2) = xz_regression_metrics(&preds[train_n..], &ys[train_n..]);
                    (train_mae, train_r2, val_mae, val_r2)
                }} else {{
                    (train_mae, train_r2, f64::NAN, f64::NAN)
                }}
            }} else {{
                (f64::NAN, f64::NAN, f64::NAN, f64::NAN)
            }};
            let score = match metric {{
                "mae" => if val_n > 0 {{ val_mae }} else {{ train_mae }},
                "r2" => -if val_n > 0 {{ val_r2 }} else {{ train_r2 }},
                _ => final_val_loss.unwrap_or(final_train_loss),
            }};
            let score = if score.is_finite() {{ score }} else {{ f64::INFINITY }};
            let metric_value = match metric {{
                "mae" => if val_n > 0 {{ val_mae }} else {{ train_mae }},
                "r2" => if val_n > 0 {{ val_r2 }} else {{ train_r2 }},
                _ => final_val_loss.unwrap_or(final_train_loss),
            }};

            let valid = model.valid();
            let recorder = PrettyJsonFileRecorder::<FullPrecisionSettings>::new();
            std::fs::create_dir_all("checkpoints")?;
            valid.save_file(
                &format!("checkpoints/{model_name}_{{}}_{{}}_{{}}", epochs, lr, batch_size),
                &recorder,
            )?;
            println!(
                "[xazz] combo epochs={{}} lr={{}} batch={{}} score={{:.6}}",
                epochs, lr, batch_size, score
            );
            rows.push(XzSweepRow {{
                epochs,
                lr,
                batch_size,
                score,
                final_train_loss,
                final_val_loss,
                metric_value,
                selected: false,
            }});
        }}
        if rows.is_empty() {{
            return Err("하이퍼파라미터 스윕 조합이 없습니다.".into());
        }}

        // The winner is the first minimum by metric, independent of sort/top (D3).
        let mut best_pos = 0usize;
        for i in 1..rows.len() {{
            if rows[i].score < rows[best_pos].score {{
                best_pos = i;
            }}
        }}
        rows[best_pos].selected = true;

        // `top:` keeps only the N best-by-metric combinations (the winner is
        // best-by-metric, so it is always retained).
        let top: Option<usize> = {top_expr};
        if let Some(top) = top {{
            if top < rows.len() {{
                let mut order: Vec<usize> = (0..rows.len()).collect();
                order.sort_by(|&a, &b| {{
                    rows[a]
                        .score
                        .partial_cmp(&rows[b].score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                }});
                order.truncate(top);
                rows = order.into_iter().map(|i| rows[i].clone()).collect();
            }}
        }}

        // `sort:`/`tiebreak:` order the reported table (D3).
        let sort = "{sort_id}";
        let tiebreak: Vec<&str> = vec![{tiebreak_list}];
        rows.sort_by(|a, b| xz_sweep_compare(a, b, sort, &tiebreak));

        println!("{{}}", "─".repeat(60));
        println!(
            "[xazz] sweep report ({{}} combos, metric: {{}}, sort: {{}})",
            rows.len(),
            metric,
            sort
        );
        println!(
            "  {{:>3}}  {{:>6}}  {{:>7}}  {{:>10}}  {{:>12}}  {{:>12}}  {{:>10}}",
            "no", "epochs", "batch", "lr", "val loss", "train loss", "metric"
        );
        for (i, r) in rows.iter().enumerate() {{
            let val = r
                .final_val_loss
                .map(|v| format!("{{v:.6}}"))
                .unwrap_or_else(|| "-".to_string());
            let mark = if r.selected {{ " ★" }} else {{ "" }};
            println!(
                "  {{:>3}}  {{:>6}}  {{:>7}}  {{:>10.6}}  {{:>12}}  {{:>12.6}}  {{:>10.6}}{{}}",
                i, r.epochs, r.batch_size, r.lr, val, r.final_train_loss, r.metric_value, mark
            );
        }}
        if let Some(best) = rows.iter().find(|r| r.selected) {{
            println!(
                "[xazz] ✅ best combo: epochs={{}} lr={{}} batch={{}} score={{:.6}}",
                best.epochs, best.lr, best.batch_size, best.score
            );
        }}
    }}
"#
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// ── schema-based column name validation ────────────────────────────────────────
// ─────────────────────────────────────────────────────────────────────────────

fn validate_op_columns(
    op: &PipelineOp,
    col_types: &HashMap<String, String>,
    var_col_types: &HashMap<String, HashMap<String, String>>,
    source_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // internal helper that checks a column name against the schema
    let check_col = |col_name: &str| -> Result<(), Box<dyn std::error::Error>> {
        if !col_types.contains_key(col_name) {
            let available: Vec<&str> = col_types.keys().map(String::as_str).collect();
            let suggestion = find_closest_col(col_name, &available).unwrap_or(col_name);
            return Err(format!(
                "[SchemaError] at {}\n\
                 ─────────────────────────────────────────────\n\
                 Cause   : 선언되지 않은 컬럼이 있습니다.\n\
                 Detail  : column '{}' not found in schema\n\
                 Available: {}\n\
                 → Did you mean: col(\"{}\")",
                source_path,
                col_name,
                available.join(", "),
                suggestion
            )
            .into());
        }
        Ok(())
    };

    match op {
        PipelineOp::Filter(expr) => {
            validate_expr_columns(expr, col_types, source_path)?;
        }
        PipelineOp::Select(cols) => {
            for col_name in cols {
                check_col(col_name.as_str())?;
            }
        }
        // operators without a column argument
        PipelineOp::Count(None) => {}
        PipelineOp::Take(_) => {}
        PipelineOp::Sample { .. } => {}

        // operators with a string column argument — all schema-validated
        PipelineOp::Count(Some(col))
        | PipelineOp::GroupBy(col)
        | PipelineOp::Sum(col)
        | PipelineOp::Mean(col)
        | PipelineOp::Min(col)
        | PipelineOp::Max(col)
        | PipelineOp::Median(col)
        | PipelineOp::Variance(col)
        | PipelineOp::Std(col)
        | PipelineOp::DropNull(col) => {
            check_col(col.as_str())?;
        }
        PipelineOp::Agg(specs) => {
            for spec in specs {
                check_col(spec.col.as_str())?;
            }
        }
        PipelineOp::OrderBy { col, .. } => {
            check_col(col.as_str())?;
        }
        PipelineOp::FillNull { col, .. } => {
            check_col(col.as_str())?;
        }

        // join: validate left_on/right_on columns + check other variable exists
        PipelineOp::Join {
            other,
            left_on,
            right_on,
            ..
        } => {
            // left_on keys: validate against current schema
            for key in left_on.iter().chain(right_on.iter()) {
                check_col(key.as_str())?;
            }
            // check other variable exists
            if !var_col_types.contains_key(other.as_str()) {
                return Err(format!(
                    "[SchemaError] at {}\n\
                     join() 의 대상 변수 '{}' 가 아직 선언되지 않았습니다.\n\
                     → join() 대상 변수를 먼저 파이프라인에서 선언하세요.",
                    source_path, other
                )
                .into());
            }
        }

        // rename: validate column exists
        PipelineOp::Rename { old_name, .. } => {
            check_col(old_name.as_str())?;
        }

        // replace: validate column exists
        PipelineOp::Replace { col, .. } => {
            check_col(col.as_str())?;
        }

        // withColumn: name is a new column so skip schema validation; validate only columns in expr
        PipelineOp::WithColumn { expr, .. } => {
            validate_expr_columns(expr, col_types, source_path)?;
        }

        // Chart: runtime only (skip validation)
        PipelineOp::Chart(_) => {}

        // Cast: columns are guaranteed by the DSL author (skip validation)
        PipelineOp::Cast { .. } => {}

        // v0.5 deep-learning operators: model variables validated at runtime (skip validation)
        PipelineOp::Train { .. } => {}
        PipelineOp::Predict { .. } => {}

        // v0.6 withDp: no column argument — argument ranges validated by the parser (skip validation)
        PipelineOp::WithDp(_) => {}

        // v0.3.2 save: path/format validated by the parser (skip validation)
        PipelineOp::Save { .. } => {}
    }
    Ok(())
}

fn validate_expr_columns(
    expr: &Expr,
    col_types: &HashMap<String, String>,
    source_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    match expr {
        Expr::Ident(col_name) => {
            if !col_types.contains_key(col_name.as_str()) {
                let available: Vec<&str> = col_types.keys().map(String::as_str).collect();
                let suggestion = find_closest_col(col_name, &available).unwrap_or(col_name);
                return Err(format!(
                    "[SchemaError] at {}\n\
                     ─────────────────────────────────────────────\n\
                     Cause   : filter()/withColumn() 에 선언되지 않은 컬럼이 있습니다.\n\
                     Detail  : column '{}' not found in schema\n\
                     Available: {}\n\
                     → Did you mean: col(\"{}\")",
                    source_path,
                    col_name,
                    available.join(", "),
                    suggestion
                )
                .into());
            }
            Ok(())
        }
        Expr::BinOp { lhs, rhs, .. } => {
            validate_expr_columns(lhs, col_types, source_path)?;
            if matches!(rhs.as_ref(), Expr::Ident(_)) {
                validate_expr_columns(rhs, col_types, source_path)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ── type-aware Polars expression generation ──────────────────────────────────────
// ─────────────────────────────────────────────────────────────────────────────

fn to_typed_polars_expr(expr: &Expr, col_types: &HashMap<String, String>) -> String {
    crate::polars_text::expr_to_polars(expr, Some(col_types))
}

// ─────────────────────────────────────────────────────────────────────────────
// ── typo hint: edit-distance-based column name suggestion ───────────────────────
// ─────────────────────────────────────────────────────────────────────────────

fn find_closest_col<'a>(name: &str, candidates: &[&'a str]) -> Option<&'a str> {
    candidates
        .iter()
        .min_by_key(|&&c| edit_distance(name, c))
        .copied()
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for (i, row) in dp.iter_mut().enumerate().take(m + 1) {
        row[0] = i;
    }
    for (j, cell) in dp[0].iter_mut().enumerate().take(n + 1) {
        *cell = j;
    }
    for i in 1..=m {
        for j in 1..=n {
            dp[i][j] = if a[i - 1] == b[j - 1] {
                dp[i - 1][j - 1]
            } else {
                1 + dp[i - 1][j].min(dp[i][j - 1]).min(dp[i - 1][j - 1])
            };
        }
    }
    dp[m][n]
}

// ─────────────────────────────────────────────────────────────────────────────
// ── CSV loader helper function source included in generated files ──────────────
// ─────────────────────────────────────────────────────────────────────────────

fn emit_csv_loader_fn() -> String {
    r#"/// EUC-KR(CP949) auto-detecting CSV loader
/// Tries UTF-8 first, falls back to EUC-KR decoding (for Korean public data)
fn load_csv(
    file_path: &str,
    has_header: bool,
    separator: Option<u8>,
) -> Result<DataFrame, Box<dyn std::error::Error>> {
    let raw_bytes = std::fs::read(file_path)?;

    // Try UTF-8 directly, fall back to EUC-KR(CP949) decoding
    let utf8_string = match String::from_utf8(raw_bytes.clone()) {
        Ok(s) => s,
        Err(_) => {
            let (cow, _, _) = EUC_KR.decode(&raw_bytes);
            cow.into_owned()
        }
    };

    let mut parse_opts = CsvParseOptions::default()
        .with_null_values(Some(NullValues::AllColumnsSingle("-".into())));
    if let Some(sep) = separator {
        parse_opts = parse_opts.with_separator(sep);
    }

    let cursor = Cursor::new(utf8_string.into_bytes());
    let df = CsvReadOptions::default()
        .with_infer_schema_length(Some(200))
        .with_has_header(has_header)
        .with_parse_options(parse_opts)
        .into_reader_with_file_handle(cursor)
        .finish()?;

    Ok(df)
}
"#
    .to_string()
}

// ─────────────────────────────────────────────────────────────────────────────
// ── emitter unit tests ───────────────────────────────────────────────────────
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Lexer;
    use crate::parser::Parser;

    /// Returns the Program parsed from a source string.
    fn parse(src: &str) -> Program {
        let tokens = Lexer::new(src).tokenize().unwrap();
        Parser::new(tokens).parse().unwrap()
    }

    /// Returns the Rust source generated for a source string.
    fn emit(src: &str) -> String {
        let program = parse(src);
        generate_rust_src(&program, "test.xzz").unwrap()
    }

    #[test]
    fn emit_rust_has_polars_imports() {
        let out = emit(
            "type S = { date: string, pm10: float };
             v a = load(\"x.csv\") :: S |> select([date, pm10]);",
        );
        assert!(
            out.contains("use polars::prelude::*;"),
            "polars import 없음"
        );
        assert!(
            out.contains("use encoding_rs::EUC_KR;"),
            "EUC_KR import 없음"
        );
    }

    #[test]
    fn emit_rust_contains_csv_loader() {
        let out = emit(
            "type S = { a: string };
             v p = load(\"x.csv\") :: S;",
        );
        assert!(out.contains("fn load_csv("), "load_csv 헬퍼 없음");
        assert!(out.contains("CsvReadOptions"), "CsvReadOptions 없음");
    }

    #[test]
    fn emit_rust_maps_load_and_collect() {
        let out = emit(
            "type S = { a: string, b: float };
             v p = load(\"x.csv\") :: S |> filter(b > 10) |> mean(\"b\");",
        );
        assert!(out.contains("load_csv("), "load_csv 없음");
        assert!(out.contains(".collect()"), ".collect() 없음");
        assert!(out.contains("mean"), "mean 집계 없음");
    }

    #[test]
    fn emit_rust_emits_burn_for_dl_program() {
        let out = emit(
            "type S = { a: float, y: float };
             model M { Dense(4) -> ReLU() -> Dense(1) }
             v data = load(\"x.csv\") :: S |> train(M, target: \"y\", epochs: 3);",
        );
        assert!(out.contains("use burn::"), "burn import 없음");
        assert!(out.contains("struct M<B: Backend>"), "모델 구조체 없음");
        assert!(out.contains("Adam"), "Adam 옵티마이저 없음");
    }

    /// Emit stage: a chain ending in Conv1d/Embedding (no final `Dense(1)`)
    /// produces a multi-output `forward`; the emit path must flag it.
    #[test]
    fn emit_output_dim_warning_flags_non_scalar_final_layer() {
        let conv_only = vec![
            LayerKind::Conv1d {
                out_channels: 4,
                kernel_size: 3,
            },
            LayerKind::ReLU,
        ];
        let msg = emit_output_dim_warning("CNN", &conv_only).expect("경고 없음");
        assert!(msg.contains("CNN"), "모델명 누락: {msg}");
        assert!(msg.contains("Dense(1)"), "안내 누락: {msg}");

        let embedding_only = vec![
            LayerKind::Embedding {
                vocab: EmbeddingVocab::Shared(5),
                embed_dim: 2,
            },
            LayerKind::ReLU,
        ];
        assert!(
            emit_output_dim_warning("E", &embedding_only).is_some(),
            "Embedding 종단 경고 없음"
        );

        let dense_n = vec![LayerKind::Dense(4), LayerKind::ReLU, LayerKind::Dense(2)];
        assert!(
            emit_output_dim_warning("D", &dense_n).is_some(),
            "Dense(n!=1) 종단 경고 없음"
        );
    }

    /// A chain ending in `Dense(1)` (the scalar regression shape) must not warn.
    #[test]
    fn emit_output_dim_warning_accepts_final_dense_one() {
        let ok = vec![
            LayerKind::Conv1d {
                out_channels: 4,
                kernel_size: 3,
            },
            LayerKind::ReLU,
            LayerKind::Dense(1),
        ];
        assert!(emit_output_dim_warning("M", &ok).is_none(), "오경고 발생");
    }

    /// VarDecl form `v m = ... |> train(...)` must emit the real Burn training
    /// block (not the old placeholder comment).
    #[test]
    fn emit_rust_vardecl_train_emits_training_block() {
        let out = emit(
            "type S = { a: float, y: float };
             model M { Dense(4) -> Dense(1) }
             v data = load(\"x.csv\") :: S |> train(M, target: \"y\", epochs: 3);",
        );
        assert!(
            out.contains("extract_xy(&data, \"y\")"),
            "VarDecl train 학습 블록 누락: {out}"
        );
        assert!(
            out.contains("AdamConfig::new().init::<TrainBackend, _>()"),
            "VarDecl train 옵티마이저 누락: {out}"
        );
        assert!(
            out.contains("checkpoints/M"),
            "VarDecl train 체크포인트 저장 누락: {out}"
        );
        assert!(
            !out.contains("Burn training runs at xazz execution"),
            "구 placeholder 주석이 남음: {out}"
        );
    }

    /// VarDecl form with list-valued args still emits the grid-search block.
    #[test]
    fn emit_rust_vardecl_sweep_emits_combo_loop() {
        let out = emit(
            "type S = { a: float, y: float };
             model M { Dense(4) -> Dense(1) }
             v data = load(\"x.csv\") :: S
                 |> train(M, target: \"y\", epochs: [3, 5], lr: [0.01, 0.001]);",
        );
        assert!(
            out.contains("let combos: Vec<(usize, f64, usize)> = vec![(3,"),
            "VarDecl 스윕 조합 목록 누락: {out}"
        );
        assert!(
            out.contains("best combo"),
            "VarDecl 스윕 최적 조합 출력 누락: {out}"
        );
    }

    /// D3 Embedding: a shared vocab is replicated per input column into one
    /// combined table, and each column's index is offset into its row range.
    #[test]
    fn emit_rust_embedding_shared_vocab() {
        let out = emit(
            "type S = { a: float, b: float, y: float };
             model M { Embedding(10, 4) -> ReLU() -> Dense(1) }
             v data = load(\"x.csv\") :: S;
             run data |> train(M, target: \"y\", epochs: 3);",
        );
        assert!(
            out.contains("embed_vocab0: Vec<usize> = vec![10; input_dim];"),
            "shared vocab 복제 누락: {out}"
        );
        assert!(
            out.contains("EmbeddingConfig::new(embed_vocab0.iter().sum::<usize>(), 4)"),
            "결합 임베딩 테이블 누락: {out}"
        );
        assert!(
            out.contains("Tensor::cat(cols, 1).int()"),
            "컬럼별 인덱스 결합 누락: {out}"
        );
        assert!(
            out.contains("keep raw category indices"),
            "정규화 생략 누락: {out}"
        );
    }

    /// D3 Embedding: a per-column list is emitted verbatim with a length assert.
    #[test]
    fn emit_rust_embedding_per_column_vocab() {
        let out = emit(
            "type S = { a: float, b: float, y: float };
             model M { Embedding([3, 5], 2) -> ReLU() -> Dense(1) }
             v data = load(\"x.csv\") :: S;
             run data |> train(M, target: \"y\", epochs: 3);",
        );
        assert!(
            out.contains("embed_vocab0: Vec<usize> = vec![3, 5];"),
            "per-column vocab 누락: {out}"
        );
        assert!(
            out.contains("assert_eq!(embed_vocab0.len(), input_dim"),
            "길이 검증 누락: {out}"
        );
    }

    /// D3 sweep: list-valued train args emit a cartesian grid loop + best tracking.
    #[test]
    fn emit_rust_sweep_emits_combo_loop() {
        let out = emit(
            "type S = { a: float, y: float };
             model M { Dense(4) -> Dense(1) }
             v data = load(\"x.csv\") :: S;
             run data |> train(M, target: \"y\", epochs: [3, 5], lr: [0.01, 0.001]);",
        );
        assert!(
            out.contains("let combos: Vec<(usize, f64, usize)> = vec![(3,"),
            "스윕 조합 목록 누락: {out}"
        );
        assert!(out.contains("(5,"), "두 번째 에폭 조합 누락: {out}");
        assert!(out.contains("best combo"), "최적 조합 출력 누락: {out}");
    }

    /// D3 sweep: the emitted selection reflects `metric:` instead of hard-coded
    /// train MSE — the metric helper is emitted and each combo is scored by the
    /// requested metric (R² negated so minimising the score maximises R²).
    #[test]
    fn emit_rust_sweep_metric_selects_by_metric() {
        let out = emit(
            "type S = { a: float, y: float };
             model M { Dense(4) -> Dense(1) }
             v data = load(\"x.csv\") :: S;
             run data |> train(M, target: \"y\", epochs: [3, 5], metric: \"r2\");",
        );
        assert!(
            out.contains("let metric = \"r2\";"),
            "metric 선택값이 emit되지 않음: {out}"
        );
        assert!(
            out.contains("fn xz_regression_metrics("),
            "회귀 지표 헬퍼 누락: {out}"
        );
        assert!(
            out.contains("\"r2\" => -if val_n > 0 { val_r2 } else { train_r2 },"),
            "R² 점수 부호 처리 누락: {out}"
        );
        assert!(
            out.contains("if rows[i].score < rows[best_pos].score"),
            "metric 기반 최적 조합 비교 누락: {out}"
        );
    }

    /// D3 sweep: `sort:`/`tiebreak:`/`top:` are reflected in the emitted report —
    /// the comparator helper is emitted, the requested sort/tiebreak axes reach
    /// the generated code, and the `top` filter is emitted.
    #[test]
    fn emit_rust_sweep_sort_tiebreak_top_reflected() {
        let out = emit(
            "type S = { a: float, y: float };
             model M { Dense(4) -> Dense(1) }
             v data = load(\"x.csv\") :: S;
             run data |> train(M, target: \"y\", epochs: [3, 5], lr: [0.01, 0.001], sort: \"lr\", tiebreak: [batch, epochs], top: 2);",
        );
        assert!(
            out.contains("fn xz_sweep_compare("),
            "스윕 정렬 비교 헬퍼 누락: {out}"
        );
        assert!(
            out.contains("let sort = \"lr\";"),
            "sort 축이 emit되지 않음: {out}"
        );
        assert!(
            out.contains("let tiebreak: Vec<&str> = vec![\"batch\", \"epochs\"];"),
            "tiebreak 축이 순서대로 emit되지 않음: {out}"
        );
        assert!(
            out.contains("let top: Option<usize> = Some(2);"),
            "top 필터가 emit되지 않음: {out}"
        );
        assert!(
            out.contains("rows.sort_by(|a, b| xz_sweep_compare(a, b, sort, &tiebreak));"),
            "정렬 적용 코드 누락: {out}"
        );
    }

    /// D3 sweep: no `sort:`/`tiebreak:`/`top:` still emits the default metric
    /// sort, an empty tiebreak, and no top filter.
    #[test]
    fn emit_rust_sweep_default_sort_emitted() {
        let out = emit(
            "type S = { a: float, y: float };
             model M { Dense(4) -> Dense(1) }
             v data = load(\"x.csv\") :: S;
             run data |> train(M, target: \"y\", epochs: [3, 5], lr: [0.01, 0.001]);",
        );
        assert!(
            out.contains("let sort = \"metric\";"),
            "기본 sort 값 누락: {out}"
        );
        assert!(
            out.contains("let tiebreak: Vec<&str> = vec![];"),
            "빈 tiebreak emit 누락: {out}"
        );
        assert!(
            out.contains("let top: Option<usize> = None;"),
            "top 비활성 emit 누락: {out}"
        );
    }

    /// D3 sweep: `validation_split:` + `patience:` emit validation-loss tracking
    /// and early stopping inside the combo loop.
    #[test]
    fn emit_rust_sweep_early_stopping_patience() {
        let out = emit(
            "type S = { a: float, y: float };
             model M { Dense(4) -> Dense(1) }
             v data = load(\"x.csv\") :: S;
             run data |> train(M, target: \"y\", epochs: [3, 5], validation_split: 0.2, patience: 3);",
        );
        assert!(
            out.contains("let val_n = (n as f64 * 0.2) as usize;"),
            "검증 분할 emit 누락: {out}"
        );
        assert!(
            out.contains("let patience: Option<usize> = Some(3);"),
            "patience emit 누락: {out}"
        );
        assert!(
            out.contains("if val_n > 0 && epochs_no_improve >= p"),
            "조기 종료 조건 누락: {out}"
        );
        assert!(
            out.contains("final_val_loss = vloss.into_data().to_vec::<f32>()"),
            "검증 손실 계산 누락: {out}"
        );
    }

    /// D3 sweep: each epoch prints val_loss when a validation split is set, so
    /// convergence and early stopping are visible in generated code (issue #129).
    #[test]
    fn emit_rust_sweep_epoch_prints_val_loss() {
        let out = emit(
            "type S = { a: float, y: float };
             model M { Dense(4) -> Dense(1) }
             v data = load(\"x.csv\") :: S;
             run data |> train(M, target: \"y\", epochs: [3, 5], validation_split: 0.2);",
        );
        assert!(
            out.contains("val_loss = {v:.6}"),
            "에폭별 val_loss 출력 누락: {out}"
        );
        assert!(
            out.contains("train_loss = {:.6}{}"),
            "에폭 출력이 train_loss/val_loss 형식이 아님: {out}"
        );
    }

    /// No `patience:` leaves early stopping disabled in the emitted sweep.
    #[test]
    fn emit_rust_sweep_without_patience_disables_early_stop() {
        let out = emit(
            "type S = { a: float, y: float };
             model M { Dense(4) -> Dense(1) }
             v data = load(\"x.csv\") :: S;
             run data |> train(M, target: \"y\", epochs: [3, 5]);",
        );
        assert!(
            out.contains("let patience: Option<usize> = None;"),
            "patience 비활성 emit 누락: {out}"
        );
    }

    #[test]
    fn emit_rust_skips_burn_for_plain_program() {
        let out = emit(
            "type S = { a: string };
             v p = load(\"x.csv\") :: S |> select([a]);",
        );
        assert!(
            !out.contains("use burn::"),
            "DL 없는 프로그램에 burn import 생성됨"
        );
    }

    #[test]
    fn emit_rust_contains_source_comment() {
        let out = emit("type S = { a: string }; v p = load(\"x.csv\") :: S;");
        assert!(out.contains("test.xzz"), "소스 경로 주석 없음");
        assert!(out.contains("Auto-generated by xazzLang"), "헤더 없음");
    }

    /// Verifies round-trip that string values with quotes/backslashes in select/fillNull
    /// are safely escaped when inserted into generated code.
    #[test]
    fn emit_rust_escapes_quotes_and_backslashes_in_strings() {
        let out = emit(
            "type S = { a: string };
             v p = load(\"x.csv\") :: S
               |> fillNull(\"a\", \"a\\\"b\")
               |> fillNull(\"a\", \"x\\\\y\");",
        );
        assert!(
            out.contains(".fill_null(lit(\"a\\\"b\"))"),
            "fillNull 따옴표 이스케이프 누락: {}",
            out
        );
        assert!(
            out.contains(".fill_null(lit(\"x\\\\y\"))"),
            "fillNull 백슬래시 이스케이프 누락: {}",
            out
        );
    }

    /// Column names with quotes in select are safely escaped.
    #[test]
    fn emit_rust_escapes_quotes_in_select_columns() {
        let out = emit(
            "type S = { a: string };
             v p = load(\"x.csv\") :: S |> select([a]);",
        );
        assert!(
            out.contains(".select([col(\"a\")])"),
            "select 컬럼 생성 누락: {}",
            out
        );
    }

    /// v0.23 agg([...]) with a preceding groupBy → group_by(...).agg([aliased...]).
    #[test]
    fn emit_rust_group_by_agg_list() {
        let out = emit(
            "type S = { g: string, val: float };
             v p = load(\"x.csv\") :: S
               |> groupBy(\"g\")
               |> agg([min(\"val\"), mean(\"val\"), max(\"val\")]);",
        );
        assert!(
            out.contains(".group_by([col(\"g\")])"),
            "group_by 누락: {out}"
        );
        assert!(out.contains(".agg(["), ".agg([ 누락: {out}");
        assert!(out.contains(".alias(\"val_min\")"), "alias 누락: {out}");
        assert!(out.contains(".alias(\"val_mean\")"), "alias 누락: {out}");
        assert!(out.contains(".alias(\"val_max\")"), "alias 누락: {out}");
    }

    /// v0.23 agg([...]) without a group → select([...]) aggregate.
    #[test]
    fn emit_rust_ungrouped_agg_list() {
        let out = emit(
            "type S = { g: string, val: float };
             v p = load(\"x.csv\") :: S |> agg([mean(\"val\")]);",
        );
        assert!(
            out.contains(".select([col(\"val\").mean().alias(\"val_mean\")])"),
            "ungrouped agg 누락: {out}"
        );
    }

    /// load(sep, header) options are emitted as reader builder calls.
    #[test]
    fn emit_rust_load_separator_and_header_options() {
        let out = emit(
            "type S = { g: string, val: float };
             v p = load(\"x.csv\", sep: \";\", header: false) :: S;",
        );
        assert!(
            out.contains("load_csv(\"x.csv\", false, Some(b'\\x3b'))"),
            "load_csv 옵션 인자 누락: {out}"
        );
        assert!(
            out.contains("separator: Option<u8>"),
            "load_csv 시그니처 누락: {out}"
        );
        assert!(
            out.contains(".with_separator(sep)"),
            "with_separator 누락: {out}"
        );
        assert!(
            out.contains(".with_has_header(has_header)"),
            "with_has_header 누락: {out}"
        );
    }

    /// Training emit must persist the feature order + z-score statistics that
    /// `predict()` replays, and `extract_xy` must return the feature names.
    #[test]
    fn emit_rust_train_writes_normalization_stats_sidecar() {
        let out = emit(
            "type S = { a: float, y: float };
             model M { Dense(4) -> Dense(1) }
             v data = load(\"x.csv\") :: S |> train(M, target: \"y\", epochs: 3);",
        );
        assert!(
            out.contains("Result<(Vec<f32>, Vec<f32>, usize, Vec<String>)"),
            "extract_xy 가 특성 이름을 반환하지 않음: {out}"
        );
        assert!(
            out.contains("checkpoints/M.stats.json"),
            "정규화 통계 사이드카 기록 누락: {out}"
        );
        assert!(
            out.contains("\"feature_names\": feature_names"),
            "특성 이름 직렬화 누락: {out}"
        );
    }

    /// `data |> predict(model_var)` must emit a real inference block that loads
    /// the checkpoint + stats sidecar and appends the prediction column instead of
    /// the old placeholder comment.
    #[test]
    fn emit_rust_predict_emits_inference_block() {
        let out = emit(
            "type S = { a: float, y: float };
             model M { Dense(4) -> Dense(1) }
             v ds = load(\"x.csv\") :: S;
             v m = ds |> train(M, target: \"y\", epochs: 3);
             v p = ds |> predict(m, as: \"y_pred\");",
        );
        assert!(
            out.contains("load_file(\"checkpoints/M.json\""),
            "체크포인트 로드 누락: {out}"
        );
        assert!(
            out.contains("model.forward(x).into_data()"),
            "추론 forward 누락: {out}"
        );
        assert!(
            out.contains("with_column(Column::new("),
            "예측 컬럼 부착 누락: {out}"
        );
        assert!(
            out.contains("\"y_pred\".into()"),
            "지정 예측 컬럼명 누락: {out}"
        );
        assert!(
            !out.contains("prediction column added (at xazz execution)"),
            "구 placeholder 주석이 남음: {out}"
        );
    }

    /// A `predict` with no `as:` uses `<target>_pred` and, when followed by more
    /// operators, restarts a lazy chain on the materialized prediction frame.
    #[test]
    fn emit_rust_predict_default_column_and_relazy_chain() {
        let out = emit(
            "type S = { a: float, y: float };
             model M { Dense(4) -> Dense(1) }
             v ds = load(\"x.csv\") :: S;
             v m = ds |> train(M, target: \"y\", epochs: 3);
             v p = ds |> predict(m) |> rename(\"y\", \"y_actual\") |> take(5);",
        );
        assert!(
            out.contains("\"y_pred\".into()"),
            "기본 예측 컬럼명(y_pred) 누락: {out}"
        );
        assert!(
            out.contains("let p = p.clone().lazy()"),
            "predict 이후 lazy 체인 재시작 누락: {out}"
        );
        assert!(
            out.contains(".rename([\"y\"], [\"y_actual\"], false)"),
            "predict 이후 rename 누락: {out}"
        );
        assert!(out.contains(".limit(5)"), "predict 이후 take 누락: {out}");
    }

    /// A sweep-trained model has no single canonical checkpoint, so `predict`
    /// falls back to a comment rather than referencing a nonexistent checkpoint.
    #[test]
    fn emit_rust_predict_sweep_falls_back_to_comment() {
        let out = emit(
            "type S = { a: float, y: float };
             model M { Dense(4) -> Dense(1) }
             v ds = load(\"x.csv\") :: S;
             v m = ds |> train(M, target: \"y\", epochs: [3, 5]);
             v p = ds |> predict(m, as: \"y_pred\");",
        );
        assert!(
            out.contains("스윕 모델은 예측 대상 체크포인트가 모호해 emit 미지원"),
            "스윕 predict 폴백 주석 누락: {out}"
        );
        assert!(
            !out.contains("load_file(\"checkpoints/M.json\""),
            "스윕 predict 가 체크포인트를 잘못 참조함: {out}"
        );
    }
}
