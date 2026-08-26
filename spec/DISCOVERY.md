# Xazz UI/UX Discovery

- 조사일: 2026-07-27
- 대상 커밋: `bdf83db181dd4ea1f751191bc87a44e37e5a8016`
- 범위: Xazz 본 저장소, 기존 x1zzLang Visual IDE, x1zzGuard Policy Gate 자산, Second Brain UI/UX 품질 게이트
- 상태: 설계 입력용 읽기/문서 감사

## 1. 한 문장 결론

Xazz의 가장 설득력 있는 제품 경험은 “기능이 많은 AI IDE”가 아니라 **데이터를 불러온 뒤
변환 결과를 즉시 확인하고, 실행 전 위험을 이해하며, 실행 후 근거를 남기는 typed pipeline
workbench**다.

현재 핵심 구현은 `.xzz` Lexer/Parser/AST, Polars 로컬 실행기, Rust 소스 emitter다.
Visual IDE와 Policy Gate 데모는 재사용 가치가 있지만 서로 분리돼 있고, README가 설명하는
Burn 학습·Policy-as-Code·DP·sLM·영구 감사 로그는 현재 사용자에게 완료 기능처럼
노출해서는 안 된다.

## 2. 저장소 기준 현재 상태

### 실제 사용자 가치로 연결 가능한 코어

| 기능 | 근거 | UX 해석 |
|---|---|---|
| 새 프로젝트 | `src/project.rs:28-92` | 샘플 CSV와 실행 가능한 `example.xzz`를 제공하므로 현재 가장 안전한 첫 성공 경로 |
| CSV import | `src/schema.rs:116-285` | 100행 표본 기반 타입·nullable 추론. 인코딩, 표본 범위, 컬럼 매핑을 사용자가 검토해야 함 |
| `.xzz` 실행 | `xazz-exec/src/runtime.rs:145-353` | Polars 연산, 행×열, 표·JSON·CSV 산출물을 UI에 구조화할 수 있음 |
| 차트 | `xazz-exec/src/runtime.rs:798-840` | bar/line/pie/scatter 계약은 있으나 대용량 표시 정책이 필요 |
| Rust emit | `xazz-compiler/src/emitter.rs:28-479` | 데이터 파이프라인의 변환 근거를 보여주는 고급 기능으로 적합 |
| SHA-256 | `xazz-server/src/main.rs:338-382` | 코드 hash 생성·검증 기능. “영구 감사 로그”가 아니라 현재 범위 그대로 표기해야 함 |

### UI에서 성숙도를 분리해야 하는 기능

| 기능 | 현재 근거 | 권장 표기 |
|---|---|---|
| `run`, `new`, `run --output` | 실행 경로 존재 | Available |
| `import` | 100행 표본, 인코딩·헤더 제약 | Beta · Review required |
| `emit rust` | Polars 소스 생성 | Available · Data pipeline |
| `check` | `src/main.rs:150-167`, `src/ux.rs:19-122`의 고정 mock | Demo |
| `run --predict` | 저장소에 없는 Python entry와 절대 checkpoint 경로 의존 | Unavailable |
| `sde` | 안내문만 출력 | Planned |
| Burn 학습 | 문법·marker만 있고 runtime이 미구현을 출력 | Planned |
| Policy/DP/sLM | 구현 경로가 확인되지 않음 | Research |
| Audit log | hash 응답만 있고 저장 없음 | Hash verification only |

성숙도는 실행 상태와 다른 축이다. 예를 들어 `Demo / Running` 또는
`Available / Failed`처럼 함께 표현할 수 있어야 한다.

## 3. 가장 큰 capability-truth gap

1. README의 “컴파일 단계 Null 안전성”과 달리 현재 `xazz-exec` 경로는 schema cast 뒤
   non-Option null을 경고로 출력한다 (`xazz-exec/src/runtime.rs:416-510`).
