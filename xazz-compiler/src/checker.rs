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
    BinOpKind, ChartConfig, Expr, FillNullValue, LayerKind, PipelineOp, PipelineSource, Program,
    Stmt, StructField, TrainConfig,
};
use crate::error::{CompileError, ErrorKind};
use crate::ir;
use xazz_core::i18n::is_korean;

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
    /// 타입이 붙은 IR (검사와 동시에 구축됨 — 이중 추론 방지).
    ir: ir::TypedProgram,
    /// 명령문별 토큰 슬라이스 (check_source 경유 시 Some, Span 해석용)
    stmt_tokens: Option<Vec<Vec<crate::Token>>>,
    /// 현재 처리 중인 명령문 인덱스
    cur_stmt: usize,
}

/// 변수에 대한 추론된 컬럼 스키마
#[derive(Debug, Clone, Default)]
struct VarInfo {
    columns: HashMap<String, ColType>,
}

/// 분석기의 최상위 진입점 — Program AST 를 검사한다.
pub fn check_program(program: &Program) -> CheckResult {
    let mut a = Analyzer::new(None);
    a.check_program(program);
    CheckResult {
        errors: a.errors,
        warnings: a.warnings,
    }
}

/// Program AST 를 검사하고, 검사와 동시에 타입이 붙은 IR 을 생성한다.
///
/// (이중 추론 방지: 진단과 IR 은 동일한 단일 순회에서 만들어진다.)
pub fn analyze_program(program: &Program) -> (CheckResult, ir::TypedProgram) {
    let mut a = Analyzer::new(None);
    a.check_program(program);
    (
        CheckResult {
            errors: a.errors,
            warnings: a.warnings,
        },
        a.ir,
    )
}

/// 소스 문자열을 렉싱·파싱·검사한 결과를 반환한다.
///
/// 렉서/파서 에러가 있으면 (Err, 빈 결과) 를 반환한다.
/// 성공 시 CheckResult 의 각 진단에는 소스 내 라인/컬럼(Span)이 첨부된다.
pub fn check_source(source: &str) -> (crate::CompileResult<crate::Program>, CheckResult) {
    let (parsed, check) = compile_ir(source);
    (parsed.map(|(program, _ir)| program), check)
}

/// 소스 문자열을 렉싱·파싱해 Program + Typed IR + 진단을 함께 생성한다.
///
/// 렉서/파서 에러가 있으면 (Err, 빈 결과) 를 반환한다.
/// 성공 시 진단에는 Span 이 첨부되고, IR 은 컬럼 수준 타입을 포함한다.
pub fn compile_ir(
    source: &str,
) -> (
    crate::CompileResult<(crate::Program, ir::TypedProgram)>,
    CheckResult,
) {
    let tokens = match crate::Lexer::new(source).tokenize() {
        Ok(t) => t,
        Err(e) => return (Err(e), CheckResult::default()),
    };
    match crate::Parser::new(tokens.clone()).parse() {
        Ok(program) => {
            let stmt_tokens = segment_statements(&tokens, program.stmts.len());
            let mut a = Analyzer::new(Some(stmt_tokens));
            for stmt in &program.stmts {
                a.check_stmt(stmt);
                a.cur_stmt += 1;
            }
            (
                Ok((program, a.ir)),
                CheckResult {
                    errors: a.errors,
                    warnings: a.warnings,
                },
            )
        }
        Err(e) => (Err(e), CheckResult::default()),
    }
}

/// 토큰 스트림을 명령문 단위로 분할한다.
///
/// 파서는 명령문을 `type` / `v` / `mut` / `model` / `run` 또는
/// `Ident |> ...`(expression statement) 로 시작한다. 이 경계에서 잘라
/// 명령문별 토큰 슬라이스를 반환한다. (개수는 AST stmts 와 일치해야 함)
fn segment_statements(tokens: &[crate::Token], expected: usize) -> Vec<Vec<crate::Token>> {
    use crate::TokenKind;

    let starts_stmt = |k: &TokenKind| {
        matches!(
            k,
            TokenKind::Type | TokenKind::V | TokenKind::Mut | TokenKind::Model | TokenKind::Run
        )
    };

    let mut segments: Vec<Vec<crate::Token>> = Vec::new();
    let mut current: Vec<crate::Token> = Vec::new();

    for i in 0..tokens.len() {
        let tk = &tokens[i].kind;
        // expression statement 경계: `Ident |> ...` 는 직전 토큰이 문장 종결자
        // (`;` `}` `)`) 일 때만 새 명령문으로 간주한다. 이는 `v x = ... |> ...`
        // 처럼 파이프라인 중간에 등장하는 Ident 를 오분할하는 것을 방지한다.
        let prev_is_terminator = current
            .last()
            .map(|t| {
                matches!(
                    &t.kind,
                    TokenKind::Semicolon | TokenKind::RBrace | TokenKind::RParen
                )
            })
            .unwrap_or(false);
        let expr_boundary = matches!(tk, TokenKind::Ident(_))
            && matches!(
                tokens.get(i + 1).map(|t| &t.kind),
                Some(TokenKind::Pipeline)
            )
            && prev_is_terminator;

        if !current.is_empty() && (starts_stmt(tk) || expr_boundary) {
            segments.push(std::mem::take(&mut current));
        }
        current.push(tokens[i].clone());
    }
    if !current.is_empty() {
        segments.push(current);
    }

    // 경계 감지 실패 시 전체를 하나로 묶어 반환 (동작은 보장)
    if segments.len() != expected {
        return vec![tokens.to_vec()];
    }
    segments
}

impl Analyzer {
    fn new(stmt_tokens: Option<Vec<Vec<crate::Token>>>) -> Self {
        Analyzer {
            schemas: HashMap::new(),
            models: HashMap::new(),
            vars: HashMap::new(),
            trained_vars: HashSet::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
            ir: ir::TypedProgram::new(),
            stmt_tokens,
            cur_stmt: 0,
        }
    }

