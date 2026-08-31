// xazz-compiler/src/policy/mod.rs — Policy-as-Code 정적 가드레일 (issue #2)
//
// 목적
//   `.xzz` 파이프라인이 **실행되기 전에** 개인정보(PII) 유출과 보안 컴플라이언스
//   위반을 정적으로 탐지하고 차단한다. Type Checker(checker.rs)가 "돌아가는가"를
//   묻는다면, 이 모듈은 "돌려도 되는가"를 묻는다.
//
// 설계 원칙
//   1. **Fail-closed** — 파싱 실패·스키마 미해석 등 "판단 불가"는 안전이 아니라
//      차단이다. `PolicyReport::safe_to_execute` 는 확신이 있을 때만 true 가 된다.
//   2. **정밀 우선** — 컬럼 분류는 부분 문자열이 아니라 정규화 후 완전 일치로만
//      한다. `message` 가 `age` 로, `sexagesimal` 이 `sex` 로 잡히면 안 된다.
//   3. **원본 미노출** — 탐지된 비밀값은 리포트에 마스킹해서만 싣는다.
//   4. **의존성 없음** — 이 크레이트는 CLI 바이너리에 링크된다(CONTRIBUTING.md).
//      정규식 크레이트조차 추가하지 않고 스캐너를 직접 구현한다.
//
// 배치 이유
//   xazz-compiler 는 Polars/Tokio 를 링크하지 않는 유일한 공용 크레이트이며
//   CLI(xazz) · 실행 엔진(xazz-exec) · API 서버(xazz-server) 모두가 의존한다.
//   따라서 여기에 두면 세 진입점 전부에 동일한 게이트를 걸 수 있다.

pub mod patterns;
pub mod printer;
pub mod remediate;
pub mod rules;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ast::Program;
use crate::{Lexer, Parser};
use xazz_core::i18n::{is_korean, tr};

pub use patterns::{LiteralFinding, SecretKind};
pub use printer::print_program;
pub use remediate::{AppliedFix, Remediation, remediate};

// ── 심각도 ───────────────────────────────────────────────────────────────────

/// 위반의 심각도. `Block` 만이 실행을 차단한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// 참고 사항 — 실행에 영향 없음
    Info,
    /// 경고 — 실행은 허용하되 리포트에 남긴다
    Warn,
    /// 차단 — 실행을 거부한다
    Block,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warn => "warn",
            Severity::Block => "block",
        }
    }
}

// ── 위험도 ───────────────────────────────────────────────────────────────────

/// 가명정보 처리 가이드라인의 위험도 구분(저·중·고위험)을 정책 속성으로 표현한다.
///
/// 위험도는 그 자체로 판정을 바꾸지 않는다. 감사 증빙에 "어떤 위험 등급의
/// 정책으로 차단되었는가"를 남기기 위한 메타데이터이며, 조직은 위험도에 따라
/// `quasi_identifier_threshold` 나 `max_epsilon` 을 다르게 설정한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

impl RiskLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            RiskLevel::Low => "low",
            RiskLevel::Medium => "medium",
            RiskLevel::High => "high",
        }
    }

    /// 표시 이름
    pub fn label(&self) -> &'static str {
        if is_korean() {
            match self {
                RiskLevel::Low => "저위험",
                RiskLevel::Medium => "중위험",
                RiskLevel::High => "고위험",
            }
        } else {
            match self {
                RiskLevel::Low => "low",
                RiskLevel::Medium => "medium",
                RiskLevel::High => "high",
            }
        }
    }
}

fn default_risk_level() -> RiskLevel {
    RiskLevel::Medium
}

fn default_domain() -> String {
    "common".to_string()
}

// ── 규제 근거 ────────────────────────────────────────────────────────────────

/// 규칙별 기본 규제 근거 — 감사 증빙(`source_ref`)에 기록된다.
///
/// ⚠️  법률 자문이 아니라 **감사 추적용 참조**다. 어떤 기준을 근거로 규칙을
///     만들었는지 남겨, 사후 감사에서 규칙의 출처를 따라갈 수 있게 한다.
///     실제 적용 법령은 조직·도메인마다 다르므로 정책 파일의
///     `rule_source_refs` 로 재정의하는 것을 전제로 한다.
pub fn default_source_ref(rule_id: &str) -> Option<&'static str> {
    match rule_id {
        RULE_DIRECT_IDENTIFIER => Some(
            "개인정보 보호법 제24조(고유식별정보의 처리 제한) · 제3조(개인정보 보호 원칙, 최소수집)",
        ),
        RULE_SENSITIVE_ROW_LEVEL => Some("개인정보 보호법 제23조(민감정보의 처리 제한)"),
        RULE_QUASI_COMBINATION => Some(
            "개인정보 보호법 제28조의5(가명정보 처리 시 금지의무 등) · 개인정보보호위원회 「가명정보 처리 가이드라인」 재식별 위험 평가",
        ),
        RULE_AGGREGATE_WITHOUT_DP => Some(
            "개인정보보호위원회 「가명정보 처리 가이드라인」 — 통계값 공개 시 재식별 위험 완화 조치",
        ),
        RULE_EPSILON_TOO_LARGE => {
            Some("개인정보보호위원회 「가명정보 처리 가이드라인」 — 프라이버시 보호 강도 기준")
        }
        RULE_PII_LITERAL => {
            Some("개인정보 보호법 제29조(안전조치의무) · 「개인정보의 안전성 확보조치 기준」")
        }
        RULE_HARDCODED_SECRET => Some(
            "「개인정보의 안전성 확보조치 기준」 제6조(접근 권한의 관리)·제7조(암호화) · 국가정보원 보안 기준",
        ),
        RULE_SENSITIVE_PATH | RULE_PATH_TRAVERSAL => {
            Some("「개인정보의 안전성 확보조치 기준」 제6조(접근 통제)")
        }
        RULE_UNRESOLVED_SCHEMA | RULE_PARSE_FAILED | RULE_POLICY_LOAD_FAILED => {
            Some("내부 통제 기준 — 검증되지 않은 처리 계획의 실행 금지 (fail-closed)")
        }
        _ => None,
    }
}

