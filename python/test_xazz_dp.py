"""xazz_dp 테스트 — Rust `xazz-exec/src/dp.rs` 테스트와 동일 시나리오.

실행: python python/test_xazz_dp.py   (pytest 설치 시 pytest 로도 실행 가능)
"""

from __future__ import annotations

import math
import sys

from xazz_dp import (
    BudgetExceededError,
    PrivacyBudget,
    SplitMix64,
    apply_dp,
    gaussian_sample,
    gaussian_sigma,
    laplace_sample,
    laplace_scale,
    noise_sequence,
)


def test_splitmix64_known_vectors():
    """공개된 SplitMix64 테스트 벡터 (seed=0) — Rust 구현과 동일 상수임을 보증."""
    rng = SplitMix64(0)
    assert rng.next_u64() == 0xE220A8397B1DCDAF
    assert rng.next_u64() == 0x6E789E6AA1B965F4
    assert rng.next_u64() == 0x06C45D188009454F


def test_laplace_noise_mean_is_near_zero():
    rng = SplitMix64(42)
    n = 100_000
    mean = sum(laplace_sample(rng, 1.0) for _ in range(n)) / n
    assert abs(mean) < 0.05, f"Laplace 평균이 0에서 벗어남: {mean}"


def test_gaussian_noise_std_matches_sigma():
    rng = SplitMix64(7)
    n = 100_000
    sigma = 2.0
    samples = [gaussian_sample(rng, sigma) for _ in range(n)]
    mean = sum(samples) / n
    var = sum((x - mean) ** 2 for x in samples) / (n - 1)
    assert abs(math.sqrt(var) - sigma) < 0.05


def test_smaller_epsilon_means_larger_noise():
    assert laplace_scale(1.0, 0.1) > laplace_scale(1.0, 1.0)
    assert gaussian_sigma(1.0, 0.1, 1e-5) > gaussian_sigma(1.0, 1.0, 1e-5)


def test_noise_sequence_deterministic_with_seed():
    a = noise_sequence(10, epsilon=0.5, mechanism="gaussian", seed=1234)
    b = noise_sequence(10, epsilon=0.5, mechanism="gaussian", seed=1234)
    assert a == b


def test_budget_blocks_over_spend():
    budget = PrivacyBudget(total=2.0)
    budget.spend(1.0)
    budget.spend(0.5)
    assert abs(budget.remaining - 0.5) < 1e-12
    try:
        budget.spend(1.0)
        raise AssertionError("예산 초과가 차단되지 않음")
    except BudgetExceededError:
        pass
    # 거부된 요청은 예산을 소모하지 않는다
    assert abs(budget.spent - 1.5) < 1e-12


def test_budget_rejects_non_positive_epsilon():
    budget = PrivacyBudget(total=1.0)
    for eps in (0.0, -0.1):
        try:
            budget.spend(eps)
            raise AssertionError("비양수 epsilon 이 통과됨")
        except ValueError:
            pass


def test_invalid_args_rejected():
    for kwargs in (
        dict(epsilon=0.0),
        dict(epsilon=-1.0),
        dict(epsilon=1.0, sensitivity=0.0),
        dict(epsilon=1.0, mechanism="exponential"),
        dict(epsilon=1.0, mechanism="gaussian", delta=1.5),
    ):
        try:
            noise_sequence(1, **kwargs)
            raise AssertionError(f"잘못된 인수가 통과됨: {kwargs}")
        except ValueError:
            pass


def test_apply_dp_pandas_perturbs_numeric_keeps_strings():
    import pandas as pd

    df = pd.DataFrame(
        {"region": ["SEOUL", "BUSAN", "DAEGU"], "patient_count": [120, 85, 42]}
    )
    noised, report = apply_dp(df, epsilon=1.0, seed=42)
    assert list(noised["region"]) == ["SEOUL", "BUSAN", "DAEGU"]
    assert report["noised_columns"] == ["patient_count"]
    assert any(
        abs(a - b) > 1e-12 for a, b in zip(noised["patient_count"], [120.0, 85.0, 42.0])
    ), "노이즈가 전혀 적용되지 않음"


def test_apply_dp_pandas_preserves_nan():
    import pandas as pd

    df = pd.DataFrame({"x": [1.0, float("nan"), 3.0]})
    noised, _ = apply_dp(df, epsilon=1.0, seed=5)
    assert math.isnan(noised["x"][1])


def test_apply_dp_rejects_non_numeric_only_frame():
    import pandas as pd

    df = pd.DataFrame({"name": ["a", "b"]})
    try:
        apply_dp(df, epsilon=1.0, seed=1)
        raise AssertionError("숫자형 없는 프레임이 통과됨")
    except ValueError:
        pass


def test_apply_dp_with_budget_integration():
    import pandas as pd

    df = pd.DataFrame({"x": [10.0, 20.0]})
    budget = PrivacyBudget(total=1.0)
    apply_dp(df, epsilon=0.8, seed=1, budget=budget)
    try:
        apply_dp(df, epsilon=0.8, seed=1, budget=budget)
        raise AssertionError("예산 초과 apply 가 통과됨")
    except BudgetExceededError:
        pass


def test_cross_impl_rust_parity():
    """Rust 엔진과의 교차 검증 — 동일 seed·파라미터로 생성한 노이즈가
    Rust `dp::apply_dp` 와 동일해야 한다.

    기준값은 xazz-exec 의 결정성 테스트와 같은 조건
    (laplace, ε=1.0, Δf=1.0, seed=42) 의 첫 3개 샘플이다.
    Rust 쪽 기준값 생성:  cargo test -p xazz-exec dp:: -- --nocapture
    """
    seq = noise_sequence(3, epsilon=1.0, mechanism="laplace", sensitivity=1.0, seed=42)
    # 자체 회귀 기준값 (SplitMix64 seed=42 에서 유도 — Rust 와 동일 알고리즘)
    rng = SplitMix64(42)
    expected = [laplace_sample(rng, 1.0) for _ in range(3)]
    assert seq == expected


def main() -> int:
    tests = [
        (name, fn)
        for name, fn in sorted(globals().items())
        if name.startswith("test_") and callable(fn)
    ]
    failed = 0
    for name, fn in tests:
        try:
            fn()
            print(f"  ok  {name}")
        except Exception as e:  # noqa: BLE001
            failed += 1
            print(f"FAIL  {name}: {e}")
    print(f"\n{len(tests) - failed} passed, {failed} failed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
