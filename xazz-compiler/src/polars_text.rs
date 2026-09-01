/// xazz-compiler/src/polars_text.rs — Polars 소스 코드 문자열 생성 (단일 위치)
///
/// `emit rust` / `codegen` 의 텍스트 생성 계층이 공유하는 **유일한**
/// AST 표현식 → Polars Rust 소스 문자열 매핑이다.
///
/// 아키텍처 (중복 제거의 단일 위치):
///   - **런타임 백엔드**: xazz-exec/src/lower.rs 가 Typed IR(DataOp/TypedExpr)을
///     실제 Polars LazyFrame 으로 lowering 한다. 실행 경로의 유일한 op→Polars 매핑.
///   - **텍스트 백엔드**: 이 모듈(polars_text)이 AST 표현식 → Polars Rust 소스
///     문자열 매핑의 유일한 위치이다. codegen.rs 와 emitter.rs 는 모두 여기에 위임한다.
///
/// 즉, "op/표현식 → Polars" 매핑은 실행(lower.rs)과 텍스트(polars_text) 각각
/// 한 곳에만 존재한다.
use std::collections::HashMap;

use crate::ast::{BinOpKind, Expr};
use crate::policy::printer::escape;

/// AST 표현식 → Polars Rust 소스 문자열.
///
/// `col_types` 는 열 이름 → DSL 타입 문자열 맵이다. 왼쪽 피연산자가 float
/// 컬럼이면 오른쪽 정수 리터럴을 f64 로 승격해 타입 안정성을 높인다.
/// (타입 정보가 필요 없으면 `None` 전달 — codegen 의 플레인 경로.)
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
                // ── 산술 연산자 (v0.16+) ──────────────────
                BinOpKind::Add => "add",
                BinOpKind::Sub => "sub",
                BinOpKind::Mul => "mul",
                BinOpKind::Div => "div",
            };
            format!("{}.{}({})", l, op_method, r)
        }
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
}