// ── 규칙 카탈로그 ────────────────────────────────────────────────────────────

/// 직접 식별자가 파이프라인 결과로 그대로 출력된다.
pub const RULE_DIRECT_IDENTIFIER: &str = "XZP001";
/// 민감 속성이 집계 없이 행 단위로 출력된다.
pub const RULE_SENSITIVE_ROW_LEVEL: &str = "XZP002";
/// 준식별자가 임계치 이상 결합되어 재식별 위험이 있다.
pub const RULE_QUASI_COMBINATION: &str = "XZP003";
/// 민감 속성 집계에 차등 프라이버시가 적용되지 않았다.
pub const RULE_AGGREGATE_WITHOUT_DP: &str = "XZP004";
/// 프라이버시 예산 ε 이 정책 상한을 넘는다.
pub const RULE_EPSILON_TOO_LARGE: &str = "XZP005";
/// 소스에 개인정보 리터럴이 하드코딩되어 있다.
pub const RULE_PII_LITERAL: &str = "XZP010";
/// 소스에 비밀키·자격증명이 하드코딩되어 있다.
pub const RULE_HARDCODED_SECRET: &str = "XZP011";
/// 시스템 민감 경로에 접근한다.
pub const RULE_SENSITIVE_PATH: &str = "XZP012";
/// 상위 디렉터리 탈출(`..`) 경로를 사용한다.
pub const RULE_PATH_TRAVERSAL: &str = "XZP013";
/// 스키마를 해석할 수 없어 안전성을 증명하지 못했다.
pub const RULE_UNRESOLVED_SCHEMA: &str = "XZP014";
/// 소스를 파싱할 수 없다 — fail-closed 로 차단한다.
pub const RULE_PARSE_FAILED: &str = "XZP000";
/// 정책 자체를 불러오지 못했다 — fail-closed 로 차단한다.
pub const RULE_POLICY_LOAD_FAILED: &str = "XZP999";

/// 규칙 ID → 사람이 읽는 규칙 이름.
pub fn rule_name(rule_id: &str) -> &'static str {
    match rule_id {
        RULE_PARSE_FAILED => "PARSE_FAILED",
        RULE_POLICY_LOAD_FAILED => "POLICY_LOAD_FAILED",
        RULE_DIRECT_IDENTIFIER => "DIRECT_IDENTIFIER_EXPOSED",
        RULE_SENSITIVE_ROW_LEVEL => "SENSITIVE_ATTRIBUTE_ROW_LEVEL",
        RULE_QUASI_COMBINATION => "QUASI_IDENTIFIER_COMBINATION",
        RULE_AGGREGATE_WITHOUT_DP => "AGGREGATE_WITHOUT_DP",
        RULE_EPSILON_TOO_LARGE => "DP_EPSILON_TOO_LARGE",
        RULE_PII_LITERAL => "PII_LITERAL_IN_SOURCE",
        RULE_HARDCODED_SECRET => "HARDCODED_SECRET",
        RULE_SENSITIVE_PATH => "SENSITIVE_PATH_ACCESS",
        RULE_PATH_TRAVERSAL => "PATH_TRAVERSAL",
        RULE_UNRESOLVED_SCHEMA => "UNRESOLVED_SCHEMA",
        _ => "UNKNOWN_RULE",
    }
}

// ── 위반 / 리포트 ────────────────────────────────────────────────────────────

/// 정책 위반 하나.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Violation {
    /// 규칙 ID (예: `XZP001`)
    pub rule_id: String,
    /// 규칙 이름 (예: `DIRECT_IDENTIFIER_EXPOSED`)
    pub rule_name: String,
    pub severity: Severity,
    /// 한국어 위반 사유
    pub message: String,
    /// 위반이 발생한 구문 인덱스 (0-base, `Program::stmts` 기준)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statement_index: Option<usize>,
    /// 위반이 귀속되는 변수명
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variable: Option<String>,
    /// 문제가 된 컬럼들
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<String>,
    /// 1-base 줄 번호 (리터럴 규칙에서만 정확, 그 외 0)
    pub line: usize,
    /// 1-base 칼럼 번호
    pub col: usize,
    /// 개발자에게 제시하는 보정 방향
    pub remediation_hint: String,
    /// 규제 근거 — 감사 증빙용 참조 (법률 자문 아님)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
}

