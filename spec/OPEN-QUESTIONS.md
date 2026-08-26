# Xazz UI/UX 승인 질문

- 상태: 2026-07-27 사용자 승인으로 종결
- 승인 원문: `승인. uiux 구현 진행`
- 2026-07-27 독립 코드 감사에서 process exit와 pipeline success가 다를 수 있음이
  확인됐다. 아래 추천안은 이 신뢰 경계를 반영했으며 Q1의 Core/Future 분리를 더 중요하게 만든다.

## Q1. 구현 현실과 미래 비전의 경계

현재 Core와 아직 구현되지 않은 Guard/DP/Burn/sLM을 Figma에서 어떻게 나눌까?

- **A — Core는 실제 구현 기준, 미래 기능은 별도 Future/Labs flow (승인)**
- B — 대회 최종 비전을 하나의 완성 제품처럼 통합
- C — 현재 구현된 Core만 디자인하고 미래 기능은 제외

추천 이유: 제품 비전은 살리면서도 demo를 실제 기능처럼 오인시키지 않는다.

## Q2. Primary persona

- **A — Python 데이터/ML 개발자 (승인)**
- B — 데이터 입문 학생
- C — 보안·컴플라이언스 검토자

추천 이유: 현재 구현과 기존 IDE 자산이 가장 직접적으로 가치를 줄 수 있고, reviewer UX는
receipt와 approval surface로 보조할 수 있다.

## Q3. Landing의 첫 CTA

- **A — 설치 없이 sample pipeline 열기 (승인)**
- B — 자신의 CSV 가져오기
- C — CLI 설치/다운로드

추천 이유: 현재 release tag와 링크 상태에 의존하지 않고 가장 빠르게 제품 가치를 증명한다.

## Q4. UI 언어

- **A — 영어 primary, 핵심 Korean localization state도 함께 검증 (승인)**
- B — 한국어 primary
- C — 모든 frame을 영어/한국어로 중복 제작

추천 이유: 글로벌 오픈소스 방향과 기존 i18n을 살리면서도 한국어 길이·가독성을 검증할 수
있다.

## Q5. 이번 Figma 범위

- **A — landing + project start + workspace + preflight + run/error/receipt 8-state prototype (승인)**
- B — landing과 workspace 기본 화면만
- C — 위 범위에 monitoring dashboard까지 포함

추천 이유: 처음 방문부터 효능감·복구·신뢰까지 한 번에 검증하면서 dashboard로 범위가
퍼지는 것을 막는다.

## Q6. 미감

- **A — 밝은 landing + 차분한 dark workspace, non-cyberpunk (승인)**
- B — 전체 light
- C — 전체 dark

추천 이유: landing의 접근성과 명료함, 개발 도구의 집중 환경을 각각 살리고 기존 dark
interaction asset도 합리적으로 이어받는다.

## 승인과 함께 적용하는 가역적 기본값

별도 이견이 없으면 다음은 가정으로 적용한다.

- IDE desktop 1440 기준, landing만 390 mobile 포함
- 실제 data 대신 synthetic air-quality sample
- Figma 새 Design file을 연결된 단일 plan의 Drafts에 생성
- production code 변경·배포 없음
- Figma 전에 `design-system/MASTER.md`와 local React/CSS prototype을 만든 뒤 왕복
