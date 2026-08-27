// xazz-compiler/src/policy/remediate.rs — 결정적(deterministic) 자동 보정 (issue #2)
//
// 가드레일은 "차단"에서 끝나지 않는다. 위반이 확인되면 **안전한 대체 코드**를
// 함께 제시해야 개발자가 다음 행동을 할 수 있다.
//
// 보정은 두 층으로 이뤄진다.
//
//   1. 이 모듈 — AST 를 직접 고쳐 쓰는 결정적 보정. 항상 동작하고, 항상 같은
//      결과를 내며, 모델도 네트워크도 필요 없다.
//   2. sLM(Qwen2.5-Coder) — 더 자연스러운 재작성을 제안한다. 다만 생성 결과는
//      **반드시 이 엔진으로 재검증**되며, 통과하지 못하면 1번 결과로 되돌린다.
//      (xazz-server/src/slm.rs)
//
// 안전 원칙
//   · 코드를 문자열로 자르지 않는다 — AST 를 고치고 printer 로 다시 찍는다.
//   · 자동으로 고칠 수 없는 위반(하드코딩된 비밀키 등)은 조용히 넘어가지 않고
//     `residual` 로 남겨 사람이 처리하도록 한다.
//   · 보정 결과는 다시 분석되어 `verified` 로 증명된다. 증명되지 않은 코드는
//     "안전하다"고 말하지 않는다.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::ast::{DpArgs, DpMechanism, PipelineOp, Program, Stmt, StructField};

use super::rules::{PipelineShape, chart_columns, infer_shape};
use super::{
    ColumnClass, Policy, PolicyReport, RULE_AGGREGATE_WITHOUT_DP, RULE_DIRECT_IDENTIFIER,
    RULE_EPSILON_TOO_LARGE, RULE_QUASI_COMBINATION, RULE_SENSITIVE_ROW_LEVEL, Violation, analyze,
    printer,
};

/// 적용된 보정 하나.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliedFix {
    /// 이 보정이 해소한 규칙 ID
    pub rule_id: String,
    /// 무엇을 했는지 (한국어)
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statement_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variable: Option<String>,
}

/// 자동 보정 결과.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Remediation {
    /// 어떤 엔진이 만들었는지 — `deterministic` | `slm` | `slm-rejected`
    pub strategy: String,
    /// 보정된 `.xzz` 소스
    pub code: String,
    /// 적용된 보정 목록
    pub applied: Vec<AppliedFix>,
    /// 자동으로 고칠 수 없어 사람이 처리해야 하는 위반
    pub residual: Vec<Violation>,
    /// 보정 과정에서 개발자가 알아야 할 부수 효과 (예: 주석 미보존)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// 보정 결과가 정책을 통과했는지 — 이 값이 false 면 안전하다고 말하지 않는다.
    pub verified: bool,
    /// 보정 후 재분석 리포트
    pub report_after: PolicyReport,
}

/// 자동으로 보정할 수 없는 규칙 — 값 자체를 지워야 하므로 사람이 판단해야 한다.
fn is_auto_fixable(rule_id: &str) -> bool {
    matches!(
        rule_id,
        RULE_DIRECT_IDENTIFIER
            | RULE_SENSITIVE_ROW_LEVEL
            | RULE_QUASI_COMBINATION
            | RULE_AGGREGATE_WITHOUT_DP
            | RULE_EPSILON_TOO_LARGE
    )
}

/// 위반이 확인된 소스에 대해 결정적 보정을 수행한다.
///
/// 원본이 이미 안전하면 원본을 그대로 돌려준다.
pub fn remediate(source: &str, policy: &Policy) -> Remediation {
    let before = analyze(source, policy);
    if before.safe_to_execute {
        return Remediation {
            strategy: "deterministic".to_string(),
            code: source.to_string(),
            applied: Vec::new(),
            residual: Vec::new(),
            notes: Vec::new(),
            verified: true,
            report_after: before,
        };
    }

    // 파싱조차 되지 않으면 AST 를 고칠 수 없다.
    if before.parse_error.is_some() {
        return Remediation {
            strategy: "deterministic".to_string(),
            code: source.to_string(),
            applied: Vec::new(),
            residual: before.violations.clone(),
            notes: vec![
                "구문 오류가 있어 자동 보정을 적용하지 못했습니다. 먼저 구문을 고쳐야 합니다."
                    .to_string(),
            ],
            verified: false,
            report_after: before,
        };
    }

    let Ok(program) = parse_program(source) else {
        return Remediation {
            strategy: "deterministic".to_string(),
            code: source.to_string(),
            applied: Vec::new(),
            residual: before.violations.clone(),
            notes: vec!["소스를 파싱하지 못해 자동 보정을 건너뛰었습니다.".to_string()],
            verified: false,
            report_after: before,
        };
    };

    let (fixed_program, applied) = rewrite_program(&program, policy);
    let code = printer::print_program(&fixed_program);
    let report_after = analyze(&code, policy);

    // 자동 보정 대상이 아니었던 위반은 그대로 남는다.
    let residual: Vec<Violation> = before
        .violations
        .iter()
        .filter(|v| !is_auto_fixable(&v.rule_id))
        .cloned()
        .collect();

    let mut notes: Vec<String> = Vec::new();
    if source.contains("//") {
        notes.push(
            "보정 코드는 AST 에서 다시 생성되므로 원본 주석과 서식은 보존되지 않습니다."
                .to_string(),
        );
    }
    if !residual.is_empty() {
        notes.push(
            "자동 보정으로 해소할 수 없는 위반이 남아 있습니다. 하드코딩된 비밀값은 소스에서 제거하는 것만으로 끝나지 않으며, 노출된 자격증명은 즉시 폐기·재발급해야 합니다."
                .to_string(),
        );
    }

    // 사람이 처리해야 할 위반이 남아 있으면 "안전하다"고 말하지 않는다.
    let verified = report_after.safe_to_execute && residual.is_empty();

    Remediation {
        strategy: "deterministic".to_string(),
        code,
        applied,
        residual,
        notes,
        verified,
        report_after,
    }
}

