// xazz-compiler/src/policy/rules.rs — 정책 규칙 판정 (issue #2)
//
// 파이프라인이 **무엇을 밖으로 내보내는가**(출력 컬럼 집합)를 정적으로 추론한 뒤,
// 그 집합을 정책에 비추어 판정한다. Xazz 에서는 파이프라인의 결과 자체가 출력이므로
// "출력 컬럼 = 노출 표면" 이다.
//
// 핵심 판단 규칙
//   · 집계 결과 컬럼은 더 이상 식별자가 아니다.
//     `groupBy("region") |> count("patient_id")` 의 `patient_id` 는 환자번호가 아니라
//     '건수'다. 이를 구분하지 못하면 정상 통계 쿼리가 전부 오탐으로 막힌다.
//   · 민감 속성이 **그룹 키**로 쓰이는 것(질병별 건수)은 집계이므로 허용하되,
//     차등 프라이버시를 요구한다. 행 단위로 그대로 나가는 것만 차단한다.

use std::collections::HashMap;

use crate::ast::{ChartConfig, DpArgs, PipelineOp, PipelineSource, Program, Stmt, StructField};
use xazz_core::i18n::{is_korean, tr};

use super::{
    ColumnClass, Policy, PolicyReport, RULE_AGGREGATE_WITHOUT_DP, RULE_DIRECT_IDENTIFIER,
    RULE_EPSILON_TOO_LARGE, RULE_HARDCODED_SECRET, RULE_PATH_TRAVERSAL, RULE_PII_LITERAL,
    RULE_QUASI_COMBINATION, RULE_SENSITIVE_PATH, RULE_SENSITIVE_ROW_LEVEL, RULE_UNRESOLVED_SCHEMA,
    Severity, Violation, patterns, record,
};

// ── 출력 컬럼 모델 ───────────────────────────────────────────────────────────

/// 파이프라인 출력에 실리는 컬럼 하나.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputColumn {
    pub name: String,
    /// 집계(sum/mean/count/…)의 결과라 원본 값이 남지 않는 컬럼
    pub aggregated: bool,
}

impl OutputColumn {
    fn raw(name: impl Into<String>) -> Self {
        OutputColumn {
            name: name.into(),
            aggregated: false,
        }
    }
    fn agg(name: impl Into<String>) -> Self {
        OutputColumn {
            name: name.into(),
            aggregated: true,
        }
    }
}

/// 한 구문(파이프라인)의 정적 분석 결과.
#[derive(Debug, Clone, Default)]
pub struct PipelineShape {
    pub columns: Vec<OutputColumn>,
    /// 집계 연산이 하나라도 적용되었는지
    pub aggregated: bool,
    /// withDp(...) 인수 (적용된 경우)
    pub dp: Option<DpArgs>,
    /// 출력 컬럼 집합을 확실히 알아냈는지 (스키마 미해석 대응)
    pub columns_known: bool,
}

impl PipelineShape {
    fn set_columns(&mut self, cols: Vec<OutputColumn>) {
        self.columns = cols;
        self.columns_known = true;
    }

    fn push_unique(&mut self, col: OutputColumn) {
        if !self.columns.iter().any(|c| c.name == col.name) {
            self.columns.push(col);
        }
    }

    /// 정책상 원본 값이 노출되는 컬럼만 골라낸다 (집계 결과는 제외).
    fn exposed(&self) -> impl Iterator<Item = &OutputColumn> {
        self.columns.iter().filter(|c| !c.aggregated)
    }
}

// ── AST 규칙 적용 ────────────────────────────────────────────────────────────

