// xazz-compiler/src/policy/mod.rs — Policy-as-Code static guardrail (issue #2)
//
// Purpose
//   Statically detects and blocks PII leakage and security compliance violations
//   before the `.xzz` pipeline **runs**. If the Type Checker (checker.rs) asks "does it work?",
//   this module asks "is it safe to run?".
//
// Design principles
//   1. **Fail-closed** — "cannot judge" cases (parse failure, unresolved schema, etc.) are
//      blocked rather than assumed safe. `PolicyReport::safe_to_execute` is only true when certain.
//   2. **Precision first** — column classification uses exact normalized matches, not
//      substrings. `message` must not be caught as `age`, nor `sexagesimal` as `sex`.
//   3. **No raw exposure** — detected secrets are only reported masked.
//   4. **No dependencies** — this crate is linked into the CLI binary (CONTRIBUTING.md).
//      The scanner is implemented by hand without even adding a regex crate.
//
// Placement rationale
//   xazz-compiler is the only shared crate that does not link Polars/Tokio, and
//   the CLI (xazz), the execution engine (xazz-exec), and the API server (xazz-server)
//   all depend on it. Placing the gate here applies the same checks to all three entry points.

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

// ── Severity ─────────────────────────────────────────────────────────────────

/// Severity of a violation. Only `Block` stops execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Informational — no effect on execution
    Info,
    /// Warning — execution is allowed but recorded in the report
    Warn,
    /// Block — execution is refused
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

// ── Risk level ───────────────────────────────────────────────────────────────

/// Expresses the risk tiers (low/medium/high) of the pseudonymized-data handling
/// guidelines as a policy attribute.
///
/// Risk level does not by itself change the judgment. It is metadata that records
/// "which risk tier's policy blocked this" for audit evidence; organizations configure
/// `quasi_identifier_threshold` or `max_epsilon` differently per risk tier.
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

    /// Display name
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

// ── Regulatory basis ─────────────────────────────────────────────────────────

/// Default regulatory basis per rule — recorded in the audit evidence (`source_ref`).
///
/// ⚠️  This is **an audit-trail reference, not legal advice**. It records which standard
///     a rule was based on so the rule's origin can be traced in a later audit.
///     The actually applicable law varies by organization and domain, so it is meant to
///     be overridden via `rule_source_refs` in the policy file.
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

// ── Rule catalog ─────────────────────────────────────────────────────────────

/// A direct identifier is emitted as-is in the pipeline result.
pub const RULE_DIRECT_IDENTIFIER: &str = "XZP001";
/// A sensitive attribute is emitted row-wise without aggregation.
pub const RULE_SENSITIVE_ROW_LEVEL: &str = "XZP002";
/// Quasi-identifiers are combined at or above the threshold, creating re-identification risk.
pub const RULE_QUASI_COMBINATION: &str = "XZP003";
/// No differential privacy is applied to the sensitive-attribute aggregate.
pub const RULE_AGGREGATE_WITHOUT_DP: &str = "XZP004";
/// The privacy budget ε exceeds the policy cap.
pub const RULE_EPSILON_TOO_LARGE: &str = "XZP005";
/// A PII literal is hardcoded in the source.
pub const RULE_PII_LITERAL: &str = "XZP010";
/// A secret key or credential is hardcoded in the source.
pub const RULE_HARDCODED_SECRET: &str = "XZP011";
/// A system-sensitive path is accessed.
pub const RULE_SENSITIVE_PATH: &str = "XZP012";
/// A parent-directory traversal (`..`) path is used.
pub const RULE_PATH_TRAVERSAL: &str = "XZP013";
/// The schema cannot be resolved, so safety cannot be proven.
pub const RULE_UNRESOLVED_SCHEMA: &str = "XZP014";
/// The source cannot be parsed — blocked fail-closed.
pub const RULE_PARSE_FAILED: &str = "XZP000";
/// The policy itself could not be loaded — blocked fail-closed.
pub const RULE_POLICY_LOAD_FAILED: &str = "XZP999";

/// Rule ID → human-readable rule name.
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

// ── Violation / report ───────────────────────────────────────────────────────

