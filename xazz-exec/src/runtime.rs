/// xazz-exec/src/runtime.rs — 런타임 실행 엔진 (v0.18)
///
/// .xzz 소스 파일을 받아 전체 컴파일 파이프라인을 실행하는 라이브러리 모듈.
///
/// ⚠️  이 모듈은 xazz-exec 크레이트에만 존재합니다.
///     xazz-compiler 에는 Polars 의존성이 없으므로 이 모듈이 없습니다.
///     CLI(xazz)는 이 모듈을 직접 링크하지 않고,
///     xazz-runner 서브프로세스를 통해 간접 실행합니다.
use std::collections::HashMap;
use std::fs;

use serde::Serialize;

use xazz_compiler::ast::{
    ChartConfig, ChartType, FillNullValue, JoinHow, PipelineOp, PipelineSource, Stmt,
};
use xazz_compiler::{BinOpKind, Codegen, Expr, Lexer, Parser, StructField};

// ─────────────────────────────────────────────────────────────────────────────
// ── ChartSpec — 프론트엔드로 전달하는 시각화 명세 (v0.19) ─────────────────────
// ─────────────────────────────────────────────────────────────────────────────

/// Recharts 호환 JSON 차트 명세
#[derive(Debug, Serialize)]
pub struct ChartSpec {
    #[serde(rename = "chartType")]
    pub chart_type: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub data: serde_json::Value,
}

// ── DataFrame → ChartSpec 변환 ────────────────────────────────────────────────
fn build_chart_spec(
    config: &ChartConfig,
    df: &polars::frame::DataFrame,
) -> Result<ChartSpec, Box<dyn std::error::Error>> {
    let check_col = |col_name: &str| -> Result<(), Box<dyn std::error::Error>> {
        if df.column(col_name).is_err() {
            let cols: Vec<String> = df
                .get_column_names()
                .iter()
                .map(|s| s.to_string())
                .collect();
            Err(format!(
                "ERROR[VIZ001]: Column '{}' not found. 사용 가능한 컬럼: {}",
                col_name,
                cols.join(", ")
            )
            .into())
        } else {
            Ok(())
        }
    };

    match &config.chart_type {
        ChartType::Bar | ChartType::Line | ChartType::Scatter => {
            if let Some(ref x) = config.x {
                check_col(x)?;
            }
            if let Some(ref y) = config.y {
                check_col(y)?;
            }
        }
        ChartType::Pie => {
            if let Some(ref l) = config.label {
                check_col(l)?;
            }
            if let Some(ref v) = config.value {
                check_col(v)?;
            }
        }
    }

    let data = df_to_json_array(df)?;

    Ok(ChartSpec {
        chart_type: config.chart_type.as_str().to_string(),
        title: config.title.clone().unwrap_or_default(),
        x: config.x.clone(),
        y: config.y.clone(),
        label: config.label.clone(),
        value: config.value.clone(),
        data,
    })
}