/// AST 기반 규칙 전부를 적용한다.
pub fn apply_ast_rules(program: &Program, policy: &Policy, report: &mut PolicyReport) {
    // 스키마는 선언 순서와 무관하게 전부 먼저 모은다.
    let mut schemas: HashMap<String, Vec<StructField>> = HashMap::new();
    for stmt in &program.stmts {
        if let Stmt::TypeDecl { name, fields } = stmt {
            schemas.insert(name.clone(), fields.clone());
        }
    }

    let mut vars: HashMap<String, PipelineShape> = HashMap::new();

    for (index, stmt) in program.stmts.iter().enumerate() {
        match stmt {
            Stmt::VarDecl {
                var_name,
                source,
                ops,
                ..
            } => {
                check_source_path(source, index, Some(var_name), policy, report);
                let shape = infer_shape(source, ops, &schemas, &vars);
                judge(&shape, index, Some(var_name), policy, report);
                vars.insert(var_name.clone(), shape);
            }
            Stmt::ExprStmt { source, ops } => {
                check_source_path(source, index, None, policy, report);
                let shape = infer_shape(source, ops, &schemas, &vars);
                judge(&shape, index, None, policy, report);
            }
            Stmt::TrainStmt { .. } | Stmt::TypeDecl { .. } | Stmt::ModelDecl { .. } => {}
        }
    }
}

/// `load("...")` 경로에 대한 규칙 (민감 경로 · 경로 탈출).
fn check_source_path(
    source: &PipelineSource,
    index: usize,
    var: Option<&str>,
    policy: &Policy,
    report: &mut PolicyReport,
) {
    let PipelineSource::Load { file_path, .. } = source else {
        return;
    };
    let normalized = file_path.replace('\\', "/").to_ascii_lowercase();

    let hit = policy
        .denied_path_fragments
        .iter()
        .map(|f| f.replace('\\', "/").to_ascii_lowercase())
        .find(|f| !f.is_empty() && normalized.contains(f.as_str()));

    let dotenv = normalized.ends_with("/.env") || normalized == ".env";

    if hit.is_some() || dotenv {
        let severity = policy.severity_for(RULE_SENSITIVE_PATH, Severity::Block);
        record(
            report,
            Violation::new(
                RULE_SENSITIVE_PATH,
                severity,
                format!(
                    "{}: load(\"{}\"). {}",
                    tr(
                        "accessing a system-sensitive path",
                        "시스템 민감 경로에 접근합니다"
                    ),
                    file_path,
                    tr(
                        "a data pipeline should only read authorized data directories",
                        "데이터 파이프라인은 인가된 데이터 디렉터리만 읽어야 합니다"
                    )
                ),
                tr(
                    "move the data file into the project's data directory and use that path",
                    "데이터 파일을 프로젝트의 데이터 디렉터리로 옮기고 그 경로를 사용하세요"
                ),
            )
            .at_stmt(index, var),
        );
    }

    // 상위 디렉터리 탈출은 기본 경고 — 정책에서 block 으로 올릴 수 있다.
    let traversal = normalized.split('/').any(|segment| segment == "..");
    if traversal {
        let severity = policy.severity_for(RULE_PATH_TRAVERSAL, Severity::Warn);
        record(
            report,
            Violation::new(
                RULE_PATH_TRAVERSAL,
                severity,
                format!(
                    "{}: load(\"{}\").",
                    tr(
                        "using a parent-directory traversal path",
                        "상위 디렉터리 탈출 경로를 사용합니다"
                    ),
                    file_path
                ),
                tr(
                    "use a project-root-relative path or an authorized absolute path",
                    "프로젝트 루트 기준의 상대 경로 또는 인가된 절대 경로를 사용하세요"
                ),
            )
            .at_stmt(index, var),
        );
    }
}

// ── 출력 컬럼 추론 ───────────────────────────────────────────────────────────

