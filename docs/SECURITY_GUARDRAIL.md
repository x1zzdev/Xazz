# Policy-as-Code 정적 보안 가드레일

> Xazz 는 `.xzz` 파이프라인이 **실행되기 전에** 개인정보 유출과 보안 컴플라이언스
> 위반을 정적으로 탐지하고 차단한다. Type Checker 가 "이 코드가 돌아가는가"를
> 묻는다면, 가드레일은 "이 코드를 돌려도 되는가"를 묻는다.

관련: [ARCHITECTURE.md](ARCHITECTURE.md) · [WORKSPACE.md](WORKSPACE.md) ·
[design/security-model.md](design/security-model.md) · [experiments/slm_guardrail](../experiments/slm_guardrail/README.md)

---

## 1. 30초 요약

```bash
# 위반 코드는 실행 전에 차단된다
$ xazz run examples/security/patient_unsafe.xzz
[xazz 보안 가드레일] 실행이 차단되었습니다
  ✖ XZP001 DIRECT_IDENTIFIER_EXPOSED
    사유 : 직접 식별자 컬럼이 결과로 그대로 출력됩니다: patient_id, name, phone.
    보정 : |> select([...]) 에서 patient_id, name, phone 을(를) 제외하거나 …
$ echo $?
1

# 차단에서 끝나지 않는다 — 검증된 안전 코드를 제안한다
$ xazz policy examples/security/patient_unsafe.xzz --fix

# 안전한 코드는 그대로 실행된다
$ xazz run examples/security/patient_safe.xzz
📊 [xazz Execution Result: 'visits_by_band' (Top 5 Rows)]
```

---

## 2. 설계 원칙

### Fail-closed — "판단 불가"는 안전이 아니다

정책 파일이 깨졌거나, 코드가 파싱되지 않거나, 스키마를 해석하지 못하면
**실행을 거부한다**. "정책을 못 읽었으니 일단 실행한다"는 가드레일의 존재
이유를 무너뜨린다.

| 상황 | 결과 |
|---|---|
| `XAZZ_POLICY_PATH` 가 가리키는 파일이 없음 | 실행 거부 · `XZP999 POLICY_LOAD_FAILED` |
| 정책 JSON 구문 오류 | 실행 거부 · `XZP999` |
| `.xzz` 구문 오류 | 실행 거부 · `XZP000 PARSE_FAILED` (리터럴 스캔은 계속 수행) |
| 스키마 미해석 + `select` 없음 | 경고 · `XZP014` (정책에서 `block` 으로 승격 가능) |

### 정밀 우선 — 부분 문자열로 판정하지 않는다

컬럼 분류는 정규화(소문자화 + 영숫자/한글만 남김) 후 **완전 일치**로만 한다.
`patient_id` · `patientID` · `Patient-Id` 는 모두 같은 컬럼이지만,
`message` 는 `age` 로, `sexagesimal` 은 `sex` 로 잡히지 않는다.

### 집계 결과는 식별자가 아니다

이 구분이 없으면 정상 통계 쿼리가 전부 오탐으로 막힌다.

```
groupBy("age_band") |> count("patient_id")
                            ^^^^^^^^^^^^ 결과 컬럼은 '건수'이지 환자번호가 아니다 → 통과
select([patient_id, age_band])
        ^^^^^^^^^^ 원본 값 그대로 → 차단
```

### 원본 값을 리포트에 싣지 않는다

탐지된 비밀값은 항상 마스킹된다. `900101-1234568` 은 `90************` 으로만
보고된다. 리포트 자체가 2차 유출 경로가 되면 안 되기 때문이다.

---

## 3. 규칙 카탈로그

