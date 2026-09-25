// xazz-exec integration tests — DP budget accounting from source to ledger (issue #119).
//
// `PrivacyBudget::spend_n(…, count)` must be charged once per noised column. The
// unit tests in `dp.rs` cover `spend_n` itself; these tests pin the *runtime*
// path: a real `.xzz` script goes through the engine binary and the `[xazz:dp]`
// marker (which is what the server ledger and `xazz run --json` consume) must
// show `count == noised columns`, `budget_spent == count · ε`, and the budget
// rejection must trigger on the same multiplied amount.
//
// The engine is run as a subprocess so each test gets its own `XAZZ_DP_BUDGET`
// without touching process-global env, and so stdout markers can be captured.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

const DEFAULT_BUDGET: f64 = 10.0;

/// Two numeric columns per station so a single `withDp` noises two aggregates.
const CSV: &str = "station,pm10,pm25\n\
gangnam,80,40\n\
gangnam,40,20\n\
seocho,120,60\n\
seocho,100,50\n";

const SCHEMA: &str = "type AQ = { station: string, pm10: float, pm25: float };\n";

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "xazz_dp_budget_{}_{}_{}",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes the CSV + script into a fresh temp dir and runs the engine binary
/// there (relative `load()` path, as the policy gate requires).
fn run_script(script: &str, envs: &[(&str, &str)]) -> Output {
    let dir = temp_dir();
    std::fs::write(dir.join("data.csv"), CSV).unwrap();
    std::fs::write(dir.join("pipeline.xzz"), format!("{SCHEMA}{script}")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_xazz-exec"))
        .arg("pipeline.xzz")
        .current_dir(&dir)
        .env("XAZZ_LANG", "en")
        .env_remove("XAZZ_DP_BUDGET")
        .env_remove("XAZZ_DP_DELTA_BUDGET")
        .envs(envs.iter().copied())
        .output()
        .expect("spawn xazz-exec");
    let _ = std::fs::remove_dir_all(&dir);
    output
}

/// Every `[xazz:dp]` marker on stdout, in emission order.
fn dp_markers(output: &Output) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|l| l.trim().strip_prefix("[xazz:dp] "))
        .map(|j| serde_json::from_str(j).expect("[xazz:dp] payload is JSON"))
        .collect()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// `groupBy` + a two-column aggregate + one `withDp(ε)`.