/// 소스와 연산자 목록으로부터 출력 컬럼 집합을 추론한다.
pub fn infer_shape(
    source: &PipelineSource,
    ops: &[PipelineOp],
    schemas: &HashMap<String, Vec<StructField>>,
    vars: &HashMap<String, PipelineShape>,
) -> PipelineShape {
    let mut shape = PipelineShape::default();

    match source {
        PipelineSource::Load { schema_name, .. } => match schemas.get(schema_name) {
            Some(fields) => {
                shape.set_columns(fields.iter().map(|f| OutputColumn::raw(&f.name)).collect());
            }
            None => {
                shape.columns_known = false;
            }
        },
        PipelineSource::VarRef(name) => match vars.get(name) {
            Some(prev) => {
                shape.columns = prev.columns.clone();
                shape.columns_known = prev.columns_known;
                shape.aggregated = prev.aggregated;
                shape.dp = prev.dp.clone();
            }
            None => {
                shape.columns_known = false;
            }
        },
    }

    let mut group_keys: Vec<String> = Vec::new();

    for op in ops {
        match op {
            // ── 컬럼 집합을 확정하는 연산 ────────────────────────────────
            PipelineOp::Select(cols) => {
                let prev = shape.columns.clone();
                let picked = cols
                    .iter()
                    .map(|name| {
                        let aggregated = prev
                            .iter()
                            .find(|c| c.name == *name)
                            .map(|c| c.aggregated)
                            .unwrap_or(false);
                        OutputColumn {
                            name: name.clone(),
                            aggregated,
                        }
                    })
                    .collect();
                // select 는 출력 컬럼을 완전히 결정한다 — 스키마를 몰라도 확실해진다.
                shape.set_columns(picked);
            }

            // ── 집계 — 결과 컬럼은 통계값이므로 식별자가 아니다 ─────────────
            PipelineOp::GroupBy(col) => {
                if !group_keys.contains(col) {
                    group_keys.push(col.clone());
                }
            }
            PipelineOp::Sum(col)
            | PipelineOp::Mean(col)
            | PipelineOp::Min(col)
            | PipelineOp::Max(col)
            | PipelineOp::Median(col)
            | PipelineOp::Variance(col)
            | PipelineOp::Std(col)
            | PipelineOp::Count(Some(col)) => {
                shape.aggregated = true;
                let mut cols: Vec<OutputColumn> =
                    group_keys.iter().map(OutputColumn::raw).collect();
                cols.push(OutputColumn::agg(col));
                shape.set_columns(cols);
            }
            PipelineOp::Count(None) => {
                shape.aggregated = true;
                let mut cols: Vec<OutputColumn> =
                    group_keys.iter().map(OutputColumn::raw).collect();
                cols.push(OutputColumn::agg("count"));
                shape.set_columns(cols);
            }

            // ── 컬럼을 추가·개명하는 연산 ────────────────────────────────
            PipelineOp::WithColumn { name, .. } => {
                shape.push_unique(OutputColumn::raw(name));
            }
            PipelineOp::Rename { old_name, new_name } => {
                for c in shape.columns.iter_mut() {
                    if c.name == *old_name {
                        c.name = new_name.clone();
                    }
                }
                for key in group_keys.iter_mut() {
                    if key == old_name {
                        *key = new_name.clone();
                    }
                }
            }
            PipelineOp::Predict { as_col, .. } => {
                shape.push_unique(OutputColumn::raw(as_col.as_deref().unwrap_or("prediction")));
            }

            // ── 조인 — 상대 변수의 컬럼이 합쳐진다 ───────────────────────
            PipelineOp::Join { other, .. } => match vars.get(other) {
                Some(rhs) => {
                    for c in &rhs.columns {
                        shape.push_unique(c.clone());
                    }
                    shape.columns_known = shape.columns_known && rhs.columns_known;
                }
                None => {
                    shape.columns_known = false;
                }
            },

            // ── 프라이버시 ──────────────────────────────────────────────
            PipelineOp::WithDp(args) => {
                shape.dp = Some(args.clone());
            }

            // ── 행만 거르거나 값을 바꾸는 연산 — 컬럼 집합 불변 ───────────
            PipelineOp::Filter(_)
            | PipelineOp::DropNull(_)
            | PipelineOp::FillNull { .. }
            | PipelineOp::OrderBy { .. }
            | PipelineOp::Take(_)
            | PipelineOp::Sample { .. }
            | PipelineOp::Cast { .. }
            | PipelineOp::Replace { .. }
            | PipelineOp::Chart(_)
            | PipelineOp::Train { .. } => {}
        }
    }

    shape
}

// ── 판정 ─────────────────────────────────────────────────────────────────────