fn parse_program(source: &str) -> Result<Program, String> {
    let tokens = crate::Lexer::new(source)
        .tokenize()
        .map_err(|e| e.to_string())?;
    crate::Parser::new(tokens)
        .parse()
        .map_err(|e| e.to_string())
}

// ── 프로그램 재작성 ──────────────────────────────────────────────────────────

fn rewrite_program(program: &Program, policy: &Policy) -> (Program, Vec<AppliedFix>) {
    let mut schemas: HashMap<String, Vec<StructField>> = HashMap::new();
    for stmt in &program.stmts {
        if let Stmt::TypeDecl { name, fields } = stmt {
            schemas.insert(name.clone(), fields.clone());
        }
    }

    let mut vars: HashMap<String, PipelineShape> = HashMap::new();
    let mut applied: Vec<AppliedFix> = Vec::new();
    let mut out_stmts: Vec<Stmt> = Vec::with_capacity(program.stmts.len());

    for (index, stmt) in program.stmts.iter().enumerate() {
        match stmt {
            Stmt::VarDecl {
                var_name,
                is_mut,
                source,
                ops,
            } => {
                let shape = infer_shape(source, ops, &schemas, &vars);
                let new_ops = rewrite_ops(ops, &shape, policy, index, Some(var_name), &mut applied);
                let new_shape = infer_shape(source, &new_ops, &schemas, &vars);
                vars.insert(var_name.clone(), new_shape);
                out_stmts.push(Stmt::VarDecl {
                    var_name: var_name.clone(),
                    is_mut: *is_mut,
                    source: source.clone(),
                    ops: new_ops,
                });
            }
            Stmt::ExprStmt { source, ops } => {
                let shape = infer_shape(source, ops, &schemas, &vars);
                let new_ops = rewrite_ops(ops, &shape, policy, index, None, &mut applied);
                out_stmts.push(Stmt::ExprStmt {
                    source: source.clone(),
                    ops: new_ops,
                });
            }
            other => out_stmts.push(other.clone()),
        }
    }

    (Program { stmts: out_stmts }, applied)
}

// ── 파이프라인 재작성 ────────────────────────────────────────────────────────