/// One policy violation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Violation {
    /// Rule ID (e.g. `XZP001`)
    pub rule_id: String,
    /// Rule name (e.g. `DIRECT_IDENTIFIER_EXPOSED`)
    pub rule_name: String,
    pub severity: Severity,
    /// Korean violation reason
    pub message: String,
    /// Index of the statement where the violation occurred (0-based, relative to `Program::stmts`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statement_index: Option<usize>,
    /// Variable the violation is attributed to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variable: Option<String>,
    /// The columns involved
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<String>,
    /// 1-based line number (only accurate for literal rules, otherwise 0)
    pub line: usize,
    /// 1-based column number
    pub col: usize,
    /// Remediation direction presented to the developer
    pub remediation_hint: String,
    /// Regulatory basis — reference for audit evidence (not legal advice)
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

/// Result of the static guardrail analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyReport {
    pub policy_id: String,
    pub policy_version: String,
    /// Applied domain (`common` · `healthcare` · `finance` · `public-sector` …)
    #[serde(default = "default_domain")]
    pub domain: String,
    /// Risk tier of the policy — for audit evidence
    #[serde(default = "default_risk_level")]
    pub risk_level: RiskLevel,
    /// Execution is only allowed when this value is true.
    pub safe_to_execute: bool,
    /// Violations with severity `block` — any one of them blocks.
    pub violations: Vec<Violation>,
    /// Items with severity `warn`/`info` — execution is allowed.
    pub warnings: Vec<Violation>,
    /// Number of statements scanned
    pub scanned_statements: usize,
    /// Reason for parse failure (if present, blocks fail-closed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse_error: Option<String>,
}

impl PolicyReport {
    /// Summarizes the block reason into a single line.
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

    /// Human-readable multi-line report.
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

// ── Column classification ────────────────────────────────────────────────────

/// What kind of privacy-relevant role a column plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnClass {
    /// Identifies an individual on its own (name, resident registration number, patient number …)
    DirectIdentifier,
    /// Sensitive in itself (diagnosis, income, religion …)
    SensitiveAttribute,
    /// Re-identification risk when combined (age, gender, zip code …)
    QuasiIdentifier,
    /// Not classified by policy
    Unclassified,
}

