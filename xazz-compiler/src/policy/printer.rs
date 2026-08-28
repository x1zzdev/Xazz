// xazz-compiler/src/policy/printer.rs — AST → `.xzz` 소스 프린터 (Policy-as-Code #2)
//
// 자동 보정(remediation)은 AST 를 고쳐 쓴 뒤 다시 `.xzz` 소스로 되돌려야
// 사용자에게 "안전한 대체 코드"를 보여줄 수 있다. 문자열 치환으로 코드를
// 고치면 구문이 깨질 위험이 크므로, AST 를 정본으로 삼고 여기서 다시 찍어낸다.
//
// 보장:
//   parse(print(parse(src))) == parse(src)
//
// 이 왕복(round-trip) 성질은 아래 테스트에서 실제 예제들로 검증한다.

use crate::ast::{
    BinOpKind, ChartConfig, DpArgs, Expr, FillNullValue, JoinHow, LayerKind, PipelineOp,
    PipelineSource, Program, Stmt, StructField, TrainConfig,
};

/// Program 전체를 `.xzz` 소스 문자열로 되돌린다.
pub fn print_program(program: &Program) -> String {
    let mut out = String::new();
    for (i, stmt) in program.stmts.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&print_stmt(stmt));
        out.push('\n');
    }
    out
}

/// 단일 구문을 `.xzz` 소스로 되돌린다.
pub fn print_stmt(stmt: &Stmt) -> String {
    match stmt {
        Stmt::TypeDecl { name, fields } => print_type_decl(name, fields),
        Stmt::ModelDecl { name, layers } => print_model_decl(name, layers),
        Stmt::VarDecl {
            var_name,
            is_mut,
            source,
            ops,
        } => {
            let head = if *is_mut { "mut v" } else { "v" };
            let mut s = format!("{} {} = {}", head, var_name, print_source(source));
            s.push_str(&print_ops(ops));
            s.push(';');
            s
        }
        Stmt::ExprStmt { source, ops } => {
            let mut s = print_source(source);
            s.push_str(&print_ops(ops));
            s.push(';');
            s
        }
        Stmt::TrainStmt {
            source_var,
            model_name,
            config,
        } => {
            format!(
                "run {}\n    |> train({})",
                source_var,
                print_train_args(model_name, config)
            )
        }
    }
}

// ── 선언부 ───────────────────────────────────────────────────────────────────

fn print_type_decl(name: &str, fields: &[StructField]) -> String {
    let mut s = format!("type {} = {{\n", name);
    for f in fields {
        s.push_str(&format!("    {}: {},\n", f.name, f.field_type));
    }
    s.push_str("};");
    s
}

fn print_model_decl(name: &str, layers: &[LayerKind]) -> String {
    let body = layers
        .iter()
        .map(print_layer)
        .collect::<Vec<_>>()
        .join(" -> ");
    format!("model {} {{\n    {}\n}}", name, body)
}

fn print_layer(layer: &LayerKind) -> String {
    match layer {
        LayerKind::Dense(n) => format!("Dense({})", n),
        LayerKind::ReLU => "ReLU()".to_string(),
        LayerKind::Sigmoid => "Sigmoid()".to_string(),
        LayerKind::Tanh => "Tanh()".to_string(),
        LayerKind::Softmax => "Softmax()".to_string(),
        LayerKind::Dropout(r) => format!("Dropout({})", print_f64(*r)),
        LayerKind::BatchNorm => "BatchNorm()".to_string(),
    }
}

fn print_source(source: &PipelineSource) -> String {
    match source {
        PipelineSource::Load {
            file_path,
            schema_name,
        } => format!("load(\"{}\") :: {}", escape(file_path), schema_name),
        PipelineSource::VarRef(name) => name.clone(),
    }
}

// ── 파이프라인 연산자 ────────────────────────────────────────────────────────

fn print_ops(ops: &[PipelineOp]) -> String {
    let mut s = String::new();
    for op in ops {
        s.push_str("\n    |> ");
        s.push_str(&print_op(op));
    }
    s
}

