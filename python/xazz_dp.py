"""xazz_dp — Xazz 차등 프라이버시(DP) 노이즈 주입 모듈 (Python 구현, v0.6)

Rust 구현(`xazz-exec/src/dp.rs`)과 동일한 알고리즘·상수·RNG(SplitMix64)를 사용하는
Python 레퍼런스 구현. 같은 seed 를 주면 Rust 엔진과 동일한 노이즈 시퀀스를 재현하므로
교차 검증(cross-language reproducibility)이 가능하다.

용도:
  - sLM 파인튜닝 데이터셋 등 Python 파이프라인에서의 비식별화 처리
  - Rust 엔진 출력의 독립 검증 (감사 목적)
  - 노트북/실험 환경에서의 DP 파라미터 탐색

지원 메커니즘:
  - Laplace  : ε-DP.       noise ~ Lap(0, Δf/ε)          (기본값)
  - Gaussian : (ε, δ)-DP.  noise ~ N(0, σ²), σ = Δf·√(2·ln(1.25/δ))/ε

코어는 표준 라이브러리만 사용한다. `apply_dp` 는 pandas / polars DataFrame 을
duck-typing 으로 지원한다 (해당 라이브러리가 설치된 경우에만).
"""

from __future__ import annotations

import math
import os
import time
from dataclasses import dataclass, field

__all__ = [
    "DEFAULT_DELTA",
    "DEFAULT_TOTAL_BUDGET",
    "SplitMix64",
    "PrivacyBudget",
    "laplace_scale",
    "gaussian_sigma",
    "laplace_sample",
    "gaussian_sample",
    "noise_sequence",
    "apply_dp",
]

# gaussian 메커니즘의 기본 δ (Rust DEFAULT_DELTA 와 동일)
DEFAULT_DELTA = 1e-5

# 세션 총 프라이버시 예산 기본값 (환경변수 XAZZ_DP_BUDGET 로 재정의 가능)
DEFAULT_TOTAL_BUDGET = 10.0

_MASK64 = (1 << 64) - 1


# ─────────────────────────────────────────────────────────────────────────────
# 난수 생성기 (SplitMix64) — Rust 구현과 비트 단위 동일
# ─────────────────────────────────────────────────────────────────────────────


class SplitMix64:
    """SplitMix64 PRNG. seed 고정 시 Rust `dp::SplitMix64` 와 동일 시퀀스를 생성한다."""

    def __init__(self, seed: int) -> None:
        self.state = seed & _MASK64

    def next_u64(self) -> int:
        self.state = (self.state + 0x9E3779B97F4A7C15) & _MASK64
        z = self.state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & _MASK64
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & _MASK64
        return (z ^ (z >> 31)) & _MASK64

    def next_f64(self) -> float:
        """(0, 1) 개구간 균등분포 — 양 끝값 0/1 제외 (ln(0) 특이점 방지)."""
        while True:
            u = (self.next_u64() >> 11) * (1.0 / (1 << 53))
            if 0.0 < u < 1.0:
                return u


def _time_seed() -> int:
    return time.time_ns() & _MASK64


# ─────────────────────────────────────────────────────────────────────────────
# 노이즈 파라미터·샘플링 — Rust 와 동일 수식
# ─────────────────────────────────────────────────────────────────────────────


def laplace_scale(sensitivity: float, epsilon: float) -> float:
    """Laplace 메커니즘 scale: b = Δf / ε"""
    return sensitivity / epsilon


def gaussian_sigma(sensitivity: float, epsilon: float, delta: float) -> float:
    """Gaussian 메커니즘 표준편차: σ = Δf · √(2·ln(1.25/δ)) / ε
    (Dwork & Roth, The Algorithmic Foundations of Differential Privacy, Thm 3.22)
    """
    return sensitivity * math.sqrt(2.0 * math.log(1.25 / delta)) / epsilon


