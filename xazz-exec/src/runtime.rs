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

use crate::chart::{build_chart_spec, df_to_json_array, write_chart_html};
use xazz_compiler::ast::LayerKind;
use xazz_compiler::ir::{ColType, MLOp, PipelineNode, Schema, SideOp, Source, Step as IrStep};
use xazz_compiler::{Lexer, Parser};

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
    optimize: bool,
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

    // ── STEP 3.5: 정적 의미 분석 (Type Checker) + Typed IR 생성 — 실행 전 결함 검출 ─
    // analyze_program 은 진단과 IR 을 **단일 순회**로 만들어 이중 추론을 제거한다.
    let (check, mut ir) = xazz_compiler::analyze_program(&program);
    if !check.errors.is_empty() || !check.warnings.is_empty() {
        eprintln!(
            "[xazz] 정적 분석: 오류 {}건 / 경고 {}건",
            check.errors.len(),
            check.warnings.len()
        );
        for err in &check.errors {
            eprintln!("  [xazz DIAGNOSTIC ERROR] {}", err.message);
        }
        for warn in &check.warnings {
            eprintln!("  [xazz DIAGNOSTIC WARN]  {}", warn.message);
        }
        // [xazz:diagnostics] JSON 마커 — 서버/IDE에서 파싱 가능
        let diag_json = serde_json::json!({
            "errors": check.errors.iter().map(|e| e.message.clone()).collect::<Vec<_>>(),
            "warnings": check.warnings.iter().map(|e| e.message.clone()).collect::<Vec<_>>(),
        });
        println!(
            "[xazz:diagnostics] {}",
            serde_json::to_string(&diag_json).unwrap_or_default()
        );
    }

    // ── STEP 3.55: 타입체커 오류는 실행을 차단한다 (fail-closed) ─────────────
    // 어드바이저리로만 흘려보내면 잘못된 데이터로 Polars 가 더 불명확한 panic 을
    // 일으키므로, 의미 오류가 있으면 실행을 중단한다. (경고는 비차단 유지)
    if !check.errors.is_empty() {
        return Err(format!(
            "[xazz TYPECHECK ERROR] 정적 분석 오류 {}건 — 실행을 중단합니다. 첫 번째 오류: {}",
            check.errors.len(),
            check.errors[0].message
        )
        .into());
    }

    // ── STEP 3.6: Policy-as-Code 정적 가드레일 — 실행 전 보안 차단 (issue #2) ─
    //
    // 이 게이트가 최종 관문이다. CLI(`xazz run`)와 API 서버(`POST /execute`)도
    // 각자 앞단에서 같은 검사를 하지만, 실제로 Polars 를 돌리는 곳은 여기뿐이므로
    // 어떤 경로로 들어오든 이 지점을 지나야 한다.
    //
    // 정책을 불러오지 못하면 실행을 **거부**한다 (fail-closed).
    let active = xazz_compiler::load_active_policy().map_err(|e| {
        let report = xazz_compiler::policy_load_failure_report(&e);
        emit_policy_marker(&report);
        format!("[xazz POLICY ERROR] {}", e)
    })?;

    let policy_report = xazz_compiler::check_policy_parsed(&program, &source, &active.policy);
    emit_policy_marker(&policy_report);

    eprintln!(
        "[xazz] 정적 가드레일: 정책 {} ({}) — {}",
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
            eprintln!("                      보정: {}", v.remediation_hint);
        }
        return Err(format!(
            "[xazz POLICY ERROR] {}\n실행이 차단되었습니다. `xazz policy <file> --fix` 로 안전한 대체 코드를 확인하세요.",
            policy_report.summary()
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

    // ── STEP 4: Codegen — Polars 흐름 매핑 문자열 생성 ──────────────────────
    //
    // (Typed IR 도입으로 문자열 codegen 은 더 이상 실행 경로에서 사용하지 않는다.
    //  raw AST 해석 대신 IR 을 lowering 한다. `xazz emit` 경로만 별도로 유지.)

    // ── STEP 4.5: IR 최적화 (선택) — 상수 폴딩 / Select 병합 / 조건 푸시다운 ──
    if optimize {
        let before = ir.pipelines.iter().map(|p| p.steps.len()).sum::<usize>();
        ir = xazz_compiler::optimize_program(&ir);
        let after = ir.pipelines.iter().map(|p| p.steps.len()).sum::<usize>();
        eprintln!(
            "[xazz] IR 최적화 적용 — 단계 {} → {} ({}개 축소)",
            before,
            after,
            before.saturating_sub(after)
        );
    }

    // ── STEP 5: 런타임 엔진 (Typed IR 소비) ─────────────────────────────────

    // 5-A: ModelRegistry 구축 — ModelDecl 수집 + 로깅 (선언 순서와 무관하게 사용 가능)
    let mut model_registry: HashMap<String, Vec<LayerKind>> = HashMap::new();
    for m in &ir.models {
        model_registry.insert(m.name.clone(), m.layers.clone());
        handle_model_decl(&m.name, &m.layers);
    }

    // 5-B: 파이프라인 순차 실행 + SymbolTable 관리
    let mut symbol_table: HashMap<String, polars::frame::DataFrame> = HashMap::new();
    let mut model_table: HashMap<String, crate::dl::TrainedModel> = HashMap::new();
    // 세션 프라이버시 예산 (ε-budget) — withDp 호출마다 차감, 초과 시 해당 파이프라인 거부
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
                        "[xazz] Pipeline #{} '{}' 완료: {} 행 × {} 열",
                        pipeline_count,
                        vname,
                        df.height(),
                        df.width()
                    );
                    last_var_name = Some(vname.clone());
                    symbol_table.insert(vname.clone(), df);
                } else {
                    eprintln!(
                        "[xazz] Pipeline #{} (ExprStmt) 완료: {} 행 × {} 열",
                        pipeline_count,
                        df.height(),
                        df.width()
                    );
                }
            }
            Ok(None) if node.yields_model => {
                eprintln!(
                    "[xazz] Pipeline #{} '{}' 완료: 학습 모델 생성",
                    pipeline_count, name
                );
                last_var_name = None;
            }
            Ok(None) => {
                eprintln!(
                    "[xazz] Pipeline #{} (TrainStmt) 완료: 학습 모델 생성 (바인딩 없음)",
                    pipeline_count
                );
            }
            Err(e) => {
                eprintln!(
                    "[xazz RUNTIME ERROR] Pipeline #{} ('{}') 실패: {}",
                    pipeline_count, name, e
                );
            }
        }
    }

    eprintln!(
        "[xazz] 완료 — AST {} 개 / 타입 {} 개 / 파이프라인 {} 개",
        program.stmts.len(),
        ir.types.len(),
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

/// IR ColType → Polars DataType (컬럼 캐스팅용).
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

// ── 타입 검증 / Null 처리 ─────────────────────────────────────────────────────
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

// ── 단일 파이프라인 노드 실행 (Typed IR 소비) ────────────────────────────────
//
// raw AST 해석 대신 타입체커가 만든 PipelineNode 를 1회 소비하여
// 데이터(lower::lower_data)/ML(dl::train/predict)/부수(chart/dp)로 분기한다.
fn execute_node(
    node: &PipelineNode,
    symbol_table: &HashMap<String, polars::frame::DataFrame>,
    model_registry: &HashMap<String, Vec<LayerKind>>,
    model_table: &mut HashMap<String, crate::dl::TrainedModel>,
    dp_budget: &mut crate::dp::PrivacyBudget,
) -> Result<Option<polars::frame::DataFrame>, Box<dyn std::error::Error>> {
    use polars::prelude::IntoLazy;

    let (mut lf, _schema_opt): (polars::prelude::LazyFrame, Option<&Schema>) = match &node.source {
        Source::Load { file_path, schema } => {
            let df_raw = load_csv_as_df(file_path)?;
            let csv_headers: Vec<String> = df_raw
                .get_column_names()
                .iter()
                .map(|s| s.to_string())
                .collect();

            let lf_raw = df_raw.lazy();

            let lf_bridged = match schema {
                Some(fields) => {
                    let lf_renamed = apply_dynamic_bridge(lf_raw, &csv_headers, fields);
                    apply_schema_cast(lf_renamed, fields)
                }
                None => lf_raw,
            };

            // 스키마 Null/타입 검증은 집계 등 파이프라인 연산 적용 전,
            // load() 직후의 원본(브리지/캐스트된) 프레임을 대상으로 수행한다.
            if let Some(fields) = schema {
                let df_loaded = lf_bridged.clone().collect()?;
                validate_schema_types(&df_loaded, file_path, fields);
            }

            (lf_bridged, schema.as_ref())
        }
        Source::Ref { var } => match symbol_table.get(var.as_str()) {
            Some(df) => (df.clone().lazy(), None),
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
                    format!(
                        "모델 블록 '{model}' 을 찾을 수 없습니다. 먼저 `model {model} {{ ... }}` 로 선언하세요."
                    )
                })?;
                let trained = crate::dl::train(&snapshot, model, &layers, config)
                    .map_err(|e| format!("학습 실패: {e}"))?;
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
                    format!(
                        "학습된 모델 변수 '{model}' 이 없습니다. 먼저 `v {model} = ... |> train(...)` 로 학습하세요."
                    )
                })?;
                let snapshot = lf.clone().collect()?;
                let out = crate::dl::predict(trained, &snapshot, as_col.as_deref())
                    .map_err(|e| format!("예측 실패: {e}"))?;
                eprintln!(
                    "[xazz] Predict '{}' 완료: 예측 컬럼 추가 ({} 행)",
                    model,
                    out.height()
                );
                lf = out.lazy();
            }
            IrStep::Side(SideOp::Chart(config)) => {
                let snapshot = lf.clone().collect()?;
                let spec = build_chart_spec(config, &snapshot)?;
                println!("[xazz:chart]");
                println!("{}", serde_json::to_string(&spec)?);

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
            IrStep::Side(SideOp::WithDp(args)) => {
                // 1) 세션 (ε, δ)-budget 차감 (조성 회계) — 초과 시 파이프라인 전체 거부
                let delta = args.delta.unwrap_or(crate::dp::DEFAULT_DELTA);
                dp_budget.spend(args.mechanism, args.epsilon, delta)?;

                // 2) 현재까지의 파이프라인을 collect 후 출력 섭동(output perturbation)
                let snapshot = lf.clone().collect()?;
                let (noised, report) = crate::dp::apply_dp(&snapshot, args)?;

                println!("[xazz:dp]");
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
                println!("{}", dp_json);
                eprintln!(
                    "[xazz] DP 적용: {} (ε={}, δ={}, Δf={}, 노이즈 파라미터={:.4}) — 컬럼 {:?} | 예산 ε {:.2}/{:.2} · δ {:.2e}/{:.2e} 사용",
                    report.mechanism,
                    report.epsilon,
                    report
                        .delta
                        .map(|d| format!("{d:.2e}"))
                        .unwrap_or_else(|| "0".to_string()),
                    report.sensitivity,
                    report.noise_param,
                    report.noised_columns,
                    dp_budget.spent(),
                    dp_budget.total(),
                    dp_budget.spent_delta(),
                    dp_budget.total_delta(),
                );

                lf = noised.lazy();
            }
        }
    }

    let df = lf.collect()?;
    Ok(Some(df))
}