/// 파이프라인 연산자 하나를 `.xzz` 표기로 되돌린다.
pub fn print_op(op: &PipelineOp) -> String {
    match op {
        PipelineOp::Filter(expr) => format!("filter({})", print_expr(expr)),
        PipelineOp::Select(cols) => format!("select([{}])", cols.join(", ")),
        PipelineOp::Count(None) => "count()".to_string(),
        PipelineOp::Count(Some(col)) => format!("count(\"{}\")", escape(col)),
        PipelineOp::GroupBy(col) => format!("groupBy(\"{}\")", escape(col)),
        PipelineOp::Sum(col) => format!("sum(\"{}\")", escape(col)),
        PipelineOp::Mean(col) => format!("mean(\"{}\")", escape(col)),
        PipelineOp::Min(col) => format!("min(\"{}\")", escape(col)),
        PipelineOp::Max(col) => format!("max(\"{}\")", escape(col)),
        PipelineOp::Median(col) => format!("median(\"{}\")", escape(col)),
        PipelineOp::Variance(col) => format!("variance(\"{}\")", escape(col)),
        PipelineOp::Std(col) => format!("std(\"{}\")", escape(col)),
        PipelineOp::OrderBy { col, desc } => {
            format!("orderBy(\"{}\", desc: {})", escape(col), desc)
        }
        PipelineOp::Take(n) => format!("take({})", n),
        PipelineOp::DropNull(col) => format!("dropNull(\"{}\")", escape(col)),
        PipelineOp::FillNull { col, value } => match value {
            FillNullValue::Int(v) => format!("fillNull(\"{}\", {})", escape(col), v),
            FillNullValue::Float(v) => {
                format!("fillNull(\"{}\", {})", escape(col), print_f64(*v))
            }
            FillNullValue::Str(v) => {
                format!("fillNull(\"{}\", \"{}\")", escape(col), escape(v))
            }
            FillNullValue::Mean => {
                format!("fillNull(\"{}\", strategy: \"mean\")", escape(col))
            }
            FillNullValue::Median => {
                format!("fillNull(\"{}\", strategy: \"median\")", escape(col))
            }
            FillNullValue::Zero => {
                format!("fillNull(\"{}\", strategy: \"zero\")", escape(col))
            }
        },
        PipelineOp::Join {
            other,
            left_on,
            right_on,
            how,
        } => print_join(other, left_on, right_on, how),
        PipelineOp::WithColumn { name, expr } => {
            format!("withColumn(\"{}\", {})", escape(name), print_expr(expr))
        }
        PipelineOp::Chart(config) => print_chart(config),
        PipelineOp::Cast { col, to_type } => {
            format!("cast(\"{}\", \"{}\")", escape(col), escape(to_type))
        }
        PipelineOp::Rename { old_name, new_name } => {
            format!("rename(\"{}\", \"{}\")", escape(old_name), escape(new_name))
        }
        PipelineOp::Replace { col, from, to } => format!(
            "replace(\"{}\", \"{}\", \"{}\")",
            escape(col),
            escape(from),
            escape(to)
        ),
        PipelineOp::Sample { n, seed } => match seed {
            Some(s) => format!("sample({}, seed: {})", n, s),
            None => format!("sample({})", n),
        },
        PipelineOp::Train { model_name, config } => {
            format!("train({})", print_train_args(model_name, config))
        }
        PipelineOp::Predict { model_var, as_col } => match as_col {
            Some(c) => format!("predict({}, as: \"{}\")", model_var, escape(c)),
            None => format!("predict({})", model_var),
        },
        PipelineOp::WithDp(args) => print_with_dp(args),
    }
}

