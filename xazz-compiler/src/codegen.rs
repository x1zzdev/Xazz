/// xazzLang - 코드 생성기 (v0.16)
///
/// Program AST → Polars LazyFrame 흐름 문자열 생성.
///
/// [v0.16 변경사항]
///   - BoolLit 표현식 지원 (true/false)
///   - Count(None) / Count(Some(col)) 구분
///   - 신규 연산자 출력: GroupBy, Sum, Mean, Min, Max, OrderBy, Take, DropNull, FillNull
///   - Join 연산자: .join(..., ..., JoinArgs::new(JoinType::Inner)) 매핑
///   - WithColumn 연산자: .with_columns([expr.alias("name")]) 매핑
///   - 산술 연산자: Add/Sub/Mul/Div → .add()/.sub()/.mul()/.div()
///
/// 중복 제거 (단일 위치):
///   - 표현식 → Polars 문자열 매핑은 `crate::polars_text` 의 유일한 구현을
///     사용한다 (이 모듈과 emitter.rs 가 공유).
///   - 런타임 op→Polars 매핑은 xazz-exec/src/lower.rs (Typed IR) 한 곳에만 존재한다.
use crate::ast::{
    BinOpKind, Expr, FillNullValue, LayerKind, PipelineOp, PipelineSource, Program, Stmt,
    TrainConfig,
};

/// 코드 생성기 — 유닛 구조체
pub struct Codegen;

/// 생성된 Rust 문자열 리터럴에 삽입할 DSL 문자열 값을 이스케이프한다.
/// (`"` 와 `\` 를 이스케이프하여 생성 코드 주입 방지)
fn esc(s: &str) -> String {
    crate::policy::printer::escape(s)
}

impl Codegen {
    pub fn new() -> Self {
        Codegen
    }

    // ── 최상위 진입점 ─────────────────────────────────────────────────────────

