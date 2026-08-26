// xazz-compiler/src/checker.rs — 정적 의미 분석기 (Type Checker)
//
// Parser 가 만든 Program AST 를 순회하며, 런타임 전에 잡아낼 수 있는
// 의미(semantic) 오류를 컴파일 시점에 검출한다.
//
// 검사 대상:
//   - 미선언 / 사용전-선언 변수, 모델, 스키마 참조
//   - 중복 선언 (스키마 / 모델 / 변수)
//   - 스키마 기반 컬럼 존재성 검증 (SafeLoadViolation + Did-you-mean)
//   - join 대상 변수 존재성
//   - cast() 대상 타입 유효성
//   - groupBy 후 집계 누락, 문자열 컬럼 집계 경고
//   - train() / predict() 모델·변수 참조 검증
//
// AST 노드에 span 이 없으므로 오류는 span(0,0) 으로 생성되며,
// CompileError::Display 는 이 경우 line 을 생략한다. AI 수정 제안은
// ErrorKind::generate_suggestion() 를 통해 자동 부여된다.

use std::collections::{HashMap, HashSet};

use crate::ast::{
    ChartConfig, Expr, FillNullValue, LayerKind, PipelineOp, PipelineSource, Program, Stmt,
    StructField, TrainConfig,
};
use crate::error::{CompileError, ErrorKind};

/// 컬럼 타입 정보 (canonical name + nullable 여부)
#[derive(Debug, Clone, PartialEq)]
pub struct ColType {
    pub name: String,
    pub option: bool,
}

impl ColType {
    fn new(name: &str, option: bool) -> Self {
        ColType {
            name: name.to_string(),
            option,
        }
    }
    fn is_numeric(&self) -> bool {
        matches!(self.name.as_str(), "int" | "float")
    }
}

/// 분석 결과 — 오류 목록과 경고 목록
#[derive(Debug, Clone, Default)]
pub struct CheckResult {
    pub errors: Vec<CompileError>,
    pub warnings: Vec<CompileError>,
}

impl CheckResult {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
    pub fn is_err(&self) -> bool {
        !self.errors.is_empty()
    }
}

/// 분석기 상태 — 스키마 / 모델 / 변수 심볼 테이블
struct Analyzer {
    schemas: HashMap<String, Vec<StructField>>,
    models: HashMap<String, Vec<LayerKind>>,
    vars: HashMap<String, VarInfo>,
    trained_vars: HashSet<String>,
    errors: Vec<CompileError>,
    warnings: Vec<CompileError>,
}

/// 변수에 대한 추론된 컬럼 스키마
#[derive(Debug, Clone, Default)]
struct VarInfo {
    columns: HashMap<String, ColType>,
}

/// 분석기의 최상위 진입점 — Program AST 를 검사한다.
pub fn check_program(program: &Program) -> CheckResult {
    let mut a = Analyzer {
        schemas: HashMap::new(),
        models: HashMap::new(),
        vars: HashMap::new(),
        trained_vars: HashSet::new(),
        errors: Vec::new(),
        warnings: Vec::new(),
    };
    a.check_program(program);
    CheckResult {
        errors: a.errors,
        warnings: a.warnings,
    }
}

/// 소스 문자열을 렉싱·파싱·검사한 결과를 반환한다.
pub fn check_source(source: &str) -> (crate::CompileResult<crate::Program>, CheckResult) {
    let tokens = match crate::Lexer::new(source).tokenize() {
        Ok(t) => t,
        Err(e) => return (Err(e), CheckResult::default()),
    };
    match crate::Parser::new(tokens).parse() {
        Ok(program) => {
            let result = check_program(&program);
            (Ok(program), result)
        }
        Err(e) => (Err(e), CheckResult::default()),
    }
}