2. `xazz check`는 파일을 읽거나 분석하지 않고 고정 `SUCCESS`, `98.2%`를 출력한다
   (`src/main.rs:150-167`, `src/ux.rs:31-40`).
3. Burn 실행은 placeholder이며 `xazz-exec`에 Burn 의존성도 없다
   (`docs/ARCHITECTURE.md:208-210`, `xazz-exec/Cargo.toml`).
4. Policy-as-Code, DP, 온프레미스 sLM, data-flow sandbox는 현재 저장소에서 제품 기능으로
   확인되지 않는다.
5. `/security/audit`와 `/security/verify`는 hash 계산·비교일 뿐 영구 이력 저장이 아니다.
6. README의 Releases·Visual IDE 링크는 `xazzdev` 소유자를 가리키며 실제 저장소 소유자
   `x1zzdev`와 다르다 (`README_kr.md:22,174-177,202`). 원격 tag도 0개라 다운로드 CTA를
   현재 랜딩의 주 CTA로 쓰기 어렵다.

디자인은 위 간극을 숨기는 장식이 아니라 **Available / Beta / Demo / Research / Planned를
일관되게 보여주는 truth layer**를 포함해야 한다.

## 4. 기존 UI/UX 자산

### React Visual IDE — 상호작용 로직을 재사용

경로: `../x1zzLang-visual-ide-openssl/frontend`

재사용 우선순위:

1. `Canvas.jsx`, `CustomNode.jsx`, DAG walker와 `.xzz` transpiler
2. `ConfigWindow.jsx`의 파일·스키마·연산 설정
3. `ResultsWindow.jsx`의 표·로그·오류 카드·내장 SVG 차트
4. 한국어/영어 i18n 사전
5. 코드/그래프 split view

시각 시스템은 그대로 채택하지 않는다. 현재 화면은 52px 툴바에 기능이 과밀하고, 좌우
inspector와 코드 패널이 중앙 canvas를 압축한다. 제품명도 여전히 `x1zzLang Visual IDE`다.

### x1zzGuard Policy Gate — 신뢰 흐름을 재사용

경로: `../tmp/team_share_260707/gate/index.html`

재사용할 패턴:

- Policy version badge
- PASS / REJECT verdict
- 위반 rule, 근거, 안전 대안
- pipeline의 bad operation 강조
- plan/policy hash receipt

단, 기존 데모는 PASS 뒤 “인간 승인 단계로 전달 가능”까지만 있고 실제 승인·동결·실행·
receipt export가 없다. 새 흐름은 이 끊긴 부분을 완성해야 한다.

### CLI NQP report — 정보 순서만 참고

대상 → 분석 상태 → pipeline delta → insight → 완료 순서는 유용하다. 수치와 모델명은
고정 mock이므로 실제 evidence UI로 승격하지 않는다.

### Chart / benchmark report

- chart의 데이터 계약은 쓸 수 있지만 현재 `result_file_input_chart.html`은 5,631행을
  그대로 bar로 직렬화하고 제목·legend label도 비어 있어 해석이 어렵다.
- benchmark report의 “요약 → 비교 → 원시 telemetry” 점진적 공개 구조는 run receipt에
  재사용할 가치가 있다.

## 5. 확인된 UX 위험

### 실행 안전성

현재 Visual IDE의 Run은 transpile 직후 `/execute`로 POST하고
(`frontend/src/App.jsx:621-719`), Auto-Run은 DAG 변경 400ms 뒤 같은 full execution을
호출한다 (`frontend/src/App.jsx:723-737`).

이는 팀이 설명한 `검증 → 위반 시 차단·수정안 → 인간 승인 → 실행 → 증적`과 충돌한다.
새 UX는 다음 두 동작을 분리해야 한다.

- **Live Check**: capped sample, side-effect 없음, 자동 가능
- **Full Run**: 명시적 클릭, 환경·preflight·승인 상태 확인 뒤 실행

### 상태 모델

