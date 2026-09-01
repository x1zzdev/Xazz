/// xazzLang - compiler/runtime error type definitions (v0.16)
/// Diagnostic Engine: precise Line/Col tracking + friendly message formatting
/// + AI Suggestion: ai_suggestion field + SafeLoadViolation
use crate::i18n::{is_korean, tr};
use crate::token::Span;

/// Compile error kinds (refined ErrorKind)
#[derive(Debug, Clone, PartialEq)]
pub enum ErrorKind {
    /// Lexer: unknown character
    UnexpectedChar(char),
    /// Parser: unexpected token
    UnexpectedToken(String),
    /// Parser: expected token not found
    ExpectedToken(String),
    /// Codegen/runtime: reference to an undeclared type
    UndeclaredType(String),
    /// Runtime: reference to an undeclared variable
    UndeclaredVariable(String),
    /// Runtime: type mismatch (declared type, actual type)
    TypeMismatch {
        expected: String,
        found: String,
        field: String,
    },
    /// Runtime: null in a required field
    NullViolation { field: String, schema: String },
    /// Runtime: file I/O error
    IoError(String),
    /// Runtime: CSV schema mapping failed
    SchemaMappingFailed { schema: String, reason: String },
    /// Safe-Load violation: reference to a column not in the schema
    SafeLoadViolation {
        col: String,
        schema: String,
        available: Vec<String>,
    },
    /// DivisionByZero: division by zero detected
    DivisionByZero {
        col: String,
        row_count: usize,
        expr_context: String,
    },
    /// Other
    Other(String),
}

impl ErrorKind {
    /// Returns the category label for an error kind
    pub fn category(&self) -> &'static str {
        match self {
            ErrorKind::UnexpectedChar(_) => tr("lexer error", "렉서 에러"),
            ErrorKind::UnexpectedToken(_) => tr("syntax error", "구문 에러"),
            ErrorKind::ExpectedToken(_) => tr("syntax error", "구문 에러"),
            ErrorKind::UndeclaredType(_) => tr("type error", "타입 에러"),
            ErrorKind::UndeclaredVariable(_) => tr("variable error", "변수 에러"),
            ErrorKind::TypeMismatch { .. } => tr("type error", "타입 에러"),
            ErrorKind::NullViolation { .. } => tr("null violation", "Null 위반"),
            ErrorKind::IoError(_) => tr("io error", "IO 에러"),
            ErrorKind::SchemaMappingFailed { .. } => tr("schema error", "스키마 에러"),
            ErrorKind::SafeLoadViolation { .. } => tr("safe-load violation", "Safe-Load 위반"),
            ErrorKind::DivisionByZero { .. } => "DivisionByZero",
            ErrorKind::Other(_) => tr("error", "에러"),
        }
    }
}

/// Compile error struct
#[derive(Debug, Clone, PartialEq)]
pub struct CompileError {
    pub kind: ErrorKind,
    pub span: Span,
    pub message: String,
    /// AI-based fix suggestion (if any)
    pub ai_suggestion: Option<String>,
}

impl CompileError {
    pub fn new(kind: ErrorKind, span: Span, message: impl Into<String>) -> Self {
        let message = message.into();
        let ai_suggestion = generate_suggestion(&kind);
        CompileError {
            kind,
            span,
            message,
            ai_suggestion,
        }
    }

    /// Create an error without a span (for runtime errors)
    pub fn runtime(kind: ErrorKind, message: impl Into<String>) -> Self {
        let message = message.into();
        let ai_suggestion = generate_suggestion(&kind);
        CompileError {
            kind,
            span: Span::new(0, 0),
            message,
            ai_suggestion,
        }
    }

    /// Create an error with ai_suggestion set directly
    pub fn with_suggestion(
        kind: ErrorKind,
        span: Span,
        message: impl Into<String>,
        suggestion: impl Into<String>,
    ) -> Self {
        CompileError {
            kind,
            span,
            message: message.into(),
            ai_suggestion: Some(suggestion.into()),
        }
    }
}