| ID | 이름 | 기본 심각도 | 무엇을 막는가 |
|---|---|---|---|
| `XZP000` | `PARSE_FAILED` | block | 파싱 불가 — 안전성을 증명하지 못함 |
| `XZP001` | `DIRECT_IDENTIFIER_EXPOSED` | block | 이름·환자번호·연락처 등이 결과로 그대로 출력 |
| `XZP002` | `SENSITIVE_ATTRIBUTE_ROW_LEVEL` | block | 진단명·소득 등 민감 속성이 집계 없이 행 단위로 출력 |
| `XZP003` | `QUASI_IDENTIFIER_COMBINATION` | block¹ | 준식별자(나이+성별+우편번호…)가 임계치 이상 결합 |
| `XZP004` | `AGGREGATE_WITHOUT_DP` | block² | 민감 속성 집계에 차등 프라이버시 미적용 |
| `XZP005` | `DP_EPSILON_TOO_LARGE` | block | ε 이 정책 상한 초과 또는 0 이하 |
| `XZP010` | `PII_LITERAL_IN_SOURCE` | block | 주민등록번호·전화번호·이메일·카드번호 하드코딩 |
| `XZP011` | `HARDCODED_SECRET` | block | API 키·개인키·비밀번호 하드코딩 |
| `XZP012` | `SENSITIVE_PATH_ACCESS` | block | `/etc/passwd`, `~/.ssh/`, `.aws/credentials` 등 접근 |
| `XZP013` | `PATH_TRAVERSAL` | warn | `..` 상위 디렉터리 탈출 경로 |
| `XZP014` | `UNRESOLVED_SCHEMA` | warn | 스키마 미해석 — 출력 컬럼을 확정하지 못함 |
| `XZP999` | `POLICY_LOAD_FAILED` | block | 정책 자체를 불러오지 못함 |

¹ 집계된 파이프라인에서는 `warn` 으로 낮아진다 (개별 레코드가 남지 않음).
² `require_dp_for_sensitive_aggregate: false` 이면 `warn`.

모든 심각도는 정책 파일의 `rule_severity` 로 재정의할 수 있다.

### 리터럴 탐지 정밀도

오탐을 줄이기 위해 형태만 보지 않고 검증까지 한다.

- **주민등록번호** — `YYMMDD-SXXXXXX` 형태 + 성별코드 1~8 + 월/일 범위 +
  가중치 체크섬. `900101-1234567`(체크섬 불일치)은 탐지하지 않는다.
- **신용카드** — 13~19자리 + **Luhn 검증**. `4111-1111-1111-1112` 는 무시.
  Luhn 만으로는 부족하다 — 임의의 긴 숫자열은 약 1/10 확률로 Luhn 을 통과한다.
  그래서 구분자 없는 숫자열에는 **발급사 식별번호(IIN) 선두 자리**(3·4·5·6, MC 2-시리즈)를
  추가로 요구하고, 식별자·경로에 붙어 있는 숫자열(`xazz_test_4150_1787805001967327111`)은
  제외한다. 나노초 타임스탬프·주문번호가 카드번호로 잡히는 것을 막는다.
- **전화번호** — 하이픈 구분자를 요구한다. `2026-08-27` 같은 날짜를
  전화번호로 오탐하지 않는다.
- **자격증명** — `<YOUR_PASSWORD>`, `********`, `${VAR}` 같은 플레이스홀더는 무시.

> 정규식 크레이트를 쓰지 않고 직접 스캐너를 구현했다. `xazz-compiler` 는 CLI
> 바이너리에 링크되므로 의존성을 늘리지 않는다는 아키텍처 제약을 지킨다
> ([CONTRIBUTING.md](../CONTRIBUTING.md) 참조).

---

## 4. 게이트가 걸리는 세 지점

```
Visual IDE ──POST /execute──► xazz-server ──┐
                              [게이트 ①]     │  위반이면 422, 실행기 스폰 안 함
                                            ▼
CLI ────────xazz run────────► xazz [게이트 ②]  위반이면 exit 1, 서브프로세스 안 띄움
                                            │
                                            ▼
                              xazz-runner (인자 릴레이)
                                            │
                                            ▼
                              xazz-exec [게이트 ③] ◄── 실제 Polars 실행 직전
```

**③ 이 최종 관문이다.** ①·② 를 우회해도 Polars 를 돌리는 곳은 ③ 하나뿐이므로,
어떤 경로로 들어오든 이 지점을 지나야 한다. `xazz-exec` 에 CSV 를 직접 넘기는
벤치마크 경로(`run_csv_benchmark`)도 결국 `run_pipeline` 을 호출하므로 덮인다.

