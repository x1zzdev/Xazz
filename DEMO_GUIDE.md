# Xazz — 시연 영상 가이드라인 (3분 이내)

> 대상: 오픈소스 개발자대회 제출용 시연 영상
> 목표: "겉은 스크립트, 핵심은 컴파일러" — 파서/타입검사/코드생성/딥러닝 컴파일/DP/감사로그가
> 하나의 `.xzz` DSL에서 엔드투엔드로 도는 모습을 **3분 안에** 보여주기.
>
> 본 가이드에 사용하는 예제는 모두 **실제로 실행 검증 완료**된 파일이다.
> (반드시 아래 명령으로 미리 한 번씩 실행해 두고 촬영할 것)

---

## 0. ⚠️ 읽기 전 필수 주의사항 (실패 방지)

1. **데이터 파일 주의**: `examples/data/*.csv` 는 Git LFS **포인터 파일**(빈 더미)이라 실행이 실패한다.
   → 반드시 실제 데이터 **`visual-ide/data/seoul_air_quality.csv`**(20행, 컬럼: `observed_at, district, pm25, temperature_c`)를 사용한다.
2. **스키마는 CSV 전체 컬럼과 일치**해야 한다. `:: Air` 타입 블록에 4개 컬럼을 전부 선언해야
   `duplicate column` 런타임 에러가 나지 않는다.
3. **`xazz`와 `xazz-runner`는 같은 디렉토리**에 있어야 실행된다. (둘 다 `target/release/`에 있음)
4. **예약어 주의**: 변수명으로 `chart`, `model`, `raw` 등은 피할 것.
5. **정적 가드레일 / sLM 자동보정은 현재 구현되어 있다** (`xazz policy` / 실행 전 게이트, `xazz-server` sLM 훅은 `XAZZ_SLM_ENABLED` 로 opt-in).
   데모 주제로 삼으면 좋은 축은 **정적 가드레일 차단 → `xazz policy --fix` 보정 → DP 노이즈 → SHA-256 감사 로그** 순이다.
   (sLM 자동보정을 보여주려면 별도 Ollama 스택 준비 필요 — 본 가이드 범위 밖)

---

## 1. 사전 준비 (촬영 전에 미리 해두기)

### 1-1. 빌드 확인 (이미 빌드돼 있으면 스킵)
```bash
cd /home/x1zz/Xazz
cargo build --release -p xazz
cargo build --release -p xazz-runner
cargo build --release -p xazz-server
# 생성물: target/release/{xazz, xazz-runner, xazz-server}
```

### 1-2. 시연 예제 준비 (본 리포 `demo/` 폴더에 포함됨)
| 파일 | 내용 | 실행 시간 |
|---|---|---|
| `demo/preprocess_chart.xzz` | Polars 전처리 + bar 차트 | <1초 |
| `demo/deep_learning.xzz` | Burn 딥러닝 학습(5 epoch) | ~1초 |
| `demo/dp.xzz` | 차등 프라이버시(Laplace) 노이즈 | <1초 |

리허설로 한 번씩 통과 확인:
```bash
./target/release/xazz run demo/preprocess_chart.xzz
./target/release/xazz run demo/deep_learning.xzz
./target/release/xazz run demo/dp.xzz
```

### 1-3. IDE 프론트엔드 의존성 (한 번만)
```bash
cd /home/x1zz/Xazz/visual-ide && npm install
```

---

## 2. 켜야 하는 서버 / 프로세스와 접속 주소

| 창 | 프로세스 | 명령 | 주소 |
|---|---|---|---|
| 터미널 A | **백엔드 서버** `xazz-server` | `./target/release/xazz-server` | `http://127.0.0.1:8005` |
| 터미널 B | **Visual IDE** (Vite dev) | `VITE_API_BASE_URL=http://127.0.0.1:8005 npm run dev` | `http://127.0.0.1:5173` |

- `xazz-server` 가 `xazz` 바이너리를 찾지 못하면 환경변수로 고정:
  ```bash
  XAZZ_EXEC_PATH=/home/x1zz/Xazz/target/release/xazz ./target/release/xazz-server
  ```
- 백엔드 정상 확인: 브라우저/curl로 `http://127.0.0.1:8005/health` → `{"status":"ok"}`