    /// Program AST 전체를 Polars 흐름 문자열로 생성
    pub fn generate(program: &Program) -> String {
        let mut out = String::new();
        out.push_str("// ═══════════════════════════════════════════════════════\n");
        out.push_str("// xazzLang → Polars LazyFrame 흐름 매핑\n");
        out.push_str("// ═══════════════════════════════════════════════════════\n\n");

        for (i, stmt) in program.stmts.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&Self::emit_stmt(stmt));
        }
        out
    }

    // ── Stmt 변환 ─────────────────────────────────────────────────────────────

    fn emit_stmt(stmt: &Stmt) -> String {
        match stmt {
            Stmt::TypeDecl { name, fields } => {
                let mut s = format!("// [Schema] {}\n", name);
                for f in fields {
                    s.push_str(&format!("//   {:<12} : {}\n", f.name, f.field_type));
                }
                s
            }
            Stmt::VarDecl {
                var_name,
                is_mut,
                source,
                ops,
            } => Self::emit_var_decl(var_name, *is_mut, source, ops),
            Stmt::ExprStmt { source, ops } => Self::emit_expr_stmt(source, ops),
            Stmt::ModelDecl { name, layers } => Self::emit_model_decl(name, layers),
            Stmt::TrainStmt {
                source_var,
                model_name,
                config,
            } => Self::emit_train_stmt(source_var, model_name, config),
        }
    }

    // ── VarDecl 변환 ──────────────────────────────────────────────────────────

    fn emit_var_decl(
        var_name: &str,
        is_mut: bool,
        source: &PipelineSource,
        ops: &[PipelineOp],
    ) -> String {
        let mut lines: Vec<String> = Vec::new();

        // 주석 헤더
        let source_comment = match source {
            PipelineSource::Load {
                file_path,
                schema_name,
            } => {
                format!("load(\"{}\") :: {}", file_path, schema_name)
            }
            PipelineSource::VarRef(name) => {
                format!("{} (varref)", name)
            }
        };
        let mut_kw = if is_mut { "mut " } else { "" };
        lines.push(format!(
            "// [VarDecl] {}v {} = {}",
            mut_kw, var_name, source_comment
        ));

        // 소스 코드 생성
        match source {
            PipelineSource::Load {
                file_path,
                schema_name,
            } => {
                lines.push(format!(
                    "let {} = LazyCsvReader::new(\"{}\")  // :: {}",
                    var_name,
                    esc(file_path),
                    schema_name
                ));
                lines.push("  .with_has_header(true)".into());
                lines.push("  .finish()?".into());
            }
            PipelineSource::VarRef(src_var) => {
                lines.push(format!("let {} = {}.clone().lazy()", var_name, src_var));
            }
        }

        // 파이프라인 각 단계
        for op in ops {
            lines.push(Self::emit_op(op));
        }

        // collect — Lazy 실행 시점
        lines.push(format!(
            "  .collect()?;  // ← {}: 모든 연산 일괄 실행",
            var_name
        ));

        lines.join("\n")
    }

    // ── ExprStmt 변환 ─────────────────────────────────────────────────────────

    fn emit_expr_stmt(source: &PipelineSource, ops: &[PipelineOp]) -> String {
        let mut lines: Vec<String> = Vec::new();

        let source_comment = match source {
            PipelineSource::Load {
                file_path,
                schema_name,
            } => {
                format!("load(\"{}\") :: {}", file_path, schema_name)
            }
            PipelineSource::VarRef(name) => {
                format!("{} (varref)", name)
            }
        };
        lines.push(format!("// [ExprStmt] source = {}", source_comment));

        match source {
            PipelineSource::Load {
                file_path,
                schema_name,
            } => {
                lines.push(format!(
                    "let _expr_result = LazyCsvReader::new(\"{}\")  // :: {}",
                    esc(file_path),
                    schema_name
                ));
                lines.push("  .with_has_header(true)".into());
                lines.push("  .finish()?".into());
            }
            PipelineSource::VarRef(src_var) => {
                lines.push(format!("let _expr_result = {}.clone().lazy()", src_var));
            }
        }

        for op in ops {
            lines.push(Self::emit_op(op));
        }

        lines.push("  .collect()?;  // ← expression statement 실행".to_string());
        lines.join("\n")
    }

    // ── ModelDecl 변환 ────────────────────────────────────────────────────────

    fn emit_model_decl(name: &str, layers: &[LayerKind]) -> String {
        let mut lines: Vec<String> = Vec::new();
        lines.push(format!("// [ModelDecl] model {}", name));
        lines.push(format!("// Burn MLP 모델 (nn 모듈로 자동 생성)"));
        for (i, layer) in layers.iter().enumerate() {
            lines.push(format!("//   [{}] {}", i, layer.to_burn_str()));
        }
        lines.push(format!(
            "// #[derive(Module, Debug)] struct {}<B: Backend> {{ ... }}",
            name
        ));
        lines.push(String::new());
        lines.push(format!(
            "// let mut model = {}::new(&device, input_dim).train():",
            name
        ));
        lines.join("\n")
    }

    // ── TrainStmt 변환 ────────────────────────────────────────────────────────

    fn emit_train_stmt(source_var: &str, model_name: &str, config: &TrainConfig) -> String {
        let batch_str = match config.batch_size {
            Some(b) => format!("{}", b),
            None => "전체".to_string(),
        };
        let val_str = match config.validation_split {
            Some(v) => format!("{:.1}%", v * 100.0),
            None => "없음".to_string(),
        };
        let mut lines: Vec<String> = Vec::new();
        lines.push(format!(
            "// [TrainStmt] run {} |> train({}, target: \"{}\", epochs: {}, lr: {})",
            source_var, model_name, config.target, config.epochs, config.learning_rate
        ));
        lines.push(format!("// 데이터 소스: {}", source_var));
        lines.push(format!("// 모델: {}", model_name));
        lines.push(format!("// 타겟 컬럼: {}", config.target));
        lines.push(format!("// 에포크: {}", config.epochs));
        lines.push(format!("// 학습률: {}", config.learning_rate));
        lines.push(format!("// 배치 크기: {}", batch_str));
        lines.push(format!("// 검증 비율: {}", val_str));
        lines.push(String::new());
        lines.push(format!(
            "// let mut model = {}::new(&device, input_dim);",
            model_name
        ));
        lines.push(format!(
            "// let mut optim = AdamConfig::new().init();  // Burn Adam",
        ));
        lines.push(format!(
            "// for _ in 0..{} {{ grads = loss.backward(); model = optim.step(lr, model, grads); }}",
            config.epochs
        ));
        lines.join("\n")
    }

    // ── Op 변환 ───────────────────────────────────────────────────────────────

    fn emit_op(op: &PipelineOp) -> String {
        match op {
            // ── 기존 ──────────────────────────────────────────────────────────
            PipelineOp::Filter(expr) => {
                format!(
                    "  .filter({})  // |> filter({})",
                    Self::expr_to_polars(expr),
                    Self::expr_to_xzz(expr)
                )
            }
            PipelineOp::Select(cols) => {
                let polars: Vec<String> = cols
                    .iter()
                    .map(|c| format!("col(\"{}\")", esc(c)))
                    .collect();
                let xzz = cols.join(", ");
                format!(
                    "  .select([{}])  // |> select([{}])",
                    polars.join(", "),
                    xzz
                )
            }
            PipelineOp::Count(None) => "  // |> count  →  df.height() 로 행 수 확인".to_string(),
            PipelineOp::Count(Some(col)) => {
                format!(
                    "  .agg([col(\"{}\").count()])  // |> count(\"{}\")",
                    esc(col),
                    esc(col)
                )
            }

            // ── v0.16 집계 ────────────────────────────────────────────────────
            PipelineOp::GroupBy(group_col) => {
                format!(
                    "  .group_by([col(\"{}\")])  // |> groupBy(\"{}\")",
                    esc(group_col),
                    esc(group_col)
                )
            }
            PipelineOp::Sum(agg_col) => {
                format!(
                    "  .agg([col(\"{}\").sum()])  // |> sum(\"{}\")",
                    esc(agg_col),
                    esc(agg_col)
                )
            }
            PipelineOp::Mean(agg_col) => {
                format!(
                    "  .agg([col(\"{}\").mean()])  // |> mean(\"{}\")",
                    esc(agg_col),
                    esc(agg_col)
                )
            }
            PipelineOp::Min(agg_col) => {
                format!(
                    "  .agg([col(\"{}\").min()])  // |> min(\"{}\")",
                    esc(agg_col),
                    esc(agg_col)
                )
            }
            PipelineOp::Max(agg_col) => {
                format!(
                    "  .agg([col(\"{}\").max()])  // |> max(\"{}\")",
                    esc(agg_col),
                    esc(agg_col)
                )
            }

            // ── v0.16 정렬 / 슬라이싱 ─────────────────────────────────────────
            PipelineOp::OrderBy { col, desc } => {
                format!(
                    "  .sort([\"{}\"], SortMultipleOptions::default().with_order_descending({}))  // |> orderBy(\"{}\", desc: {})",
                    esc(col),
                    desc,
                    esc(col),
                    desc
                )
            }
            PipelineOp::Take(n) => {
                format!("  .limit({})  // |> take({})", n, n)
            }

            // ── v0.16 Null 처리 ────────────────────────────────────────────────
            PipelineOp::DropNull(drop_col) => {
                format!(
                    "  .drop_nulls(Some(vec![col(\"{}\")]))  // |> dropNull(\"{}\")",
                    esc(drop_col),
                    esc(drop_col)
                )
            }
            PipelineOp::FillNull { col, value } => {
                let lit_str = match value {
                    FillNullValue::Mean => format!("col(\"{}\").mean()", esc(col)),
                    FillNullValue::Median => format!("col(\"{}\").median()", esc(col)),
                    FillNullValue::Zero => "lit(0)".to_string(),
                    FillNullValue::Int(n) => format!("lit({}i64)", n),
                    FillNullValue::Float(f) => format!("lit({}f64)", f),
                    FillNullValue::Str(s) => format!("lit(\"{}\")", esc(s)),
                };
                format!(
                    "  .with_columns([col(\"{}\").fill_null({})])  // |> fillNull(\"{}\", ...)",
                    esc(col),
                    lit_str,
                    esc(col)
                )
            }

            // ── v0.16+ / v0.21 Join ──────────────────────────────────────────
            PipelineOp::Join {
                other,
                left_on,
                right_on,
                how,
            } => {
                let left_cols: Vec<String> = left_on
                    .iter()
                    .map(|k| format!("col(\"{}\")", esc(k)))
                    .collect();
                let right_cols: Vec<String> = right_on
                    .iter()
                    .map(|k| format!("col(\"{}\")", esc(k)))
                    .collect();
                let left_str = left_cols.join(", ");
                let right_str = right_cols.join(", ");
                format!(
                    "  .join({}.lazy(), [{}], [{}], JoinArgs::new({}))  // |> join({}, left_on: {:?}, right_on: {:?}, how: {:?})",
                    other,
                    left_str,
                    right_str,
                    how.as_polars_str(),
                    other,
                    left_on,
                    right_on,
                    how
                )
            }

            // ── v0.16+ WithColumn ─────────────────────────────────────────────
            PipelineOp::WithColumn { name, expr } => {
                format!(
                    "  .with_columns([{}.alias(\"{}\")])  // |> withColumn(\"{}\", {})",
                    Self::expr_to_polars(expr),
                    esc(name),
                    esc(name),
                    Self::expr_to_xzz(expr)
                )
            }

            // ── Chart: codegen 미지원 (런타임에서만 처리) ─────────────────────
            PipelineOp::Chart(config) => {
                format!(
                    "  // |> chart {{ type: {} }}  →  [xazz:chart] JSON 출력",
                    config.chart_type.as_str()
                )
            }

            // ── v0.20 Cast ───────────────────────────────────────────────────
            PipelineOp::Cast { col, to_type } => {
                let polars_type = match to_type.as_str() {
                    "float" => "DataType::Float64",
                    "int" => "DataType::Int64",
                    "str" => "DataType::String",
                    "bool" => "DataType::Boolean",
                    other => other,
                };
                format!(
                    "  .with_columns([col(\"{}\").cast({})])  // |> cast(\"{}\", \"{}\")",
                    esc(col),
                    polars_type,
                    esc(col),
                    esc(to_type)
                )
            }

            // ── Rename ───────────────────────────────────────────────────────
            PipelineOp::Rename { old_name, new_name } => {
                format!(
                    "  .rename([\"{}\"], [\"{}\"], false)  // |> rename(\"{}\", \"{}\")",
                    esc(old_name),
                    esc(new_name),
                    esc(old_name),
                    esc(new_name)
                )
            }

            // ── Replace ──────────────────────────────────────────────────────
            PipelineOp::Replace { col, from, to } => {
                format!(
                    "  .with_columns([col(\"{}\").str().replace(lit(\"{}\"), lit(\"{}\"), false).alias(\"{}\")])  // |> replace(\"{}\", \"{}\", \"{}\")",
                    esc(col),
                    esc(from),
                    esc(to),
                    esc(col),
                    esc(col),
                    esc(from),
                    esc(to)
                )
            }

            // ── v0.22 sample(n) / sample(n, seed: 42) — 무작위 샘플링 ───────
            PipelineOp::Sample { n, seed } => match seed {
                Some(s) => format!(
                    "  .collect()?.sample_n_literal({}, false, false, Some({}))?.lazy()  // |> sample({}, seed: {})",
                    n, s, n, s
                ),
                None => format!(
                    "  .collect()?.sample_n_literal({}, false, false, None)?.lazy()  // |> sample({})",
                    n, n
                ),
            },

            // ── v0.22 median / variance / std 집계 ──────────────────────────
            PipelineOp::Median(agg_col) => {
                format!(
                    "  .agg([col(\"{}\").median()])  // |> median(\"{}\")",
                    agg_col, agg_col
                )
            }
            PipelineOp::Variance(agg_col) => {
                format!(
                    "  .agg([col(\"{}\").var(1)])  // |> variance(\"{}\")",
                    agg_col, agg_col
                )
            }
            PipelineOp::Std(agg_col) => {
                format!(
                    "  .agg([col(\"{}\").std(1)])  // |> std(\"{}\")",
                    agg_col, agg_col
                )
            }
            PipelineOp::Train { model_name, config } => format!(
                "  // |> train({}, target: \"{}\", epochs: {}, lr: {})  → 학습된 모델 변수",
                model_name, config.target, config.epochs, config.learning_rate
            ),
            PipelineOp::Predict { model_var, as_col } => {
                let as_str = as_col
                    .as_deref()
                    .map(|c| format!(", as: \"{}\"", c))
                    .unwrap_or_default();
                format!("  // |> predict({}{})  → 예측 컬럼 추가", model_var, as_str)
            }

            // ── v0.6 withDp — 차등 프라이버시 노이즈 주입 ───────────────────
            PipelineOp::WithDp(args) => format!(
                "  .collect()?  → dp::apply_dp(ε={}, {}, Δf={})  // |> withDp(epsilon: {})",
                args.epsilon,
                args.mechanism.as_str(),
                args.sensitivity,
                args.epsilon
            ),
        }
    }

    // ── 표현식 → Polars Rust ──────────────────────────────────────────────────

    pub fn expr_to_polars(expr: &Expr) -> String {
        crate::polars_text::expr_to_polars(expr, None)
    }

    // ── 표현식 → xazzLang 소스 표현 ──────────────────────────────────────────

    pub fn expr_to_xzz(expr: &Expr) -> String {
        match expr {
            Expr::Ident(s) => s.clone(),
            Expr::StringLit(s) => format!("\"{}\"", s),
            Expr::IntLit(n) => n.to_string(),
            Expr::FloatLit(f) => f.to_string(),
            Expr::BoolLit(b) => b.to_string(),
            Expr::BinOp { lhs, op, rhs } => {
                let op_str = match op {
                    BinOpKind::Eq => "==",
                    BinOpKind::NotEq => "!=",
                    BinOpKind::Lt => "<",
                    BinOpKind::Gt => ">",
                    BinOpKind::LtEq => "<=",
                    BinOpKind::GtEq => ">=",
                    BinOpKind::Add => "+",
                    BinOpKind::Sub => "-",
                    BinOpKind::Mul => "*",
                    BinOpKind::Div => "/",
                };
                format!(
                    "{} {} {}",
                    Self::expr_to_xzz(lhs),
                    op_str,
                    Self::expr_to_xzz(rhs)
                )
            }
        }
    }
}