fn print_join(other: &str, left_on: &[String], right_on: &[String], how: &JoinHow) -> String {
    let key_list = |keys: &[String]| -> String {
        if keys.len() == 1 {
            format!("\"{}\"", escape(&keys[0]))
        } else {
            let inner = keys
                .iter()
                .map(|k| format!("\"{}\"", escape(k)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{}]", inner)
        }
    };

    let how_str = match how {
        JoinHow::Inner => "inner",
        JoinHow::Left => "left",
        JoinHow::Outer => "outer",
        JoinHow::Cross => "cross",
    };

    // left_on 과 right_on 이 같으면 축약형 on: 을 쓴다.
    if left_on == right_on && !left_on.is_empty() {
        format!(
            "join({}, on: {}, how: \"{}\")",
            other,
            key_list(left_on),
            how_str
        )
    } else {
        format!(
            "join({}, left_on: {}, right_on: {}, how: \"{}\")",
            other,
            key_list(left_on),
            key_list(right_on),
            how_str
        )
    }
}

fn print_chart(config: &ChartConfig) -> String {
    let mut s = format!("chart {{\n        type: {}\n", config.chart_type.as_str());
    if let Some(x) = &config.x {
        s.push_str(&format!("        x: {}\n", x));
    }
    if let Some(y) = &config.y {
        s.push_str(&format!("        y: {}\n", y));
    }
    if let Some(label) = &config.label {
        s.push_str(&format!("        label: {}\n", label));
    }
    if let Some(value) = &config.value {
        s.push_str(&format!("        value: {}\n", value));
    }
    if let Some(title) = &config.title {
        s.push_str(&format!("        title: \"{}\"\n", escape(title)));
    }
    s.push_str("    }");
    s
}

fn print_with_dp(args: &DpArgs) -> String {
    let mut parts = vec![format!("epsilon: {}", print_f64(args.epsilon))];
    parts.push(format!("mechanism: {}", args.mechanism.as_str()));
    parts.push(format!("sensitivity: {}", print_f64(args.sensitivity)));
    if let Some(delta) = args.delta {
        parts.push(format!("delta: {}", print_f64(delta)));
    }
    if let Some(seed) = args.seed {
        parts.push(format!("seed: {}", seed));
    }
    format!("withDp({})", parts.join(", "))
}

fn print_train_args(model_name: &str, config: &TrainConfig) -> String {
    let mut parts = vec![
        model_name.to_string(),
        format!("target: \"{}\"", escape(&config.target)),
        format!("epochs: {}", config.epochs),
        format!("lr: {}", print_f64(config.learning_rate)),
    ];
    if let Some(bs) = config.batch_size {
        parts.push(format!("batch_size: {}", bs));
    }
    if let Some(vs) = config.validation_split {
        parts.push(format!("validation_split: {}", print_f64(vs)));
    }
    parts.join(", ")
}

// ── 표현식 ───────────────────────────────────────────────────────────────────

/// 표현식을 `.xzz` 표기로 되돌린다.
pub fn print_expr(expr: &Expr) -> String {
    match expr {
        Expr::Ident(name) => name.clone(),
        Expr::StringLit(s) => format!("\"{}\"", escape(s)),
        Expr::IntLit(v) => v.to_string(),
        Expr::FloatLit(v) => print_f64(*v),
        Expr::BoolLit(v) => v.to_string(),
        Expr::BinOp { lhs, op, rhs } => {
            format!(
                "{} {} {}",
                print_expr(lhs),
                print_binop(op),
                print_expr(rhs)
            )
        }
    }
}

fn print_binop(op: &BinOpKind) -> &'static str {
    match op {
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
    }
}

// ── 리터럴 포맷 ──────────────────────────────────────────────────────────────

/// f64 를 `.xzz` 부동소수 리터럴로 찍는다.
///
/// 렉서가 소수점 없는 값을 정수로 읽어버리므로 항상 소수점을 유지한다.
pub(crate) fn print_f64(v: f64) -> String {
    if v.is_finite() && v == v.trunc() && v.abs() < 1e15 {
        format!("{:.1}", v)
    } else {
        // {} 는 f64 의 최단 왕복(round-trip) 표기를 보장한다.
        let s = format!("{}", v);
        if s.contains('.') || s.contains('e') || s.contains('E') {
            s
        } else {
            format!("{}.0", s)
        }
    }
}

/// 문자열 리터럴 내부의 `"` 와 `\` 를 이스케이프한다.
/// codegen/emitter에서 생성된 Rust 소스에 삽입되는 DSL 문자열 값에 공통 사용한다.
pub(crate) fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

