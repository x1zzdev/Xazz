/// xazz-compiler/src/polars_text.rs — Polars source string generation (single location)
///
/// The **only** shared AST-expression → Polars Rust source string mapping used by the
/// text-generation layers of `emit rust` / `codegen`.
///
/// Architecture (single location for de-duplication):
///   - **Runtime backend**: xazz-exec/src/lower.rs lowers Typed IR (DataOp/TypedExpr) into
///     an actual Polars LazyFrame. The only op→Polars mapping on the execution path.
///   - **Text backend**: this module (polars_text) is the only location of the AST-expression →
///     Polars Rust source string mapping. Both codegen.rs and emitter.rs delegate to it.
///
/// In other words, the "op/expression → Polars" mapping exists in exactly one place for
/// each of execution (lower.rs) and text (polars_text).
use std::collections::HashMap;

use crate::ast::{BinOpKind, Expr, FillNullValue};
use crate::policy::printer::escape;

/// AST expression → Polars Rust source string.
///
/// `col_types` is a map of column name → DSL type string. If the left operand is a float
/// column, the right integer literal is promoted to f64 to improve type safety.
/// (Pass `None` when type info is not needed — codegen's plain path.)
pub fn expr_to_polars(expr: &Expr, col_types: Option<&HashMap<String, String>>) -> String {
    match expr {
        Expr::Ident(s) => format!("col(\"{}\")", escape(s)),
        Expr::StringLit(s) => format!("lit(\"{}\")", escape(s)),
        Expr::IntLit(n) => format!("lit({}i64)", n),
        Expr::FloatLit(f) => format!("lit({}f64)", f),
        Expr::BoolLit(b) => format!("lit({})", b),
        Expr::BinOp { lhs, op, rhs } => {
            let lhs_is_float = if let Expr::Ident(col_name) = lhs.as_ref() {
                col_types
                    .and_then(|m| m.get(col_name.as_str()))
                    .map(|t| t.contains("float"))
                    .unwrap_or(false)
            } else {
                false
            };

            let l = expr_to_polars(lhs, col_types);

            let r = if lhs_is_float {
                match rhs.as_ref() {
                    Expr::IntLit(n) => format!("lit({:.1}f64)", *n as f64),
                    Expr::FloatLit(f) => format!("lit({}f64)", f),
                    other => expr_to_polars(other, col_types),
                }
            } else {
                expr_to_polars(rhs, col_types)
            };

            let op_method = match op {
                BinOpKind::Eq => "eq",
                BinOpKind::NotEq => "neq",
                BinOpKind::Lt => "lt",
                BinOpKind::Gt => "gt",
                BinOpKind::LtEq => "lt_eq",
                BinOpKind::GtEq => "gt_eq",
                // ── Arithmetic operators (v0.16+) ──────────────────
                BinOpKind::Add => "add",
                BinOpKind::Sub => "sub",
                BinOpKind::Mul => "mul",
                BinOpKind::Div => "div",
            };
            format!("{}.{}({})", l, op_method, r)
        }
    }
}

/// fillNull fill value → Polars expression source string (shared by codegen/emitter).
///
/// `col` is the target column to fill — the mean/median strategies reference that column's aggregate.
pub fn fill_value_to_polars(value: &FillNullValue, col: &str) -> String {
    match value {
        FillNullValue::Mean => format!("col(\"{}\").mean()", escape(col)),
        FillNullValue::Median => format!("col(\"{}\").median()", escape(col)),
        FillNullValue::Zero => "lit(0)".to_string(),
        FillNullValue::Int(n) => format!("lit({}i64)", n),
        FillNullValue::Float(f) => format!("lit({}f64)", f),
        FillNullValue::Str(s) => format!("lit(\"{}\")", escape(s)),
    }
}

/// Returns the Polars method call string for an aggregate operator (shared by codegen/emitter).
///
/// e.g. `AggKind::Sum` → `"col(\"pm10\").sum()"`.
pub fn agg_expr_to_polars(kind: crate::ir::AggKind, col: &str) -> String {
    let method = match kind {
        crate::ir::AggKind::Count => "count()",
        crate::ir::AggKind::Len => "count()",
        crate::ir::AggKind::Sum => "sum()",
        crate::ir::AggKind::Mean => "mean()",
        crate::ir::AggKind::Min => "min()",
        crate::ir::AggKind::Max => "max()",
        crate::ir::AggKind::Median => "median()",
        // var(1)/std(1) take an argument, so the parentheses are already part of the method name.
        crate::ir::AggKind::Variance => "var(1)",
        crate::ir::AggKind::Std => "std(1)",
    };
    format!("col(\"{}\").{}", escape(col), method)
}

