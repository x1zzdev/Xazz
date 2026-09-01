// xazz-compiler/src/opt.rs — IR optimization passes (v0.3)
//
// Performs semantics-preserving optimizations on the Typed IR.
//
// passes (all semantics-preserving — Xazz expressions are pure and have no side effects):
//   1. fold_constants   — constant folding of literal binary expressions
//   2. merge_selects    — merge consecutive Selects (projection reduction)
//   3. pushdown_filters — move Filters after Select/WithColumn forward (predicate pushdown)
//
// null-semantics note:
//   - the DSL filter keeps only rows where "the expression is true" (null is treated as false).
//   - Filter relocation is allowed only when the columns referenced by the filter expression
//     still exist identically across the relocation boundary (e.g., when Select keeps those columns).
//   - this is a layer that explicitly defines at the **language level** the pushdown
//     that Polars LazyFrame performs internally. (backend-independent, in preparation for future backends)

use std::collections::HashSet;

use crate::ast::BinOpKind;
use crate::ir::{ColType, DataOp, Step, TypedExpr, TypedExprKind, TypedProgram};

/// Returns a new IR with the whole program optimized (the input is immutable).
pub fn optimize_program(program: &TypedProgram) -> TypedProgram {
    let mut out = program.clone();
    for node in &mut out.pipelines {
        fold_constants(&mut node.steps);
        merge_selects(&mut node.steps);
        pushdown_filters(&mut node.steps);
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. constant folding
// ─────────────────────────────────────────────────────────────────────────────

/// Folds literal binary expressions in every operation (Filter/WithColumn) that holds a TypedExpr.
fn fold_constants(steps: &mut [Step]) {
    for step in steps.iter_mut() {
        if let Step::Data(op) = step {
            match op {
                DataOp::Filter(e) => *e = fold_expr(e.clone()),
                DataOp::WithColumn { expr, .. } => *expr = fold_expr(expr.clone()),
                _ => {}
            }
        }
    }
}

/// Literal node (for folding operands).
#[derive(Debug, Clone, PartialEq)]
enum Lit {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
}

fn lit_of(e: &TypedExpr) -> Option<Lit> {
    match &e.kind {
        TypedExprKind::Int(n) => Some(Lit::Int(*n)),
        TypedExprKind::Float(f) => Some(Lit::Float(*f)),
        TypedExprKind::Str(s) => Some(Lit::Str(s.clone())),
        TypedExprKind::Bool(b) => Some(Lit::Bool(*b)),
        _ => None,
    }
}

fn num(l: &Lit) -> Option<f64> {
    match l {
        Lit::Int(n) => Some(*n as f64),
        Lit::Float(f) => Some(*f),
        _ => None,
    }
}

fn bool_expr(b: bool) -> TypedExpr {
    TypedExpr::new(TypedExprKind::Bool(b), ColType::Bool)
}

fn float_expr(v: f64) -> TypedExpr {
    TypedExpr::new(TypedExprKind::Float(v), ColType::Float)
}

fn int_expr(v: i64) -> TypedExpr {
    TypedExpr::new(TypedExprKind::Int(v), ColType::Int)
}

/// Recursively folds an expression. (Keeps the original when it cannot be folded.)
fn fold_expr(expr: TypedExpr) -> TypedExpr {
    let TypedExpr { kind, ty } = expr;
    match kind {
        TypedExprKind::BinOp { op, lhs, rhs } => {
            let lhs = fold_expr(*lhs);
            let rhs = fold_expr(*rhs);
            let folded = match (lit_of(&lhs), lit_of(&rhs)) {
                (Some(a), Some(b)) => fold_binop(op.clone(), &a, &b),
                _ => None,
            };
            folded.unwrap_or_else(|| {
                TypedExpr::new(
                    TypedExprKind::BinOp {
                        op,
                        lhs: Box::new(lhs.clone()),
                        rhs: Box::new(rhs.clone()),
                    },
                    ty,
                )
            })
        }
        other => TypedExpr::new(other, ty),
    }
}

fn fold_binop(op: BinOpKind, a: &Lit, b: &Lit) -> Option<TypedExpr> {
    use BinOpKind::*;
    // ── arithmetic ─────────────────────────────────────────────
    match op {
        Add | Sub | Mul | Div => {
            // int·int arithmetic (Div folds to float — because Polars integer division produces a float)
            if let (Lit::Int(x), Lit::Int(y)) = (a, b)
                && op != Div
            {
                let v = match op {
                    Add => x.checked_add(*y)?,
                    Sub => x.checked_sub(*y)?,
                    Mul => x.checked_mul(*y)?,
                    _ => unreachable!(),
                };
                return Some(int_expr(v));
            }
            let x = num(a)?;
            let y = num(b)?;
            if op == Div && y == 0.0 {
                return None; // division by zero preserves runtime semantics (no folding)
            }
            let v = match op {
                Add => x + y,
                Sub => x - y,
                Mul => x * y,
                Div => x / y,
                _ => unreachable!(),
            };
            Some(float_expr(v))
        }
        // ── comparison ─────────────────────────────────────────
        Eq | NotEq => match (a, b) {
            (Lit::Int(x), Lit::Int(y)) => Some(bool_expr(if op == Eq { x == y } else { x != y })),
            (Lit::Float(x), Lit::Float(y)) => {
                Some(bool_expr(if op == Eq { x == y } else { x != y }))
            }
            (Lit::Str(x), Lit::Str(y)) => Some(bool_expr(if op == Eq { x == y } else { x != y })),
            (Lit::Bool(x), Lit::Bool(y)) => Some(bool_expr(if op == Eq { x == y } else { x != y })),
            _ => {
                let x = num(a)?;
                let y = num(b)?;
                Some(bool_expr(if op == Eq { x == y } else { x != y }))
            }
        },
        Lt | Gt | LtEq | GtEq => {
            let x = num(a)?;
            let y = num(b)?;
            let b = match op {
                Lt => x < y,
                Gt => x > y,
                LtEq => x <= y,
                GtEq => x >= y,
                _ => unreachable!(),
            };
            Some(bool_expr(b))
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. consecutive Select merging (projection reduction)
// ─────────────────────────────────────────────────────────────────────────────

fn merge_selects(steps: &mut Vec<Step>) {
    let mut out: Vec<Step> = Vec::with_capacity(steps.len());
    let mut pending: Option<Vec<String>> = None;

    for step in steps.drain(..) {
        if let Step::Data(DataOp::Select(cols)) = step {
            pending = Some(cols);
        } else {
            if let Some(sel) = pending.take() {
                out.push(Step::Data(DataOp::Select(sel)));
            }
            out.push(step);
        }
    }
    if let Some(sel) = pending {
        out.push(Step::Data(DataOp::Select(sel)));
    }
    *steps = out;
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. predicate pushdown (move Filter before Select/WithColumn)
// ─────────────────────────────────────────────────────────────────────────────

fn expr_columns(e: &TypedExpr, out: &mut HashSet<String>) {
    match &e.kind {
        TypedExprKind::Column(c) => {
            out.insert(c.clone());
        }
        TypedExprKind::BinOp { lhs, rhs, .. } => {
            expr_columns(lhs, out);
            expr_columns(rhs, out);
        }
        _ => {}
    }
}

fn pushdown_filters(steps: &mut Vec<Step>) {
    let mut i = 1;
    while i < steps.len() {
        let can_move = match &steps[i - 1] {
            // Select only keeps/reduces existing columns → can move forward if all the needed columns are kept.
            Step::Data(DataOp::Select(cols)) => {
                if let Step::Data(DataOp::Filter(expr)) = &steps[i] {
                    let mut needed = HashSet::new();
                    expr_columns(expr, &mut needed);
                    let keep: HashSet<String> = cols.iter().cloned().collect();
                    needed.is_subset(&keep)
                } else {
                    false
                }
            }
            // WithColumn adds one new column → can only move when it does not reference that column.
            Step::Data(DataOp::WithColumn { name, .. }) => {
                if let Step::Data(DataOp::Filter(expr)) = &steps[i] {
                    let mut needed = HashSet::new();
                    expr_columns(expr, &mut needed);
                    !needed.contains(name)
                } else {
                    false
                }
            }
            _ => false,
        };
        if can_move {
            steps.swap(i - 1, i);
        }
        i += 1;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{DataOp, Step, TypedProgram};
    use crate::{Lexer, Parser};

    fn analyze(src: &str) -> TypedProgram {
        let tokens = Lexer::new(src).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        crate::checker::analyze_program(&program).1
    }

    fn data_steps(ir: &TypedProgram, idx: usize) -> &[Step] {
        &ir.pipelines[idx].steps
    }

    #[test]
    fn folds_constant_arithmetic() {
        let src = "type X = { a: float };
             v p = load(\"x.csv\") :: X |> filter(1 + 2 > 2);";
        let ir = analyze(src);
        let opt = optimize_program(&ir);
        // filter(1 + 2 > 2) → filter(true)
        match &data_steps(&opt, 0)[0] {
            Step::Data(DataOp::Filter(e)) => {
                assert_eq!(e.kind, TypedExprKind::Bool(true));
            }
            other => panic!("필터가 아님: {:?}", other),
        }
    }

    #[test]
    fn folds_division_when_safe_keeps_zero_div() {
        // 8 / 2 folds to float 4.0
        let src = "type X = { a: float };
             v p = load(\"x.csv\") :: X |> withColumn(\"c\", 8 / 2);";
        let ir = analyze(src);
        let opt = optimize_program(&ir);
        match &data_steps(&opt, 0)[0] {
            Step::Data(DataOp::WithColumn { expr, .. }) => {
                assert_eq!(expr.kind, TypedExprKind::Float(4.0));
            }
            other => panic!("withColumn 아님: {:?}", other),
        }

        // 8 / 0 is not folded to preserve runtime semantics
        let src2 = "type X = { a: float };
             v p = load(\"x.csv\") :: X |> withColumn(\"c\", 8 / 0);";
        let ir2 = analyze(src2);
        let opt2 = optimize_program(&ir2);
        match &data_steps(&opt2, 0)[0] {
            Step::Data(DataOp::WithColumn { expr, .. }) => {
                assert!(matches!(
                    expr.kind,
                    TypedExprKind::BinOp {
                        op: BinOpKind::Div,
                        ..
                    }
                ));
            }
            other => panic!("withColumn 아님: {:?}", other),
        }
    }

    #[test]
    fn merges_consecutive_selects() {
        let src = "type X = { a: float, b: float, c: float };
             v p = load(\"x.csv\") :: X |> select([a, b]) |> select([a]);";
        let ir = analyze(src);
        let opt = optimize_program(&ir);
        assert_eq!(
            data_steps(&opt, 0),
            &[Step::Data(DataOp::Select(vec!["a".into()]))]
        );
    }

    #[test]
    fn pushes_filter_before_select() {
        // feedback example: filter |> select |> filter → filter |> filter |> select
        let src = "type X = { a: float, b: float };
             v p = load(\"x.csv\") :: X |> filter(a > 0) |> select([a, b]) |> filter(b > 0);";
        let ir = analyze(src);
        let opt = optimize_program(&ir);
        let steps = data_steps(&opt, 0);
        assert!(matches!(steps[0], Step::Data(DataOp::Filter(_))));
        assert!(matches!(steps[1], Step::Data(DataOp::Filter(_))));
        assert!(
            matches!(steps[2], Step::Data(DataOp::Select(_))),
            "Select 가 맨 뒤로 밀려야 함: {:?}",
            steps
        );
    }

    #[test]
    fn does_not_push_filter_using_new_column() {
        // withColumn(new) |> filter(new > 0) — references new, so relocation is forbidden
        let src = "type X = { a: float };
             v p = load(\"x.csv\") :: X |> withColumn(\"b\", a * 2) |> filter(b > 0);";
        let ir = analyze(src);
        let opt = optimize_program(&ir);
        let steps = data_steps(&opt, 0);
        assert!(matches!(steps[0], Step::Data(DataOp::WithColumn { .. })));
        assert!(matches!(steps[1], Step::Data(DataOp::Filter(_))));
    }
}
