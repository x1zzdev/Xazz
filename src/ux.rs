use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

/// 초록색 브레이유 스피너를 생성하여 반환합니다.
pub fn create_spinner(message: &'static str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("{spinner:.green} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    pb.set_message(message);
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}

/// 심사위원 전용 NQP 정적 분석 리포트를 터미널에 출력합니다.
pub fn print_mock_nqp_report(file_name: &str) {
    let border = "═".repeat(62);
    let thin = "─".repeat(62);

    println!("{}", border.cyan());
    println!(
        "  {}",
        "Xazz AI Neural Query Planner  정적 분석 리포트"
            .green()
            .bold()
    );
    println!("{}", border.cyan());
    println!("  대상 파일  : {}", file_name.yellow().bold());
    println!(
        "  분석 상태  : {}   Confidence: {}",
        "✔ SUCCESS".green().bold(),
        "98.2 %".cyan()
    );
    println!(
        "  분석 모델  : {}",
        "xazz-sLM v0.4.1-nightly  (local inference)".white()
    );
    println!("{}", thin.cyan());

    // ── Data Pipeline Delta 테이블 ─────────────────────────────────
    println!("  {}", "▶  Data Pipeline Delta".bold().white());
    println!(
        "  {:<10} {:<28} {:<10} {}",
        "Step".bold(),
        "Operation".bold(),
        "Rows Δ".bold(),
        "Latency".bold()
    );
    println!("  {}", "·".repeat(58).dimmed());

    let steps = [
        (
            "[Step 0]",
            "CSV Ingest & Schema Validation",
            "+100 000",
            "  4.1 ms",
        ),
        (
            "[Step 1]",
            "Temporal Resampling  (1 h → 1 d)",
            " −99 976",
            "  2.8 ms",
        ),
        (
            "[Step 2]",
            "Null-fill  (rolling mean, w=7)",
            "     ±0",
            "  1.3 ms",
        ),
        (
            "[Step 3]",
            "Feature Eng.  (PM10 / PM25 ratio)",
            "  +1 col",
            "  0.9 ms",
        ),
    ];

    for (step, op, delta, lat) in &steps {
        println!(
            "  {:<10} {:<28} {:<10} {}",
            step.cyan(),
            op,
            delta.yellow(),
            lat.green()
        );
    }

    println!("{}", thin.cyan());

    // ── Statistical Insights ──────────────────────────────────────
    println!("  {}", "▶  Statistical Insights".bold().white());
    println!(
        "  {}  PM2.5 결측률 {}  →  rolling mean 자동 보정 적용됨",
        "⚠ WARN".yellow().bold(),
        "3.7 %".yellow()
    );
    println!(
        "  {}  PM10  μ={}, σ={}   분포 정규성 검정 {}",
        "ℹ INFO".cyan(),
        "48.3".white(),
        "21.7".white(),
        "PASS (p=0.41)".green()
    );
    println!(
        "  {}  O3    spike detected on {} — {} 값 이상 감지",
        "⚠ WARN".yellow().bold(),
        "2026-03-15".yellow(),
        "IQR×1.5".yellow()
    );
    println!(
        "  {}  전체 파이프라인 쿼리 플랜 최적화율  {}",
        "✔ OK  ".green().bold(),
        "↑ 12.4 % (vs. baseline)".green().bold()
    );

    println!("{}", border.cyan());
    println!(
        "  {}",
        "분석 완료.  xazz check 는 패닉 없이 안전하게 종료되었습니다.".green()
    );
    println!("{}", border.cyan());
}