/// cast() target DSL type → Polars DataType source string (shared by codegen/emitter).
///
/// Unknown types are returned as the original string (the checker has already handled errors).
pub fn cast_dtype_to_polars(to_type: &str) -> String {
    match to_type {
        "float" => "DataType::Float64".to_string(),
        "int" => "DataType::Int64".to_string(),
        "str" => "DataType::String".to_string(),
        "bool" => "DataType::Boolean".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ident(s: &str) -> Expr {
        Expr::Ident(s.to_string())
    }

    fn int(n: i64) -> Expr {
        Expr::IntLit(n)
    }

    fn float_col_map() -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("pm10".to_string(), "float".to_string());
        m
    }

    #[test]
    fn maps_column_to_col() {
        assert_eq!(expr_to_polars(&ident("pm10"), None), "col(\"pm10\")");
    }

    #[test]
    fn escapes_quotes_in_columns() {
        assert_eq!(expr_to_polars(&ident("a\"b"), None), "col(\"a\\\"b\")");
    }

    #[test]
    fn comparison_uses_polars_methods() {
        let expr = Expr::BinOp {
            lhs: Box::new(ident("pm10")),
            op: BinOpKind::Gt,
            rhs: Box::new(int(50)),
        };
        assert_eq!(expr_to_polars(&expr, None), "col(\"pm10\").gt(lit(50i64))");
    }

    #[test]
    fn arithmetic_uses_add_method() {
        let expr = Expr::BinOp {
            lhs: Box::new(ident("a")),
            op: BinOpKind::Add,
            rhs: Box::new(ident("b")),
        };
        assert_eq!(expr_to_polars(&expr, None), "col(\"a\").add(col(\"b\"))");
    }

    #[test]
    fn promotes_int_rhs_to_f64_when_lhs_is_float_column() {
        let expr = Expr::BinOp {
            lhs: Box::new(ident("pm10")),
            op: BinOpKind::Add,
            rhs: Box::new(int(3)),
        };
        let types = float_col_map();
        assert_eq!(
            expr_to_polars(&expr, Some(&types)),
            "col(\"pm10\").add(lit(3.0f64))"
        );
    }

    #[test]
    fn keeps_int_rhs_without_type_info() {
        let expr = Expr::BinOp {
            lhs: Box::new(ident("pm10")),
            op: BinOpKind::Add,
            rhs: Box::new(int(3)),
        };
        assert_eq!(expr_to_polars(&expr, None), "col(\"pm10\").add(lit(3i64))");
    }

    #[test]
    fn fill_value_maps_each_strategy() {
        assert_eq!(
            fill_value_to_polars(&FillNullValue::Mean, "a"),
            "col(\"a\").mean()"
        );
        assert_eq!(
            fill_value_to_polars(&FillNullValue::Median, "a"),
            "col(\"a\").median()"
        );
        assert_eq!(fill_value_to_polars(&FillNullValue::Zero, "a"), "lit(0)");
        assert_eq!(
            fill_value_to_polars(&FillNullValue::Int(3), "a"),
            "lit(3i64)"
        );
        assert_eq!(
            fill_value_to_polars(&FillNullValue::Float(2.5), "a"),
            "lit(2.5f64)"
        );
        assert_eq!(
            fill_value_to_polars(&FillNullValue::Str("x".into()), "a"),
            "lit(\"x\")"
        );
        assert_eq!(
            fill_value_to_polars(&FillNullValue::Str("a\"b".into()), "a"),
            "lit(\"a\\\"b\")"
        );
    }

    #[test]
    fn agg_expr_maps_each_kind() {
        use crate::ir::AggKind;
        assert_eq!(
            agg_expr_to_polars(AggKind::Sum, "pm10"),
            "col(\"pm10\").sum()"
        );
        assert_eq!(
            agg_expr_to_polars(AggKind::Mean, "pm10"),
            "col(\"pm10\").mean()"
        );
        assert_eq!(
            agg_expr_to_polars(AggKind::Count, "id"),
            "col(\"id\").count()"
        );
        assert_eq!(
            agg_expr_to_polars(AggKind::Variance, "v"),
            "col(\"v\").var(1)"
        );
        assert_eq!(agg_expr_to_polars(AggKind::Std, "v"), "col(\"v\").std(1)");
        assert_eq!(
            agg_expr_to_polars(AggKind::Median, "v"),
            "col(\"v\").median()"
        );
    }

    #[test]
    fn cast_dtype_maps_known_types() {
        assert_eq!(cast_dtype_to_polars("float"), "DataType::Float64");
        assert_eq!(cast_dtype_to_polars("int"), "DataType::Int64");
        assert_eq!(cast_dtype_to_polars("str"), "DataType::String");
        assert_eq!(cast_dtype_to_polars("bool"), "DataType::Boolean");
        // unknown passes through the original (never reached, since the checker rejects it beforehand)
        assert_eq!(cast_dtype_to_polars("custom"), "custom");
    }
}