impl Analyzer {
    fn check_program(&mut self, program: &Program) {
        for stmt in &program.stmts {
            self.check_stmt(stmt);
        }
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::TypeDecl { name, fields } => self.check_type_decl(name, fields),
            Stmt::ModelDecl { name, layers } => self.check_model_decl(name, layers),
            Stmt::VarDecl {
                var_name,
                source,
                ops,
                ..
            } => self.check_var_decl(var_name, source, ops),
            Stmt::ExprStmt { source, ops } => {
                self.check_pipeline(None, source, ops);
            }
            Stmt::TrainStmt {
                source_var,
                model_name,
                config,
            } => self.check_train_stmt(source_var, model_name, config),
        }
    }

    fn check_type_decl(&mut self, name: &str, fields: &[StructField]) {
        if self.schemas.contains_key(name) {
            self.error(
                ErrorKind::Other("중복된 스키마 선언".to_string()),
                format!(
                    "스키마 '{}' 가 두 번 선언되었습니다. 이름을 다르게 지정하거나 중복을 제거하세요.",
                    name
                ),
            );
            return;
        }
        self.schemas.insert(name.to_string(), fields.to_vec());
    }

    fn check_model_decl(&mut self, name: &str, layers: &[LayerKind]) {
        if self.models.contains_key(name) {
            self.error(
                ErrorKind::Other("중복된 모델 선언".to_string()),
                format!(
                    "모델 '{}' 이 두 번 선언되었습니다. 이름을 다르게 지정하세요.",
                    name
                ),
            );
            return;
        }
        let has_dense = layers
            .iter()
            .any(|l| matches!(l, LayerKind::Dense(n) if *n > 0));
        if !has_dense {
            self.error(
                ErrorKind::Other("Dense 레이어 없음".to_string()),
                format!(
                    "모델 '{}' 에 유효한 Dense 레이어가 없습니다. 최소 하나의 Dense(units) 레이어가 필요합니다.",
                    name
                ),
            );
        }
        self.models.insert(name.to_string(), layers.to_vec());
    }

    fn check_var_decl(&mut self, var_name: &str, source: &PipelineSource, ops: &[PipelineOp]) {
        if self.vars.contains_key(var_name) || self.trained_vars.contains(var_name) {
            self.error(
                ErrorKind::Other("중복된 변수 선언".to_string()),
                format!(
                    "변수 '{}' 가 이미 선언되어 있습니다. 이름을 다르게 지정하세요.",
                    var_name
                ),
            );
            return;
        }
        let is_trained = self.check_pipeline(Some(var_name), source, ops);
        if is_trained {
            self.trained_vars.insert(var_name.to_string());
        }
    }

    fn check_train_stmt(&mut self, source_var: &str, model_name: &str, config: &TrainConfig) {
        if !self.vars.contains_key(source_var) {
            self.error(
                ErrorKind::UndeclaredVariable(source_var.to_string()),
                format!(
                    "run {} : 데이터 소스 변수 '{}' 가 선언되지 않았습니다.",
                    source_var, source_var
                ),
            );
            return;
        }
        if !self.models.contains_key(model_name) {
            self.error(
                ErrorKind::Other("미선언 모델".to_string()),
                format!(
                    "run |> train({}) : 모델 '{}' 이 선언되지 않았습니다. 먼저 `model {} {{ ... }}` 로 선언하세요.",
                    model_name, model_name, model_name
                ),
            );
            return;
        }
        if let Some(var) = self.vars.get(source_var) {
            if !var.columns.contains_key(&config.target) {
                self.error(
                    ErrorKind::SafeLoadViolation {
                        col: config.target.clone(),
                        schema: source_var.to_string(),
                        available: sorted_keys(&var.columns),
                    },
                    format!(
                        "run |> train({}) : 타겟 컬럼 '{}' 이 소스 변수 '{}' 의 스키마에 없습니다.",
                        model_name, config.target, source_var
                    ),
                );
            } else if let Some(t) = var.columns.get(&config.target)
                && !t.is_numeric()
            {
                self.warning(format!(
                    "타겟 컬럼 '{}' 이 숫자형이 아니어서 학습 입력이 될 수 없습니다. 학습은 숫자형 타겟을 요구합니다.",
                    config.target
                ));
            }
        }
    }

    /// 파이프라인을 검사하고, 최종 컬럼 스키마를 추론한다.
    /// 반환값: 파이프라인이 train() 으로 끝나 모델 변수가 되는지 여부.
    fn check_pipeline(
        &mut self,
        binding: Option<&str>,
        source: &PipelineSource,
        ops: &[PipelineOp],
    ) -> bool {
        let mut columns: Option<HashMap<String, ColType>> = None;

        match source {
            PipelineSource::Load {
                file_path: _,
                schema_name,
            } => match self.schemas.get(schema_name) {
                Some(fields) => {
                    let mut map = HashMap::new();
                    for f in fields {
                        map.insert(f.name.clone(), col_type_of_field(&f.field_type));
                    }
                    columns = Some(map);
                }
                None => {
                    self.error(
                        ErrorKind::UndeclaredType(schema_name.to_string()),
                        format!(
                            "load(...) :: {} : 스키마 '{}' 이(가) 선언되지 않았습니다. `type {} = {{ ... }}` 로 먼저 선언하세요.",
                            schema_name, schema_name, schema_name
                        ),
                    );
                }
            },
            PipelineSource::VarRef(name) => {
                if self.trained_vars.contains(name) {
                    self.error(
                        ErrorKind::UndeclaredVariable(name.to_string()),
                        format!(
                            "변수 '{}' 은 학습된 모델입니다. DataFrame 파이프라인 소스로 사용할 수 없습니다.",
                            name
                        ),
                    );
                } else if let Some(var) = self.vars.get(name) {
                    columns = Some(var.columns.clone());
                } else {
                    self.error(
                        ErrorKind::UndeclaredVariable(name.to_string()),
                        format!(
                            "변수 '{}' 이(가) 선언되지 않았습니다. 이 변수를 이전 파이프라인에서 먼저 선언하세요.",
                            name
                        ),
                    );
                }
            }
        }

        let mut cols = columns.unwrap_or_default();
        let mut pending_group: Option<String> = None;

        for op in ops {
            match op {
                PipelineOp::Train { model_name, .. } => {
                    if !self.models.contains_key(model_name) {
                        self.error(
                            ErrorKind::Other("미선언 모델".to_string()),
                            format!(
                                "train({}) : 모델 '{}' 은(는) 선언되지 않았습니다. 먼저 `model {} {{ ... }}` 로 선언하세요.",
                                model_name, model_name, model_name
                            ),
                        );
                    }
                    return true;
                }
                PipelineOp::Predict { model_var, as_col } => {
                    if !self.trained_vars.contains(model_var) {
                        self.error(
                            ErrorKind::UndeclaredVariable(model_var.to_string()),
                            format!(
                                "predict({}) : 변수 '{}' 은(는) 학습된 모델이 아닙니다. 먼저 `v {} = ... |> train(...)` 으로 학습하세요.",
                                model_var, model_var, model_var
                            ),
                        );
                    }
                    if let Some(name) = as_col {
                        cols.insert(name.clone(), ColType::new("float", true));
                    }
                }
                PipelineOp::Filter(expr) => {
                    self.check_expr_columns(expr, &cols);
                }
                PipelineOp::Select(cols_sel) => {
                    let mut next = HashMap::new();
                    for c in cols_sel {
                        if let Some(t) = cols.get(c) {
                            next.insert(c.clone(), t.clone());
                        } else {
                            self.column_missing(c, "select");
                        }
                    }
                    cols = next;
                }
                PipelineOp::GroupBy(group_col) => {
                    self.check_column(group_col, "groupBy", &cols);
                    pending_group = Some(group_col.clone());
                }
                PipelineOp::Count(Some(c))
                | PipelineOp::Sum(c)
                | PipelineOp::Mean(c)
                | PipelineOp::Min(c)
                | PipelineOp::Max(c)
                | PipelineOp::Median(c)
                | PipelineOp::Variance(c)
                | PipelineOp::Std(c) => {
                    self.check_agg_column(c, &cols);
                    pending_group = None;
                }
                PipelineOp::Count(None) => {}
                PipelineOp::OrderBy { col, .. } => {
                    self.check_column(col, "orderBy", &cols);
                }
                PipelineOp::Take(_) | PipelineOp::Sample { .. } => {}
                PipelineOp::DropNull(drop_col) => {
                    self.check_column(drop_col, "dropNull", &cols);
                }
                PipelineOp::FillNull { col, value } => {
                    self.check_column(col, "fillNull", &cols);
                    self.check_fill_value(col, value, &cols);
                }
                PipelineOp::Join {
                    other,
                    left_on,
                    right_on,
                    ..
                } => {
                    for k in left_on {
                        self.check_column(k, "join(left_on)", &cols);
                    }
                    if self.trained_vars.contains(other) {
                        self.error(
                            ErrorKind::UndeclaredVariable(other.to_string()),
                            format!("join() 대상 변수 '{}' 은 학습된 모델입니다.", other),
                        );
                    } else if let Some(var) = self.vars.get(other) {
                        let right_cols: HashMap<String, ColType> = var.columns.clone();
                        for k in right_on {
                            self.check_column(k, "join(right_on)", &right_cols);
                        }
                    } else {
                        self.error(
                            ErrorKind::UndeclaredVariable(other.to_string()),
                            format!("join() 대상 변수 '{}' 이(가) 선언되지 않았습니다.", other),
                        );
                    }
                }
                PipelineOp::WithColumn { name, expr } => {
                    self.check_expr_columns(expr, &cols);
                    let t = infer_expr_type(expr, &cols);
                    cols.insert(name.clone(), t);
                }
                PipelineOp::Chart(config) => {
                    self.check_chart(config, &cols);
                }
                PipelineOp::Cast { col, to_type } => {
                    if !matches!(to_type.as_str(), "float" | "int" | "str" | "bool") {
                        self.error(
                            ErrorKind::Other("알 수 없는 cast 타입".to_string()),
                            format!(
                                "cast(\"{}\", \"{}\") : 알 수 없는 타입 '{}'. 지원 타입: \"float\", \"int\", \"str\", \"bool\"",
                                col, to_type, to_type
                            ),
                        );
                    }
                    self.check_column(col, "cast", &cols);
                    if let Some(t) = cols.get(col).cloned() {
                        let nt = normalize_type(to_type);
                        cols.insert(col.clone(), ColType::new(nt, t.option));
                    }
                }
                PipelineOp::Rename { old_name, new_name } => {
                    self.check_column(old_name, "rename", &cols);
                    if let Some(t) = cols.remove(old_name) {
                        cols.insert(new_name.clone(), t);
                    }
                }
                PipelineOp::Replace { col, .. } => {
                    self.check_column(col, "replace", &cols);
                }
            }
        }

        if let Some(g) = pending_group {
            self.error(
                ErrorKind::Other("groupBy 후 집계 누락".to_string()),
                format!(
                    "groupBy(\"{}\") 뒤에 sum/mean/min/max/count 등 집계 연산이 없습니다. 파이프라인이 그룹된 상태로 종료될 수 없습니다.",
                    g
                ),
            );
        }

        if let Some(binding) = binding {
            self.vars
                .insert(binding.to_string(), VarInfo { columns: cols });
        }
        false
    }

    fn check_agg_column(&mut self, col: &str, cols: &HashMap<String, ColType>) {
        match cols.get(col) {
            Some(t) if !t.is_numeric() => {
                self.warning(format!(
                    "집계 컬럼 '{}' 이 숫자형이 아닙니다. 집계(sum/mean/min/max)는 숫자형 컬럼에만 의미가 있습니다.",
                    col
                ));
            }
            None => self.column_missing(col, "집계"),
            _ => {}
        }
    }

    fn check_fill_value(
        &mut self,
        col: &str,
        value: &FillNullValue,
        cols: &HashMap<String, ColType>,
    ) {
        if let Some(t) = cols.get(col) {
            let is_str_fill = matches!(value, FillNullValue::Str(_));
            if is_str_fill && t.is_numeric() {
                self.warning(format!(
                    "fillNull(\"{}\", <문자열>) : 숫자형 컬럼 '{}' 을 문자열로 채우면 타입이 바뀔 수 있습니다.",
                    col, col
                ));
            } else if !is_str_fill && !t.is_numeric() && t.name != "unknown" {
                self.warning(format!(
                    "fillNull(\"{}\", <숫자>) : 문자열 컬럼 '{}' 에 숫자 값을 채우면 타입이 바뀔 수 있습니다.",
                    col, col
                ));
            }
        }
    }

    fn check_chart(&mut self, config: &ChartConfig, cols: &HashMap<String, ColType>) {
        let checks: [Option<&str>; 2] = match config.chart_type.as_str() {
            "pie" => [config.label.as_deref(), config.value.as_deref()],
            _ => [config.x.as_deref(), config.y.as_deref()],
        };
        for c in checks.iter().flatten() {
            self.check_column(c, "chart", cols);
        }
    }

    fn check_expr_columns(&mut self, expr: &Expr, cols: &HashMap<String, ColType>) {
        match expr {
            Expr::Ident(c) => {
                self.check_column(c, "표현식", cols);
            }
            Expr::BinOp { lhs, rhs, .. } => {
                self.check_expr_columns(lhs, cols);
                self.check_expr_columns(rhs, cols);
            }
            _ => {}
        }
    }

    fn check_column(&mut self, col: &str, ctx: &str, cols: &HashMap<String, ColType>) {
        if !cols.contains_key(col) {
            self.column_missing(col, ctx);
        }
    }

    fn column_missing(&mut self, col: &str, ctx: &str) {
        let available: Vec<String> = self
            .vars
            .values()
            .flat_map(|v| v.columns.keys().cloned())
            .collect();
        self.error(
            ErrorKind::SafeLoadViolation {
                col: col.to_string(),
                schema: ctx.to_string(),
                available,
            },
            format!("{}: 스키마에 '{}' 컬럼이 존재하지 않습니다.", ctx, col),
        );
    }

    fn error(&mut self, kind: ErrorKind, message: impl Into<String>) {
        self.errors
            .push(CompileError::new(kind, crate::Span::new(0, 0), message));
    }

    fn warning(&mut self, message: impl Into<String>) {
        self.warnings.push(CompileError::new(
            ErrorKind::Other("경고".to_string()),
            crate::Span::new(0, 0),
            message,
        ));
    }
}