/// 추론된 출력 컬럼 집합을 정책에 비추어 판정한다.
fn judge(
    shape: &PipelineShape,
    index: usize,
    var: Option<&str>,
    policy: &Policy,
    report: &mut PolicyReport,
) {
    // ── XZP014: 컬럼 집합을 확정하지 못했다 ────────────────────────────────
    if !shape.columns_known {
        let severity = policy.severity_for(RULE_UNRESOLVED_SCHEMA, Severity::Warn);
        record(
            report,
            Violation::new(
                RULE_UNRESOLVED_SCHEMA,
                severity,
                tr(
                    "schema could not be resolved, so output columns cannot be determined; PII exposure cannot be statically proven",
                    "스키마를 해석하지 못해 출력 컬럼을 확정할 수 없습니다. 개인정보 노출 여부를 정적으로 증명하지 못합니다."
                )
                .to_string(),
                tr(
                    "add a type declaration or make the output columns explicit with |> select([...])",
                    "type 선언을 추가하거나 |> select([...]) 로 출력 컬럼을 명시하세요"
                ),
            )
            .at_stmt(index, var),
        );
    }

    // ── XZP001: 직접 식별자 노출 ───────────────────────────────────────────
    let direct: Vec<String> = shape
        .exposed()
        .filter(|c| policy.classify(&c.name) == ColumnClass::DirectIdentifier)
        .map(|c| c.name.clone())
        .collect();
    if !direct.is_empty() {
        let severity = policy.severity_for(RULE_DIRECT_IDENTIFIER, Severity::Block);
        record(
            report,
            Violation::new(
                RULE_DIRECT_IDENTIFIER,
                severity,
                if is_korean() {
                    format!(
                        "직접 식별자 컬럼이 결과로 그대로 출력됩니다: {}. 개인을 특정할 수 있는 값은 파이프라인 밖으로 나갈 수 없습니다.",
                        direct.join(", ")
                    )
                } else {
                    format!(
                        "direct identifier columns are emitted as-is in the result: {}. Values that can identify an individual must not leave the pipeline.",
                        direct.join(", ")
                    )
                },
                if is_korean() {
                    format!(
                        "|> select([...]) 에서 {} 을(를) 제외하거나, groupBy + 집계로 통계만 남기세요.",
                        direct.join(", ")
                    )
                } else {
                    format!(
                        "remove {} from |> select([...]), or keep only statistics via groupBy + aggregate.",
                        direct.join(", ")
                    )
                },
            )
            .at_stmt(index, var)
            .with_columns(direct),
        );
    }

    // ── XZP002: 민감 속성 행 단위 노출 ─────────────────────────────────────
    if !shape.aggregated {
        let sensitive: Vec<String> = shape
            .exposed()
            .filter(|c| policy.classify(&c.name) == ColumnClass::SensitiveAttribute)
            .map(|c| c.name.clone())
            .collect();
        if !sensitive.is_empty() {
            let severity = policy.severity_for(RULE_SENSITIVE_ROW_LEVEL, Severity::Block);
            record(
                report,
                Violation::new(
                    RULE_SENSITIVE_ROW_LEVEL,
                    severity,
                    if is_korean() {
                    format!(
                        "민감 속성이 집계 없이 행 단위로 출력됩니다: {}. 개별 레코드 단위 민감정보 조회는 허용되지 않습니다.",
                        sensitive.join(", ")
                    )
                } else {
                    format!(
                        "sensitive attributes are emitted row-wise without aggregation: {}. Row-level sensitive-data lookup is not allowed.",
                        sensitive.join(", ")
                    )
                },
                if is_korean() {
                    format!(
                        "|> groupBy(\"<범주형 컬럼>\") |> count(\"{}\") 형태의 집계로 바꾸고 |> withDp(...) 를 적용하세요.",
                        sensitive.first().cloned().unwrap_or_default()
                    )
                } else {
                    format!(
                        "convert to an aggregate like |> groupBy(\"<categorical column>\") |> count(\"{}\") and apply |> withDp(...).",
                        sensitive.first().cloned().unwrap_or_default()
                    )
                },
                )
                .at_stmt(index, var)
                .with_columns(sensitive),
            );
        }
    }

    // ── XZP003: 준식별자 결합 재식별 위험 ──────────────────────────────────
    let quasi: Vec<String> = shape
        .exposed()
        .filter(|c| policy.classify(&c.name) == ColumnClass::QuasiIdentifier)
        .map(|c| c.name.clone())
        .collect();
    if quasi.len() >= policy.quasi_identifier_threshold {
        // 집계된 파이프라인이면 개별 레코드가 남지 않으므로 경고로 낮춘다.
        let default_severity = if shape.aggregated {
            Severity::Warn
        } else {
            Severity::Block
        };
        let severity = policy.severity_for(RULE_QUASI_COMBINATION, default_severity);
        record(
            report,
            Violation::new(
                RULE_QUASI_COMBINATION,
                severity,
                if is_korean() {
                    format!(
                        "준식별자 {}개가 함께 출력되어 재식별 위험이 있습니다: {} (임계치 {}개).",
                        quasi.len(),
                        quasi.join(", "),
                        policy.quasi_identifier_threshold
                    )
                } else {
                    format!(
                        "{} quasi-identifiers are emitted together, creating re-identification risk: {} (threshold {}).",
                        quasi.len(),
                        quasi.join(", "),
                        policy.quasi_identifier_threshold
                    )
                },
                tr(
                    "remove some quasi-identifiers or generalize via binning (e.g. age → age_band)",
                    "준식별자 일부를 제거하거나 구간화(예: age → age_band)해 일반화하세요"
                ),
            )
            .at_stmt(index, var)
            .with_columns(quasi),
        );
    }

    // ── XZP004: 민감 속성 집계에 DP 미적용 ─────────────────────────────────
    if shape.aggregated && shape.dp.is_none() {
        let sensitive_keys: Vec<String> = shape
            .columns
            .iter()
            .filter(|c| policy.classify(&c.name) == ColumnClass::SensitiveAttribute)
            .map(|c| c.name.clone())
            .collect();
        if !sensitive_keys.is_empty() {
            let default_severity = if policy.require_dp_for_sensitive_aggregate {
                Severity::Block
            } else {
                Severity::Warn
            };
            let severity = policy.severity_for(RULE_AGGREGATE_WITHOUT_DP, default_severity);
            record(
                report,
                Violation::new(
                    RULE_AGGREGATE_WITHOUT_DP,
                    severity,
                    if is_korean() {
                    format!(
                        "민감 속성({})에 대한 집계 결과에 차등 프라이버시가 적용되지 않았습니다. 소집단에서는 집계값만으로도 개인이 역추적될 수 있습니다.",
                        sensitive_keys.join(", ")
                    )
                } else {
                    format!(
                        "the aggregate over sensitive attribute(s) ({}) has no differential privacy applied. In small groups, aggregates alone can re-identify individuals.",
                        sensitive_keys.join(", ")
                    )
                },
                if is_korean() {
                    format!(
                        "파이프라인 끝에 |> withDp(epsilon: {}, mechanism: laplace, sensitivity: 1.0) 를 추가하세요.",
                        super::printer::print_f64(policy.remediation_epsilon)
                    )
                } else {
                    format!(
                        "append |> withDp(epsilon: {}, mechanism: laplace, sensitivity: 1.0) to the pipeline.",
                        super::printer::print_f64(policy.remediation_epsilon)
                    )
                },
                )
                .at_stmt(index, var)
                .with_columns(sensitive_keys),
            );
        }
    }

    // ── XZP005: 프라이버시 예산 상한 초과 ──────────────────────────────────
    // NaN 도 "상한 위반"으로 다룬다 — 비교로는 걸러지지 않으므로 명시적으로 확인한다.
    if let Some(dp) = shape.dp.as_ref().filter(|dp| {
        !dp.epsilon.is_finite() || dp.epsilon <= 0.0 || dp.epsilon > policy.max_epsilon
    }) {
        {
            let severity = policy.severity_for(RULE_EPSILON_TOO_LARGE, Severity::Block);
            record(
                report,
                Violation::new(
                    RULE_EPSILON_TOO_LARGE,
                    severity,
                    if is_korean() {
                    format!(
                        "프라이버시 예산 ε={} 이 정책 상한(0 < ε ≤ {})을 벗어납니다. ε 이 클수록 노이즈가 작아져 보호 강도가 떨어집니다.",
                        super::printer::print_f64(dp.epsilon),
                        super::printer::print_f64(policy.max_epsilon)
                    )
                } else {
                    format!(
                        "privacy budget ε={} exceeds the policy cap (0 < ε ≤ {}). Larger ε means less noise and weaker protection.",
                        super::printer::print_f64(dp.epsilon),
                        super::printer::print_f64(policy.max_epsilon)
                    )
                },
                if is_korean() {
                    format!(
                        "withDp(epsilon: {}) 이하로 낮추세요.",
                        super::printer::print_f64(policy.max_epsilon)
                    )
                } else {
                    format!(
                        "lower it to withDp(epsilon: {}) or below.",
                        super::printer::print_f64(policy.max_epsilon)
                    )
                },
                )
                .at_stmt(index, var),
            );
        }
    }
}

