// xazz-compiler/src/policy/remediate.rs — deterministic auto-remediation (issue #2)
//
// The guardrail does not end at "blocking". Once a violation is confirmed, a **safe
// replacement snippet** must be offered so the developer has a next step.
//
// Remediation has two layers.
//
//   1. This module — deterministic remediation that rewrites the AST directly. Always
//      works, always produces the same result, and needs neither a model nor a network.
//   2. sLM (Qwen2.5-Coder) — suggests a more natural rewrite. However, its output is
//      **always re-validated by this engine**, and on failure it falls back to layer 1.
//      (xazz-server/src/slm.rs)
//
// Safety principles
//   · Code is never edited as strings — the AST is modified and re-printed via the printer.
//   · Violations that cannot be fixed automatically (e.g. hardcoded secrets) are not
//     silently skipped; they are left in `residual` for a human to handle.
//   · Remediation results are re-analyzed and proven via `verified`. Unproven code is
//     never called "safe".

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::ast::{DpArgs, DpMechanism, PipelineOp, Program, Stmt, StructField};
use xazz_core::i18n::{is_korean, tr};

use super::rules::{PipelineShape, chart_columns, infer_shape};
use super::{
    ColumnClass, Policy, PolicyReport, RULE_AGGREGATE_WITHOUT_DP, RULE_DIRECT_IDENTIFIER,
    RULE_EPSILON_TOO_LARGE, RULE_QUASI_COMBINATION, RULE_SENSITIVE_ROW_LEVEL, Violation, analyze,
    printer,
};

/// One applied fix.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliedFix {
    /// Rule ID resolved by this fix
    pub rule_id: String,
    /// What was done (in Korean)
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statement_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variable: Option<String>,
}

/// Result of auto-remediation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Remediation {
    /// Which engine produced it — `deterministic` | `slm` | `slm-rejected`
    pub strategy: String,
    /// The remediated `.xzz` source
    pub code: String,
    /// The list of applied fixes
    pub applied: Vec<AppliedFix>,
    /// Violations that cannot be auto-fixed and need a human
    pub residual: Vec<Violation>,
    /// Side effects the developer should know about during remediation (e.g. comments not preserved)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// Whether the remediation result passes the policy — if false, never call it safe.
    pub verified: bool,
    /// The re-analysis report after remediation
    pub report_after: PolicyReport,
}

/// Rules that cannot be auto-fixed — the value itself must be deleted, so a human must decide.
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

