# Policy-as-Code Static Security Guardrail

> Xazz statically detects and blocks PII leaks and security-compliance violations **before** a `.xzz` pipeline runs. The Type Checker asks "does this code run?"; the guardrail asks "is it safe to run this code?"

Related: [ARCHITECTURE.md](ARCHITECTURE.md) · [WORKSPACE.md](WORKSPACE.md) ·
[design/security-model.md](design/security-model.md) · [experiments/slm_guardrail](../experiments/slm_guardrail/README.md)

---

## 1. 30-second summary

```bash
# Violating code is blocked before execution
$ xazz run examples/security/patient_unsafe.xzz
[xazz security guardrail] execution blocked
  ✖ XZP001 DIRECT_IDENTIFIER_EXPOSED
    reason : direct identifier columns are emitted as-is in the result: patient_id, name, phone.
    fix    : remove patient_id, name, phone from |> select([...]) or …
$ echo $?
1

# It does not stop at blocking — it proposes verified safe code
$ xazz policy examples/security/patient_unsafe.xzz --fix

# Safe code runs normally
$ xazz run examples/security/patient_safe.xzz
📊 [xazz Execution Result: 'visits_by_band' (Top 5 Rows)]
```

---

## 2. Design principles

### Fail-closed — "can't decide" is not safe

If the policy file is broken, the code fails to parse, or the schema cannot be resolved, execution is **refused**. "I couldn't read the policy, so let's run anyway" would defeat the purpose of a guardrail.

| Situation | Result |
|---|---|
| File pointed to by `XAZZ_POLICY_PATH` missing | Execution refused · `XZP999 POLICY_LOAD_FAILED` |
| Policy JSON syntax error | Execution refused · `XZP999` |
| `.xzz` syntax error | Execution refused · `XZP000 PARSE_FAILED` (literal scan still runs) |
| Unresolved schema + no `select` | Warning · `XZP014` (promotable to `block` in policy) |

### Precision first — no substring matching

Column classification normalizes (lowercase + keep alphanumerics/Hangul) then requires **exact match**. `patient_id` · `patientID` · `Patient-Id` are the same column, but `message` is not caught as `age`, and `sexagesimal` is not caught as `sex`.

### Aggregate results are not identifiers

Without this distinction, every legitimate statistical query would be blocked as a false positive.

```
groupBy("age_band") |> count("patient_id")
                            ^^^^^^^^^^^^ result column is a *count*, not a patient number → allowed
select([patient_id, age_band])
        ^^^^^^^^^^ raw values passed through → blocked
```

### Reports never carry raw values

Detected secrets are always masked. `900101-1234568` is reported only as `90************`. A report must not become a secondary leak channel.

---

## 3. Rule catalog

| ID | Name | Default severity | What it blocks |
|---|---|---|---|
| `XZP000` | `PARSE_FAILED` | block | Unparseable — safety cannot be proven |
| `XZP001` | `DIRECT_IDENTIFIER_EXPOSED` | block | Names, patient numbers, contact info emitted as-is in results |
| `XZP002` | `SENSITIVE_ATTRIBUTE_ROW_LEVEL` | block | Sensitive attributes (diagnosis, income, …) emitted row-wise without aggregation |
| `XZP003` | `QUASI_IDENTIFIER_COMBINATION` | block¹ | Quasi-identifiers (age+gender+zip…) combined above a threshold |
| `XZP004` | `AGGREGATE_WITHOUT_DP` | block² | Sensitive-attribute aggregate without differential privacy |
| `XZP005` | `DP_EPSILON_TOO_LARGE` | block | ε above the policy cap or ≤ 0 |
| `XZP010` | `PII_LITERAL_IN_SOURCE` | block | Hardcoded RRN, phone, email, or card number |
| `XZP011` | `HARDCODED_SECRET` | block | Hardcoded API key, private key, or password |
| `XZP012` | `SENSITIVE_PATH_ACCESS` | block | Access to `/etc/passwd`, `~/.ssh/`, `.aws/credentials`, etc. |
| `XZP013` | `PATH_TRAVERSAL` | warn | `..` parent-directory escape paths |
| `XZP014` | `UNRESOLVED_SCHEMA` | warn | Schema unresolved — output columns cannot be determined |
| `XZP999` | `POLICY_LOAD_FAILED` | block | The policy itself cannot be loaded |