/// Polars DataFrame을 JSON 배열(`[{col: val, ...}, ...]`)로 직렬화
fn df_to_json_array(
    df: &polars::frame::DataFrame,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    use polars::prelude::AnyValue;

    let col_names: Vec<String> = df
        .get_column_names()
        .iter()
        .map(|s| s.to_string())
        .collect();
    let height = df.height();
    let mut rows: Vec<serde_json::Value> = Vec::with_capacity(height);

    for row_idx in 0..height {
        let mut obj = serde_json::Map::new();
        for col_name in &col_names {
            if let Ok(series) = df.column(col_name) {
                let val = match series.get(row_idx) {
                    Ok(AnyValue::Null) => serde_json::Value::Null,
                    Ok(AnyValue::Boolean(b)) => serde_json::Value::Bool(b),
                    Ok(AnyValue::Int8(n)) => serde_json::json!(n),
                    Ok(AnyValue::Int16(n)) => serde_json::json!(n),
                    Ok(AnyValue::Int32(n)) => serde_json::json!(n),
                    Ok(AnyValue::Int64(n)) => serde_json::json!(n),
                    Ok(AnyValue::UInt8(n)) => serde_json::json!(n),
                    Ok(AnyValue::UInt16(n)) => serde_json::json!(n),
                    Ok(AnyValue::UInt32(n)) => serde_json::json!(n),
                    Ok(AnyValue::UInt64(n)) => serde_json::json!(n),
                    Ok(AnyValue::Float32(f)) => serde_json::json!(f),
                    Ok(AnyValue::Float64(f)) => serde_json::json!(f),
                    Ok(AnyValue::String(s)) => serde_json::Value::String(s.to_string()),
                    Ok(AnyValue::StringOwned(s)) => serde_json::Value::String(s.to_string()),
                    Ok(other) => serde_json::Value::String(format!("{}", other)),
                    Err(_) => serde_json::Value::Null,
                };
                obj.insert(col_name.to_string(), val);
            }
        }
        rows.push(serde_json::Value::Object(obj));
    }

    Ok(serde_json::Value::Array(rows))
}

// ─────────────────────────────────────────────────────────────────────────────
// ── 최상위 공개 진입점 ─────────────────────────────────────────────────────────
// ─────────────────────────────────────────────────────────────────────────────

/// .xzz 소스 파일 경로를 받아 전체 컴파일+런타임 파이프라인을 실행한다.
///
/// - `verbose`: true 이면 Lexer 토큰 스트림과 AST 를 stdout 에 출력한다.
/// - `output_csv`: Some(path) 이면 마지막 DataFrame 결과를 CSV 파일로 저장한다.
pub fn run_pipeline(
    source_path: &str,
    verbose: bool,
    output_csv: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    // ── STEP 1: 소스 파일 읽기 ───────────────────────────────────────────────
    let source = fs::read_to_string(source_path)
        .map_err(|e| format!("IO 에러: 파일 읽기 실패 '{}' — {}", source_path, e))?;

    eprintln!("[xazz] 입력: {}  ({} bytes)", source_path, source.len());

    // ── STEP 2: Lexer — 토크나이징 ──────────────────────────────────────────
    let mut lexer = Lexer::new(&source);
    let tokens = lexer
        .tokenize()
        .map_err(|e| format!("[xazz LEXER ERROR] {}", e))?;

    eprintln!("[xazz] Lexer 완료: {} 토큰", tokens.len());

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

    // ── STEP 3: Parser — AST 구축 ───────────────────────────────────────────
    let mut parser = Parser::new(tokens);
    let program = parser
        .parse()
        .map_err(|e| format!("[xazz PARSER ERROR] {}", e))?;

    eprintln!("[xazz] Parser 완료: {} AST 노드", program.stmts.len());

    if verbose {
        println!();
        println!("⚡ STEP 2. Abstract Syntax Tree (Parser)");
        println!("{}", "─".repeat(60));
        for (i, stmt) in program.stmts.iter().enumerate() {
            println!("  [{}] {:#?}", i, stmt);
        }
        println!();
    }

    // ── STEP 4: Codegen — Polars 흐름 매핑 문자열 생성 ──────────────────────
    let _codegen_output = Codegen::generate(&program);

    // ── STEP 5: 런타임 엔진 ─────────────────────────────────────────────────

    // 5-A: TypeRegistry 구축 — TypeDecl 수집
    let mut type_registry: HashMap<String, Vec<StructField>> = HashMap::new();
    for stmt in &program.stmts {
        if let Stmt::TypeDecl { name, fields } = stmt {
            type_registry.insert(name.clone(), fields.clone());
        }
    }

    // 5-B: VarDecl 순차 실행 + SymbolTable 관리
    let mut symbol_table: HashMap<String, polars::frame::DataFrame> = HashMap::new();
    let mut pipeline_count = 0usize;
    let mut last_var_name: Option<String> = None;

    for stmt in &program.stmts {
        match stmt {
            Stmt::VarDecl {
                var_name,
                is_mut: _,
                source,
                ops,
            } => {
                pipeline_count += 1;

                match execute_var_decl(var_name, source, ops, &symbol_table, &type_registry) {
                    Ok(df) => {
                        eprintln!(
                            "[xazz] Pipeline #{} '{}' 완료: {} 행 × {} 열",
                            pipeline_count,
                            var_name,
                            df.height(),
                            df.width()
                        );
                        last_var_name = Some(var_name.clone());
                        symbol_table.insert(var_name.clone(), df);
                    }
                    Err(e) => {
                        eprintln!(
                            "[xazz RUNTIME ERROR] Pipeline #{} ('{}') 실패: {}",
                            pipeline_count, var_name, e
                        );
                    }
                }
            }

            Stmt::ExprStmt { source, ops } => {
                pipeline_count += 1;
                let anon_name = match source {
                    PipelineSource::VarRef(src) => src.clone(),
                    _ => format!("chart_{}", pipeline_count),
                };

                match execute_var_decl(&anon_name, source, ops, &symbol_table, &type_registry) {
                    Ok(df) => {
                        eprintln!(
                            "[xazz] Pipeline #{} (ExprStmt) 완료: {} 행 × {} 열",
                            pipeline_count,
                            df.height(),
                            df.width()
                        );
                    }
                    Err(e) => {
                        eprintln!(
                            "[xazz RUNTIME ERROR] Pipeline #{} (ExprStmt) 실패: {}",
                            pipeline_count, e
                        );
                    }
                }
            }

            _ => {}
        }
    }

    eprintln!(
        "[xazz] 완료 — AST {} 개 / 스키마 {} 개 / 파이프라인 {} 개",
        program.stmts.len(),
        type_registry.len(),
        pipeline_count
    );

    // ── STEP 6: 최종 DataFrame 자동 출력 (Top 5) ────────────────────────────
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

            // ── [xazz:result] JSON 마커 ──────────────────────────────────────
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

            // ── STEP 7: CSV Export (--output 플래그) ──────────────────────────
            if let Some(csv_path) = output_csv {
                match save_df_as_csv(df, csv_path) {
                    Ok(_) => {
                        println!();
                        println!("💾 [xazz] CSV 저장 완료: {}", csv_path);
                    }
                    Err(e) => {
                        eprintln!("[xazz] ⚠️  CSV 저장 실패: {}", e);
                    }
                }
            }
        }
    }

    Ok(())
}