① 을 따로 두는 이유는 차단이 아니라 **응답 품질**이다. 프런트엔드는 서브프로세스
기동 비용 없이 구조화된 위반 리포트를 즉시 받는다. 또한 차단된 요청도
감사 로그에 `outcome: "blocked"` 로 기록된다.

### 알려진 한계 — PATH 셰도잉

`xazz-server` 는 `xazz` 를, `xazz-runner` 는 `xazz-exec` 를 찾을 때 마지막
수단으로 `PATH` 를 뒤진다. 공격자가 서버 프로세스의 `PATH` 를 조작해 가짜
`xazz-exec` 를 심을 수 있다면 게이트를 통과하지 않고 임의 실행이 가능하다.

이것은 Rust 코드로 닫을 수 있는 문제가 아니라 **배포 환경의 문제**다. 운영
환경에서는 실행 파일을 고정 경로에 두고 서버 프로세스의 `PATH` 를 통제해야
한다. "우회 불가"라고 쓰지 않고 이 한계를 명시하는 편이 정직하다.

---

## 5. 정책 파일

정책은 코드가 아니라 데이터다. 조직은 JSON 하나만 교체해서 가드레일 동작을
바꾼다.

### 적용 우선순위 (fail-closed)

1. 환경변수 `XAZZ_POLICY_PATH` — 지정되면 **반드시** 로딩에 성공해야 한다.
2. 작업 디렉터리의 `xazz.policy.json` — 존재하면 **반드시** 로딩에 성공해야 한다.
3. 둘 다 없으면 내장 기본 정책 (`xazz-builtin-pii`).

```bash
# 강화된 의료 정책 적용
XAZZ_POLICY_PATH=examples/security/healthcare_policy.json \
    xazz policy pipeline.xzz
```

### 스키마

```jsonc
{
  "id": "xazz-healthcare-strict",          // 필수 (비어 있으면 로딩 실패)
  "version": "1.0.0",
  "description": "...",

  "direct_identifiers":   ["patient_id", "name", "주민등록번호", ...],
  "sensitive_attributes": ["disease", "salary", "종교", ...],
  "quasi_identifiers":    ["age", "gender", "zip_code", ...],

  "quasi_identifier_threshold": 2,          // 이 개수 이상 결합 시 위반 (기본 3)
  "require_dp_for_sensitive_aggregate": true,
  "max_epsilon": 1.0,                       // ε 상한 (기본 3.0)
  "remediation_epsilon": 0.5,               // 자동 보정이 삽입하는 ε (기본 1.0)

  "allowed_output_columns": ["age_band"],   // 분류를 무력화하는 allowlist
  "denied_path_fragments": ["/etc/passwd", "/.ssh/", ...],

  "rule_severity": { "XZP013": "block" },   // 규칙별 심각도 재정의

  // ── 감사 증빙용 메타데이터 ──────────────────────────────────────────
  "domain": "healthcare",                   // common | healthcare | finance | public-sector | …
  "risk_level": "high",                     // low | medium | high (가명정보 가이드라인 위험도 구분)
  "rule_source_refs": {                     // 규칙별 규제 근거 재정의
    "XZP002": "의료법 제19조 · 개인정보 보호법 제23조"
  }
}
```

컬럼명은 한국어로 써도 된다 — 정규화가 한글을 보존한다.

### Domain Policy Pack

공통 기준(Common Security Baseline)을 내장 정책이 담당하고, 도메인별 고유 규제는
정책 팩으로 확장한다. 모든 법규를 코드에 박아 넣는 대신 **규제를 실행 가능한
룰셋으로 바꿀 수 있는 구조**를 제공하는 것이 목표다.

| 팩 | `domain` | 위험도 | 준식별자 임계치 | 특징 |
|---|---|---|---|---|
| 내장 기본 | `common` | medium | 3 | 한국 개인정보보호법 맥락의 공통 기준 |
| [`healthcare_policy.json`](../examples/security/healthcare_policy.json) | `healthcare` | high | 2 | 환자ID·진단명·처방·검사결과, 의료법 제19조 |
| [`finance_policy.json`](../examples/security/finance_policy.json) | `finance` | high | 2 | 계좌·카드번호·신용점수·거래내역, 신용정보법 |
| [`public_sector_policy.json`](../examples/security/public_sector_policy.json) | `public-sector` | high | 2 | 주민등록번호·민원내용·수급자격, 공공데이터법 |