fn rewrite_ops(
    ops: &[PipelineOp],
    shape: &PipelineShape,
    policy: &Policy,
    index: usize,
    var: Option<&str>,
    applied: &mut Vec<AppliedFix>,
) -> Vec<PipelineOp> {
    // 컬럼 집합을 확정하지 못했다면 손대지 않는다 — 추측으로 코드를 바꾸지 않는다.
    if !shape.columns_known {
        return ops.to_vec();
    }

    let mut note = |rule_id: &str, description: String| {
        applied.push(AppliedFix {
            rule_id: rule_id.to_string(),
            description,
            statement_index: Some(index),
            variable: var.map(|s| s.to_string()),
        });
    };

    // ── 1) 제거해야 할 컬럼 결정 ───────────────────────────────────────────
    let mut drop: Vec<String> = Vec::new();
    let mut quasi_seen = 0usize;

    for col in shape.columns.iter().filter(|c| !c.aggregated) {
        match policy.classify(&col.name) {
            ColumnClass::DirectIdentifier => drop.push(col.name.clone()),
            ColumnClass::SensitiveAttribute if !shape.aggregated => drop.push(col.name.clone()),
            ColumnClass::QuasiIdentifier => {
                quasi_seen += 1;
                // 임계치 미만까지만 남기고 나머지는 제거한다.
                if !shape.aggregated && quasi_seen >= policy.quasi_identifier_threshold {
                    drop.push(col.name.clone());
                }
            }
            _ => {}
        }
    }

    let keep: Vec<String> = shape
        .columns
        .iter()
        .filter(|c| !drop.contains(&c.name))
        .map(|c| c.name.clone())
        .collect();

    // 차트가 제거 대상 컬럼을 참조하면 투영으로 고칠 수 없다 — 원본을 유지한다.
    let chart_refs: Vec<String> = ops
        .iter()
        .filter_map(|op| match op {
            PipelineOp::Chart(cfg) => Some(chart_columns(cfg)),
            _ => None,
        })
        .flatten()
        .collect();
    let chart_conflict = chart_refs.iter().any(|c| drop.contains(c));

    let projection_possible = !drop.is_empty() && !keep.is_empty() && !chart_conflict;

    // ── 2) 차등 프라이버시 보정 판단 ───────────────────────────────────────
    let sensitive_aggregate = shape.aggregated
        && shape
            .columns
            .iter()
            .any(|c| policy.classify(&c.name) == ColumnClass::SensitiveAttribute);
    let needs_dp =
        sensitive_aggregate && shape.dp.is_none() && policy.require_dp_for_sensitive_aggregate;

    // ── 3) 새 연산자 목록 구성 ─────────────────────────────────────────────
    // 차트는 항상 마지막에 남기고, 투영과 DP 는 차트 앞에 삽입한다.
    let split_at = ops
        .iter()
        .position(|op| matches!(op, PipelineOp::Chart(_)))
        .unwrap_or(ops.len());

    let mut new_ops: Vec<PipelineOp> = Vec::with_capacity(ops.len() + 2);
    for op in &ops[..split_at] {
        // ε 상한 초과는 그 자리에서 클램프한다.
        // NaN 도 "상한 위반"으로 다룬다 — 비교만으로는 걸러지지 않는다.
        if let Some(args) = match op {
            PipelineOp::WithDp(args)
                if !args.epsilon.is_finite()
                    || args.epsilon <= 0.0
                    || args.epsilon > policy.max_epsilon =>
            {
                Some(args)
            }
            _ => None,
        } {
            {
                let mut clamped = args.clone();
                clamped.epsilon = policy.max_epsilon;
                note(
                    RULE_EPSILON_TOO_LARGE,
                    format!(
                        "프라이버시 예산을 정책 상한으로 낮췄습니다: ε {} → {}",
                        printer::print_f64(args.epsilon),
                        printer::print_f64(policy.max_epsilon)
                    ),
                );
                new_ops.push(PipelineOp::WithDp(clamped));
                continue;
            }
        }
        new_ops.push(op.clone());
    }

    if projection_possible {
        // 마지막 연산이 이미 select 라면 중복 투영을 쌓지 않고 교체한다.
        if matches!(new_ops.last(), Some(PipelineOp::Select(_))) {
            new_ops.pop();
        }
        for name in &drop {
            let rule_id = match policy.classify(name) {
                ColumnClass::DirectIdentifier => RULE_DIRECT_IDENTIFIER,
                ColumnClass::SensitiveAttribute => RULE_SENSITIVE_ROW_LEVEL,
                _ => RULE_QUASI_COMBINATION,
            };
            note(rule_id, format!("출력에서 '{}' 컬럼을 제거했습니다.", name));
        }
        new_ops.push(PipelineOp::Select(keep.clone()));
    }

    if needs_dp {
        note(
            RULE_AGGREGATE_WITHOUT_DP,
            format!(
                "민감 속성 집계에 차등 프라이버시를 적용했습니다: withDp(epsilon: {}, mechanism: laplace).",
                printer::print_f64(policy.remediation_epsilon)
            ),
        );
        new_ops.push(PipelineOp::WithDp(DpArgs {
            epsilon: policy.remediation_epsilon,
            mechanism: DpMechanism::Laplace,
            sensitivity: 1.0,
            delta: None,
            seed: None,
        }));
    }

    new_ops.extend_from_slice(&ops[split_at..]);
    new_ops
}