impl Violation {
    fn new(
        rule_id: &str,
        severity: Severity,
        message: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Violation {
            rule_id: rule_id.to_string(),
            rule_name: rule_name(rule_id).to_string(),
            severity,
            message: message.into(),
            statement_index: None,
            variable: None,
            columns: Vec::new(),
            line: 0,
            col: 0,
            remediation_hint: hint.into(),
            source_ref: None,
        }
    }

    fn at_stmt(mut self, index: usize, variable: Option<&str>) -> Self {
        self.statement_index = Some(index);
        self.variable = variable.map(|s| s.to_string());
        self
    }

    fn with_columns(mut self, columns: Vec<String>) -> Self {
        self.columns = columns;
        self
    }

    fn at_position(mut self, line: usize, col: usize) -> Self {
        self.line = line;
        self.col = col;
        self
    }
}

/// 정적 가드레일 분석 결과.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyReport {
    pub policy_id: String,
    pub policy_version: String,
    /// 적용 도메인 (`common` · `healthcare` · `finance` · `public-sector` …)
    #[serde(default = "default_domain")]
    pub domain: String,
    /// 정책의 위험도 등급 — 감사 증빙용
    #[serde(default = "default_risk_level")]
    pub risk_level: RiskLevel,
    /// 이 값이 true 일 때만 실행을 허용한다.
    pub safe_to_execute: bool,
    /// 심각도 `block` 인 위반 — 하나라도 있으면 차단.
    pub violations: Vec<Violation>,
    /// 심각도 `warn`/`info` 인 항목 — 실행은 허용한다.
    pub warnings: Vec<Violation>,
    /// 검사한 구문 수
    pub scanned_statements: usize,
    /// 파싱 실패 사유 (있으면 fail-closed 로 차단)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse_error: Option<String>,
}

impl PolicyReport {
    /// 차단 사유를 한 줄 요약으로 만든다.
    pub fn summary(&self) -> String {
        if self.safe_to_execute {
            if self.warnings.is_empty() {
                tr(
                    "policy check passed — no violations",
                    "정책 검사 통과 — 위반 없음",
                )
                .to_string()
            } else {
                if is_korean() {
                    format!("정책 검사 통과 — 경고 {}건", self.warnings.len())
                } else {
                    format!("policy check passed — {} warning(s)", self.warnings.len())
                }
            }
        } else {
            let ids: Vec<&str> = self.violations.iter().map(|v| v.rule_id.as_str()).collect();
            if is_korean() {
                format!(
                    "정책 위반 {}건으로 실행이 차단되었습니다 [{}]",
                    self.violations.len(),
                    ids.join(", ")
                )
            } else {
                format!(
                    "execution blocked by {} policy violation(s) [{}]",
                    self.violations.len(),
                    ids.join(", ")
                )
            }
        }
    }

    /// 사람이 읽는 여러 줄 리포트.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "정책: {} v{}\n{}\n",
            self.policy_id,
            self.policy_version,
            self.summary()
        ));
        for v in &self.violations {
            out.push_str(&format!(
                "  [차단] {} {} — {}\n         보정: {}\n",
                v.rule_id, v.rule_name, v.message, v.remediation_hint
            ));
        }
        for w in &self.warnings {
            out.push_str(&format!(
                "  [경고] {} {} — {}\n",
                w.rule_id, w.rule_name, w.message
            ));
        }
        out
    }
}

// ── 컬럼 분류 ────────────────────────────────────────────────────────────────

/// 컬럼이 프라이버시 관점에서 어떤 성격인지.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnClass {
    /// 단독으로 개인을 특정한다 (이름·주민등록번호·환자번호 …)
    DirectIdentifier,
    /// 그 자체로 민감하다 (진단명·소득·종교 …)
    SensitiveAttribute,
    /// 조합되면 재식별 위험이 있다 (나이·성별·우편번호 …)
    QuasiIdentifier,
    /// 정책상 분류되지 않음
    Unclassified,
}