// ── CSV 저장 헬퍼 ─────────────────────────────────────────────────────────────
fn save_df_as_csv(
    df: &polars::frame::DataFrame,
    path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use polars::prelude::{CsvWriter, SerWriter};

    let mut file = std::fs::File::create(path)
        .map_err(|e| format!("CSV 파일 생성 실패 '{}' — {}", path, e))?;

    CsvWriter::new(&mut file)
        .finish(&mut df.clone())
        .map_err(|e| format!("CSV 쓰기 실패 — {}", e))?;

    Ok(())
}

// ── AST Expr → Polars Expr 변환 ──────────────────────────────────────────────
fn to_polars_expr(expr: &Expr) -> polars::prelude::Expr {
    use polars::prelude::{col, lit};
    match expr {
        Expr::Ident(s) => col(s.as_str()),
        Expr::IntLit(n) => lit(*n),
        Expr::FloatLit(f) => lit(*f),
        Expr::StringLit(s) => lit(s.clone()),
        Expr::BoolLit(b) => lit(*b),
        Expr::BinOp { lhs, op, rhs } => {
            let l = to_polars_expr(lhs);
            let r = to_polars_expr(rhs);
            match op {
                BinOpKind::Eq => l.eq(r),
                BinOpKind::NotEq => l.neq(r),
                BinOpKind::Lt => l.lt(r),
                BinOpKind::Gt => l.gt(r),
                BinOpKind::LtEq => l.lt_eq(r),
                BinOpKind::GtEq => l.gt_eq(r),
                BinOpKind::Add => l + r,
                BinOpKind::Sub => l - r,
                BinOpKind::Mul => l * r,
                BinOpKind::Div => l / r,
            }
        }
    }
}