// ── 리터럴 규칙 적용 ─────────────────────────────────────────────────────────

/// 소스 텍스트(주석 포함)에서 개인정보·비밀키 리터럴을 찾아 위반으로 기록한다.
pub fn apply_literal_rules(source: &str, policy: &Policy, report: &mut PolicyReport) {
    for finding in patterns::scan_source(source) {
        let (rule_id, hint) = if finding.kind.is_pii() {
            (
                RULE_PII_LITERAL,
                tr(
                    "do not write PII values into source; replace them with de-identified keys or parameters",
                    "개인정보 값을 소스에 직접 쓰지 말고, 비식별화된 키나 파라미터로 대체하세요"
                ),
            )
        } else {
            (
                RULE_HARDCODED_SECRET,
                tr(
                    "remove the credential from source and move it to env vars / a secret store; revoke and reissue any exposed key immediately",
                    "자격증명을 소스에서 제거하고 환경변수·시크릿 저장소로 옮긴 뒤, 노출된 키는 즉시 폐기·재발급하세요"
                ),
            )
        };
        let severity = policy.severity_for(rule_id, Severity::Block);
        record(
            report,
            Violation::new(
                rule_id,
                severity,
                if is_korean() {
                    format!(
                        "{} 값이 소스에 하드코딩되어 있습니다 ({}행 {}열, 값: {}).",
                        finding.kind.label(),
                        finding.line,
                        finding.col,
                        finding.redacted
                    )
                } else {
                    format!(
                        "{} value is hardcoded in the source (line {} col {}, value: {}).",
                        finding.kind.label(),
                        finding.line,
                        finding.col,
                        finding.redacted
                    )
                },
                hint,
            )
            .at_position(finding.line, finding.col),
        );
    }
}