/// 컬럼명을 비교용으로 정규화한다.
///
/// 소문자화 후 영숫자·한글만 남긴다. `patient_id`, `patientID`, `Patient-Id`
/// 가 모두 `patientid` 로 정규화되어 표기 방식과 무관하게 일치한다.
pub fn normalize_column(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

// ── 정책 ─────────────────────────────────────────────────────────────────────

/// Policy-as-Code 정책 문서.
///
/// JSON 으로 직렬화·역직렬화되며, 조직은 이 파일 하나만 교체해서
/// 가드레일 동작을 바꿀 수 있다.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub id: String,
    pub version: String,
    #[serde(default)]
    pub description: String,

    /// 직접 식별자 컬럼명 목록
    #[serde(default)]
    pub direct_identifiers: Vec<String>,
    /// 민감 속성 컬럼명 목록
    #[serde(default)]
    pub sensitive_attributes: Vec<String>,
    /// 준식별자 컬럼명 목록
    #[serde(default)]
    pub quasi_identifiers: Vec<String>,

    /// 준식별자가 몇 개 이상 함께 출력되면 위반으로 볼지
    #[serde(default = "default_quasi_threshold")]
    pub quasi_identifier_threshold: usize,
    /// 민감 속성 집계에 withDp 를 강제할지
    #[serde(default = "default_true")]
    pub require_dp_for_sensitive_aggregate: bool,
    /// 허용하는 최대 프라이버시 예산 ε
    #[serde(default = "default_max_epsilon")]
    pub max_epsilon: f64,
    /// 자동 보정이 삽입하는 기본 ε
    #[serde(default = "default_remediation_epsilon")]
    pub remediation_epsilon: f64,

    /// 분류에도 불구하고 출력을 허용할 컬럼 (allowlist)
    #[serde(default)]
    pub allowed_output_columns: Vec<String>,
    /// load() 경로에 포함되면 차단할 문자열
    #[serde(default)]
    pub denied_path_fragments: Vec<String>,

    /// 규칙별 심각도 재정의 (예: `{"XZP013": "block"}`)
    #[serde(default)]
    pub rule_severity: BTreeMap<String, Severity>,

    /// 적용 도메인 — 감사 증빙에 기록된다 (`healthcare` · `finance` · `public-sector` …)
    #[serde(default = "default_domain")]
    pub domain: String,
    /// 정책의 위험도 등급 (가명정보 처리 가이드라인의 저·중·고위험 구분)
    #[serde(default = "default_risk_level")]
    pub risk_level: RiskLevel,
    /// 규칙별 규제 근거 재정의 — 조직·도메인마다 적용 법령이 다르다
    #[serde(default)]
    pub rule_source_refs: BTreeMap<String, String>,
}

fn default_quasi_threshold() -> usize {
    3
}
fn default_true() -> bool {
    true
}
fn default_max_epsilon() -> f64 {
    3.0
}
fn default_remediation_epsilon() -> f64 {
    1.0
}

/// 정책 로딩 실패 — 호출자는 반드시 fail-closed 로 처리해야 한다.
#[derive(Debug, Clone)]
pub struct PolicyError {
    pub message: String,
}

impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for PolicyError {}

impl Policy {
    /// 정책 JSON 문자열을 파싱한다.
    ///
    /// 실패하면 `Err` 를 돌려준다. 호출자는 이를 "정책 없음 = 허용" 이 아니라
    /// "검증 불가 = 차단" 으로 다뤄야 한다.
    pub fn from_json_str(text: &str) -> Result<Policy, PolicyError> {
        let policy: Policy = serde_json::from_str(text).map_err(|e| PolicyError {
            message: format!("정책 JSON 파싱 실패: {}", e),
        })?;
        policy.validate()?;
        Ok(policy)
    }

    /// 정책 자체의 정합성을 검증한다.
    pub fn validate(&self) -> Result<(), PolicyError> {
        if self.id.trim().is_empty() {
            return Err(PolicyError {
                message: "정책 id 가 비어 있습니다.".to_string(),
            });
        }
        if self.max_epsilon <= 0.0 || !self.max_epsilon.is_finite() {
            return Err(PolicyError {
                message: format!(
                    "max_epsilon 은 0 보다 큰 유한값이어야 합니다: {}",
                    self.max_epsilon
                ),
            });
        }
        if self.remediation_epsilon <= 0.0 || self.remediation_epsilon > self.max_epsilon {
            return Err(PolicyError {
                message: format!(
                    "remediation_epsilon({}) 은 0 초과이며 max_epsilon({}) 이하여야 합니다.",
                    self.remediation_epsilon, self.max_epsilon
                ),
            });
        }
        if self.quasi_identifier_threshold == 0 {
            return Err(PolicyError {
                message: "quasi_identifier_threshold 는 1 이상이어야 합니다.".to_string(),
            });
        }
        Ok(())
    }

