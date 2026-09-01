//! xazz-exec/src/dp.rs — differential privacy (DP) noise injection engine (v0.6)
//!
//! Executes the `|> withDp(epsilon: 1.0, ...)` operator of a `.xzz` pipeline.
//! Adds calibrated noise to the numeric columns of aggregate-result DataFrames,
//! preserving the usefulness of the reported statistics while mathematically
//! preventing the re-identification of individual contributions.
//!
//! Supported mechanisms:
//!   - Laplace  : ε-DP.       noise ~ Lap(0, Δf/ε)          (default)
//!   - Gaussian : (ε, δ)-DP.  noise ~ N(0, σ²), σ = Δf·√(2·ln(1.25/δ))/ε
//!
//! Session privacy budget (ε-budget):
//!   Tracks the cumulative ε spent per execution session and refuses runs that
//!   would exceed the total budget. (Defends against reconstruction attacks
//!   that average noise across repeated overlapping aggregate queries.)
//!
//! Randomness: self-contained SplitMix64 + inverse-CDF/Box-Muller, no external
//! crate dependency. A seed makes it fully deterministic (audit/test replay);
//! without one it is seeded from OS entropy (/dev/urandom).

use polars::prelude::{Column, DataFrame, DataType};
use xazz_compiler::ast::{DpArgs, DpMechanism};
use xazz_core::i18n::tr;

/// Default δ for the gaussian mechanism (when unspecified)
pub const DEFAULT_DELTA: f64 = 1e-5;

/// Default session total privacy budget (overridable via the XAZZ_DP_BUDGET env var)
pub const DEFAULT_TOTAL_BUDGET: f64 = 10.0;

/// Default session total δ budget (overridable via the XAZZ_DP_DELTA_BUDGET env var)
pub const DEFAULT_TOTAL_DELTA_BUDGET: f64 = 1e-4;

// ─────────────────────────────────────────────────────────────────────────────
// Session privacy budget (ε/δ composition accounting)
// ─────────────────────────────────────────────────────────────────────────────

/// Consumption record of a single query (input unit of composition accounting).
#[derive(Debug, Clone, Copy)]
pub struct CompositionRecord {
    pub mechanism: DpMechanism,
    pub epsilon: f64,
    pub delta: f64,
}

/// Budget manager tracking cumulative (ε, δ) spending during an execution session.
///
/// Composition accounting rules (Dwork & Roth, basic sequential composition — exact):
///   - k (εᵢ, δᵢ)-DP mechanisms compose to (Σεᵢ, Σδᵢ)-DP.
///   - Laplace is pure ε-DP, so its δ contribution is 0.
///   - Gaussian is (ε, δ)-DP, so both ε and δ accumulate.
///
/// Deducts (ε, δ) per `withDp` call and returns an error the moment total ε or
/// total δ is exceeded, structurally blocking noise averaging via repeated queries (reconstruction attacks).
#[derive(Debug, Clone)]
pub struct PrivacyBudget {
    total_eps: f64,
    total_delta: f64,
    spent_eps: f64,
    spent_delta: f64,
    queries: Vec<CompositionRecord>,
}

impl PrivacyBudget {
    pub fn new(total_eps: f64) -> Self {
        PrivacyBudget {
            total_eps,
            total_delta: DEFAULT_TOTAL_DELTA_BUDGET,
            spent_eps: 0.0,
            spent_delta: 0.0,
            queries: Vec::new(),
        }
    }

    /// Creates a budget with both ε/δ total budgets specified.
    pub fn new_with_delta(total_eps: f64, total_delta: f64) -> Self {
        PrivacyBudget {
            total_eps,
            total_delta,
            spent_eps: 0.0,
            spent_delta: 0.0,
            queries: Vec::new(),
        }
    }

