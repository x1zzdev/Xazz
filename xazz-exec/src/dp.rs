//! xazz-exec/src/dp.rs — 차등 프라이버시(DP) 노이즈 주입 엔진 (v0.6)
//!
//! `.xzz` 파이프라인의 `|> withDp(epsilon: 1.0, ...)` 연산을 실행한다.
//! 집계 결과 DataFrame의 숫자형 컬럼에 보정된 노이즈를 더해,
//! 결과 통계치의 유용성은 유지하되 개인 단위 기여 여부의 역추적을 수학적으로 차단한다.
//!
//! 지원 메커니즘:
//!   - Laplace  : ε-DP.       noise ~ Lap(0, Δf/ε)          (기본값)
//!   - Gaussian : (ε, δ)-DP.  noise ~ N(0, σ²), σ = Δf·√(2·ln(1.25/δ))/ε
//!
//! 세션 프라이버시 예산(ε-budget):
//!   실행 세션마다 누적 ε 소모량을 추적하고, 총 예산 초과 시 실행을 거부한다.
//!   (겹치는 집계쿼리 대량 반복 → 연립방정식 원본 복원 공격 방어. RULE_010 계열)
//!
//! 난수: 외부 crate 의존 없이 SplitMix64 + 역CDF/Box-Muller로 자체 구현.
//!   seed 지정 시 완전 결정적(감사·테스트 재현), 미지정 시 시스템 시간 기반.

use polars::prelude::{Column, DataFrame, DataType};
use xazz_compiler::ast::{DpArgs, DpMechanism};

/// gaussian 메커니즘의 기본 δ (미지정 시)
pub const DEFAULT_DELTA: f64 = 1e-5;

/// 세션 총 프라이버시 예산 기본값 (환경변수 XAZZ_DP_BUDGET 로 재정의 가능)
pub const DEFAULT_TOTAL_BUDGET: f64 = 10.0;

/// 세션 총 δ 예산 기본값 (환경변수 XAZZ_DP_DELTA_BUDGET 로 재정의 가능)
pub const DEFAULT_TOTAL_DELTA_BUDGET: f64 = 1e-4;

// ─────────────────────────────────────────────────────────────────────────────
// 세션 프라이버시 예산 (ε/δ 조성 회계)
// ─────────────────────────────────────────────────────────────────────────────

/// 단일 쿼리의 소비 기록 (조성 회계의 입력 단위).
#[derive(Debug, Clone, Copy)]
pub struct CompositionRecord {
    pub mechanism: DpMechanism,
    pub epsilon: f64,
    pub delta: f64,
}

/// 실행 세션 동안 누적 (ε, δ) 소모량을 추적하는 예산 관리자.
///
/// 조성 회계 규칙 (Dwork & Roth, 기본 순차 조성 — 정확):
///   - k 개의 (εᵢ, δᵢ)-DP 메커니즘은 (Σεᵢ, Σδᵢ)-DP 로 합성된다.
///   - Laplace 는 순수 ε-DP 이므로 δ 기여는 0 이다.
///   - Gaussian 은 (ε, δ)-DP 이므로 ε 과 δ 를 모두 누적한다.
///
/// `withDp` 호출마다 (ε, δ) 를 차감하고, 총 ε 또는 총 δ 를 넘는 순간 에러를
/// 반환하여 반복 질의를 통한 노이즈 평균화(재구성 공격)를 구조적으로 차단한다.
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

    /// ε/δ 총 예산을 모두 지정해 생성한다.
    pub fn new_with_delta(total_eps: f64, total_delta: f64) -> Self {
        PrivacyBudget {
            total_eps,
            total_delta,
            spent_eps: 0.0,
            spent_delta: 0.0,
            queries: Vec::new(),
        }
    }

    /// 환경변수에서 총 예산을 읽는다 (기본 ε=10.0, δ=1e-4).
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

    /// (ε, δ) 만큼 예산을 소모한다. 초과 시 Err (실행 거부).
    ///
    /// Laplace 는 순수 ε-DP 이므로 δ 는 0 으로 처리된다.
    /// 실패 시 예산은 소모되지 않는다 (원자적).
    pub fn spend(
        &mut self,
        mechanism: DpMechanism,
        epsilon: f64,
        delta: f64,
    ) -> Result<(), String> {
        if epsilon <= 0.0 {
            return Err(format!(
                "DP 에러: epsilon 은 0보다 커야 합니다. 실제: {epsilon}"
            ));
        }
        let delta = match mechanism {
            DpMechanism::Laplace => 0.0,
            DpMechanism::Gaussian => {
                if !(0.0 < delta && delta < 1.0) {
                    return Err(format!(
                        "DP 에러: gaussian 의 delta 는 (0, 1) 범위여야 합니다. 실제: {delta}"
                    ));
                }
                delta
            }
        };

        // 조성 회계: (Σεᵢ, Σδᵢ)
        let new_eps = self.spent_eps + epsilon;
        let new_delta = self.spent_delta + delta;

        const EPS_TOL: f64 = 1e-9;
        if new_eps > self.total_eps + EPS_TOL {
            return Err(format!(
                "DP 예산 초과(ε): 이번 요청 ε={epsilon:.4} 를 더하면 누적 {new_eps:.4} > 총 예산 {:.4}. \
                 반복 질의를 통한 노이즈 평균화(재구성 공격) 방지를 위해 실행을 거부합니다. \
                 (총 예산은 XAZZ_DP_BUDGET 환경변수로 조정 가능)",
                self.total_eps
            ));
        }
        if new_delta > self.total_delta + EPS_TOL {
            return Err(format!(
                "DP 예산 초과(δ): 이번 요청 δ={delta:.4e} 를 더하면 누적 {new_delta:.4e} > 총 δ 예산 {:.4e}. \
                 gaussian 메커니즘의 조성 회계를 위해 실행을 거부합니다. \
                 (총 δ 예산은 XAZZ_DP_DELTA_BUDGET 환경변수로 조정 가능)",
                self.total_delta
            ));
        }

        self.spent_eps = new_eps;
        self.spent_delta = new_delta;
        self.queries.push(CompositionRecord {
            mechanism,
            epsilon,
            delta,
        });
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

    /// 지금까지 합성된 쿼리 수 (조성 회계에 반영된 메커니즘 개수).
    pub fn query_count(&self) -> usize {
        self.queries.len()
    }

    pub fn remaining(&self) -> f64 {
        (self.total_eps - self.spent_eps).max(0.0)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 난수 생성기 (SplitMix64) — 외부 의존성 없는 결정적 RNG
// ─────────────────────────────────────────────────────────────────────────────

/// SplitMix64: 단순·고품질 64bit PRNG. seed 고정 시 완전 재현 가능.
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

    /// (0, 1) 개구간 균등분포 — 양 끝값 0/1 을 제외해 ln(0) 등 특이점 방지.
    fn next_f64(&mut self) -> f64 {
        loop {
            // 상위 53bit → [0, 1) 균등
            let u = (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64);
            if u > 0.0 && u < 1.0 {
                return u;
            }
        }
    }
}

/// seed 미지정 시 시스템 시간 기반 시드 생성.
fn time_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x5EED_5EED_5EED_5EED)
}