    /// 정책을 JSON 문자열로 직렬화한다.
    pub fn to_json_string(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// 컬럼을 분류한다. allowlist 에 있으면 항상 `Unclassified` 다.
    pub fn classify(&self, column: &str) -> ColumnClass {
        let key = normalize_column(column);
        if key.is_empty() {
            return ColumnClass::Unclassified;
        }
        let contains = |list: &[String]| list.iter().any(|c| normalize_column(c) == key);

        if contains(&self.allowed_output_columns) {
            return ColumnClass::Unclassified;
        }
        if contains(&self.direct_identifiers) {
            return ColumnClass::DirectIdentifier;
        }
        if contains(&self.sensitive_attributes) {
            return ColumnClass::SensitiveAttribute;
        }
        if contains(&self.quasi_identifiers) {
            return ColumnClass::QuasiIdentifier;
        }
        ColumnClass::Unclassified
    }

    /// 규칙의 유효 심각도 — 정책의 재정의가 있으면 그것을 따른다.
    pub fn severity_for(&self, rule_id: &str, default: Severity) -> Severity {
        self.rule_severity.get(rule_id).copied().unwrap_or(default)
    }

    /// 내장 기본 정책 — 한국 개인정보보호법·의료법 맥락의 보수적 기본값.
    ///
    /// 컬럼명은 정규화 후 **완전 일치**로만 비교되므로,
    /// `region`·`station`·`case_id` 같은 일반 컬럼은 걸리지 않는다.
    pub fn builtin() -> Policy {
        let s = |items: &[&str]| items.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        Policy {
            id: "xazz-builtin-pii".to_string(),
            version: "1.0.0".to_string(),
            description: "Xazz 내장 기본 정책 — 개인정보 직접 노출·재식별·비밀키 유출 차단"
                .to_string(),
            direct_identifiers: s(&[
                // 이름
                "name",
                "full_name",
                "first_name",
                "last_name",
                "성명",
                "이름",
                "성함",
                // 고유 식별번호
                "ssn",
                "rrn",
                "resident_registration_number",
                "주민등록번호",
                "주민번호",
                "passport_no",
                "passport_number",
                "여권번호",
                "driver_license",
                "driver_license_no",
                "운전면허번호",
                "foreigner_registration_number",
                "외국인등록번호",
                // 연락처
                "phone",
                "phone_number",
                "mobile",
                "mobile_phone",
                "telephone",
                "tel",
                "cell_phone",
                "전화번호",
                "휴대전화",
                "연락처",
                "email",
                "email_address",
                "이메일",
                "address",
                "home_address",
                "street_address",
                "주소",
                "상세주소",
                // 금융
                "account_no",
                "account_number",
                "bank_account",
                "계좌번호",
                "card_no",
                "card_number",
                "credit_card",
                "카드번호",
                // 의료·조직 내 식별자
                "patient_id",
                "patient_no",
                "mrn",
                "chart_no",
                "환자번호",
                "차트번호",
                "employee_id",
                "사번",
                "user_id",
                "customer_id",
                "member_id",
                // 기기·네트워크
                "ip",
                "ip_address",
                "mac_address",
                "device_id",
                "imei",
            ]),
            sensitive_attributes: s(&[
                "disease",
                "diagnosis",
                "diagnosis_code",
                "진단명",
                "병명",
                "질병",
                "medication",
                "prescription",
                "처방",
                "약물",
                "blood_type",
                "혈액형",
                "genetic_info",
                "dna",
                "medical_history",
                "병력",
                "health_status",
                "건강상태",
                "disability",
                "장애",
                "mental_health",
                "정신건강",
                "pregnancy",
                "임신",
                "hiv_status",
                "salary",
                "income",
                "annual_income",
                "급여",
                "연봉",
                "소득",
                "credit_score",
                "신용등급",
                "신용점수",
                "religion",
                "종교",
                "political_view",
                "political_affiliation",
                "정치성향",
                "sexual_orientation",
                "성적지향",
                "criminal_record",
                "범죄경력",
                "전과",
                "union_membership",
                "노조가입여부",
            ]),
            quasi_identifiers: s(&[
                "age",
                "나이",
                "연령",
                "birth_date",
                "birthday",
                "date_of_birth",
                "dob",
                "생년월일",
                "생일",
                "gender",
                "sex",
                "성별",
                "zip_code",
                "postal_code",
                "post_code",
                "우편번호",
                "nationality",
                "국적",
                "occupation",
                "job",
                "직업",
                "marital_status",
                "결혼여부",
                "ethnicity",
                "race",
                "인종",
            ]),
            quasi_identifier_threshold: default_quasi_threshold(),
            require_dp_for_sensitive_aggregate: true,
            max_epsilon: default_max_epsilon(),
            remediation_epsilon: default_remediation_epsilon(),
            allowed_output_columns: Vec::new(),
            denied_path_fragments: s(&[
                "/etc/passwd",
                "/etc/shadow",
                "/etc/sudoers",
                "/proc/self/environ",
                "/.ssh/",
                "id_rsa",
                "id_ed25519",
                ".aws/credentials",
                ".kube/config",
            ]),
            rule_severity: BTreeMap::new(),
            domain: default_domain(),
            risk_level: RiskLevel::Medium,
            rule_source_refs: BTreeMap::new(),
        }
    }

    /// 규칙의 규제 근거 — 정책의 재정의가 있으면 그것을, 없으면 내장 기본값을 쓴다.
    pub fn source_ref_for(&self, rule_id: &str) -> Option<String> {
        self.rule_source_refs
            .get(rule_id)
            .cloned()
            .or_else(|| default_source_ref(rule_id).map(|s| s.to_string()))
    }
}

impl Default for Policy {
    fn default() -> Self {
        Policy::builtin()
    }
}

// ── 활성 정책 로딩 (fail-closed) ─────────────────────────────────────────────

/// 정책 파일을 찾을 때 사용하는 환경변수 이름.
pub const POLICY_PATH_ENV: &str = "XAZZ_POLICY_PATH";
/// 프로젝트 루트에서 자동으로 찾는 정책 파일 이름.
pub const DEFAULT_POLICY_FILE: &str = "xazz.policy.json";

/// 어떤 정책이 어떻게 적용되었는지.
#[derive(Debug, Clone)]
pub struct ActivePolicy {
    pub policy: Policy,
    /// 정책 출처 — 파일 경로 또는 `"builtin"`
    pub origin: String,
}

/// 실행에 적용할 정책을 결정한다.
///
/// 우선순위
///   1. 환경변수 `XAZZ_POLICY_PATH` — 지정되었으면 **반드시** 로딩에 성공해야 한다.
///   2. 작업 디렉터리의 `xazz.policy.json` — 존재하면 **반드시** 로딩에 성공해야 한다.
///   3. 둘 다 없으면 내장 기본 정책.
///
/// 1·2 에서 파일이 깨져 있으면 `Err` 를 돌려준다. "정책을 못 읽었으니 그냥
/// 실행한다"는 것은 가드레일의 존재 이유를 무너뜨리므로, 호출자는 이 오류를
/// 반드시 **실행 거부**로 처리해야 한다.
pub fn load_active_policy() -> Result<ActivePolicy, PolicyError> {
    if let Some(path) = std::env::var_os(POLICY_PATH_ENV) {
        let path = std::path::PathBuf::from(path);
        if path.as_os_str().is_empty() {
            return Ok(ActivePolicy {
                policy: Policy::builtin(),
                origin: "builtin".to_string(),
            });
        }
        return load_policy_file(&path);
    }

    let default_path = std::path::Path::new(DEFAULT_POLICY_FILE);
    if default_path.is_file() {
        return load_policy_file(default_path);
    }

    Ok(ActivePolicy {
        policy: Policy::builtin(),
        origin: "builtin".to_string(),
    })
}

/// 지정한 경로의 정책 파일을 읽는다. 실패는 항상 오류다 (fail-closed).
pub fn load_policy_file(path: &std::path::Path) -> Result<ActivePolicy, PolicyError> {
    let text = std::fs::read_to_string(path).map_err(|e| PolicyError {
        message: format!("정책 파일을 읽을 수 없습니다 '{}': {}", path.display(), e),
    })?;
    let policy = Policy::from_json_str(&text).map_err(|e| PolicyError {
        message: format!(
            "정책 파일 '{}' 이 올바르지 않습니다: {}",
            path.display(),
            e.message
        ),
    })?;
    Ok(ActivePolicy {
        policy,
        origin: path.display().to_string(),
    })
}

/// 정책 로딩 실패를 그대로 차단 리포트로 바꾼다.
///
/// 호출자는 이 리포트를 사용자에게 그대로 보여 주면 된다.
pub fn policy_load_failure_report(error: &PolicyError) -> PolicyReport {
    PolicyReport {
        policy_id: "POLICY_LOAD_FAILED".to_string(),
        policy_version: "-".to_string(),
        domain: default_domain(),
        risk_level: RiskLevel::High,
        safe_to_execute: false,
        violations: vec![Violation::new(
            RULE_POLICY_LOAD_FAILED,
            Severity::Block,
            format!(
                "보안 정책을 불러오지 못해 실행을 거부했습니다: {}",
                error.message
            ),
            "정책 파일의 JSON 구문을 고치거나, XAZZ_POLICY_PATH 를 올바른 경로로 지정하세요.",
        )],
        warnings: Vec::new(),
        scanned_statements: 0,
        parse_error: None,
    }
}

// ── 공개 진입점 ──────────────────────────────────────────────────────────────

/// `.xzz` 소스를 렉싱·파싱한 뒤 정책을 적용한다.
///
/// 파싱에 실패해도 `PolicyReport` 를 돌려준다 — 이때 `safe_to_execute` 는
/// 항상 false 이고 `parse_error` 가 채워진다(fail-closed). 리터럴 스캔은
/// 파싱 실패와 무관하게 수행되므로, 구문 오류가 있는 코드에 숨은 비밀키도
/// 함께 보고된다.
pub fn analyze(source: &str, policy: &Policy) -> PolicyReport {
    let parsed = Lexer::new(source)
        .tokenize()
        .map_err(|e| format!("[LEXER] {}", e))
        .and_then(|tokens| {
            Parser::new(tokens)
                .parse()
                .map_err(|e| format!("[PARSER] {}", e))
        });

    match parsed {
        Ok(program) => analyze_parsed(&program, source, policy),
        Err(message) => {
            let mut report = PolicyReport {
                policy_id: policy.id.clone(),
                policy_version: policy.version.clone(),
                domain: policy.domain.clone(),
                risk_level: policy.risk_level,
                safe_to_execute: false,
                violations: vec![Violation::new(
                    RULE_PARSE_FAILED,
                    Severity::Block,
                    format!(
                        "소스를 파싱할 수 없어 안전성을 검증하지 못했습니다: {}",
                        message
                    ),
                    "구문 오류를 수정한 뒤 다시 검사하세요. 검증되지 않은 코드는 실행하지 않습니다.",
                )],
                warnings: Vec::new(),
                scanned_statements: 0,
                parse_error: Some(message),
            };
            // 파싱이 실패해도 텍스트 스캔은 유효하다.
            rules::apply_literal_rules(source, policy, &mut report);
            attach_source_refs(&mut report, policy);
            report.safe_to_execute = false;
            report
        }
    }
}

/// 이미 파싱된 AST 에 정책을 적용한다.
///
/// `source` 는 리터럴 스캔(주석 포함)과 줄 번호 계산에 쓰인다.
pub fn analyze_parsed(program: &Program, source: &str, policy: &Policy) -> PolicyReport {
    let mut report = PolicyReport {
        policy_id: policy.id.clone(),
        policy_version: policy.version.clone(),
        domain: policy.domain.clone(),
        risk_level: policy.risk_level,
        safe_to_execute: true,
        violations: Vec::new(),
        warnings: Vec::new(),
        scanned_statements: program.stmts.len(),
        parse_error: None,
    };

    rules::apply_ast_rules(program, policy, &mut report);
    rules::apply_literal_rules(source, policy, &mut report);
    attach_source_refs(&mut report, policy);

    report.safe_to_execute = report.violations.is_empty();
    report
}

/// 모든 위반에 규제 근거를 붙인다.
///
/// 규칙 판정부가 근거 문자열을 알 필요는 없으므로, 판정이 끝난 뒤 한 곳에서
/// 채운다. 감사 증빙(`rule_id` · `source_ref` · `policy_version` · `domain` ·
/// `risk_level`)이 리포트 하나에 모두 담기게 된다.
fn attach_source_refs(report: &mut PolicyReport, policy: &Policy) {
    for v in report
        .violations
        .iter_mut()
        .chain(report.warnings.iter_mut())
    {
        if v.source_ref.is_none() {
            v.source_ref = policy.source_ref_for(&v.rule_id);
        }
    }
}

/// 위반을 심각도에 맞는 목록에 넣는다.
pub(crate) fn record(report: &mut PolicyReport, violation: Violation) {
    match violation.severity {
        Severity::Block => report.violations.push(violation),
        Severity::Warn | Severity::Info => report.warnings.push(violation),
    }
}

// ── 유닛 테스트 ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// 정규화는 표기 방식(스네이크·카멜·하이픈)을 흡수한다.
    #[test]
    fn normalizes_column_spellings() {
        assert_eq!(normalize_column("patient_id"), "patientid");
        assert_eq!(normalize_column("patientID"), "patientid");
        assert_eq!(normalize_column("Patient-Id"), "patientid");
        assert_eq!(normalize_column("주민등록번호"), "주민등록번호");
    }