    fn check_program(&mut self, program: &Program) {
        for stmt in &program.stmts {
            self.check_stmt(stmt);
            self.cur_stmt += 1;
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
                if is_korean() {
                    format!(
                        "스키마 '{}' 가 두 번 선언되었습니다. 이름을 다르게 지정하거나 중복을 제거하세요.",
                        name
                    )
                } else {
                    format!(
                        "Schema '{}' is declared twice. Rename it or remove the duplicate.",
                        name
                    )
                },
            );
            return;
        }
        self.schemas.insert(name.to_string(), fields.to_vec());
        self.ir.types.push(ir::TypeDecl {
            name: name.to_string(),
            schema: ir_schema_from_fields(fields),
        });
    }

    fn check_model_decl(&mut self, name: &str, layers: &[LayerKind]) {
        if self.models.contains_key(name) {
            self.error(
                ErrorKind::Other("중복된 모델 선언".to_string()),
                if is_korean() {
                    format!(
                        "모델 '{}' 이 두 번 선언되었습니다. 이름을 다르게 지정하세요.",
                        name
                    )
                } else {
                    format!(
                        "Model '{}' is declared twice. Choose a different name.",
                        name
                    )
                },
            );
            return;
        }
        let has_dense = layers
            .iter()
            .any(|l| matches!(l, LayerKind::Dense(n) if *n > 0));
        if !has_dense {
            self.error(
                ErrorKind::Other("Dense 레이어 없음".to_string()),
                if is_korean() {
                    format!(
                        "모델 '{}' 에 유효한 Dense 레이어가 없습니다. 최소 하나의 Dense(units) 레이어가 필요합니다.",
                        name
                    )
                } else {
                    format!(
                        "Model '{}' has no valid Dense layer. At least one Dense(units) layer is required.",
                        name
                    )
                },
            );
        }
        self.models.insert(name.to_string(), layers.to_vec());
        self.ir.models.push(ir::ModelGraph {
            name: name.to_string(),
            layers: layers.to_vec(),
        });
    }

    fn check_var_decl(&mut self, var_name: &str, source: &PipelineSource, ops: &[PipelineOp]) {
        if self.vars.contains_key(var_name) || self.trained_vars.contains(var_name) {
            self.error(
                ErrorKind::Other("중복된 변수 선언".to_string()),
                if is_korean() {
                    format!(
                        "변수 '{}' 가 이미 선언되어 있습니다. 이름을 다르게 지정하세요.",
                        var_name
                    )
                } else {
                    format!(
                        "Variable '{}' is already declared. Choose a different name.",
                        var_name
                    )
                },
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
                if is_korean() {
                    format!(
                        "run {} : 데이터 소스 변수 '{}' 가 선언되지 않았습니다.",
                        source_var, source_var
                    )
                } else {
                    format!(
                        "run {} : data source variable '{}' is not declared.",
                        source_var, source_var
                    )
                },
            );
            return;
        }
        if !self.models.contains_key(model_name) {
            self.error(
                ErrorKind::Other("미선언 모델".to_string()),
                if is_korean() {
                    format!(
                        "run |> train({}) : 모델 '{}' 이 선언되지 않았습니다. 먼저 `model {} {{ ... }}` 로 선언하세요.",
                        model_name, model_name, model_name
                    )
                } else {
                    format!(
                        "run |> train({}) : model '{}' is not declared. Declare it first with `model {} {{ ... }}`.",
                        model_name, model_name, model_name
                    )
                },
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
                    if is_korean() {
                        format!(
                            "run |> train({}) : 타겟 컬럼 '{}' 이 소스 변수 '{}' 의 스키마에 없습니다.",
                            model_name, config.target, source_var
                        )
                    } else {
                        format!(
                            "run |> train({}) : target column '{}' is not in the schema of source variable '{}'.",
                            model_name, config.target, source_var
                        )
                    },
                );
            } else if let Some(t) = var.columns.get(&config.target)
                && !t.is_numeric()
            {
                self.warning(if is_korean() {
                    format!(
                        "타겟 컬럼 '{}' 이 숫자형이 아니어서 학습 입력이 될 수 없습니다. 학습은 숫자형 타겟을 요구합니다.",
                        config.target
                    )
                } else {
                    format!(
                        "Target column '{}' is not numeric, so it cannot be a training target. Training requires a numeric target.",
                        config.target
                    )
                });
            }
        }

        // TrainStmt 는 `run <var> |> train(...)` — 바인딩 없는 모델 학습 노드로 IR 에 기록한다.
        let input_schema = self
            .vars
            .get(source_var)
            .map(|v| ir_schema_from_map(&v.columns));
        self.ir.pipelines.push(ir::PipelineNode {
            id: self.ir.pipelines.len(),
            name: None,
            source: ir::Source::Ref {
                var: source_var.to_string(),
            },
            input_schema: input_schema.clone(),
            output_schema: input_schema.unwrap_or_default(),
            steps: vec![ir::Step::ML(ir::MLOp::Train {
                model: model_name.to_string(),
                config: config.clone(),
            })],
            yields_model: false,
        });
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
        let mut yields_model = false;
        let mut steps: Vec<ir::Step> = Vec::new();

        let ir_source: ir::Source = match source {
            PipelineSource::Load {
                file_path,
                schema_name,
            } => {
                let schema_ir = self
                    .schemas
                    .get(schema_name)
                    .map(|f| ir_schema_from_fields(f));
                match self.schemas.get(schema_name) {
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
                            if is_korean() {
                                format!(
                                    "load(...) :: {} : 스키마 '{}' 이(가) 선언되지 않았습니다. `type {} = {{ ... }}` 로 먼저 선언하세요.",
                                    schema_name, schema_name, schema_name
                                )
                            } else {
                                format!(
                                    "load(...) :: {} : schema '{}' is not declared. Declare it first with `type {} = {{ ... }}`.",
                                    schema_name, schema_name, schema_name
                                )
                            },
                        );
                    }
                }
                ir::Source::Load {
                    file_path: file_path.clone(),
                    schema: schema_ir,
                }
            }
            PipelineSource::VarRef(name) => {
                if self.trained_vars.contains(name) {
                    self.error(
                        ErrorKind::UndeclaredVariable(name.to_string()),
                        if is_korean() {
                            format!(
                                "변수 '{}' 은 학습된 모델입니다. DataFrame 파이프라인 소스로 사용할 수 없습니다.",
                                name
                            )
                        } else {
                            format!(
                                "Variable '{}' is a trained model and cannot be used as a DataFrame pipeline source.",
                                name
                            )
                        },
                    );
                } else if let Some(var) = self.vars.get(name) {
                    columns = Some(var.columns.clone());
                } else {
                    self.error(
                        ErrorKind::UndeclaredVariable(name.to_string()),
                        if is_korean() {
                            format!(
                                "변수 '{}' 이(가) 선언되지 않았습니다. 이 변수를 이전 파이프라인에서 먼저 선언하세요.",
                                name
                            )
                        } else {
                            format!(
                                "Variable '{}' is not declared. Declare it in an earlier pipeline first.",
                                name
                            )
                        },
                    );
                }
                ir::Source::Ref { var: name.clone() }
            }
        };

        let input_schema = columns.as_ref().map(ir_schema_from_map);
        let mut cols = columns.unwrap_or_default();
        let mut pending_group: Option<String> = None;

        for op in ops {
            match op {
                PipelineOp::Train { model_name, config } => {
                    if !self.models.contains_key(model_name) {
                        self.error(
                            ErrorKind::Other("미선언 모델".to_string()),
                            if is_korean() {
                                format!(
                                    "train({}) : 모델 '{}' 은(는) 선언되지 않았습니다. 먼저 `model {} {{ ... }}` 로 선언하세요.",
                                    model_name, model_name, model_name
                                )
                            } else {
                                format!(
                                    "train({}) : model '{}' is not declared. Declare it first with `model {} {{ ... }}`.",
                                    model_name, model_name, model_name
                                )
                            },
                        );
                    }
                    steps.push(ir::Step::ML(ir::MLOp::Train {
                        model: model_name.clone(),
                        config: config.clone(),
                    }));
                    yields_model = true;
                    break;
                }
                PipelineOp::Predict { model_var, as_col } => {
                    if !self.trained_vars.contains(model_var) {
                        self.error(
                            ErrorKind::UndeclaredVariable(model_var.to_string()),
                            if is_korean() {
                                format!(
                                    "predict({}) : 변수 '{}' 은(는) 학습된 모델이 아닙니다. 먼저 `v {} = ... |> train(...)` 으로 학습하세요.",
                                    model_var, model_var, model_var
                                )
                            } else {
                                format!(
                                    "predict({}) : variable '{}' is not a trained model. Train it first with `v {} = ... |> train(...)`.",
                                    model_var, model_var, model_var
                                )
                            },
                        );
                    }
                    if let Some(name) = as_col {
                        cols.insert(name.clone(), ColType::new("float", true));
                    }
                    steps.push(ir::Step::ML(ir::MLOp::Predict {
                        model: model_var.clone(),
                        as_col: as_col.clone(),
                    }));
                }
                PipelineOp::Filter(expr) => {
                    self.check_expr_columns(expr, &cols);
                    let typed = type_expr(expr, &cols);
                    steps.push(ir::Step::Data(ir::DataOp::Filter(typed)));
                }
                PipelineOp::Select(cols_sel) => {
                    let mut next = HashMap::new();
                    for c in cols_sel {
                        if let Some(t) = cols.get(c) {
                            next.insert(c.clone(), t.clone());
                        } else {
                            self.column_missing_with_available(c, "select", &cols);
                        }
                    }
                    cols = next;
                    steps.push(ir::Step::Data(ir::DataOp::Select(cols_sel.clone())));
                }
                PipelineOp::GroupBy(group_col) => {
                    self.check_column(group_col, "groupBy", &cols);
                    pending_group = Some(group_col.clone());
                    steps.push(ir::Step::Data(ir::DataOp::GroupBy(group_col.clone())));
                }
                // count(col) 은 행 수를 세는 연산이라 컬럼 타입과 무관하다 — 존재성만 검사
                PipelineOp::Count(Some(c)) => {
                    self.check_column(c, "count", &cols);
                    pending_group = None;
                    steps.push(ir::Step::Data(ir::DataOp::Aggregate {
                        kind: ir::AggKind::Count,
                        col: c.clone(),
                    }));
                }
                PipelineOp::Sum(_)
                | PipelineOp::Mean(_)
                | PipelineOp::Min(_)
                | PipelineOp::Max(_)
                | PipelineOp::Median(_)
                | PipelineOp::Variance(_)
                | PipelineOp::Std(_) => {
                    let (kind, agg_col) = aggregate_kind_col(op);
                    self.check_agg_column(&agg_col, &cols);
                    pending_group = None;
                    steps.push(ir::Step::Data(ir::DataOp::Aggregate { kind, col: agg_col }));
                }
                PipelineOp::Count(None) => {
                    pending_group = None;
                    steps.push(ir::Step::Data(ir::DataOp::Aggregate {
                        kind: ir::AggKind::Len,
                        col: String::new(),
                    }));
                }
                PipelineOp::OrderBy { col, desc } => {
                    self.check_column(col, "orderBy", &cols);
                    steps.push(ir::Step::Data(ir::DataOp::Sort {
                        col: col.clone(),
                        desc: *desc,
                    }));
                }
                PipelineOp::Take(n) => {
                    steps.push(ir::Step::Data(ir::DataOp::Limit(*n)));
                }
                PipelineOp::Sample { n, seed } => {
                    steps.push(ir::Step::Data(ir::DataOp::Sample { n: *n, seed: *seed }));
                }
                PipelineOp::DropNull(drop_col) => {
                    self.check_column(drop_col, "dropNull", &cols);
                    steps.push(ir::Step::Data(ir::DataOp::DropNull(drop_col.clone())));
                }
                PipelineOp::FillNull { col, value } => {
                    self.check_column(col, "fillNull", &cols);
                    if let Some(t) = cols.get(col) {
                        if !t.option && t.name != "unknown" {
                            self.error(
                                ErrorKind::Other("fillNull on non-nullable column".to_string()),
                                if is_korean() {
                                    format!(
                                        "fillNull(\"{}\", ...) : 컬럼 '{}' 은 null을 허용하지 않는 타입으로 선언되어 있습니다. 스키마에서 '{}' 을(를) Option<{}> 으로 선언하거나, 이 연산을 제거하세요.",
                                        col, col, col, t.name
                                    )
                                } else {
                                    format!(
                                        "fillNull(\"{}\", ...) : column '{}' is declared as a non-nullable type. Declare '{}' as Option<{}> in the schema, or remove this operation.",
                                        col, col, col, t.name
                                    )
                                },
                            );
                        }
                    }
                    self.check_fill_value(col, value, &cols);
                    steps.push(ir::Step::Data(ir::DataOp::FillNull {
                        col: col.clone(),
                        value: fill_value_ir(value),
                    }));
                }
                PipelineOp::Join {
                    other,
                    left_on,
                    right_on,
                    how,
                } => {
                    for k in left_on {
                        self.check_column(k, "join(left_on)", &cols);
                    }
                    if self.trained_vars.contains(other) {
                        self.error(
                            ErrorKind::UndeclaredVariable(other.to_string()),
                            if is_korean() {
                                format!("join() 대상 변수 '{}' 은 학습된 모델입니다.", other)
                            } else {
                                format!("join() target variable '{}' is a trained model.", other)
                            },
                        );
                    } else if let Some(var) = self.vars.get(other) {
                        let right_cols: HashMap<String, ColType> = var.columns.clone();
                        for k in right_on {
                            self.check_column(k, "join(right_on)", &right_cols);
                        }
                    } else {
                        self.error(
                            ErrorKind::UndeclaredVariable(other.to_string()),
                            if is_korean() {
                                format!("join() 대상 변수 '{}' 이(가) 선언되지 않았습니다.", other)
                            } else {
                                format!("join() target variable '{}' is not declared.", other)
                            },
                        );
                    }
                    steps.push(ir::Step::Data(ir::DataOp::Join {
                        other: other.clone(),
                        left_on: left_on.clone(),
                        right_on: right_on.clone(),
                        how: how.clone(),
                    }));
                }
                PipelineOp::WithColumn { name, expr } => {
                    self.check_expr_columns(expr, &cols);
                    self.check_division_by_zero(expr);
                    let typed = type_expr(expr, &cols);
                    cols.insert(name.clone(), ir_col_type_to_checker(&typed.ty));
                    steps.push(ir::Step::Data(ir::DataOp::WithColumn {
                        name: name.clone(),
                        expr: typed,
                    }));
                }
                PipelineOp::Chart(config) => {
                    self.check_chart(config, &cols);
                    steps.push(ir::Step::Side(ir::SideOp::Chart(config.clone())));
                }
                PipelineOp::Cast { col, to_type } => {
                    if !matches!(to_type.as_str(), "float" | "int" | "str" | "bool") {
                        self.error(
                            ErrorKind::Other("알 수 없는 cast 타입".to_string()),
                            if is_korean() {
                                format!(
                                    "cast(\"{}\", \"{}\") : 알 수 없는 타입 '{}'. 지원 타입: \"float\", \"int\", \"str\", \"bool\"",
                                    col, to_type, to_type
                                )
                            } else {
                                format!(
                                    "cast(\"{}\", \"{}\") : unknown type '{}'. Supported types: \"float\", \"int\", \"str\", \"bool\"",
                                    col, to_type, to_type
                                )
                            },
                        );
                    }
                    self.check_column(col, "cast", &cols);
                    if let Some(t) = cols.get(col).cloned() {
                        let nt = normalize_type(to_type);
                        cols.insert(col.clone(), ColType::new(nt, t.option));
                    }
                    steps.push(ir::Step::Data(ir::DataOp::Cast {
                        col: col.clone(),
                        to: to_type.clone(),
                    }));
                }
                PipelineOp::Rename { old_name, new_name } => {
                    self.check_column(old_name, "rename", &cols);
                    if let Some(t) = cols.remove(old_name) {
                        cols.insert(new_name.clone(), t);
                    }
                    steps.push(ir::Step::Data(ir::DataOp::Rename {
                        old: old_name.clone(),
                        new: new_name.clone(),
                    }));
                }
                PipelineOp::Replace { col, from, to } => {
                    self.check_column(col, "replace", &cols);
                    steps.push(ir::Step::Data(ir::DataOp::Replace {
                        col: col.clone(),
                        from: from.clone(),
                        to: to.clone(),
                    }));
                }

                // ── v0.6 withDp — 인수 범위는 파서가 검증, 숫자형 컬럼 존재는 런타임이 검증 ──
                PipelineOp::WithDp(args) => {
                    // 노이즈 주입 후 숫자형 컬럼은 float 로 승격된다
                    for (_, t) in cols.iter_mut() {
                        if t.is_numeric() {
                            *t = ColType::new("float", t.option);
                        }
                    }
                    if args.epsilon > 10.0 {
                        self.warning(if is_korean() {
                            format!(
                                "withDp(epsilon: {}) : ε 이 10을 초과하면 프라이버시 보호 효과가 사실상 없습니다. 1.0 이하 권장.",
                                args.epsilon
                            )
                        } else {
                            format!(
                                "withDp(epsilon: {}) : ε above 10 provides effectively no privacy protection. 1.0 or less is recommended.",
                                args.epsilon
                            )
                        });
                    }
                    steps.push(ir::Step::Side(ir::SideOp::WithDp(args.clone())));
                }
            }
        }

        if let Some(g) = pending_group {
            self.error(
                ErrorKind::Other("groupBy 후 집계 누락".to_string()),
                if is_korean() {
                    format!(
                        "groupBy(\"{}\") 뒤에 sum/mean/min/max/count 등 집계 연산이 없습니다. 파이프라인이 그룹된 상태로 종료될 수 없습니다.",
                        g
                    )
                } else {
                    format!(
                        "groupBy(\"{}\") is missing an aggregate (sum/mean/min/max/count) after it. A pipeline cannot end while still grouped.",
                        g
                    )
                },
            );
        }

        let node = ir::PipelineNode {
            id: self.ir.pipelines.len(),
            name: binding.map(|s| s.to_string()),
            source: ir_source,
            input_schema,
            output_schema: ir_schema_from_map(&cols),
            steps,
            yields_model,
        };
        self.ir.pipelines.push(node);

        if let Some(binding) = binding {
            self.vars
                .insert(binding.to_string(), VarInfo { columns: cols });
        }
        yields_model
    }

    fn check_agg_column(&mut self, col: &str, cols: &HashMap<String, ColType>) {
        match cols.get(col) {
            Some(t) if !t.is_numeric() => {
                self.warning(if is_korean() {
                    format!(
                        "집계 컬럼 '{}' 이 숫자형이 아닙니다. 집계(sum/mean/min/max)는 숫자형 컬럼에만 의미가 있습니다.",
                        col
                    )
                } else {
                    format!(
                        "Aggregate column '{}' is not numeric. Aggregates (sum/mean/min/max) only make sense on numeric columns.",
                        col
                    )
                });
            }
            None => {
                self.column_missing_with_available(
                    col,
                    if is_korean() { "집계" } else { "aggregate" },
                    cols,
                );
            }
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
                self.warning(if is_korean() {
                    format!(
                        "fillNull(\"{}\", <문자열>) : 숫자형 컬럼 '{}' 을 문자열로 채우면 타입이 바뀔 수 있습니다.",
                        col, col
                    )
                } else {
                    format!(
                        "fillNull(\"{}\", <string>) : filling numeric column '{}' with a string may change its type.",
                        col, col
                    )
                });
            } else if !is_str_fill && !t.is_numeric() && t.name != "unknown" {
                self.warning(if is_korean() {
                    format!(
                        "fillNull(\"{}\", <숫자>) : 문자열 컬럼 '{}' 에 숫자 값을 채우면 타입이 바뀔 수 있습니다.",
                        col, col
                    )
                } else {
                    format!(
                        "fillNull(\"{}\", <number>) : filling string column '{}' with a number may change its type.",
                        col, col
                    )
                });
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
                self.check_column(
                    c,
                    if is_korean() {
                        "표현식"
                    } else {
                        "expression"
                    },
                    cols,
                );
            }
            Expr::BinOp { lhs, rhs, .. } => {
                self.check_expr_columns(lhs, cols);
                self.check_expr_columns(rhs, cols);
            }
            _ => {}
        }
    }

    /// Div 연산의 분모가 리터럴 0 인 경우를 정적으로 감지해 경고를 남긴다.
    /// (데이터 의존적인 "컬럼에 0 존재" 는 런타임에만 판별 가능하므로,
    ///  여기서는 컴파일 타임에 확정 가능한 리터럴 0 분모만 처리한다.)
    fn check_division_by_zero(&mut self, expr: &Expr) {
        match expr {
            Expr::BinOp { lhs, op, rhs } => {
                if *op == BinOpKind::Div && is_zero_literal(rhs.as_ref()) {
                    let message = if is_korean() {
                        "0으로 나누기 감지 — DivisionByZero. 필터/치환으로 분모 0 을 처리하세요."
                    } else {
                        "Division by zero detected — DivisionByZero. Handle zero denominators with a filter or replacement."
                    };
                    let span = self.resolve_span(message);
                    self.warnings.push(CompileError::new(
                        ErrorKind::DivisionByZero {
                            col: "(literal)".to_string(),
                            row_count: 0,
                            expr_context: format_expr_display(expr),
                        },
                        span,
                        message,
                    ));
                }
                self.check_division_by_zero(lhs);
                self.check_division_by_zero(rhs);
            }
            _ => {}
        }
    }

    fn check_column(&mut self, col: &str, ctx: &str, cols: &HashMap<String, ColType>) {
        if !cols.contains_key(col) {
            self.column_missing_with_available(col, ctx, cols);
        }
    }

    fn column_missing(&mut self, col: &str, ctx: &str) {
        self.column_missing_with_available(col, ctx, &HashMap::new())
    }

    /// 컬럼 존재 검사 실패 시 did-you-mean 힌트를 포함한 오류를 남긴다.
    /// `cols` 는 현재 파이프라인의 스키마 컬럼(비어 있으면 다른 변수들의 컬럼으로 폴백).
    fn column_missing_with_available(
        &mut self,
        col: &str,
        ctx: &str,
        cols: &HashMap<String, ColType>,
    ) {
        let mut available: Vec<String> = cols.keys().cloned().collect();
        if available.is_empty() {
            available = self
                .vars
                .values()
                .flat_map(|v| v.columns.keys().cloned())
                .collect();
        }
        available.sort();
        available.dedup();
        self.error(
            ErrorKind::SafeLoadViolation {
                col: col.to_string(),
                schema: ctx.to_string(),
                available,
            },
            if is_korean() {
                format!("{}: 스키마에 '{}' 컬럼이 존재하지 않습니다.", ctx, col)
            } else {
                format!("{}: column '{}' does not exist in the schema.", ctx, col)
            },
        );
    }

    fn error(&mut self, kind: ErrorKind, message: impl Into<String>) {
        let message = message.into();
        let span = self.resolve_span(&message);
        self.errors.push(CompileError::new(kind, span, message));
    }

    fn warning(&mut self, message: impl Into<String>) {
        let message = message.into();
        let span = self.resolve_span(&message);
        self.warnings.push(CompileError::new(
            ErrorKind::Other("경고".to_string()),
            span,
            message,
        ));
    }

    /// 진단 메시지에서 피식별자(첫 번째 `'...'`)를 뽑아 현재 명령문의 토큰에서
    /// 해당 식별자의 Span(라인/컬럼)을 찾아 반환한다.
    ///
    /// - stmt_tokens 가 없으면(내부/런타임 경유) Span(0,0) 반환
    /// - 식별자를 찾지 못하면 명령문 시작 토큰의 Span 으로 폴백
    fn resolve_span(&self, message: &str) -> crate::Span {
        use crate::TokenKind;

        let Some(stmts) = &self.stmt_tokens else {
            return crate::Span::new(0, 0);
        };
        let Some(tokens) = stmts.get(self.cur_stmt) else {
            return crate::Span::new(0, 0);
        };

        // 첫 번째 '...' 안의 식별자 추출
        let name = extract_quoted(message);

        // 식별자 토큰 매칭 (Ident 또는 예약 키워드)
        let matched = tokens.iter().find(|t| match &t.kind {
            TokenKind::Ident(n) => Some(n.as_str()) == name.as_deref(),
            other => {
                format!("{:?}", other).to_lowercase()
                    == name
                        .as_deref()
                        .map(|s| s.to_lowercase())
                        .unwrap_or_default()
            }
        });

        match matched {
            Some(t) => t.span.clone(),
            None => tokens
                .first()
                .map(|t| t.span.clone())
                .unwrap_or_else(|| crate::Span::new(0, 0)),
        }
    }
}

