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
| `XAZZ_DP_MAX_EPSILON` | unset (no cap) | **Per-query** ε cap, opt-in (issue #118). A `withDp` whose ε exceeds it is refused before any noise is drawn, so no budget is spent. Invalid or `<= 0` values are ignored. |

Two layers cap ε, and they are independent:

- **Policy pack (compile time)** — `max_epsilon` (default 3.0) via rule XZP005. Enforced by the
  policy gate in the CLI, the engine and the server; a pack may lower it, raise it, or downgrade the
  rule's severity.
- **Runtime (`XAZZ_DP_MAX_EPSILON`)** — checked inside `apply_dp` regardless of which policy pack is
  active or whether the gate ran. The server also checks it before reserving the tenant's budget
  (`POST /execute` → 422 naming the offending steps), and the runner inherits the variable so the
  engine re-checks it.

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

### Scope: in-process session vs. server ledger

There are two budget scopes, and they are deliberately separate.

- **In-process (`PrivacyBudget`, CLI/engine).** Within one `xazz run` process, repeated `withDp` calls in the same `.xzz` file share one budget. The CLI executes one file per process, so this budget lives only for that run.
- **Server (per-tenant persistent ledger).** `xazz-server` keeps a tenant-scoped ledger in its store (`dp_budget`), so a tenant's ε/δ spend **does accumulate across `/execute` requests** — the "session" is the tenant, not the subprocess. Because the server still spawns a fresh subprocess per request, the ledger — not the process — is the source of truth:
  - Before a run, `/execute` **reserves** the tenant's remaining envelope in one atomic store transaction (`reserve_dp_budget`), which also serializes concurrent runs of the same tenant across server instances. A reservation that outlives `XAZZ_DP_RESERVATION_TTL_SECS` is reclaimed, so a crashed instance cannot pin a tenant's budget forever.
  - After the run, the actual `[xazz:dp]` spend is **settled** against the ledger (`settle_dp_reservation`) and the reservation is released. A unique reservation id means a stale or duplicate settle cannot charge the tenant twice or release a newer holder's reservation.
  - The envelope is per tenant (`XAZZ_TENANT_DP_BUDGET` / `XAZZ_TENANT_DP_DELTA_BUDGET`) with an optional rolling window (`XAZZ_TENANT_DP_WINDOW_SECS`, overridable via `PUT /dp/budget/window`). The remaining envelope is handed to the runner as `XAZZ_DP_BUDGET` / `XAZZ_DP_DELTA_BUDGET` and re-checked inside `apply_dp`, so the ledger is authoritative for the run.

**Known limitation.** The server ledger is scoped per **tenant**, not per end-user or per named session: multiple users sharing one tenant share one envelope. Per-user/session sub-budgets are not implemented.

---

## 4. Standardized vs. rigorously verified

This implementation provides **deterministic, explainable composition accounting**. The following are not covered yet and require follow-up review before this can be called a "rigorously verified privacy framework":

- Theoretical bounds for adaptive queries
- Tighter bounds via RDP / advanced composition
- Parallel composition beyond sequential execution
- Automatic sensitivity inference (aggregation-aware, clipping)
- Cross-checking results against external audit tools (e.g. OpenDP)

The semantics in this document implement exactly the **correct composition of basic sequential composition**.