/// xazzLang - 재귀 하강 파서 (v0.16 완전 구현)
///
/// BNF:
///   program        = stmt* EOF
///   stmt           = type_decl | var_stmt
///   type_decl      = "type" IDENT "=" "{" field_list "}" ";"?
///   field_list     = (field ("," field)* ","?)?
///   field          = IDENT ":" type_name
///   type_name      = "Option" "<" IDENT ">" | IDENT
///   var_stmt       = "mut"? "v" IDENT "=" pipeline_expr ";"?
///   pipeline_expr  = (load_expr | var_ref_expr) ("|>" pipeline_op)*
///   load_expr      = "load" "(" STRING_LIT ")" "::" IDENT
///   var_ref_expr   = IDENT  (기존 변수 참조)
///   pipeline_op    = "filter" "(" expr ")"
///                  | "select" "(" "[" ident_list "]" ")"
///                  | "count" ("(" STRING_LIT ")")?
///                  | "groupBy" "(" STRING_LIT ")"
///                  | "sum"     "(" STRING_LIT ")"
///                  | "mean"    "(" STRING_LIT ")"
///                  | "min"     "(" STRING_LIT ")"
///                  | "max"     "(" STRING_LIT ")"
///                  | "orderBy" "(" STRING_LIT ("," "desc" ":" BOOL)? ")"
///                  | "take"    "(" INT_LIT ")"
///                  | "dropNull" "(" STRING_LIT ")"
///                  | "fillNull" "(" STRING_LIT "," (INT_LIT | FLOAT_LIT | STRING_LIT) ")"
///                  | "join" "(" IDENT "," "on" ":" (STRING_LIT | "[" str_list "]")
///                             ("," "how" ":" STRING_LIT)? ")"
///                  | "withColumn" "(" STRING_LIT "," expr ")"
///   expr           = additive (cmp_op additive)?
///   additive       = multiplicative (('+' | '-') multiplicative)*
///   multiplicative = primary (('*' | '/') primary)*
///   primary        = "col" "(" STRING_LIT ")"
///                  | IDENT | INT_LIT | FLOAT_LIT | STRING_LIT
///                  | "true" | "false"
///                  | "(" expr ")"
///   cmp_op         = "==" | "!=" | "<" | ">" | "<=" | ">="
///
/// [v0.16 변경사항]
///   - col("col_name") 표현식 지원: col("x") → Expr::Ident("x")
///   - true / false 불리언 리터럴 지원
///   - Count(None) / Count(Some(col)) 구분
///   - 9종 신규 파이프라인 연산자 파싱
///   - join() 연산자: on: / how: 명명 인수 파싱
///   - withColumn() 연산자 파싱
///   - 산술 표현식 우선순위: * / > + - > 비교 연산자
use crate::ast::{
    BinOpKind, ChartConfig, ChartType, DpArgs, DpMechanism, Expr, FillNullValue, JoinHow,
    LayerKind, PipelineOp, PipelineSource, Program, Stmt, StructField, TrainConfig,
};
use crate::error::{CompileError, CompileResult, ErrorKind};
use crate::token::{Span, Token, TokenKind};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, pos: 0 }
    }

    // ── 내부 헬퍼 ─────────────────────────────────────────────────────────────

    fn current_kind(&self) -> TokenKind {
        self.tokens
            .get(self.pos)
            .map(|t| t.kind.clone())
            .unwrap_or(TokenKind::Eof)
    }

    fn current_span(&self) -> Span {
        self.tokens
            .get(self.pos)
            .map(|t| t.span.clone())
            .unwrap_or(Span::new(0, 0))
    }

    fn advance(&mut self) {
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
    }

    fn expect(&mut self, expected: &TokenKind) -> CompileResult<Span> {
        let kind = self.current_kind();
        let span = self.current_span();
        if kind == *expected {
            self.advance();
            Ok(span)
        } else {
            Err(CompileError::new(
                ErrorKind::ExpectedToken(format!("{:?}", expected)),
                span.clone(),
                format!("예상 토큰 {:?} 없음, 실제: {:?}", expected, kind),
            ))
        }
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.current_kind() == *kind {
            self.advance();
            true
        } else {
            false
        }
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.tokens.len().saturating_sub(1)
            || matches!(self.current_kind(), TokenKind::Eof)
    }

    // ── 최상위 ────────────────────────────────────────────────────────────────

    pub fn parse(&mut self) -> CompileResult<Program> {
        let mut program = Program::new();
        while !self.is_eof() {
            program.stmts.push(self.parse_stmt()?);
        }
        Ok(program)
    }

    // ── Stmt ──────────────────────────────────────────────────────────────────

    fn parse_stmt(&mut self) -> CompileResult<Stmt> {
        match self.current_kind() {
            TokenKind::Type => self.parse_type_decl(),
            TokenKind::V | TokenKind::Mut => self.parse_var_stmt(),
            TokenKind::Model => self.parse_model_decl(),
            TokenKind::Run => self.parse_train_stmt(),
            TokenKind::Ident(_) if self.peek_pipeline() => {
                // expression statement: ident |> op1 |> op2 ...
                self.parse_expr_stmt()
            }
            other => Err(CompileError::new(
                ErrorKind::UnexpectedToken(format!("{:?}", other)),
                self.current_span(),
                format!("구문 시작 불가 토큰: {:?}", other),
            )),
        }
    }

    // ── ModelDecl ─────────────────────────────────────────────────────────────
    // model_decl = "model" IDENT "{" layer_chain "}" ";"?
    // layer_chain = layer ("->" layer)*
    // layer = IDENT "(" NUMBER? ")"

    fn parse_model_decl(&mut self) -> CompileResult<Stmt> {
        self.expect(&TokenKind::Model)?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LBrace)?;

        let mut layers = Vec::new();
        loop {
            if matches!(self.current_kind(), TokenKind::RBrace | TokenKind::Eof) {
                break;
            }
            layers.push(self.parse_layer()?);
            if !self.eat(&TokenKind::Arrow) {
                break;
            }
            if matches!(self.current_kind(), TokenKind::RBrace) {
                break;
            }
        }

        self.expect(&TokenKind::RBrace)?;
        self.eat(&TokenKind::Semicolon);
        Ok(Stmt::ModelDecl { name, layers })
    }

    fn parse_layer(&mut self) -> CompileResult<LayerKind> {
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LParen)?;

        let layer = match name.as_str() {
            "Dense" => {
                let units = self.expect_number()? as usize;
                LayerKind::Dense(units)
            }
            "Dropout" => {
                let rate = self.expect_float()?;
                LayerKind::Dropout(rate)
            }
            "ReLU" => LayerKind::ReLU,
            "Sigmoid" => LayerKind::Sigmoid,
            "Tanh" => LayerKind::Tanh,
            "Softmax" => LayerKind::Softmax,
            "BatchNorm" => LayerKind::BatchNorm,
            other => {
                return Err(CompileError::new(
                    ErrorKind::UnexpectedToken(other.into()),
                    self.current_span(),
                    format!(
                        "알 수 없는 레이어 타입: '{}'. 지원: Dense, ReLU, Sigmoid, Tanh, Softmax, Dropout, BatchNorm",
                        other
                    ),
                ));
            }
        };

        self.expect(&TokenKind::RParen)?;
        Ok(layer)
    }

    fn expect_number(&mut self) -> CompileResult<i64> {
        match self.current_kind() {
            TokenKind::IntLit(n) => {
                self.advance();
                Ok(n)
            }
            other => Err(CompileError::new(
                ErrorKind::ExpectedToken("IntLit".into()),
                self.current_span(),
                format!("정수가 필요합니다. 실제: {:?}", other),
            )),
        }
    }

    fn expect_float(&mut self) -> CompileResult<f64> {
        match self.current_kind() {
            TokenKind::FloatLit(f) => {
                self.advance();
                Ok(f)
            }
            TokenKind::IntLit(n) => {
                self.advance();
                Ok(n as f64)
            }
            other => Err(CompileError::new(
                ErrorKind::ExpectedToken("FloatLit".into()),
                self.current_span(),
                format!("실수가 필요합니다. 실제: {:?}", other),
            )),
        }
    }

    // ── TrainStmt ─────────────────────────────────────────────────────────────
    // train_stmt = "run" IDENT "|>" "train" "(" IDENT train_args ")" ";"?
    // train_args = ("," named_arg)*
    // named_arg = IDENT ":" literal

    fn parse_train_stmt(&mut self) -> CompileResult<Stmt> {
        self.expect(&TokenKind::Run)?;
        let source_var = self.expect_ident()?;
        self.expect(&TokenKind::Pipeline)?;
        self.expect(&TokenKind::Train)?;
        self.expect(&TokenKind::LParen)?;

        let model_name = self.expect_ident()?;
        let config = self.parse_train_args()?;

        self.expect(&TokenKind::RParen)?;
        self.eat(&TokenKind::Semicolon);
        Ok(Stmt::TrainStmt {
            source_var,
            model_name,
            config,
        })
    }

    /// train() 명명 인자를 파싱한다 (model_name 소비 후 호출).
    /// train_args = ("," named_arg)*
    /// named_arg = ("target" | "epochs" | "lr" | "batch_size" | "validation_split") ":" literal
    fn parse_train_args(&mut self) -> CompileResult<TrainConfig> {
        let mut config = TrainConfig::default();
        while self.eat(&TokenKind::Comma) {
            let arg_name = match self.current_kind() {
                TokenKind::Target => {
                    self.advance();
                    "target".to_string()
                }
                TokenKind::Epochs => {
                    self.advance();
                    "epochs".to_string()
                }
                TokenKind::Lr => {
                    self.advance();
                    "lr".to_string()
                }
                TokenKind::Ident(name) => {
                    let n = name.clone();
                    self.advance();
                    n
                }
                other => {
                    return Err(CompileError::new(
                        ErrorKind::UnexpectedToken(format!("{:?}", other)),
                        self.current_span(),
                        format!("train() 인수 이름이 필요합니다. 실제: {:?}", other),
                    ));
                }
            };
            self.expect(&TokenKind::Colon)?;

            match arg_name.as_str() {
                "target" => {
                    config.target = match self.current_kind() {
                        TokenKind::StringLit(s) => {
                            self.advance();
                            s
                        }
                        other => {
                            return Err(CompileError::new(
                                ErrorKind::ExpectedToken("StringLit".into()),
                                self.current_span(),
                                format!("target은 문자열이어야 합니다. 실제: {:?}", other),
                            ));
                        }
                    };
                }
                "epochs" => {
                    config.epochs = self.expect_number()? as usize;
                }
                "lr" => {
                    config.learning_rate = self.expect_float()?;
                }
                "batch_size" => {
                    config.batch_size = Some(self.expect_number()? as usize);
                }
                "validation_split" => {
                    config.validation_split = Some(self.expect_float()?);
                }
                other => {
                    return Err(CompileError::new(
                        ErrorKind::UnexpectedToken(other.into()),
                        self.current_span(),
                        format!(
                            "알 수 없는 train() 인수: '{}'. 지원: target, epochs, lr, batch_size, validation_split",
                            other
                        ),
                    ));
                }
            }
        }

        Ok(config)
    }

    // ── withDp 인수 파싱 (v0.6) ───────────────────────────────────────────────
    // name ":" value ("," name ":" value)* — epsilon 은 필수, 나머지는 선택.
    fn parse_with_dp_args(&mut self) -> CompileResult<DpArgs> {
        let mut args = DpArgs::default();
        let mut epsilon_given = false;
        let mut first = true;

        while !matches!(self.current_kind(), TokenKind::RParen) {
            if !first {
                self.expect(&TokenKind::Comma)?;
            }
            first = false;

            let arg_name = match self.current_kind() {
                // "seed" 는 v0.22 에서 키워드로 등록되어 Ident 로 오지 않는다
                TokenKind::Seed => {
                    self.advance();
                    "seed".to_string()
                }
                TokenKind::Ident(name) => {
                    let n = name.clone();
                    self.advance();
                    n
                }
                other => {
                    return Err(CompileError::new(
                        ErrorKind::UnexpectedToken(format!("{:?}", other)),
                        self.current_span(),
                        format!("withDp() 인수 이름이 필요합니다. 실제: {:?}", other),
                    ));
                }
            };
            self.expect(&TokenKind::Colon)?;

            match arg_name.as_str() {
                "epsilon" => {
                    args.epsilon = self.expect_float()?;
                    if args.epsilon <= 0.0 {
                        return Err(CompileError::new(
                            ErrorKind::UnexpectedToken("epsilon <= 0".into()),
                            self.current_span(),
                            format!(
                                "withDp() 의 epsilon 은 0보다 커야 합니다. 실제: {}",
                                args.epsilon
                            ),
                        ));
                    }
                    epsilon_given = true;
                }
                "mechanism" => {
                    let m = self.expect_ident_or_str()?;
                    args.mechanism = DpMechanism::from_str(&m).ok_or_else(|| {
                        CompileError::new(
                            ErrorKind::UnexpectedToken(m.clone()),
                            self.current_span(),
                            format!(
                                "withDp() 의 mechanism 은 laplace 또는 gaussian 이어야 합니다. 실제: '{m}'"
                            ),
                        )
                    })?;
                }
                "sensitivity" => {
                    args.sensitivity = self.expect_float()?;
                    if args.sensitivity <= 0.0 {
                        return Err(CompileError::new(
                            ErrorKind::UnexpectedToken("sensitivity <= 0".into()),
                            self.current_span(),
                            format!(
                                "withDp() 의 sensitivity 는 0보다 커야 합니다. 실제: {}",
                                args.sensitivity
                            ),
                        ));
                    }
                }
                "delta" => {
                    args.delta = Some(self.expect_float()?);
                }
                "seed" => {
                    args.seed = Some(self.expect_number()?);
                }
                other => {
                    return Err(CompileError::new(
                        ErrorKind::UnexpectedToken(other.into()),
                        self.current_span(),
                        format!(
                            "알 수 없는 withDp() 인수: '{}'. 지원: epsilon, mechanism, sensitivity, delta, seed",
                            other
                        ),
                    ));
                }
            }
        }

        if !epsilon_given {
            return Err(CompileError::new(
                ErrorKind::ExpectedToken("epsilon".into()),
                self.current_span(),
                "withDp() 에는 epsilon 인수가 필수입니다. 예: withDp(epsilon: 1.0)".to_string(),
            ));
        }

        Ok(args)
    }

    /// 현재 Ident 토큰 뒤에 |> (Pipeline) 토큰이 오는지 확인
    fn peek_pipeline(&self) -> bool {
        self.pos + 1 < self.tokens.len()
            && matches!(self.tokens[self.pos + 1].kind, TokenKind::Pipeline)
    }

    /// expression statement 파싱: ident |> op1 |> op2 ...
    /// 변수에 할당하지 않고 파이프라인만 실행
    fn parse_expr_stmt(&mut self) -> CompileResult<Stmt> {
        let (source, ops) = self.parse_pipeline_expr()?;
        self.eat(&TokenKind::Semicolon);
        Ok(Stmt::ExprStmt { source, ops })
    }

    // ── TypeDecl ──────────────────────────────────────────────────────────────
    // type_decl = "type" IDENT "=" "{" field_list "}" ";"?

    fn parse_type_decl(&mut self) -> CompileResult<Stmt> {
        self.expect(&TokenKind::Type)?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::Assign)?;
        self.expect(&TokenKind::LBrace)?;
        let fields = self.parse_field_list()?;
        self.expect(&TokenKind::RBrace)?;
        self.eat(&TokenKind::Semicolon);
        Ok(Stmt::TypeDecl { name, fields })
    }

    fn parse_field_list(&mut self) -> CompileResult<Vec<StructField>> {
        let mut fields = Vec::new();
        loop {
            if matches!(self.current_kind(), TokenKind::RBrace | TokenKind::Eof) {
                break;
            }
            fields.push(self.parse_field()?);
            if !self.eat(&TokenKind::Comma) {
                break;
            }
            if matches!(self.current_kind(), TokenKind::RBrace) {
                break;
            }
        }
        Ok(fields)
    }

    fn parse_field(&mut self) -> CompileResult<StructField> {
        let name = self.expect_ident()?;
        self.expect(&TokenKind::Colon)?;
        let field_type = self.parse_type_name()?;
        Ok(StructField { name, field_type })
    }

    fn parse_type_name(&mut self) -> CompileResult<String> {
        if matches!(self.current_kind(), TokenKind::OptionKw) {
            let span = self.current_span();
            self.advance();
            if !matches!(self.current_kind(), TokenKind::Lt) {
                return Err(CompileError::new(
                    ErrorKind::ExpectedToken("<".into()),
                    span,
                    "Option 뒤에는 '<' 가 와야 합니다. 예: Option<float>",
                ));
            }
            self.expect(&TokenKind::Lt)?;
            let inner = self.expect_ident()?;
            self.expect(&TokenKind::Gt)?;
            Ok(format!("Option<{}>", inner))
        } else {
            self.expect_ident()
        }
    }

    // ── var_stmt ──────────────────────────────────────────────────────────────
    // var_stmt = "mut"? "v" IDENT "=" pipeline_expr ";"?

    fn parse_var_stmt(&mut self) -> CompileResult<Stmt> {
        let is_mut = self.eat(&TokenKind::Mut);
        self.expect(&TokenKind::V)?;
        let var_name = self.expect_ident()?;
        self.expect(&TokenKind::Assign)?;
        let (source, ops) = self.parse_pipeline_expr()?;
        self.eat(&TokenKind::Semicolon);
        Ok(Stmt::VarDecl {
            var_name,
            is_mut,
            source,
            ops,
        })
    }

    // ── 파이프라인 표현식 ──────────────────────────────────────────────────────
    // pipeline_expr = (load_expr | var_ref_expr) ("|>" pipeline_op)*

    fn parse_pipeline_expr(&mut self) -> CompileResult<(PipelineSource, Vec<PipelineOp>)> {
        // 소스 결정: load(...) 또는 변수 참조
        let source = match self.current_kind() {
            TokenKind::Load => self.parse_load_source()?,
            TokenKind::Ident(name) => {
                let var_name = name.clone();
                self.advance();
                PipelineSource::VarRef(var_name)
            }
            other => {
                return Err(CompileError::new(
                    ErrorKind::UnexpectedToken(format!("{:?}", other)),
                    self.current_span(),
                    format!(
                        "파이프라인은 load(...) 또는 변수 참조로 시작해야 합니다. 실제: {:?}",
                        other
                    ),
                ));
            }
        };

        // |> 연산자 체이닝
        let mut ops: Vec<PipelineOp> = Vec::new();
        while matches!(self.current_kind(), TokenKind::Pipeline) {
            self.advance(); // |> 소비
            ops.push(self.parse_pipeline_op()?);
        }

        Ok((source, ops))
    }

    // load_expr = "load" "(" STRING_LIT ")" "::" IDENT
    fn parse_load_source(&mut self) -> CompileResult<PipelineSource> {
        self.expect(&TokenKind::Load)?;
        self.expect(&TokenKind::LParen)?;

        let file_path = match self.current_kind() {
            TokenKind::StringLit(s) => {
                self.advance();
                s
            }
            other => {
                return Err(CompileError::new(
                    ErrorKind::ExpectedToken("StringLit".into()),
                    self.current_span(),
                    format!(
                        "load() 경로는 문자열 리터럴이어야 합니다. 실제: {:?}",
                        other
                    ),
                ));
            }
        };

        self.expect(&TokenKind::RParen)?;

        // :: 뒤에 스키마 이름
        if !matches!(self.current_kind(), TokenKind::TypeAssign) {
            let span = self.current_span();
            return Err(CompileError::new(
                ErrorKind::ExpectedToken("::".into()),
                span,
                "'::' 뒤에는 스키마 이름이 와야 합니다. 예: load(\"data.csv\") :: AirQuality",
            ));
        }
        self.expect(&TokenKind::TypeAssign)?; // ::
        let schema_name = self.expect_ident()?;

        Ok(PipelineSource::Load {
            file_path,
            schema_name,
        })
    }

    // ── PipelineOp ────────────────────────────────────────────────────────────

    fn parse_pipeline_op(&mut self) -> CompileResult<PipelineOp> {
        match self.current_kind() {
            // ── 기존 연산자 ──────────────────────────────────────────────────
            TokenKind::Filter => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                let expr = self.parse_expr()?;
                self.expect(&TokenKind::RParen)?;
                Ok(PipelineOp::Filter(expr))
            }
            TokenKind::Select => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                self.expect(&TokenKind::LBracket)?;

                let mut cols = Vec::new();
                loop {
                    if matches!(self.current_kind(), TokenKind::RBracket | TokenKind::Eof) {
                        break;
                    }
                    cols.push(self.expect_ident()?);
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                    if matches!(self.current_kind(), TokenKind::RBracket) {
                        break;
                    }
                }

                self.expect(&TokenKind::RBracket)?;
                self.expect(&TokenKind::RParen)?;
                Ok(PipelineOp::Select(cols))
            }
            TokenKind::Count => {
                self.advance();
                // count("col") 형식이면 컬럼 집계 / count() 또는 count (인수 없음) 이면 전체 행 수
                if matches!(self.current_kind(), TokenKind::LParen) {
                    self.advance(); // ( 소비
                    if matches!(self.current_kind(), TokenKind::RParen) {
                        // count() — 빈 괄호 → Count(None)
                        self.advance(); // ) 소비
                        Ok(PipelineOp::Count(None))
                    } else {
                        // count("col") — 컬럼 인수 → Count(Some(col))
                        let col_name = self.expect_string_lit()?;
                        self.expect(&TokenKind::RParen)?;
                        Ok(PipelineOp::Count(Some(col_name)))
                    }
                } else {
                    Ok(PipelineOp::Count(None))
                }
            }

            // ── v0.16 신규 집계 연산자 ────────────────────────────────────────
            TokenKind::GroupBy => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                let col_name = self.expect_string_lit()?;
                self.expect(&TokenKind::RParen)?;
                Ok(PipelineOp::GroupBy(col_name))
            }
            TokenKind::Sum => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                let col_name = self.expect_string_lit()?;
                self.expect(&TokenKind::RParen)?;
                Ok(PipelineOp::Sum(col_name))
            }
            TokenKind::Mean => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                let col_name = self.expect_string_lit()?;
                self.expect(&TokenKind::RParen)?;
                Ok(PipelineOp::Mean(col_name))
            }
            TokenKind::Min => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                let col_name = self.expect_string_lit()?;
                self.expect(&TokenKind::RParen)?;
                Ok(PipelineOp::Min(col_name))
            }
            TokenKind::Max => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                let col_name = self.expect_string_lit()?;
                self.expect(&TokenKind::RParen)?;
                Ok(PipelineOp::Max(col_name))
            }

            // ── v0.16 정렬 / 슬라이싱 ─────────────────────────────────────────
            TokenKind::OrderBy => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                let col_name = self.expect_string_lit()?;
                // 선택적 desc: true/false
                let desc = if matches!(self.current_kind(), TokenKind::Comma) {
                    self.advance(); // , 소비
                    self.expect(&TokenKind::Desc)?;
                    self.expect(&TokenKind::Colon)?;
                    match self.current_kind() {
                        TokenKind::True => {
                            self.advance();
                            true
                        }
                        TokenKind::False => {
                            self.advance();
                            false
                        }
                        other => {
                            return Err(CompileError::new(
                                ErrorKind::ExpectedToken("true or false".into()),
                                self.current_span(),
                                format!(
                                    "orderBy 의 desc: 뒤에는 true 또는 false 가 와야 합니다. 실제: {:?}",
                                    other
                                ),
                            ));
                        }
                    }
                } else {
                    false // 기본값: 오름차순
                };
                self.expect(&TokenKind::RParen)?;
                Ok(PipelineOp::OrderBy {
                    col: col_name,
                    desc,
                })
            }
            TokenKind::Take => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                let n = match self.current_kind() {
                    TokenKind::IntLit(n) => {
                        self.advance();
                        n
                    }
                    other => {
                        return Err(CompileError::new(
                            ErrorKind::ExpectedToken("IntLit".into()),
                            self.current_span(),
                            format!("take() 에는 정수 리터럴이 필요합니다. 실제: {:?}", other),
                        ));
                    }
                };
                // 음수/0은 u32 캐스팅 시 거대한 wrap-around limit 을 만들므로 거부
                if n <= 0 {
                    return Err(CompileError::new(
                        ErrorKind::UnexpectedToken("take n <= 0".into()),
                        self.current_span(),
                        format!("take() 의 n 은 0보다 커야 합니다. 실제: {}", n),
                    ));
                }
                self.expect(&TokenKind::RParen)?;
                Ok(PipelineOp::Take(n))
            }

            // ── v0.16 Null 처리 ────────────────────────────────────────────────
            TokenKind::DropNull => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                let col_name = self.expect_string_lit()?;
                self.expect(&TokenKind::RParen)?;
                Ok(PipelineOp::DropNull(col_name))
            }
            TokenKind::FillNull => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                let col_name = self.expect_string_lit()?;
                self.expect(&TokenKind::Comma)?;
                // 전략 폼: fillNull("col", strategy: "mean")
                if matches!(
                    self.current_kind(),
                    TokenKind::Ident(_) | TokenKind::Strategy
                ) {
                    self.advance(); // 'strategy'
                    self.expect(&TokenKind::Colon)?;
                    let strat = self.expect_string_lit()?.to_lowercase();
                    let value = match strat.as_str() {
                        "mean" => FillNullValue::Mean,
                        "median" => FillNullValue::Median,
                        "zero" | "0" => FillNullValue::Zero,
                        other => {
                            return Err(CompileError::new(
                                ErrorKind::ExpectedToken(
                                    "strategy: \"mean\" | \"median\" | \"zero\"".into(),
                                ),
                                self.current_span(),
                                format!(
                                    "fillNull() 전략 '{other}' 은 지원하지 않습니다. 지원: mean, median, zero"
                                ),
                            ));
                        }
                    };
                    self.expect(&TokenKind::RParen)?;
                    return Ok(PipelineOp::FillNull {
                        col: col_name,
                        value,
                    });
                }
                // 채우기 값: 정수, 부동소수 또는 문자열 리터럴
                let value = match self.current_kind() {
                    TokenKind::IntLit(n) => {
                        self.advance();
                        FillNullValue::Int(n)
                    }
                    TokenKind::FloatLit(f) => {
                        self.advance();
                        FillNullValue::Float(f)
                    }
                    TokenKind::StringLit(s) => {
                        self.advance();
                        FillNullValue::Str(s)
                    }
                    other => {
                        return Err(CompileError::new(
                            ErrorKind::ExpectedToken("number or string".into()),
                            self.current_span(),
                            format!(
                                "fillNull() 채우기 값은 정수, 부동소수 또는 문자열이어야 합니다. 실제: {:?}",
                                other
                            ),
                        ));
                    }
                };
                self.expect(&TokenKind::RParen)?;
                Ok(PipelineOp::FillNull {
                    col: col_name,
                    value,
                })
            }

            // ── v0.16+ / v0.21 join() 연산자 ─────────────────────────────────
            // join(other_var, on: "key")
            // join(other_var, on: ["key1", "key2"], how: "left")
            // join(other_var, left_on: "station", right_on: "adm_name")  (v0.21)
            TokenKind::Join => {
                self.advance();
                self.expect(&TokenKind::LParen)?;

                // other_var: 조인 대상 변수명
                let other = self.expect_ident()?;
                self.expect(&TokenKind::Comma)?;

                let mut on_keys: Vec<String> = Vec::new();
                let mut left_on_keys: Vec<String> = Vec::new();
                let mut right_on_keys: Vec<String> = Vec::new();
                let mut how = JoinHow::Inner;

                // 명명 인수 루프 — on: / left_on: / right_on: / how: 순서 무관
                loop {
                    if matches!(self.current_kind(), TokenKind::RParen | TokenKind::Eof) {
                        break;
                    }

                    match self.current_kind() {
                        TokenKind::On => {
                            self.advance();
                            self.expect(&TokenKind::Colon)?;
                            // on 값: 단일 문자열 또는 [ ... ] 리스트
                            on_keys = if matches!(self.current_kind(), TokenKind::LBracket) {
                                self.advance();
                                let mut keys = Vec::new();
                                loop {
                                    if matches!(
                                        self.current_kind(),
                                        TokenKind::RBracket | TokenKind::Eof
                                    ) {
                                        break;
                                    }
                                    keys.push(self.expect_string_lit()?);
                                    if !self.eat(&TokenKind::Comma) {
                                        break;
                                    }
                                    if matches!(self.current_kind(), TokenKind::RBracket) {
                                        break;
                                    }
                                }
                                self.expect(&TokenKind::RBracket)?;
                                keys
                            } else {
                                vec![self.expect_string_lit()?]
                            };
                        }
                        TokenKind::LeftOn => {
                            self.advance();
                            self.expect(&TokenKind::Colon)?;
                            left_on_keys = if matches!(self.current_kind(), TokenKind::LBracket) {
                                self.advance();
                                let mut keys = Vec::new();
                                loop {
                                    if matches!(
                                        self.current_kind(),
                                        TokenKind::RBracket | TokenKind::Eof
                                    ) {
                                        break;
                                    }
                                    keys.push(self.expect_string_lit()?);
                                    if !self.eat(&TokenKind::Comma) {
                                        break;
                                    }
                                    if matches!(self.current_kind(), TokenKind::RBracket) {
                                        break;
                                    }
                                }
                                self.expect(&TokenKind::RBracket)?;
                                keys
                            } else {
                                vec![self.expect_string_lit()?]
                            };
                        }
                        TokenKind::RightOn => {
                            self.advance();
                            self.expect(&TokenKind::Colon)?;
                            right_on_keys = if matches!(self.current_kind(), TokenKind::LBracket) {
                                self.advance();
                                let mut keys = Vec::new();
                                loop {
                                    if matches!(
                                        self.current_kind(),
                                        TokenKind::RBracket | TokenKind::Eof
                                    ) {
                                        break;
                                    }
                                    keys.push(self.expect_string_lit()?);
                                    if !self.eat(&TokenKind::Comma) {
                                        break;
                                    }
                                    if matches!(self.current_kind(), TokenKind::RBracket) {
                                        break;
                                    }
                                }
                                self.expect(&TokenKind::RBracket)?;
                                keys
                            } else {
                                vec![self.expect_string_lit()?]
                            };
                        }
                        TokenKind::How => {
                            self.advance();
                            self.expect(&TokenKind::Colon)?;
                            let how_str = self.expect_string_lit()?;
                            how = JoinHow::from_str(&how_str).ok_or_else(|| {
                                CompileError::new(
                                    ErrorKind::ExpectedToken("inner|left|outer|cross".into()),
                                    self.current_span(),
                                    format!(
                                        "join how: 는 \"inner\", \"left\", \"outer\", \"cross\" 중 하나여야 합니다. 실제: \"{}\"",
                                        how_str
                                    ),
                                )
                            })?;
                        }
                        _ => break,
                    }

                    self.eat(&TokenKind::Comma);
                }

                // 검증: on 과 left_on/right_on 은 동시 사용 불가
                if !on_keys.is_empty() && (!left_on_keys.is_empty() || !right_on_keys.is_empty()) {
                    return Err(CompileError::new(
                        ErrorKind::UnexpectedToken("on/left_on 동시 사용".into()),
                        self.current_span(),
                        "join 에서 on: 과 left_on:/right_on: 은 동시에 사용할 수 없습니다.",
                    ));
                }
                // left_on / right_on 은 항상 쌍으로 사용
                if !left_on_keys.is_empty() && left_on_keys.len() != right_on_keys.len() {
                    return Err(CompileError::new(
                        ErrorKind::UnexpectedToken("left_on/right_on 개수 불일치".into()),
                        self.current_span(),
                        format!(
                            "join 에서 left_on 과 right_on 의 개수가 일치해야 합니다. left_on: {}, right_on: {}",
                            left_on_keys.len(),
                            right_on_keys.len()
                        ),
                    ));
                }

                let (final_left, final_right) = if !left_on_keys.is_empty() {
                    (left_on_keys, right_on_keys)
                } else {
                    (on_keys.clone(), on_keys)
                };

                self.expect(&TokenKind::RParen)?;
                Ok(PipelineOp::Join {
                    other,
                    left_on: final_left,
                    right_on: final_right,
                    how,
                })
            }

            // ── v0.16+ withColumn() 연산자 ────────────────────────────────────
            // withColumn("new_col", expr)
            TokenKind::WithColumn => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                let col_name = self.expect_string_lit()?;
                self.expect(&TokenKind::Comma)?;
                let expr = self.parse_expr()?;
                self.expect(&TokenKind::RParen)?;
                Ok(PipelineOp::WithColumn {
                    name: col_name,
                    expr,
                })
            }

            // ── v0.19 chart { ... } 시각화 연산자 ────────────────────────────
            // chart {
            //   type: bar|line|pie|scatter
            //   x: col_name
            //   y: col_name
            //   label: col_name    (pie 전용)
            //   value: col_name    (pie 전용)
            //   title: "제목"
            // }
            // 또는 chart({ type: "bar", x: "col", ... }) 형태도 허용
            TokenKind::Chart => {
                self.advance(); // chart 소비
                // chart({ ... }) 형태: LParen 이 오면 소비
                let has_paren = self.eat(&TokenKind::LParen);
                self.expect(&TokenKind::LBrace)?;
                let config = self.parse_chart_config()?;
                self.expect(&TokenKind::RBrace)?;
                if has_paren {
                    self.expect(&TokenKind::RParen)?;
                }
                Ok(PipelineOp::Chart(config))
            }

            // ── v0.20 cast("col", "type") — DSL-레벨 타입 캐스팅 ────────────
            // cast("pm10", "float")   → PipelineOp::Cast { col, to_type }
            // 지원 타입: "float", "int", "str", "bool"
            TokenKind::Cast => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                let col_name = self.expect_string_lit()?;
                self.expect(&TokenKind::Comma)?;
                let to_type = self.expect_string_lit()?;
                self.expect(&TokenKind::RParen)?;
                Ok(PipelineOp::Cast {
                    col: col_name,
                    to_type,
                })
            }

            // ── v0.21 rename("old_name", "new_name") — 컬럼 이름 변경 ───────
            TokenKind::Rename => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                let old_name = self.expect_string_lit()?;
                self.expect(&TokenKind::Comma)?;
                let new_name = self.expect_string_lit()?;
                self.expect(&TokenKind::RParen)?;
                Ok(PipelineOp::Rename { old_name, new_name })
            }

            // ── v0.21 replace("col", "from", "to") — 문자열 치환 ────────────
            TokenKind::Replace => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                let col_name = self.expect_string_lit()?;
                self.expect(&TokenKind::Comma)?;
                let from = self.expect_string_lit()?;
                self.expect(&TokenKind::Comma)?;
                let to = self.expect_string_lit()?;
                self.expect(&TokenKind::RParen)?;
                Ok(PipelineOp::Replace {
                    col: col_name,
                    from,
                    to,
                })
            }

            // ── v0.22 sample(n) / sample(n, seed: 42) — 무작위 샘플링 ───────
            TokenKind::Sample => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                let n = match self.current_kind() {
                    TokenKind::IntLit(n) => {
                        self.advance();
                        n
                    }
                    other => {
                        return Err(CompileError::new(
                            ErrorKind::ExpectedToken("IntLit".into()),
                            self.current_span(),
                            format!("sample() 에는 정수 리터럴이 필요합니다. 실제: {:?}", other),
                        ));
                    }
                };
                if n <= 0 {
                    return Err(CompileError::new(
                        ErrorKind::UnexpectedToken("sample n <= 0".into()),
                        self.current_span(),
                        format!("sample() 의 n 은 0보다 커야 합니다. 실제: {}", n),
                    ));
                }
                let seed = if matches!(self.current_kind(), TokenKind::Comma) {
                    self.advance(); // , 소비
                    self.expect(&TokenKind::Seed)?;
                    self.expect(&TokenKind::Colon)?;
                    match self.current_kind() {
                        TokenKind::IntLit(s) => {
                            self.advance();
                            Some(s)
                        }
                        other => {
                            return Err(CompileError::new(
                                ErrorKind::ExpectedToken("IntLit".into()),
                                self.current_span(),
                                format!(
                                    "sample 의 seed: 뒤에는 정수 리터럴이 필요합니다. 실제: {:?}",
                                    other
                                ),
                            ));
                        }
                    }
                } else {
                    None
                };
                self.expect(&TokenKind::RParen)?;
                Ok(PipelineOp::Sample { n, seed })
            }

            // ── v0.22 median("col") — 중앙값 집계 ────────────────────────────
            TokenKind::Median => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                let col_name = self.expect_string_lit()?;
                self.expect(&TokenKind::RParen)?;
                Ok(PipelineOp::Median(col_name))
            }

            // ── v0.22 variance("col") — 분산 집계 ────────────────────────────
            TokenKind::Variance => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                let col_name = self.expect_string_lit()?;
                self.expect(&TokenKind::RParen)?;
                Ok(PipelineOp::Variance(col_name))
            }

            // ── v0.22 std("col") — 표준편차 집계 ─────────────────────────────
            TokenKind::Std => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                let col_name = self.expect_string_lit()?;
                self.expect(&TokenKind::RParen)?;
                Ok(PipelineOp::Std(col_name))
            }

            // ── v0.5 train(Model, ...) — Burn 딥러닝 학습 파이프라인 연산자 ──
            TokenKind::Train => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                let model_name = self.expect_ident()?;
                let config = self.parse_train_args()?;
                self.expect(&TokenKind::RParen)?;
                Ok(PipelineOp::Train { model_name, config })
            }

            // ── v0.6 withDp(epsilon: 1.0, ...) — 차등 프라이버시 노이즈 주입 ──
            // withDp "(" "epsilon" ":" FLOAT
            //         ("," "mechanism"   ":" ("laplace" | "gaussian"))?
            //         ("," "sensitivity" ":" FLOAT)?
            //         ("," "delta"       ":" FLOAT)?
            //         ("," "seed"        ":" INT)? ")"
            TokenKind::Ident(name) if name == "withDp" => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                let args = self.parse_with_dp_args()?;
                self.expect(&TokenKind::RParen)?;
                Ok(PipelineOp::WithDp(args))
            }

            // ── v0.5 predict(model_var, as: "col") — 학습 모델 예측 연산자 ───
            TokenKind::Ident(name) if name == "predict" => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                let model_var = self.expect_ident()?;
                let mut as_col: Option<String> = None;
                if self.eat(&TokenKind::Comma) {
                    let arg_name = self.expect_ident()?;
                    if arg_name != "as" {
                        return Err(CompileError::new(
                            ErrorKind::UnexpectedToken(arg_name.clone()),
                            self.current_span(),
                            format!("predict() 지원 인수: as. 실제: '{arg_name}'"),
                        ));
                    }
                    self.expect(&TokenKind::Colon)?;
                    as_col = Some(self.expect_string_lit()?);
                }
                self.expect(&TokenKind::RParen)?;
                Ok(PipelineOp::Predict { model_var, as_col })
            }

            other => Err(CompileError::new(
                ErrorKind::UnexpectedToken(format!("{:?}", other)),
                self.current_span(),
                format!(
                    "|> 뒤에는 filter/select/count/groupBy/sum/mean/min/max/orderBy/take/dropNull/fillNull/join/withColumn/chart/cast/rename/replace/sample/median/variance/std/train/predict/withDp 중 하나가 와야 합니다. 실제: {:?}",
                    other
                ),
            )),
        }
    }

    // ── chart { ... } 블록 내부 파싱 ─────────────────────────────────────────
    // 지원 필드: type, x, y, label, value, title
    // 필드 구분자: 쉼표(,) 또는 공백/줄바꿈 — Colon(:)으로 key: value 쌍 파싱
    // 값: 식별자(bar) 또는 문자열 리터럴("bar") 모두 허용
    fn parse_chart_config(&mut self) -> CompileResult<ChartConfig> {
        let span = self.current_span();
        let mut chart_type: Option<ChartType> = None;
        let mut title: Option<String> = None;
        let mut x: Option<String> = None;
        let mut y: Option<String> = None;
        let mut label: Option<String> = None;
        let mut value: Option<String> = None;

        loop {
            // 쉼표를 선택적 필드 구분자로 처리
            self.eat(&TokenKind::Comma);

            match self.current_kind() {
                TokenKind::RBrace | TokenKind::Eof => break,

                // type: bar|line|pie|scatter  (식별자 또는 문자열 리터럴)
                TokenKind::Type => {
                    self.advance();
                    self.expect(&TokenKind::Colon)?;
                    let type_str = self.expect_ident_or_str()?;
                    chart_type = Some(ChartType::from_str(&type_str).ok_or_else(|| {
                        CompileError::new(
                            ErrorKind::Other(format!(
                                "VIZ001: 알 수 없는 차트 타입 '{}'",
                                type_str
                            )),
                            self.current_span(),
                            format!(
                                "ERROR[VIZ001]: '{}' 는 지원하지 않는 차트 타입입니다. \
                                 지원 타입: bar, line, pie, scatter",
                                type_str
                            ),
                        )
                    })?);
                }

                // x: column_name (식별자, 키워드, 문자열 리터럴 모두 허용)
                TokenKind::Ident(ref key) if key == "x" => {
                    self.advance();
                    self.expect(&TokenKind::Colon)?;
                    x = Some(self.expect_col_name_or_str()?);
                }

                // y: column_name
                TokenKind::Ident(ref key) if key == "y" => {
                    self.advance();
                    self.expect(&TokenKind::Colon)?;
                    y = Some(self.expect_col_name_or_str()?);
                }

                // label: column_name
                TokenKind::Ident(ref key) if key == "label" => {
                    self.advance();
                    self.expect(&TokenKind::Colon)?;
                    label = Some(self.expect_col_name_or_str()?);
                }

                // value: column_name
                TokenKind::Ident(ref key) if key == "value" => {
                    self.advance();
                    self.expect(&TokenKind::Colon)?;
                    value = Some(self.expect_col_name_or_str()?);
                }

                // title: "문자열"
                TokenKind::Ident(ref key) if key == "title" => {
                    self.advance();
                    self.expect(&TokenKind::Colon)?;
                    title = Some(self.expect_string_lit()?);
                }

                other => {
                    return Err(CompileError::new(
                        ErrorKind::UnexpectedToken(format!("{:?}", other)),
                        self.current_span(),
                        format!(
                            "chart 블록에 사용할 수 없는 필드: {:?}. \
                             사용 가능: type, x, y, label, value, title",
                            other
                        ),
                    ));
                }
            }
        }

        // type 필드는 필수
        let chart_type = chart_type.ok_or_else(|| {
            CompileError::new(
                ErrorKind::Other("VIZ001: chart 타입 미지정".into()),
                span.clone(),
                "ERROR[VIZ001]: chart 블록에 'type:' 필드가 없습니다. \
                 예: type: bar",
            )
        })?;

        // 차트 타입별 필수 필드 검증
        match &chart_type {
            ChartType::Bar | ChartType::Line => {
                if x.is_none() || y.is_none() {
                    return Err(CompileError::new(
                        ErrorKind::Other("VIZ002: x/y 필드 누락".into()),
                        span,
                        format!(
                            "ERROR[VIZ002]: {} 차트는 x 와 y 필드가 필요합니다.",
                            chart_type.as_str()
                        ),
                    ));
                }
            }
            ChartType::Scatter => {
                if x.is_none() || y.is_none() {
                    return Err(CompileError::new(
                        ErrorKind::Other("VIZ002: x/y 필드 누락".into()),
                        span,
                        "ERROR[VIZ002]: scatter 차트는 x 와 y 필드가 필요합니다.",
                    ));
                }
            }
            ChartType::Pie => {
                if label.is_none() || value.is_none() {
                    return Err(CompileError::new(
                        ErrorKind::Other("VIZ003: label/value 필드 누락".into()),
                        span,
                        "ERROR[VIZ003]: pie 차트는 label 과 value 필드가 필요합니다.",
                    ));
                }
            }
        }

        Ok(ChartConfig {
            chart_type,
            title,
            x,
            y,
            label,
            value,
        })
    }

    // ── 표현식 파서 (우선순위 계층) ───────────────────────────────────────────
    //
    // 우선순위 (낮음 → 높음):
    //   1. 비교 연산자: == != < > <= >=   (parse_expr)
    //   2. 덧셈/뺄셈:  + -               (parse_additive)
    //   3. 곱셈/나눗셈: * /              (parse_multiplicative)
    //   4. 단항/기본식                   (parse_primary)
    //
    // expr = additive (cmp_op additive)?

    fn parse_expr(&mut self) -> CompileResult<Expr> {
        let lhs = self.parse_additive()?;
        if let Some(op) = self.current_cmp_op() {
            self.advance();
            let rhs = self.parse_additive()?;
            Ok(Expr::BinOp {
                lhs: Box::new(lhs),
                op,
                rhs: Box::new(rhs),
            })
        } else {
            Ok(lhs)
        }
    }

    fn current_cmp_op(&self) -> Option<BinOpKind> {
        match self.current_kind() {
            TokenKind::EqEq => Some(BinOpKind::Eq),
            TokenKind::NotEq => Some(BinOpKind::NotEq),
            TokenKind::Lt => Some(BinOpKind::Lt),
            TokenKind::Gt => Some(BinOpKind::Gt),
            TokenKind::LtEq => Some(BinOpKind::LtEq),
            TokenKind::GtEq => Some(BinOpKind::GtEq),
            _ => None,
        }
    }

    // additive = multiplicative (('+' | '-') multiplicative)*
    fn parse_additive(&mut self) -> CompileResult<Expr> {
        let mut lhs = self.parse_multiplicative()?;
        loop {
            let op = match self.current_kind() {
                TokenKind::Plus => BinOpKind::Add,
                TokenKind::Minus => BinOpKind::Sub,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_multiplicative()?;
            lhs = Expr::BinOp {
                lhs: Box::new(lhs),
                op,
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    // multiplicative = primary (('*' | '/') primary)*
    fn parse_multiplicative(&mut self) -> CompileResult<Expr> {
        let mut lhs = self.parse_primary()?;
        loop {
            let op = match self.current_kind() {
                TokenKind::Star => BinOpKind::Mul,
                TokenKind::Slash => BinOpKind::Div,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_primary()?;
            lhs = Expr::BinOp {
                lhs: Box::new(lhs),
                op,
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_primary(&mut self) -> CompileResult<Expr> {
        match self.current_kind() {
            TokenKind::Ident(s) => {
                self.advance();
                // col("column_name") 함수 호출 형태 처리
                // col("x") → Expr::Ident("x")  (런타임에서 polars::col("x") 로 변환됨)
                if s == "col" && matches!(self.current_kind(), TokenKind::LParen) {
                    self.advance(); // ( 소비
                    let col_name = match self.current_kind() {
                        TokenKind::StringLit(name) => {
                            self.advance();
                            name
                        }
                        other => {
                            return Err(CompileError::new(
                                ErrorKind::ExpectedToken("StringLit".into()),
                                self.current_span(),
                                format!(
                                    "col() 안에는 문자열 리터럴이 필요합니다. 예: col(\"income\"). 실제: {:?}",
                                    other
                                ),
                            ));
                        }
                    };
                    self.expect(&TokenKind::RParen)?;
                    Ok(Expr::Ident(col_name))
                } else {
                    Ok(Expr::Ident(s))
                }
            }
            TokenKind::IntLit(n) => {
                self.advance();
                Ok(Expr::IntLit(n))
            }
            TokenKind::FloatLit(f) => {
                self.advance();
                Ok(Expr::FloatLit(f))
            }
            TokenKind::StringLit(s) => {
                self.advance();
                Ok(Expr::StringLit(s))
            }
            // 불리언 리터럴 (v0.16)
            TokenKind::True => {
                self.advance();
                Ok(Expr::BoolLit(true))
            }
            TokenKind::False => {
                self.advance();
                Ok(Expr::BoolLit(false))
            }
            TokenKind::LParen => {
                self.advance();
                let e = self.parse_expr()?;
                self.expect(&TokenKind::RParen)?;
                Ok(e)
            }
            other => Err(CompileError::new(
                ErrorKind::UnexpectedToken(format!("{:?}", other)),
                self.current_span(),
                format!("표현식에 사용할 수 없는 토큰: {:?}", other),
            )),
        }
    }

    // ── 유틸리티 ──────────────────────────────────────────────────────────────

    fn expect_ident(&mut self) -> CompileResult<String> {
        match self.current_kind() {
            TokenKind::Ident(s) => {
                self.advance();
                Ok(s)
            }
            other => Err(CompileError::new(
                ErrorKind::ExpectedToken("Ident".into()),
                self.current_span(),
                format!("식별자(변수명/컬럼명)가 필요합니다. 실제: {:?}", other),
            )),
        }
    }

    /// 현재 토큰이 StringLit이면 소비하고 반환, 아니면 에러
    fn expect_string_lit(&mut self) -> CompileResult<String> {
        match self.current_kind() {
            TokenKind::StringLit(s) => {
                self.advance();
                Ok(s)
            }
            other => Err(CompileError::new(
                ErrorKind::ExpectedToken("StringLit".into()),
                self.current_span(),
                format!("문자열 리터럴이 필요합니다. 실제: {:?}", other),
            )),
        }
    }

    /// 식별자 또는 문자열 리터럴을 수용 — chart type 값 파싱에 사용
    /// type: bar   (Ident)  또는  type: "bar"  (StringLit) 모두 허용
    fn expect_ident_or_str(&mut self) -> CompileResult<String> {
        match self.current_kind() {
            TokenKind::Ident(s) => {
                self.advance();
                Ok(s)
            }
            TokenKind::StringLit(s) => {
                self.advance();
                Ok(s)
            }
            other => Err(CompileError::new(
                ErrorKind::ExpectedToken("Ident or StringLit".into()),
                self.current_span(),
                format!("식별자 또는 문자열 리터럴이 필요합니다. 실제: {:?}", other),
            )),
        }
    }

    /// chart 블록 내 컬럼명 파싱 — 식별자, 예약어, 문자열 리터럴 모두 허용
    /// x: "district"  또는  x: district  둘 다 처리
    fn expect_col_name_or_str(&mut self) -> CompileResult<String> {
        if let TokenKind::StringLit(s) = self.current_kind() {
            self.advance();
            return Ok(s);
        }
        self.expect_col_name()
    }

    /// chart 블록 내 컬럼명 파싱 — 식별자 또는 예약어(count, sum, mean 등)도 허용
    /// 데이터프레임 컬럼명이 xazz 키워드와 같을 수 있기 때문에 필요함
    fn expect_col_name(&mut self) -> CompileResult<String> {
        let name = match self.current_kind() {
            TokenKind::Ident(s) => s.clone(),
            // 키워드도 컬럼명으로 허용 (count, sum, mean, min, max 등은 흔한 컬럼명)
            TokenKind::Count => "count".to_string(),
            TokenKind::Sum => "sum".to_string(),
            TokenKind::Mean => "mean".to_string(),
            TokenKind::Min => "min".to_string(),
            TokenKind::Max => "max".to_string(),
            TokenKind::Filter => "filter".to_string(),
            TokenKind::Select => "select".to_string(),
            TokenKind::GroupBy => "groupBy".to_string(),
            TokenKind::OrderBy => "orderBy".to_string(),
            TokenKind::Take => "take".to_string(),
            TokenKind::DropNull => "dropNull".to_string(),
            TokenKind::FillNull => "fillNull".to_string(),
            TokenKind::Join => "join".to_string(),
            TokenKind::WithColumn => "withColumn".to_string(),
            TokenKind::Chart => "chart".to_string(),
            TokenKind::Sample => "sample".to_string(),
            TokenKind::Median => "median".to_string(),
            TokenKind::Variance => "variance".to_string(),
            TokenKind::Std => "std".to_string(),
            other => {
                return Err(CompileError::new(
                    ErrorKind::ExpectedToken("Ident".into()),
                    self.current_span(),
                    format!("컬럼명(식별자)이 필요합니다. 실제: {:?}", other),
                ));
            }
        };
        self.advance();
        Ok(name)
    }
}

// ── 유닛 테스트 ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{
        BinOpKind, Expr, FillNullValue, JoinHow, PipelineOp, PipelineSource, Stmt, StructField,
    };
    use crate::lexer::Lexer;

    fn parse_src(src: &str) -> CompileResult<Program> {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize()?;
        let mut parser = Parser::new(tokens);
        parser.parse()
    }

    // ── 테스트 1: 변수 선언 파싱 및 VarDecl AST 빌드 ────────────────────────
    #[test]
    fn test_var_decl_and_pipeline_op() {
        let src = r#"v result = load("data.csv") :: AirQuality |> count;"#;
        let program = parse_src(src).expect("파싱 실패");
        assert_eq!(program.stmts.len(), 1);

        match &program.stmts[0] {
            Stmt::VarDecl {
                var_name,
                is_mut,
                source,
                ops,
            } => {
                assert_eq!(var_name, "result");
                assert!(!is_mut);
                assert_eq!(
                    source,
                    &PipelineSource::Load {
                        file_path: "data.csv".into(),
                        schema_name: "AirQuality".into(),
                    }
                );
                assert_eq!(ops.len(), 1);
                assert_eq!(ops[0], PipelineOp::Count(None));
            }
            other => panic!("VarDecl 예상, 실제: {:?}", other),
        }
    }

    // ── 테스트 2: load :: filter 파싱 및 AST BinOp 검증 ─────────────────────
    #[test]
    fn test_load_filter_select_ast() {
        let src = r#"v air = load("seoul.csv") :: AirQuality |> filter(pm10 > 50) |> select([station, date]);"#;
        let program = parse_src(src).expect("파싱 실패");
        assert_eq!(program.stmts.len(), 1);

        match &program.stmts[0] {
            Stmt::VarDecl {
                var_name,
                source,
                ops,
                ..
            } => {
                assert_eq!(var_name, "air");
                assert_eq!(
                    source,
                    &PipelineSource::Load {
                        file_path: "seoul.csv".into(),
                        schema_name: "AirQuality".into(),
                    }
                );
                assert_eq!(ops.len(), 2);

                // filter(pm10 > 50)
                match &ops[0] {
                    PipelineOp::Filter(expr) => match expr {
                        Expr::BinOp { lhs, op, rhs } => {
                            assert_eq!(**lhs, Expr::Ident("pm10".into()));
                            assert_eq!(*op, BinOpKind::Gt);
                            assert_eq!(**rhs, Expr::IntLit(50));
                        }
                        _ => panic!("BinOp 예상"),
                    },
                    _ => panic!("Filter 예상"),
                }

                // select([station, date])
                match &ops[1] {
                    PipelineOp::Select(cols) => {
                        assert_eq!(cols, &vec!["station".to_string(), "date".to_string()]);
                    }
                    _ => panic!("Select 예상"),
                }
            }
            other => panic!("VarDecl 예상, 실제: {:?}", other),
        }
    }

    // ── 테스트 3: TypeDecl 파싱 검증 ────────────────────────────────────────
    #[test]
    fn test_type_decl_parsing() {
        let src = r#"
type AirQuality = {
  station: string,
  pm10: Option<float>,
};
"#;
        let program = parse_src(src).expect("파싱 실패");
        assert_eq!(program.stmts.len(), 1);
        match &program.stmts[0] {
            Stmt::TypeDecl { name, fields } => {
                assert_eq!(name, "AirQuality");
                assert_eq!(fields.len(), 2);
                assert_eq!(
                    fields[0],
                    StructField {
                        name: "station".into(),
                        field_type: "string".into()
                    }
                );
                assert_eq!(
                    fields[1],
                    StructField {
                        name: "pm10".into(),
                        field_type: "Option<float>".into()
                    }
                );
            }
            other => panic!("TypeDecl 예상, 실제: {:?}", other),
        }
    }

    // ── 테스트 4: VarRef (변수 참조) 파싱 ────────────────────────────────────
    #[test]
    fn test_var_ref_pipeline() {
        let src = r#"v filtered = air |> filter(pm25 > 10);"#;
        let program = parse_src(src).expect("파싱 실패");
        assert_eq!(program.stmts.len(), 1);
        match &program.stmts[0] {
            Stmt::VarDecl {
                var_name,
                source,
                ops,
                ..
            } => {
                assert_eq!(var_name, "filtered");
                assert_eq!(source, &PipelineSource::VarRef("air".into()));
                assert_eq!(ops.len(), 1);
            }
            other => panic!("VarDecl 예상, 실제: {:?}", other),
        }
    }

    // ── 테스트 5: :: 없이 load 시 에러 ───────────────────────────────────────
    #[test]
    fn test_missing_schema_error() {
        let src = r#"v x = load("data.csv") |> count;"#;
        let result = parse_src(src);
        assert!(result.is_err(), "스키마 없이 load 하면 에러여야 함");
        let err = result.unwrap_err();
        assert!(
            err.message.contains("::") || format!("{}", err).contains("::"),
            "에러 메시지에 '::' 포함돼야 함: {}",
            err
        );
    }

    // ── 테스트 6: mut 변수 선언 ───────────────────────────────────────────────
    #[test]
    fn test_mut_var_decl() {
        let src = r#"mut v data = load("file.csv") :: Schema;"#;
        let program = parse_src(src).expect("파싱 실패");
        match &program.stmts[0] {
            Stmt::VarDecl { is_mut, .. } => assert!(*is_mut),
            other => panic!("VarDecl 예상: {:?}", other),
        }
    }

    // ── 테스트 7 (v0.16): col("x") 표현식 파싱 ──────────────────────────────
    #[test]
    fn test_col_function_in_filter() {
        let src = r#"v result = data |> filter(col("income") < 1_200_000);"#;
        let program = parse_src(src).expect("파싱 실패");
        match &program.stmts[0] {
            Stmt::VarDecl { ops, .. } => {
                assert_eq!(ops.len(), 1);
                match &ops[0] {
                    PipelineOp::Filter(Expr::BinOp { lhs, op, rhs }) => {
                        assert_eq!(
                            **lhs,
                            Expr::Ident("income".into()),
                            "col(\"income\") → Ident(income) 실패"
                        );
                        assert_eq!(*op, BinOpKind::Lt);
                        assert_eq!(**rhs, Expr::IntLit(1_200_000));
                    }
                    other => panic!("Filter(BinOp) 예상: {:?}", other),
                }
            }
            other => panic!("VarDecl 예상: {:?}", other),
        }
    }

    // ── 테스트 8 (v0.16): 불리언 리터럴 파싱 ────────────────────────────────
    #[test]
    fn test_boolean_literal_in_filter() {
        let src = r#"v result = data |> filter(col("support") == false);"#;
        let program = parse_src(src).expect("파싱 실패");
        match &program.stmts[0] {
            Stmt::VarDecl { ops, .. } => match &ops[0] {
                PipelineOp::Filter(Expr::BinOp { lhs, op, rhs }) => {
                    assert_eq!(**lhs, Expr::Ident("support".into()));
                    assert_eq!(*op, BinOpKind::Eq);
                    assert_eq!(**rhs, Expr::BoolLit(false));
                }
                other => panic!("Filter(BinOp) 예상: {:?}", other),
            },
            other => panic!("VarDecl 예상: {:?}", other),
        }
    }

    // ── 테스트 9 (v0.16): groupBy + count 파싱 ───────────────────────────────
    #[test]
    fn test_group_by_and_count() {
        let src = r#"v result = data |> groupBy("region") |> count("population");"#;
        let program = parse_src(src).expect("파싱 실패");
        match &program.stmts[0] {
            Stmt::VarDecl { ops, .. } => {
                assert_eq!(ops.len(), 2);
                assert_eq!(ops[0], PipelineOp::GroupBy("region".into()));
                assert_eq!(ops[1], PipelineOp::Count(Some("population".into())));
            }
            other => panic!("VarDecl 예상: {:?}", other),
        }
    }

    // ── 테스트 10 (v0.16): mean + orderBy(desc) + take 파싱 ─────────────────
    #[test]
    fn test_mean_orderby_take() {
        let src = r#"v result = data |> groupBy("region") |> mean("income") |> orderBy("income", desc: true) |> take(10);"#;
        let program = parse_src(src).expect("파싱 실패");
        match &program.stmts[0] {
            Stmt::VarDecl { ops, .. } => {
                assert_eq!(ops.len(), 4);
                assert_eq!(ops[0], PipelineOp::GroupBy("region".into()));
                assert_eq!(ops[1], PipelineOp::Mean("income".into()));
                assert_eq!(
                    ops[2],
                    PipelineOp::OrderBy {
                        col: "income".into(),
                        desc: true
                    }
                );
                assert_eq!(ops[3], PipelineOp::Take(10));
            }
            other => panic!("VarDecl 예상: {:?}", other),
        }
    }

    // ── 테스트 11 (v0.16): dropNull + fillNull 파싱 ──────────────────────────
    #[test]
    fn test_drop_null_and_fill_null() {
        let src = r#"v result = data |> dropNull("income") |> fillNull("income", 0);"#;
        let program = parse_src(src).expect("파싱 실패");
        match &program.stmts[0] {
            Stmt::VarDecl { ops, .. } => {
                assert_eq!(ops.len(), 2);
                assert_eq!(ops[0], PipelineOp::DropNull("income".into()));
                assert_eq!(
                    ops[1],
                    PipelineOp::FillNull {
                        col: "income".into(),
                        value: FillNullValue::Int(0),
                    }
                );
            }
            other => panic!("VarDecl 예상: {:?}", other),
        }
    }

    // ── 테스트 12 (v0.16): sum / min / max 파싱 ─────────────────────────────
    #[test]
    fn test_sum_min_max_parse() {
        let src = r#"v result = data |> groupBy("region") |> sum("pop") |> min("price") |> max("price");"#;
        let program = parse_src(src).expect("파싱 실패");
        match &program.stmts[0] {
            Stmt::VarDecl { ops, .. } => {
                assert_eq!(ops.len(), 4);
                assert_eq!(ops[0], PipelineOp::GroupBy("region".into()));
                assert_eq!(ops[1], PipelineOp::Sum("pop".into()));
                assert_eq!(ops[2], PipelineOp::Min("price".into()));
                assert_eq!(ops[3], PipelineOp::Max("price".into()));
            }
            other => panic!("VarDecl 예상: {:?}", other),
        }
    }

    // ── 테스트 13 (v0.16): README 전체 파이프라인 파싱 ──────────────────────
    #[test]
    fn test_readme_full_pipeline() {
        let src = r#"v blind_spots = data
  |> dropNull("income")
  |> filter(col("income") < 1_200_000)
  |> filter(col("support") == false)
  |> groupBy("region")
  |> count("population")
  |> orderBy("population", desc: true)
  |> take(10);"#;

        let program = parse_src(src).expect("README 파이프라인 파싱 실패");
        match &program.stmts[0] {
            Stmt::VarDecl { var_name, ops, .. } => {
                assert_eq!(var_name, "blind_spots");
                assert_eq!(ops.len(), 7, "연산자 7개여야 함: {:?}", ops);

                assert_eq!(ops[0], PipelineOp::DropNull("income".into()));
                // filter(col("income") < 1_200_000)
                assert!(matches!(&ops[1], PipelineOp::Filter(Expr::BinOp { .. })));
                // filter(col("support") == false)
                assert!(matches!(&ops[2], PipelineOp::Filter(Expr::BinOp { .. })));

                assert_eq!(ops[3], PipelineOp::GroupBy("region".into()));
                assert_eq!(ops[4], PipelineOp::Count(Some("population".into())));
                assert_eq!(
                    ops[5],
                    PipelineOp::OrderBy {
                        col: "population".into(),
                        desc: true
                    }
                );
                assert_eq!(ops[6], PipelineOp::Take(10));
            }
            other => panic!("VarDecl 예상: {:?}", other),
        }
    }

    // ── 테스트 14 (v0.16+): join 단일 키 파싱 ────────────────────────────────
    #[test]
    fn test_join_operator_parse() {
        let src = r#"v joined = left |> join(right, on: "id");"#;
        let program = parse_src(src).expect("join 파싱 실패");
        match &program.stmts[0] {
            Stmt::VarDecl { ops, .. } => {
                assert_eq!(ops.len(), 1);
                match &ops[0] {
                    PipelineOp::Join {
                        other,
                        left_on,
                        right_on,
                        how,
                    } => {
                        assert_eq!(other, "right");
                        assert_eq!(left_on, &vec!["id".to_string()]);
                        assert_eq!(right_on, &vec!["id".to_string()]);
                        assert_eq!(how, &JoinHow::Inner);
                    }
                    other => panic!("Join 예상: {:?}", other),
                }
            }
            other => panic!("VarDecl 예상: {:?}", other),
        }
    }

    // ── 테스트 15 (v0.16+): join how 지정 파싱 ───────────────────────────────
    #[test]
    fn test_join_with_how() {
        let src = r#"v joined = left |> join(right, on: "id", how: "left");"#;
        let program = parse_src(src).expect("join how 파싱 실패");
        match &program.stmts[0] {
            Stmt::VarDecl { ops, .. } => match &ops[0] {
                PipelineOp::Join { how, .. } => {
                    assert_eq!(how, &JoinHow::Left);
                }
                other => panic!("Join 예상: {:?}", other),
            },
            other => panic!("VarDecl 예상: {:?}", other),
        }
    }

    // ── 테스트 16 (v0.16+): join 복합 키 파싱 ────────────────────────────────
    #[test]
    fn test_join_multi_key() {
        let src = r#"v joined = a |> join(b, on: ["station", "date"]);"#;
        let program = parse_src(src).expect("join multi-key 파싱 실패");
        match &program.stmts[0] {
            Stmt::VarDecl { ops, .. } => match &ops[0] {
                PipelineOp::Join {
                    left_on, right_on, ..
                } => {
                    assert_eq!(left_on, &vec!["station".to_string(), "date".to_string()]);
                    assert_eq!(right_on, &vec!["station".to_string(), "date".to_string()]);
                }
                other => panic!("Join 예상: {:?}", other),
            },
            other => panic!("VarDecl 예상: {:?}", other),
        }
    }

    // ── 테스트 17 (v0.16+): withColumn 파싱 ──────────────────────────────────
    #[test]
    fn test_with_column_parse() {
        let src = r#"v result = data |> withColumn("total", col("a") + col("b"));"#;
        let program = parse_src(src).expect("withColumn 파싱 실패");
        match &program.stmts[0] {
            Stmt::VarDecl { ops, .. } => {
                assert_eq!(ops.len(), 1);
                match &ops[0] {
                    PipelineOp::WithColumn { name, expr } => {
                        assert_eq!(name, "total");
                        assert!(matches!(
                            expr,
                            Expr::BinOp {
                                op: BinOpKind::Add,
                                ..
                            }
                        ));
                    }
                    other => panic!("WithColumn 예상: {:?}", other),
                }
            }
            other => panic!("VarDecl 예상: {:?}", other),
        }
    }

    // ── 테스트 18 (v0.16+): 산술 표현식 파싱 ─────────────────────────────────
    #[test]
    fn test_arithmetic_expr() {
        let src = r#"v result = data |> withColumn("ratio", col("a") * col("b"));"#;
        let program = parse_src(src).expect("산술 파싱 실패");
        match &program.stmts[0] {
            Stmt::VarDecl { ops, .. } => match &ops[0] {
                PipelineOp::WithColumn { expr, .. } => {
                    assert!(matches!(
                        expr,
                        Expr::BinOp {
                            op: BinOpKind::Mul,
                            ..
                        }
                    ));
                }
                other => panic!("WithColumn 예상: {:?}", other),
            },
            other => panic!("VarDecl 예상: {:?}", other),
        }
    }

    // ── 테스트 19 (v0.16+): 산술 + 비교 복합 표현식 파싱 ────────────────────
    #[test]
    fn test_arithmetic_comparison() {
        // filter(col("a") + col("b") > 100) → BinOp(Add) 가 lhs, Gt 가 비교 op
        let src = r#"v result = data |> filter(col("a") + col("b") > 100);"#;
        let program = parse_src(src).expect("산술+비교 파싱 실패");
        match &program.stmts[0] {
            Stmt::VarDecl { ops, .. } => match &ops[0] {
                PipelineOp::Filter(Expr::BinOp {
                    op: BinOpKind::Gt,
                    lhs,
                    rhs,
                }) => {
                    assert!(
                        matches!(
                            lhs.as_ref(),
                            Expr::BinOp {
                                op: BinOpKind::Add,
                                ..
                            }
                        ),
                        "lhs 가 Add BinOp 여야 함: {:?}",
                        lhs
                    );
                    assert_eq!(*rhs.as_ref(), Expr::IntLit(100));
                }
                other => panic!("Filter(BinOp Gt) 예상: {:?}", other),
            },
            other => panic!("VarDecl 예상: {:?}", other),
        }
    }

    // ── 테스트 20 (v0.19): bar chart 파싱 ────────────────────────────────────
    #[test]
    fn test_chart_bar_parse() {
        use crate::ast::{ChartConfig, ChartType};
        let src = r#"v result = data
            |> groupBy("region")
            |> count()
            |> chart {
                type: bar
                x: region
                y: count
                title: "지역별 범죄 건수"
            };"#;
        let program = parse_src(src).expect("bar chart 파싱 실패");
        match &program.stmts[0] {
            Stmt::VarDecl { ops, .. } => {
                let chart_op = ops.last().expect("마지막 op 없음");
                match chart_op {
                    PipelineOp::Chart(ChartConfig {
                        chart_type,
                        x,
                        y,
                        title,
                        ..
                    }) => {
                        assert_eq!(chart_type, &ChartType::Bar);
                        assert_eq!(x.as_deref(), Some("region"));
                        assert_eq!(y.as_deref(), Some("count"));
                        assert_eq!(title.as_deref(), Some("지역별 범죄 건수"));
                    }
                    other => panic!("Chart op 예상: {:?}", other),
                }
            }
            other => panic!("VarDecl 예상: {:?}", other),
        }
    }

    // ── 테스트 21 (v0.19): pie chart 파싱 ────────────────────────────────────
    #[test]
    fn test_chart_pie_parse() {
        use crate::ast::{ChartConfig, ChartType};
        let src = r#"v result = data |> chart {
            type: pie
            label: category
            value: amount
            title: "예산 분포"
        };"#;
        let program = parse_src(src).expect("pie chart 파싱 실패");
        match &program.stmts[0] {
            Stmt::VarDecl { ops, .. } => match &ops[0] {
                PipelineOp::Chart(ChartConfig {
                    chart_type,
                    label,
                    value,
                    title,
                    ..
                }) => {
                    assert_eq!(chart_type, &ChartType::Pie);
                    assert_eq!(label.as_deref(), Some("category"));
                    assert_eq!(value.as_deref(), Some("amount"));
                    assert_eq!(title.as_deref(), Some("예산 분포"));
                }
                other => panic!("Chart op 예상: {:?}", other),
            },
            other => panic!("VarDecl 예상: {:?}", other),
        }
    }

    // ── 테스트 22 (v0.19): scatter chart 파싱 ────────────────────────────────
    #[test]
    fn test_chart_scatter_parse() {
        use crate::ast::{ChartConfig, ChartType};
        let src = r#"v result = data |> chart {
            type: scatter
            x: study_hours
            y: score
            title: "공부시간과 성적"
        };"#;
        let program = parse_src(src).expect("scatter chart 파싱 실패");
        match &program.stmts[0] {
            Stmt::VarDecl { ops, .. } => match &ops[0] {
                PipelineOp::Chart(ChartConfig {
                    chart_type, x, y, ..
                }) => {
                    assert_eq!(chart_type, &ChartType::Scatter);
                    assert_eq!(x.as_deref(), Some("study_hours"));
                    assert_eq!(y.as_deref(), Some("score"));
                }
                other => panic!("Chart op 예상: {:?}", other),
            },
            other => panic!("VarDecl 예상: {:?}", other),
        }
    }

    // ── 테스트 23 (v0.19): VIZ001 — 잘못된 chart type 에러 ──────────────────
    #[test]
    fn test_chart_invalid_type_error() {
        let src = r#"v result = data |> chart { type: heatmap x: a y: b };"#;
        let result = parse_src(src);
        assert!(result.is_err(), "잘못된 chart type 은 에러여야 함");
        let err = result.unwrap_err();
        assert!(
            err.message.contains("VIZ001") || format!("{}", err).contains("VIZ001"),
            "에러에 VIZ001 포함돼야 함: {}",
            err
        );
    }

    // ── 테스트 24 (v0.19): VIZ002 — bar chart에 x/y 누락 에러 ──────────────
    #[test]
    fn test_chart_missing_xy_error() {
        let src = r#"v result = data |> chart { type: bar x: region };"#;
        let result = parse_src(src);
        assert!(result.is_err(), "x/y 누락은 에러여야 함");
        let err = result.unwrap_err();
        assert!(
            err.message.contains("VIZ002") || format!("{}", err).contains("VIZ002"),
            "에러에 VIZ002 포함돼야 함: {}",
            err
        );
    }

    // ── 테스트 25 (v0.19): VIZ003 — pie chart에 label/value 누락 에러 ────────
    #[test]
    fn test_chart_pie_missing_fields_error() {
        let src = r#"v result = data |> chart { type: pie label: category };"#;
        let result = parse_src(src);
        assert!(result.is_err(), "label/value 누락은 에러여야 함");
        let err = result.unwrap_err();
        assert!(
            err.message.contains("VIZ003") || format!("{}", err).contains("VIZ003"),
            "에러에 VIZ003 포함돼야 함: {}",
            err
        );
    }

    // ── 테스트 26 (v0.19): chart 키워드 렉서 토크나이징 ─────────────────────
    #[test]
    fn test_chart_token() {
        use crate::lexer::Lexer;
        use crate::token::TokenKind;
        let mut lexer = Lexer::new("chart");
        let tokens = lexer.tokenize().expect("토크나이징 실패");
        let kinds: Vec<_> = tokens.iter().map(|t| &t.kind).collect();
        assert!(
            kinds.contains(&&TokenKind::Chart),
            "Chart 토큰 없음: {:?}",
            kinds
        );
    }

    // ── 테스트 27 (v0.22): sample(n) 파싱 ────────────────────────────────────
    #[test]
    fn test_sample_parse() {
        let src = r#"v result = data |> sample(100);"#;
        let program = parse_src(src).expect("sample 파싱 실패");
        match &program.stmts[0] {
            Stmt::VarDecl { ops, .. } => {
                assert_eq!(ops.len(), 1);
                assert_eq!(ops[0], PipelineOp::Sample { n: 100, seed: None });
            }
            other => panic!("VarDecl 예상: {:?}", other),
        }
    }

    // ── 테스트 28 (v0.22): sample(n, seed: 42) 파싱 ──────────────────────────
    #[test]
    fn test_sample_with_seed_parse() {
        let src = r#"v result = data |> sample(100, seed: 42);"#;
        let program = parse_src(src).expect("sample seed 파싱 실패");
        match &program.stmts[0] {
            Stmt::VarDecl { ops, .. } => {
                assert_eq!(
                    ops[0],
                    PipelineOp::Sample {
                        n: 100,
                        seed: Some(42)
                    }
                );
            }
            other => panic!("VarDecl 예상: {:?}", other),
        }
    }

    // ── 테스트 29 (v0.22): sample(0) 은 에러 ─────────────────────────────────
    #[test]
    fn test_sample_zero_error() {
        let src = r#"v result = data |> sample(0);"#;
        let result = parse_src(src);
        assert!(result.is_err(), "sample(0) 은 에러여야 함");
    }

    // ── 테스트 30 (v0.22): median/variance/std 파싱 ──────────────────────────
    #[test]
    fn test_median_variance_std_parse() {
        let src = r#"v result = data |> groupBy("region") |> median("score") |> variance("score") |> std("score");"#;
        let program = parse_src(src).expect("median/variance/std 파싱 실패");
        match &program.stmts[0] {
            Stmt::VarDecl { ops, .. } => {
                assert_eq!(ops.len(), 4);
                assert_eq!(ops[0], PipelineOp::GroupBy("region".into()));
                assert_eq!(ops[1], PipelineOp::Median("score".into()));
                assert_eq!(ops[2], PipelineOp::Variance("score".into()));
                assert_eq!(ops[3], PipelineOp::Std("score".into()));
            }
            other => panic!("VarDecl 예상: {:?}", other),
        }
    }

    // ── 테스트 31 (v0.6): withDp(epsilon: 1.0) 최소 형태 파싱 ────────────────
    #[test]
    fn test_with_dp_minimal_parse() {
        use crate::ast::{DpArgs, DpMechanism};
        let src = r#"v result = data |> count("id") |> withDp(epsilon: 1.0);"#;
        let program = parse_src(src).expect("withDp 파싱 실패");
        match &program.stmts[0] {
            Stmt::VarDecl { ops, .. } => {
                assert_eq!(
                    ops[1],
                    PipelineOp::WithDp(DpArgs {
                        epsilon: 1.0,
                        mechanism: DpMechanism::Laplace,
                        sensitivity: 1.0,
                        delta: None,
                        seed: None,
                    })
                );
            }
            other => panic!("VarDecl 예상: {:?}", other),
        }
    }

    // ── 테스트 32 (v0.6): withDp 전체 인수 파싱 ──────────────────────────────
    #[test]
    fn test_with_dp_full_args_parse() {
        use crate::ast::{DpArgs, DpMechanism};
        let src = r#"v result = data |> mean("cost") |> withDp(epsilon: 0.5, mechanism: gaussian, sensitivity: 2.0, delta: 0.00001, seed: 42);"#;
        let program = parse_src(src).expect("withDp 전체 인수 파싱 실패");
        match &program.stmts[0] {
            Stmt::VarDecl { ops, .. } => {
                assert_eq!(
                    ops[1],
                    PipelineOp::WithDp(DpArgs {
                        epsilon: 0.5,
                        mechanism: DpMechanism::Gaussian,
                        sensitivity: 2.0,
                        delta: Some(0.00001),
                        seed: Some(42),
                    })
                );
            }
            other => panic!("VarDecl 예상: {:?}", other),
        }
    }

    // ── 테스트 33 (v0.6): withDp epsilon 누락/비양수는 에러 ──────────────────
    #[test]
    fn test_with_dp_requires_positive_epsilon() {
        assert!(
            parse_src(r#"v r = data |> withDp(seed: 1);"#).is_err(),
            "epsilon 누락은 에러여야 함"
        );
        assert!(
            parse_src(r#"v r = data |> withDp(epsilon: 0.0);"#).is_err(),
            "epsilon 0 은 에러여야 함"
        );
    }

    // ── 테스트 34 (v0.6): withDp 알 수 없는 mechanism 은 에러 ────────────────
    #[test]
    fn test_with_dp_unknown_mechanism_error() {
        assert!(
            parse_src(r#"v r = data |> withDp(epsilon: 1.0, mechanism: exponential);"#).is_err(),
            "미지원 mechanism 은 에러여야 함"
        );
    }
}