    /// Reads the total budget from environment variables (defaults ε=10.0, δ=1e-4).
    pub fn from_env() -> Self {
        let total_eps = std::env::var("XAZZ_DP_BUDGET")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|v| *v > 0.0)
            .unwrap_or(DEFAULT_TOTAL_BUDGET);
        let total_delta = std::env::var("XAZZ_DP_DELTA_BUDGET")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|v| (0.0..1.0).contains(v))
            .unwrap_or(DEFAULT_TOTAL_DELTA_BUDGET);
        PrivacyBudget::new_with_delta(total_eps, total_delta)
    }

    /// Spends (ε, δ) of budget. Returns Err when exceeded (execution refused).
    ///
    /// Laplace is pure ε-DP, so δ is treated as 0.
    /// On failure the budget is not spent (atomic).
    pub fn spend(
        &mut self,
        mechanism: DpMechanism,
        epsilon: f64,
        delta: f64,
    ) -> Result<(), String> {
        self.spend_n(mechanism, epsilon, delta, 1)
    }

    /// Spends budget by composing `count` identical (ε, δ) mechanisms.
    ///
    /// k independent mechanisms (e.g., k columns receiving noise) compose to (k·ε, k·δ)
    /// under sequential composition. Charging only once via a single `spend` would miss
    /// the accounting, so callers charge the number of applied mechanisms atomically in one call.
    pub fn spend_n(
        &mut self,
        mechanism: DpMechanism,
        epsilon: f64,
        delta: f64,
        count: usize,
    ) -> Result<(), String> {
        if count == 0 {
            return Ok(());
        }
        if epsilon <= 0.0 {
            return Err(format!(
                "{}: {} {epsilon}",
                tr("DP error", "DP 에러"),
                tr(
                    "epsilon must be greater than 0",
                    "epsilon 은 0보다 커야 합니다. 실제:"
                )
            ));
        }
        let delta = match mechanism {
            DpMechanism::Laplace => 0.0,
            DpMechanism::Gaussian => {
                if !(0.0 < delta && delta < 1.0) {
                    return Err(format!(
                        "{}: {} {delta}",
                        tr("DP error", "DP 에러"),
                        tr(
                            "gaussian delta must be in (0, 1)",
                            "gaussian 의 delta 는 (0, 1) 범위여야 합니다. 실제:"
                        )
                    ));
                }
                delta
            }
        };

        let count_f = count as f64;
        // Composition accounting: k mechanisms → (k·Σεᵢ, k·Σδᵢ)
        let new_eps = self.spent_eps + epsilon * count_f;
        let new_delta = self.spent_delta + delta * count_f;

        const EPS_TOL: f64 = 1e-9;
        if new_eps > self.total_eps + EPS_TOL {
            return Err(format!(
                "DP 예산 초과(ε): 이번 요청 ε={epsilon:.4} × {count} 컬럼 을 더하면 누적 {new_eps:.4} > 총 예산 {:.4}. \
                 반복 질의를 통한 노이즈 평균화(재구성 공격) 방지를 위해 실행을 거부합니다. \
                 (총 예산은 XAZZ_DP_BUDGET 환경변수로 조정 가능)",
                self.total_eps
            ));
        }
        if new_delta > self.total_delta + EPS_TOL {
            return Err(format!(
                "DP 예산 초과(δ): 이번 요청 δ={delta:.4e} × {count} 컬럼 을 더하면 누적 {new_delta:.4e} > 총 δ 예산 {:.4e}. \
                 gaussian 메커니즘의 조성 회계를 위해 실행을 거부합니다. \
                 (총 δ 예산은 XAZZ_DP_DELTA_BUDGET 환경변수로 조정 가능)",
                self.total_delta
            ));
        }

        self.spent_eps = new_eps;
        self.spent_delta = new_delta;
        for _ in 0..count {
            self.queries.push(CompositionRecord {
                mechanism,
                epsilon,
                delta,
            });
        }
        Ok(())
    }

    pub fn spent(&self) -> f64 {
        self.spent_eps
    }

    pub fn total(&self) -> f64 {
        self.total_eps
    }

    pub fn spent_delta(&self) -> f64 {
        self.spent_delta
    }

    pub fn total_delta(&self) -> f64 {
        self.total_delta
    }

    /// Number of queries composed so far (count of mechanisms reflected in composition accounting).
    pub fn query_count(&self) -> usize {
        self.queries.len()
    }

    pub fn remaining(&self) -> f64 {
        (self.total_eps - self.spent_eps).max(0.0)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Random number generator (SplitMix64) — deterministic RNG with no external dependency
// ─────────────────────────────────────────────────────────────────────────────

/// SplitMix64: simple, high-quality 64-bit PRNG. Fully reproducible with a fixed seed.
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        SplitMix64 { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    /// Uniform on the open interval (0, 1) — excludes endpoints 0/1 to avoid singularities like ln(0).
    fn next_f64(&mut self) -> f64 {
        loop {
            // Top 53 bits → uniform [0, 1)
            let u = (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64);
            if u > 0.0 && u < 1.0 {
                return u;
            }
        }
    }
}

/// Generates a seed from OS entropy (/dev/urandom) when no seed is specified.
///
/// A seed from system time alone lets an attacker narrow seed candidates using
/// wall-clock time, enabling noise inversion — so 8 bytes are read directly from
/// a CSPRNG (/dev/urandom). On read failure, a fallback mixing time+PID+address is used (no unpredictability guarantee).
fn os_seed() -> u64 {
    #[cfg(unix)]
    {
        use std::io::Read;
        let mut f = match std::fs::File::open("/dev/urandom") {
            Ok(f) => f,
            Err(_) => return fallback_seed(),
        };
        let mut buf = [0u8; 8];
        if f.read_exact(&mut buf).is_ok() {
            return u64::from_ne_bytes(buf);
        }
        fallback_seed()
    }
    #[cfg(not(unix))]
    {
        fallback_seed()
    }
}

fn fallback_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let pid = std::process::id() as u64;
    let addr = &t as *const _ as u64;
    t ^ pid.rotate_left(17) ^ addr.rotate_left(31) ^ 0x5EED_5EED_5EED_5EED
}

