# 실무 검증 전략 — 3단계 증거 모델

상태: 문서 v1 · 관련: [`examples/field_cases/`](../../examples/field_cases/), [`regulatory-mapping.md`](regulatory-mapping.md), [`submission_visuals_plan.md`](../submission_visuals_plan.md)

> 이 문서는 "Xazz가 실무에서 쓰인다"는 주장을 **어떤 증거로, 어디까지** 뒷받침할지
> 정한다. 검증되지 않은 채택을 주장하지 않는다 — 심사·리뷰에서 검증 가능하기
> 때문이다. 각 항목은 **증거의 종류**로 구분한다.

## 원칙

1. **합성/실데이터/채택을 섞지 않는다.** 합성 데이터로 만든 PoC를 "도입 사례"로
   쓰지 않는다. 각 주장 옆에 증거 등급을 표기한다.
2. **"대기 중"은 "성사"가 아니다.** 컨택·제안·시도는 근거(스레드 URL, 메일 날짜)가
   있을 때만 인용한다. 응답이 없으면 "제안함"까지가 정직한 표현이다.
3. **재현 가능성.** 실데이터 사례는 데이터 출처와 생성 스크립트를 함께 둔다.
4. **규제 근거는 감사 추적용이지 컴플라이언스 보증이 아니다** (`regulatory-mapping.md` 면책).

## 3단계 증거 모델

| Tier | 이름 | 증거의 성격 | 검증 가능성 | 대외 표현 |
|---|---|---|---|---|
| **1** | 실데이터 재현 | 공개 실데이터 + 재현 스크립트 | 누구나 재실행 | "실데이터에서 동작" |
| **2** | 도전·대기 | 컨택/제안 로그, 공개 스레드 | 링크로 확인 | "제안·검증 중" |
| **3** | 로드맵 | 계획·설계 문서 | 문서로 확인 | "계획" |

발표·보고서에서는 Tier 1 → 2 → 3 순으로 **신뢰도를 낮춰** 서술한다. Tier 2를 Tier 1처럼
쓰면 리스크, Tier 1을 Tier 3처럼 과소평가하면 손해다.

---

## Tier 1 — 실데이터 재현 (지금 당장 가능)

공개 데이터로 파이프라인을 재현한다. "실제 현장 데이터가 아니라도, 실제 데이터에서
동작한다"는 가장 강한 정직한 증거다.

### 1-A. 서울시 공기질 (이미 사용 중)

- 데이터: `examples/data/seoul_air_*.csv` (서울 열린데이터광장, 2008–2026)
- 재현: `python benches/run_readme_benchmark.py` → pandas 대비 2.04×/3.84×/2.14×
- 증거 등급: **실데이터 · 벤치마크**

### 1-B. 도메인 실데이터 재현 (권장 다음 단계)

| 도메인 | 공개 데이터 출처(예) | Xazz로 보여줄 것 | 산출물 |
|---|---|---|---|
| 공공/가명정보 | 공공데이터포털(data.go.kr) 개방 데이터셋 | 집계 + DP + 정책 게이트 | `examples/field_cases/public_sector_stats/` 를 실제 데이터셋으로 교체 |
| 금융 | 금융위 공시·오픈API (합성 대체 가능) | 성능 + 정책 차단 | `examples/field_cases/finance_fds/` |
| 반도체/제조 | (사내 데이터 대체) 합성 웨이퍼 | SPC + 타입 널 안전 | `examples/semiconductor/` |
| LLM/파인튜닝 | 공개 말뭉치(예: AI Hub) | `xazz sanitize` PII/중복/편향 리포트 | `examples/field_cases/llm_finetune/`, `xazz sanitize <file> --json` |

### 1-C. `xazz sanitize` 실데이터 리포트

- 실제 공개 학습데이터에 PII 스캔을 돌려 구조화 리포트(`--json`)를 남긴다.
- "우리 데이터에서 실제로 N건의 PII가 나왔다"는 재현 가능한 증거가 된다.

### Tier 1 체크리스트

- [ ] 데이터 출처(URL·라이선스·수집일)를 사례 README에 명시
- [ ] 생성/다운로드 스크립트를 커밋 (`generate_*.py` 또는 `fetch_*.py`)
- [ ] `xazz check`/`xazz policy`/`xazz run` 출력을 캡처
- [ ] 캡션에 "합성" vs "실데이터"를 명시

### 원커맨드 재현

사례 전체를 한 번에 재현·검증한다(각 단계의 기대 exit code까지 확인):