> `xazz-server`가 IDE를 함께 서빙하는 "same-origin" 모드는 **릴리즈 패키지에만** 동작한다
> (리포에는 `web/` 디렉토리가 없음). 개발 리포에서는 **Vite dev(5173) + xazz-server(8005)** 조합이 가장 확실하다.

---

## 3. 타임라인 총정리 (총 약 2:50)

| 구간 | 시각 | 화면 | 내용 |
|---|---|---|---|
| 오프닝 + 정적 검사 | 0:00~0:35 | 터미널 | 프로젝트 소개 → `xazz check` 정적 타입/널 검증 |
| 컴파일러 (emit) | 0:35~1:00 | 터미널 | `.xzz` → Rust 소스 변환 |
| 전처리 + 차트 | 1:00~1:35 | 터미널 | Polars 전처리 → `[xazz:chart]` HTML 차트 |
| 딥러닝 컴파일 | 1:35~2:10 | 터미널 | `model{}` 선언 → Burn `train()` loss 감소 |
| 프라이버시 (DP) | 2:10~2:35 | 터미널 | `withDp()` 노이즈 + `[xazz:dp]` 예산 리포트 |
| Visual IDE | 2:35~2:50 | 브라우저 | `http://127.0.0.1:5173` 파이프라인 실행 + 감사해시 |

---

## 4. 액트별 상세 (화면 전환 · 실행 명령 · 대본)

### ACT 1 — 오프닝 + 정적 타입·널 검사 (0:00~0:35) — 터미널 A
**화면**: 전체 화면 터미널, 왼쪽에 `demo/deep_learning.xzz` 를 에디터로 펼쳐둠.

**실행 명령**:
```bash
./target/release/xazz check demo/deep_learning.xzz
```

**대본**:
> "Xazz는 파이썬 생태계의 생산성은 그대로, 하지만 대규모 데이터 처리에서 발생하는
> 런타임 타입 에러와 GPU 자원 낭비, 보안 검증 부재를 구조적으로 해결하기 위해 만든
> Rust 기반 차세대 AI 파이프라인 DSL입니다."
> (엔터) "스크립트처럼 즉시 작성하되, 실행 전에 컴파일러가 타입과 결측치를 정적으로 검증합니다.
> `xazz check` — 미선언 컬럼·타입 불일치·널 안전성을 실행 전에 전부 잡아냅니다. 통과했습니다."

---

### ACT 2 — 컴파일러: .xzz → Rust (0:35~1:00) — 터미널 A
**실행 명령**:
```bash
./target/release/xazz emit rust demo/deep_learning.xzz | head -40
```

**대본**:
> "겉으로는 스크립트지만, 핵심은 진짜 컴파일러입니다. 이 `.xzz` 스크립트가
> Polars LazyFrame과 Burn 텐서 연산을 포함한 실제 Rust 소스로 완전히 변환됩니다.
> 파서, AST, 타입 검사, 코드 생성까지 우리가 직접 구현한 툴체인입니다."

---

### ACT 3 — Polars 전처리 + 차트 (1:00~1:35) — 터미널 A
**실행 명령**:
```bash
./target/release/xazz run demo/preprocess_chart.xzz
```
(브라우저에서 생성된 `result_chart_chart.html` 을 열어 보여주면 더 좋다)

**대본**:
> "이제 실제 파이프라인을 돌립니다. 서울 공기질 데이터를 Polars LazyFrame으로 초고속 전처리하고,
> 결측치는 `fillNull`로 평균 채움, 구청별 평균을 내서 bar 차트로 렌더링했습니다.
> 파이썬 환경 대비 최대 2.62배(228K 행), 409만 행에서는 1.93배 성능을 냅니다 (README Performance 참고)."

---

### ACT 4 — Burn 딥러닝 컴파일 & 학습 (1:35~2:10) — 터미널 A
**실행 명령**:
```bash
./target/release/xazz run demo/deep_learning.xzz
```

