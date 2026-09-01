/// xazzLang - code generator (v0.16)
///
/// Generates a Polars LazyFrame flow string from a Program AST.
///
/// [v0.16 changes]
///   - BoolLit expression support (true/false)
///   - Count(None) / Count(Some(col)) distinction
///   - New operator output: GroupBy, Sum, Mean, Min, Max, OrderBy, Take, DropNull, FillNull
///   - Join operator: .join(..., ..., JoinArgs::new(JoinType::Inner)) mapping
///   - WithColumn operator: .with_columns([expr.alias("name")]) mapping
///   - Arithmetic operators: Add/Sub/Mul/Div → .add()/.sub()/.mul()/.div()
///
/// Deduplication (single location):
///   - The expression → Polars string mapping uses the single implementation in
///     `crate::polars_text` (shared by this module and emitter.rs).
///   - The runtime op→Polars mapping exists in only one place: xazz-exec/src/lower.rs (Typed IR).
use crate::ast::{
    BinOpKind, Expr, LayerKind, PipelineOp, PipelineSource, Program, Stmt, TrainConfig,
};

/// Code generator — unit struct
pub struct Codegen;

/// Escapes a DSL string value for insertion into a generated Rust string literal.
/// (escapes `"` and `\` to prevent generated code injection)
fn esc(s: &str) -> String {
    crate::policy::printer::escape(s)
}

impl Codegen {
    pub fn new() -> Self {
        Codegen
    }

    // ── top-level entry point ─────────────────────────────────────────────────────────

