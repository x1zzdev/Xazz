/// xazzLang - 컴파일러/런타임 에러 타입 정의 (v0.16)
/// Diagnostic Engine: Line/Col 정확 추적 + 친화적 메시지 포맷
/// + AI Suggestion: ai_suggestion 필드 + SafeLoadViolation
use crate::token::Span;

/// 컴파일 에러 종류 (ErrorKind 고도화)
#[derive(Debug, Clone, PartialEq)]
pub enum ErrorKind {
    /// 렉서: 알 수 없는 문자
    UnexpectedChar(char),
    /// 파서: 예상치 못한 토큰
    UnexpectedToken(String),
    /// 파서: 예상 토큰 미등장
    ExpectedToken(String),
    /// 코드젠/런타임: 미선언 타입 참조
    UndeclaredType(String),
    /// 런타임: 미선언 변수 참조
    UndeclaredVariable(String),
    /// 런타임: 타입 불일치 (선언 타입, 실제 타입)
    TypeMismatch {
        expected: String,
        found: String,
        field: String,
    },
    /// 런타임: 필수 필드에 null 발생
    NullViolation { field: String, schema: String },
    /// 런타임: 파일 입출력 오류
    IoError(String),
    /// 런타임: CSV 스키마 매핑 실패
    SchemaMappingFailed { schema: String, reason: String },
    /// Safe-Load 위반: 스키마에 없는 컬럼 참조
    SafeLoadViolation {
        col: String,
        schema: String,
        available: Vec<String>,
    },
    /// DivisionByZero: 0으로 나누기 감지
    DivisionByZero {
        col: String,
        row_count: usize,
        expr_context: String,
    },
    /// 기타
    Other(String),
}

impl ErrorKind {
    /// 에러 종류의 카테고리 레이블 반환
    pub fn category(&self) -> &'static str {
        match self {
            ErrorKind::UnexpectedChar(_) => "렉서 에러",
            ErrorKind::UnexpectedToken(_) => "구문 에러",
            ErrorKind::ExpectedToken(_) => "구문 에러",
            ErrorKind::UndeclaredType(_) => "타입 에러",
            ErrorKind::UndeclaredVariable(_) => "변수 에러",
            ErrorKind::TypeMismatch { .. } => "타입 에러",
            ErrorKind::NullViolation { .. } => "Null 위반",
            ErrorKind::IoError(_) => "IO 에러",
            ErrorKind::SchemaMappingFailed { .. } => "스키마 에러",
            ErrorKind::SafeLoadViolation { .. } => "Safe-Load 위반",
            ErrorKind::DivisionByZero { .. } => "DivisionByZero",
            ErrorKind::Other(_) => "에러",
        }
    }
}

/// 컴파일 에러 구조체
#[derive(Debug, Clone, PartialEq)]
pub struct CompileError {
    pub kind: ErrorKind,
    pub span: Span,
    pub message: String,
    /// AI 기반 수정 제안 (있는 경우)
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

    /// span이 없는 에러 생성 (런타임 에러용)
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

    /// ai_suggestion을 직접 지정하여 에러 생성
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

/// ErrorKind에서 AI 수정 제안 자동 생성
fn generate_suggestion(kind: &ErrorKind) -> Option<String> {
    match kind {
        ErrorKind::TypeMismatch {
            expected,
            found,
            field,
        } => Some(format!(
            "필드 '{}' 의 타입이 '{}' 가 아닌 '{}' 입니다. → 올바른 타입 '{}' 으로 변경하거나 cast() 를 사용하세요.",
            field, expected, found, expected
        )),
        ErrorKind::NullViolation { field, schema } => Some(format!(
            "스키마 '{}' 의 필수 필드 '{}' 에 null 값이 있습니다. → dropNull(\"{}\") 또는 fillNull(\"{}\", <기본값>) 을 파이프라인에 추가하세요.",
            schema, field, field, field
        )),
        ErrorKind::SafeLoadViolation {
            col,
            schema,
            available,
        } => {
            let hint = find_closest(col, available)
                .map(|s| format!("  Did you mean: col(\"{}\")?", s))
                .unwrap_or_default();
            Some(format!(
                "스키마 '{}' 에 '{}' 컬럼이 없습니다.\n💡 사용 가능한 컬럼: {}\n{}",
                schema,
                col,
                available.join(", "),
                hint
            ))
        }
        ErrorKind::UndeclaredVariable(name) => Some(format!(
            "변수 '{}' 가 선언되지 않았습니다. → 이 변수를 먼저 `v {} = ...` 으로 선언하세요.",
            name, name
        )),
        ErrorKind::UndeclaredType(name) => Some(format!(
            "타입 '{}' 가 선언되지 않았습니다. → `type {} = {{ ... }}` 으로 먼저 선언하세요.",
            name, name
        )),
        ErrorKind::DivisionByZero {
            col,
            row_count,
            expr_context: _,
        } => Some(format!(
            "컬럼 '{}' 에 0이 {} 개 포함되어 있습니다. → filter({} != 0) 또는 fillNull(\"{}\", 1) 등을 파이프라인에 추가하세요.",
            col, row_count, col, col
        )),
        _ => None,
    }
}

/// Levenshtein 편집 거리 기반 가장 가까운 후보 반환
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
            write!(f, "\n💡 AI Suggestion: {}", suggestion)?;
        }
        Ok(())
    }
}

impl std::error::Error for CompileError {}

/// 컴파일 결과 타입 별칭
pub type CompileResult<T> = Result<T, CompileError>;

// ── 에러 모듈 테스트 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(
            display.contains("💡 AI Suggestion:"),
            "AI Suggestion 출력 없음: {}",
            display
        );
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
        assert!(
            display.contains("💡 AI Suggestion:"),
            "AI Suggestion 없음: {}",
            display
        );
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
        assert!(
            display.contains("💡 AI Suggestion:"),
            "AI Suggestion 없음: {}",
            display
        );
        assert!(display.contains("pm25"), "컬럼명 포함 안 됨: {}", display);
        assert!(display.contains("3"), "0 개수 포함 안 됨: {}", display);
    }
}