/// Auto-generate an AI fix suggestion from an ErrorKind
fn generate_suggestion(kind: &ErrorKind) -> Option<String> {
    let ko = crate::i18n::is_korean();
    match kind {
        ErrorKind::TypeMismatch {
            expected,
            found,
            field,
        } => {
            if ko {
                Some(format!(
                    "필드 '{}' 의 타입이 '{}' 가 아닌 '{}' 입니다. → 올바른 타입 '{}' 으로 변경하거나 cast() 를 사용하세요.",
                    field, expected, found, expected
                ))
            } else {
                Some(format!(
                    "Field '{}' has type '{}', not '{}'. → change it to '{}' or use cast().",
                    field, found, expected, expected
                ))
            }
        }
        ErrorKind::NullViolation { field, schema } => {
            if ko {
                Some(format!(
                    "스키마 '{}' 의 필수 필드 '{}' 에 null 값이 있습니다. → dropNull(\"{}\") 또는 fillNull(\"{}\", <기본값>) 을 파이프라인에 추가하세요.",
                    schema, field, field, field
                ))
            } else {
                Some(format!(
                    "Required field '{}' of schema '{}' contains null. → add dropNull(\"{}\") or fillNull(\"{}\", <default>) to the pipeline.",
                    field, schema, field, field
                ))
            }
        }
        ErrorKind::SafeLoadViolation {
            col,
            schema,
            available,
        } => {
            let hint = find_closest(col, available)
                .map(|s| format!("  Did you mean: col(\"{}\")?", s))
                .unwrap_or_default();
            if ko {
                Some(format!(
                    "스키마 '{}' 에 '{}' 컬럼이 없습니다.\n💡 사용 가능한 컬럼: {}\n{}",
                    schema,
                    col,
                    available.join(", "),
                    hint
                ))
            } else {
                Some(format!(
                    "Schema '{}' does not contain column '{}'.\n💡 available columns: {}\n{}",
                    schema,
                    col,
                    available.join(", "),
                    hint
                ))
            }
        }
        ErrorKind::UndeclaredVariable(name) => {
            if ko {
                Some(format!(
                    "변수 '{}' 가 선언되지 않았습니다. → 이 변수를 먼저 `v {} = ...` 으로 선언하세요.",
                    name, name
                ))
            } else {
                Some(format!(
                    "Variable '{}' is not declared. → declare it first with `v {} = ...`.",
                    name, name
                ))
            }
        }
        ErrorKind::UndeclaredType(name) => {
            if ko {
                Some(format!(
                    "타입 '{}' 가 선언되지 않았습니다. → `type {} = {{ ... }}` 으로 먼저 선언하세요.",
                    name, name
                ))
            } else {
                Some(format!(
                    "Type '{}' is not declared. → declare it first with `type {} = {{ ... }}`.",
                    name, name
                ))
            }
        }
        ErrorKind::DivisionByZero {
            col,
            row_count: _,
            expr_context: _,
        } => {
            if ko {
                Some(format!(
                    "0 으로 나누는 연산이 감지되었습니다. → filter({} != 0) 또는 fillNull(\"{}\", 1) 등을 파이프라인에 추가하세요.",
                    col, col
                ))
            } else {
                Some(format!(
                    "Division by zero detected. → add filter({} != 0) or fillNull(\"{}\", 1) to the pipeline.",
                    col, col
                ))
            }
        }
        _ => None,
    }
}

/// Return the closest candidate based on Levenshtein edit distance
pub fn find_closest<'a>(name: &str, candidates: &'a [String]) -> Option<&'a str> {
    candidates
        .iter()
        .min_by_key(|c| edit_distance(name, c.as_str()))
        .map(String::as_str)
}

pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 0..=m {
        dp[i][0] = i;
    }
    for j in 0..=n {
        dp[0][j] = j;
    }
    for i in 1..=m {
        for j in 1..=n {
            dp[i][j] = if a[i - 1] == b[j - 1] {
                dp[i - 1][j - 1]
            } else {
                1 + dp[i - 1][j].min(dp[i][j - 1]).min(dp[i - 1][j - 1])
            };
        }
    }
    dp[m][n]
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.span.line == 0 {
            write!(f, "{}: {}", self.kind.category(), self.message)?;
        } else {
            write!(
                f,
                "{} [Line {}: Col {}]: {}",
                self.kind.category(),
                self.span.line,
                self.span.col,
                self.message
            )?;
        }
        if let Some(ref suggestion) = self.ai_suggestion {
            if is_korean() {
                write!(f, "\n💡 제안: {}", suggestion)?;
            } else {
                write!(f, "\n💡 Suggestion: {}", suggestion)?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for CompileError {}

/// Compile result type alias
pub type CompileResult<T> = Result<T, CompileError>;

// ── Error module tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{is_korean, tr};
    use crate::token::Span;

    #[test]
    fn test_error_ai_suggestion_display() {
        let err = CompileError::new(
            ErrorKind::TypeMismatch {
                expected: "float".into(),
                found: "string".into(),
                field: "pm10".into(),
            },
            Span::new(3, 5),
            "타입 불일치",
        );
        let display = format!("{}", err);
        assert!(display.contains("💡"), "제안 없음: {}", display);
        assert!(display.contains("pm10"), "필드명 포함 안 됨: {}", display);
    }

    #[test]
    fn test_safe_load_violation_suggestion() {
        let err = CompileError::new(
            ErrorKind::SafeLoadViolation {
                col: "pm_10".into(),
                schema: "AirQuality".into(),
                available: vec!["pm10".into(), "pm25".into(), "station".into()],
            },
            Span::new(0, 0),
            "컬럼 없음",
        );
        let display = format!("{}", err);
        assert!(display.contains("💡"), "제안 없음: {}", display);
        assert!(
            display.contains("pm10"),
            "Did you mean 제안 없음: {}",
            display
        );
    }

    #[test]
    fn test_type_mismatch_with_suggestion() {
        let err = CompileError::new(
            ErrorKind::TypeMismatch {
                expected: "int".into(),
                found: "float".into(),
                field: "age".into(),
            },
            Span::new(1, 1),
            "타입 오류",
        );
        assert!(err.ai_suggestion.is_some());
        let s = err.ai_suggestion.unwrap();
        assert!(s.contains("age"));
        assert!(s.contains("int"));
    }

    #[test]
    fn test_error_without_suggestion() {
        let err = CompileError::new(
            ErrorKind::UnexpectedChar('@'),
            Span::new(2, 4),
            "알 수 없는 문자",
        );
        assert!(err.ai_suggestion.is_none());
        let display = format!("{}", err);
        assert!(
            !display.contains("💡"),
            "제안 없는 에러에 💡 출력됨: {}",
            display
        );
    }

    #[test]
    fn test_edit_distance() {
        assert_eq!(edit_distance("pm10", "pm10"), 0);
        assert_eq!(edit_distance("pm_10", "pm10"), 1);
        assert_eq!(edit_distance("abc", "xyz"), 3);
    }

    #[test]
    fn test_division_by_zero_suggestion() {
        let err = CompileError::new(
            ErrorKind::DivisionByZero {
                col: "pm25".into(),
                row_count: 3,
                expr_context: "col(\"pm10\") / col(\"pm25\")".into(),
            },
            Span::new(0, 0),
            "0으로 나누기 감지",
        );
        let display = format!("{}", err);
        assert!(
            display.contains("DivisionByZero"),
            "DivisionByZero 카테고리 없음: {}",
            display
        );
        assert!(display.contains("💡"), "제안 없음: {}", display);
        assert!(display.contains("pm25"), "컬럼명 포함 안 됨: {}", display);
        assert!(
            display.contains("filter(pm25 != 0)"),
            "분모 처리 제안 누락: {}",
            display
        );
    }
}