    /// 분류는 완전 일치다 — 부분 문자열로 오탐하지 않는다.
    #[test]
    fn classification_is_exact_not_substring() {
        let p = Policy::builtin();
        assert_eq!(p.classify("age"), ColumnClass::QuasiIdentifier);
        assert_eq!(p.classify("message"), ColumnClass::Unclassified);
        assert_eq!(p.classify("average"), ColumnClass::Unclassified);
        assert_eq!(p.classify("sex"), ColumnClass::QuasiIdentifier);
        assert_eq!(p.classify("sexagesimal"), ColumnClass::Unclassified);
        assert_eq!(p.classify("name"), ColumnClass::DirectIdentifier);
        assert_eq!(p.classify("station_name"), ColumnClass::Unclassified);
    }

    /// 기존 예제에 등장하는 일반 컬럼은 절대 분류되지 않는다 (오탐 회귀 방지).
    #[test]
    fn common_example_columns_are_unclassified() {
        let p = Policy::builtin();
        for col in [
            "date", "station", "pm10", "pm25", "region", "case_id", "count", "adm_code",
        ] {
            assert_eq!(
                p.classify(col),
                ColumnClass::Unclassified,
                "'{}' 이 잘못 분류되었습니다",
                col
            );
        }
    }

    /// allowlist 는 분류를 무력화한다.
    #[test]
    fn allowlist_overrides_classification() {
        let mut p = Policy::builtin();
        assert_eq!(p.classify("age"), ColumnClass::QuasiIdentifier);
        p.allowed_output_columns.push("age".to_string());
        assert_eq!(p.classify("age"), ColumnClass::Unclassified);
    }