// ── JoinHow → Polars JoinType 변환 ────────────────────────────────────────────
fn to_polars_join_type(how: &JoinHow) -> polars::prelude::JoinType {
    use polars::prelude::JoinType;
    match how {
        JoinHow::Inner => JoinType::Inner,
        JoinHow::Left => JoinType::Left,
        JoinHow::Outer => JoinType::Full,
        JoinHow::Cross => JoinType::Cross,
    }
}

// ── Schema-Based Type Cast ────────────────────────────────────────────────────
fn apply_schema_cast(
    lf: polars::prelude::LazyFrame,
    schema_fields: &[StructField],
) -> polars::prelude::LazyFrame {
    use polars::prelude::{DataType, col};

    let cast_exprs: Vec<polars::prelude::Expr> = schema_fields
        .iter()
        .filter_map(|field| {
            let inner_type = if field.field_type.starts_with("Option<") {
                field
                    .field_type
                    .trim_start_matches("Option<")
                    .trim_end_matches('>')
            } else {
                field.field_type.as_str()
            };

            let dtype = match inner_type {
                "string" | "str" => Some(DataType::String),
                "int" => Some(DataType::Int64),
                "float" => Some(DataType::Float64),
                "bool" => Some(DataType::Boolean),
                _ => None,
            };

            dtype.map(|dt| col(field.name.as_str()).cast(dt).alias(field.name.as_str()))
        })
        .collect();

    if cast_exprs.is_empty() {
        lf
    } else {
        lf.with_columns(cast_exprs)
    }
}