¹ Downgraded to `warn` in aggregated pipelines (no individual records remain).
² `warn` if `require_dp_for_sensitive_aggregate: false`.

Every severity can be overridden via `rule_severity` in the policy file.

### Literal-detection precision

To cut false positives, detection validates beyond shape.

- **Resident registration number (RRN)** — `YYMMDD-SXXXXXX` form + gender code 1–8 + month/day ranges + weighted checksum. `900101-1234567` (checksum mismatch) is not flagged.
- **Credit cards** — 13–19 digits + **Luhn validation**. `4111-1111-1111-1112` is ignored. Luhn alone is insufficient — about 1 in 10 arbitrary long digit runs passes Luhn. So digit runs without separators additionally require an **issuer identifier (IIN) prefix** (3/4/5/6, MC 2-series), and digit runs attached to identifiers/paths (`xazz_test_4150_1787805001967327111`) are excluded. This prevents nanosecond timestamps and order numbers from being flagged as card numbers.
- **Phone numbers** — hyphen separators required. Dates like `2026-08-27` are not misdetected as phone numbers.
- **Credentials** — placeholders like `<YOUR_PASSWORD>`, `********`, `${VAR}` are ignored.

> Scanners are hand-rolled instead of a regex crate. `xazz-compiler` links into the CLI binary, so it honors the architecture constraint of not growing dependencies (see [CONTRIBUTING.md](../CONTRIBUTING.md)).

---

## 4. Three gate points

```
Visual IDE ──POST /execute──► xazz-server ──┐
                              [gate ①]     │  violations → 422, engine never spawned
                                            ▼
CLI ────────xazz run────────► xazz [gate ②]  violations → exit 1, no subprocess
                                            │
                                            ▼
                              xazz-runner (argument relay)
                                            │
                                            ▼
                              xazz-exec [gate ③] ◄── right before Polars executes
```

**③ is the final gate.** Even if ① and ② are bypassed, ③ is the only place that runs Polars, so every path must pass through it. The benchmark path that feeds a CSV directly to `xazz-exec` (`run_csv_benchmark`) also ends up calling `run_pipeline`, so it is covered.

① exists not for blocking but for **response quality**: the frontend receives a structured violation report without subprocess startup cost. Blocked requests are also recorded in the audit log with `outcome: "blocked"`.

### Known limitation — PATH shadowing

When `xazz-server` looks for `xazz` and `xazz-runner` looks for `xazz-exec`, `PATH` is used as a last resort. If an attacker can manipulate the server process's `PATH` to plant a fake `xazz-exec`, arbitrary execution without passing the gate becomes possible.

This is not something Rust code can close — it is a **deployment-environment** issue. In production, place executables at pinned paths and control the server process's `PATH`. We state this limitation explicitly instead of claiming "unbypassable".

---

## 5. Policy files

Policy is data, not code. An organization changes guardrail behavior by swapping one JSON file.

### Load precedence (fail-closed)

1. Env var `XAZZ_POLICY_PATH` — if set, loading **must** succeed.
2. `xazz.policy.json` in the working directory — if present, loading **must** succeed.
3. If neither, the built-in default policy (`xazz-builtin-pii`).

```bash
# Apply a hardened healthcare policy
XAZZ_POLICY_PATH=examples/security/healthcare_policy.json \
    xazz policy pipeline.xzz
```

### Schema

```jsonc
{
  "id": "xazz-healthcare-strict",          // required (empty → load failure)
  "version": "1.0.0",
  "description": "...",

  "direct_identifiers":   ["patient_id", "name", "주민등록번호", ...],
  "sensitive_attributes": ["disease", "salary", "종교", ...],
  "quasi_identifiers":    ["age", "gender", "zip_code", ...],

  "quasi_identifier_threshold": 2,          // violation when this many combine (default 3)
  "require_dp_for_sensitive_aggregate": true,
  "max_epsilon": 1.0,                       // ε cap (default 3.0)
  "remediation_epsilon": 0.5,               // ε inserted by auto-remediation (default 1.0)

  "allowed_output_columns": ["age_band"],   // allowlist that overrides classification
  "denied_path_fragments": ["/etc/passwd", "/.ssh/", ...],

  "rule_severity": { "XZP013": "block" },   // per-rule severity override

  // ── audit-evidence metadata ──────────────────────────────────────────
  "domain": "healthcare",                   // common | healthcare | finance | public-sector | …
  "risk_level": "high",                     // low | medium | high (PII-guideline risk tiers)
  "rule_source_refs": {                     // per-rule regulatory basis override
    "XZP002": "의료법 제19조 · 개인정보 보호법 제23조"
  }
}
```