    /// 정책 JSON 왕복.
    #[test]
    fn policy_json_round_trip() {
        let original = Policy::builtin();
        let text = original.to_json_string();
        let parsed = Policy::from_json_str(&text).expect("왕복 파싱 실패");
        assert_eq!(parsed.id, original.id);
        assert_eq!(
            parsed.direct_identifiers.len(),
            original.direct_identifiers.len()
        );
    }

    /// 깨진 정책 JSON 은 반드시 Err 다 (fail-closed 의 전제).
    #[test]
    fn broken_policy_json_is_rejected() {
        assert!(Policy::from_json_str("{ not json").is_err());
        assert!(
            Policy::from_json_str("{}").is_err(),
            "id 없는 정책은 거부되어야 한다"
        );
    }

    /// 정책 정합성 검증 — 잘못된 ε 조합은 거부한다.
    #[test]
    fn invalid_epsilon_configuration_is_rejected() {
        let mut p = Policy::builtin();
        p.max_epsilon = 0.0;
        assert!(p.validate().is_err());

        let mut p = Policy::builtin();
        p.remediation_epsilon = 99.0;
        assert!(p.validate().is_err());
    }

    /// 규칙 심각도 재정의가 반영된다.
    #[test]
    fn rule_severity_override_applies() {
        let mut p = Policy::builtin();
        assert_eq!(
            p.severity_for(RULE_PATH_TRAVERSAL, Severity::Warn),
            Severity::Warn
        );
        p.rule_severity
            .insert(RULE_PATH_TRAVERSAL.to_string(), Severity::Block);
        assert_eq!(
            p.severity_for(RULE_PATH_TRAVERSAL, Severity::Warn),
            Severity::Block
        );
    }

