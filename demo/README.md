# 시연 영상 (`demo/`)

오픈소스 개발자대회 제출용 3분 이내 시연 영상을 **재현 가능하게** 만드는 폴더다.

화면에 나오는 모든 글자는 실제로 실행한 명령의 진짜 출력이다
(`demo/capture.sh` 가 캡처 → `demo/make_video.py` 가 그림).
손으로 쓴 가짜 출력은 한 줄도 들어가지 않는다.

---

## 결과물

| 파일 | 내용 |
|---|---|
| `xazz_demo.mp4` | 시연 영상 · 1920×1080 · 2분 45초 · 한국어 자막 새겨짐 |
| `xazz_demo.srt` | 자막 파일 (영상과 동일 타임코드, 30컷) |
| `narration.wav` | 나레이션 음성 (`make_voice.py` 가 생성) |
| `xazz_demo_voiced.mp4` | 음성까지 입힌 최종본 |
| `narration.txt` | 나레이션 평문 — 다른 TTS 에 붙여 넣는 용도 |
| `narration.json` | 대본 + 컷별 노출 시간 (타임라인의 원본) |

생성물은 `.gitignore` 로 제외돼 있다 — 아래 명령이면 언제든 다시 만들어진다.

---

## 다시 만들기

```bash
cargo build --release -p xazz -p xazz-runner -p xazz-exec
bash demo/capture.sh              # 실제 명령 실행 → demo/_capture/*.txt
python3 demo/make_video.py        # → xazz_demo.mp4 + xazz_demo.srt
```

레이아웃만 빠르게 확인하려면:

```bash
python3 demo/make_video.py --preview   # 첫 프레임 1장만
```

---

## 나레이션 음성 넣기

영상에는 자막만 새겨져 있고 음성 트랙은 비어 있다.
`make_voice.py` 가 **자막 컷 하나하나를 따로 합성해 SRT 타임코드에 그대로 얹는다** —
전체를 한 번에 읽어 길이를 맞추는 방식이 아니라서 자막과 음성이 어긋나지 않는다.

```bash
pip install edge-tts
python3 demo/make_voice.py --mux      # → demo/xazz_demo_voiced.mp4
```

| 옵션 | 뜻 |
|---|---|
| `--voice ko-KR-InJoonNeural` | 남성 보이스로 교체 (기본값은 여성 `ko-KR-SunHiNeural`) |
| `--rate -5%` | 조금 천천히 |
| `--mux` | 영상에 바로 입힌다 (생략하면 `narration.wav` 만) |

컷이 배정된 시간보다 길게 읽히면 그 컷만 최대 1.25배까지 조이고,
그래도 넘치면 어떤 컷이 몇 초 모자란지 목록으로 알려 준다.
그때는 `narration.json` 의 문장을 줄이거나 `sec` 을 늘린 뒤
`make_video.py` 를 다시 돌리면 자막·영상·음성이 함께 맞춰진다.

이미 만들어 둔 오디오가 따로 있다면:

```bash
bash demo/add_voice.sh narration.mp3   # → demo/xazz_demo_voiced.mp4
```

> `narration.json` 의 `sec` 은 한국어 낭독 속도(약 6자/초)에 문장 사이 여백
> 0.5초를 더해 잡았다. 다른 속도의 TTS 를 쓸 거라면 이 값을 함께 조정한다.

---

## 구성 (2분 45초)

| 구간 | 시각 | 명령 | 보여주는 것 |
|---|---|---|---|
| 오프닝 | 0:00 | — | 프로젝트 한 줄 요약 |
| 정적 검사 | 0:21 | `xazz check` | 타입·널 안전성을 실행 전에 검증 |
| 컴파일러 | 0:41 | `xazz emit rust` | `.xzz` → 실제 Rust 소스 |
| 전처리 | 0:58 | `xazz run demo/preprocess_chart.xzz` | Polars LazyFrame · 자치구별 집계 |
| 딥러닝 | 1:18 | `xazz run demo/deep_learning.xzz` | Burn 학습 · 에포크별 손실 감소 |
| **보안 차단** | **1:41** | `xazz run examples/security/patient_unsafe.xzz` | **개인정보 노출 코드를 실행 전 차단 + 법 조항 근거** |
| **자동 보정** | **2:04** | `xazz policy … --fix` | **검증된 안전 대체 코드** |
| 프라이버시 | 2:17 | `xazz run demo/dp.xzz` | 차등 프라이버시 노이즈 · 예산 리포트 |
| 클로징 | 2:35 | — | 요약 |

굵게 표시한 두 구간이 `docs/submission_visuals_plan.md` 의 **B2(최우선)** 항목에 해당한다.

---

## 화면에서 감추는 줄

`make_video.py` 의 `display_filter()` 가 아래만 걸러낸다 — 판정을 바꾸는 내용은 감추지 않는다.

| 감추는 것 | 이유 |
|---|---|
| `[xazz] …` | 내부 진행 로그(stderr). 파이프라인 단계 카운터 등 |
| `[xazz WARN] 스키마 필드 … 찾을 수 없음` | 집계 파이프라인에서 스키마 검증이 결과 프레임을 대상으로 도는 기존 버그([#15](https://github.com/x1zzdev/Xazz/issues/15)). 이번 데모 논지와 무관한 잡음 |
| `[xazz:result] {…}` · `[xazz:train] {…}` | 프런트엔드용 한 줄짜리 대용량 JSON. 같은 내용이 바로 위에 사람이 읽는 표·로그로 이미 나와 있음 |

`[xazz:policy]` 판정과 `[xazz:dp]` 예산 리포트는 **감추지 않는다** — 데모가 증명하려는 대상이다.
다만 둘 다 한 줄이 화면 폭을 넘겨 접히므로 값은 그대로 두고 표현만 한 줄로 줄여 보여 준다.

---

## 데이터 주의

- `examples/data/*.csv` 는 Git LFS 포인터라 그대로는 실행이 안 된다.
  데모는 실제 데이터가 들어 있는 `visual-ide/data/seoul_air_quality.csv` 를 쓴다.
- 보안 데모용 환자 데이터는 `examples/security/generate_patients.py` 가 만드는 **합성 데이터**다.
  실제 개인정보는 한 건도 쓰지 않으며, CSV 는 저장소에 커밋되지 않는다.
