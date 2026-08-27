// src/policy_cli.rs — `xazz policy` 및 `xazz run` 앞단 가드레일 (issue #2)
//
// CLI 는 Polars 를 링크하지 않으므로, 여기서 하는 일은 순수 정적 분석뿐이다
// (xazz-compiler::policy). 실행 엔진(xazz-exec)에도 동일한 게이트가 걸려 있어
// 이 검사를 건너뛰어도 결국 차단되지만, 여기서 먼저 막으면
//
//   · 서브프로세스를 띄우기 전에 즉시 실패해 피드백이 빠르고,
//   · 위반 코드가 담긴 임시 파일이 실행 엔진까지 흘러가지 않는다.

use std::path::Path;

use colored::Colorize;
use xazz_compiler::policy::{self, Policy, PolicyReport, Remediation};

/// 정책을 불러온다. 실패하면 차단 리포트로 바꿔 돌려준다 (fail-closed).
fn active_policy() -> Result<(Policy, String), Box<PolicyReport>> {
    match policy::load_active_policy() {
        Ok(active) => Ok((active.policy, active.origin)),
        Err(e) => Err(Box::new(policy::policy_load_failure_report(&e))),
    }
}

/// 소스 파일을 읽는다.
fn read_source(file: &Path) -> Result<String, String> {
    std::fs::read_to_string(file)
        .map_err(|e| format!("IO 에러: 파일 읽기 실패 '{}' — {}", file.display(), e))
}

/// `xazz run` 직전에 호출하는 게이트.
///
/// 위반이 있으면 사유를 출력하고 프로세스를 종료한다(코드 1).
/// 통과하면 조용히 반환한다 — 정상 실행 경로의 출력이 지저분해지지 않도록,
/// 경고가 있을 때만 stderr 에 남긴다.
pub fn gate_before_run(source_path: &str, json: bool) {
    let source = match read_source(Path::new(source_path)) {
        Ok(s) => s,
        // 파일 읽기 실패는 기존 run 경로가 이미 처리한다 — 여기서는 통과시킨다.
        Err(_) => return,
    };

    let (policy, origin) = match active_policy() {
        Ok(v) => v,
        Err(report) => {
            emit_block(&report, source_path, json);
            std::process::exit(1);
        }
    };

    let report = policy::analyze(&source, &policy);

    if !report.safe_to_execute {
        emit_block(&report, source_path, json);
        std::process::exit(1);
    }

    for w in &report.warnings {
        eprintln!(
            "{} {} {}",
            "[정책 경고]".yellow().bold(),
            w.rule_id,
            w.message
        );
    }
    if !report.warnings.is_empty() {
        eprintln!("           정책: {} ({})", policy.id, origin);
    }
}