fn two_column_script(epsilon: &str) -> String {
    format!(
        "v s = load(\"data.csv\") :: AQ
           |> groupBy(\"station\")
           |> agg([mean(\"pm10\"), mean(\"pm25\")])
           |> withDp(epsilon: {epsilon}, seed: 7);"
    )
}

fn one_column_script(epsilon: &str) -> String {
    format!(
        "v s = load(\"data.csv\") :: AQ
           |> groupBy(\"station\")
           |> mean(\"pm10\")
           |> withDp(epsilon: {epsilon}, seed: 7);"
    )
}

/// Column count reaches the ledger: two noised columns ⇒ ε charged twice.
#[test]
fn multi_column_with_dp_charges_epsilon_per_column() {
    let out = run_script(&two_column_script("1.0"), &[]);
    assert!(out.status.success(), "run failed:\n{}", stderr_of(&out));

    let markers = dp_markers(&out);
    assert_eq!(markers.len(), 1, "exactly one withDp step");
    let dp = &markers[0];

    let noised: Vec<&str> = dp["noised_columns"]
        .as_array()
        .expect("noised_columns array")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(noised.len(), 2, "both aggregate columns noised: {noised:?}");
    assert!(noised.contains(&"pm10_mean") && noised.contains(&"pm25_mean"));

    assert!(approx(dp["epsilon"].as_f64().unwrap(), 1.0));
    assert_eq!(dp["query_count"], 2, "one composed mechanism per column");
    assert!(
        approx(dp["budget_spent"].as_f64().unwrap(), 2.0),
        "ε must be charged k=2 times, got {}",
        dp["budget_spent"]
    );
    assert!(approx(dp["budget_total"].as_f64().unwrap(), DEFAULT_BUDGET));
    // Laplace spends no δ.
    assert!(approx(dp["budget_spent_delta"].as_f64().unwrap(), 0.0));
}

/// Control: a single noised column is charged exactly once.
#[test]
fn single_column_with_dp_charges_epsilon_once() {
    let out = run_script(&one_column_script("1.0"), &[]);
    assert!(out.status.success(), "run failed:\n{}", stderr_of(&out));

    let markers = dp_markers(&out);
    assert_eq!(markers.len(), 1);
    let dp = &markers[0];
    assert_eq!(dp["noised_columns"].as_array().unwrap().len(), 1);
    assert_eq!(dp["query_count"], 1);
    assert!(approx(dp["budget_spent"].as_f64().unwrap(), 1.0));
}

/// Boundary: a budget of exactly k·ε is allowed, anything below is rejected —
/// and the rejection happens *before* any budget is recorded.
#[test]
fn budget_boundary_uses_multiplied_charge() {
    // k·ε = 2.0 exactly → allowed, budget fully consumed.
    let ok = run_script(&two_column_script("1.0"), &[("XAZZ_DP_BUDGET", "2.0")]);
    assert!(
        ok.status.success(),
        "k·ε == budget must pass:\n{}",
        stderr_of(&ok)
    );
    let dp = &dp_markers(&ok)[0];
    assert!(approx(dp["budget_spent"].as_f64().unwrap(), 2.0));
    assert!(approx(dp["budget_total"].as_f64().unwrap(), 2.0));

    // Budget 1.999 < 2.0: would pass if the runtime under-counted (charged ε once).
    //
    // The refusal is observable as the pipeline's runtime error plus the absence
    // of the marker. The engine currently logs a failed pipeline and keeps going
    // (exit code stays 0), so the exit status is deliberately not asserted here.
    let rejected = run_script(&two_column_script("1.0"), &[("XAZZ_DP_BUDGET", "1.999")]);
    let err = stderr_of(&rejected);
    assert!(
        err.contains("[xazz RUNTIME ERROR]") && err.contains("XAZZ_DP_BUDGET"),
        "k·ε > budget must be refused with the budget error (under-counting would let it through):\n{err}"
    );
    // The refusal message carries the column multiplier the runtime handed to the ledger.
    assert!(
        err.contains("× 2"),
        "rejection must reflect count=2 columns, got:\n{err}"
    );
    assert!(
        dp_markers(&rejected).is_empty(),
        "no [xazz:dp] marker may be emitted when the step is refused"
    );
}

/// Sequential composition across steps: 2-column ε=1.0 then 1-column ε=0.5
/// accumulates to 2.5 over three composed mechanisms.
#[test]
fn composition_across_two_with_dp_steps_accumulates_per_column() {
    let script = "v a = load(\"data.csv\") :: AQ
           |> groupBy(\"station\")
           |> agg([mean(\"pm10\"), mean(\"pm25\")])
           |> withDp(epsilon: 1.0, seed: 7);
         v b = load(\"data.csv\") :: AQ
           |> groupBy(\"station\")
           |> mean(\"pm25\")
           |> withDp(epsilon: 0.5, seed: 7);";
    let out = run_script(script, &[]);
    assert!(out.status.success(), "run failed:\n{}", stderr_of(&out));

    let markers = dp_markers(&out);
    assert_eq!(markers.len(), 2, "one marker per withDp step");
    assert!(approx(markers[0]["budget_spent"].as_f64().unwrap(), 2.0));
    assert_eq!(markers[0]["query_count"], 2);
    assert!(approx(markers[1]["budget_spent"].as_f64().unwrap(), 2.5));
    assert_eq!(markers[1]["query_count"], 3);

    // The same script is refused once the cumulative 2.5 exceeds the budget,
    // even though each step alone (2.0, 0.5) would fit.
    let rejected = run_script(script, &[("XAZZ_DP_BUDGET", "2.4")]);
    let err = stderr_of(&rejected);
    assert!(
        err.contains("[xazz RUNTIME ERROR]") && err.contains("XAZZ_DP_BUDGET"),
        "cumulative 2.5 > 2.4 must refuse the second step:\n{err}"
    );
    let markers = dp_markers(&rejected);
    assert_eq!(
        markers.len(),
        1,
        "first step is recorded, second is refused"
    );
    assert!(
        approx(markers[0]["budget_spent"].as_f64().unwrap(), 2.0),
        "the refused step must not change the ledger"
    );
}
