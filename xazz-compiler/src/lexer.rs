/// xazzLang - lexer (complete Peekable<Chars> state machine)
///
/// Every field is actually used so there are no dead_code warnings:
///   source  - byte-offset boundary checks (is_at_end)
///   chars   - state-machine iterator
///   pos     - current byte offset (accumulated via UTF-8 len_utf8)
///   line/col- source-position tracking
///
/// [v0.16 changes]
///   - underscore in numeric literals: 1_200_000 → 1200000
///   - new keywords: groupBy, sum, mean, min, max, orderBy, take, dropNull, fillNull
///   - boolean keywords: true, false
///   - sort-direction keyword: desc
use crate::error::{CompileError, CompileResult, ErrorKind};
use crate::token::{Span, Token, TokenKind};

pub struct Lexer<'src> {
    source: &'src str,
    chars: std::iter::Peekable<std::str::Chars<'src>>,
    pos: usize,
    line: usize,
    col: usize,
}

impl<'src> Lexer<'src> {
    pub fn new(source: &'src str) -> Self {
        Lexer {
            source,
            chars: source.chars().peekable(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    // ── basic helpers ────────────────────────────────────────────────────────────

    /// Checks whether the iterator has been fully consumed (see the source field)
    fn is_at_end(&self) -> bool {
        self.pos >= self.source.len()
    }

    fn peek(&mut self) -> Option<char> {
        self.chars.peek().copied()
    }

    /// Consumes one character and updates pos / line / col
    fn advance(&mut self) -> Option<char> {
        match self.chars.next() {
            Some(c) => {
                self.pos += c.len_utf8();
                if c == '\n' {
                    self.line += 1;
                    self.col = 1;
                } else {
                    self.col += 1;
                }
                Some(c)
            }
            None => None,
        }
    }

    fn span(&self) -> Span {
        Span::new(self.line, self.col)
    }

    // ── string literals ────────────────────────────────────────────────────────

    /// Called when the opening '"' has already been consumed
    fn read_string(&mut self, open_span: Span) -> CompileResult<TokenKind> {
        let mut s = String::new();
        loop {
            match self.advance() {
                Some('"') => break,
                Some('\\') => match self.advance() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('\\') => s.push('\\'),
                    Some('"') => s.push('"'),
                    Some(c) => s.push(c),
                    None => {
                        return Err(CompileError::new(
                            ErrorKind::UnexpectedToken("EOF in string escape".into()),
                            open_span,
                            "문자열 이스케이프 처리 중 파일 끝",
                        ));
                    }
                },
                Some(c) => s.push(c),
                None => {
                    return Err(CompileError::new(
                        ErrorKind::UnexpectedToken("Unterminated string".into()),
                        open_span,
                        "닫는 '\"' 없이 파일이 끝남",
                    ));
                }
            }
        }
        Ok(TokenKind::StringLit(s))
    }

    // ── numeric literals ──────────────────────────────────────────────────────────

    /// The first digit (first) has already been consumed
    /// Allows underscores (_): 1_200_000 → IntLit(1200000)
    fn read_number(&mut self, first: char, span: &Span) -> CompileResult<TokenKind> {
        let mut buf = String::new();
        buf.push(first);

        // integer part (ignore underscores)
        while self
            .peek()
            .map_or(false, |c| c.is_ascii_digit() || c == '_')
        {
            let c = self.advance().unwrap();
            if c != '_' {
                buf.push(c);
            }
        }

        // decimal point + fractional part
        if self.peek() == Some('.') {
            self.advance(); // consume '.'
            buf.push('.');
            while self
                .peek()
                .map_or(false, |c| c.is_ascii_digit() || c == '_')
            {
                let c = self.advance().unwrap();
                if c != '_' {
                    buf.push(c);
                }
            }
            let value = buf.parse::<f64>().map_err(|_| {
                CompileError::new(
                    ErrorKind::Other(format!("숫자 리터럴 파싱 실패: {}", buf)),
                    span.clone(),
                    format!("숫자 리터럴 파싱 실패: '{}'", buf),
                )
            })?;
            return Ok(TokenKind::FloatLit(value));
        }

        let value = buf.parse::<i64>().map_err(|_| {
            CompileError::new(
                ErrorKind::Other(format!("숫자 리터럴 파싱 실패: {}", buf)),
                span.clone(),
                format!("정수 리터럴이 i64 범위를 벗어났거나 파싱 실패: '{}'", buf),
            )
        })?;
        Ok(TokenKind::IntLit(value))
    }

    // ── identifiers · keywords ──────────────────────────────────────────────────────

    fn read_ident(&mut self, first: char) -> TokenKind {
        let mut buf = String::new();
        buf.push(first);
        while self
            .peek()
            .map_or(false, |c| c.is_alphanumeric() || c == '_')
        {
            buf.push(self.advance().unwrap());
        }
        Self::keyword_or_ident(buf)
    }

    fn keyword_or_ident(s: String) -> TokenKind {
        match s.as_str() {
            // ── existing keywords ──────────────────────────────────
            "type" => TokenKind::Type,
            "load" => TokenKind::Load,
            "filter" => TokenKind::Filter,
            "select" => TokenKind::Select,
            "count" => TokenKind::Count,
            "v" => TokenKind::V,
            "mut" => TokenKind::Mut,
            "Option" => TokenKind::OptionKw,
            // ── v0.16 new pipeline operator keywords ────────────
            "groupBy" => TokenKind::GroupBy,
            "sum" => TokenKind::Sum,
            "mean" => TokenKind::Mean,
            "min" => TokenKind::Min,
            "max" => TokenKind::Max,
            "orderBy" => TokenKind::OrderBy,
            "take" => TokenKind::Take,
            "dropNull" => TokenKind::DropNull,
            "fillNull" => TokenKind::FillNull,
            // ── v0.16+ new pipeline operator keywords ───────────
            "join" => TokenKind::Join,
            "withColumn" => TokenKind::WithColumn,
            "on" => TokenKind::On,
            "how" => TokenKind::How,
            // ── v0.16 literal / named-argument keywords ─────────────
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "desc" => TokenKind::Desc,
            // ── v0.19 visualization keywords ───────────────────────────
            "chart" => TokenKind::Chart,
            // ── v0.20 type casting keywords ─────────────────────
            "cast" => TokenKind::Cast,
            // ── v0.21 new pipeline operator keywords ────────────────
            "rename" => TokenKind::Rename,
            "replace" => TokenKind::Replace,
            "left_on" => TokenKind::LeftOn,
            "right_on" => TokenKind::RightOn,
            // ── v0.22 new pipeline operator keywords ────────────────
            "sample" => TokenKind::Sample,
            "median" => TokenKind::Median,
            "variance" => TokenKind::Variance,
            "std" => TokenKind::Std,
            "seed" => TokenKind::Seed,
            // ── v0.3 deep-learning keywords ───────────────────────────────
            "model" => TokenKind::Model,
            "run" => TokenKind::Run,
            "train" => TokenKind::Train,
            "epochs" => TokenKind::Epochs,
            "lr" => TokenKind::Lr,
            "target" => TokenKind::Target,
            "strategy" => TokenKind::Strategy,

            // ── identifier ───────────────────────────────────────────
            _ => TokenKind::Ident(s),
        }
    }

    // ── main state machine ────────────────────────────────────────────────────────

    /// Returns the next Token (core of the state machine)
    pub fn next_token(&mut self) -> CompileResult<Token> {
        // skip whitespace
        while self.peek().map_or(false, |c| c.is_whitespace()) {
            self.advance();
        }

        // end of file
        if self.is_at_end() {
            return Ok(Token::new(TokenKind::Eof, self.span()));
        }

        let span = self.span();
        let ch = match self.advance() {
            Some(c) => c,
            None => return Ok(Token::new(TokenKind::Eof, span)),
        };

        let kind = match ch {
            // ── comments ──────────────────────────────────────────────────
            '/' if self.peek() == Some('/') => {
                // consume to end of line, then recurse
                while self.peek().map_or(false, |c| c != '\n') {
                    self.advance();
                }
                return self.next_token();
            }
            '/' => TokenKind::Slash,

            // ── strings ────────────────────────────────────────────────
            '"' => self.read_string(span.clone())?,

            // ── two-character operators ─────────────────────────────────────────
            '|' if self.peek() == Some('>') => {
                self.advance();
                TokenKind::Pipeline
            }
            ':' if self.peek() == Some(':') => {
                self.advance();
                TokenKind::TypeAssign
            }
            '=' if self.peek() == Some('=') => {
                self.advance();
                TokenKind::EqEq
            }
            '!' if self.peek() == Some('=') => {
                self.advance();
                TokenKind::NotEq
            }
            '<' if self.peek() == Some('=') => {
                self.advance();
                TokenKind::LtEq
            }
            '>' if self.peek() == Some('=') => {
                self.advance();
                TokenKind::GtEq
            }
            '-' if self.peek() == Some('>') => {
                self.advance();
                TokenKind::Arrow
            }

            // ── single-character operators ───────────────────────────────────────
            '=' => TokenKind::Assign,
            '<' => TokenKind::Lt,
            '>' => TokenKind::Gt,
            '+' => TokenKind::Plus,
            '*' => TokenKind::Star,
            '!' => TokenKind::Bang,
            '.' => TokenKind::Dot,
            ':' => TokenKind::Colon,

            // ── negative number or Minus ────────────────────────────────────────
            '-' if self.peek().map_or(false, |c| c.is_ascii_digit()) => {
                let digit = self.advance().unwrap();
                match self.read_number(digit, &span)? {
                    TokenKind::IntLit(n) => {
                        // -i64::MIN would overflow, so explicitly reject it
                        if n == i64::MIN {
                            return Err(CompileError::new(
                                ErrorKind::Other(
                                    "숫자 리터럴 파싱 실패: -9223372036854775808".to_string(),
                                ),
                                span,
                                "리터럴이 i64 범위를 벗어났습니다: -9223372036854775808",
                            ));
                        }
                        TokenKind::IntLit(-n)
                    }
                    TokenKind::FloatLit(f) => TokenKind::FloatLit(-f),
                    other => other,
                }
            }
            '-' => TokenKind::Minus,

            // ── delimiters ────────────────────────────────────────────────
            '{' => TokenKind::LBrace,
            '}' => TokenKind::RBrace,
            '(' => TokenKind::LParen,
            ')' => TokenKind::RParen,
            '[' => TokenKind::LBracket,
            ']' => TokenKind::RBracket,
            ',' => TokenKind::Comma,
            ';' => TokenKind::Semicolon,

            // ── numbers ───────────────────────────────────────────────────
            c if c.is_ascii_digit() => self.read_number(c, &span)?,

            // ── identifiers / keywords ────────────────────────────────────────
            c if c.is_alphabetic() || c == '_' => self.read_ident(c),

            // ── unknown character ────────────────────────────────────────
            other => {
                return Err(CompileError::new(
                    ErrorKind::UnexpectedChar(other),
                    span,
                    format!("예상치 못한 문자: '{}'", other),
                ));
            }
        };

        Ok(Token::new(kind, span))
    }

    /// Tokenizes the entire source and returns Vec<Token>
    pub fn tokenize(&mut self) -> CompileResult<Vec<Token>> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token()?;
            let done = matches!(tok.kind, TokenKind::Eof) || self.is_at_end();
            tokens.push(tok);
            if done {
                break;
            }
        }
        Ok(tokens)
    }
}