impl Default for Codegen {
    fn default() -> Self {
        Codegen::new()
    }
}

// ── Codegen 유닛 테스트 ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{
        BinOpKind, ChartConfig, ChartType, Expr, FillNullValue, JoinHow, PipelineOp,
        PipelineSource, Program, Stmt, StructField,
    };

    /// 단일 VarDecl (Load 소스)을 갖는 Program 생성 헬퍼
    fn make_load_program(ops: Vec<PipelineOp>) -> Program {
        let mut p = Program::new();
        p.stmts.push(Stmt::VarDecl {
            var_name: "result".into(),
            is_mut: false,
            source: PipelineSource::Load {
                file_path: "data.csv".into(),
                schema_name: "MySchema".into(),
            },
            ops,
        });
        p
    }

    // ── expr_to_polars 출력 검증 ───────────────────────────────────────────────

    /// Ident → col("...") 변환
    #[test]
    fn test_expr_to_polars_ident() {
        assert_eq!(
            Codegen::expr_to_polars(&Expr::Ident("pm10".into())),
            "col(\"pm10\")"
        );
    }

    /// IntLit → lit(...i64) 변환
    #[test]
    fn test_expr_to_polars_int_lit() {
        assert_eq!(Codegen::expr_to_polars(&Expr::IntLit(42)), "lit(42i64)");
    }

    /// FloatLit → lit(...f64) 변환
    #[test]
    fn test_expr_to_polars_float_lit() {
        assert_eq!(Codegen::expr_to_polars(&Expr::FloatLit(2.5)), "lit(2.5f64)");
    }

    /// BoolLit → lit(true/false) 변환
    #[test]
    fn test_expr_to_polars_bool_lit() {
        assert_eq!(Codegen::expr_to_polars(&Expr::BoolLit(true)), "lit(true)");
        assert_eq!(Codegen::expr_to_polars(&Expr::BoolLit(false)), "lit(false)");
    }

    /// StringLit → lit("...") 변환
    #[test]
    fn test_expr_to_polars_string_lit() {
        assert_eq!(
            Codegen::expr_to_polars(&Expr::StringLit("hello".into())),
            "lit(\"hello\")"
        );
    }

    /// StringLit 내 따옴표/백슬래시는 이스케이프되어야 유효한 Rust 코드가 된다
    #[test]
    fn test_expr_to_polars_string_lit_escapes_quotes_and_backslashes() {
        assert_eq!(
            Codegen::expr_to_polars(&Expr::StringLit("a\"b".into())),
            "lit(\"a\\\"b\")"
        );
        assert_eq!(
            Codegen::expr_to_polars(&Expr::StringLit("a\\b".into())),
            "lit(\"a\\\\b\")"
        );
        assert_eq!(
            Codegen::expr_to_polars(&Expr::Ident("x\"y".into())),
            "col(\"x\\\"y\")"
        );
    }

    /// BinOp Gt → col(...).gt(lit(...)) 변환
    #[test]
    fn test_expr_to_polars_binop_gt() {
        let expr = Expr::BinOp {
            lhs: Box::new(Expr::Ident("pm10".into())),
            op: BinOpKind::Gt,
            rhs: Box::new(Expr::IntLit(50)),
        };
        assert_eq!(
            Codegen::expr_to_polars(&expr),
            "col(\"pm10\").gt(lit(50i64))"
        );
    }

    /// BinOp Eq → .eq(...) 변환
    #[test]
    fn test_expr_to_polars_binop_eq() {
        let expr = Expr::BinOp {
            lhs: Box::new(Expr::Ident("support".into())),
            op: BinOpKind::Eq,
            rhs: Box::new(Expr::BoolLit(false)),
        };
        assert_eq!(
            Codegen::expr_to_polars(&expr),
            "col(\"support\").eq(lit(false))"
        );
    }

    /// BinOp Add → .add(...) 변환 (산술 연산자)
    #[test]
    fn test_expr_to_polars_binop_add() {
        let expr = Expr::BinOp {
            lhs: Box::new(Expr::Ident("a".into())),
            op: BinOpKind::Add,
            rhs: Box::new(Expr::Ident("b".into())),
        };
        assert_eq!(Codegen::expr_to_polars(&expr), "col(\"a\").add(col(\"b\"))");
    }

    /// BinOp Mul → .mul(...) 변환
    #[test]
    fn test_expr_to_polars_binop_mul() {
        let expr = Expr::BinOp {
            lhs: Box::new(Expr::Ident("price".into())),
            op: BinOpKind::Mul,
            rhs: Box::new(Expr::IntLit(2)),
        };
        let result = Codegen::expr_to_polars(&expr);
        assert!(result.contains(".mul("), ".mul( 없음: {}", result);
    }

    // ── expr_to_xzz 출력 검증 ─────────────────────────────────────────────────

    /// Ident → 식별자 문자열 그대로
    #[test]
    fn test_expr_to_xzz_ident() {
        assert_eq!(Codegen::expr_to_xzz(&Expr::Ident("pm10".into())), "pm10");
    }

    /// BinOp Gt → "lhs > rhs"
    #[test]
    fn test_expr_to_xzz_binop_gt() {
        let expr = Expr::BinOp {
            lhs: Box::new(Expr::Ident("age".into())),
            op: BinOpKind::Gt,
            rhs: Box::new(Expr::IntLit(18)),
        };
        assert_eq!(Codegen::expr_to_xzz(&expr), "age > 18");
    }

    /// BinOp Add → "lhs + rhs"
    #[test]
    fn test_expr_to_xzz_binop_add() {
        let expr = Expr::BinOp {
            lhs: Box::new(Expr::Ident("a".into())),
            op: BinOpKind::Add,
            rhs: Box::new(Expr::Ident("b".into())),
        };
        assert_eq!(Codegen::expr_to_xzz(&expr), "a + b");
    }

    // ── generate() 전체 파이프라인 출력 검증 ──────────────────────────────────

    /// TypeDecl → Schema 주석 블록 생성
    #[test]
    fn test_generate_type_decl_comment() {
        let mut p = Program::new();
        p.stmts.push(Stmt::TypeDecl {
            name: "AirQuality".into(),
            fields: vec![
                StructField {
                    name: "station".into(),
                    field_type: "string".into(),
                },
                StructField {
                    name: "pm10".into(),
                    field_type: "Option<float>".into(),
                },
            ],
        });
        let output = Codegen::generate(&p);
        assert!(
            output.contains("// [Schema] AirQuality"),
            "Schema 주석 없음: {}",
            output
        );
        assert!(output.contains("station"), "station 필드 없음");
        assert!(output.contains("Option<float>"), "Option<float> 없음");
    }

    /// VarDecl (Load 소스) → LazyCsvReader 코드 생성
    #[test]
    fn test_generate_var_decl_load_source() {
        let program = make_load_program(vec![]);
        let output = Codegen::generate(&program);
        assert!(
            output.contains("LazyCsvReader::new(\"data.csv\")"),
            "LazyCsvReader 없음: {}",
            output
        );
        assert!(output.contains("result"), "변수명 result 없음");
    }

    /// VarDecl → .collect()? 종결자 포함
    #[test]
    fn test_generate_ends_with_collect() {
        let program = make_load_program(vec![]);
        let output = Codegen::generate(&program);
        assert!(
            output.contains(".collect()?"),
            ".collect()? 없음: {}",
            output
        );
    }

    /// VarDecl (VarRef 소스) → .clone().lazy() 코드 생성
    #[test]
    fn test_generate_var_ref_source() {
        let mut p = Program::new();
        p.stmts.push(Stmt::VarDecl {
            var_name: "filtered".into(),
            is_mut: false,
            source: PipelineSource::VarRef("air".into()),
            ops: vec![],
        });
        let output = Codegen::generate(&p);
        assert!(
            output.contains("air.clone().lazy()"),
            "clone().lazy() 없음: {}",
            output
        );
    }

    /// mut 변수 선언 → 주석에 "mut " 포함
    #[test]
    fn test_generate_mut_var_comment() {
        let mut p = Program::new();
        p.stmts.push(Stmt::VarDecl {
            var_name: "data".into(),
            is_mut: true,
            source: PipelineSource::Load {
                file_path: "f.csv".into(),
                schema_name: "S".into(),
            },
            ops: vec![],
        });
        let output = Codegen::generate(&p);
        assert!(output.contains("mut v data"), "mut v 없음: {}", output);
    }

    /// Filter op → .filter(...) 출력
    #[test]
    fn test_generate_filter_op() {
        let program = make_load_program(vec![PipelineOp::Filter(Expr::BinOp {
            lhs: Box::new(Expr::Ident("pm10".into())),
            op: BinOpKind::Gt,
            rhs: Box::new(Expr::IntLit(50)),
        })]);
        let output = Codegen::generate(&program);
        assert!(output.contains(".filter("), ".filter( 없음: {}", output);
        assert!(output.contains("pm10"), "pm10 없음");
    }

    /// Select op → .select([col(...)]) 출력
    #[test]
    fn test_generate_select_op() {
        let program = make_load_program(vec![PipelineOp::Select(vec![
            "station".into(),
            "date".into(),
        ])]);
        let output = Codegen::generate(&program);
        assert!(output.contains(".select(["), ".select([ 없음: {}", output);
        assert!(output.contains("col(\"station\")"), "col(\"station\") 없음");
        assert!(output.contains("col(\"date\")"), "col(\"date\") 없음");
    }

    /// GroupBy op → .group_by([col(...)]) 출력
    #[test]
    fn test_generate_group_by_op() {
        let program = make_load_program(vec![PipelineOp::GroupBy("region".into())]);
        let output = Codegen::generate(&program);
        assert!(
            output.contains(".group_by([col(\"region\")])"),
            "group_by 없음: {}",
            output
        );
    }

    /// Sum op → .agg([col(...).sum()]) 출력
    #[test]
    fn test_generate_sum_op() {
        let program = make_load_program(vec![PipelineOp::Sum("pop".into())]);
        let output = Codegen::generate(&program);
        assert!(output.contains(".sum()"), ".sum() 없음: {}", output);
    }

    /// Mean op → .agg([col(...).mean()]) 출력
    #[test]
    fn test_generate_mean_op() {
        let program = make_load_program(vec![PipelineOp::Mean("score".into())]);
        let output = Codegen::generate(&program);
        assert!(output.contains(".mean()"), ".mean() 없음: {}", output);
    }

    /// OrderBy desc:true → with_order_descending(true)
    #[test]
    fn test_generate_order_by_desc() {
        let program = make_load_program(vec![PipelineOp::OrderBy {
            col: "income".into(),
            desc: true,
        }]);
        let output = Codegen::generate(&program);
        assert!(
            output.contains("with_order_descending(true)"),
            "with_order_descending 없음: {}",
            output
        );
    }

    /// Take(10) → .limit(10)
    #[test]
    fn test_generate_take_op() {
        let program = make_load_program(vec![PipelineOp::Take(10)]);
        let output = Codegen::generate(&program);
        assert!(output.contains(".limit(10)"), ".limit(10) 없음: {}", output);
    }

    /// DropNull → .drop_nulls(...)
    #[test]
    fn test_generate_drop_null() {
        let program = make_load_program(vec![PipelineOp::DropNull("pm10".into())]);
        let output = Codegen::generate(&program);
        assert!(
            output.contains(".drop_nulls("),
            ".drop_nulls 없음: {}",
            output
        );
    }

    /// FillNull Int → fill_null(lit(0i64))
    #[test]
    fn test_generate_fill_null_int() {
        let program = make_load_program(vec![PipelineOp::FillNull {
            col: "pm10".into(),
            value: FillNullValue::Int(0),
        }]);
        let output = Codegen::generate(&program);
        assert!(
            output.contains("fill_null(lit(0i64))"),
            "fill_null 없음: {}",
            output
        );
    }

    /// FillNull Float → fill_null(lit(...f64))
    #[test]
    fn test_generate_fill_null_float() {
        let program = make_load_program(vec![PipelineOp::FillNull {
            col: "score".into(),
            value: FillNullValue::Float(0.0),
        }]);
        let output = Codegen::generate(&program);
        assert!(output.contains("f64"), "f64 없음: {}", output);
    }

    /// Join op → .join(...) + JoinType::Inner
    #[test]
    fn test_generate_join_op() {
        let program = make_load_program(vec![PipelineOp::Join {
            other: "right".into(),
            left_on: vec!["id".into()],
            right_on: vec!["id".into()],
            how: JoinHow::Inner,
        }]);
        let output = Codegen::generate(&program);
        assert!(output.contains(".join("), ".join( 없음: {}", output);
        assert!(
            output.contains("JoinType::Inner"),
            "JoinType::Inner 없음: {}",
            output
        );
    }

    /// Cast "float" → DataType::Float64
    #[test]
    fn test_generate_cast_float() {
        let program = make_load_program(vec![PipelineOp::Cast {
            col: "pm10".into(),
            to_type: "float".into(),
        }]);
        let output = Codegen::generate(&program);
        assert!(
            output.contains("DataType::Float64"),
            "DataType::Float64 없음: {}",
            output
        );
    }

    /// Cast "int" → DataType::Int64
    #[test]
    fn test_generate_cast_int() {
        let program = make_load_program(vec![PipelineOp::Cast {
            col: "count".into(),
            to_type: "int".into(),
        }]);
        let output = Codegen::generate(&program);
        assert!(
            output.contains("DataType::Int64"),
            "DataType::Int64 없음: {}",
            output
        );
    }

    /// Rename → .rename(["old"], ["new"], false)
    #[test]
    fn test_generate_rename_op() {
        let program = make_load_program(vec![PipelineOp::Rename {
            old_name: "old_col".into(),
            new_name: "new_col".into(),
        }]);
        let output = Codegen::generate(&program);
        assert!(
            output.contains(".rename([\"old_col\"], [\"new_col\"]"),
            ".rename 없음: {}",
            output
        );
    }

    /// Replace → .str().replace(...)
    #[test]
    fn test_generate_replace_op() {
        let program = make_load_program(vec![PipelineOp::Replace {
            col: "code".into(),
            from: ".".into(),
            to: "".into(),
        }]);
        let output = Codegen::generate(&program);
        assert!(
            output.contains(".str().replace("),
            ".str().replace 없음: {}",
            output
        );
    }

    /// Chart op → [xazz:chart] JSON 출력 주석
    #[test]
    fn test_generate_chart_op_comment() {
        let program = make_load_program(vec![PipelineOp::Chart(ChartConfig {
            chart_type: ChartType::Bar,
            title: None,
            x: Some("region".into()),
            y: Some("count".into()),
            label: None,
            value: None,
        })]);
        let output = Codegen::generate(&program);
        assert!(
            output.contains("[xazz:chart]"),
            "chart JSON 출력 주석 없음: {}",
            output
        );
    }

    /// Count(None) → 전체 행 수 주석
    #[test]
    fn test_generate_count_none() {
        let program = make_load_program(vec![PipelineOp::Count(None)]);
        let output = Codegen::generate(&program);
        assert!(output.contains("|> count"), "|> count 없음: {}", output);
    }

    /// Count(Some(col)) → .agg([col(...).count()])
    #[test]
    fn test_generate_count_some() {
        let program = make_load_program(vec![PipelineOp::Count(Some("population".into()))]);
        let output = Codegen::generate(&program);
        assert!(output.contains(".count()"), ".count() 없음: {}", output);
    }

    /// WithColumn → .with_columns([expr.alias("name")])
    #[test]
    fn test_generate_with_column_op() {
        let program = make_load_program(vec![PipelineOp::WithColumn {
            name: "total".into(),
            expr: Expr::BinOp {
                lhs: Box::new(Expr::Ident("a".into())),
                op: BinOpKind::Add,
                rhs: Box::new(Expr::Ident("b".into())),
            },
        }]);
        let output = Codegen::generate(&program);
        assert!(
            output.contains(".with_columns("),
            ".with_columns( 없음: {}",
            output
        );
        assert!(
            output.contains(".alias(\"total\")"),
            ".alias(\"total\") 없음"
        );
    }

    /// Sample(n) (seed 없음) → .sample_n_literal(n, false, false, None)
    #[test]
    fn test_generate_sample_op() {
        let program = make_load_program(vec![PipelineOp::Sample { n: 100, seed: None }]);
        let output = Codegen::generate(&program);
        assert!(
            output.contains(".sample_n_literal(100, false, false, None)"),
            "sample_n_literal 없음: {}",
            output
        );
    }

    /// Sample(n, seed) → .sample_n_literal(n, false, false, Some(seed))
    #[test]
    fn test_generate_sample_op_with_seed() {
        let program = make_load_program(vec![PipelineOp::Sample {
            n: 100,
            seed: Some(42),
        }]);
        let output = Codegen::generate(&program);
        assert!(
            output.contains(".sample_n_literal(100, false, false, Some(42))"),
            "sample_n_literal with seed 없음: {}",
            output
        );
    }

    /// Median op → .agg([col(...).median()]) 출력
    #[test]
    fn test_generate_median_op() {
        let program = make_load_program(vec![PipelineOp::Median("score".into())]);
        let output = Codegen::generate(&program);
        assert!(output.contains(".median()"), ".median() 없음: {}", output);
    }

    /// Variance op → .agg([col(...).var(1)]) 출력
    #[test]
    fn test_generate_variance_op() {
        let program = make_load_program(vec![PipelineOp::Variance("score".into())]);
        let output = Codegen::generate(&program);
        assert!(output.contains(".var(1)"), ".var(1) 없음: {}", output);
    }

    /// Std op → .agg([col(...).std(1)]) 출력
    #[test]
    fn test_generate_std_op() {
        let program = make_load_program(vec![PipelineOp::Std("score".into())]);
        let output = Codegen::generate(&program);
        assert!(output.contains(".std(1)"), ".std(1) 없음: {}", output);
    }
}