// ── Dynamic Schema Bridge ─────────────────────────────────────────────────────
fn apply_dynamic_bridge(
    lf: polars::prelude::LazyFrame,
    csv_headers: &[String],
    schema_fields: &[StructField],
) -> polars::prelude::LazyFrame {
    let map_count = csv_headers.len().min(schema_fields.len());

    let old_names: Vec<&str> = csv_headers[..map_count]
        .iter()
        .map(String::as_str)
        .collect();
    let new_names: Vec<&str> = schema_fields[..map_count]
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

// ── 타입 검증 / Null 처리 ─────────────────────────────────────────────────────
fn validate_schema_types(
    df: &polars::frame::DataFrame,
    schema_name: &str,
    schema_fields: &[StructField],
) {
    for field in schema_fields {
        let is_optional = field.field_type.starts_with("Option<");
        match df.column(&field.name) {
            Ok(series) => {
                let null_count = series.null_count();
                let dtype = series.dtype();
                if null_count > 0 && !is_optional {
                    eprintln!(
                        "[xazz WARN] Null 위반 [{}]: 필수 필드 '{}' ({:?}) 에 null {} 개 발견",
                        schema_name, field.name, dtype, null_count
                    );
                }
            }
            Err(_) => {
                eprintln!(
                    "[xazz WARN] 스키마 필드 '{}' 를 DataFrame에서 찾을 수 없음",
                    field.name
                );
            }
        }
    }
}

// ── CSV 로더 (인코딩 자동 처리 + Dirty-data null 정규화) ──────────────────────
fn load_csv_as_df(file_path: &str) -> Result<polars::frame::DataFrame, Box<dyn std::error::Error>> {
    use polars::prelude::{CsvParseOptions, CsvReadOptions, NullValues, SerReader};
    use std::io::Cursor;

    let raw_bytes = std::fs::read(file_path)
        .map_err(|e| format!("IO 에러: CSV 파일 읽기 실패 '{}' — {}", file_path, e))?;

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
        .with_infer_schema_length(Some(200))
        .with_parse_options(CsvParseOptions::default().with_null_values(Some(null_vals)))
        .into_reader_with_file_handle(cursor)
        .finish()?;

    Ok(df)
}

// ── 단일 파이프라인 실행 ──────────────────────────────────────────────────────
fn execute_var_decl(
    var_name: &str,
    source: &PipelineSource,
    ops: &[PipelineOp],
    symbol_table: &HashMap<String, polars::frame::DataFrame>,
    type_registry: &HashMap<String, Vec<StructField>>,
) -> Result<polars::frame::DataFrame, Box<dyn std::error::Error>> {
    use polars::prelude::{IntoLazy, JoinArgs, SortMultipleOptions, col, lit};

    let (mut lf, schema_fields_opt): (polars::prelude::LazyFrame, Option<Vec<StructField>>) =
        match source {
            PipelineSource::Load {
                file_path,
                schema_name,
            } => {
                let df_raw = load_csv_as_df(file_path)?;
                let csv_headers: Vec<String> = df_raw
                    .get_column_names()
                    .iter()
                    .map(|s| s.to_string())
                    .collect();

                let schema_fields = type_registry.get(schema_name.as_str()).cloned();
                let lf_raw = df_raw.lazy();

                let lf_bridged = if let Some(ref fields) = schema_fields {
                    let lf_renamed = apply_dynamic_bridge(lf_raw, &csv_headers, fields);
                    apply_schema_cast(lf_renamed, fields)
                } else {
                    lf_raw
                };

                (lf_bridged, schema_fields)
            }

            PipelineSource::VarRef(src_var) => match symbol_table.get(src_var.as_str()) {
                Some(df) => (df.clone().lazy(), None),
                None => {
                    return Err(format!(
                        "변수 에러: 미선언 변수 '{}' 참조. 이전 파이프라인에서 먼저 선언하세요.",
                        src_var
                    )
                    .into());
                }
            },
        };

    let mut pending_group_by: Option<String> = None;
    let mut has_count_flag = false;

    for op in ops {
        match op {
            PipelineOp::Filter(expr) => {
                lf = lf.filter(to_polars_expr(expr));
            }
            PipelineOp::Select(cols) => {
                let exprs: Vec<polars::prelude::Expr> =
                    cols.iter().map(|c| col(c.as_str())).collect();
                lf = lf.select(exprs);
            }
            PipelineOp::Count(None) => {
                has_count_flag = true;
            }

            PipelineOp::GroupBy(group_col) => {
                pending_group_by = Some(group_col.clone());
            }

            PipelineOp::Count(Some(agg_col)) => {
                if let Some(group_col) = pending_group_by.take() {
                    lf = lf
                        .group_by([col(group_col.as_str())])
                        .agg([col(agg_col.as_str()).count()]);
                } else {
                    lf = lf.select([col(agg_col.as_str()).count()]);
                }
            }
            PipelineOp::Sum(agg_col) => {
                if let Some(group_col) = pending_group_by.take() {
                    lf = lf
                        .group_by([col(group_col.as_str())])
                        .agg([col(agg_col.as_str()).sum()]);
                } else {
                    lf = lf.select([col(agg_col.as_str()).sum()]);
                }
            }
            PipelineOp::Mean(agg_col) => {
                if let Some(group_col) = pending_group_by.take() {
                    lf = lf
                        .group_by([col(group_col.as_str())])
                        .agg([col(agg_col.as_str()).mean()]);
                } else {
                    lf = lf.select([col(agg_col.as_str()).mean()]);
                }
            }
            PipelineOp::Min(agg_col) => {
                if let Some(group_col) = pending_group_by.take() {
                    lf = lf
                        .group_by([col(group_col.as_str())])
                        .agg([col(agg_col.as_str()).min()]);
                } else {
                    lf = lf.select([col(agg_col.as_str()).min()]);
                }
            }
            PipelineOp::Max(agg_col) => {
                if let Some(group_col) = pending_group_by.take() {
                    lf = lf
                        .group_by([col(group_col.as_str())])
                        .agg([col(agg_col.as_str()).max()]);
                } else {
                    lf = lf.select([col(agg_col.as_str()).max()]);
                }
            }

            // ── v0.22 median / variance / std 집계 (ddof=1) ────────────────
            PipelineOp::Median(agg_col) => {
                if let Some(group_col) = pending_group_by.take() {
                    lf = lf
                        .group_by([col(group_col.as_str())])
                        .agg([col(agg_col.as_str()).median()]);
                } else {
                    lf = lf.select([col(agg_col.as_str()).median()]);
                }
            }
            PipelineOp::Variance(agg_col) => {
                if let Some(group_col) = pending_group_by.take() {
                    lf = lf
                        .group_by([col(group_col.as_str())])
                        .agg([col(agg_col.as_str()).var(1)]);
                } else {
                    lf = lf.select([col(agg_col.as_str()).var(1)]);
                }
            }
            PipelineOp::Std(agg_col) => {
                if let Some(group_col) = pending_group_by.take() {
                    lf = lf
                        .group_by([col(group_col.as_str())])
                        .agg([col(agg_col.as_str()).std(1)]);
                } else {
                    lf = lf.select([col(agg_col.as_str()).std(1)]);
                }
            }

            // ── v0.22 sample(n) / sample(n, seed: 42) — 무작위 샘플링 ───────
            PipelineOp::Sample { n, seed } => {
                let snapshot = lf.clone().collect()?;
                let seed_u64 = seed.map(|s| s as u64);
                let sampled = snapshot.sample_n_literal(*n as usize, false, false, seed_u64)?;
                lf = sampled.lazy();
            }

            PipelineOp::OrderBy {
                col: sort_col,
                desc,
            } => {
                let sort_opts = SortMultipleOptions::default().with_order_descending(*desc);
                lf = lf.sort([sort_col.as_str()], sort_opts);
            }

            PipelineOp::Take(n) => {
                lf = lf.limit(*n as u32);
            }

            PipelineOp::DropNull(drop_col) => {
                lf = lf.filter(col(drop_col.as_str()).is_not_null());
            }
            PipelineOp::FillNull {
                col: fill_col,
                value,
            } => {
                let fill_lit: polars::prelude::Expr = match value {
                    FillNullValue::Int(n) => lit(*n),
                    FillNullValue::Float(f) => lit(*f),
                    FillNullValue::Str(s) => lit(s.clone()),
                };
                lf = lf.with_columns([col(fill_col.as_str()).fill_null(fill_lit)]);
            }

            PipelineOp::Join {
                other,
                left_on,
                right_on,
                how,
            } => match symbol_table.get(other.as_str()) {
                Some(other_df) => {
                    let other_lf = other_df.clone().lazy();
                    let left_keys: Vec<polars::prelude::Expr> =
                        left_on.iter().map(|k| col(k.as_str())).collect();
                    let right_keys: Vec<polars::prelude::Expr> =
                        right_on.iter().map(|k| col(k.as_str())).collect();
                    let join_type = to_polars_join_type(how);
                    lf = lf.join(other_lf, left_keys, right_keys, JoinArgs::new(join_type));
                }
                None => {
                    return Err(format!(
                        "런타임 에러: join() 대상 변수 '{}' 가 심볼 테이블에 없습니다.",
                        other
                    )
                    .into());
                }
            },

            PipelineOp::WithColumn {
                name: col_name,
                expr,
            } => {
                let polars_expr = to_polars_expr(expr).alias(col_name.as_str());
                lf = lf.with_columns([polars_expr]);
            }

            PipelineOp::Cast {
                col: cast_col,
                to_type,
            } => {
                use polars::prelude::DataType;
                let dtype = match to_type.as_str() {
                    "float" => DataType::Float64,
                    "int" => DataType::Int64,
                    "str" => DataType::String,
                    "bool" => DataType::Boolean,
                    other => {
                        return Err(format!(
                            "런타임 에러: cast() 에 알 수 없는 타입 '{}'. 지원 타입: \"float\", \"int\", \"str\", \"bool\"",
                            other
                        )
                        .into());
                    }
                };
                lf = lf.with_columns([col(cast_col.as_str()).cast(dtype)]);
            }

            PipelineOp::Rename { old_name, new_name } => {
                let old: Vec<&str> = vec![old_name.as_str()];
                let new: Vec<&str> = vec![new_name.as_str()];
                lf = lf.rename(old, new, false);
            }

            PipelineOp::Replace {
                col: replace_col,
                from,
                to,
            } => {
                lf = lf.with_columns([col(replace_col.as_str())
                    .str()
                    .replace_all(lit(from.as_str()), lit(to.as_str()), true)
                    .alias(replace_col.as_str())]);
            }

            PipelineOp::Chart(config) => {
                let snapshot = lf.clone().collect()?;
                let spec = build_chart_spec(config, &snapshot)?;
                println!("[xazz:chart]");
                println!("{}", serde_json::to_string(&spec)?);

                let safe_name: String = var_name
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
                        println!("[xazz] 📊 차트 HTML 생성: {}", html_path);
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
                        eprintln!("[xazz] ⚠️  차트 HTML 생성 실패: {}", e);
                    }
                }

                eprintln!(
                    "[xazz] Chart '{}' 생성 완료: {} 행",
                    config.chart_type.as_str(),
                    snapshot.height()
                );
                lf = snapshot.lazy();
            }
        }
    }

    let df = lf.collect()?;

    let has_rename_or_select = ops
        .iter()
        .any(|op| matches!(op, PipelineOp::Rename { .. } | PipelineOp::Select(_)));
    if let Some(ref fields) = schema_fields_opt {
        if !has_rename_or_select {
            let schema_name = match source {
                PipelineSource::Load { schema_name, .. } => schema_name.as_str(),
                _ => "unknown",
            };
            validate_schema_types(&df, schema_name, fields);
        }
    }

    let _ = (var_name, has_count_flag);

    Ok(df)
}

