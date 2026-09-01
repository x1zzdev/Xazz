// src/policy_cli.rs — front-end guardrail for `xazz policy` and `xazz run` (issue #2)
//
// The CLI does not link Polars, so this only performs pure static analysis
// (xazz-compiler::policy). The execution engine (xazz-exec) also has the same
// gate, so skipping this check would eventually be blocked anyway; but blocking
// here first means
//
//   · it fails immediately before spawning a subprocess, giving fast feedback, and
//   · temporary files containing violating code never reach the execution engine.

use std::path::Path;

use colored::Colorize;
use xazz_compiler::policy::{self, Policy, PolicyReport, Remediation};

/// Load the policy. On failure, return a blocking report instead (fail-closed).
fn active_policy() -> Result<(Policy, String), Box<PolicyReport>> {
    match policy::load_active_policy() {
        Ok(active) => Ok((active.policy, active.origin)),
        Err(e) => Err(Box::new(policy::policy_load_failure_report(&e))),
    }
}

/// Read the source file.
fn read_source(file: &Path) -> Result<String, String> {
    std::fs::read_to_string(file)
        .map_err(|e| format!("IO error: failed to read file '{}' — {}", file.display(), e))
}

/// Gate called right before `xazz run`.
///
/// If there are violations, print the reason and exit the process (code 1).
/// On pass, return silently — so normal run output stays clean, and
/// only warnings are written to stderr.
pub fn gate_before_run(source_path: &str, json: bool) {
    let source = match read_source(Path::new(source_path)) {
        Ok(s) => s,
        // The existing run path already handles file read failures — pass through here.
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
            "[policy warning]".yellow().bold(),
            w.rule_id,
            w.message
        );
    }
    if !report.warnings.is_empty() {
        eprintln!("           policy: {} ({})", policy.id, origin);
    }
}

/// Print the block reason in a human- or machine-readable form.
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
        "[xazz security guardrail] execution blocked".red().bold()
    );
    eprintln!("─────────────────────────────────────────────");
    eprintln!("source : {}", source_path);
    eprintln!("policy : {} v{}", report.policy_id, report.policy_version);
    eprintln!();
    for v in &report.violations {
        eprintln!("  {} {} {}", "✖".red(), v.rule_id.bold(), v.rule_name);
        eprintln!("    {} : {}", label_reason(), v.message);
        eprintln!("    {}    : {}", label_fix(), v.remediation_hint);
        if let Some(src) = &v.source_ref {
            eprintln!("    {}  : {}", label_basis(), src);
        }
        eprintln!();
    }
    for w in &report.warnings {
        eprintln!("  {} {} {}", "!".yellow(), w.rule_id, w.message);
    }
    eprintln!(
        "→ {}: {}",
        if xazz_compiler::is_korean() {
            "안전한 대체 코드를 보려면"
        } else {
            "to review a safe alternative, run"
        },
        format!("xazz policy {} --fix", source_path).cyan()
    );
}

/// Body of the `xazz policy <file>` subcommand.
///
/// Return value is the process exit code (0 = pass, 1 = violation/error).
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

    // If --out is given, save the remediation code to a file.
    if let (Some(path), Some(rem)) = (out, remediation.as_ref()) {
        if let Err(e) = std::fs::write(path, &rem.code) {
            eprintln!(
                "IO error: failed to save remediation code '{}' — {}",
                path.display(),
                e
            );
            return 1;
        }
        if !json {
            println!("remediation code saved to: {}", path.display());
        }
    }

    print_result(&report, remediation.as_ref(), &origin, json);

    if report.safe_to_execute { 0 } else { 1 }
}

fn origin_unknown() -> String {
    "unknown".to_string()
}

fn label_reason() -> &'static str {
    if xazz_compiler::is_korean() {
        "사유"
    } else {
        "reason"
    }
}

fn label_fix() -> &'static str {
    if xazz_compiler::is_korean() {
        "보정"
    } else {
        "fix"
    }
}

fn label_columns() -> &'static str {
    if xazz_compiler::is_korean() {
        "컬럼"
    } else {
        "columns"
    }
}

fn label_basis() -> &'static str {
    if xazz_compiler::is_korean() {
        "근거"
    } else {
        "basis"
    }
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
        "{}  policy {} v{}  ({})",
        "⛨ Policy-as-Code".bold(),
        report.policy_id,
        report.policy_version,
        origin
    );
    println!(
        "   domain {} · risk {}",
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
        println!("    {} : {}", label_reason(), v.message);
        println!("    {}    : {}", label_fix(), v.remediation_hint);
        if !v.columns.is_empty() {
            println!("    {}: {}", label_columns(), v.columns.join(", "));
        }
        if let Some(src) = &v.source_ref {
            println!("    {}  : {}", label_basis(), src);
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
    println!("{}", "── remediation proposal ───────────────".bold());
    println!("strategy : {}", rem.strategy);
    println!(
        "verified : {}",
        if rem.verified {
            "passed — remediated code satisfies the policy"
                .green()
                .to_string()
        } else {
            "not verified — manual review required".red().to_string()
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
            "  {} [{}] {} — cannot auto-remediate",
            "✖".red(),
            residual.rule_id,
            residual.message
        );
    }
    if !rem.applied.is_empty() {
        println!();
        println!("{}", "── remediated code ──────────────────────".bold());
        println!("{}", rem.code);
    }
}