**대본**:
> "핵심입니다. `model AirPredictor` 한 줄로 딥러닝 모델을 선언하고, `train()` 한 번으로
> Burn 엔진에서 실제 학습을 수행합니다. 에포크가 진행될수록 loss가 줄어드는 게 보이죠.
> PyTorch의 장황한 보일러플레이트 없이, 제로카피 텐서 변환으로 전처리 결과를 그대로 학습에 넘깁니다.
> 이게 바로 '겉은 스크립트, 핵심은 컴파일러'의 정수입니다."

---

### ACT 5 — 차등 프라이버시(DP) (2:10~2:35) — 터미널 A
**실행 명령**:
```bash
./target/release/xazz run demo/dp.xzz
```

**대본**:
> "금융·의료 같은 민감 데이터를 다루는 기업 환경을 위해, 통계 결과에 수학적 보호를 보장하는
> 차등 프라이버시 노이즈를 주입합니다. `withDp(epsilon:1.0, laplace)` —
> 집계 결과에 노이즈를 더해 특정 개인의 포함 여부를 역추적할 수 없게 하고,
> 프라이버시 예산 사용량을 리포트로 남깁니다. 모든 실행 이력은 SHA-256 감사 로그로 영구 기록됩니다."

---

### ACT 6 — Visual IDE (2:35~2:50) — 브라우저
**접속**: `http://127.0.0.1:5173` (터미널 B의 Vite dev)

**화면 이동**: 랜딩 → "Project Start / Open Workspace" → 편집기에서
`visual-ide/src/data.js` 의 기본 `runnableCode`(전처리→train→predict→DP) 를 **Run** 클릭.

**대본**:
> "마지막으로, 코드 없이도 파이프라인을 보고 실행할 수 있는 Visual IDE입니다.
> 전처리 → 모델 컴파일 → 학습 → 예측이 노드 그래프로 시각화되고, 오른쪽에 실행 결과와
> 코드 해시(감사 무결성)가 함께 표시됩니다. 입문자와 AI 에이전트 모두가 바로 쓸 수 있는
> Agent-Ready 환경입니다. 감사합니다."

---

## 5. 전체 대본 모음 (컷 편집용)

```
[0:00] Xazz — Rust 기반 차세대 AI 파이프라인 DSL. 
       파이썬의 생산성은 살리고, 런타임 타입 에러·GPU 낭비·보안 검증 부재를 컴파일러가 해결합니다.
[0:10] xazz check — 실행 전에 타입과 결측치를 정적으로 검증. 통과.
[0:35] xazz emit rust — .xzz가 Polars + Burn을 담은 실제 Rust 소스로 변환됩니다. 우리가 만든 컴파일러입니다.
[1:00] xazz run — 서울 공기질 데이터 전처리, 결측치 평균 채움, 구청별 평균 bar 차트. pandas 대비 최대 2.62배.
[1:35] model AirPredictor { } + train() — Burn에서 실제 학습. loss가 줄어듭니다. 제로카피 텐서 변환.
[2:10] withDp() — 차등 프라이버시 노이즈 주입, 예산 리포트. 모든 실행은 SHA-256 감사 로그로 기록.
[2:35] Visual IDE — 노드 그래프로 전처리→컴파일→학습→예측. 코드 해시로 무결성 확인. 감사합니다.
```

---

## 6. 리스크 & 백업 플랜

| 리스크 | 대비 |
|---|---|
| `examples/data` LFS 더미로 실패 | **반드시** `visual-ide/data/seoul_air_quality.csv` 사용 |
| `duplicate column` 에러 | 스키마에 CSV 4개 컬럼 전부 선언 (본 가이드 예제 그대로) |
| `xazz-runner` 못 찾음 | `xazz`와 `xazz-runner`가 같은 폴더인지 확인 |
| 8005 포트 충돌 | `xazz-server` 백그라운드 프로세스가 이미 떠 있지 않은지 확인 |
| 5173 포트 사용 중 | Vite가 자동으로 5174 등으로 올림 → 그 주소로 접속 |
| 딥러닝 학습이 느림 | epoch 수를 5로 낮춤(본 예제 기준 ~1초) |
| **가드레일/sLM 시연 요구** | 정적 가드레일은 시연 가능(`xazz policy --fix`). sLM 자동보정은 Ollama 스택 필요 |
| `xazz-server`가 IDE 안 서빙(404) | same-origin은 릴리즈 전용 → Vite dev(5173)로 IDE 열기 |