// ── write_chart_html — ChartSpec → Chart.js 기반 HTML 파일 생성 ───────────────
fn write_chart_html(spec: &ChartSpec, output_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let data_json = serde_json::to_string(&spec.data)?;
    let title = &spec.title;
    let chart_type_str = &spec.chart_type;

    let chartjs_type = match chart_type_str.as_str() {
        "bar" => "bar",
        "line" => "line",
        "pie" => "pie",
        "scatter" => "scatter",
        other => other,
    };

    let dataset_js = match chart_type_str.as_str() {
        "pie" => {
            let label_field = spec.label.as_deref().unwrap_or("label");
            let value_field = spec.value.as_deref().unwrap_or("value");
            format!(
                r#"{{
            type: '{chartjs_type}',
            data: {{
                labels: data.map(d => d['{label_field}']),
                datasets: [{{
                    label: '{title}',
                    data: data.map(d => d['{value_field}']),
                    backgroundColor: [
                        'rgba(255, 99, 132, 0.7)',
                        'rgba(54, 162, 235, 0.7)',
                        'rgba(255, 206, 86, 0.7)',
                        'rgba(75, 192, 192, 0.7)',
                        'rgba(153, 102, 255, 0.7)',
                        'rgba(255, 159, 64, 0.7)',
                        'rgba(199, 199, 199, 0.7)',
                        'rgba(83, 102, 255, 0.7)',
                        'rgba(40, 159, 64, 0.7)',
                        'rgba(210, 99, 132, 0.7)'
                    ],
                    borderWidth: 1
                }}]
            }},
            options: {{
                responsive: true,
                plugins: {{
                    legend: {{ display: true, position: 'right' }},
                    title: {{ display: false }}
                }}
            }}
        }}"#,
                chartjs_type = chartjs_type,
                label_field = label_field,
                value_field = value_field,
                title = title,
            )
        }
        "scatter" => {
            let x_field = spec.x.as_deref().unwrap_or("x");
            let y_field = spec.y.as_deref().unwrap_or("y");
            format!(
                r#"{{
            type: '{chartjs_type}',
            data: {{
                datasets: [{{
                    label: '{title}',
                    data: data.map(d => ({{ x: d['{x_field}'], y: d['{y_field}'] }})),
                    backgroundColor: 'rgba(54, 162, 235, 0.5)',
                    borderColor: 'rgba(54, 162, 235, 1)',
                    pointRadius: 5
                }}]
            }},
            options: {{
                responsive: true,
                plugins: {{ legend: {{ display: true }} }},
                scales: {{
                    x: {{ title: {{ display: true, text: '{x_field}' }} }},
                    y: {{ title: {{ display: true, text: '{y_field}' }}, beginAtZero: false }}
                }}
            }}
        }}"#,
                chartjs_type = chartjs_type,
                title = title,
                x_field = x_field,
                y_field = y_field,
            )
        }
        _ => {
            let x_field = spec.x.as_deref().unwrap_or("x");
            let y_field = spec.y.as_deref().unwrap_or("y");
            let bg_color = if chart_type_str == "line" {
                "rgba(54, 162, 235, 0.1)"
            } else {
                "rgba(54, 162, 235, 0.5)"
            };
            let border_fill = if chart_type_str == "line" {
                "true"
            } else {
                "false"
            };
            format!(
                r#"{{
            type: '{chartjs_type}',
            data: {{
                labels: data.map(d => d['{x_field}']),
                datasets: [{{
                    label: '{title}',
                    data: data.map(d => d['{y_field}']),
                    backgroundColor: '{bg_color}',
                    borderColor: 'rgba(54, 162, 235, 1)',
                    borderWidth: 2,
                    fill: {border_fill}
                }}]
            }},
            options: {{
                responsive: true,
                plugins: {{ legend: {{ display: true }} }},
                scales: {{
                    x: {{ title: {{ display: true, text: '{x_field}' }} }},
                    y: {{ beginAtZero: true, title: {{ display: true, text: '{y_field}' }} }}
                }}
            }}
        }}"#,
                chartjs_type = chartjs_type,
                x_field = x_field,
                y_field = y_field,
                title = title,
                bg_color = bg_color,
                border_fill = border_fill,
            )
        }
    };

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="ko">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>{title}</title>
  <script src="https://cdn.jsdelivr.net/npm/chart.js"></script>
  <style>
    * {{ box-sizing: border-box; margin: 0; padding: 0; }}
    body {{
      display: flex;
      justify-content: center;
      align-items: center;
      min-height: 100vh;
      background: #f0f2f5;
      font-family: 'Segoe UI', sans-serif;
    }}
    .chart-container {{
      width: 900px;
      max-width: 95vw;
      background: white;
      border-radius: 16px;
      padding: 32px;
      box-shadow: 0 8px 24px rgba(0, 0, 0, 0.12);
    }}
    h1 {{
      text-align: center;
      color: #1a1a2e;
      font-size: 1.5em;
      margin-bottom: 24px;
      font-weight: 600;
    }}
    .meta {{
      text-align: center;
      color: #888;
      font-size: 0.8em;
      margin-top: 16px;
    }}
  </style>
</head>
<body>
  <div class="chart-container">
    <h1>{title}</h1>
    <canvas id="xazz-chart"></canvas>
    <p class="meta">Generated by xazz-lang 📊</p>
  </div>
  <script>
    const data = {data_json};
    new Chart(document.getElementById('xazz-chart'), {dataset_js});
  </script>
</body>
</html>
"#,
        title = title,
        data_json = data_json,
        dataset_js = dataset_js,
    );

    std::fs::write(output_path, html)?;
    Ok(())
}