/// 메시지의 첫 번째 `'...'` 사이 문자열을 반환한다. 없으면 None.
fn extract_quoted(message: &str) -> Option<String> {
    let start = message.find('\'')? + 1;
    let rest = &message[start..];
    let end = rest.find('\'')?;
    Some(rest[..end].to_string())
}

/// 리터럴이 0 인지 확인한다 (DivisionByZero 정적 검사용).
fn is_zero_literal(expr: &Expr) -> bool {
    match expr {
        Expr::IntLit(0) => true,
        Expr::FloatLit(f) => *f == 0.0,
        _ => false,
    }
}

/// 표현식을 사람이 읽기 좋은 문자열로 변환한다 (DivisionByZero 경고 컨텍스트).
fn format_expr_display(expr: &Expr) -> String {
    match expr {
        Expr::Ident(s) => format!("col(\"{}\")", s),
        Expr::StringLit(s) => format!("\"{}\"", s),
        Expr::IntLit(n) => n.to_string(),
        Expr::FloatLit(f) => f.to_string(),
        Expr::BoolLit(b) => b.to_string(),
        Expr::BinOp { lhs, op, rhs } => {
            let op_str = match op {
                BinOpKind::Add => "+",
                BinOpKind::Sub => "-",
                BinOpKind::Mul => "*",
                BinOpKind::Div => "/",
                BinOpKind::Eq => "==",
                BinOpKind::NotEq => "!=",
                BinOpKind::Lt => "<",
                BinOpKind::Gt => ">",
                BinOpKind::LtEq => "<=",
                BinOpKind::GtEq => ">=",
            };
            format!(
                "{} {} {}",
                format_expr_display(lhs),
                op_str,
                format_expr_display(rhs)
            )
        }
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

/// 표현식의 결과 타입을 추론해 타입이 붙은 IR 표현식을 생성한다.
/// (withColumn / filter 등에 사용 — 단일 추론 지점.)
fn type_expr(expr: &Expr, cols: &HashMap<String, ColType>) -> ir::TypedExpr {
    match expr {
        Expr::Ident(c) => {
            let ty = cols
                .get(c)
                .map(to_ir_col_type)
                .unwrap_or(ir::ColType::Nullable(Box::new(ir::ColType::Unknown)));
            ir::TypedExpr::new(ir::TypedExprKind::Column(c.clone()), ty)
        }
        Expr::IntLit(n) => ir::TypedExpr::new(ir::TypedExprKind::Int(*n), ir::ColType::Int),
        Expr::FloatLit(f) => ir::TypedExpr::new(ir::TypedExprKind::Float(*f), ir::ColType::Float),
        Expr::BoolLit(b) => ir::TypedExpr::new(ir::TypedExprKind::Bool(*b), ir::ColType::Bool),
        Expr::StringLit(s) => {
            ir::TypedExpr::new(ir::TypedExprKind::Str(s.clone()), ir::ColType::String)
        }
        Expr::BinOp { lhs, op, rhs } => {
            let l = type_expr(lhs, cols);
            let r = type_expr(rhs, cols);
            let ty = match op {
                BinOpKind::Eq
                | BinOpKind::NotEq
                | BinOpKind::Lt
                | BinOpKind::Gt
                | BinOpKind::LtEq
                | BinOpKind::GtEq => ir::ColType::Bool,
                BinOpKind::Add | BinOpKind::Sub | BinOpKind::Mul | BinOpKind::Div => {
                    ir::ColType::Float
                }
            };
            ir::TypedExpr::new(
                ir::TypedExprKind::BinOp {
                    op: op.clone(),
                    lhs: Box::new(l),
                    rhs: Box::new(r),
                },
                ty,
            )
        }
    }
}

/// checker 로컬 ColType → IR ColType 변환.
fn to_ir_col_type(ct: &ColType) -> ir::ColType {
    let base = match ct.name.as_str() {
        "string" => ir::ColType::String,
        "int" => ir::ColType::Int,
        "float" => ir::ColType::Float,
        "bool" => ir::ColType::Bool,
        _ => ir::ColType::Unknown,
    };
    if ct.option {
        ir::ColType::Nullable(Box::new(base))
    } else {
        base
    }
}

/// IR ColType → checker 로컬 ColType 변환.
fn ir_col_type_to_checker(ty: &ir::ColType) -> ColType {
    match ty {
        ir::ColType::Nullable(inner) => ColType::new(inner.name(), true),
        other => ColType::new(other.name(), false),
    }
}

/// AST StructField 목록 → IR Schema 변환 (타입 선언용).
fn ir_schema_from_fields(fields: &[StructField]) -> ir::Schema {
    ir::Schema::new(
        fields
            .iter()
            .map(|f| {
                ir::SchemaField::new(
                    f.name.clone(),
                    to_ir_col_type(&col_type_of_field(&f.field_type)),
                )
            })
            .collect(),
    )
}

/// checker 컬럼 맵 → IR Schema 변환 (파이프라인 입출력용, 이름순 정렬로 결정적).
fn ir_schema_from_map(cols: &HashMap<String, ColType>) -> ir::Schema {
    let mut fields: Vec<ir::SchemaField> = cols
        .iter()
        .map(|(k, v)| ir::SchemaField::new(k.clone(), to_ir_col_type(v)))
        .collect();
    fields.sort_by(|a, b| a.name.cmp(&b.name));
    ir::Schema::new(fields)
}

/// fillNull 채우기 값 → IR FillValue 변환.
fn fill_value_ir(value: &FillNullValue) -> ir::FillValue {
    match value {
        FillNullValue::Int(n) => ir::FillValue::Int(*n),
        FillNullValue::Float(f) => ir::FillValue::Float(*f),
        FillNullValue::Str(s) => ir::FillValue::Str(s.clone()),
        FillNullValue::Mean => ir::FillValue::Mean,
        FillNullValue::Median => ir::FillValue::Median,
        FillNullValue::Zero => ir::FillValue::Zero,
    }
}

/// 집계 연산자 → (AggKind, 컬럼명) 추출.
fn aggregate_kind_col(op: &PipelineOp) -> (ir::AggKind, String) {
    match op {
        PipelineOp::Sum(c) => (ir::AggKind::Sum, c.clone()),
        PipelineOp::Mean(c) => (ir::AggKind::Mean, c.clone()),
        PipelineOp::Min(c) => (ir::AggKind::Min, c.clone()),
        PipelineOp::Max(c) => (ir::AggKind::Max, c.clone()),
        PipelineOp::Median(c) => (ir::AggKind::Median, c.clone()),
        PipelineOp::Variance(c) => (ir::AggKind::Variance, c.clone()),
        PipelineOp::Std(c) => (ir::AggKind::Std, c.clone()),
        _ => unreachable!("집계 연산자만 전달"),
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
    use xazz_core::i18n::{Lang, reset_lang, set_lang};

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
        set_lang(Lang::Ko);
        let r = check(
            "type X = { station: string, pm10: float };
             v p = load(\"x.csv\") :: X |> groupBy(\"station\") |> sum(\"station\");",
        );
        reset_lang();
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

    // ── Span(위치) 해석 검증 ──────────────────────────────────────────────────

    #[test]
    fn source_diagnostics_carry_line_numbers() {
        let src = "type X = { a: string, val: float };\nv bad = load(\"x.csv\") :: X |> filter(missing_col > 1);\n";
        let (parse, r) = check_source(src);
        assert!(parse.is_ok(), "파싱 실패: {:?}", parse);
        assert!(r.is_err(), "오류가 있어야 함");
        let err = &r.errors[0];
        assert!(
            err.span.line >= 2,
            "오류 Span 라인이 2행이어야 함(실제 {}): {}",
            err.span.line,
            err.message
        );
        assert!(err.span.line > 0, "Span 이 0,0 이면 안 됨");
    }

    #[test]
    fn source_diagnostics_point_to_offending_identifier() {
        // missing_col 은 두 번째 명령문에만 있으므로 그쪽 Span(라인 2)을 가리켜야 함
        let src = "type X = { missing_col: float };\nv bad = load(\"x.csv\") :: X |> filter(other_col > 1);\n";
        let (parse, r) = check_source(src);
        assert!(parse.is_ok());
        let err = r
            .errors
            .iter()
            .find(|e| e.message.contains("other_col"))
            .unwrap();
        assert!(
            err.span.line == 2,
            "오타 컬럼이 라인 2에 위치해야 함(실제 {}): {}",
            err.span.line,
            err.message
        );
    }

    #[test]
    fn program_check_has_no_span() {
        // check_program(내부 경로)는 Span 이 없어 0,0 을 유지
        let r = check(
            "type X = { a: string };
             v bad = load(\"x.csv\") :: X |> filter(nope > 1);",
        );
        let err = &r.errors[0];
        assert_eq!(err.span.line, 0);
        assert_eq!(err.span.col, 0);
    }

    #[test]
    fn fillnull_on_non_nullable_column_is_a_type_error() {
        let r = check(
            "type X = { temp: float };
             v p = load(\"x.csv\") :: X |> fillNull(\"temp\", strategy: \"mean\");",
        );
        assert!(
            r.errors.iter().any(|e| e.message.contains("Option")),
            "non-Option 컬럼에 fillNull 은 스키마 수정을 제안하는 오류여야 함: {:?}",
            r.errors
                .iter()
                .map(|e| e.message.clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fillnull_on_nullable_column_is_ok() {
        let r = check(
            "type X = { temp: Option<float> };
             v p = load(\"x.csv\") :: X |> fillNull(\"temp\", strategy: \"mean\");",
        );
        assert!(
            r.errors.is_empty(),
            "Option 컬럼에 fillNull 은 허용: {:?}",
            r.errors
        );
    }

    // ── IR 생성 검증 ──────────────────────────────────────────────────────────

    fn analyze(src: &str) -> (CheckResult, ir::TypedProgram) {
        let tokens = Lexer::new(src).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        analyze_program(&program)
    }

    #[test]
    fn analyze_builds_types_models_and_pipelines() {
        let src = "type X = { a: float, b: string, y: float };
             model M { Dense(4) -> ReLU() -> Dense(1) }
             v data = load(\"x.csv\") :: X |> filter(a > 10) |> select([a, y]);
             v agged = data |> groupBy(\"a\") |> sum(\"a\");";
        let (check, ir) = analyze(src);
        assert!(check.is_ok(), "오류: {:?}", check.errors);

        assert_eq!(ir.types.len(), 1);
        assert_eq!(ir.types[0].name, "X");
        assert_eq!(ir.models.len(), 1);
        assert_eq!(ir.models[0].layers.len(), 3);
        assert_eq!(ir.pipelines.len(), 2);

        // 파이프라인 0: filter → select
        let p0 = &ir.pipelines[0];
        assert_eq!(p0.name.as_deref(), Some("data"));
        assert!(matches!(p0.source, ir::Source::Load { .. }));
        assert!(matches!(p0.steps[0], ir::Step::Data(ir::DataOp::Filter(_))));
        assert_eq!(
            p0.steps[0],
            ir::Step::Data(ir::DataOp::Filter(ir::TypedExpr::new(
                ir::TypedExprKind::BinOp {
                    op: crate::BinOpKind::Gt,
                    lhs: Box::new(ir::TypedExpr::new(
                        ir::TypedExprKind::Column("a".into()),
                        ir::ColType::Float,
                    )),
                    rhs: Box::new(ir::TypedExpr::new(
                        ir::TypedExprKind::Int(10),
                        ir::ColType::Int,
                    )),
                },
                ir::ColType::Bool,
            ))),
        );
        assert_eq!(
            p0.steps[1],
            ir::Step::Data(ir::DataOp::Select(vec!["a".into(), "y".into()]))
        );
        assert_eq!(p0.output_schema.names(), vec!["a", "y"]);

        // 파이프라인 1: groupBy → sum (집계는 GroupBy + Aggregate 두 스텝으로 보존)
        let p1 = &ir.pipelines[1];
        assert!(matches!(p1.source, ir::Source::Ref { .. }));
        assert_eq!(p1.steps[0], ir::Step::Data(ir::DataOp::GroupBy("a".into())));
        assert_eq!(
            p1.steps[1],
            ir::Step::Data(ir::DataOp::Aggregate {
                kind: ir::AggKind::Sum,
                col: "a".into(),
            })
        );
    }

    #[test]
    fn analyze_produces_ml_steps_and_train_stmt_node() {
        let src = "type X = { a: float, y: float };
             model M { Dense(4) }
             v data = load(\"x.csv\") :: X;
             v trained = data |> train(M, target: \"y\", epochs: 5);
             v pred = data |> predict(trained, as: \"p\");
             run data |> train(M, target: \"y\");";
        let (check, ir) = analyze(src);
        assert!(check.is_ok(), "오류: {:?}", check.errors);

        // trained: source Ref(data), ML(Train), yields_model=true
        let trained = &ir.pipelines[1];
        assert_eq!(trained.name.as_deref(), Some("trained"));
        assert!(matches!(
            trained.steps[0],
            ir::Step::ML(ir::MLOp::Train { .. })
        ));
        assert!(trained.yields_model);

        // pred: ML(Predict)
        let pred = &ir.pipelines[2];
        assert!(matches!(
            pred.steps[0],
            ir::Step::ML(ir::MLOp::Predict { .. })
        ));

        // TrainStmt: name None, ML(Train), yields_model=false
        let stmt = &ir.pipelines[3];
        assert_eq!(stmt.name, None);
        assert!(matches!(
            stmt.steps[0],
            ir::Step::ML(ir::MLOp::Train { .. })
        ));
        assert!(!stmt.yields_model);
    }

    #[test]
    fn analyze_orders_side_ops_between_data_ops() {
        let src = "type X = { a: float };
             v p = load(\"x.csv\") :: X |> filter(a > 0) |> withDp(epsilon: 1.0) |> select([a]);";
        let (_, ir) = analyze(src);
        let steps = &ir.pipelines[0].steps;
        assert!(matches!(steps[0], ir::Step::Data(ir::DataOp::Filter(_))));
        assert!(matches!(steps[1], ir::Step::Side(ir::SideOp::WithDp(_))));
        assert!(matches!(steps[2], ir::Step::Data(ir::DataOp::Select(_))));
    }

    #[test]
    fn compile_ir_returns_program_and_ir() {
        let src = "type X = { a: float };
             v p = load(\"x.csv\") :: X |> select([a]);";
        let (parsed, check) = compile_ir(src);
        assert!(parsed.is_ok());
        let (program, ir) = parsed.unwrap();
        assert_eq!(program.stmts.len(), 2);
        assert_eq!(ir.pipelines.len(), 1);
        assert!(check.is_ok());
    }
}