    /// 파싱 불가 소스는 fail-closed 로 차단된다.
    #[test]
    fn unparseable_source_is_blocked() {
        let report = analyze("v x = |>|> ???", &Policy::builtin());
        assert!(!report.safe_to_execute);
        assert!(report.parse_error.is_some());
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.rule_id == RULE_PARSE_FAILED)
        );
    }

    /// 파싱에 실패해도 숨은 비밀키는 함께 보고된다.
    #[test]
    fn unparseable_source_still_reports_secrets() {
        let report = analyze("v x = ??? // AKIAIOSFODNN7EXAMPLE", &Policy::builtin());
        assert!(!report.safe_to_execute);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.rule_id == RULE_HARDCODED_SECRET),
            "비밀키 미보고: {:?}",
            report.violations
        );
    }

    /// 모든 위반에 규제 근거가 붙는다 (감사 증빙 요건).
    #[test]
    fn violations_carry_regulatory_source_ref() {
        let src = "type P = { name: string, age_band: string };\n\
                   v x = load(\"d.csv\") :: P |> select([name, age_band]);";
        let report = analyze(src, &Policy::builtin());
        assert!(!report.violations.is_empty());
        for v in &report.violations {
            assert!(
                v.source_ref.is_some(),
                "규칙 {} 에 규제 근거가 없습니다 — 감사 증빙이 불완전합니다",
                v.rule_id
            );
        }
    }

    /// 정책이 규제 근거를 재정의하면 그 값이 우선한다.
    #[test]
    fn policy_can_override_source_ref() {
        let mut policy = Policy::builtin();
        policy.rule_source_refs.insert(
            RULE_DIRECT_IDENTIFIER.to_string(),
            "사내 데이터 취급 지침 3.2".to_string(),
        );
        let src = "type P = { name: string, age_band: string };\n\
                   v x = load(\"d.csv\") :: P |> select([name, age_band]);";
        let report = analyze(src, &policy);
        let v = report
            .violations
            .iter()
            .find(|v| v.rule_id == RULE_DIRECT_IDENTIFIER)
            .expect("XZP001 미탐지");
        assert_eq!(v.source_ref.as_deref(), Some("사내 데이터 취급 지침 3.2"));
    }

    /// 리포트가 도메인·위험도를 실어 나른다 (감사 증빙 요건).
    #[test]
    fn report_carries_domain_and_risk_level() {
        let mut policy = Policy::builtin();
        policy.domain = "healthcare".to_string();
        policy.risk_level = RiskLevel::High;
        let report = analyze(
            "type P = { a: string };\nv x = load(\"d.csv\") :: P;",
            &policy,
        );
        assert_eq!(report.domain, "healthcare");
        assert_eq!(report.risk_level, RiskLevel::High);
    }

    /// 정책 로딩 실패는 고위험으로 기록된다.
    #[test]
    fn policy_load_failure_is_high_risk() {
        let err = PolicyError {
            message: "테스트".to_string(),
        };
        let report = policy_load_failure_report(&err);
        assert!(!report.safe_to_execute);
        assert_eq!(report.risk_level, RiskLevel::High);
    }

    /// 도메인 정책 팩 3종이 모두 유효하게 로딩된다.
    #[test]
    fn shipped_domain_policy_packs_are_valid() {
        for name in [
            "healthcare_policy.json",
            "finance_policy.json",
            "public_sector_policy.json",
        ] {
            let path = std::path::Path::new("../examples/security").join(name);
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{} 읽기 실패: {}", path.display(), e));
            let policy = Policy::from_json_str(&text)
                .unwrap_or_else(|e| panic!("{} 파싱 실패: {}", name, e.message));
            assert_ne!(policy.domain, "common", "{} 에 domain 이 없습니다", name);
            assert!(
                !policy.direct_identifiers.is_empty(),
                "{} 에 직접 식별자 목록이 없습니다",
                name
            );
        }
    }

    /// 심각도 순서 — Block 이 가장 높다.
    #[test]
    fn severity_ordering() {
        assert!(Severity::Block > Severity::Warn);
        assert!(Severity::Warn > Severity::Info);
    }
}