```bash
XAZZ_POLICY_PATH=examples/security/finance_policy.json \
    xazz policy pipeline.xzz
```

### 감사 증빙 (Compliance Evidence)

모든 위반은 **무엇이 왜 막혔는지 사후에 따라갈 수 있는 형태**로 기록된다.

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

`rule_id` · `source_ref` · `policy_version` · `domain` · `risk_level` 다섯 항목이
한 리포트에 함께 담기므로, 내부통제·사후감사에서 "이 차단은 어떤 기준의 몇 조에
근거했는가"를 바로 확인할 수 있다. 여기에 기존 SHA-256 감사 로그가 코드 해시와
실행 결과(`outcome`)를 이어 붙인다 — 차단된 실행도 `outcome: "blocked"` 로 남는다.

> ⚠️ `source_ref` 는 **법률 자문이 아니라 감사 추적용 참조**다. 어떤 기준을 근거로
> 규칙을 만들었는지 남기기 위한 것이며, 실제 적용 법령은 조직·도메인마다 다르므로
> `rule_source_refs` 로 재정의하는 것을 전제로 설계했다.

---

## 6. 자동 보정

차단에서 끝나면 개발자는 다음 행동을 못 한다. 가드레일은 **검증된 안전 코드**를
함께 제시한다.

### 2단 구조

```
① 결정적 보정 (항상 동작)
   AST 를 직접 고쳐 쓰고 printer 로 .xzz 를 다시 찍어낸다.
   문자열 치환이 아니므로 구문이 깨지지 않는다.
     · 직접 식별자·행 단위 민감 속성 → select 투영에서 제거
     · 준식별자 초과분 → 임계치 미만까지 제거
     · 민감 속성 집계 → |> withDp(...) 삽입
     · ε 상한 초과 → 상한값으로 클램프

② 온프레미스 sLM (선택 — Qwen2.5-Coder-1.5B)
   더 자연스러운 재작성을 제안한다. 단, 제안은 반드시 같은 정책 엔진으로
   재파싱·재검증되며, 통과하지 못하면 ① 로 되돌아간다.
```

### 자동으로 고치지 않는 것

하드코딩된 비밀키·개인정보는 `residual` 로 남긴다. **소스에서 값을 지우는
것만으로는 끝나지 않기 때문이다** — 노출된 자격증명은 폐기·재발급해야 한다.
`residual` 이 비어 있지 않으면 `verified` 는 `false` 이며, 보정 코드를
"안전하다"고 표시하지 않는다.

### 보정이 의도를 바꿀 수 있다

결정적 보정은 위반을 확실히 없애지만 질문 자체를 좁힐 수 있다.

```
원본:  select([age, disease])     "40대 이상 환자의 진단명"
보정:  select([age])              위반은 사라졌지만 질문도 사라졌다
```

sLM 층이 존재하는 이유가 이것이다. 자세한 지표 정의는
[experiments/slm_guardrail/README.md](../experiments/slm_guardrail/README.md) 참조.

> 보정 코드는 AST 에서 다시 생성되므로 **원본 주석과 서식은 보존되지 않는다.**
> 이 사실은 응답의 `notes` 에 항상 명시된다.

---

## 7. 사용법

### CLI

```bash
xazz policy <file.xzz>                    # 검사 (통과 0 / 위반 1)
xazz policy <file.xzz> --json             # 기계 판독용 JSON
xazz policy <file.xzz> --fix              # 안전한 대체 코드까지 제안
xazz policy <file.xzz> --fix --out safe.xzz
xazz run <file.xzz>                       # 실행 — 위반이면 자동 차단
```

### HTTP API