// ── 유닛 테스트 ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const PATIENT_SCHEMA: &str = "type Patient = {
    patient_id: string,
    name: string,
    age: int,
    gender: string,
    zip_code: string,
    disease: string,
    age_band: string,
};
";

    fn fix(pipeline: &str) -> Remediation {
        let src = format!("{}{}", PATIENT_SCHEMA, pipeline);
        remediate(&src, &Policy::builtin())
    }

    /// 직접 식별자를 제거하고 검증까지 통과한다.
    #[test]
    fn removes_direct_identifier_and_verifies() {
        let r = fix("v out = load(\"data/p.csv\") :: Patient |> select([name, age_band]);");
        assert!(r.verified, "보정 후 검증 실패: {}", r.report_after.render());
        // 스키마 선언에는 name 필드가 남아 있어도 되지만, 파이프라인 투영에서는 빠져야 한다.
        assert!(
            r.code.contains("select([age_band])"),
            "안전한 투영이 생성되지 않음:\n{}",
            r.code
        );
        assert!(
            !r.code.contains("select([name"),
            "보정 코드가 여전히 name 을 투영함:\n{}",
            r.code
        );
        assert!(!r.applied.is_empty());
    }

    /// 보정된 코드는 반드시 다시 파싱된다 (구문 무결성).
    #[test]
    fn remediated_code_reparses() {
        let r = fix("v out = load(\"data/p.csv\") :: Patient |> filter(age > 30);");
        assert!(
            parse_program(&r.code).is_ok(),
            "보정 코드가 파싱되지 않음:\n{}",
            r.code
        );
    }

    /// 민감 속성 집계에는 withDp 가 자동 삽입된다.
    #[test]
    fn injects_with_dp_for_sensitive_aggregate() {
        let r = fix("v out = load(\"data/p.csv\") :: Patient
    |> groupBy(\"disease\")
    |> count(\"patient_id\");");
        assert!(r.code.contains("withDp"), "withDp 미삽입:\n{}", r.code);
        assert!(r.verified, "{}", r.report_after.render());
        assert!(
            r.applied
                .iter()
                .any(|f| f.rule_id == RULE_AGGREGATE_WITHOUT_DP)
        );
    }

    /// ε 상한 초과는 상한값으로 클램프된다.
    #[test]
    fn clamps_excessive_epsilon() {
        let r = fix("v out = load(\"data/p.csv\") :: Patient
    |> groupBy(\"disease\")
    |> count(\"patient_id\")
    |> withDp(epsilon: 50.0);");
        assert!(r.verified, "{}", r.report_after.render());
        assert!(r.code.contains("epsilon: 3"), "클램프 실패:\n{}", r.code);
    }

    /// 준식별자 결합은 임계치 미만으로 줄인다.
    #[test]
    fn reduces_quasi_identifier_combination() {
        let r = fix("v out = load(\"data/p.csv\") :: Patient |> select([age, gender, zip_code]);");
        assert!(r.verified, "{}", r.report_after.render());
    }

    /// 하드코딩된 비밀키는 자동 보정하지 않고 residual 로 남긴다.
    #[test]
    fn hardcoded_secret_is_left_as_residual() {
        let r = fix("// AKIAIOSFODNN7EXAMPLE
v out = load(\"data/p.csv\") :: Patient |> select([age_band]);");
        assert!(!r.verified, "비밀키가 있는데 안전하다고 판정됨");
        assert!(
            !r.residual.is_empty(),
            "residual 이 비어 있음 — 사람이 처리해야 할 위반이 사라졌다"
        );
    }

    /// 이미 안전한 코드는 그대로 반환된다.
    #[test]
    fn safe_code_is_returned_unchanged() {
        let src = format!(
            "{}v out = load(\"data/p.csv\") :: Patient |> select([age_band]);",
            PATIENT_SCHEMA
        );
        let r = remediate(&src, &Policy::builtin());
        assert!(r.verified);
        assert_eq!(r.code, src);
        assert!(r.applied.is_empty());
    }

    /// 남길 컬럼이 하나도 없으면 투영으로 고치지 않는다 (거짓 안전 방지).
    #[test]
    fn does_not_fabricate_empty_projection() {
        let r = fix("v out = load(\"data/p.csv\") :: Patient |> select([name, patient_id]);");
        assert!(!r.code.contains("select([])"), "빈 투영 생성:\n{}", r.code);
        assert!(!r.verified, "고칠 수 없는데 안전하다고 판정됨");
    }

    /// 차트가 제거 대상 컬럼을 참조하면 코드를 망가뜨리지 않는다.
    #[test]
    fn does_not_break_chart_referencing_dropped_column() {
        let r = fix("v out = load(\"data/p.csv\") :: Patient
    |> select([name, age_band])
    |> chart {
        type: bar
        x: name
        y: age_band
    };");
        assert!(
            parse_program(&r.code).is_ok(),
            "차트 보정으로 구문이 깨짐:\n{}",
            r.code
        );
        assert!(!r.verified, "차트가 식별자를 그리는데 안전하다고 판정됨");
    }

    /// 정상 파이프라인에 대해서는 아무것도 바꾸지 않는다.
    #[test]
    fn leaves_clean_air_quality_pipeline_untouched() {
        let src = "type AQ = { station: string, pm10: Option<float> };
v x = load(\"examples/data/seoul_air_2024.csv\") :: AQ
    |> groupBy(\"station\")
    |> mean(\"pm10\");
";
        let r = remediate(src, &Policy::builtin());
        assert!(r.verified);
        assert_eq!(r.code, src);
    }
}