/// 차트가 참조하는 컬럼 목록 — 보정 로직에서 재사용한다.
pub fn chart_columns(config: &ChartConfig) -> Vec<String> {
    [&config.x, &config.y, &config.label, &config.value]
        .into_iter()
        .flatten()
        .cloned()
        .collect()
}

// ── 유닛 테스트 ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::super::{Policy, RULE_AGGREGATE_WITHOUT_DP, RULE_DIRECT_IDENTIFIER, analyze};
    use super::super::{RULE_EPSILON_TOO_LARGE, RULE_QUASI_COMBINATION, RULE_SENSITIVE_ROW_LEVEL};

    const PATIENT_SCHEMA: &str = "type Patient = {
        patient_id: string,
        name: string,
        age: int,
        gender: string,
        zip_code: string,
        disease: string,
        age_band: string,
    };\n";

    fn report_for(pipeline: &str) -> super::super::PolicyReport {
        let src = format!("{}{}", PATIENT_SCHEMA, pipeline);
        analyze(&src, &Policy::builtin())
    }

    fn blocked_by(pipeline: &str, rule: &str) -> bool {
        let r = report_for(pipeline);
        !r.safe_to_execute && r.violations.iter().any(|v| v.rule_id == rule)
    }

    /// 직접 식별자를 select 로 내보내면 차단된다.
    #[test]
    fn blocks_direct_identifier_in_select() {
        assert!(blocked_by(
            "v out = load(\"data/p.csv\") :: Patient |> select([name, patient_id]);",
            RULE_DIRECT_IDENTIFIER
        ));
    }

    /// select 없이 전체 컬럼을 내보내도 직접 식별자가 잡힌다.
    #[test]
    fn blocks_direct_identifier_without_select() {
        assert!(blocked_by(
            "v out = load(\"data/p.csv\") :: Patient |> filter(age > 30);",
            RULE_DIRECT_IDENTIFIER
        ));
    }

    /// 민감 속성이 행 단위로 나가면 차단된다.
    #[test]
    fn blocks_row_level_sensitive_attribute() {
        assert!(blocked_by(
            "v out = load(\"data/p.csv\") :: Patient |> select([age, disease]);",
            RULE_SENSITIVE_ROW_LEVEL
        ));
    }

    /// 준식별자 3개 결합은 차단된다.
    #[test]
    fn blocks_quasi_identifier_combination() {
        assert!(blocked_by(
            "v out = load(\"data/p.csv\") :: Patient |> select([age, gender, zip_code]);",
            RULE_QUASI_COMBINATION
        ));
    }

    /// 준식별자 2개는 임계치 미만이라 통과한다.
    #[test]
    fn allows_quasi_identifiers_below_threshold() {
        let r = report_for("v out = load(\"data/p.csv\") :: Patient |> select([age, gender]);");
        assert!(
            r.safe_to_execute,
            "임계치 미만인데 차단됨: {:?}",
            r.violations
        );
    }

    /// 집계 결과 컬럼은 식별자로 보지 않는다 — 정상 통계 쿼리 오탐 방지.
    #[test]
    fn aggregated_identifier_column_is_not_a_leak() {
        let r = report_for(
            "v out = load(\"data/p.csv\") :: Patient
               |> groupBy(\"age_band\")
               |> count(\"patient_id\");",
        );
        assert!(r.safe_to_execute, "정상 집계가 차단됨: {:?}", r.violations);
    }

    /// 민감 속성을 그룹 키로 쓴 집계는 DP 를 요구한다.
    #[test]
    fn sensitive_aggregate_requires_dp() {
        assert!(blocked_by(
            "v out = load(\"data/p.csv\") :: Patient
               |> groupBy(\"disease\")
               |> count(\"patient_id\");",
            RULE_AGGREGATE_WITHOUT_DP
        ));
    }

    /// withDp 를 붙이면 같은 집계가 통과한다.
    #[test]
    fn sensitive_aggregate_passes_with_dp() {
        let r = report_for(
            "v out = load(\"data/p.csv\") :: Patient
               |> groupBy(\"disease\")
               |> count(\"patient_id\")
               |> withDp(epsilon: 1.0, mechanism: laplace, sensitivity: 1.0);",
        );
        assert!(
            r.safe_to_execute,
            "DP 적용 후에도 차단됨: {:?}",
            r.violations
        );
    }

    /// ε 이 정책 상한을 넘으면 차단된다.
    #[test]
    fn blocks_excessive_epsilon() {
        assert!(blocked_by(
            "v out = load(\"data/p.csv\") :: Patient
               |> groupBy(\"disease\")
               |> count(\"patient_id\")
               |> withDp(epsilon: 50.0);",
            RULE_EPSILON_TOO_LARGE
        ));
    }

    /// 변수 참조를 거쳐도 컬럼 추적이 이어진다.
    #[test]
    fn tracks_columns_through_variable_reference() {
        assert!(blocked_by(
            "v base = load(\"data/p.csv\") :: Patient |> select([age_band, name]);
             v out = base |> filter(age_band == \"30s\");",
            RULE_DIRECT_IDENTIFIER
        ));
    }

    /// rename 으로 이름을 바꿔도 원본 분류가 따라오지는 않는다 —
    /// 대신 rename 대상이 정책 컬럼이면 이름이 바뀐 시점부터 재분류된다.
    #[test]
    fn rename_changes_classification_target() {
        // name → nickname 으로 바꾸면 직접 식별자 분류에서 벗어난다.
        let r = report_for(
            "v out = load(\"data/p.csv\") :: Patient
               |> select([name, age_band])
               |> rename(\"name\", \"nickname\");",
        );
        assert!(
            r.safe_to_execute,
            "rename 후에도 차단됨: {:?}",
            r.violations
        );
    }

    /// 정상적인 대기질 파이프라인은 통과한다 (기존 예제 회귀 방지).
    #[test]
    fn existing_air_quality_pipeline_passes() {
        let src =
            "type AQ = { date: string, station: string, pm10: Option<float>, pm25: Option<float> };
                   v cleaned = load(\"examples/data/seoul_air_2024.csv\") :: AQ
                     |> dropNull(\"pm10\")
                     |> filter(pm10 < 120);
                   v by_station = cleaned
                     |> groupBy(\"station\")
                     |> mean(\"pm10\")
                     |> orderBy(\"pm10\", desc: true)
                     |> take(10);";
        let r = analyze(src, &Policy::builtin());
        assert!(
            r.safe_to_execute,
            "정상 파이프라인 차단됨: {:?}",
            r.violations
        );
    }

    /// 민감 경로 접근은 차단된다.
    #[test]
    fn blocks_sensitive_path_access() {
        let r = analyze(
            "type P = { a: string };\nv x = load(\"/etc/passwd\") :: P |> select([a]);",
            &Policy::builtin(),
        );
        assert!(!r.safe_to_execute);
        assert!(
            r.violations
                .iter()
                .any(|v| v.rule_id == super::super::RULE_SENSITIVE_PATH)
        );
    }

    /// 경로 탈출은 기본적으로 경고이며 실행을 막지 않는다.
    #[test]
    fn path_traversal_is_warning_by_default() {
        let r = analyze(
            "type P = { a: string };\nv x = load(\"../data/p.csv\") :: P |> select([a]);",
            &Policy::builtin(),
        );
        assert!(r.safe_to_execute, "{:?}", r.violations);
        assert!(
            r.warnings
                .iter()
                .any(|w| w.rule_id == super::super::RULE_PATH_TRAVERSAL)
        );
    }

    /// 정책에서 경로 탈출을 차단으로 올릴 수 있다.
    #[test]
    fn path_traversal_can_be_escalated_by_policy() {
        let mut policy = Policy::builtin();
        policy.rule_severity.insert(
            super::super::RULE_PATH_TRAVERSAL.to_string(),
            super::super::Severity::Block,
        );
        let r = analyze(
            "type P = { a: string };\nv x = load(\"../data/p.csv\") :: P |> select([a]);",
            &policy,
        );
        assert!(!r.safe_to_execute);
    }

    /// 스키마를 해석하지 못하면 경고를 남긴다.
    #[test]
    fn unresolved_schema_is_reported() {
        let r = analyze("v x = load(\"data/a.csv\") :: Unknown;", &Policy::builtin());
        assert!(
            r.warnings
                .iter()
                .any(|w| w.rule_id == super::super::RULE_UNRESOLVED_SCHEMA)
        );
    }

    /// 스키마를 몰라도 select 로 컬럼을 명시하면 분류가 작동한다.
    #[test]
    fn explicit_select_classifies_without_schema() {
        let r = analyze(
            "v x = load(\"data/a.csv\") :: Unknown |> select([name, age_band]);",
            &Policy::builtin(),
        );
        assert!(!r.safe_to_execute);
        assert!(
            r.violations
                .iter()
                .any(|v| v.rule_id == RULE_DIRECT_IDENTIFIER)
        );
    }
}