    /// Generates a Polars flow string from the entire Program AST
    pub fn generate(program: &Program) -> String {
        let mut out = String::new();
        out.push_str("// ═══════════════════════════════════════════════════════\n");
        out.push_str("// xazzLang → Polars LazyFrame flow mapping\n");
        out.push_str("// ═══════════════════════════════════════════════════════\n\n");

        for (i, stmt) in program.stmts.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&Self::emit_stmt(stmt));
        }
        out
    }

    // ── Stmt conversion ─────────────────────────────────────────────────────────────

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

    // ── VarDecl conversion ──────────────────────────────────────────────────────────

    fn emit_var_decl(
        var_name: &str,
        is_mut: bool,
        source: &PipelineSource,
        ops: &[PipelineOp],
    ) -> String {
        let mut lines: Vec<String> = Vec::new();

        // comment header
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

        // generate source code
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

        // each pipeline stage
        for op in ops {
            lines.push(Self::emit_op(op));
        }

        // collect — point of lazy execution
        lines.push(format!(
            "  .collect()?;  // ← {}: runs all operations at once",
            var_name
        ));

        lines.join("\n")
    }

    // ── ExprStmt conversion ─────────────────────────────────────────────────────────

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

        lines.push("  .collect()?;  // ← expression statement execution".to_string());
        lines.join("\n")
    }

    // ── ModelDecl conversion ────────────────────────────────────────────────────────

    fn emit_model_decl(name: &str, layers: &[LayerKind]) -> String {
        let mut lines: Vec<String> = Vec::new();
        lines.push(format!("// [ModelDecl] model {}", name));
        lines.push(format!("// Burn MLP model (auto-generated nn module)"));
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

    // ── TrainStmt conversion ────────────────────────────────────────────────────────

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
        lines.push(format!("// data source: {}", source_var));
        lines.push(format!("// model: {}", model_name));
        lines.push(format!("// target column: {}", config.target));
        lines.push(format!("// epochs: {}", config.epochs));
        lines.push(format!("// learning rate: {}", config.learning_rate));
        lines.push(format!("// batch size: {}", batch_str));
        lines.push(format!("// validation split: {}", val_str));
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

    // ── Op conversion ───────────────────────────────────────────────────────────────

    fn emit_op(op: &PipelineOp) -> String {
        match op {
            // ── existing ──────────────────────────────────────────────────────────
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
            PipelineOp::Count(None) => {
                "  // |> count  →  check row count via df.height()".to_string()
            }
            PipelineOp::Count(Some(col)) => {
                format!(
                    "  .agg([{}])  // |> count(\"{}\")",
                    crate::polars_text::agg_expr_to_polars(crate::ir::AggKind::Count, col),
                    esc(col)
                )
            }

            // ── v0.16 aggregates ────────────────────────────────────────────────────
            PipelineOp::GroupBy(group_col) => {
                format!(
                    "  .group_by([col(\"{}\")])  // |> groupBy(\"{}\")",
                    esc(group_col),
                    esc(group_col)
                )
            }
            PipelineOp::Sum(agg_col) => {
                format!(
                    "  .agg([{}])  // |> sum(\"{}\")",
                    crate::polars_text::agg_expr_to_polars(crate::ir::AggKind::Sum, agg_col),
                    esc(agg_col)
                )
            }
            PipelineOp::Mean(agg_col) => {
                format!(
                    "  .agg([{}])  // |> mean(\"{}\")",
                    crate::polars_text::agg_expr_to_polars(crate::ir::AggKind::Mean, agg_col),
                    esc(agg_col)
                )
            }
            PipelineOp::Min(agg_col) => {
                format!(
                    "  .agg([{}])  // |> min(\"{}\")",
                    crate::polars_text::agg_expr_to_polars(crate::ir::AggKind::Min, agg_col),
                    esc(agg_col)
                )
            }
            PipelineOp::Max(agg_col) => {
                format!(
                    "  .agg([{}])  // |> max(\"{}\")",
                    crate::polars_text::agg_expr_to_polars(crate::ir::AggKind::Max, agg_col),
                    esc(agg_col)
                )
            }

            // ── v0.16 sorting / slicing ─────────────────────────────────────────
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

            // ── v0.16 Null handling ────────────────────────────────────────────────
            PipelineOp::DropNull(drop_col) => {
                format!(
                    "  .drop_nulls(Some(vec![col(\"{}\")]))  // |> dropNull(\"{}\")",
                    esc(drop_col),
                    esc(drop_col)
                )
            }
            PipelineOp::FillNull { col, value } => {
                let lit_str = crate::polars_text::fill_value_to_polars(value, col);
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

            // ── Chart: unsupported by codegen (runtime only) ─────────────────────
            PipelineOp::Chart(config) => {
                format!(
                    "  // |> chart {{ type: {} }}  →  [xazz:chart] JSON output",
                    config.chart_type.as_str()
                )
            }

            // ── v0.20 Cast ───────────────────────────────────────────────────
            PipelineOp::Cast { col, to_type } => {
                let polars_type = crate::polars_text::cast_dtype_to_polars(to_type);
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

            // ── v0.22 sample(n) / sample(n, seed: 42) — random sampling ───────
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

            // ── v0.22 median / variance / std aggregates ──────────────────────────
            PipelineOp::Median(agg_col) => {
                format!(
                    "  .agg([{}])  // |> median(\"{}\")",
                    crate::polars_text::agg_expr_to_polars(crate::ir::AggKind::Median, agg_col),
                    agg_col
                )
            }
            PipelineOp::Variance(agg_col) => {
                format!(
                    "  .agg([{}])  // |> variance(\"{}\")",
                    crate::polars_text::agg_expr_to_polars(crate::ir::AggKind::Variance, agg_col),
                    agg_col
                )
            }
            PipelineOp::Std(agg_col) => {
                format!(
                    "  .agg([{}])  // |> std(\"{}\")",
                    crate::polars_text::agg_expr_to_polars(crate::ir::AggKind::Std, agg_col),
                    agg_col
                )
            }
            PipelineOp::Train { model_name, config } => format!(
                "  // |> train({}, target: \"{}\", epochs: {}, lr: {})  → trained model variable",
                model_name, config.target, config.epochs, config.learning_rate
            ),
            PipelineOp::Predict { model_var, as_col } => {
                let as_str = as_col
                    .as_deref()
                    .map(|c| format!(", as: \"{}\"", c))
                    .unwrap_or_default();
                format!(
                    "  // |> predict({}{})  → prediction column added",
                    model_var, as_str
                )
            }

            // ── v0.6 withDp — differential privacy noise injection ───────────────────
            PipelineOp::WithDp(args) => format!(
                "  .collect()?  → dp::apply_dp(ε={}, {}, Δf={})  // |> withDp(epsilon: {})",
                args.epsilon,
                args.mechanism.as_str(),
                args.sensitivity,
                args.epsilon
            ),
        }
    }

    // ── expression → Polars Rust ──────────────────────────────────────────────────

    pub fn expr_to_polars(expr: &Expr) -> String {
        crate::polars_text::expr_to_polars(expr, None)
    }

    // ── expression → xazzLang source representation ──────────────────────────────────────────

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

// ── Codegen unit tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{
        BinOpKind, ChartConfig, ChartType, Expr, FillNullValue, JoinHow, PipelineOp,
        PipelineSource, Program, Stmt, StructField,
    };

    /// Helper that builds a Program with a single VarDecl (Load source)
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

    // ── expr_to_polars output verification ───────────────────────────────────────────────

    /// Ident → col("...") conversion
    #[test]
    fn test_expr_to_polars_ident() {
        assert_eq!(
            Codegen::expr_to_polars(&Expr::Ident("pm10".into())),
            "col(\"pm10\")"
        );
    }

    /// IntLit → lit(...i64) conversion
    #[test]
    fn test_expr_to_polars_int_lit() {
        assert_eq!(Codegen::expr_to_polars(&Expr::IntLit(42)), "lit(42i64)");
    }

    /// FloatLit → lit(...f64) conversion
    #[test]
    fn test_expr_to_polars_float_lit() {
        assert_eq!(Codegen::expr_to_polars(&Expr::FloatLit(2.5)), "lit(2.5f64)");
    }

    /// BoolLit → lit(true/false) conversion
    #[test]
    fn test_expr_to_polars_bool_lit() {
        assert_eq!(Codegen::expr_to_polars(&Expr::BoolLit(true)), "lit(true)");
        assert_eq!(Codegen::expr_to_polars(&Expr::BoolLit(false)), "lit(false)");
    }

    /// StringLit → lit("...") conversion
    #[test]
    fn test_expr_to_polars_string_lit() {
        assert_eq!(
            Codegen::expr_to_polars(&Expr::StringLit("hello".into())),
            "lit(\"hello\")"
        );
    }

    /// Quotes/backslashes inside StringLit must be escaped to produce valid Rust code
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

    /// BinOp Gt → col(...).gt(lit(...)) conversion
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

    /// BinOp Eq → .eq(...) conversion
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

    /// BinOp Add → .add(...) conversion (arithmetic operator)
    #[test]
    fn test_expr_to_polars_binop_add() {
        let expr = Expr::BinOp {
            lhs: Box::new(Expr::Ident("a".into())),
            op: BinOpKind::Add,
            rhs: Box::new(Expr::Ident("b".into())),
        };
        assert_eq!(Codegen::expr_to_polars(&expr), "col(\"a\").add(col(\"b\"))");
    }

    /// BinOp Mul → .mul(...) conversion
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

    // ── expr_to_xzz output verification ─────────────────────────────────────────────────

    /// Ident → the identifier string as-is
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

    // ── generate() full pipeline output verification ──────────────────────────────────

    /// TypeDecl → generates a Schema comment block
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

    /// VarDecl (Load source) → generates LazyCsvReader code
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

    /// VarDecl → includes .collect()? terminator
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

    /// VarDecl (VarRef source) → generates .clone().lazy() code
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

    /// mut variable declaration → includes "mut " in the comment
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

    /// Filter op → .filter(...) output
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

    /// Select op → .select([col(...)]) output
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

    /// GroupBy op → .group_by([col(...)]) output
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

    /// Sum op → .agg([col(...).sum()]) output
    #[test]
    fn test_generate_sum_op() {
        let program = make_load_program(vec![PipelineOp::Sum("pop".into())]);
        let output = Codegen::generate(&program);
        assert!(output.contains(".sum()"), ".sum() 없음: {}", output);
    }

    /// Mean op → .agg([col(...).mean()]) output
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

    /// Chart op → [xazz:chart] JSON output comment
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

    /// Count(None) → total row count comment
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

    /// Sample(n) (no seed) → .sample_n_literal(n, false, false, None)
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

    /// Median op → .agg([col(...).median()]) output
    #[test]
    fn test_generate_median_op() {
        let program = make_load_program(vec![PipelineOp::Median("score".into())]);
        let output = Codegen::generate(&program);
        assert!(output.contains(".median()"), ".median() 없음: {}", output);
    }

    /// Variance op → .agg([col(...).var(1)]) output
    #[test]
    fn test_generate_variance_op() {
        let program = make_load_program(vec![PipelineOp::Variance("score".into())]);
        let output = Codegen::generate(&program);
        assert!(output.contains(".var(1)"), ".var(1) 없음: {}", output);
    }

    /// Std op → .agg([col(...).std(1)]) output
    #[test]
    fn test_generate_std_op() {
        let program = make_load_program(vec![PipelineOp::Std("score".into())]);
        let output = Codegen::generate(&program);
        assert!(output.contains(".std(1)"), ".std(1) 없음: {}", output);
    }
}
