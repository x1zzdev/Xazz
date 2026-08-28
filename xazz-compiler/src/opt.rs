// xazz-compiler/src/opt.rs — IR 최적화 패스 (v0.3)
//
// Typed IR 위에서 의미를 보존하는 최적화를 수행한다.
//
// 패스 (전부 의미 보존 — Xazz 표현식은 순수하며 부작용이 없다):
//   1. fold_constants   — 리터럴 이항식 상수 폴딩
//   2. merge_selects    — 연속 Select 병합 (projection 축소)
//   3. pushdown_filters — Select/WithColumn 뒤의 Filter 를 앞으로 (조건 푸시다운)
//
// null 의미론 주의:
//   - DSL 필터는 "식이 true 인 행"만 유지한다 (null 은 false 처리).
//   - Filter 재배치는 필터 식이 참조하는 컬럼이 이동 대상 경계를 통과해도
//     동일하게 존재할 때만 허용한다 (ex: Select 가 해당 컬럼을 유지할 때).
//   - 이는 Polars LazyFrame 이 내부에서 수행하는 푸시다운을 **언어 수준에서**
//     명시적으로 정의하는 계층이다. (backend 미의존, 향후 후행 백엔드 대비)

use std::collections::HashSet;

use crate::ast::BinOpKind;
use crate::ir::{ColType, DataOp, Step, TypedExpr, TypedExprKind, TypedProgram};

/// 프로그램 전체를 최적화한 새 IR 을 반환한다 (입력은 불변).
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
// 1. 상수 폴딩
// ─────────────────────────────────────────────────────────────────────────────

/// TypedExpr 를 담는 모든 연산(Filter/WithColumn)의 리터럴 이항식을 접는다.
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

/// 리터럴 노드 (폴딩 피연산자용).
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

/// 표현식을 재귀적으로 폴딩한다. (폴딩 불가 시 원형 유지)
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
    // ── 산술 ─────────────────────────────────────────────
    match op {
        Add | Sub | Mul | Div => {
            // int·int 산술 (Div 는 float 로 폴딩 — Polars 정수 나눗셈 결과가 실수이기 때문)
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
                return None; // 0 나눗셈은 런타임 의미를 보존 (폴딩 금지)
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
        // ── 비교 ─────────────────────────────────────────
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
// 2. 연속 Select 병합 (projection 축소)
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
// 3. 조건 푸시다운 (Filter 를 Select/WithColumn 앞으로)
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
            // Select 는 기존 컬럼만 유지/축소 → 필요한 컬럼이 모두 유지되면 앞으로 이동 가능.
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
            // WithColumn 은 새 컬럼 1개를 추가 → 그 컬럼을 참조하지 않을 때만 이동 가능.
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
// 테스트
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
        // 8 / 2 는 float 4.0 으로 폴딩
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

        // 8 / 0 은 런타임 의미 보존을 위해 폴딩 금지
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
        // 피드백 예시: filter |> select |> filter → filter |> filter |> select
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
        // withColumn(new) |> filter(new > 0) — new 를 참조하므로 재배치 금지
        let src = "type X = { a: float };
             v p = load(\"x.csv\") :: X |> withColumn(\"b\", a * 2) |> filter(b > 0);";
        let ir = analyze(src);
        let opt = optimize_program(&ir);
        let steps = data_steps(&opt, 0);
        assert!(matches!(steps[0], Step::Data(DataOp::WithColumn { .. })));
        assert!(matches!(steps[1], Step::Data(DataOp::Filter(_))));
    }
}