def laplace_sample(rng: SplitMix64, scale: float) -> float:
    """Laplace(0, scale) 샘플 — 역CDF 방식 (Rust 와 동일)."""
    u = rng.next_f64() - 0.5
    return -scale * math.copysign(1.0, u) * math.log(1.0 - 2.0 * abs(u))


def gaussian_sample(rng: SplitMix64, sigma: float) -> float:
    """N(0, sigma²) 샘플 — Box-Muller 변환 (Rust 와 동일)."""
    u1 = rng.next_f64()
    u2 = rng.next_f64()
    return sigma * math.sqrt(-2.0 * math.log(u1)) * math.cos(2.0 * math.pi * u2)


def noise_sequence(
    n: int,
    *,
    epsilon: float,
    mechanism: str = "laplace",
    sensitivity: float = 1.0,
    delta: float | None = None,
    seed: int | None = None,
) -> list[float]:
    """길이 n 의 노이즈 시퀀스를 생성한다. 같은 인수면 Rust 엔진과 동일한 값."""
    _validate(epsilon, sensitivity, mechanism, delta)
    rng = SplitMix64(seed if seed is not None else _time_seed())
    if mechanism == "laplace":
        b = laplace_scale(sensitivity, epsilon)
        return [laplace_sample(rng, b) for _ in range(n)]
    sigma = gaussian_sigma(sensitivity, epsilon, delta or DEFAULT_DELTA)
    return [gaussian_sample(rng, sigma) for _ in range(n)]


def _validate(
    epsilon: float, sensitivity: float, mechanism: str, delta: float | None
) -> None:
    if epsilon <= 0.0:
        raise ValueError(f"DP 에러: epsilon 은 0보다 커야 합니다. 실제: {epsilon}")
    if sensitivity <= 0.0:
        raise ValueError(f"DP 에러: sensitivity 는 0보다 커야 합니다. 실제: {sensitivity}")
    if mechanism not in ("laplace", "gaussian"):
        raise ValueError(
            f"DP 에러: mechanism 은 laplace 또는 gaussian 이어야 합니다. 실제: '{mechanism}'"
        )
    if mechanism == "gaussian":
        d = DEFAULT_DELTA if delta is None else delta
        if not (0.0 < d < 1.0):
            raise ValueError(f"DP 에러: gaussian 의 delta 는 (0, 1) 범위여야 합니다. 실제: {d}")


# ─────────────────────────────────────────────────────────────────────────────
# 세션 프라이버시 예산 (ε-budget) — Rust PrivacyBudget 와 동일 정책
# ─────────────────────────────────────────────────────────────────────────────


class BudgetExceededError(RuntimeError):
    """세션 ε-budget 초과 — 반복 질의 노이즈 평균화(재구성 공격) 방지."""


@dataclass
class PrivacyBudget:
    total: float
    spent: float = field(default=0.0)

    @classmethod
    def from_env(cls) -> "PrivacyBudget":
        """환경변수 XAZZ_DP_BUDGET 에서 총 예산을 읽는다 (기본 10.0)."""
        raw = os.environ.get("XAZZ_DP_BUDGET", "")
        try:
            total = float(raw)
        except ValueError:
            total = DEFAULT_TOTAL_BUDGET
        if total <= 0.0:
            total = DEFAULT_TOTAL_BUDGET
        return cls(total=total)

    def spend(self, epsilon: float) -> None:
        if epsilon <= 0.0:
            raise ValueError(f"DP 에러: epsilon 은 0보다 커야 합니다. 실제: {epsilon}")
        if self.spent + epsilon > self.total:
            raise BudgetExceededError(
                f"DP 예산 초과: 이번 요청 ε={epsilon:.4f} 를 더하면 누적 "
                f"{self.spent + epsilon:.4f} > 총 예산 {self.total:.4f}. "
                "반복 질의를 통한 노이즈 평균화(재구성 공격) 방지를 위해 실행을 거부합니다. "
                "(총 예산은 XAZZ_DP_BUDGET 환경변수로 조정 가능)"
            )
        self.spent += epsilon

    @property
    def remaining(self) -> float:
        return max(0.0, self.total - self.spent)