// ── 유닛 테스트 ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Lexer, Parser};

    fn parse(src: &str) -> Program {
        let tokens = Lexer::new(src)
            .tokenize()
            .unwrap_or_else(|e| panic!("렉싱 실패: {}\n--- 소스 ---\n{}", e, src));
        Parser::new(tokens)
            .parse()
            .unwrap_or_else(|e| panic!("파싱 실패: {}\n--- 소스 ---\n{}", e, src))
    }

    /// parse(print(parse(src))) == parse(src) 왕복 성질을 검증한다.
    fn assert_round_trip(src: &str) {
        let first = parse(src);
        let printed = print_program(&first);
        let second = parse(&printed);
        assert_eq!(
            first, second,
            "왕복 실패\n--- 원본 ---\n{}\n--- 출력 ---\n{}",
            src, printed
        );
    }

    /// 기본 전처리 파이프라인 왕복.
    #[test]
    fn round_trip_preprocessing() {
        assert_round_trip(
            "type AQ = { station: string, pm10: Option<float>, pm25: Option<float> };
             v a = load(\"data/air.csv\") :: AQ
               |> select([station, pm10, pm25])
               |> dropNull(\"pm10\")
               |> fillNull(\"pm25\", 0)
               |> filter(pm10 > 10)
               |> orderBy(\"pm10\", desc: true)
               |> take(10);",
        );
    }

    /// 집계 + 차등 프라이버시 파이프라인 왕복.
    #[test]
    fn round_trip_aggregation_with_dp() {
        assert_round_trip(
            "type AQ = { station: string, pm10: Option<float> };
             v a = load(\"data/air.csv\") :: AQ
               |> groupBy(\"station\")
               |> mean(\"pm10\")
               |> withDp(epsilon: 0.5, mechanism: gaussian, sensitivity: 2.0, delta: 0.00001, seed: 42);",
        );
    }

    /// 차트 블록 왕복.
    #[test]
    fn round_trip_chart() {
        assert_round_trip(
            "type C = { region: string, case_id: int };
             v c = load(\"data/crime.csv\") :: C
               |> groupBy(\"region\")
               |> count(\"case_id\")
               |> chart {
                    type: bar
                    x: region
                    y: case_id
                    title: \"지역별 건수\"
                  };",
        );
    }

    /// 모델 선언 + 학습 구문 왕복.
    #[test]
    fn round_trip_model_and_train() {
        assert_round_trip(
            "type AQ = { pm10: Option<float>, pm25: Option<float> };
             v ds = load(\"data/air.csv\") :: AQ
               |> cast(\"pm10\", \"float\")
               |> fillNull(\"pm10\", strategy: \"mean\");
             model P {
                 Dense(64) -> ReLU() -> Dropout(0.2) -> Dense(1)
             }
             run ds
               |> train(P, target: \"pm10\", epochs: 10, lr: 0.01);",
        );
    }

    /// 변수 참조 소스 + 컬럼 연산 왕복.
    #[test]
    fn round_trip_var_ref_and_column_ops() {
        assert_round_trip(
            "type AQ = { station: string, pm10: Option<float> };
             v a = load(\"data/air.csv\") :: AQ;
             v b = a
               |> rename(\"pm10\", \"dust\")
               |> withColumn(\"double_dust\", dust * 2)
               |> replace(\"station\", \".\", \"\")
               |> sample(100, seed: 7);",
        );
    }

    /// 실수 리터럴이 정수로 퇴화하지 않는다 (렉서 왕복 안전성).
    #[test]
    fn float_literals_keep_decimal_point() {
        assert_eq!(print_f64(1.0), "1.0");
        assert_eq!(print_f64(0.5), "0.5");
        assert_eq!(print_f64(2.0), "2.0");
    }

    /// 문자열 이스케이프 처리.
    #[test]
    fn escapes_quotes_and_backslashes() {
        assert_eq!(escape("a\"b"), "a\\\"b");
        assert_eq!(escape("a\\b"), "a\\\\b");
    }
}