/// 차단 사유를 사람 또는 기계가 읽을 수 있게 출력한다.
fn emit_block(report: &PolicyReport, source_path: &str, json: bool) {
    if json {
        let out = serde_json::json!({
            "success": false,
            "blocked_by": "policy-guardrail",
            "source": source_path,
            "policy": report,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return;
    }

    eprintln!();
    eprintln!(
        "{}",
        "[xazz 보안 가드레일] 실행이 차단되었습니다".red().bold()
    );
    eprintln!("─────────────────────────────────────────────");
    eprintln!("소스   : {}", source_path);
    eprintln!("정책   : {} v{}", report.policy_id, report.policy_version);
    eprintln!();
    for v in &report.violations {
        eprintln!("  {} {} {}", "✖".red(), v.rule_id.bold(), v.rule_name);
        eprintln!("    사유 : {}", v.message);
        eprintln!("    보정 : {}", v.remediation_hint);
        if let Some(src) = &v.source_ref {
            eprintln!("    근거 : {}", src);
        }
        eprintln!();
    }
    for w in &report.warnings {
        eprintln!("  {} {} {}", "!".yellow(), w.rule_id, w.message);
    }
    eprintln!(
        "→ 안전한 대체 코드를 보려면: {}",
        format!("xazz policy {} --fix", source_path).cyan()
    );
}

/// `xazz policy <file>` 서브커맨드 본체.
///
/// 반환값은 프로세스 종료 코드다 (0 = 통과, 1 = 위반/오류).
pub fn run_policy_command(file: &Path, json: bool, fix: bool, out: Option<&Path>) -> i32 {
    let source = match read_source(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };

    let (policy, origin) = match active_policy() {
        Ok(v) => v,
        Err(report) => {
            print_result(&report, None, &origin_unknown(), json);
            return 1;
        }
    };

    let report = policy::analyze(&source, &policy);
    let remediation = if fix {
        Some(policy::remediate(&source, &policy))
    } else {
        None
    };

    // --out 이 주어지면 보정 코드를 파일로 저장한다.
    if let (Some(path), Some(rem)) = (out, remediation.as_ref()) {
        if let Err(e) = std::fs::write(path, &rem.code) {
            eprintln!("IO 에러: 보정 코드 저장 실패 '{}' — {}", path.display(), e);
            return 1;
        }
        if !json {
            println!("보정 코드를 저장했습니다: {}", path.display());
        }
    }

    print_result(&report, remediation.as_ref(), &origin, json);

    if report.safe_to_execute { 0 } else { 1 }
}

fn origin_unknown() -> String {
    "unknown".to_string()
}

fn print_result(
    report: &PolicyReport,
    remediation: Option<&Remediation>,
    origin: &str,
    json: bool,
) {
    if json {
        let out = serde_json::json!({
            "success": report.safe_to_execute,
            "policy_origin": origin,
            "policy": report,
            "remediation": remediation,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return;
    }

    println!();
    println!(
        "{}  정책 {} v{}  ({})",
        "⛨ Policy-as-Code".bold(),
        report.policy_id,
        report.policy_version,
        origin
    );
    println!(
        "   도메인 {} · 위험도 {}",
        report.domain,
        report.risk_level.label()
    );
    println!("─────────────────────────────────────────────");

    if report.safe_to_execute {
        println!("{} {}", "✔".green().bold(), report.summary());
    } else {
        println!("{} {}", "✖".red().bold(), report.summary());
    }
    println!();

    for v in &report.violations {
        println!("  {} {} {}", "✖".red(), v.rule_id.bold(), v.rule_name);
        println!("    사유 : {}", v.message);
        println!("    보정 : {}", v.remediation_hint);
        if !v.columns.is_empty() {
            println!("    컬럼 : {}", v.columns.join(", "));
        }
        if let Some(src) = &v.source_ref {
            println!("    근거 : {}", src);
        }
        println!();
    }
    for w in &report.warnings {
        println!("  {} {} {}", "!".yellow(), w.rule_id, w.message);
    }

    let Some(rem) = remediation else {
        return;
    };

    println!();
    println!("{}", "── 자동 보정 제안 ───────────────────────────".bold());
    println!("전략   : {}", rem.strategy);
    println!(
        "검증   : {}",
        if rem.verified {
            "통과 — 보정 코드가 정책을 만족합니다".green().to_string()
        } else {
            "미통과 — 사람이 처리해야 할 위반이 남아 있습니다"
                .red()
                .to_string()
        }
    );
    for fix in &rem.applied {
        println!("  · [{}] {}", fix.rule_id, fix.description);
    }
    for note in &rem.notes {
        println!("  ⓘ {}", note);
    }
    for residual in &rem.residual {
        println!(
            "  {} [{}] {} — 자동 보정 불가",
            "✖".red(),
            residual.rule_id,
            residual.message
        );
    }
    if !rem.applied.is_empty() {
        println!();
        println!("{}", "── 보정된 코드 ──────────────────────────────".bold());
        println!("{}", rem.code);
    }
}
