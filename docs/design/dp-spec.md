# Differential Privacy (DP) Specification — Mechanisms and Composition Accounting

Status: implemented (v0.3.0) · code: [`xazz-exec/src/dp.rs`](../../xazz-exec/src/dp.rs)

---

## 1. Privacy model

Xazz's `withDp(...)` applies **output perturbation to aggregate results**. The composition of each `(εᵢ, δᵢ)`-DP mechanism is the privacy certification for the whole queryset.

- **Neighboring datasets**: two datasets differing by one added/removed row.
- **DP definition**: (ε, δ)-Differential Privacy — for all neighboring datasets D, D′ and all output sets S, Pr[M(D)∈S] ≤ e^ε · Pr[M(D′)∈S] + δ.

---

## 2. Mechanisms

### Laplace (ε-DP, δ=0)

- Noise: `Lap(0, Δf/ε)` — scale `b = Δf/ε`.
- Pure ε-DP, so the δ contribution is **0**.

### Gaussian (ε, δ)-DP

- Noise: `N(0, σ²)`, σ = Δf·√(2·ln(1.25/δ))/ε (Dwork & Roth Thm 3.22).
- (ε, δ)-DP, so **both ε and δ** are reflected in composition accounting.

### Sensitivity

- `sensitivity` argument (Δf) — default 1.0. The user sets it to match the aggregate (e.g. count → 1, mean → 1/n).
- Clipped queries and group-count validation are currently out of scope.
- **Group keys are NOT excluded from noising** — `apply_dp` noises every numeric column in the result, including numeric group keys (e.g. `groupBy("year")` where `year` is an integer). For a truly sound sensitivity bound the user must set `sensitivity` to match the aggregate (mean/sum have Δf ≠ 1.0) and the numeric group key caveat applies. This is a documented limitation, not a guarantee.

---

## 3. Composition Accounting

`PrivacyBudget` uses **basic sequential composition** (Dwork & Roth Thm 3.16) — k mechanisms that are each (εᵢ, δᵢ)-DP compose into **(Σεᵢ, Σδᵢ)-DP** (exact).

- Laplace: δ contribution 0 → only ε accumulates.
- Gaussian: both ε and δ accumulate.
- **Multi-column noising is charged per column.** Noising `k` columns with the same mechanism is `k` independent mechanisms, so a single `withDp` over `k` numeric columns is charged `k·ε` (and `k·δ` for Gaussian) via `spend_n`. The budget is deducted only **after** noise injection succeeds — a failed `apply_dp` (e.g. no numeric columns) does not consume budget.

### Budget configuration

| Env var | Default | Meaning |
|---|---|---|
| `XAZZ_DP_BUDGET` | 10.0 | Total ε budget |
| `XAZZ_DP_DELTA_BUDGET` | 1e-4 | Total δ budget (for Gaussian) |

### Rejection rules (fail-closed)

Each `withDp` call spends budget via `spend_n(mechanism, ε, δ, k)`; if `Σε > total_ε` or `Σδ > total_δ`, the query is **rejected**. Rejected requests do not consume budget (atomic). This structurally blocks noise-averaging (reconstruction) attacks via repeated queries.

### Audit output (`[xazz:dp]` marker, `xazz run --json`)

Every `withDp` step prints one single-line stdout marker, `[xazz:dp] <JSON>`, after the budget is deducted. The payload is the `DpReport` (`mechanism`, `epsilon`, `delta`, `sensitivity`, `noise_param`, `noised_columns`, `seed`) plus the session budget **after that step**:

| Field | Meaning |
|---|---|
| `budget_spent` / `budget_total` / `budget_remaining` | Σε so far, the ε cap, and `max(total − spent, 0)` |
| `budget_spent_delta` / `budget_total_delta` / `budget_remaining_delta` | The same three for δ (Laplace steps leave δ unchanged) |
| `query_count` | Mechanisms composed so far — a `k`-column `withDp` counts `k` |

`xazz run --json` collects these markers into a `dp` array in execution order (issue #117), so a dashboard or CI job reads the last entry's `budget_remaining` instead of scraping the human-readable stderr line. The server's `parse_stdout_markers` reads the same marker for its per-tenant ledger. Training reports (`[xazz:train]`) are surfaced separately as `training`; DP is a property of the pipeline step, not of the model.

### Session scope (important limitation)

The ε/δ budget is scoped to **one process run** of the pipeline. The xazz CLI executes one `.xzz` file per process, so repeated `withDp` calls *within a single script* share one budget. However, **the server spawns a fresh subprocess per `/execute` request**, so a session that spans multiple requests does **not** accumulate budget across them — the "session" is effectively per-request in the server deployment. A true cross-request budget requires the server to maintain per-session budget state, which is not yet implemented.

---

## 4. Standardized vs. rigorously verified

This implementation provides **deterministic, explainable composition accounting**. The following are not covered yet and require follow-up review before this can be called a "rigorously verified privacy framework":

- Theoretical bounds for adaptive queries
- Tighter bounds via RDP / advanced composition
- Parallel composition beyond sequential execution
- Automatic sensitivity inference (aggregation-aware, clipping)
- Cross-checking results against external audit tools (e.g. OpenDP)

The semantics in this document implement exactly the **correct composition of basic sequential composition**.