/// Performs deterministic remediation on source with confirmed violations.
///
/// If the original is already safe, it is returned unchanged.
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

    // If it cannot even be parsed, the AST cannot be fixed.
    if before.parse_error.is_some() {
        return Remediation {
            strategy: "deterministic".to_string(),
            code: source.to_string(),
            applied: Vec::new(),
            residual: before.violations.clone(),
            notes: vec![
                tr(
                    "could not apply auto-remediation due to a syntax error; fix the syntax first",
                    "구문 오류가 있어 자동 보정을 적용하지 못했습니다. 먼저 구문을 고쳐야 합니다",
                )
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
            notes: vec![
                tr(
                    "source could not be parsed, so auto-remediation was skipped",
                    "소스를 파싱하지 못해 자동 보정을 건너뛰었습니다",
                )
                .to_string(),
            ],
            verified: false,
            report_after: before,
        };
    };

    let (fixed_program, applied) = rewrite_program(&program, policy);
    let code = printer::print_program(&fixed_program);
    let report_after = analyze(&code, policy);

    // Violations not eligible for auto-remediation remain as-is.
    let residual: Vec<Violation> = before
        .violations
        .iter()
        .filter(|v| !is_auto_fixable(&v.rule_id))
        .cloned()
        .collect();

    let mut notes: Vec<String> = Vec::new();
    if source.contains("//") {
        notes.push(
            tr(
                "remediated code is regenerated from the AST, so original comments and formatting are not preserved",
                "보정 코드는 AST 에서 다시 생성되므로 원본 주석과 서식은 보존되지 않습니다"
            )
            .to_string(),
        );
    }
    if !residual.is_empty() {
        notes.push(
            tr(
                "residual violations remain that auto-remediation cannot resolve. Hardcoded secrets are not fixed by merely deleting them from source — exposed credentials must be revoked and reissued immediately",
                "자동 보정으로 해소할 수 없는 위반이 남아 있습니다. 하드코딩된 비밀값은 소스에서 제거하는 것만으로 끝나지 않으며, 노출된 자격증명은 즉시 폐기·재발급해야 합니다"
            )
            .to_string(),
        );
    }

    // If violations remain that a human must handle, do not call it "safe".
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

// ── Program rewriting ──────────────────────────────────────────────────────────

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

// ── Pipeline rewriting ────────────────────────────────────────────────────────

fn rewrite_ops(
    ops: &[PipelineOp],
    shape: &PipelineShape,
    policy: &Policy,
    index: usize,
    var: Option<&str>,
    applied: &mut Vec<AppliedFix>,
) -> Vec<PipelineOp> {
    // If the column set could not be determined, leave it alone — do not change code by guessing.
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

    // ── 1) Decide which columns to drop ───────────────────────────────────────────
    let mut drop: Vec<String> = Vec::new();
    let mut quasi_seen = 0usize;

    for col in shape.columns.iter().filter(|c| !c.aggregated) {
        match policy.classify(&col.name) {
            ColumnClass::DirectIdentifier => drop.push(col.name.clone()),
            ColumnClass::SensitiveAttribute if !shape.aggregated => drop.push(col.name.clone()),
            ColumnClass::QuasiIdentifier => {
                quasi_seen += 1;
                // Keep only below the threshold and drop the rest.
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

    // If a chart references a column being dropped, projection cannot fix it — keep the original.
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

    // ── 2) Decide differential-privacy remediation ───────────────────────────────────────
    let sensitive_aggregate = shape.aggregated
        && shape
            .columns
            .iter()
            .any(|c| policy.classify(&c.name) == ColumnClass::SensitiveAttribute);
    let needs_dp =
        sensitive_aggregate && shape.dp.is_none() && policy.require_dp_for_sensitive_aggregate;

    // ── 3) Build the new operator list ─────────────────────────────────────────────
    // Keep the chart always last; insert projection and DP before it.
    let split_at = ops
        .iter()
        .position(|op| matches!(op, PipelineOp::Chart(_)))
        .unwrap_or(ops.len());

    let mut new_ops: Vec<PipelineOp> = Vec::with_capacity(ops.len() + 2);
    for op in &ops[..split_at] {
        // Clamp ε over the cap in place.
        // NaN is also treated as a "cap violation" — it is not caught by comparison alone.
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
                    if is_korean() {
                        format!(
                            "프라이버시 예산을 정책 상한으로 낮췄습니다: ε {} → {}",
                            printer::print_f64(args.epsilon),
                            printer::print_f64(policy.max_epsilon)
                        )
                    } else {
                        format!(
                            "clamped the privacy budget to the policy cap: ε {} → {}",
                            printer::print_f64(args.epsilon),
                            printer::print_f64(policy.max_epsilon)
                        )
                    },
                );
                new_ops.push(PipelineOp::WithDp(clamped));
                continue;
            }
        }
        new_ops.push(op.clone());
    }

    if projection_possible {
        // If the last op is already a select, replace it rather than stacking a duplicate projection.
        if matches!(new_ops.last(), Some(PipelineOp::Select(_))) {
            new_ops.pop();
        }
        for name in &drop {
            let rule_id = match policy.classify(name) {
                ColumnClass::DirectIdentifier => RULE_DIRECT_IDENTIFIER,
                ColumnClass::SensitiveAttribute => RULE_SENSITIVE_ROW_LEVEL,
                _ => RULE_QUASI_COMBINATION,
            };
            note(
                rule_id,
                if is_korean() {
                    format!("출력에서 '{}' 컬럼을 제거했습니다.", name)
                } else {
                    format!("removed the '{}' column from output.", name)
                },
            );
        }
        new_ops.push(PipelineOp::Select(keep.clone()));
    }

    if needs_dp {
        note(
            RULE_AGGREGATE_WITHOUT_DP,
            if is_korean() {
                format!(
                    "민감 속성 집계에 차등 프라이버시를 적용했습니다: withDp(epsilon: {}, mechanism: laplace).",
                    printer::print_f64(policy.remediation_epsilon)
                )
            } else {
                format!(
                    "applied differential privacy to the sensitive aggregate: withDp(epsilon: {}, mechanism: laplace).",
                    printer::print_f64(policy.remediation_epsilon)
                )
            },
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

// ── Unit tests ──────────────────────────────────────────────────────────────

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

    /// Removes the direct identifier and passes verification.
    #[test]
    fn removes_direct_identifier_and_verifies() {
        let r = fix("v out = load(\"data/p.csv\") :: Patient |> select([name, age_band]);");
        assert!(r.verified, "보정 후 검증 실패: {}", r.report_after.render());
        // The name field may remain in the schema declaration, but must be absent from the pipeline projection.
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

    /// The remediated code must re-parse (syntactic integrity).
    #[test]
    fn remediated_code_reparses() {
        let r = fix("v out = load(\"data/p.csv\") :: Patient |> filter(age > 30);");
        assert!(
            parse_program(&r.code).is_ok(),
            "보정 코드가 파싱되지 않음:\n{}",
            r.code
        );
    }

    /// withDp is auto-inserted for sensitive-attribute aggregates.
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

    /// ε over the cap is clamped to the cap value.
    #[test]
    fn clamps_excessive_epsilon() {
        let r = fix("v out = load(\"data/p.csv\") :: Patient
    |> groupBy(\"disease\")
    |> count(\"patient_id\")
    |> withDp(epsilon: 50.0);");
        assert!(r.verified, "{}", r.report_after.render());
        assert!(r.code.contains("epsilon: 3"), "클램프 실패:\n{}", r.code);
    }

    /// Quasi-identifier combinations are reduced below the threshold.
    #[test]
    fn reduces_quasi_identifier_combination() {
        let r = fix("v out = load(\"data/p.csv\") :: Patient |> select([age, gender, zip_code]);");
        assert!(r.verified, "{}", r.report_after.render());
    }

    /// Hardcoded secrets are not auto-fixed and are left as residual.
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

    /// Already-safe code is returned unchanged.
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

    /// If no column remains, it is not fixed with a projection (prevents false safety).
    #[test]
    fn does_not_fabricate_empty_projection() {
        let r = fix("v out = load(\"data/p.csv\") :: Patient |> select([name, patient_id]);");
        assert!(!r.code.contains("select([])"), "빈 투영 생성:\n{}", r.code);
        assert!(!r.verified, "고칠 수 없는데 안전하다고 판정됨");
    }

    /// A chart referencing a dropped column does not break the code.
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

    /// Nothing is changed for a normal pipeline.
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