기존 node 상태는 `idle/running/success/error`뿐이다. 다음 상태가 별도로 필요하다.

- 작성/실행: Draft, Validating, Ready, Running, Succeeded, Failed, Cancelled
- 통제: Not configured, Needs review, Rejected, Approved, Frozen
- 성숙도: Available, Beta, Demo, Research, Planned

### 접근성

- `#52525b` on `#18181b` 대비는 직접 계산 결과 `2.29:1`이며 작은 muted text에 널리 쓰인다.
- 전역 button outline 제거 뒤 `:focus-visible`이 없다.
- clickable `div`, mouse-hover 전용 chart tooltip이 있다.
- 모바일 대응은 landing에 한정하고, IDE는 desktop 최소 폭을 명시하는 편이 현실적이다.

### 차트

- 수천 category를 그대로 그리지 않는다.
- title, unit, label, aggregation, sample/cap 정보를 항상 표시한다.
- category가 임계치를 넘으면 top-N, 집계, table 전환 중 하나를 제안한다.
- status와 series를 색만으로 구분하지 않는다.

### 폐쇄망

현재 생성 chart의 Chart.js CDN과 Visual IDE의 Google Fonts 요청은 “외부 유출 없음 /
폐쇄망” 서사와 충돌한다. 프로토타입과 향후 구현은 local font·bundled asset을 기본으로
설계한다.

## 6. 외부 제품에서 확인한 패턴

한 제품을 복제하지 않고 여정별 강점만 사용한다.