/// 스키마 field_type 문자열 → ColType 변환 (Option<T> 지원)
fn col_type_of_field(field_type: &str) -> ColType {
    if let Some(inner) = field_type.strip_prefix("Option<") {
        ColType::new(normalize_type(inner.trim_end_matches('>')), true)
    } else {
        ColType::new(normalize_type(field_type), false)
    }
}

fn normalize_type(t: &str) -> &'static str {
    match t {
        "string" | "str" => "string",
        "int" => "int",
        "float" => "float",
        "bool" => "bool",
        _ => "unknown",
    }
}

/// 표현식의 결과 타입 추론 (withColumn 새 컬럼용)
fn infer_expr_type(expr: &Expr, cols: &HashMap<String, ColType>) -> ColType {
    match expr {
        Expr::Ident(c) => cols
            .get(c)
            .cloned()
            .unwrap_or_else(|| ColType::new("unknown", true)),
        Expr::IntLit(_) => ColType::new("int", false),
        Expr::FloatLit(_) => ColType::new("float", false),
        Expr::BoolLit(_) => ColType::new("bool", false),
        Expr::StringLit(_) => ColType::new("string", false),
        Expr::BinOp { op, .. } => match op {
            crate::BinOpKind::Eq
            | crate::BinOpKind::NotEq
            | crate::BinOpKind::Lt
            | crate::BinOpKind::Gt
            | crate::BinOpKind::LtEq
            | crate::BinOpKind::GtEq => ColType::new("bool", false),
            crate::BinOpKind::Add
            | crate::BinOpKind::Sub
            | crate::BinOpKind::Mul
            | crate::BinOpKind::Div => ColType::new("float", false),
        },
    }
}