// ── Burn 딥러닝 실행 (v0.4) ───────────────────────────────────────────────────

/// ModelDecl 처리 — 모델 정의를 로깅하고 학습에 사용할 레이어 정보를 등록한다.
fn handle_model_decl(name: &str, layers: &[LayerKind]) {
    println!();
    println!("🧠 [xazz Model Declaration: {}]", name);
    println!("{}", "─".repeat(60));
    println!("  레이어 구성 ({} 개):", layers.len());
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

    // [xazz:model] JSON 마커 — 서버/IDE에서 모델 정보를 파싱할 수 있도록 출력
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

/// 학습된 모델(TrainedModel)의 리포트를 콘솔에 출력한다.
fn print_train_report(trained: &crate::dl::TrainedModel) {
    let report = &trained.report;
    println!("{}", "─".repeat(60));
    println!("✅  학습 완료");
    println!("  입력 차원  : {}", report.input_dim);
    println!("  출력 차원  : {}", report.output_dim);
    println!("  파라미터 수 : {}", report.num_params);
    println!("  최종 손실(MSE) : {:.6}", report.final_train_loss);
    if let Some(v) = report.final_val_loss {
        println!("  검증 손실(MSE) : {:.6}", v);
    }
    println!("  특성 컬럼  : {:?}", report.feature_names);
    if !report.predictions.is_empty() {
        println!("  샘플 예측  :");
        for i in 0..report.predictions.len().min(5) {
            println!(
                "    [{i}] 예측 = {:.4}   실제 = {:.4}",
                report.predictions[i], report.targets[i]
            );
        }
    }
    println!("  체크포인트 : {}", report.checkpoint_path);
    println!();
}

/// `[xazz:policy]` 마커로 정책 리포트를 stdout 에 내보낸다.
///
/// 서버/IDE 는 이 마커를 파싱해 차단 사유와 보정 힌트를 그대로 보여 준다.
/// 차단·통과 여부와 무관하게 항상 내보내므로, 프런트엔드는 "검사를 했다"는
/// 사실 자체를 신뢰할 수 있다.
fn emit_policy_marker(report: &xazz_compiler::PolicyReport) {
    match serde_json::to_string(report) {
        Ok(json) => println!("[xazz:policy] {}", json),
        Err(e) => eprintln!("[xazz] ⚠️ 정책 리포트 직렬화 실패: {}", e),
    }
}