Column names may be Korean — normalization preserves Hangul.

### Domain policy packs

A common baseline is provided by the built-in policy; domain-specific regulation is extended via policy packs. The goal is not to hardcode every statute into code but to provide a structure that turns **regulation into executable rulesets**.

| Pack | `domain` | Risk | Quasi-id threshold | Focus |
|---|---|---|---|---|
| Built-in default | `common` | medium | 3 | Common baseline in a K-PIPA context |
| [`healthcare_policy.json`](../examples/security/healthcare_policy.json) | `healthcare` | high | 2 | Patient ID, diagnosis, prescription, lab results, Medical Service Act §19 |
| [`finance_policy.json`](../examples/security/finance_policy.json) | `finance` | high | 2 | Account, card number, credit score, transactions, Credit Information Act |
| [`public_sector_policy.json`](../examples/security/public_sector_policy.json) | `public-sector` | high | 2 | RRN, civil complaints, benefit eligibility, Public Data Act |

```bash
XAZZ_POLICY_PATH=examples/security/finance_policy.json \
    xazz policy pipeline.xzz
```

### Compliance evidence

Every violation is recorded in a form that makes it possible to trace **what was blocked and why**, after the fact.

```jsonc
{
  "policy_id": "xazz-finance-strict",
  "policy_version": "1.0.0",
  "domain": "finance",
  "risk_level": "high",
  "violations": [{
    "rule_id": "XZP001",
    "rule_name": "DIRECT_IDENTIFIER_EXPOSED",
    "severity": "block",
    "columns": ["name", "phone"],
    "source_ref": "신용정보의 이용 및 보호에 관한 법률 제32조 · 개인정보 보호법 제24조"
  }]
}
```

`rule_id` · `source_ref` · `policy_version` · `domain` · `risk_level` travel together in one report, so internal control and post-hoc audit can directly answer "which rule, which clause of which standard was this block based on". The existing SHA-256 audit log then chains the code hash and execution outcome — blocked runs are recorded with `outcome: "blocked"` too.

> ⚠️ `source_ref` is an **audit-trail reference, not legal advice**. It records which standard a rule was based on; the applicable statute varies by organization and domain, so the design assumes `rule_source_refs` overrides.

---

## 6. Auto-remediation

Blocking alone leaves developers with no next step. The guardrail also presents **verified safe code**.

### Two-layer structure

```
① Deterministic remediation (always works)
   Rewrites the AST directly and re-prints .xzz with the printer.
   Not a string substitution, so syntax never breaks.
     · direct identifiers & row-wise sensitive attributes → removed from the select projection
     · excess quasi-identifiers → removed down to below the threshold
     · sensitive-attribute aggregates → |> withDp(...) inserted
     · ε above the cap → clamped to the cap

② On-premise sLM (optional — Qwen2.5-Coder-1.5B)
   Proposes a more natural rewrite. Proposals are always re-parsed and
   re-verified by the same policy engine; if they fail, it falls back to ①.
```

### What is never auto-fixed

Hardcoded secrets and personal data are left as `residual`. **Deleting the value from source is not the end** — exposed credentials must be revoked and reissued. If `residual` is non-empty, `verified` is `false` and the remediated code is not labeled "safe".

### Remediation can change intent

Deterministic remediation reliably removes violations but can narrow the question itself.

```
original: select([age, disease])    "diagnosis of patients in their 40s+"
remediation: select([age])          violation gone, but so is the question
```

This is why the sLM layer exists. See [experiments/slm_guardrail/README.md](../experiments/slm_guardrail/README.md) for metric definitions.

> Remediated code is regenerated from the AST, so **original comments and formatting are not preserved**. This is always stated in the response's `notes`.

---

## 7. Usage

### CLI

```bash
xazz policy <file.xzz>                    # inspect (pass 0 / violation 1)
xazz policy <file.xzz> --json             # machine-readable JSON
xazz policy <file.xzz> --fix              # also propose safe alternatives
xazz policy <file.xzz> --fix --out safe.xzz
xazz run <file.xzz>                       # run — auto-blocked on violation
```

### HTTP API