```bash
python3 examples/field_cases/run_all.py            # target/release 또는 PATH의 xazz 사용
python3 scripts/make_pitch_deck.py                 # 발표덱(docs/demo/pitch-deck.html) 재생성
```

---

## Tier 2 — 도전·대기 (정직하게)

"이런 곳에서도 시도해 봤고, 지금 검증을 기다린다"를 **근거와 함께** 남긴다. 대회
평가에서 트랙션은 가점이지만, 검증 불가한 주장은 감점이다.

### 허용되는 형태 (근거 필수)

| 형태 | 증거 | 표현 |
|---|---|---|
| 오픈소스 이슈/PR | 저장소 이슈 URL | "제안함 / 응답 대기" |
| 관련 프로젝트 연동 시도 | PR·이슈 링크 | "연동 시도 중" |
| 기관·기업 PoC 제안 | 메일 날짜·제안서 사본 | "PoC 제안함" |
| 공개 밋업/컨퍼런스 | 발표 자료·영상 | "소개함" |
| 규제/표준 대응표 | `regulatory-mapping.md` 조항 매핑 | "감사 근거로 매핑" |

### 컨택 로그 (커밋하지 않는 것이 원칙)

개인·기관과의 컨택은 **비공개 로그**로 관리하고, 대외 자료에는 **익명화한 요약**만
넣는다. 템플릿은 [`field-contact-templates.md`](field-contact-templates.md).

| 날짜 | 대상(익명) | 채널 | 내용 | 상태 | 근거 |
|---|---|---|---|---|---|
| YYYY-MM-DD | 금융권 데이터팀 | 이메일 | FDS PoC 제안 | 제안함 | 메일(비공개) |
| YYYY-MM-DD | OSS 프로젝트 X | GitHub | 연동 이슈 #N | 대기 | URL |

### Tier 2 체크리스트

- [ ] 모든 "시도"에 날짜·채널·근거(URL 또는 사본)가 있다
- [ ] "성사/도입 확정"으로 읽힐 표현이 없다
- [ ] 상대방 실명·기관명은 공개 동의 없이 노출하지 않는다
- [ ] 발표 슬라이드에서 Tier 2는 별도 배지(`제안·대기`)로 Tier 1과 분리

---

## Tier 3 — 로드맵 (미래)

- EU AI Act / ISO 42001 정책 팩 (`regulatory-mapping.md` 백로그)
- burn-engine LoRA/QLoRA 연동 (Track F4)
- Postgres TLS + 테넌트 스코프 토큰 (기술 하드닝)
- 분산 학습·GPU 백엔드 실기 확장

표현: "계획" / "설계 완료, 구현 예정".

---

## 도메인 타겟 매트릭스

| 도메인 | 왜 Xazz인가 | 있는 자산 | 다음 액션 |
|---|---|---|---|
| 금융 FDS/신용 | 개인신용정보 행 단위 반출 금지 + 고빈도 배치 | `finance_policy.json`, `field_cases/finance_fds` | 실데이터 교체, PoC 제안 |
| 의료/바이오 | 민감정보(제23조) + 재식별 | `healthcare_policy.json`, `security/patient_*` | 코호트 집계 사례 추가 |
| 공공/가명정보 | 개방=식별불가, 소집단 재식별 | `public_sector_policy.json`, `field_cases/public_sector_stats` | 공공데이터포털 실데이터 |
| 반도체/제조 | 계측 데이터 품질 게이트 + 타입 널 안전 | `semiconductor/` | 사내 SPC 대체 사례 |
| LLM/파인튜닝 | 학습 데이터 PII 정화, 입출력 게이트 | `xazz sanitize`, Track F1/F2 | 실 말뭉치 리포트 |
| MLOps | 파이프라인 PR 게이트 | GitHub Actions action, `/catalog` 계보 | CI 통합 사례 |

---

## 대외 표현 가이드 (한 줄 요약)

- Tier 1: "**공개 실데이터에서 재현**했습니다 (스크립트 포함)."
- Tier 2: "**PoC를 제안**했고 **검증을 기다립니다** (근거: 링크/날짜)."
- Tier 3: "**로드맵에 있습니다**."

## 한국어 요약

Xazz의 실무 적용 주장을 증거 등급으로 나눈다. Tier 1(공개 실데이터 재현)은 지금
가능하고 가장 강하다. Tier 2(도전·대기)는 컨택·제안 로그와 링크가 있을 때만
인용한다. Tier 3는 로드맵이다. 합성 PoC를 도입 사례로 포장하지 않는 것이 이 문서의
핵심 규칙이다.