// ─────────────────────────────────────────────────────────────────────────────
// Noise sampling
// ─────────────────────────────────────────────────────────────────────────────

/// Laplace(0, scale) sample — inverse-CDF method.
/// u ~ U(-1/2, 1/2),  x = -scale · sign(u) · ln(1 - 2|u|)
fn laplace_sample(rng: &mut SplitMix64, scale: f64) -> f64 {
    let u = rng.next_f64() - 0.5;
    -scale * u.signum() * (1.0 - 2.0 * u.abs()).ln()
}

/// N(0, sigma²) sample — Box-Muller transform.
fn gaussian_sample(rng: &mut SplitMix64, sigma: f64) -> f64 {
    let u1 = rng.next_f64();
    let u2 = rng.next_f64();
    sigma * (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
}

/// Gaussian mechanism standard deviation: σ = Δf · √(2·ln(1.25/δ)) / ε
/// (Dwork & Roth, The Algorithmic Foundations of Differential Privacy, Thm 3.22)
pub fn gaussian_sigma(sensitivity: f64, epsilon: f64, delta: f64) -> f64 {
    sensitivity * (2.0 * (1.25 / delta).ln()).sqrt() / epsilon
}

/// Laplace mechanism scale: b = Δf / ε
pub fn laplace_scale(sensitivity: f64, epsilon: f64) -> f64 {
    sensitivity / epsilon
}

// ─────────────────────────────────────────────────────────────────────────────
// Public API — DataFrame noise injection
// ─────────────────────────────────────────────────────────────────────────────

/// Report of the applied result (for audit-log/marker output)
#[derive(Debug, Clone, serde::Serialize)]
pub struct DpReport {
    pub mechanism: String,
    pub epsilon: f64,
    pub delta: Option<f64>,
    pub sensitivity: f64,
    /// Actually used noise parameter (laplace: scale b / gaussian: sigma σ)
    pub noise_param: f64,
    /// List of columns to which noise was applied
    pub noised_columns: Vec<String>,
    pub seed: Option<i64>,
}

/// Returns a new DataFrame with DP noise injected into all numeric columns.
///
/// - Numeric (int/uint/float) columns → promoted to f64, then noise is added
/// - Non-numeric columns (strings, etc.) → kept as-is (preserves group keys)
/// - null → stays null (missing values are not masked with noise)
///
/// Note: this function is output perturbation applied to aggregate *results*.
/// The caller (runtime) must deduct ε via `PrivacyBudget::spend` before calling.
pub fn apply_dp(df: &DataFrame, args: &DpArgs) -> Result<(DataFrame, DpReport), String> {
    if args.epsilon <= 0.0 {
        return Err(format!(
            "{}: {} {}",
            tr("DP error", "DP 에러"),
            tr(
                "epsilon must be greater than 0",
                "epsilon 은 0보다 커야 합니다. 실제:"
            ),
            args.epsilon
        ));
    }
    if args.sensitivity <= 0.0 {
        return Err(format!(
            "{}: {} {}",
            tr("DP error", "DP 에러"),
            tr(
                "sensitivity must be greater than 0",
                "sensitivity 는 0보다 커야 합니다. 실제:"
            ),
            args.sensitivity
        ));
    }

    let delta = args.delta.unwrap_or(DEFAULT_DELTA);
    if matches!(args.mechanism, DpMechanism::Gaussian) && !(0.0 < delta && delta < 1.0) {
        return Err(format!(
            "{}: {} {delta}",
            tr("DP error", "DP 에러"),
            tr(
                "gaussian delta must be in (0, 1)",
                "gaussian 의 delta 는 (0, 1) 범위여야 합니다. 실제:"
            )
        ));
    }

    let noise_param = match args.mechanism {
        DpMechanism::Laplace => laplace_scale(args.sensitivity, args.epsilon),
        DpMechanism::Gaussian => gaussian_sigma(args.sensitivity, args.epsilon, delta),
    };

    let mut rng = SplitMix64::new(args.seed.map(|s| s as u64).unwrap_or_else(os_seed));

    let mut out = df.clone();
    let mut noised_columns: Vec<String> = Vec::new();

    let col_names: Vec<String> = df
        .get_column_names()
        .iter()
        .map(|s| s.to_string())
        .collect();

    for name in &col_names {
        let column = df.column(name.as_str()).map_err(|e| {
            format!(
                "{}: {} '{name}' — {e}",
                tr("DP error", "DP 에러"),
                tr("column access failed", "컬럼 접근 실패"),
            )
        })?;

        if !is_numeric_dtype(column.dtype()) {
            continue; // group keys (strings, etc.) are kept as-is
        }

        // Promote to f64 (int columns become floats after noise is added)
        let casted = column.cast(&DataType::Float64).map_err(|e| {
            format!(
                "{}: {} '{name}' — {e}",
                tr("DP error", "DP 에러"),
                tr("float cast failed", "컬럼 float 캐스팅 실패")
            )
        })?;
        let ca = casted.f64().map_err(|e| {
            format!(
                "{}: {} '{name}' — {e}",
                tr("DP error", "DP 에러"),
                tr("f64 access failed", "컬럼 f64 접근 실패")
            )
        })?;

        let noised: Vec<Option<f64>> = ca
            .iter()
            .map(|opt| {
                opt.map(|v| {
                    let noise = match args.mechanism {
                        DpMechanism::Laplace => laplace_sample(&mut rng, noise_param),
                        DpMechanism::Gaussian => gaussian_sample(&mut rng, noise_param),
                    };
                    v + noise
                })
            })
            .collect();

        out.with_column(Column::new(name.as_str().into(), noised))
            .map_err(|e| {
                format!(
                    "{}: {} '{name}' — {e}",
                    tr("DP error", "DP 에러"),
                    tr("replacement failed", "컬럼 치환 실패")
                )
            })?;
        noised_columns.push(name.clone());
    }

    if noised_columns.is_empty() {
        return Err(format!(
            "{}: {}",
            tr("DP error", "DP 에러"),
            tr(
                "no numeric columns to apply noise to. Use withDp after an aggregate (count/sum/mean etc).",
                "노이즈를 적용할 숫자형 컬럼이 없습니다. withDp 는 집계 결과(count/sum/mean 등) 뒤에 사용하세요."
            )
        ));
    }

    let report = DpReport {
        mechanism: args.mechanism.as_str().to_string(),
        epsilon: args.epsilon,
        delta: matches!(args.mechanism, DpMechanism::Gaussian).then_some(delta),
        sensitivity: args.sensitivity,
        noise_param,
        noised_columns,
        seed: args.seed,
    };

    Ok((out, report))
}

fn is_numeric_dtype(dt: &DataType) -> bool {
    matches!(
        dt,
        DataType::Float64
            | DataType::Float32
            | DataType::Int64
            | DataType::Int32
            | DataType::Int16
            | DataType::Int8
            | DataType::UInt64
            | DataType::UInt32
            | DataType::UInt16
            | DataType::UInt8
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use polars::prelude::df;

    fn test_args(mechanism: DpMechanism, epsilon: f64, seed: i64) -> DpArgs {
        DpArgs {
            epsilon,
            mechanism,
            sensitivity: 1.0,
            delta: Some(1e-5),
            seed: Some(seed),
        }
    }

    #[test]
    fn laplace_noise_mean_is_near_zero() {
        // Law of large numbers: the mean of Lap(0, b) samples converges near 0
        let mut rng = SplitMix64::new(42);
        let n = 100_000;
        let scale = 1.0;
        let mean: f64 = (0..n).map(|_| laplace_sample(&mut rng, scale)).sum::<f64>() / n as f64;
        assert!(mean.abs() < 0.05, "Laplace 평균이 0에서 벗어남: {mean}");
    }

    #[test]
    fn gaussian_noise_std_matches_sigma() {
        let mut rng = SplitMix64::new(7);
        let n = 100_000;
        let sigma = 2.0;
        let samples: Vec<f64> = (0..n).map(|_| gaussian_sample(&mut rng, sigma)).collect();
        let mean: f64 = samples.iter().sum::<f64>() / n as f64;
        let var: f64 = samples.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
        assert!(
            (var.sqrt() - sigma).abs() < 0.05,
            "Gaussian 표준편차가 σ={sigma}와 불일치: {}",
            var.sqrt()
        );
    }

    #[test]
    fn apply_dp_perturbs_numeric_keeps_strings() {
        let frame = df!(
            "region" => ["SEOUL", "BUSAN", "DAEGU"],
            "patient_count" => [120i64, 85, 42],
        )
        .unwrap();

        let (noised, report) = apply_dp(&frame, &test_args(DpMechanism::Laplace, 1.0, 42)).unwrap();

        // String columns stay unchanged
        assert_eq!(
            noised.column("region").unwrap().str().unwrap().get(0),
            Some("SEOUL")
        );
        // Numeric column is promoted to f64 + noise applied (differs from the original)
        let vals = noised.column("patient_count").unwrap().f64().unwrap();
        let originals = [120.0, 85.0, 42.0];
        let changed = (0..3).any(|i| (vals.get(i).unwrap() - originals[i]).abs() > 1e-12);
        assert!(changed, "노이즈가 전혀 적용되지 않음");
        assert_eq!(report.noised_columns, vec!["patient_count"]);
    }

    #[test]
    fn apply_dp_is_deterministic_with_seed() {
        let frame = df!("x" => [10.0f64, 20.0, 30.0]).unwrap();
        let args = test_args(DpMechanism::Gaussian, 0.5, 1234);
        let (a, _) = apply_dp(&frame, &args).unwrap();
        let (b, _) = apply_dp(&frame, &args).unwrap();
        let (ca, cb) = (
            a.column("x").unwrap().f64().unwrap(),
            b.column("x").unwrap().f64().unwrap(),
        );
        for i in 0..3 {
            assert_eq!(ca.get(i), cb.get(i), "동일 seed 결과가 재현되지 않음");
        }
    }

    #[test]
    fn apply_dp_preserves_null() {
        let frame = df!("x" => [Some(1.0f64), None, Some(3.0)]).unwrap();
        let (noised, _) = apply_dp(&frame, &test_args(DpMechanism::Laplace, 1.0, 5)).unwrap();
        assert_eq!(noised.column("x").unwrap().f64().unwrap().get(1), None);
    }

    #[test]
    fn apply_dp_rejects_non_numeric_only_frame() {
        let frame = df!("name" => ["a", "b"]).unwrap();
        assert!(apply_dp(&frame, &test_args(DpMechanism::Laplace, 1.0, 1)).is_err());
    }

    #[test]
    fn apply_dp_rejects_invalid_epsilon() {
        let frame = df!("x" => [1.0f64]).unwrap();
        let mut args = test_args(DpMechanism::Laplace, 0.0, 1);
        assert!(apply_dp(&frame, &args).is_err());
        args.epsilon = -1.0;
        assert!(apply_dp(&frame, &args).is_err());
    }

    #[test]
    fn smaller_epsilon_means_larger_noise() {
        // The smaller ε, the larger the noise scale must be (stronger protection)
        assert!(laplace_scale(1.0, 0.1) > laplace_scale(1.0, 1.0));
        assert!(gaussian_sigma(1.0, 0.1, 1e-5) > gaussian_sigma(1.0, 1.0, 1e-5));
    }

    #[test]
    fn budget_blocks_over_spend() {
        let mut budget = PrivacyBudget::new(2.0);
        assert!(budget.spend(DpMechanism::Laplace, 1.0, 0.0).is_ok());
        assert!(budget.spend(DpMechanism::Laplace, 0.5, 0.0).is_ok());
        assert!((budget.remaining() - 0.5).abs() < 1e-12);
        // 1.0 requested while 0.5 remains → rejected
        let err = budget.spend(DpMechanism::Laplace, 1.0, 0.0).unwrap_err();
        assert!(err.contains("예산 초과"), "예산 초과 메시지 아님: {err}");
        // A rejected request does not consume budget
        assert!((budget.spent() - 1.5).abs() < 1e-12);
    }

    #[test]
    fn budget_rejects_non_positive_epsilon() {
        let mut budget = PrivacyBudget::new(1.0);
        assert!(budget.spend(DpMechanism::Laplace, 0.0, 0.0).is_err());
        assert!(budget.spend(DpMechanism::Laplace, -0.1, 0.0).is_err());
    }

    #[test]
    fn gaussian_composition_accumulates_delta() {
        // gaussian is (ε, δ)-DP → δ also accumulates in composition accounting
        let mut budget = PrivacyBudget::new_with_delta(10.0, 1e-4);
        assert!(budget.spend(DpMechanism::Gaussian, 1.0, 1e-5).is_ok());
        assert!(budget.spend(DpMechanism::Gaussian, 1.0, 1e-5).is_ok());
        assert!((budget.spent_delta() - 2e-5).abs() < 1e-12);
        assert_eq!(budget.query_count(), 2);
    }

    #[test]
    fn gaussian_delta_over_budget_is_blocked() {
        let mut budget = PrivacyBudget::new_with_delta(10.0, 1e-5);
        // First query's δ=1e-5 exhausts the total δ budget → second gaussian is rejected for exceeding δ
        assert!(budget.spend(DpMechanism::Gaussian, 1.0, 1e-5).is_ok());
        let err = budget.spend(DpMechanism::Gaussian, 1.0, 1e-5).unwrap_err();
        assert!(err.contains("δ"), "δ 초과 메시지 아님: {err}");
        // A rejected request does not consume ε/δ
        assert!((budget.spent() - 1.0).abs() < 1e-12);
        assert!((budget.spent_delta() - 1e-5).abs() < 1e-12);
    }

    #[test]
    fn laplace_does_not_accumulate_delta() {
        let mut budget = PrivacyBudget::new_with_delta(10.0, 1e-4);
        assert!(budget.spend(DpMechanism::Laplace, 1.0, 0.0).is_ok());
        assert!(budget.spend(DpMechanism::Laplace, 2.0, 0.0).is_ok());
        assert_eq!(budget.spent_delta(), 0.0, "laplace 는 순수 ε-DP (δ=0)");
    }

    #[test]
    fn spend_n_charges_per_mechanism_for_multi_column() {
        // k columns = k mechanisms → billed k·ε (sequential composition)
        let mut budget = PrivacyBudget::new(10.0);
        assert!(budget.spend_n(DpMechanism::Laplace, 1.0, 0.0, 3).is_ok());
        assert!((budget.spent() - 3.0).abs() < 1e-12);
        assert_eq!(budget.query_count(), 3);
    }

    #[test]
    fn spend_n_blocks_when_multi_column_exceeds_budget() {
        // Noise on 3 columns while 2ε remains → 3ε charge is rejected (k mechanisms reflected)
        let mut budget = PrivacyBudget::new(2.0);
        let err = budget
            .spend_n(DpMechanism::Laplace, 1.0, 0.0, 3)
            .unwrap_err();
        assert!(err.contains("예산 초과"), "예산 초과 메시지 아님: {err}");
        // No budget is spent on rejection
        assert!((budget.spent() - 0.0).abs() < 1e-12);
        assert_eq!(budget.query_count(), 0);
    }

    #[test]
    fn spend_n_zero_count_is_noop() {
        let mut budget = PrivacyBudget::new(1.0);
        assert!(budget.spend_n(DpMechanism::Laplace, 1.0, 0.0, 0).is_ok());
        assert_eq!(budget.spent(), 0.0);
    }

    #[test]
    fn spend_n_gaussian_accumulates_delta_per_column() {
        let mut budget = PrivacyBudget::new_with_delta(10.0, 1e-4);
        assert!(budget.spend_n(DpMechanism::Gaussian, 1.0, 1e-5, 2).is_ok());
        assert!((budget.spent() - 2.0).abs() < 1e-12);
        assert!((budget.spent_delta() - 2e-5).abs() < 1e-12);
        assert_eq!(budget.query_count(), 2);
    }
}