| 메서드 | 경로 | 설명 |
|---|---|---|
| `GET` | `/security/policy` | 활성 정책 + sLM 설정 조회 |
| `POST` | `/security/policy/check` | 실행 없이 검사만 (위반이어도 200) |
| `POST` | `/security/remediate` | 보정 코드 + 위반 리포트 반환 |
| `POST` | `/execute` | 실행 — 위반이면 **422** + 리포트 |

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
        "message": "직접 식별자 컬럼이 결과로 그대로 출력됩니다: name. …",
        "statement_index": 1,
        "variable": "x",
        "columns": ["name"],
        "remediation_hint": "|> select([...]) 에서 name 을(를) 제외하거나 …"
      }
    ],
    "warnings": []
  },
  "remediation": {
    "strategy": "deterministic",
    "code": "type P = {\n    name: string,\n    age_band: string,\n};\n\nv x = load(\"d.csv\") :: P\n    |> select([age_band]);\n",
    "applied": [{ "rule_id": "XZP001", "description": "출력에서 'name' 컬럼을 제거했습니다." }],
    "residual": [],
    "notes": [],
    "verified": true
  },
  "slm": { "enabled": false, "model": "xazz-guardrail", "endpoint": "http://127.0.0.1:11434" }
}
```

### 실행 엔진 마커

`xazz-exec` 는 차단·통과와 무관하게 항상 `[xazz:policy]` 마커를 stdout 에
내보낸다. 프런트엔드는 "검사가 실제로 수행되었다"는 사실 자체를 신뢰할 수 있다.

```
[xazz:policy] {"policy_id":"xazz-builtin-pii","safe_to_execute":false,"violations":[…]}
```

### 온프레미스 sLM 켜기

```bash
XAZZ_SLM_ENABLED=1 \
XAZZ_SLM_MODEL=xazz-guardrail \
XAZZ_SLM_ENDPOINT=http://127.0.0.1:11434 \
    cargo run -p xazz-server
```

| 환경변수 | 기본값 | 설명 |
|---|---|---|
| `XAZZ_SLM_ENABLED` | `false` | `1`/`true`/`on` 일 때만 sLM 호출 |
| `XAZZ_SLM_ENDPOINT` | `http://127.0.0.1:11434` | 로컬호스트 고정 권장 |
| `XAZZ_SLM_MODEL` | `xazz-guardrail` | Ollama 모델명 |
| `XAZZ_SLM_TIMEOUT_MS` | `20000` | 응답 대기 시간 |

sLM 이 꺼져 있거나 연결에 실패하면 결정적 보정이 그대로 쓰인다.
서비스가 멈추지 않는다는 것은 테스트로 보장된다.

---

## 8. 예제

```bash
# 합성 데이터 생성 (실제 개인정보 없음, CSV 는 커밋되지 않는다)
python3 examples/security/generate_patients.py

xazz policy examples/security/patient_unsafe.xzz --fix       # 4개 규칙 위반 + 보정
xazz policy examples/security/patient_secret_leak.xzz --fix  # 자동 보정 불가 사례
xazz policy examples/security/patient_safe.xzz               # 통과
xazz run    examples/security/patient_safe.xzz               # 실행
```

---

## 9. 구현 위치

| 경로 | 역할 |
|---|---|
| `xazz-compiler/src/policy/mod.rs` | 정책·리포트 타입, 규칙 카탈로그, 진입점, 정책 로딩 |
| `xazz-compiler/src/policy/rules.rs` | 출력 컬럼 추론(`PipelineShape`) 및 규칙 판정 |
| `xazz-compiler/src/policy/patterns.rs` | 리터럴 스캐너 (RRN 체크섬 · Luhn · API 키) |
| `xazz-compiler/src/policy/printer.rs` | AST → `.xzz` 프린터 (왕복 성질 테스트 포함) |
| `xazz-compiler/src/policy/remediate.rs` | 결정적 AST 보정 + 재검증 |
| `src/policy_cli.rs` | `xazz policy` 서브커맨드 및 `xazz run` 게이트 |
| `xazz-exec/src/runtime.rs` (STEP 3.6) | 최종 실행 게이트 + `[xazz:policy]` 마커 |
| `xazz-server/src/guardrail.rs` | `/execute` 게이트, 보정 오케스트레이션 |
| `xazz-server/src/slm.rs` | Ollama 어댑터, 프롬프트 구성, 코드 추출 |

가드레일은 `xazz-compiler` 에 있다. Polars/Tokio 를 링크하지 않는 유일한 공용
크레이트이므로, CLI·실행 엔진·API 서버 세 진입점 모두에 **같은 코드**로 게이트를
걸 수 있다. 정책 판정이 진입점마다 달라지면 그 자체가 취약점이다.