// ── lexer unit tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::TokenKind;

    fn tokenize(src: &str) -> Vec<TokenKind> {
        let mut lexer = Lexer::new(src);
        lexer
            .tokenize()
            .expect("토크나이징 실패")
            .into_iter()
            .map(|t| t.kind)
            .collect()
    }

    // ── test 1: variable declaration keyword + pipeline operator (|>) tokenizing ────────
    #[test]
    fn test_var_decl_and_pipeline_token() {
        let kinds = tokenize("v result = load(\"data.csv\") :: MySchema |> count");
        assert!(kinds.contains(&TokenKind::V), "V 토큰 없음");
        assert!(kinds.contains(&TokenKind::Assign), "Assign 토큰 없음");
        assert!(kinds.contains(&TokenKind::Load), "Load 토큰 없음");
        assert!(
            kinds.contains(&TokenKind::TypeAssign),
            "TypeAssign(::) 토큰 없음"
        );
        assert!(kinds.contains(&TokenKind::Pipeline), "|> 토큰 없음");
        assert!(kinds.contains(&TokenKind::Count), "Count 토큰 없음");
        assert!(
            kinds.contains(&TokenKind::Ident("MySchema".into())),
            "MySchema Ident 없음"
        );
        assert!(
            kinds.contains(&TokenKind::StringLit("data.csv".into())),
            "StringLit 없음"
        );
    }

    // ── test 2: mut keyword + negative literal ──────────────────────────────────
    #[test]
    fn test_mut_keyword_and_negative_literal() {
        let kinds = tokenize("mut v x = -42");
        assert!(kinds.contains(&TokenKind::Mut), "Mut 토큰 없음");
        assert!(kinds.contains(&TokenKind::V), "V 토큰 없음");
        assert!(kinds.contains(&TokenKind::IntLit(-42)), "IntLit(-42) 없음");
    }

    // ── test 3: Option<float> type tokenizing ─────────────────────────────
    #[test]
    fn test_option_type_tokens() {
        let kinds = tokenize("pm10: Option<float>");
        assert!(kinds.contains(&TokenKind::Colon), "Colon 없음");
        assert!(kinds.contains(&TokenKind::OptionKw), "OptionKw 없음");
        assert!(kinds.contains(&TokenKind::Lt), "Lt(<) 없음");
        assert!(
            kinds.contains(&TokenKind::Ident("float".into())),
            "float Ident 없음"
        );
        assert!(kinds.contains(&TokenKind::Gt), "Gt(>) 없음");
    }

    // ── test 4: all comparison operators ──────────────────────────────────────────
    #[test]
    fn test_comparison_operators() {
        let kinds = tokenize("a == b != c < d > e <= f >= g");
        assert!(kinds.contains(&TokenKind::EqEq));
        assert!(kinds.contains(&TokenKind::NotEq));
        assert!(kinds.contains(&TokenKind::Lt));
        assert!(kinds.contains(&TokenKind::Gt));
        assert!(kinds.contains(&TokenKind::LtEq));
        assert!(kinds.contains(&TokenKind::GtEq));
    }

    // ── test 5: comment ignoring ─────────────────────────────────────────────────
    #[test]
    fn test_comment_ignored() {
        let kinds = tokenize("v x = 1 // this is a comment\n");
        // comment content must not appear as tokens
        assert!(
            !kinds.contains(&TokenKind::Slash),
            "Slash 토큰이 주석에서 생성됨"
        );
        assert!(kinds.contains(&TokenKind::V));
        assert!(kinds.contains(&TokenKind::IntLit(1)));
    }

    // ── test 6: string escaping ─────────────────────────────────────────
    #[test]
    fn test_string_escape_sequences() {
        let kinds = tokenize(r#""hello\nworld""#);
        assert!(kinds.contains(&TokenKind::StringLit("hello\nworld".into())));
    }

    // ── test 7: Span (position) tracking accuracy ────────────────────────────────────
    #[test]
    fn test_span_tracking() {
        let src = "v\n result";
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        // first token 'v' → line 1
        assert_eq!(tokens[0].span.line, 1);
        // second token 'result' → line 2
        let result_tok = tokens
            .iter()
            .find(|t| t.kind == TokenKind::Ident("result".into()));
        assert!(result_tok.is_some());
        assert_eq!(result_tok.unwrap().span.line, 2);
    }

    // ── test 8: unknown character error ─────────────────────────────────────
    #[test]
    fn test_unknown_char_error() {
        let mut lexer = Lexer::new("v @ x");
        let result = lexer.tokenize();
        assert!(result.is_err(), "@ 문자는 에러여야 함");
        let err = result.unwrap_err();
        assert!(matches!(
            err.kind,
            crate::error::ErrorKind::UnexpectedChar('@')
        ));
    }

    // ── test 9 (v0.16): new pipeline keyword tokenizing ──────────────────
    #[test]
    fn test_new_pipeline_keywords() {
        let src = "groupBy sum mean min max orderBy take dropNull fillNull";
        let kinds = tokenize(src);
        assert!(kinds.contains(&TokenKind::GroupBy), "GroupBy 없음");
        assert!(kinds.contains(&TokenKind::Sum), "Sum 없음");
        assert!(kinds.contains(&TokenKind::Mean), "Mean 없음");
        assert!(kinds.contains(&TokenKind::Min), "Min 없음");
        assert!(kinds.contains(&TokenKind::Max), "Max 없음");
        assert!(kinds.contains(&TokenKind::OrderBy), "OrderBy 없음");
        assert!(kinds.contains(&TokenKind::Take), "Take 없음");
        assert!(kinds.contains(&TokenKind::DropNull), "DropNull 없음");
        assert!(kinds.contains(&TokenKind::FillNull), "FillNull 없음");
    }

    // ── test 10 (v0.16): boolean keywords ────────────────────────────────────
    #[test]
    fn test_boolean_keywords() {
        let kinds = tokenize("true false");
        assert!(kinds.contains(&TokenKind::True), "True 없음");
        assert!(kinds.contains(&TokenKind::False), "False 없음");
    }

    // ── test 11 (v0.16): numeric underscore ──────────────────────────────────
    #[test]
    fn test_number_underscore() {
        let kinds = tokenize("1_200_000");
        assert!(
            kinds.contains(&TokenKind::IntLit(1_200_000)),
            "1_200_000 → IntLit(1200000) 변환 실패: {:?}",
            kinds
        );
    }

    // ── test 12 (v0.16): desc keyword ──────────────────────────────────────
    #[test]
    fn test_desc_keyword() {
        let kinds = tokenize("desc");
        assert!(kinds.contains(&TokenKind::Desc), "Desc 없음");
    }

    // ── test 13 (v0.22): sample/median/variance/std/seed keywords ──────────
    #[test]
    fn test_new_v22_keywords() {
        let kinds = tokenize("sample median variance std seed");
        assert!(kinds.contains(&TokenKind::Sample), "Sample 없음");
        assert!(kinds.contains(&TokenKind::Median), "Median 없음");
        assert!(kinds.contains(&TokenKind::Variance), "Variance 없음");
        assert!(kinds.contains(&TokenKind::Std), "Std 없음");
        assert!(kinds.contains(&TokenKind::Seed), "Seed 없음");
    }
}
