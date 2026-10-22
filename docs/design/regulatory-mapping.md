# 규제 매핑 — 우선순위 · 커버리지 · 신뢰도

상태: 문서 v1 (issue #310) · 관련: [`xazz-compiler/src/policy/mod.rs`](../../xazz-compiler/src/policy/mod.rs) `default_source_ref`, [`docs/SECURITY_GUARDRAIL.md`](../SECURITY_GUARDRAIL.md)

> ⚠️ **법률 자문이 아니다.** 아래 매핑은 각 규칙이 어느 규범에 근거했는지 감사
> 추적(audit trail)하기 위한 참조다. 실제 적용 법령은 조직·도메인·관할에 따라
> 다르므로 정책 파일의 `rule_source_refs`로 덮어쓴다. 이 문서와 정책 팩은
> 컴플라이언스를 보증하지 않는다.

## 프레임워크 우선순위

정책 팩은 다음 순서로 규범을 정렬한다. 위에 있을수록 먼저 매핑하고, 아래는
필요할 때만 보강한다.

| 순위 | 프레임워크 | 적용 범위 | 현재 상태 |
|------|-----------|-----------|-----------|
| 1 | **국내법** — 개인정보 보호법, 가명정보 처리 가이드라인, 개인정보의 안전성 확보조치 기준 | PII·민감정보·재식별·안전조치 규칙의 기본 참조 | 매핑됨(기본 정책 팩) |
| 2 | **NIST AI RMF** (GEN-4.3) | GenAI 입력/출력 게이트, 모델 출처·라이선스 | 부분 매핑 |
| 3 | **EU AI Act (GPAI)** | 해외 배포 시 투명성·문서화 의무 | 미매핑(백로그) |
| 4 | **ISO/IEC 42001** | 조직 AI 관리체계 | 미매핑(백로그) |

국내법을 우선하는 이유: 현재 정책 팩(healthcare/finance/public-sector)이 국내
도메인을 대상으로 하고, 조항 대응을 국내 전문가가 검증할 수 있기 때문이다.
EU AI Act·ISO는 해외 배포·기업 도입 시 별도 정책 팩으로 추가한다.

## 커버리지 · 신뢰도 정의

각 매핑은 두 축으로 표기한다.

- **coverage** — 조항이 규칙의 판단을 얼마나 포괄하는가
  - `mapped`: 규칙 전체를 직접 규율하는 조항
  - `partial`: 규칙의 일부 측면만 규율(해석·보강 필요)
- **confidence** — 조항↔규칙 대응의 확실성
  - `high`: 조항 문언이 규칙과 직접 대응
  - `medium`: 해석의 여지가 있음
  - `low`: 방향성 참조(정량 기준 아님)

`low`/`partial` 항목은 감사 근거로는 유효하나, 컴플라이언스 주장의 근거로 쓰지
않는다. 검증(아래 절차)을 통과하면 confidence를 올린다.

## 규칙별 매핑 (기본값)

| rule_id | 근거(source_ref) | coverage | confidence |
|---------|------------------|----------|------------|
| `XZP001` DIRECT_IDENTIFIER_EXPOSED | 개인정보 보호법 제24조(고유식별정보) · 제3조(최소수집) | mapped | high |
| `XZP002` SENSITIVE_ROW_LEVEL | 개인정보 보호법 제23조(민감정보 처리 제한) | mapped | high |
| `XZP003` QUASI_COMBINATION | 개인정보 보호법 제28조의5 · 가명정보 처리 가이드라인(재식별 위험) | partial | medium |
| `XZP004` AGGREGATE_WITHOUT_DP | 가명정보 처리 가이드라인 — 통계값 공개 시 재식별 완화 | partial | medium |
| `XZP005` EPSILON_TOO_LARGE | 가명정보 처리 가이드라인 — 프라이버시 보호 강도 | partial | low |
| `XZP010` PII_LITERAL | 개인정보 보호법 제29조(안전조치) · 안전성 확보조치 기준 | mapped | high |
| `XZP011` HARDCODED_SECRET | 안전성 확보조치 기준 제6조·제7조 · 국가정보원 보안 기준 | mapped | high |
| `XZP012` SENSITIVE_PATH | 안전성 확보조치 기준 제6조(접근 통제) | partial | medium |
| `XZP013` PATH_TRAVERSAL | 안전성 확보조치 기준 제6조(접근 통제) | mapped | high |
| `XZP014` UNRESOLVED_SCHEMA | 내부 통제 기준 — 검증되지 않은 처리 계획 실행 금지(fail-closed) | mapped | high |
| `XZP000` PARSE_FAILED | 내부 통제 기준(fail-closed) | mapped | high |
| `XZP999` POLICY_LOAD_FAILED | 내부 통제 기준(fail-closed) | mapped | high |
| `XZP020` PROMPT_INJECTION | NIST AI RMF(GEN-4.3) · 국가정보원 생성형 AI 보안 가이드라인 | partial | medium |
| `XZP021` PROMPT_JAILBREAK | NIST AI RMF(GEN-4.3) · 국가정보원 생성형 AI 보안 가이드라인 | partial | medium |
| `XZP022` PROMPT_EXFILTRATION | NIST AI RMF(GEN-4.3) · 국가정보원 생성형 AI 보안 가이드라인 | partial | medium |
| `XZP030` MODEL_LICENSE_BLOCKED | NIST AI RMF(GEN-4.3) · AI 학습데이터 정책(출처·라이선스) | partial | medium |
| `XZP031` MODEL_PROVENANCE_UNKNOWN | NIST AI RMF(GEN-4.3) · AI 학습데이터 정책(출처·라이선스) | partial | medium |

정책 팩은 `rule_source_refs`로 위 기본값을 덮어쓸 수 있다(예:
`examples/security/*_policy.json`). 덮어쓴 참조도 같은 coverage/confidence
기준으로 표기한다.

## 검증 절차

1. **원문 대조** — 조항 원문과 규칙의 판단 조건을 나란히 두고 대응을 확인한다.
2. **도메인 리뷰** — 해당 분야 전문가(법무/컴플라이언스)가 coverage/confidence를
   승인한다. 언어 리뷰만으로는 불충분하다.
3. **회귀 고정** — 정책 팩 테스트(`xazz policy`)가 위반 `source_ref`를 그대로
   내보내는지 확인한다.
4. **메타데이터 갱신** — 승인된 coverage/confidence와 검증일·검증자를 기록한다.
   근거가 바뀌면 `default_source_ref`와 이 표를 같은 PR에서 함께 고친다.

## 면책 문구가 노출되는 표면

- 감사 증거: 위반 리포트의 `source_ref`(감사 로그에 체인됨).
- CLI: `xazz policy` 리포트.
- 문서: 이 문서, [SECURITY_GUARDRAIL.md](../SECURITY_GUARDRAIL.md),
  `README.md` 보안 절.
- IDE: 정책 위반 패널.

면책은 "법률 자문이 아님"을 명시하며, 검증되지 않은 매핑은 컴플라이언스 주장의
근거로 표기하지 않는다.