| Method | Path | Description |
|---|---|---|
| `GET` | `/security/policy` | Active policy + sLM config |
| `POST` | `/security/policy/check` | Inspect only, no execution (200 even on violation) |
| `POST` | `/security/remediate` | Remediated code + violation report |
| `POST` | `/execute` | Execute — **422** + report on violation |

```bash
curl -X POST localhost:8005/security/remediate \
  -H 'content-type: application/json' \
  -d '{"code":"type P = { name: string, age_band: string };\nv x = load(\"d.csv\") :: P |> select([name, age_band]);"}'
```

```jsonc
{
  "safe_to_execute": false,
  "policy_origin": "builtin",
  "policy": {
    "policy_id": "xazz-builtin-pii",
    "safe_to_execute": false,
    "violations": [
      {
        "rule_id": "XZP001",
        "rule_name": "DIRECT_IDENTIFIER_EXPOSED",
        "severity": "block",
        "message": "direct identifier columns are emitted as-is: name. …",
        "statement_index": 1,
        "variable": "x",
        "columns": ["name"],
        "remediation_hint": "remove name from |> select([...]) or …"
      }
    ],
    "warnings": []
  },
  "remediation": {
    "strategy": "deterministic",
    "code": "type P = {\n    name: string,\n    age_band: string,\n};\n\nv x = load(\"d.csv\") :: P\n    |> select([age_band]);\n",
    "applied": [{ "rule_id": "XZP001", "description": "removed the 'name' column from output" }],
    "residual": [],
    "notes": [],
    "verified": true
  },
  "slm": { "enabled": false, "model": "xazz-guardrail", "endpoint": "http://127.0.0.1:11434" }
}
```

### Execution-engine marker

`xazz-exec` always emits a `[xazz:policy]` marker on stdout, regardless of block or pass. The frontend can trust that "an inspection actually ran".

```
[xazz:policy] {"policy_id":"xazz-builtin-pii","safe_to_execute":false,"violations":[…]}
```

### Enabling the on-premise sLM

```bash
XAZZ_SLM_ENABLED=1 \
XAZZ_SLM_MODEL=xazz-guardrail \
XAZZ_SLM_ENDPOINT=http://127.0.0.1:11434 \
    cargo run -p xazz-server
```

| Env var | Default | Description |
|---|---|---|
| `XAZZ_SLM_ENABLED` | `false` | sLM invoked only for `1`/`true`/`on` |
| `XAZZ_SLM_ENDPOINT` | `http://127.0.0.1:11434` | localhost pinned recommended |
| `XAZZ_SLM_MODEL` | `xazz-guardrail` | Ollama model name |
| `XAZZ_SLM_TIMEOUT_MS` | `20000` | response wait time |

If the sLM is off or unreachable, deterministic remediation is used as-is. The service never stops — guaranteed by tests.

---

## 8. Examples

```bash
# Generate synthetic data (no real PII; CSV is not committed)
python3 examples/security/generate_patients.py

xazz policy examples/security/patient_unsafe.xzz --fix       # 4 rule violations + remediation
xazz policy examples/security/patient_secret_leak.xzz --fix  # cannot be auto-fixed case
xazz policy examples/security/patient_safe.xzz               # passes
xazz run    examples/security/patient_safe.xzz               # runs
```

---

## 9. Implementation locations

| Path | Role |
|---|---|
| `xazz-compiler/src/policy/mod.rs` | Policy/report types, rule catalog, entry points, policy loading |
| `xazz-compiler/src/policy/rules.rs` | Output-column inference (`PipelineShape`) and rule evaluation |
| `xazz-compiler/src/policy/patterns.rs` | Literal scanners (RRN checksum · Luhn · API keys) |
| `xazz-compiler/src/policy/printer.rs` | AST → `.xzz` printer (round-trip tests included) |
| `xazz-compiler/src/policy/remediate.rs` | Deterministic AST remediation + re-verification |
| `src/policy_cli.rs` | `xazz policy` subcommand and the `xazz run` gate |
| `xazz-exec/src/runtime.rs` (STEP 3.6) | Final execution gate + `[xazz:policy]` marker |
| `xazz-server/src/guardrail.rs` | `/execute` gate, remediation orchestration |
| `xazz-server/src/slm.rs` | Ollama adapter, prompt construction, code extraction |

The guardrail lives in `xazz-compiler` — the only shared crate that links neither Polars nor Tokio, so the CLI, the execution engine, and the API server all gate with **the same code**. If policy evaluation differed per entry point, that inconsistency would itself be a vulnerability.