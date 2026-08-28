//! xazz-exec/src/lower.rs — Data IR → Polars 백엔드 lowering (v0.19)
//!
//! Typed IR 의 `DataOp` 와 `TypedExpr` 를 Polars `LazyFrame` / `Expr` 로 변환한다.
//! 이 모듈은 **데이터 계층만** 알고 있다 (ML 은 dl.rs, DP 는 dp.rs, 차트는 runtime.rs).
//!
//! 런타임은 raw AST 를 해석하지 않고, 타입체커가 만든 Typed IR 을 여기로 흘려보낸다.
//! → 이중 해석 제거 + backend 지식의 단일 위치화 (DataOp 하나 = Polars 변환 한 곳).

use std::collections::HashMap;

use polars::frame::DataFrame;
use polars::prelude::{IntoLazy, JoinArgs, JoinType, LazyFrame, SortMultipleOptions, col, lit};

use xazz_compiler::ast::BinOpKind;
use xazz_compiler::ir::{AggKind, DataOp, FillValue, TypedExpr, TypedExprKind};

// ─────────────────────────────────────────────────────────────────────────────
// 표현식 lowering
// ─────────────────────────────────────────────────────────────────────────────

/// 타입이 붙은 IR 표현식을 Polars `Expr` 로 변환한다. (타입 정보는 검사 단계에서 소비됨)
pub fn typed_expr_to_polars(expr: &TypedExpr) -> polars::prelude::Expr {
    match &expr.kind {
        TypedExprKind::Column(s) => col(s.as_str()),
        TypedExprKind::Int(n) => lit(*n),
        TypedExprKind::Float(f) => lit(*f),
        TypedExprKind::Str(s) => lit(s.clone()),
        TypedExprKind::Bool(b) => lit(*b),
        TypedExprKind::BinOp { op, lhs, rhs } => {
            let l = typed_expr_to_polars(lhs);
            let r = typed_expr_to_polars(rhs);
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

// ─────────────────────────────────────────────────────────────────────────────
// 연산 lowering
// ─────────────────────────────────────────────────────────────────────────────

/// JoinHow(AST) → Polars JoinType 변환.
fn to_polars_join_type(how: &xazz_compiler::ast::JoinHow) -> JoinType {
    use xazz_compiler::ast::JoinHow;
    match how {
        JoinHow::Inner => JoinType::Inner,
        JoinHow::Left => JoinType::Left,
        JoinHow::Outer => JoinType::Full,
        JoinHow::Cross => JoinType::Cross,
    }
}

/// 단일 `DataOp` 를 현재 `LazyFrame` 에 적용한다.
///
/// - `pending_group`: 선행 `GroupBy` 가 있을 때 집계를 그룹 집계로 바꾸는 상태.
/// - `symbol_table`: `Join` 이 참조하는 상대 DataFrame.
pub fn lower_data(
    op: &DataOp,
    lf: &mut LazyFrame,
    symbol_table: &HashMap<String, DataFrame>,
    pending_group: &mut Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    match op {
        DataOp::Filter(expr) => {
            *lf = lf.clone().filter(typed_expr_to_polars(expr));
        }
        DataOp::Select(cols) => {
            let exprs: Vec<polars::prelude::Expr> =
                cols.iter().map(|c| col(c.as_str())).collect();
            *lf = lf.clone().select(exprs);
        }
        DataOp::GroupBy(group_col) => {
            *pending_group = Some(group_col.clone());
        }
        DataOp::Aggregate { kind, col: agg_col } => {
            let agg_expr: polars::prelude::Expr = match kind {
                AggKind::Count => col(agg_col.as_str()).count(),
                AggKind::Sum => col(agg_col.as_str()).sum(),
                AggKind::Mean => col(agg_col.as_str()).mean(),
                AggKind::Min => col(agg_col.as_str()).min(),
                AggKind::Max => col(agg_col.as_str()).max(),
                AggKind::Median => col(agg_col.as_str()).median(),
                AggKind::Variance => col(agg_col.as_str()).var(1),
                AggKind::Std => col(agg_col.as_str()).std(1),
            };
            if let Some(group_col) = pending_group.take() {
                *lf = lf
                    .clone()
                    .group_by([col(group_col.as_str())])
                    .agg([agg_expr]);
            } else {
                *lf = lf.clone().select([agg_expr]);
            }
        }
        DataOp::Sort { col: sort_col, desc } => {
            let opts = SortMultipleOptions::default().with_order_descending(*desc);
            *lf = lf.clone().sort([sort_col.as_str()], opts);
        }
        DataOp::Limit(n) => {
            if *n <= 0 {
                return Err("take() 의 n 은 0보다 커야 합니다.".into());
            }
            *lf = lf.clone().limit(*n as u32);
        }
        DataOp::Sample { n, seed } => {
            let snapshot = lf.clone().collect()?;
            let seed_u64 = seed.map(|s| s as u64);
            let sampled = snapshot.sample_n_literal(*n as usize, false, false, seed_u64)?;
            *lf = sampled.lazy();
        }
        DataOp::DropNull(drop_col) => {
            *lf = lf.clone().filter(col(drop_col.as_str()).is_not_null());
        }
        DataOp::FillNull { col: c, value } => {
            let fill_expr: polars::prelude::Expr = match value {
                FillValue::Mean => col(c.as_str()).mean(),
                FillValue::Median => col(c.as_str()).median(),
                FillValue::Zero => lit(0),
                FillValue::Int(n) => lit(*n),
                FillValue::Float(f) => lit(*f),
                FillValue::Str(s) => lit(s.clone()),
            };
            *lf = lf
                .clone()
                .with_columns([col(c.as_str()).fill_null(fill_expr)]);
        }
        DataOp::Join {
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
                *lf = lf.clone().join(
                    other_lf,
                    left_keys,
                    right_keys,
                    JoinArgs::new(to_polars_join_type(how)),
                );
            }
            None => {
                return Err(format!(
                    "런타임 에러: join() 대상 변수 '{}' 가 심볼 테이블에 없습니다.",
                    other
                )
                .into());
            }
        },
        DataOp::WithColumn { name, expr } => {
            let polars_expr = typed_expr_to_polars(expr).alias(name.as_str());
            *lf = lf.clone().with_columns([polars_expr]);
        }
        DataOp::Cast { col: c, to } => {
            use polars::prelude::DataType;
            let dtype = match to.as_str() {
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
            *lf = lf.clone().with_columns([col(c.as_str()).cast(dtype)]);
        }
        DataOp::Rename { old, new } => {
            let old: Vec<&str> = vec![old.as_str()];
            let new: Vec<&str> = vec![new.as_str()];
            *lf = lf.clone().rename(old, new, false);
        }
        DataOp::Replace { col: c, from, to } => {
            *lf = lf.clone().with_columns(
                [col(c.as_str())
                    .str()
                    .replace_all(lit(from.as_str()), lit(to.as_str()), true)
                    .alias(c.as_str())],
            );
        }
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// 테스트
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use polars::prelude::df;
    use xazz_compiler::ir::{DataOp, TypedExpr, TypedExprKind};

    fn collect_pipeline(
        initial: DataFrame,
        ops: &[DataOp],
    ) -> Result<DataFrame, Box<dyn std::error::Error>> {
        let mut lf = initial.lazy();
        let mut pending: Option<String> = None;
        for op in ops {
            lower_data(op, &mut lf, &HashMap::new(), &mut pending)?;
        }
        Ok(lf.collect()?)
    }

    fn col_expr(name: &str) -> TypedExpr {
        TypedExpr::new(
            TypedExprKind::Column(name.to_string()),
            xazz_compiler::ir::ColType::Unknown,
        )
    }

    #[test]
    fn filter_then_select() {
        let frame = df!("a" => [1i64, 2, 3, 4], "b" => [10i64, 20, 30, 40]).unwrap();
        let ops = vec![
            DataOp::Filter(TypedExpr::new(
                TypedExprKind::BinOp {
                    op: BinOpKind::Gt,
                    lhs: Box::new(col_expr("a")),
                    rhs: Box::new(TypedExpr::new(
                        TypedExprKind::Int(2),
                        xazz_compiler::ir::ColType::Int,
                    )),
                },
                xazz_compiler::ir::ColType::Bool,
            )),
            DataOp::Select(vec!["a".into()]),
        ];
        let out = collect_pipeline(frame, &ops).unwrap();
        assert_eq!(out.height(), 2);
        assert_eq!(out.get_column_names(), vec!["a"]);
    }

    #[test]
    fn group_by_aggregate_pairs_groupby_with_agg() {
        let frame = df!("g" => ["x", "x", "y"], "v" => [1i64, 3, 10]).unwrap();
        let ops = vec![
            DataOp::GroupBy("g".into()),
            DataOp::Aggregate {
                kind: AggKind::Sum,
                col: "v".into(),
            },
        ];
        let out = collect_pipeline(frame, &ops).unwrap();
        assert_eq!(out.height(), 2);
        assert!(out.column("g").is_ok());
        assert!(out.column("v").is_ok());
    }

    #[test]
    fn limit_rejects_non_positive() {
        let frame = df!("a" => [1i64]).unwrap();
        let err = collect_pipeline(frame, &[DataOp::Limit(0)]).unwrap_err();
        assert!(err.to_string().contains("0보다 커야"));
    }

    #[test]
    fn fill_null_mean_substitutes_nan() {
        let frame = df!("a" => [Some(1.0f64), None, Some(3.0)]).unwrap();
        let ops = vec![DataOp::FillNull {
            col: "a".into(),
            value: FillValue::Mean,
        }];
        let out = collect_pipeline(frame, &ops).unwrap();
        let col_a = out.column("a").unwrap();
        assert_eq!(col_a.null_count(), 0);
    }
}