# ─────────────────────────────────────────────────────────────────────────────
# DataFrame 노이즈 주입 (pandas / polars duck-typing)
# ─────────────────────────────────────────────────────────────────────────────


def apply_dp(
    df,
    *,
    epsilon: float,
    mechanism: str = "laplace",
    sensitivity: float = 1.0,
    delta: float | None = None,
    seed: int | None = None,
    budget: PrivacyBudget | None = None,
):
    """DataFrame 의 모든 숫자형 컬럼에 DP 노이즈를 주입한 새 DataFrame 을 반환한다.

    - 숫자형 컬럼 → float 승격 후 노이즈 합산 / 비숫자 컬럼 → 원본 유지
    - 결측(NaN/null) → 그대로 유지 (노이즈로 결측을 위장하지 않음)
    - `budget` 을 주면 적용 전에 ε 을 차감하고, 초과 시 BudgetExceededError

    pandas.DataFrame 과 polars.DataFrame 을 지원한다.
    반환: (노이즈 적용된 DataFrame, 리포트 dict) — 리포트 스키마는 Rust 의
    `[xazz:dp]` JSON 마커(DpReport)와 동일하다.
    """
    _validate(epsilon, sensitivity, mechanism, delta)
    if budget is not None:
        budget.spend(epsilon)

    d = DEFAULT_DELTA if delta is None else delta
    if mechanism == "laplace":
        noise_param = laplace_scale(sensitivity, epsilon)
    else:
        noise_param = gaussian_sigma(sensitivity, epsilon, d)

    rng = SplitMix64(seed if seed is not None else _time_seed())

    def sample() -> float:
        if mechanism == "laplace":
            return laplace_sample(rng, noise_param)
        return gaussian_sample(rng, noise_param)

    kind = type(df).__module__.split(".")[0]  # "pandas" | "polars"
    if kind == "pandas":
        out, noised_columns = _apply_pandas(df, sample)
    elif kind == "polars":
        out, noised_columns = _apply_polars(df, sample)
    else:
        raise TypeError(
            f"지원하지 않는 DataFrame 타입: {type(df)!r} (pandas / polars 지원)"
        )

    if not noised_columns:
        raise ValueError(
            "DP 에러: 노이즈를 적용할 숫자형 컬럼이 없습니다. "
            "apply_dp 는 집계 결과(count/sum/mean 등)에 사용하세요."
        )

    report = {
        "mechanism": mechanism,
        "epsilon": epsilon,
        "delta": d if mechanism == "gaussian" else None,
        "sensitivity": sensitivity,
        "noise_param": noise_param,
        "noised_columns": noised_columns,
        "seed": seed,
    }
    return out, report


def _apply_pandas(df, sample):
    import pandas as pd

    out = df.copy()
    noised = []
    for name in df.columns:
        col = df[name]
        if not pd.api.types.is_numeric_dtype(col) or pd.api.types.is_bool_dtype(col):
            continue
        vals = col.astype("float64")
        # 결측은 그대로 두고, 관측값에만 노이즈 합산 (Rust 와 동일한 순회 순서)
        out[name] = [
            v if pd.isna(v) else v + sample() for v in vals
        ]
        noised.append(str(name))
    return out, noised


def _apply_polars(df, sample):
    import polars as pl

    exprs = []
    noised = []
    numeric = {
        pl.Float64, pl.Float32,
        pl.Int64, pl.Int32, pl.Int16, pl.Int8,
        pl.UInt64, pl.UInt32, pl.UInt16, pl.UInt8,
    }
    out = df.clone()
    for name, dtype in zip(df.columns, df.dtypes):
        if dtype not in numeric:
            continue
        vals = df[name].cast(pl.Float64).to_list()
        out = out.with_columns(
            pl.Series(name, [v if v is None else v + sample() for v in vals])
        )
        noised.append(name)
    _ = exprs
    return out, noised
