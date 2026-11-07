# 실무 사례 (field cases)

산업 현장의 요청을 Xazz로 재현한 예제 모음이다. 각 사례는 **"순진한 구현(위반)"과
"안전한 구현(통과)"을 한 쌍**으로 두어, 규제가 왜 필요하고 Xazz가 그것을 어떻게
파이프라인 안에서 처리하는지 보여준다.

> ⚠️ **정직성 고지.** 모든 데이터는 고정 시드로 생성한 **합성 데이터**이며, 실제
> 기관·기업의 데이터가 아니다. 이 예제들은 "실무에서 쓰이는 형태"를 재현한
> **PoC / 데모**이지, 실제 도입 사례가 아니다. 대외 설명 시 이 구분을 유지한다.
> 실무 검증의 3단계 구분(Tier 1/2/3)과 프레이밍은
> [`docs/design/field-validation.md`](../../docs/design/field-validation.md)를 따른다.

## 사례 목록

| 사례 | 현장 요청 | 규제 맥락 | 정책 팩 | 상태 |
|---|---|---|---|---|
| [`finance_fds/`](finance_fds/) | "이상거래 의심 거래를 뽑아 달라" | 신용정보법 · 전자금융거래법 | `finance_policy.json` | 실행 가능 |
| [`public_sector_stats/`](public_sector_stats/) | "시군구별 수급 실태를 개방하라" | 개인정보 보호법 · 공공데이터법 · 가명정보 가이드라인 | `public_sector_policy.json` | 실행 가능 |
| [`llm_finetune/`](llm_finetune/) | "학습 데이터에서 PII를 걷어내라" | 개인정보 보호법 · AI 학습데이터 정책 | `xazz sanitize` | 실행 가능 |
| 의료 | "환자 코호트별 진단 분포" | 개인정보 보호법 제23조 · 의료법 | `healthcare_policy.json` | [`examples/security/patient_*.xzz`](../security/) |
| 반도체/제조 | "장비 드리프트를 잡아라" | (사내 품질 기준) | — | [`examples/semiconductor/`](../semiconductor/) |

## 실행

각 사례 디렉터리에서 실행한다 (`load()` 와 `xazz.policy.json` 이 디렉터리 기준):

```bash
cd examples/field_cases/finance_fds
python3 generate_txns.py
xazz policy fds_safe.xzz
xazz run    fds_safe.xzz
```

## 왜 위반 예제를 두는가

가드레일은 "무엇을 막는지"를 보여줄 때 가장 설득력이 있다. 위반 예제는 실행 전에
차단되고(`exit 1`), `xazz policy <file> --fix` 가 안전한 대체 코드를 제안한다.
차단 리포트의 각 위반은 법조항(`source_ref`)까지 추적되어 감사 로그에 체인된다.

## 한국어 요약

현장 요청을 Xazz로 재현한 사례 모음이다. 모든 데이터는 합성이며 실제 기관 데이터가
아니다(PoC/데모). 각 사례는 위반 예제와 안전 예제를 한 쌍으로 제공해, 규제가 왜
필요하고 Xazz가 그것을 어떻게 파이프라인 안에서 처리하는지 보여준다.