// ─────────────────────────────────────────────────────────────────────────────
// 노이즈 샘플링
// ─────────────────────────────────────────────────────────────────────────────

/// Laplace(0, scale) 샘플 — 역CDF 방식.
/// u ~ U(-1/2, 1/2),  x = -scale · sign(u) · ln(1 - 2|u|)
fn laplace_sample(rng: &mut SplitMix64, scale: f64) -> f64 {
    let u = rng.next_f64() - 0.5;
    -scale * u.signum() * (1.0 - 2.0 * u.abs()).ln()
}

/// N(0, sigma²) 샘플 — Box-Muller 변환.
fn gaussian_sample(rng: &mut SplitMix64, sigma: f64) -> f64 {
    let u1 = rng.next_f64();
    let u2 = rng.next_f64();
    sigma * (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
}

/// Gaussian 메커니즘 표준편차: σ = Δf · √(2·ln(1.25/δ)) / ε
/// (Dwork & Roth, The Algorithmic Foundations of Differential Privacy, Thm 3.22)
pub fn gaussian_sigma(sensitivity: f64, epsilon: f64, delta: f64) -> f64 {
    sensitivity * (2.0 * (1.25 / delta).ln()).sqrt() / epsilon
}

/// Laplace 메커니즘 scale: b = Δf / ε
pub fn laplace_scale(sensitivity: f64, epsilon: f64) -> f64 {
    sensitivity / epsilon
}

// ─────────────────────────────────────────────────────────────────────────────
// 공개 API — DataFrame 노이즈 주입
// ─────────────────────────────────────────────────────────────────────────────

/// 적용 결과 리포트 (감사로그·마커 출력용)
#[derive(Debug, Clone, serde::Serialize)]
pub struct DpReport {
    pub mechanism: String,
    pub epsilon: f64,
    pub delta: Option<f64>,
    pub sensitivity: f64,
    /// 실제 사용된 노이즈 파라미터 (laplace: scale b / gaussian: sigma σ)
    pub noise_param: f64,
    /// 노이즈가 적용된 컬럼 목록
    pub noised_columns: Vec<String>,
    pub seed: Option<i64>,
}

/// DataFrame의 모든 숫자형 컬럼에 DP 노이즈를 주입한 새 DataFrame을 반환한다.
///
/// - 숫자형(int/uint/float) 컬럼 → f64 로 승격 후 노이즈 합산
/// - 문자열 등 비숫자 컬럼 → 원본 유지 (그룹 키 보존)
/// - null → null 유지 (노이즈로 결측을 위장하지 않음)
///
/// 주의: 이 함수는 집계 *결과*에 적용하는 output perturbation 이다.
/// 호출 측(runtime)은 반드시 `PrivacyBudget::spend` 로 ε을 차감한 뒤 호출해야 한다.
pub fn apply_dp(df: &DataFrame, args: &DpArgs) -> Result<(DataFrame, DpReport), String> {
    if args.epsilon <= 0.0 {
        return Err(format!(
            "DP 에러: epsilon 은 0보다 커야 합니다. 실제: {}",
            args.epsilon
        ));
    }
    if args.sensitivity <= 0.0 {
        return Err(format!(
            "DP 에러: sensitivity 는 0보다 커야 합니다. 실제: {}",
            args.sensitivity
        ));
    }

    let delta = args.delta.unwrap_or(DEFAULT_DELTA);
    if matches!(args.mechanism, DpMechanism::Gaussian) && !(0.0 < delta && delta < 1.0) {
        return Err(format!(
            "DP 에러: gaussian 의 delta 는 (0, 1) 범위여야 합니다. 실제: {delta}"
        ));
    }

    let noise_param = match args.mechanism {
        DpMechanism::Laplace => laplace_scale(args.sensitivity, args.epsilon),
        DpMechanism::Gaussian => gaussian_sigma(args.sensitivity, args.epsilon, delta),
    };

    let mut rng = SplitMix64::new(args.seed.map(|s| s as u64).unwrap_or_else(time_seed));

    let mut out = df.clone();
    let mut noised_columns: Vec<String> = Vec::new();

    let col_names: Vec<String> = df
        .get_column_names()
        .iter()
        .map(|s| s.to_string())
        .collect();

    for name in &col_names {
        let column = df
            .column(name.as_str())
            .map_err(|e| format!("DP 에러: 컬럼 '{name}' 접근 실패 — {e}"))?;

        if !is_numeric_dtype(column.dtype()) {
            continue; // 그룹 키(문자열 등)는 원본 유지
        }

        // f64 로 승격 (int 컬럼도 노이즈 합산 후엔 실수가 됨)
        let casted = column
            .cast(&DataType::Float64)
            .map_err(|e| format!("DP 에러: 컬럼 '{name}' float 캐스팅 실패 — {e}"))?;
        let ca = casted
            .f64()
            .map_err(|e| format!("DP 에러: 컬럼 '{name}' f64 접근 실패 — {e}"))?;

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
            .map_err(|e| format!("DP 에러: 컬럼 '{name}' 치환 실패 — {e}"))?;
        noised_columns.push(name.clone());
    }

    if noised_columns.is_empty() {
        return Err("DP 에러: 노이즈를 적용할 숫자형 컬럼이 없습니다. \
             withDp 는 집계 결과(count/sum/mean 등) 뒤에 사용하세요."
            .to_string());
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
// 테스트
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
        // 대수의 법칙: Lap(0, b) 샘플 평균은 0 근방으로 수렴
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

        // 문자열 컬럼은 그대로
        assert_eq!(
            noised.column("region").unwrap().str().unwrap().get(0),
            Some("SEOUL")
        );
        // 숫자 컬럼은 f64 로 승격 + 노이즈 적용 (원본과 달라짐)
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
        // ε 이 작을수록 노이즈 scale 이 커야 한다 (강한 보호)
        assert!(laplace_scale(1.0, 0.1) > laplace_scale(1.0, 1.0));
        assert!(gaussian_sigma(1.0, 0.1, 1e-5) > gaussian_sigma(1.0, 1.0, 1e-5));
    }

    #[test]
    fn budget_blocks_over_spend() {
        let mut budget = PrivacyBudget::new(2.0);
        assert!(budget.spend(DpMechanism::Laplace, 1.0, 0.0).is_ok());
        assert!(budget.spend(DpMechanism::Laplace, 0.5, 0.0).is_ok());
        assert!((budget.remaining() - 0.5).abs() < 1e-12);
        // 잔여 0.5 인데 1.0 요청 → 거부
        let err = budget.spend(DpMechanism::Laplace, 1.0, 0.0).unwrap_err();
        assert!(err.contains("예산 초과"), "예산 초과 메시지 아님: {err}");
        // 거부된 요청은 예산을 소모하지 않는다
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
        // gaussian 은 (ε, δ)-DP → δ 도 조성 회계에 누적된다
        let mut budget = PrivacyBudget::new_with_delta(10.0, 1e-4);
        assert!(budget.spend(DpMechanism::Gaussian, 1.0, 1e-5).is_ok());
        assert!(budget.spend(DpMechanism::Gaussian, 1.0, 1e-5).is_ok());
        assert!((budget.spent_delta() - 2e-5).abs() < 1e-12);
        assert_eq!(budget.query_count(), 2);
    }

    #[test]
    fn gaussian_delta_over_budget_is_blocked() {
        let mut budget = PrivacyBudget::new_with_delta(10.0, 1e-5);
        // 첫 쿼리 δ=1e-5 로 총 δ 예산 소진 → 두 번째 gaussian 은 δ 초과로 거부
        assert!(budget.spend(DpMechanism::Gaussian, 1.0, 1e-5).is_ok());
        let err = budget.spend(DpMechanism::Gaussian, 1.0, 1e-5).unwrap_err();
        assert!(err.contains("δ"), "δ 초과 메시지 아님: {err}");
        // 거부된 요청은 ε/δ 를 소모하지 않는다
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
}