fn sorted_keys(map: &HashMap<String, ColType>) -> Vec<String> {
    let mut keys: Vec<String> = map.keys().cloned().collect();
    keys.sort();
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Lexer;
    use crate::parser::Parser;

    fn check(src: &str) -> CheckResult {
        let tokens = Lexer::new(src).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        check_program(&program)
    }

    fn err_kinds(result: &CheckResult) -> Vec<String> {
        result
            .errors
            .iter()
            .map(|e| format!("{:?}", e.kind))
            .collect()
    }

    #[test]
    fn ok_pipeline_no_diagnostics() {
        let r = check(
            "type S = { station: string, pm10: Option<float> };
             v a = load(\"x.csv\") :: S |> filter(pm10 > 10) |> mean(\"pm10\");",
        );
        assert!(r.is_ok(), "예상치 못한 오류: {:?}", r.errors);
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn undeclared_schema_is_error() {
        let r = check("v a = load(\"x.csv\") :: Missing |> select([a]);");
        assert!(r.is_err());
        assert!(err_kinds(&r).iter().any(|k| k.contains("UndeclaredType")));
    }

    #[test]
    fn undeclared_variable_ref_is_error() {
        let r = check("v a = b |> select([x]);");
        assert!(r.is_err());
        assert!(
            err_kinds(&r)
                .iter()
                .any(|k| k.contains("UndeclaredVariable"))
        );
    }

    #[test]
    fn missing_column_is_safe_load_violation() {
        let r = check(
            "type X = { station: string, pm10: float };
             v a = load(\"x.csv\") :: X |> filter(pm25 > 1);",
        );
        assert!(r.is_err());
        assert!(
            err_kinds(&r)
                .iter()
                .any(|k| k.contains("SafeLoadViolation"))
        );
    }

    #[test]
    fn missing_column_gets_did_you_mean_hint() {
        let r = check(
            "type X = { pm10: float, pm25: float };
             v a = load(\"x.csv\") :: X |> select([pm10, pm_20]);",
        );
        assert!(r.is_err());
        assert!(r.errors.iter().any(|e| e.ai_suggestion.is_some()));
    }

    #[test]
    fn duplicate_schema_is_error() {
        let r = check(
            "type X = { a: string };
             type X = { b: int };",
        );
        assert!(r.is_err());
        assert!(err_kinds(&r).iter().any(|k| k.contains("중복된 스키마")));
    }

    #[test]
    fn duplicate_var_is_error() {
        let r = check(
            "type X = { a: string };
             v p = load(\"x.csv\") :: X;
             v p = load(\"x.csv\") :: X;",
        );
        assert!(r.is_err());
        assert!(err_kinds(&r).iter().any(|k| k.contains("중복된 변수")));
    }

    #[test]
    fn join_undeclared_other_is_error() {
        let r = check(
            "type X = { a: string, id: int };
             v left = load(\"x.csv\") :: X;
             v right = left |> join(nope, left_on: [\"id\"], right_on: [\"id\"], how: \"inner\");",
        );
        assert!(r.is_err());
        assert!(
            err_kinds(&r)
                .iter()
                .any(|k| k.contains("UndeclaredVariable"))
        );
    }

    #[test]
    fn invalid_cast_type_is_error() {
        let r = check(
            "type X = { a: string };
             v p = load(\"x.csv\") :: X |> cast(\"a\", \"decimal\");",
        );
        assert!(r.is_err());
        assert!(err_kinds(&r).iter().any(|k| k.contains("알 수 없는 cast")));
    }

    #[test]
    fn group_by_without_aggregation_is_error() {
        let r = check(
            "type X = { g: string, val: float };
             v p = load(\"x.csv\") :: X |> groupBy(\"g\");",
        );
        assert!(r.is_err());
        assert!(err_kinds(&r).iter().any(|k| k.contains("groupBy 후 집계")));
    }

    #[test]
    fn train_missing_model_is_error() {
        let r = check(
            "type X = { val: float };
             v p = load(\"x.csv\") :: X |> train(Nope, target: \"val\", epochs: 10);",
        );
        assert!(r.is_err());
        assert!(err_kinds(&r).iter().any(|k| k.contains("미선언 모델")));
    }

    #[test]
    fn predict_non_model_var_is_error() {
        let r = check(
            "type X = { val: float };
             v data = load(\"x.csv\") :: X;
             v out = data |> predict(not_a_model);",
        );
        assert!(r.is_err());
        assert!(
            err_kinds(&r)
                .iter()
                .any(|k| k.contains("UndeclaredVariable"))
        );
    }

    #[test]
    fn aggregation_on_string_warns() {
        let r = check(
            "type X = { station: string, pm10: float };
             v p = load(\"x.csv\") :: X |> groupBy(\"station\") |> sum(\"station\");",
        );
        assert!(r.is_ok());
        assert!(
            r.warnings.iter().any(|w| w.message.contains("숫자형")),
            "문자열 집계 경고 없음: {:?}",
            r.warnings
        );
    }

    #[test]
    fn valid_train_and_predict_no_error() {
        let r = check(
            "type X = { a: float, b: float, y: float };
             model M { Dense(8) -> ReLU() -> Dense(1) }
             v data = load(\"x.csv\") :: X;
             v trained = data |> train(M, target: \"y\", epochs: 5);
             v pred = data |> predict(trained, as: \"pred\");",
        );
        assert!(r.is_ok(), "오류: {:?}", r.errors);
    }

    #[test]
    fn trained_model_var_not_usable_as_source() {
        let r = check(
            "type X = { a: float, y: float };
             model M { Dense(4) }
             v data = load(\"x.csv\") :: X;
             v m = data |> train(M, target: \"y\");
             v bad = m |> select([a]);",
        );
        assert!(r.is_err());
        assert!(
            err_kinds(&r)
                .iter()
                .any(|k| k.contains("UndeclaredVariable"))
        );
    }

    #[test]
    fn model_decl_requires_dense() {
        let r = check("model M { ReLU() -> ReLU() }");
        assert!(r.is_err());
        assert!(err_kinds(&r).iter().any(|k| k.contains("Dense 레이어")));
    }

    #[test]
    fn chart_checks_columns() {
        let r = check(
            "type X = { a: string, val: float };
             v p = load(\"x.csv\") :: X |> chart { type: bar, x: a, y: missing_col };",
        );
        assert!(r.is_err());
        assert!(
            err_kinds(&r)
                .iter()
                .any(|k| k.contains("SafeLoadViolation"))
        );
    }
}