- [Hex 첫 분석](https://learn.hex.tech/docs/getting-started/develop-your-notebook):
  데이터 선택 후 100행 query를 자동 생성·실행하는 빠른 첫 결과.
- [Hex Graph view](https://learn.hex.tech/docs/explore-data/projects/project-execution/graph-view):
  graph와 logic의 양방향 반영, 선택 node의 upstream/downstream과 code 동시 강조.
- [Dagster Runs](https://docs.dagster.io/guides/operate/webserver):
  timing, error, structured/raw log, 동일 설정 re-execution을 한 run detail에 결합.
- [Positron Data Explorer](https://positron.posit.co/data-explorer.html):
  grid, summary, ephemeral filter를 code-first flow에 보조적으로 제공.
- [Positron error actions](https://positron.posit.co/managing-interpreters.html):
  Fix와 Explain을 분리하고 session 상태를 명시.
- [Polars remote query](https://docs.pola.rs/polars-cloud/run/remote-query/):
  기존 query와 remote 실행 사이의 작은 전환 비용.
- [Polars query profiler](https://docs.pola.rs/polars-cloud/run/query-profile/):
  timeline → logical plan → physical plan → row flow와 CPU/I/O 병목의 점진적 drill-down.

## 7. 검증 receipt

| finding | test | result | 설계 반영 |
|---|---|---|---|
| Visual IDE가 검증 없이 실행 | `App.jsx:610-755` 직접 확인 | CONFIRMED | Live Check와 Full Run 분리 |
| Auto-Run이 400ms 뒤 full execute | 같은 코드 경로 직접 확인 | CONFIRMED | 자동 동작은 capped sample만 허용 |
| Policy Gate가 승인 이후를 구현하지 않음 | `gate/index.html:530-600` 직접 확인 | CONFIRMED | Approve → Freeze → Run → Receipt 상태 추가 |
| Xazz와 core screenshot이 동일 | SHA-256 비교 | CONFIRMED · 두 쌍 모두 동일 | 현재 이미지는 baseline이지 새 브랜드 자산이 아님 |
| muted text 대비 부족 | WCAG 상대휘도 계산 | CONFIRMED · `2.29:1` | 새 token에서 AA 대비 확보 |
| README Visual IDE 링크 | `git ls-remote` | CONFIRMED · 대상 저장소 not found | landing CTA에서 제외 |
| 원격 release tag | `git ls-remote --tags origin` | CONFIRMED · 0개 | 다운로드보다 sample CTA 우선 |
| 현재 CLI runtime 동작 | `cargo run` 시도 | UNTESTABLE · 로컬에 cargo 없음 | source 근거 이상의 완료 주장을 하지 않음 |
| Ploomber partial-run 세부 | 공식 문서 fetch | UNTESTABLE · 502 | core requirement 근거에서 제외 |

## 8. 설계에 바로 적용할 결정 후보

1. 첫 CTA는 다운로드가 아니라 **Open sample pipeline**.
2. sample은 `xazz new`가 생성하는 단순 공기질 파이프라인을 기준으로 한다.
3. graph와 `.xzz` code는 동일 source를 양방향으로 가리킨다.
4. 자동 피드백은 sample preview와 lint까지만, full run은 명시적으로 실행한다.
5. 실패는 toast가 아니라 node·code line·downstream impact·복구 action을 한 표면에 묶는다.
6. 성공은 confetti가 아니라 결과 preview, rows/null/type delta, artifact, hash receipt로
   효능감을 준다.
7. Guard/DP/sLM/Burn은 Core와 섞지 않고 Labs 또는 Future flow로 분리한다.
8. landing은 밝고 간결하게, workspace는 차분한 dark workbench로 구성하되 cyberpunk
   glow와 과도한 card를 피한다.

## 9. 독립 코드 감사 delta — 신뢰 상태 계약

기존 초안을 독립적으로 다시 읽은 결과, 화면 구조보다 먼저 고정해야 할 P0 계약이
확인됐다.

| 확인 사실 | 코드 근거 | UX 제약 |
|---|---|---|
| 실제 실행 사슬은 `xazz → xazz-runner → xazz-exec` 세 바이너리 | `src/main.rs::find_runner`, `xazz-runner/src/main.rs::resolve_exec_binary` | preflight는 세 항목을 따로 확인한다. 현재 README의 “두 바이너리” 안내를 그대로 반복하지 않는다 |
| 개별 pipeline과 CSV/chart artifact 실패가 stderr 경고 뒤 최종 `Ok(())`가 될 수 있음 | `xazz-exec/src/runtime.rs::run_pipeline` | process exit와 pipeline verdict를 분리한다. 현 응답만으로 `Succeeded`를 표시하지 않는다 |
| 서버 `success`는 subprocess exit code만 반영 | `xazz-server/src/main.rs::handle_execute` | runtime error log가 있거나 결과가 비어 있으면 `Unknown/Partial`로 두고 성공 receipt를 만들지 않는다 |
| 기존 IDE는 `[x1zz:chart]`, 현재 runtime은 `[xazz:chart]`를 사용 | `frontend/src/transpiler/stdoutParser.js`, `xazz-exec/src/runtime.rs` | 기존 result parser를 그대로 재사용하지 않고 protocol adapter를 먼저 검증한다 |
| `/schema`는 파일을 저장하지만 encoding, nullable, 실제 표본 수와 보존 기한을 반환하지 않음 | `xazz-server/src/main.rs::handle_schema` | 누락값을 추정하지 않는다. current API는 `Not reported`, 미래 계약은 synthetic/future로 구분한다 |
| IDE 언어 저장 key가 읽기/쓰기에서 다름 | `frontend/src/i18n.js`, `frontend/src/components/ToolPalette.jsx` | locale persistence는 재사용 전 수정·회귀 검증 대상이다 |

따라서 현재 가장 정직한 first-value 기준선은 self-contained scaffold다.

```text
xazz new my-project
→ cd my-project
→ xazz run example.xzz
→ Top 5 + [xazz:result] JSON 확인
```

웹의 `Open sample pipeline`은 이 기준선을 더 짧게 체험시키는 **승인 전 제품 가설**이지,
현재 production 경로가 아니다. Figma에서는 구현된 Core와 synthetic prototype,
Future/Labs 상태를 한 화면에서도 명확히 구분한다.