/// Normalizes a column name for comparison.
///
/// Lowercases and keeps only alphanumeric characters. `patient_id`, `patientID`, and
/// `Patient-Id` all normalize to `patientid`, matching regardless of spelling style.
pub fn normalize_column(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

// ── Policy ───────────────────────────────────────────────────────────────────

/// Policy-as-Code policy document.
///
/// Serialized/deserialized as JSON; an organization can change guardrail behavior by
/// replacing this single file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub id: String,
    pub version: String,
    #[serde(default)]
    pub description: String,

    /// Direct identifier column names
    #[serde(default)]
    pub direct_identifiers: Vec<String>,
    /// Sensitive attribute column names
    #[serde(default)]
    pub sensitive_attributes: Vec<String>,
    /// Quasi-identifier column names
    #[serde(default)]
    pub quasi_identifiers: Vec<String>,

    /// How many quasi-identifiers emitted together count as a violation
    #[serde(default = "default_quasi_threshold")]
    pub quasi_identifier_threshold: usize,
    /// Whether withDp is required for sensitive-attribute aggregates
    #[serde(default = "default_true")]
    pub require_dp_for_sensitive_aggregate: bool,
    /// Maximum allowed privacy budget ε
    #[serde(default = "default_max_epsilon")]
    pub max_epsilon: f64,
    /// Default ε inserted by auto-remediation
    #[serde(default = "default_remediation_epsilon")]
    pub remediation_epsilon: f64,

    /// Columns allowed to be output despite classification (allowlist)
    #[serde(default)]
    pub allowed_output_columns: Vec<String>,
    /// Strings that block a load() path if present
    #[serde(default)]
    pub denied_path_fragments: Vec<String>,
    /// Authorized directories for absolute-path load() (allowlist).
    ///
    /// When empty, all absolute-path access is blocked (fail-closed).
    /// Relative paths are checked against the project root regardless of this list.
    #[serde(default)]
    pub allowed_absolute_path_prefixes: Vec<String>,

    /// Per-rule severity override (e.g. `{"XZP013": "block"}`)
    #[serde(default)]
    pub rule_severity: BTreeMap<String, Severity>,

    /// Applied domain — recorded in audit evidence (`healthcare` · `finance` · `public-sector` …)
    #[serde(default = "default_domain")]
    pub domain: String,
    /// Risk tier of the policy (low/medium/high per the pseudonymized-data guidelines)
    #[serde(default = "default_risk_level")]
    pub risk_level: RiskLevel,
    /// Per-rule regulatory-basis override — the applicable law differs by organization and domain
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

/// Policy load failure — callers must handle it fail-closed.
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
    /// Parses a policy JSON string.
    ///
    /// Returns `Err` on failure. Callers must treat this as "unverifiable = blocked",
    /// not "no policy = allowed".
    pub fn from_json_str(text: &str) -> Result<Policy, PolicyError> {
        let policy: Policy = serde_json::from_str(text).map_err(|e| PolicyError {
            message: format!("정책 JSON 파싱 실패: {}", e),
        })?;
        policy.validate()?;
        Ok(policy)
    }

    /// Validates the internal consistency of the policy itself.
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

    /// Serializes the policy to a JSON string.
    pub fn to_json_string(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Classifies a column. If it is on the allowlist, it is always `Unclassified`.
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

    /// Effective severity for a rule — follows the policy override when present.
    pub fn severity_for(&self, rule_id: &str, default: Severity) -> Severity {
        self.rule_severity.get(rule_id).copied().unwrap_or(default)
    }

    /// Built-in default policy — conservative defaults in the context of the Korean
    /// Personal Information Protection Act and medical law.
    ///
    /// Column names are only compared by **exact match** after normalization, so
    /// common columns like `region`·`station`·`case_id` are not caught.
    pub fn builtin() -> Policy {
        let s = |items: &[&str]| items.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        Policy {
            id: "xazz-builtin-pii".to_string(),
            version: "1.0.0".to_string(),
            description: "Xazz 내장 기본 정책 — 개인정보 직접 노출·재식별·비밀키 유출 차단"
                .to_string(),
            direct_identifiers: s(&[
                // Names
                "name",
                "full_name",
                "first_name",
                "last_name",
                "성명",
                "이름",
                "성함",
                // Unique identification numbers
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
                // Contact info
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
                // Financial
                "account_no",
                "account_number",
                "bank_account",
                "계좌번호",
                "card_no",
                "card_number",
                "credit_card",
                "카드번호",
                // Medical and organizational identifiers
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
                // Devices and network
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
            // The default policy denies all absolute-path access (fail-closed).
            allowed_absolute_path_prefixes: Vec::new(),
            rule_severity: BTreeMap::new(),
            domain: default_domain(),
            risk_level: RiskLevel::Medium,
            rule_source_refs: BTreeMap::new(),
        }
    }

    /// Regulatory basis for a rule — uses the policy override if present, otherwise the built-in default.
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

// ── Active policy loading (fail-closed) ──────────────────────────────────────

/// Environment variable used to locate the policy file.
pub const POLICY_PATH_ENV: &str = "XAZZ_POLICY_PATH";
/// Policy file name automatically looked up at the project root.
pub const DEFAULT_POLICY_FILE: &str = "xazz.policy.json";

/// Which policy was applied and how.
#[derive(Debug, Clone)]
pub struct ActivePolicy {
    pub policy: Policy,
    /// Policy origin — a file path or `"builtin"`
    pub origin: String,
}

/// Determines the policy to apply for execution.
///
/// Priority
///   1. Environment variable `XAZZ_POLICY_PATH` — if set, loading **must** succeed.
///   2. `xazz.policy.json` in the working directory — if present, loading **must** succeed.
///   3. If neither exists, the built-in default policy.
///
/// If the file in 1 or 2 is broken, `Err` is returned. "Couldn't read the policy, so just
/// run anyway" would defeat the purpose of the guardrail, so callers must treat this
/// error as an **execution refusal**.
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

/// Reads the policy file at the given path. Failure is always an error (fail-closed).
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

/// Turns a policy load failure directly into a block report.
///
/// Callers can show this report to the user as-is.
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

// ── Public entry points ──────────────────────────────────────────────────────

/// Lexes and parses `.xzz` source, then applies the policy.
///
/// Even on parse failure a `PolicyReport` is returned — in that case `safe_to_execute`
/// is always false and `parse_error` is populated (fail-closed). The literal scan runs
/// regardless of parse failure, so secrets hidden in code with syntax errors are
/// reported too.
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
            // Even if parsing fails, the text scan is still valid.
            rules::apply_literal_rules(source, policy, &mut report);
            attach_source_refs(&mut report, policy);
            report.safe_to_execute = false;
            report
        }
    }
}

/// Applies the policy to an already-parsed AST.
///
/// `source` is used for the literal scan (including comments) and line-number calculation.
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

/// Attaches a regulatory basis to every violation.
///
/// Since rule judgments do not need to know the basis string, it is filled in one
/// place after judgment. All audit evidence (`rule_id` · `source_ref` · `policy_version` ·
/// `domain` · `risk_level`) ends up in a single report.
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

/// Places a violation into the list matching its severity.
pub(crate) fn record(report: &mut PolicyReport, violation: Violation) {
    match violation.severity {
        Severity::Block => report.violations.push(violation),
        Severity::Warn | Severity::Info => report.warnings.push(violation),
    }
}

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Normalization absorbs spelling style (snake, camel, hyphen).
    #[test]
    fn normalizes_column_spellings() {
        assert_eq!(normalize_column("patient_id"), "patientid");
        assert_eq!(normalize_column("patientID"), "patientid");
        assert_eq!(normalize_column("Patient-Id"), "patientid");
        assert_eq!(normalize_column("주민등록번호"), "주민등록번호");
    }

    /// Classification is an exact match — no substring false positives.
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

    /// Common columns from existing examples are never classified (false-positive regression guard).
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

    /// The allowlist overrides classification.
    #[test]
    fn allowlist_overrides_classification() {
        let mut p = Policy::builtin();
        assert_eq!(p.classify("age"), ColumnClass::QuasiIdentifier);
        p.allowed_output_columns.push("age".to_string());
        assert_eq!(p.classify("age"), ColumnClass::Unclassified);
    }

    /// Policy JSON round-trip.
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

    /// Broken policy JSON must be Err (the premise of fail-closed).
    #[test]
    fn broken_policy_json_is_rejected() {
        assert!(Policy::from_json_str("{ not json").is_err());
        assert!(
            Policy::from_json_str("{}").is_err(),
            "id 없는 정책은 거부되어야 한다"
        );
    }

    /// Policy consistency validation — invalid ε combinations are rejected.
    #[test]
    fn invalid_epsilon_configuration_is_rejected() {
        let mut p = Policy::builtin();
        p.max_epsilon = 0.0;
        assert!(p.validate().is_err());

        let mut p = Policy::builtin();
        p.remediation_epsilon = 99.0;
        assert!(p.validate().is_err());
    }

    /// The rule severity override is applied.
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

    /// Unparseable source is blocked fail-closed.
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

    /// Hidden secrets are reported even when parsing fails.
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

    /// Every violation carries a regulatory basis (audit-evidence requirement).
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

    /// A policy override of the regulatory basis takes precedence.
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

    /// The report carries domain and risk level (audit-evidence requirement).
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

    /// Policy load failure is recorded as high risk.
    #[test]
    fn policy_load_failure_is_high_risk() {
        let err = PolicyError {
            message: "테스트".to_string(),
        };
        let report = policy_load_failure_report(&err);
        assert!(!report.safe_to_execute);
        assert_eq!(report.risk_level, RiskLevel::High);
    }

    /// All three shipped domain policy packs load validly.
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

    /// Severity ordering — Block is the highest.
    #[test]
    fn severity_ordering() {
        assert!(Severity::Block > Severity::Warn);
        assert!(Severity::Warn > Severity::Info);
    }
}
