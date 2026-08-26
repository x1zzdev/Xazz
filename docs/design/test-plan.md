# Xazz 프로토타입 테스트 계획

## 상태와 목적

아직 사용자 테스트를 수행하지 않았다. 이 문서는 승인된 Figma prototype을 검증하기 위한
사전 계획이며, 통과율·시간·인용문을 실제 결과처럼 기록하지 않는다.

검증 질문:

1. 방문자가 stack 설명 없이 Xazz의 가치를 이해하는가?
2. 설치·인증 없이 sample result에서 첫 가치를 얻는가?
3. Compiler Canvas에서 graph·code·data impact를 함께 추적하는가?
4. Live Check, Full Run, process exit, pipeline verdict를 구분하는가?
5. 실패 후 자동 적용 없이 안전한 복구 행동을 찾는가?

## 참여자와 방식

- 목표 참여자: Python 데이터/ML 개발자 5명; 가능하면 pipeline reviewer 경험자를 포함
- 형식: 30분 내외 moderated task-based clickable-prototype test
- 데이터: synthetic air-quality sample만 사용
- 언어: English primary, 최소 1회 Korean localization 길이/가독성 확인
- 기록: 동의 후 화면·음성 또는 관찰 노트; 개인·의료·금융 데이터 수집 금지
- moderator는 막혔을 때 정답을 말하지 않고 행동·발화·시간·도움 횟수만 기록

## 과업

| ID | 과업 | 관찰할 감정/행동 | 성공 기준 | 대표 실패와 probe |
|---|---|---|---|---|
| T1 | Hero를 10초 보고 제품 설명 | 명료함 또는 혼란 | 4/5가 “학습 전 typed data pipeline 문제와 변화를 확인”이라고 설명 | stack만 반복하면 “어떤 비용/실수를 줄이나?” |
| T2 | `Open a sample pipeline`에서 첫 결과 찾기 | 즉시성, 효능감 | 4/5가 90초·3결정 이내 table/chart 도달 | CTA·빈 canvas·용어에서 정지한 지점 기록 |
| T3 | filter node가 데이터에 준 영향 찾기 | 이해, 통제 | graph, `.xzz` line, upstream/downstream, row/null/type delta를 모두 지목 | inspector만 보거나 code만 보면 기대 mapping 질문 |
| T4 | 조건 변경 후 Live Check 수행 | 안전한 탐색 | 100-row cap·no side effects·stale downstream을 설명 | Full Run으로 오인한 단서 기록 |
| T5 | preflight의 실행 범위와 증거 판단 | 신뢰, 책임감 | 세 binary가 `Future contract · not verified`임을 설명하고 Local, `Not evaluated`, `Control · Not configured`, `Artifact · Not requested`, 별도 run confirmation을 구분 | synthetic readiness를 실제 Ready로 오인하거나 Control과 confirmation을 합치는지 확인 |
| T6 | Full Run 상태와 receipt 해석 | 확신, 성취 | process와 verdict를 구분하고 result/artifact/warning/hash maturity를 찾음 | `Exited = Succeeded`로 오인하는지 확인 |
| T7 | runtime/artifact failure에서 복구 | 회복 가능성 | 4/5가 45초 내 what/where/affected/safe next step을 찾음 | 자동 fix 기대, partial retry/Future 오인 기록 |
| T8 | maturity map 해석 | 정직성 | 5/5가 Available과 Beta/Demo/Research/Planned를 구분 | Burn/Guard를 현재 기능으로 오인하는 문구 수집 |

## Frontstage와 backstage 확인

- Frontstage: CTA hierarchy, node/code 동기화, result dock, readiness, status 축, error action.
- Backstage prototype: sample-ready, live-check, preflight, running, success/receipt,
  failed/partial 정적 state가 링크와 명칭대로 전환되는지 확인. 별도 `Blocked`
  acceptance는 실제 runtime readiness 계약이 생긴 뒤 검증한다.
- 구현되지 않은 backend 동작은 시뮬레이션이라고 facilitator note에 명시한다.
- policy approval, partial retry, restore, cancel/resume은 Future label이 빠지면 실패다.

## 정량·정성 기록

| 항목 | 기록 단위 |
|---|---|
| Task success | 독립 성공 / 도움 후 성공 / 실패 |
| Time | T2 90초, T7 45초 기준 |
| Decisions | T2의 의미 있는 선택 수, 목표 3 이하 |
| Misclassification | process/verdict, location, maturity 오인 횟수 |
| Recovery | 선택한 action과 자동 적용 기대 여부 |
| Confidence | 각 과업 후 1–5 자기평가; 성과 주장 대신 진단용 |
| Quote | 문제를 설명하는 짧은 발화와 맥락 |

## 접근성·시각 검수

- keyboard-only: sample open → run → result → receipt.
- visible focus, skip link, native controls, graph의 keyboard-selectable list.
- normal text `>= 4.5:1`, boundary/icon `>= 3:1`, status non-color-only.
- 1440 desktop과 390 mobile landing overflow/clipping 0건.
- chart title/unit/series/sample scope/table alternative 확인.
- 외부 font/image/chart/icon CDN 요청 0건.
- reduced-motion에서 비필수 motion 제거.

## 판정과 후속

- Must requirement 하나라도 실패하면 관련 frame/copy/component를 수정하고 새 prototype
  version에서 해당 과업만 재검증한다.
- 관찰과 해석을 분리하고, 결과 보고서에는 참여자 수·성공/실패·시간·도움·미해결 위험을
  그대로 남긴다.
- Compiler Canvas 자체가 T2/T3에서 반복적으로 막히면 Notebook Ledger의 inline-result
  concept을 비교하되, 단순 선호 투표가 아니라 과업 성공과 trace 정확도로 판단한다.
