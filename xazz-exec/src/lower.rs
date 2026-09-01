//! xazz-exec/src/lower.rs — Data IR → Polars backend lowering (v0.19)
//!
//! Converts Typed IR `DataOp` and `TypedExpr` into Polars `LazyFrame` / `Expr`.
//! This module knows **only the data layer** (ML is in dl.rs, DP in dp.rs, charts in runtime.rs).
//!
//! The runtime does not interpret the raw AST; it feeds the type-checker's Typed IR here.
//! → eliminates double interpretation + centralizes backend knowledge in one place (one DataOp = one Polars translation).

use std::collections::HashMap;

use polars::frame::DataFrame;
use polars::prelude::{IntoLazy, JoinArgs, JoinType, LazyFrame, SortMultipleOptions, col, lit};

use xazz_compiler::ast::BinOpKind;
use xazz_compiler::ir::{AggKind, DataOp, FillValue, TypedExpr, TypedExprKind};
use xazz_core::i18n::tr;

// ─────────────────────────────────────────────────────────────────────────────
// Expression lowering
// ─────────────────────────────────────────────────────────────────────────────

/// Converts a typed IR expression to a Polars `Expr`. (Type info is consumed at the check stage)
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
// Operation lowering
// ─────────────────────────────────────────────────────────────────────────────

/// Converts JoinHow (AST) → Polars JoinType.
fn to_polars_join_type(how: &xazz_compiler::ast::JoinHow) -> JoinType {
    use xazz_compiler::ast::JoinHow;
    match how {
        JoinHow::Inner => JoinType::Inner,
        JoinHow::Left => JoinType::Left,
        JoinHow::Outer => JoinType::Full,
        JoinHow::Cross => JoinType::Cross,
    }
}

/// Applies a single `DataOp` to the current `LazyFrame`.
///
/// - `pending_group`: state that turns an aggregate into a group aggregate when a preceding `GroupBy` exists.
/// - `symbol_table`: the counterpart DataFrame referenced by `Join`.
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
            let exprs: Vec<polars::prelude::Expr> = cols.iter().map(|c| col(c.as_str())).collect();
            *lf = lf.clone().select(exprs);
        }
        DataOp::GroupBy(group_col) => {
            *pending_group = Some(group_col.clone());
        }
        DataOp::Aggregate { kind, col: agg_col } => {
            let agg_expr: polars::prelude::Expr = match kind {
                AggKind::Count => col(agg_col.as_str()).count(),
                AggKind::Len => polars::prelude::len(),
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
        DataOp::Sort {
            col: sort_col,
            desc,
        } => {
            let opts = SortMultipleOptions::default().with_order_descending(*desc);
            *lf = lf.clone().sort([sort_col.as_str()], opts);
        }
        DataOp::Limit(n) => {
            if *n <= 0 {
                return Err(tr(
                    "take() n must be greater than 0.",
                    "take() 의 n 은 0보다 커야 합니다.",
                )
                .into());
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
                    "{}: {} '{}' {}",
                    tr("runtime error", "런타임 에러"),
                    tr("join() target variable", "join() 대상 변수"),
                    other,
                    tr("is not in the symbol table", "가 심볼 테이블에 없습니다")
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
                        "{}: cast() {} '{}'. {}: \"float\", \"int\", \"str\", \"bool\"",
                        tr("runtime error", "런타임 에러"),
                        tr("unknown type", "에 알 수 없는 타입"),
                        other,
                        tr("supported types", "지원 타입")
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
            *lf = lf.clone().with_columns([col(c.as_str())
                .str()
                .replace_all(lit(from.as_str()), lit(to.as_str()), true)
                .alias(c.as_str())]);
        }
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use polars::prelude::df;
    use xazz_compiler::ir::{DataOp, TypedExpr, TypedExprKind};
    use xazz_core::i18n::{Lang, reset_lang, set_lang};

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
        let err = collect_pipeline(frame.clone(), &[DataOp::Limit(0)]).unwrap_err();
        assert!(
            err.to_string().contains("greater than 0"),
            "영어 기본이어야 함: {err}"
        );
        set_lang(Lang::Ko);
        let err2 = collect_pipeline(frame, &[DataOp::Limit(0)]).unwrap_err();
        reset_lang();
        assert!(
            err2.to_string().contains("0보다 커야"),
            "한국어 메시지: {err2}"
        );
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

    // ── Equivalence check of pre/post-optimization results (proves the IR optimization layer's correctness) ──

    fn gt_col(name: &str, n: i64) -> DataOp {
        DataOp::Filter(TypedExpr::new(
            TypedExprKind::BinOp {
                op: BinOpKind::Gt,
                lhs: Box::new(TypedExpr::new(
                    TypedExprKind::Column(name.to_string()),
                    xazz_compiler::ir::ColType::Int,
                )),
                rhs: Box::new(TypedExpr::new(
                    TypedExprKind::Int(n),
                    xazz_compiler::ir::ColType::Int,
                )),
            },
            xazz_compiler::ir::ColType::Bool,
        ))
    }

    fn only_data_ops(steps: &[xazz_compiler::ir::Step]) -> Vec<DataOp> {
        steps
            .iter()
            .map(|s| match s {
                xazz_compiler::ir::Step::Data(op) => op.clone(),
                other => panic!("데이터 연산만 기대: {:?}", other),
            })
            .collect()
    }

    #[test]
    fn optimizer_preserves_filter_select_reorder_semantics() {
        use xazz_compiler::ir::{PipelineNode, Source, Step as IrStep, TypedProgram};

        let frame = df!("a" => [1i64, 2, 3, 4], "b" => [10i64, 20, 30, 40]).unwrap();

        let steps = vec![
            IrStep::Data(gt_col("a", 1)),
            IrStep::Data(DataOp::Select(vec!["a".into(), "b".into()])),
            IrStep::Data(gt_col("b", 20)),
        ];

        let program = TypedProgram {
            types: vec![],
            models: vec![],
            pipelines: vec![PipelineNode {
                id: 0,
                name: None,
                source: Source::Ref { var: String::new() },
                input_schema: None,
                output_schema: xazz_compiler::ir::Schema::default(),
                steps: steps.clone(),
                yields_model: false,
            }],
        };
        let opt = xazz_compiler::opt::optimize_program(&program);
        let opt_steps = opt.pipelines[0].steps.clone();

        let orig = collect_pipeline(frame.clone(), &only_data_ops(&steps)).unwrap();
        let optimized = collect_pipeline(frame, &only_data_ops(&opt_steps)).unwrap();

        // Rewriting filter |> select |> filter into filter |> filter |> select still yields identical results
        assert_eq!(orig, optimized);
    }
}
