# Changelog

All notable changes to Xazz are documented in this file.

Format: [Keep a Changelog](https://keepachangelog.com/en/1.0.0/)  
Versioning: [Semantic Versioning](https://semver.org/)

---

## [Unreleased]

### Changed — D1 CUDA provider를 burn-tch에서 burn-cuda(네이티브 CubeCL)로 교체 (issue #62)

- **`xazz-exec`** — `--features cuda`가 `burn-tch`(LibTorch) 대신 `burn-cuda`
  (native CubeCL)를 사용한다. 순수 Rust라 LibTorch/시스템 SDK/MSVC 툴체인이
  필요 없고, `burn-wgpu`와 동일한 CubeCL 스택을 공유해 Burn 0.22의 CUDA 방향
  (CubeCL CUDA · graph replay · LLVM GPU 백엔드)과 정렬된다. 런타임에는 NVIDIA
  드라이버만 필요하며, 장치가 없으면 probe 후 CPU로 폴백한다(`CudaBackend::new`)
- **`build.rs`** — windows-gnu MSVC 가드를 ONNX 전용으로 축소. `cuda`는 이제
  GNU/MSVC 양쪽에서 빌드된다. `onnx*`는 여전히 `ort-sys` prebuilt 부재로 MSVC 전용
- `burn-tch`/`tch` 의존 제거. 비활성 선택적 백엔드 항목(burn-candle/rocm/flex 등)은
  `Cargo.lock`에 남아 있을 수 있으나 빌드 그래프에는 포함되지 않는다(`cargo tree`로 확인)

### Direction — Burn 0.22 대응: ONNX export 위임 예정, emitter parity 동결

- **ONNX (D2 #63)** — Burn 0.22의 `burn-onnx`(graph capture 기반 export)가
  손으로 작성한 `dl::onnx_export`(`ModelProto` 빌더)를 대체한다. `ort` 추론은
  유지하고, 0.22 안정화 시 exporter와 `rlx-onnx-proto`/`protobuf` 의존을 제거한다.
  현 exporter에는 더 투자하지 않는다
- **emitter** — `emit rust`의 Burn 코드 생성은 "reference emit" 지원 등급으로
  두고, 런타임(`xazz-exec`)과의 기능별 parity 추가 투자를 동결한다. 드리프트
  비용을 상한으로 묶기 위함(후속은 런타임에 집중)

### Tests — 다중 컬럼 DP 소비 개수 통합 회귀 테스트 (issue #119)

- **`xazz-exec/tests/dp_budget.rs`** — 실제 `.xzz`를 엔진 바이너리로 실행해 `[xazz:dp]` 마커
  (서버 원장과 `xazz run --json`이 읽는 값)로 소스→런타임→원장 경로를 고정한다:
  2컬럼 `agg([...]) |> withDp(ε)`가 `query_count=2`·`budget_spent=2ε`로 청구되는지,
  1컬럼 대조군, 예산 경계(`XAZZ_DP_BUDGET`가 정확히 `k·ε`면 통과·그 아래면 실행 전 거부
  + 거부 메시지에 컬럼 배수 `× 2` 반영 + 마커 미출력), 두 `withDp` 단계의 순차 조성
  (2.0 → 2.5, 3 mechanisms)과 누적 초과 거부. 프로세스별 env로 예산을 주므로 병렬 테스트와
  간섭이 없다

### Added — `xazz run --json`에 DP 소비량·잔량 노출 (issue #117)

- **`dp` 배열** — `--json` 요약에 `withDp` 단계마다 한 항목씩 `[xazz:dp]` 마커를 실행
  순서대로 담는다. 각 항목은 DpReport(`mechanism`, `epsilon`, `delta`, `sensitivity`,
  `noise_param`, `noised_columns`, `seed`)에 세션 예산 `budget_spent`/`budget_total`/
  `budget_remaining`(+ `_delta` 3종)과 `query_count`를 더한 값이다. 대시보드·CI가 stderr
  텍스트를 긁지 않고 ε/δ 소비량을 읽을 수 있다
- **`training` 필드** — 같은 경로에서 버려지던 `[xazz:train]` 마커(TrainReport/SweepReport)를
  `--json`에 그대로 싣는다. DP는 `withDp` 파이프라인 단계의 속성이라 TrainReport 자체에는
  DP 필드를 넣지 않는다(학습 리포트와 DP 감사 정보는 별 항목으로 병렬 제공)
- **엔진 마커 확장** — `[xazz:dp]`에 `budget_remaining`·`budget_remaining_delta`를 추가
  (`PrivacyBudget::remaining_delta` 신설). 기존 필드와 stderr 텍스트는 그대로다. 옛 엔진의
  마커에는 CLI가 `total − spent`로 잔량을 채워 넣어 소비자 입장에서 항상 존재한다
- 서버 `parse_stdout_markers`와 동일한 형태(단일 행 + 레거시 2행)를 CLI도 받아들인다
- 검증: CLI 마커 파서 단위 테스트 6종(순서·잔량 보정·0 하한·레거시·train/diagnostics·
  깨진 마커), `PrivacyBudget::remaining_delta` 단위 테스트

### Security — 클라이언트 연결이 끊겨도 런 회계·감사·DP 정산 완료 (GHSA-wxqx-r7f6-qq3p)

- **`xazz-server`** — `/execute` 핸들러가 실행과 후처리(런 기록·감사 체인 추가·DP 예약 정산)를
  클라이언트 future와 분리된 blocking 태스크(`run_execution_job`)에서 끝까지 수행한다.
  클라이언트가 응답 전에 연결을 끊어도 실행된 파이프라인이 감사 체인·ε 원장·`/runs`에 남는다
- 회귀 테스트 `disconnected_execute_still_audits_and_records` — 클라이언트를 중간에 끊은 뒤
  런·감사 레코드가 기록되는지 검증
- 테스트 격리 — `test_state()`가 호출마다 별도 SQLite 파일을 사용해 병렬 실행 시
  `database is locked`로 실패하던 플레이크를 제거
- 후속 — 연결이 끊긴 동안에도 테넌트 실행 락과 동시성 permit을 러너 종료까지 보유하도록
  `ExecutionSlot`(`OwnedSemaphorePermit` + `OwnedMutexGuard`)을 blocking 태스크로 이동.
  핸들러 future가 드롭돼도 같은 테넌트 동시 실행이 차단되고 용량 상한이 유지된다.
  회귀 테스트 `disconnected_execute_holds_slot_until_runner_exits` 추가

### Docs — GPU/ONNX 백엔드 빌드·실행 가이드 (issue #151)

- **`docs/GPU_BACKENDS.md`** — `xazz-exec`의 `wgpu`/`cuda`/`onnx`/`onnx-cuda`/
  `onnx-tensorrt`/`onnx-directml`/`onnx-coreml` feature별 요구사항·빌드 명령,
  플랫폼별 주의(Windows MSVC 전용, macOS CoreML), 런타임 선택 변수
  (`XAZZ_BACKEND`, 통합 `XAZZ_DEVICE`, 폴백 `XAZZ_WGPU_DEVICE`/`XAZZ_CUDA_DEVICE`/
  `XAZZ_ORT_EP`/`XAZZ_ORT_DEVICE`, `XAZZ_INFER_*`)와 acceptance 실행·현재 검증 상태 표
- **`README.md`** "Build from source"에 GPU/ONNX feature 안내와 문서 링크,
  **`DEMO_GUIDE.md`** 빌드 단계에 GPU 선택 안내 링크 추가

### Docs — DuckDB·PostgreSQL 커넥터 한계 문서화 (issue #152)

- **`docs/CONNECTORS.md`** — 두 DB 소스의 URI 형식·타입 매핑과 함께, 번들 DuckDB
  `1.10505`에서 `COPY (...) TO parquet`가 세그폴트되어 행 단위 `ValueRef` 읽기로
  우회한다는 한계(대량 결과는 전량 메모리 적재)와 PostgreSQL 커넥터가 항상
  `NoTls`(평문 TCP)로만 연결되어 TLS를 협상하지 못한다는 보안 주의·권장 구성
  (SSH 터널/stunnel/프록시, 읽기 전용 최소 권한, 자격증명 평문 노출)을 명시
- **`README.md`/`README_kr.md`** 커넥터 feature 행에 문서 링크와 한계 요약,
  **`DEMO_GUIDE.md`** 예제 단계에 커넥터 문서 링크 추가

### Docs — mdBook 문서 사이트 스캐폴드 (issue #153)

- **`book.toml`** — mdBook 설정(`src = "docs"`, `build-dir = "book"`,
  `[output.linkcheck]`)과 **`docs/SUMMARY.md`** — 46개 문서를 개요/실행/보안·절차/
  설계/디자인 시스템/데모·제출/스펙/장애 분석/그림으로 묶은 목차,
  **`docs/README.md`** — 문서 사이트 진입점
- 내부 링크 검증은 `[output.linkcheck]`(mdbook-linkcheck) 백엔드로 `mdbook build`
  시 자동 실행. 저장소 상대 링크(`../README.md`, `../xazz-*/src/...`)와 에셋
  디렉터리(`src/`)는 사이트 밖이므로 검사에서 제외
- **`.github/workflows/docs.yml`** — mdBook 0.4.52 + mdbook-linkcheck 0.7.7 설치 후
  `docs/**`/`book.toml` 변경 시 빌드·링크 검증, `book/html`을 `xazz-docs` 아티팩트로
  업로드. GitHub Pages 배포는 `ENABLE_PAGES` 저장소 변수로 opt-in
- `.gitignore`에 `book/` 추가

### Build — Windows GPU 기능 MSVC 툴체인 가드 (issue #103)

- **`xazz-exec/build.rs`** — `windows-gnu` 타깃에서 `cuda`/`onnx*` feature를 켜면
  빌드 초입에 MSVC 설치 안내(`rustup default stable-x86_64-pc-windows-msvc` + VS
  Build Tools)와 함께 중단한다. LibTorch는 MSVC ABI 전용이고 `ort-sys`에는
  windows-gnu prebuilt가 없어 GNU 툴체인에서는 컴파일이 불가능한데, 지금까지는
  torch-sys/ort-sys의 장황한 C++·다운로드 오류로만 드러났다. `XAZZ_ALLOW_WINDOWS_GNU_GPU=1`
  로 가드를 우회할 수 있다(비지원)
- **`xazz-exec/Cargo.toml`** feature 주석과 **`CONTRIBUTING.md`** "Optional GPU backends"
  표에 `--release`(windows-gnu debug 바이너리 4GB 초과) 및 Windows MSVC 요건 명시
- `docs/design/gpu-backend-acceptance.md` §6.1에 반영 표기

### Docs — GPU 백엔드 실기 검증 기록 (issue #103)

- **`docs/design/gpu-backend-acceptance.md`** — Windows 11 / RTX 4070 Laptop + Intel Arc iGPU
  호스트에서 D1(#62)·D2(#63)의 `#[ignore]` acceptance를 실행한 기록. `wgpu`는
  `wgpu_matches_cpu_losses` 통과 + `XAZZ_DEVICE=dgpu:0`/`igpu:0`가 각각 RTX 4070 / Intel Arc를
  선택함을 프로세스별 GPU 엔진 카운터로 확인. `cuda`(torch-sys C++ 셰임 g++ 컴파일 불가)와
  `onnx`(`ort-sys`에 windows-gnu prebuilt 없음)는 **Windows에서 MSVC 툴체인 필수**로 판명 —
  오류 원문과 재현 명령 수록. 부수 발견: windows-gnu debug 테스트 바이너리 4GB 초과(`--release`
  필요), wgpu 어댑터 선택 로그 부재, `xazz-runner` 300s 기본 타임아웃
- `docs/ROADMAP.md` D1/D2 항목에 실기 결과 반영

### Performance — GPU/ONNX 벤치 측정 왜곡 제거 (NEXT #74/#75/#86)

- **GPU→CPU 인메모리 핸드오프 (D1/D2)** — `train_on_device`가 학습 후 CPU
  아티팩트를 만들 때 pretty-JSON 체크포인트를 **디스크에서 다시 읽던** 경로를
  Burn `BinBytesRecorder`(bincode) 기반 인메모리 전송(`materialize_cpu_model`)으로
  교체했다. 디스크 체크포인트는 기존 pretty-JSON 형식을 유지해 `predict`/emit
  호환을 보존한다. 학습 시간에 save→load 왕복이 더 이상 섞이지 않는다
- **스윕 우승자만 저장 (D1/D3)** — trait `ComputeBackend::train_unpersisted`
  (기본 구현은 `train`)와 `dl::train_unpersisted`/`train_on_device_unpersisted`,
  `train_impl(persist:)`를 추가했다. 스윕 그리드는 조합마다 unpersisted 학습을
  쓰고 **우승 조합만** 체크포인트·매니페스트를 쓴다(조합마다 저장/왕복 제거).
  ONNX도 조합별 export를 하지 않고 우승 아티팩트를 `ensure_export`로 지연 export
- **추론 chunking (D1/D2)** — `XAZZ_INFER_CHUNK`(기본 4096, `0`=비활성)와
  `chunk_ranges`/`forward_predictions`로 `[n, feature_count]`를 행 단위 청크로
  업로드한다. wgpu/cuda/CPU와 ONNX `predict_with_session`에 공통 적용해 대량 n의
  VRAM OOM·전송 병목을 막는다(결과 불변)
- **추론 캐시 LRU (D1/D2)** — 단일슬롯 캐시를 `dl::LruCache`(MRU 승격 + LRU 퇴출)로
  일반화하고 `XAZZ_INFER_CACHE_SLOTS`(기본 4)로 크기를 정한다. 모델을 교대 예측하는
  워크로드에서 재로드를 줄이며 무한 성장하지 않는다. wgpu/cuda 모듈·ONNX 세션에 적용
- **device/EP 선택 통합 (D1/D2)** — 통합 `XAZZ_DEVICE`
  (`auto|cpu|dgpu[:N]|igpu[:N]|vgpu[:N]|cuda[:N]|tensorrt[:N]|directml[:N]|coreml`)와
  `parse_device_spec`를 추가해 wgpu 어댑터·CUDA 인덱스·ONNX EP를 한 문법으로
  선택한다. 기존 `XAZZ_WGPU_DEVICE`/`XAZZ_CUDA_DEVICE`/`XAZZ_ORT_EP`는 폴백으로 유지
- 검증: default `cargo test -p xazz-exec` 83 통과(신규 인메모리 핸드오프·unpersisted·
  chunk 범위·LRU·device 파서 테스트 포함). `cargo clippy -p xazz-exec`가
  default/`wgpu`/`cuda`/`onnx`/`onnx-cuda` 및 `--workspace --all-targets -- -D warnings`
  전 조합 통과

### Added — C2 정책 이력 정기 만료 스윕

- **유휴 테넌트 만료 행 물리 삭제** — 기존에는 정책 팩 변경(write) 트랜잭션에서만
  만료 행을 prune해서, 변경이 없는 테넌트의 만료 행이 디스크에 잔존했다(read 필터로만
  숨김). `Store::sweep_expired_policy_history()`가 전 테넌트를 순회하며 **테넌트별
  유효 보존 윈도**(override 우선, 없으면 전역 기본값)를 해석해 만료 행을 삭제한다.
  명시적 `0` override(만료 비활성)인 테넌트는 건드리지 않는다
- **주기 실행** — 서버가 `XAZZ_POLICY_HISTORY_SWEEP_SECS`(기본 3600초, `0`=비활성)
  간격으로 백그라운드 스윕을 돌린다. 삭제 행이 있으면 로그로 보고
- 검증: `resolve_policy_history_sweep` 파서 테스트 + 유휴 테넌트 스윕/명시적 `0`
  override 보호/재스윕 no-op 회귀 테스트 (xazz-server 77 tests)

### Added — D3 스윕 tiebreak 기준 (issue #64)

- **`train(..., tiebreak: "epochs" | "lr" | "batch")`** — 스윕 리포트 정렬에서
  주 정렬 키(`sort:`)가 동률일 때 먼저 비교할 하이퍼파라미터 축을 지정한다.
  리스트(`tiebreak: [lr, batch]`)로 **여러 축을 순서대로** 지정할 수 있으며,
  지정하지 않으면 기존 `sort:`별 결정적 폴백 순서를 유지한다. `metric`은 축이
  아니므로 파서가 거부하고 중복 축도 오류로 처리한다
- `TrainConfig.sweep_tiebreak: Vec<SweepSort>`(xazz-core AST) + 파서 `tiebreak:`
  인수(문자열/식별자 또는 리스트, 별칭 허용, `metric`/미지원/중복 값은 오류) +
  `SweepReport.tiebreak`(JSON 노출, 순서 보존) + `SweepReport::compare(..., tiebreak: &[SweepSort])`
  폴백 순서 재구성
- 체커 무그리드 경고에 `tiebreak:` 포함, 정책 프린터 `tiebreak:` 라운드트립 보존,
  런타임 스윕 표 헤더에 tiebreak 표기
- 검증: `SweepSort::is_axis`, expand_sweep tiebreak 중립화, 파서 tiebreak/오류 2종,
  파서 리스트/중복/빈 목록 3종, `compare` 명시 다축 tiebreak, CPU 백엔드 tiebreak 전달 E2E

### Added — ONNX GPU 실행 프로바이더 (D2 #63)

- **EP feature 4종** — `onnx-cuda`(`ort/cuda`), `onnx-tensorrt`(`ort/tensorrt`),
  `onnx-directml`(`ort/directml`), `onnx-coreml`(`ort/coreml`). 각 feature를 켜면
  `ort`의 `download-binaries`가 **GPU 빌드**를 받는다(Windows x86_64:
  `cuda13,tensorrt,nvrtx,directml`, macOS aarch64: `coreml`). 즉 `--features
  onnx-cuda`만으로 GPU ONNX Runtime이 확보된다
- **`XAZZ_ORT_EP` 선택** — `auto`(기본)는 컴파일된 GPU EP를 우선순위
  (cuda→tensorrt→directml→coreml)로 등록하고 실패 시 다음/CPU로 조용히 폴백한다.
  명시 목록(`cpu,cuda,...`)은 **fail-closed**: 요청한 EP가 미컴파일/미가용이면
  CPU로 몰래 도는 대신 오류를 낸다(벤치가 CPU를 GPU로 오측정하지 않도록)
- **`XAZZ_ORT_DEVICE`** — CUDA/TensorRT/DirectML EP device index(기본 0)
- `onnx_export::load_session`이 세션 생성 시 EP를 적용한다(캐시되어 1회).
  `OrtEpKind`/`OrtEpSpec`/`parse_ort_ep_spec`는 `ort` 없이 단위 테스트 가능
- 검증: `cargo check/clippy -p xazz-exec`가 `onnx`/`onnx-cuda`/`onnx-tensorrt`/
  `onnx-directml`/`onnx-coreml` 및 4종 동시에서 `--all-targets -- -D warnings`
  통과. `parse_ort_ep_spec` 단위 테스트. 실기 EP 실행은 Windows/macOS에서
  (WSL은 ort prebuilt C++ 링크 제약)

### Performance — GPU/ONNX 추론 캐시 + WebGPU device 선택·폴백 (NEXT #74/#75/#86)

- **추론 캐시 (D1/D2)** — `dl::load_inference_model`/`dl::predict_with_model`를
  분리하고, `ArtifactKey`(경로 + mtime + 길이) 기반 **단일슬롯 캐시**를 wgpu/cuda
  백엔드에 적용했다. 반복 `predict`가 체크포인트를 디스크에서 재로드하지 않으며,
  재학습으로 파일이 바뀌면 키가 달라져 자동 무효화된다
- **ONNX 재export/세션 캐시 (D2)** — `onnx_export::ensure_export`(체크포인트보다
  최신이면 재사용)/`load_session`/`predict_with_session`로 분리하고, `OnnxBackend`가
  export 아티팩트 정체성 기준으로 `Session`을 캐시한다. `predict` 호출마다
  `.onnx` 재작성 + 세션 재생성을 하지 않는다
- **WebGPU device 선택 (D1 #75)** — `XAZZ_WGPU_DEVICE`
  (`default|cpu|dgpu[:N]|igpu[:N]|vgpu[:N]`, `DiscreteGpu(N)` 표기 허용)로 어댑터를
  고정한다. 예: RTX 4070 + Intel Arc 노트북에서 `dgpu`/`igpu` 선택
- **device probe + CPU 폴백 (D1 #75)** — provider 생성 시 device를 1회 probe하고,
  어댑터가 없으면 `resolve()`가 CPU로 폴백하며 경고한다(cubecl의 어댑터 선택 panic을
  `catch_unwind`로 오류화). CUDA도 생성 시 probe로 이동
- 검증: `cargo check/clippy -p xazz-exec`가 default/`wgpu`/`cuda`/`onnx` 전 조합에서
  `--all-targets -- -D warnings` 통과. wgpu 실기 acceptance(lavapipe 소프트웨어
  Vulkan) 통과. `parse_wgpu_device`/`artifact_key` 단위 테스트 추가
  (2026-09-22)

### Added — D2 ONNX export + ONNX Runtime 추론 (issue #63)

- **`OnnxBackend` 실제 구현** — `--features onnx`에서 CPU 참조 학습 후
  `checkpoints/<Model>.onnx`로 export하고, 추론은 ONNX Runtime(`ort`)으로 실행한다.
  기존 스캐폴드(에러 반환)를 대체한다
- **ONNX exporter** (`xazz-exec/src/dl/onnx_export.rs`, `dl`의 자식 모듈) — `Mlp`
  그래프를 표준 ONNX `ModelProto`로 방출한다. `Dense`(Gemm), `Conv1d`(Same 패딩
  `pads`), `Embedding`(Split→Clip→Add→Cast→Gather→Reshape), ReLU/Sigmoid/Tanh/
  Softmax 지원(Dropout은 추론 시 항등). 미지원 그래프는 fail-closed
- **런타임** — `ort`(ONNX Runtime) + protobuf 타입은 `rlx-onnx-proto`(빌드 시
  `protoc` 불필요), 바이너리는 `ort`의 `download-binaries`로 자동 확보. 기본
  `tls-native`(시스템 OpenSSL) 대신 `tls-rustls` 사용
- **파리티 검증** — D2 acceptance(`onnx_matches_cpu_losses`)는 동일 CPU 가중치를
  CPU 인메모리 추론과 ONNX Runtime 추론으로 각각 실행해 예측이 일치하는지 확인.
  exporter 단위 테스트(`dense_export_emits_gemm_relu_gemm`) 추가
- 검증: `cargo check/clippy -p xazz-exec --features onnx --all-targets -- -D warnings`
  통과. 프로그램 전체 `cargo test --workspace`/`cargo deny check` 회귀 없음
  (이 WSL은 zig C++ 링커라 ort의 prebuilt C++ 정적 라이브러리 링크가 불가해
  실기 acceptance는 표준 툴체인/Windows에서 실행)

### Added — D1 `burn-tch`(CUDA) 실제 provider (issue #62)

- **`CudaBackend` 실제 구현** — `--features cuda`에서 `burn-tch`(LibTorch)로 NVIDIA
  GPU 학습·추론을 수행한다. 기존 스캐폴드(에러 반환)를 대체한다
- **device 주입 리팩터** — `dl::train_on_device`/`dl::predict_on_device`가 명시적
  `B::Device`를 받는다. `burn-tch`의 기본 device가 CPU이므로 provider가
  `LibTorchDevice::Cuda(index)`를 전달한다(`XAZZ_CUDA_DEVICE`, 기본 0)
- **fail-closed 진단** — 연결된 LibTorch에 CUDA 런타임이 없거나 인덱스가 범위를
  벗어나면 tch 내부 panic 대신 명확한 오류를 반환한다(`tch::Cuda::is_available`)
- 기존 `train_on`/`predict_on`은 `Default::default()` device로 위임(하위호환, wgpu 경로 유지)
- 검증: `cargo check/clippy -p xazz-exec --features cuda --all-targets -- -D warnings`
  통과, CUDA device 부재 시 fail-closed 단위 테스트 추가. 실기 acceptance는 CUDA
  호스트에서 `cargo test -p xazz-exec --features cuda -- --ignored`

### Fixed — 스윕 그리드 없는 `metric:`/`sort:`/`top:` 무시 경고 (issue #64)

- 스윕 그리드(리스트형 `epochs`/`lr`/`batch_size`) 없이 `metric:`/`sort:`/`top:`만
  지정하면 조합이 하나뿐이라 옵션이 조용히 무시되던 문제를 체커 경고로 알린다.
  오류가 아니라 경고(비치명적)이며, 지정된 옵션만 안내한다
- `metric: "mse"`/`sort: "metric"`처럼 기본값을 명시 지정한 경우에도 "지정됨"을
  구분하지 못해 경고가 누락되던 문제를 `sweep_metric_explicit`/`sweep_sort_explicit`
  플래그로 수정 (2026-09-22)
- 검증: `metric:`만/`sort:`만/`top:`만 무그리드 경고 3종 + 3종 동시 지정 통합
  경고 1종 + 스윕 동반 시 무경고 1종 + 명시 기본값 경고 2종

### Added — D3 스윕 리포트 정렬/필터 (`sort:`, `top:`) (issue #64)

- **`train(..., sort: "metric" | "epochs" | "lr" | "batch")`** — 하이퍼파라미터
  스윕 표/리포트에 출력되는 조합의 정렬 기준을 고를 수 있다. `metric`(기본)은
  선택 지표 기준 최적 우선, 나머지는 해당 하이퍼파라미터 오름차순이며 동률은
  나머지 축으로 결정적으로 정렬된다. 우승 조합 선정은 기존 `metric` 기준 유지
- **`train(..., top: N)`** — 지표 기준 상위 N개 조합만 리포트에 남긴다(1 이상,
  `top: 0`은 오류). 우승 조합은 항상 포함되며 `SweepReport.total_combos`가
  필터 전 전체 조합 수를 보존한다
- `SweepSort`(xazz-core AST) + 파서 `sort:`/`top:` 인수(문자열/식별자, 별칭 허용,
  미지원 값/0은 오류). `SweepReport`에 `sort`/`top`/`total_combos` 추가(JSON 노출),
  `dl::SweepReport::compare` 정렬 헬퍼, 런타임 표 헤더에 지표/정렬·`표시/전체` 표기
- 검증: `SweepSort` 파싱/별칭, expand_sweep의 sort/top 중립화, 파서 sort/top/오류
  3종, `compare` 축별 정렬, `sort: lr`·`top: 2` 스윕 E2E 2종

### Added — D3 스윕 선택 지표 (MAE/R²) (issue #64)

- **`train(..., metric: "mae" | "r2")`** — 하이퍼파라미터 스윕의 우승 조합 선택
  기준을 MSE(기본) 외에 MAE·R²로 고를 수 있다. 검증 분할이 있으면 검증 지표를,
  없으면 학습 지표를 사용하고 R²는 높을수록 좋은 지표로 처리한다
- `SweepMetric`(xazz-core AST) + 파서 `metric:` 인수(문자열/식별자, 별칭 허용,
  미지원 값은 오류). `TrainConfig`/`expand_sweep`가 지표를 조합마다 전달
- `dl::train`이 학습/검증 분할 전체에 대해 MAE·R²를 계산해 `TrainReport`와
  `SweepCombo`에 기록(무분산 타깃 R²=0, 비유한 값은 최하위). `SweepReport`가
  `metric`과 지표 인식 `score()`를 보유
- 런타임 스윕 표에 선택 지표 컬럼/이름, 학습 리포트에 MAE·R² 출력
- 검증: 지표 파싱/별칭/오류, expand_sweep 지표 전달, `regression_metrics`
  (MAE/R²/무분산/빈 입력), `score` 지표별 정렬, `metric: r2` 스윕 E2E

### Added — `emit rust` predict 추론 코드 (issue #64)

- **`data |> predict(model_var, as: "...")`가 실제 Burn 추론 블록을 생성** — 기존에는
  predict op이 주석만 emit됐다(런타임에는 반영). 이제 같은 프로그램에서
  `v m = ... |> train(...)`로 학습된 모델 변수를 해석해, 체크포인트
  (`checkpoints/<Model>.json`)를 로드하고 forward를 실행해 예측 컬럼을 부착한다
- **정규화 사이드카** — 학습 emit이 특성 순서와 z-score 통계를
  `checkpoints/<Model>.stats.json`에 기록한다. `extract_xy`가 특성 이름을 함께
  반환하고, predict가 이를 읽어 학습과 동일한 전처리(z-score, Embedding-first는
  raw 인덱스)를 재현한다. `emit` 산출물 Cargo.toml에 `serde_json` 의존성 추가
- predict가 파이프라인 중간에 오면 lazy 체인을 `collect()`로 끊어 추론한 뒤
  이후 op부터 새 lazy 체인을 시작한다. 예측 기본 컬럼명은 `<target>_pred`
- 스윕 학습 모델은 예측 대상 체크포인트가 모호해 주석 폴백 처리
- 검증: 사이드카 기록/extract_xy 특성 이름, 추론 블록(체크포인트 로드·forward·
  컬럼 부착), 중간 predict 후 relazy 체인, 기본 컬럼명, 스윕 폴백 회귀 테스트 5종.
  xazz-compiler 301 tests

### Fixed — `emit rust` VarDecl 종단 train 블록 (issue #64)

- **`v m = ... |> train(...)` 형태도 실제 Burn 학습 코드를 생성** — 기존에는 VarDecl의
  train op이 주석만 emit되고 학습이 생성 코드에서 누락됐다(런타임에는 반영). 이제
  train이 종단 op이면 파이프라인 데이터를 `collect()`한 뒤 `emit_dl_train_call`로
  단일/스윕 학습 블록을 emit하고 결과 println을 생략한다(런타임과 동일 의미)
- `program_has_dl`이 VarDecl의 train op도 DL로 감지해 burn import/헬퍼를 포함한다
- 검증: VarDecl train 실제 블록·체크포인트 저장·구 placeholder 부재, VarDecl 스윕
  조합 루프 회귀 테스트 2종

### Added — D3 체크포인트 버저닝 (issue #64)

- **`CheckpointManifest` + 사이드카** — Burn 체크포인트(`<model>.json`) 옆에
  `<model>.meta.json`을 기록한다. Xazz 체크포인트 형식 버전(`CHECKPOINT_FORMAT_VERSION`),
  xazz 버전, 모델명/타깃/입출력 차원/특성 컬럼/레이어 그래프/학습 하이퍼파라미터와
  손실을 담는다
- **로드 시 검증** — `load_checkpoint_manifest`가 사이드카가 선언한 형식이 이 빌드보다
  최신이면 fail-closed로 거부한다. 사이드카가 없는 레거시 체크포인트는 그대로 로드한다.
  `TrainReport`에 `checkpoint_format_version`을 추가했다
- 학습(`train_impl`)과 스윕(winner 재저장) 양 경로에서 매니페스트를 기록하고, GPU
  `predict_on`이 디스크에서 로드하기 전에 검증한다
- 검증: 매니페스트 라운드트립(버전·레이어), 신버전 거부, 레거시 부재 허용 단위 테스트.
  xazz-exec 65 tests

### Added — D3 Embedding 컬럼별 독립 vocab (issue #64)

- **`Embedding([v0, v1, ...], embed_dim)`** — 선두 Embedding 의 vocab 인자를
  리스트로 주면 입력 특성 컬럼별로 독립된 임베딩 테이블을 쓴다. 스칼라
  `Embedding(v, embed_dim)` 은 기존처럼 모든 컬럼이 하나의 테이블을 공유한다
- 런타임은 컬럼별 vocab 을 하나의 결합 테이블로 만들고, 컬럼 j 의 인덱스를
  `clamp(value, 0, vocab_j-1) + offset_j` 로 각자의 행 범위에 매핑한다(그래디언트
  분리). 컬럼별 vocab 개수와 입력 특성 컬럼 수가 다르면 fail-closed
- 범위 밖 인덱스 진단도 컬럼별 vocab 기준으로 계산하며, vocab 이 불균일하면
  컬럼별 목록을 함께 안내한다
- 파서/체커/emitter/dl/printer 전 계층 반영. 검증: 파서(AST·빈 목록 오류),
  체커(유효·0 항목 오류), emitter(공유 복제·per-column 목록·결합 테이블),
  CPU E2E(컬럼별 학습/예측·길이 불일치 거부)

### Added — per-tenant DP window 설정 (issue #59, Track C2)

- **`tenant_dp_config` 테이블** — tenant별 DP window 길이 override 저장. `GET /dp/budget`에
  `window_source`(`"tenant"` | `"global"`)를 추가해 override와 전역 기본값을 구분한다
- **`PUT /dp/budget/window` / `DELETE /dp/budget/window`** — 인증된 tenant가 자기 window를
  설정/해제(self-service). `window_secs: 0`은 명시적 누적 override, DELETE는 전역
  `XAZZ_TENANT_DP_WINDOW_SECS` 기본값으로 복귀한다
- **`Store::set_dp_window` 재앵커** — window 변경 시 기존 예산 row의 anchor를 현재로 옮겨
  새 window가 설정 시점부터 시작하고, 이전에 누적된 spend를 소급 만료시키지 않는다
- 검증: store override 격리/clear/재앵커(누적 spend 보존), 핸들러 tenant 격리·`window_source`.
  xazz-server 63 tests

### Added — `xazz registry deploy` 테넌트 정책 배포 (Track C2)

- **`xazz registry deploy <name> --tenant T`** — 내장 정책 팩을 실행 중인 서버의
  `PUT /security/policy`로 배포한다. `--server`(기본 `http://127.0.0.1:8005`),
  `--token`(미지정 시 `XAZZ_ADMIN_TOKEN` → `XAZZ_SERVER_TOKEN`), `--actor`(관리자
  대리 변경 시 `X-Xazz-Actor`)를 지원한다. stdlib 모듈은 배포 대상이 아니며
  네트워크 호출 전에 거부된다
- **`src/http.rs`** — Tokio/Polars 없이 std만 쓰는 최소 HTTP/1.1 클라이언트
  (`http://` 전용, chunked 디코딩·헤더 인젝션 거부). CLI 경량 제약 유지
- 검증: URL/응답 파싱 단위 테스트 + 로컬 TcpListener 가짜 서버로 PUT 경로·헤더·
  본문·오류 상태 E2E 검증. xazz 14 tests

### Added — 관리자 대리 정책 변경 감사 actor (Track C2)

- **`XAZZ_ADMIN_TOKEN`** — 이 토큰으로 인증된 요청은 `X-Xazz-Tenant`가 가리키는
  임의 테넌트의 정책 팩을 변경할 수 있다. self-service 테넌트는 기존대로 자기
  네임스페이스만 쓴다
- **`X-Xazz-Actor` 헤더** — 관리자 대리 변경 시 `changed_by`에 기록할 주체 이름
  (미지정/공백은 `admin`). 네임스페이스는 대상 테넌트로 유지되고 감사 이력에는
  actor가 남는다. 관리자 요청이 `X-Xazz-Tenant`를 지정하지 않으면 400으로 거부해
  빈/전역 네임스페이스에 변경이 떨어지지 않는다
- 검증: 관리자 대리 set/delete가 대상 네임스페이스에 반영되고 이력 `changed_by`가
  actor로 기록, 테넌트 격리 유지, 대상 미지정 400, actor 기본값. xazz-server 60 tests

### Added — 정책 팩 변경 이력 감사 (issue #59, Track C2)

- **SQLite `tenant_policy_history` 테이블** — tenant별 정책 팩 변경을 append-only로
  기록(누가/언제/이전 팩/새 팩). `tenant_policies`는 최신 상태만 유지하므로
  교체·삭제 이력이 남지 않던 공백을 메운다
- **`set_tenant_policy`/`delete_tenant_policy`에 `changed_by` 추가** — 팩 변경과 이력
  append를 하나의 트랜잭션으로 커밋해 감사 기록이 실제 상태와 어긋나지 않음.
  이전 팩은 같은 트랜잭션에서 읽어 `old_policy_json`으로 보존
- **`GET /security/policy/history`** — 인증된 tenant의 변경 이력을 최신순으로 반환
  (tenant 스코프, `old_policy_json`/`new_policy_json`은 임베드된 JSON)
- **보존 상한 + 페이지네이션** — tenant별 이력은 `XAZZ_TENANT_POLICY_HISTORY_MAX`
  (기본 1000, 0/무효는 기본값) 개수 상한을 두고, 변경과 같은 트랜잭션에서 오래된
  행을 정리해 무한 성장을 막는다. `GET /security/policy/history?limit=&offset=`
  로 최신순 페이징(`limit` 기본 100, `1..=500` 클램프, 응답에 `limit`/`offset` 에코)
- 검증: 저장소 append-only·tenant 격리·이전 팩 보존, 엔드포인트 who/when/previous,
  보존 상한 프루닝·페이지 오프셋·limit 클램프. xazz-server 57 tests pass

### Added — D3 하이퍼파라미터 스윕 (그리드 서치, issue #64)

- **`train()` 리스트 인자** — `epochs`/`lr`/`batch_size` 에 리스트를 넘기면
  카티전 곱 그리드 서치로 전 조합을 학습한다. 예:
  `train(M, target: "y", epochs: [10, 20], lr: [0.01, 0.001], batch_size: [16, 32])`.
  단일 원소 리스트(`[0.05]`)는 기존 스칼라 인자로 취급된다. 파서는 빈 목록
  `[]` 을 오류로 거부한다
- **결과 보고** — 조합별 표(epochs/batch/lr/검증·학습 손실)와 `★ 최적 조합` 을
  출력하고, `validation_split` 이 있으면 검증 손실, 없으면 학습 손실이 가장
  낮은 조합을 선택한다. 선택된 모델이 이후 `predict()` 에 사용되며, 체크포인트도
  최적 모델로 다시 저장된다. `[xazz:sweep]` JSON 마커로도 노출
- **반영 범위**: AST(`SweepGrid`/`TrainConfig::expand_sweep`) → 파서 → 체커
  (범위 검증: epochs/batch_size ≥ 1, lr > 0) → emitter(조합 루프 코드 생성) →
  policy printer → 런타임(`ComputeBackend::sweep` 기본 메서드) → dl 체크포인트 재저장
- 테스트: AST 전개, 파서 목록/빈 목록, 체커 유효·범위 오류, emitter 문자열,
  xazz-exec CPU 백엔드 선택·E2E 스윕

### Added — v0.23 `agg([...])` 다중 집계 + `load()` sep/header 옵션

- **`agg([...])`** — 한 번의 group/select 패스로 여러 집계를 계산하는 파이프라인
  연산자. `groupBy("station") |> agg([min("pm10"), mean("pm10"), max("pm10")])`
  처럼 사용하며, 각 결과 컬럼은 `<col>_<agg>` 로 aliasing 된다
  (`pm10_min`, `pm10_mean`, `pm10_max`). 그룹이 없으면 `select([...])` 로 축약된다.
  `sum`/`mean`/`min`/`max`/`count`/`median`/`variance`/`std` 지원, 빈 목록·미지원
  함수·비숫자형 집계는 진단(오류/경고)으로 처리
- **`load(..., sep: ";", header: false)`** — CSV 파서 옵션 named argument. `sep:`/
  `separator:`(1바이트), `header:`/`hasHeader:`(true/false)를 지원하며 런타임·emitter
  양쪽 CSV 리더에 반영. 헤더가 없어도 `:: Schema` 로 위치 기반 컬럼 매핑
- 반영 범위: AST/토큰/파서 → IR → 체커 → codegen/emitter → 런타임(lower) →
  policy(shape/lineage) → catalog
- 테스트: 파서 AST/별칭/오류, 체커 lowering/누락컬럼/경고, codegen·emitter 문자열,
  policy DP 규칙, xazz-exec E2E(집계 컬럼·sep/header 파싱)

### Added — D3 Conv1d 레이어 (CNN, issue #64)

- **`Conv1d(out_channels, kernel_size)`** — `model {}` 선언에서 1D 합성곱 레이어 지원.
  `Same` padding + stride 1로 특징축 길이를 보존하고, 파서 / 정적 체커 / emitter /
  runtime / dl 전체 파이프라인에 반영. 공유 `Mlp` 그래프가 Dense와 Conv1d를 선언
  순서대로 실행
- 체커: Dense 없이 Conv1d만 있어도 유효하며, `out_channels`/`kernel_size` < 1은
  컴파일 오류
- 테스트: 파서 AST, 체커 유효/오류, emitter(`PaddingConfig1d`), CPU E2E train/predict
- 임베딩 레이어와 하이퍼파라미터 스윕은 후속(백로그)

### Added — D3 Embedding 레이어 (범주 입력, issue #64)

- **`Embedding(vocab_size, embed_dim)`** — `model {}` 선언의 범주형 입력 임베딩.
  반드시 첫 번째 레이어여야 하며, 입력 특성값을 범주 인덱스로 해석해
  `[batch, input_dim, embed_dim]` 으로 매핑한 뒤 `[batch, input_dim * embed_dim]`
  으로 펼친다. 파서 / 정적 체커 / emitter / runtime / dl 전체 파이프라인에 반영
- 첫 레이어가 Embedding 이면 z-score 표준화를 건너뛰고 원본 범주 인덱스를 그대로
  사용(결측 → 0). 인덱스는 `[0, vocab_size-1]` 로 clamp
- 체커: `vocab_size`/`embed_dim` < 1 은 오류, Embedding 이 첫 레이어가 아니면 오류
- 테스트: 파서 AST, 체커 유효/파라미터·위치 오류, emitter(`EmbeddingConfig`·정규화 생략),
  CPU E2E train/predict
- 하이퍼파라미터 스윕은 후속(백로그)

### Added — 오픈소스 거버넌스·라이선스 정책 (커뮤니티 확장)

- **`deny.toml`** — `cargo deny` 기반 의존성 정책: permissive 라이선스 allow 목록,
  MPL-2.0은 크레이트별 exception으로 한정(전역 미허용), GPL/AGPL은 라이선스 단계에서
  자동 차단, registry/git 출처 고정, RustSec advisories 검사
- **CI `licenses` 잡** — `EmbarkStudios/cargo-deny-action`으로 licenses · bans ·
  sources · advisories를 매 PR/push마다 검증
- **거버넌스 문서** — `SECURITY.md`(취약점 신고·지원 범위), `CODE_OF_CONDUCT.md`
  (Contributor Covenant 2.1), `GOVERNANCE.md`(역할·의사결정·릴리스·품질관리)
- **GitHub 메타데이터** — 이슈 템플릿(bug/feature/question + config), PR 템플릿,
  `dependabot.yml`(cargo·npm×2·actions 주간 업데이트)
- `xazz-server/Cargo.toml`에 `license.workspace` 누락 수정 (SBOM 라이선스 식별)

### Fixed — 보안 권고 및 문서 수치 정합성

- `anyhow` 1.0.102 → 1.0.104 (RUSTSEC-2026-0190 unsound 패치).
  `cargo deny check advisories` 통과
- 벤치마크 수치를 최신 측정으로 통일: README / README_kr / ROADMAP / 결과보고서 모두
  228K 1.39× · 912K 1.95× · 4.09M 1.39× (기존 2.62×/1.93× 혼재 제거)

### Fixed — 같은 tenant 동시 실행 DP 사전검사 원자성 (issue #59, Track C2)

- **per-tenant 실행 락** — `AppState`에 tenant별 async Mutex를 두고 `POST /execute`의
  precheck → run → accrue 구간을 직렬화. 같은 tenant의 동시 실행이 같은 `remaining`을
  읽어 envelope를 초과하던 경합 제거. 다른 tenant는 서로 영향 없음(테넌트별 락)
- 검증: 락 동일성/tenant 간 비공유/try_lock 직렬화 테스트. xazz-server 50 tests pass

### Added — DP 예산 리셋 / 윈도우 API (issue #59, Track C2)

- **`POST /dp/budget/reset`** — 인증된 tenant의 누적 ε/δ spend를 0으로 초기화하고 window를
  재시작. tenant 스코프(self-service)로 다른 tenant 원장은 불변
- **`XAZZ_TENANT_DP_WINDOW_SECS`** — 0(기본)은 기존 누적 한도, 양수면 슬라이딩 window.
  window 경과 시 `dp_spent`/`add_dp_spend`가 원장을 0으로 롤하고 새 window를 anchor
- **`GET /dp/budget`** — `window_secs` / `window_started_at` / `resets_at`(자동 리셋 시각) 추가
- `dp_budget` 스키마에 `window_started_at` 컬럼(기존 DB 마이그레이션 포함)
- 검증: 리셋 tenant 격리, window 롤오버, window 비활성 시 누적 유지, env 파싱.
  xazz-server 49 tests pass

### Added — per-tenant 정책 팩 namespace 격리 (issue #59, Track C2)

- **SQLite `tenant_policies` 테이블** — tenant별 정책 팩 JSON 저장. 쓰기 시
  `Policy::from_json_str`로 검증하고, 한 tenant의 팩은 다른 tenant에 적용되지 않음
- **`PUT /security/policy` / `DELETE /security/policy`** — 인증된 tenant가 자기 namespace의
  정책 팩을 설정/삭제(self-service), `GET /security/policy`는 tenant의 유효 정책과 origin 반환
- **`guardrail::load_policy_for` / `gate_for`** — tenant 팩 우선, 없으면 전역
  (`XAZZ_POLICY_PATH` / `xazz.policy.json`) / builtin으로 폴백. tenant 팩 파싱 실패는
  fail-closed(실행 거부)
- `POST /execute` · `/security/policy/check` · `/security/remediate`가 tenant 정책을 적용
- 검증: 저장소/가드레일/엔드포인트 tenant 격리, 잘못된 팩 4xx 거부, 손상 팩 fail-closed.
  xazz-server 45 tests pass

### Added — per-tenant DP 예산 격리 (issue #59, Track C2)

- **per-tenant DP 누적 원장** — SQLite `dp_budget` 테이블(tenant별 `spent_epsilon`/`spent_delta`).
  한 tenant의 DP 소비가 다른 tenant에 영향을 주지 않음
- **실행 전 남은 예산 주입** — `POST /execute`가 tenant의 누적 소비를 조회해 남은 ε/δ를
  `XAZZ_DP_BUDGET`/`XAZZ_DP_DELTA_BUDGET`로 러너에 전달. 실행 후 `[xazz:dp]`의 `budget_spent`를
  원자적 UPSERT로 누적 → 런 간 순차 합성(composition) 강제
- **`GET /dp/budget`** — tenant의 spent/total/remaining ε·δ 조회 (tenant 스코프)
- envelope는 `XAZZ_TENANT_DP_BUDGET`/`XAZZ_TENANT_DP_DELTA_BUDGET`로 설정(기본 ε=10, δ=1e-4)
- 검증: 원장 누적·tenant 격리, envelope/마커 파싱, 엔드포인트 격리 테스트. xazz-server 39 tests pass

### Added — 모델 지문 감사 체인 연동 (issue #73, Track F4/F5)

- **`AuditRecord.model_fingerprint`** — SHA-256 모델 가중치 지문을 코드·프롬프트·응답 해시와
  함께 감사 레코드에 기록. `record_hash`에 포함되어 지문 변조 시 `verify()`가 실패
- **`POST /security/inference/check`** — 선택적 `model_fingerprint` 필드 수용, 응답에 에코.
  미지정 시 기존과 동일(필드 생략)으로 하위 호환
- 검증: `append_inference_call_records_model_fingerprint` + `inference_check_records_model_fingerprint`
  (지문 기록·변조 탐지·체인 검증). xazz-server 35 tests pass

### Added — ML 컴퓨트 백엔드 추상화 (issue #62 · #63 · #73, Track D1/D2/F4)

- **`xazz-exec/src/backend.rs`** — `ComputeBackend` trait을 `MLOp` 하향 경계에 도입.
  `runtime`이 `dl::train/predict`를 직접 부르지 않고 `backend::active()`를 통해 디스패치한다.
  Burn(ndarray CPU)이 첫 provider이며, burn-engine/ONNX Runtime은 동일 trait 뒤에서 교체 가능
- **`XAZZ_BACKEND`** 환경변수 선택(`cpu`|`cuda`|`wgpu`|`onnx`, 엔진 별칭 허용). 바이너리에
  포함되지 않았거나 알 수 없는 값이면 **명시적 경고와 함께 CPU로 폴백** (무단 디바이스 전환 없음)
- Cargo feature `cuda`/`wgpu`/`onnx` — 실기 provider는 스캐폴드. 각 acceptance 테스트는
  `#[ignore]`로 게이트: `cargo test -p xazz-exec --features cuda -- --ignored`
- 검증: trait 경유 CPU 학습·예측 왕복, resolver 폴백/별칭, `--all-features` 컴파일, 전체 테스트 통과
  (GPU/ONNX 실기 검증은 하드웨어/외부 릴리스에 게이트 — 이슈 참조)

### Added — 조기 종료 (early stopping) (issue #64, Track D3)

- **`train(..., validation_split: 0.3, patience: N)`** — 검증 손실이 N epoch 동안 개선되지 않으면
  학습을 조기 종료. 학습 리포트에 `stopped_early`(bool) + `best_epoch`(1-based) 추가
- 검증 분할이 없으면 patience는 무시 (조기 종료는 검증 손실 기반)
- E2E 검증: plateau에서 다음 epoch에 종료, `stopped_early:true`·`best_epoch` 기록
- CNN/임베딩 레이어·하이퍼파라미터 스윕·체크포인트 버전닝은 후속 (D1 GPU 작업과 연동)

### Added — 정책 팩 · stdlib 레지스트리 (issue #68, Track E4)

- **`xazz registry list`** — 사용 가능한 정책 팩·stdlib 모듈 목록 (오프라인 내장)
- **`xazz registry show <name>`** — 항목 상세 + 내용 출력
- **`xazz registry install <name>`** — 프로젝트에 설치:
  - 정책 팩(healthcare/finance/public-sector) → `xazz.policy.json` (자동 로드되는 활성 정책 경로)
  - stdlib 모듈(common/math/models) → `std/<name>.xzz` (프로젝트 로컬 커스터마이즈용)
  - `--out PATH` 대상 지정, `--force` 덮어쓰기. 기존 파일 보호(기본 거부)
- 3개 정책 팩·3개 stdlib 모듈을 CLI에 임베드 → 네트워크 없이 동작. 향후 원격 레지스트리는 동일한
  명령 표면·매니페스트로 확장
- 검증: 설치한 healthcare 팩이 `patient_id`를 차단, overwrite 가드/미지원 항목 오류
- 테스트 3건: 항목 유일성, 정책 팩 JSON 유효성, 기본 설치 경로

### Added — 표준 라이브러리 (`xazz-stdlib`, issue #56, Track B2)

- **`import "std/<name>"`** — 임베디드 표준 라이브러리 모듈. `xazz-stdlib/`의 `.xzz` 소스를
  `include_str!`로 컴파일러에 내장해 파일시스템 설정 없이 어디서나 해석
- 모듈: `std/common`(TimeSeries·Measurement·AirQuality·Regression 스키마),
  `std/math`(Stats 타입, Linear·SmallMLP 모델), `std/models`(LinearRegressor·MLPSmall·
  MLPMedium·MLPDeep 아키텍처)
- 모듈 시스템(B1) 재사용 — type/model/v 선언이 import 지점에 인라인, 사이클 감지·중복 검사 유지
- 함수 추상화가 아직 없어 재사용 단위는 **타입·모델** 선언. 날짜/문자열/통계 *연산자* 확장은 후속
- 예약어(mean/std/min/max/count 등)는 필드명으로 사용 불가 — 문서화
- 데모: `examples/stdlib_import.xzz` — `std/common`+`std/models` import 후 `MLPSmall` 학습
  end-to-end
- 테스트 3건: stdlib 해석(공통/수학), 미지원 모듈 오류

### Added — GitHub Actions 공식 액션 (issue #67, Track E3)

- **`.github/actions/xazz` composite action** — Rust 툴체인 설치 + cargo 캐시 + CLI 빌드 후
  `check`/`policy`/`run`을 파일 또는 디렉토리(모든 `.xzz`)에 실행. non-zero exit 시 잡 실패.
  `policy-path` 입력으로 `XAZZ_POLICY_PATH` 전달
- **`.github/workflows/policy.yml` policy 게이트** — ① demo 파이프라인 `check`,
  ② safe 파이프라인 policy 통과, ③ unsafe 파이프라인이 **차단됨을 단언**(차단이 성공 조건),
  ④ demo 파이프라인 end-to-end `run`
- 로컬에서 4단계 모두 검증 (PASS/PASS/BLOCKED/PASS)

### Added — 공식 Docker 이미지 (issue #66, Track E2)

- **`Dockerfile`** (멀티스테이지) — Rust 바이너리(`xazz`/`xazz-runner`/`xazz-server`) 빌드 →
  Visual IDE 프론트엔드 빌드 → slim 런타임(비루트 사용자, `/app/web`, `VOLUME /data`, `EXPOSE 8005`)
- **`docker-compose.yml`** — 빌드·실행·데이터 볼륨·테넌트/토큰 env 예시
- **`XAZZ_BIND` env** — 서버 바인드 주소 설정 (기본 루프백, 컨테이너에서는 `0.0.0.0:8005`)
- `.dockerignore`로 빌드 컨텍스트 축소 (target/node_modules/dist 제외)
- 실행: `docker compose up --build` → http://127.0.0.1:8005

### Added — VS Code 확장 (issue #65, Track E1)

- **`vscode-xazz/`** — `.xzz` 언어 지원 확장:
  - **LSP 클라이언트**: `xazz-lsp`를 stdio로 실행 — 진단·hover·go-to-def·rename (B3 재사용)
  - **명령**: `Xazz: Run Pipeline` / `Xazz: Check` — 활성 파일에 `xazz` CLI 실행, 출력 채널 스트림
  - **문법 강조**: TextMate 문법 (`syntaxes/xazz.tmLanguage.json`)
  - **바이너리 자동 탐색**: `xazz.lspPath`/`xazz.cliPath` 설정 → `XAZZ_LSP_PATH`/`XAZZ_PATH` env
    → 워크스페이스 `target/{debug,release}` → `PATH`
- `npm install && npm run compile`로 TypeScript 컴파일 검증 (`out/extension.js` 생성)
- `vsce package`로 `.vsix` 패키징 가능

### Added — LSP rename (issue #76, Track B3)

- **`textDocument/prepare_rename` + `textDocument/rename`** — 심볼 테이블의 모든 참조(정의 +
  사용)를 모아 파일 전체 rename을 TextEdit으로 반환
- `initialize`가 `renameProvider: true` 광고
- stdio E2E 검증: `mydata` → `my_data`가 정의·참조 각각의 위치에 정확한 편집 생성
- 테스트 2건: 참조에 정의+사용 포함, rename 편집이 모든 발생부를 커버

### Added — 파이프라인 카탈로그 & 컬럼 리니지 (issue #60, Track C3)

- **`xazz-compiler::catalog`** — Typed IR를 쿼리해 파이프라인 카탈로그 + 컬럼 리니지 생성:
  - 카탈로그: 각 파이프라인의 id/이름, 입출력 컬럼
  - 리니지: 출력 컬럼이 어떤 소스 컬럼에서 왔는지 — `Select`/`Rename`/`WithColumn`(표현식 참조
    컬럼)/`GroupBy`+`Aggregate`(집계 컬럼 ← 원본) 추적, `Filter`/`DropNull`/`FillNull`/`Cast`/
    `Sort`/`Limit`/`Sample`/`Replace`는 패스스루
- **`POST /catalog {code}`** — 코드를 컴파일해 카탈로그 반환 (실행과 동일한 Typed IR 사용 —
  리뷰어가 `groupBy → agg → chart` 출력 컬럼을 소스까지 추적 가능)
- 테스트 3건: select+rename 리니지, groupBy+agg 소스 추적, withColumn 파생 컬럼
- E2E 검증: `by_station` 출력 `pm10_rank ← pm10`, `pm10 ← pm10`, `station ← station`

### Added — 인증 & 다중 테넌시 (issue #59, Track C2)

- **토큰 인증**: `XAZZ_SERVER_TOKEN`(단일) + `XAZZ_TENANT_TOKENS`(`tenant1=token1,tenant2=token2`)
  — 다중 테넌트 모드는 `X-Xazz-Tenant` 헤더 + Bearer 토큰 검증. 누락/불일치 시 401
- **테넌트별 실행 격리**: `runs.tenant` 컬럼 추가(기존 DB 마이그레이션 포함), `/execute` 기록에
  인증 테넌트 태깅, `/runs`·`/runs/:id`가 해당 테넌트로만 스코프 — 타 테넌트 run 조회 시 404
- 미들웨어가 테넌트를 request extension에 주입, 핸들러가 읽음
- 테넌트별 정책 팩·DP 예산 네임스페이스 격리는 후속 (run 격리는 완료)
- E2E 검증: tenant-a/b 각각 자신의 run만 조회, cross-tenant GET /runs/:id → 404, 단일 토큰
  모드 401/200
- 테스트: store 테넌트 격리 1건 + 서버 통합 33건

### Added — Python 바인딩 (issue #61, Track C4)

- **`python/xazz/` 패키지** — 순수 Python 어댑터로 CLI를 호출: `xazz.check(src)`,
  `xazz.run(src)`, `xazz.policy(src)`. Rust 컴파일러/런타임이 단일 진실 — `xazz.check`가
  CLI와 동일한 진단을 byte-for-byte 반환
- 바이너리 탐색: `XAZZ_PATH` / `set_xazz_path` → `target/{debug,release}/` → `PATH`
- PyO3 네이티브 확장은 python3-dev 헤더 부재(무 sudo)로 연기 — 서브프로세스 브리지가
  동일한 계약 제공
- 테스트 5건: check clean/typo(did-you-mean), run rows, run error, policy report
- `python/README.md`에 사용법 문서화

### Added — 서버 런 영속화 (issue C1)

- **SQLite 저장 (`xazz-server`, `rusqlite` bundled)** — `xazz.db`의 `runs` 테이블에 각 실행 기록:
  id, code_hash, status(success/failed/blocked), rows, error, created_at
- **`GET /runs`** — 최신순 실행 목록 / **`GET /runs/:id`** — 단일 레코드 조회
- **`POST /execute` 응답에 `run_id` 포함** — 영속 레코드와의 연결
- 감사 SHA-256 체인은 기존 append-only JSONL 유지 (별도)
- 실행 history가 서버 재시작 후에도 유지 — 재시작 후 `/runs`에 이전 실행 잔존 검증
- 테스트: store 유닛 2건(기록·목록·단일 조회, 미존재 None) + 서버 통합 33건

### Added — LSP 네비게이션 · 심볼 테이블 (issue #75, Track B3)

- **`xazz-compiler::symbols` 심볼 테이블** — 토큰 스트림에서 변수/타입/모델 정의·참조 위치를
  1-based line:col로 수집 (`mut v`, `v`, `type`, `model` 선언 + 동일 이름 참조 추적)
- **`xazz-lsp` hover** — 커서 위치의 심볼에 대해 kind(변수/타입/모델) + 정의/참조 표시
- **`xazz-lsp` go-to-definition** — 참조에서 동일 이름 선언 위치로 점프
- stdio 스모크 테스트로 E2E 검증: hover `**variable** \`x\` (reference)`, goto가 선언
  line:col로 이동
- rename은 후속 (#75에 추적)
- 테스트: 심볼 인덱서 4건 + LSP 네비게이션 3건

### Added — PostgreSQL 소스 커넥터 (issue #54, Track A3)

- **`load("postgres://user:pass@host:port/db?sql=...")`** — PostgreSQL 쿼리 결과를 Polars
  파이프라인 소스로 사용
- 연결은 `postgres`(tokio-postgres) NoTls(TCP) — 로컬 개발 DB 가정. `?sql=` 외의 쿼리
  파라미터(예: `sslmode=disable`)는 연결 문자열에 그대로 전달
- 결과를 `Row::try_get` 타입별로 읽어 Polars Series로 변환 — int/float/str/bool 컬럼을
  올바른 dtype으로 구성
- `:: Type` 스키마 주석, filter/groupBy/mean 등 일반 파이프라인 연산과 연동
- `xazz run` / `xazz check` / `xazz sanitize` 모두 지원
- 테스트 3건: URI 파싱(?sql= / &sql=), 비-postgres 경로, 연결 실패 graceful
- 데모: `examples/duckdb/postgres_demo.xzz` (로컬 Postgres 필요)

### Added — DuckDB 소스 커넥터 (issue #54, Track A3)

- **`load("duckdb://...")`** — DuckDB(in-memory `:memory:` 또는 파일 DB) 쿼리 결과를 Polars
  파이프라인 소스로 사용. URI 형식 `duckdb://:memory:?sql=...` / `duckdb://data.db?sql=...`
- DuckDB `bundled` feature — 시스템 설치 불필요 (self-contained)
- 결과를 `ValueRef` 타입 태그로 읽어 Polars Series로 변환 — int/float(Decimal 포함)/str 컬럼을
  올바른 dtype으로 구성. UNION 등 동적 타입 혼합 컬럼도 자동 결정
- `:: Type` 스키마 주석, filter/groupBy/mean 등 일반 파이프라인 연산과 자연스럽게 연동
- `xazz run` / `xazz check` / `xazz sanitize` 모두 지원
- 참고: `COPY (...) TO parquet` 교환 경로는 이 bundled 빌드에서 세그폴트되어
  ValueRef 직접 읽기로 구현
- 데모: `examples/duckdb/hello_duckdb.xzz` · 단위 테스트 4건

### Added — LSP 서버 (`xazz-lsp`, issue #75, Track B3)

- **새 크레이트 `xazz-lsp`** (tower-lsp) — `xazz check` 체커를 그대로 재사용해 에디터에
  진단을 게시
- **진단 byte-for-byte 일치**: 열기/변경/저장 시 `xazz check`와 동일한 라인:컬럼 변환
  (1-based 체커 span → 0-based LSP position)으로 `publishDiagnostics` 전송
- **import 모듈 해석**: 문서 디렉토리 기준 `resolve_imports` 후 `analyze_program` — B1(#69)
  모듈 시스템 재사용
- hover / go-to-def / rename은 체커가 심볼 테이블을 export하기 전까지 후속 (#75에 추적)
- stdio 스모크 테스트로 검증: initialize 응답, didOpen → `xazz check`와 동일한 진단 발행
- `xazz check` 진단 변환 유닛 테스트 4건

### Added — 모델 프로비넌스 (issue #74, Track F5)

- **정책 레지스트리 (`allowed_models`)** — `{ id, license, fingerprint }` 항목. 외부 모델
  참조(`load("hf://...")` / `load("model://...")`)를 컴파일 타임에 검사
- **`XZP030 MODEL_LICENSE_BLOCKED`** — 레지스트리에 등록됐지만 `denied_licenses`에 있는
  라이선스(예: `Llama3-License`)는 웨이트 사용 거부
- **`XZP031 MODEL_PROVENANCE_UNKNOWN`** — 레지스트리에 없는 외부 모델은 fail-closed 차단
  (`require_model_provenance` 기본 true — "핑거프린트 미검증"은 안전하지 않음)
- 로컬 `model Name { ... }` 선언은 코드이므로 판정하지 않음 (기존 deep_learning 데모 영향 없음)
- 레지스트리 매칭은 대소문자·공백 정규화. 핑거프린트-감사 체인 연동은 F4(#73)에서
- 테스트 6건: 미등록 fail-closed · 등록+허용 라이선스 통과 · 거부 라이선스 차단 · 로컬 모델 비판정 ·
  일반 파일 비판정 · 정규화 매칭

### Added — 파인튜닝 데이터 정화 (issue #72, Track F3)

- **`xazz sanitize <file>`** — 파인튜닝 전 데이터 안전성 검사 (CSV/Parquet/Arrow). `--json`으로
  구조화 리포트(파인튜닝 인테이크 아티팩트) 출력
- **PII 스캔**: 셀 단위로 가드레일과 동일한 precision-first 스캐너 재사용 — 원본 값은 절대
  리포트에 없고 마스킹 샘플만 표시
- **중복 검사**: 정확 중복 행 비율 + 텍스트 컬럼별 정규화(공백 축소·대소문자 무시) 근접 중복 비율
  — `"  Summarize   the   report  "`와 `"summarize the report"`가 매칭됨
- **편향 검사**: 카디널리티 ≤50 범주형 컬럼에서 max/min 불균형 ≥10배 + 지배 범주 ≥50%면
  파인튜닝 편향 신호로 플래그, 상위 범주와 비율 보고
- `xazz-exec/src/sanitize.rs` 신규 — 실행 엔진에 구현되어 CLI는 runner IPC로 위임 (CLI 경량
  바이너리 원칙 유지)
- 테스트 6건: PII 마스킹 · 정확/근접 중복 · 불균형/균형 편향 · 클린 데이터 통과

### Added — GenAI 출력 게이트 (issue #71, Track F2)

- **`scan_output_text()`** — LLM 응답(자유 형식 텍스트) 전용 재스캔 함수 추가
  (`xazz-compiler/src/policy/patterns.rs`). 동일한 precision-first 스캐너(RRN 체크섬 · Luhn ·
  TLD · 토큰 프리픽스)를 적용하고, `GenericSecret`(`password = "..."` 형태)은 제외 —
  생성 텍스트가 자격증명을 *예시*로 언급하는 오탐 방지
- **감사 체인 확장 (`append_inference_call`)** — `AuditRecord`에 `prompt_hash`/`response_hash`
  필드 추가. 응답 텍스트는 절대 저장하지 않고 해시만 체인에 기록 — "이 프롬프트가 이 응답을
  만들었다"를 사후 증명하되 로그 자체는 유출원이 되지 않음
- **`POST /security/inference/check`** — 런타임 출력 게이트: 응답을 재스캔해 PII/시크릿이
  발견되면 `safe_to_emit: false` + 마스킹된 findings 반환, 호출을 감사 체인에 기록
  (`outcome: safe|blocked`). `GET /security/audit/chain`으로 체인 무결성 검증 가능
- 테스트: `scan_output_text` 오탐 방지 4건, 감사 체인 inference 기록 1건, 서버 통합 2건

### Added — GenAI 입력 게이트 (issue #70, Track F1)

- **프롬프트 리터럴 정적 스캔 (`XZP020`–`XZP022`)**: `prompt("...")` · `prompt: "..."` ·
  `prompt = "..."` 형태의 프롬프트 리터럴을 컴파일 타임에 스캔해 위험 패턴을 `line:col` 진단으로
  차단한다.
  - `XZP020 PROMPT_INJECTION` — 지시 우회 ("ignore all previous instructions" 계열)
  - `XZP021 PROMPT_JAILBREAK` — 안전장치 우회 (DAN · "do anything now" 계열)
  - `XZP022 PROMPT_EXFILTRATION` — 비밀·내부정보·개인정보 탈취 유도 ("reveal your system prompt" 계열)
  - 우선순위: Exfiltration > Jailbreak > Injection
- **오탐 방지 (precision-first)**: `prompt(...)` 형태의 리터럴만 스캔 — 데이터 파이프라인의
  일반 문자열(`filter(note == "ignore all previous instructions")`)은 미탐지. 대소문자·공백은
  정규화로 흡수. 식별자 일부(`xprompt`)는 제외
- **보고서 안전성**: 리포트에는 매칭된 위험 구문만 표시 — 전체 프롬프트 텍스트는 노출하지 않음
- **자동 보정 제외**: 프롬프트는 `--fix`로 자동 재작성되지 않고 `residual`로 남음 (의미 변경 방지)
- 규제 근거: NIST AI RMF(GEN-4.3) · 국가정보원 「생성형 AI 보안 가이드라인」 (audit-trail 참조)
- 데모: `examples/security/prompt_unsafe.xzz`(차단) / `prompt_safe.xzz`(통과)
- `xazz policy`/`xazz run` 게이트에 자동 적용 (3-게이트 인프라 재사용)

### Changed — 포지셔닝 전환 (Phase 0)

- **거버넌스 레이어로 재포지셔닝**: README/README_kr 태그라인을 "Rust 기반 AI 파이프라인
  DSL" → "안전한 파인튜닝 데이터 준비 + 추론 게이트 + 정적 보안 가드레일을 통합한
  AI 파이프라인 거버넌스 레이어"로 변경. DSL은 거버넌스를 실행 가능하게 하는 수단이지
  제품이 아니라는 명시적 입장
- **ROADMAP Track F (GenAI 거버넌스) 신설**: 프롬프트 입력 게이트(F1), LLM 출력
  재스캔 + 감사 체인(F2), 파인튜닝 데이터 정화(F3), burn-engine/ONNX 연동(F4),
  모델 프로비넌스(F5). burn-engine(Burn 0.22) 출시 시점과 연동 계획
- README 로드맵에 Phase 7 — GenAI 거버넌스 행 추가

### Added — 데이터 스케일 기반 (#52, #53, #55)

- **모듈 시스템 (#69)**: `import "path.xzz"` — 모듈 파일의 `type`·`model`·`v` 파이프라인
  선언을 import 지점에 인라인 병합. 상대경로 기준(importing 파일 기준), **사이클 감지는
  fail-closed**, 누락 파일/파싱 실패는 해석 오류. 체커는 병합된 AST를 그대로 검사하므로
  중복 선언·미선언 참조 검증이 모듈 간에도 동일하게 동작. 모듈 소스는 정책 가드레일의
  리터럴 스캔에도 포함. `xazz check`/`xazz run` 모두 지원
- **`save()` 출력 연산자**: 파이프라인 결과를 아티팩트 파일로 기록 — `save("out.csv")`,
  `save("out.parquet")`, `save("out.arrow", format: "arrow")`. 포맷은 확장자에서 추론하거나
  `format:` 인수로 명시 (미지원 포맷·미확정 포맷은 파싱 에러)
- **컬럼형 소스 로드**: `load()`가 확장자 기반으로 디스패치 — `.parquet`/`.pq`,
  `.arrow`/`.ipc`/`.feather`, 나머지는 기존 CSV 경로(EUC-KR 자동감지 + null 정규화) 유지
- Polars `parquet`/`ipc` features 활성화 (`xazz-exec`)
- **Out-of-core lazy 로드 (#53)**: 32MB 초과 소스는 `LazyFrame` scan(`scan_parquet`/`scan_ipc`/`scan_csv`)
  을 사용해 디스크에서 스트리밍 — 대용량 파일도 터미널 `.collect()` 전까지 전체 메모리에 적재하지 않는다.
  작은 파일(≤32MB)은 **eager 1회 로드 + 메모리 검증** 패스트 경로를 유지해 이중 디스크 읽기를 피한다 (적응형)
- **선택적 streaming collect (#53)**: `XAZZ_STREAMING=1` 로 강제하거나, 큰 소스에서 기본 활성 —
  Polars `Engine::Streaming` 사용, 지원하지 않는 계획은 in-memory 엔진으로 자동 폴백
- **벤치 스케일 확장 (#53)**: `make_scale_data.py --xlarge`로 200M 행 스케일 생성,
  `run_readme_benchmark.py --xlarge`로 측정. 벤치가 policy 가드레일을 통과하도록 상대경로 사용
- **벤치 측정 (2026-09-04, median of 3)**: 228K/912K/4.09M 행에서 각각
  **1.39× / 1.95× / 1.39×** vs pandas (556/1,054/4,344 ms vs 770/2,052/6,040 ms),
  4.09M 행에서 피크 RSS 570MB vs 656MB
- **`xazz import` 컬럼형 확장 (#55)**: `.parquet`/`.pq`/`.arrow`/`.ipc`/`.feather` 입력도
  스키마 추론 가능 — CLI는 Polars-free를 유지하므로 `xazz-exec --schema <file>`(via runner)
  에 위임. 컬럼명·dtype(정수/실수/불리언/문자열 매핑)·널 허용 여부(샘플 100행)로
  `type` 블록 + `load` 구문 생성, CSV와 동일한 프로젝트 root·중복 검사 로직 공유
- 통합 테스트: Parquet/Arrow save→load 왕복 (Schema cast 경유), streaming 엔진의
  벤치 파이프라인(벤치마크 shape) 실행·네이티브 연산 지원 검증, 컬럼형 스키마 추론 왕복

---

## [v0.3.1] — 2026-08-31

> **정확성·보안·국제화 정비 릴리스.** 언어 자체의 동작 버그와 보안 하드닝, 그리고
> 영어 기본 CLI 출력(XAZZ_LANG=ko 로 한국어 유지). README 예제가 이제 그대로 실행된다.

### Fixed — 언어 동작

- **`count()` 집계가 실제로 동작**: `groupBy("col") |> count()` 가 그룹별 행 수를,
  단독 `count()` 는 전체 행 수를 반환 (IR `AggKind::Len` 신설, 체커의 집계 누락 오탐 제거)
- **`select(["col", ...])` 문자열 리터럴 허용**: 컬럼 리스트에서 bare ident 와 문자열 모두 수용 — README 예제가 수정 없이 실행됨
- **`Option<T>` 널 안전성 강제**: non-nullable 로 선언된 컬럼에 `fillNull` 을 쓰면 컴파일 타임 오류 (스키마 수정 제안 포함)
- **Dropout 이 추론에서도 적용되던 버그**: `Mlp` 에 training 플래그를 두어 `predict()`/검증 경로에서 비활성화
- did-you-mean 제안이 현재 파이프라인 스키마 기준으로 정확히 나오도록 개선 (available columns 가 비던 문제)

### Security

- **차트 HTML Stored XSS 방지**: 인라인 JSON의 `<` `>` `&` U+2028/29 를 `\uXXXX` 이스케이프 (`</script>` 탈출 차단, 테스트 추가)
- **DP 노이즈 시드를 OS 엔트로피로**: 시간 기반 시드의 노이즈 역산 공격 차단 (`/dev/urandom`)
- **`xazz-server` CORS를 루프백 오리진으로 제한** + 선택적 `XAZZ_SERVER_TOKEN` Bearer 인증 — 임의 웹페이지의 로컬 파일 탈취/실행 차단

### Changed — 국제화 (i18n)

- **CLI 출력이 기본 영어**: `xazz check/run/policy`, 컴파일러 진단, 런타임 로그, 정책 리포트 전반. `XAZZ_LANG=ko` 로 한국어 유지
- README 스크린샷 4장을 영어 출력으로 재캡처 (`demo/capture.sh` 에 오타 감지 시나리오 추가, `demo/make_screenshots.py` 신규)
- 문서: `SECURITY_GUARDRAIL.md`, `dp-spec.md` 영어화, 벤치마크 수치/방법론 공개 정비, README 예제 교정

### Removed

- 실행 시 실패하던 데드코드 제거: `xazz run --predict`(NQP) 플래그와 `predict.rs`

---

## [v0.3.0] — 2026-08-29

> **Typed IR 컴파일러 아키텍처 마일스톤.** 이번 릴리스는 "기능 추가"가 아니라
> **핵심 컴파일러 추상화 도입**에 초점을 맞췄다. AST와 백엔드(Polars/Burn) 사이에
> 정적 타입이 부착된 중간 표현(Typed IR)을 두어 기존의 **이중 해석 구조**를 제거했다.
>
> 변경 전: 컴파일러가 문자열 codegen만 만들고, 런타임(`xazz-exec`)이 소스를
> **다시 렉싱·파싱**해 raw AST를 직접 Polars/Burn에 해석.
> 변경 후: `Lexer → Parser → AST → (정적 분석) → Typed IR → (최적화) → lowering → 백엔드`.
> 컴파일러가 구조화된 IR을 1회 생성하고, 런타임은 이를 1회 소비한다.

### Added — Typed IR / 컴파일러 (이슈 #37~#45)

- **Typed IR** (`xazz-core::ir`): `ColType`/`Schema`/`TypedExpr`(모든 표현식이 결과 타입 보유), `DataOp`(데이터), `MLOp`(학습/예측), `SideOp`(차트/DP), `Step`(순서 보존 태그), `PipelineNode`, `TypedProgram`
- **이중 해석 제거**: 타입체커가 검사와 함께 IR을 **단일 순회**로 생성 (`analyze_program`/`compile_ir`). `xazz-exec`는 raw AST 대신 IR을 소비 (기존 `execute_var_decl`/`handle_train_stmt` 등 구 인터프리터 -606줄 삭제)
- **IR 최적화 계층** (`xazz-compiler::opt`): 상수 폴딩, 연속 `Select` 병합(projection 축소), 조건 푸시다운(`filter`를 `select` 앞으로). Polars 실행 동치 테스트로 정당성 검증. `xazz-exec --opt` 로 선택 활성화
- **zero-copy 텐서 브리지**: 연속(단일 청크·무결측) `Float64/Float32` 컬럼은 Arrow raw 버퍼(`cont_slice`)를 직접 읽어 컬럼별 중간 `Vec` 복사 제거. 불가피한 복사 경계(f64→f32 강등, columnar→row-major, host→device)는 모듈 주석에 명시
- **DP 조성 회계**: `PrivacyBudget`가 ε 단순 합산 대신 `(ε, δ)` 병행 누적(기본 순차 조성). Laplace는 순수 ε-DP(δ=0), Gaussian은 δ도 소모. `XAZZ_DP_DELTA_BUDGET` 신설. `[xazz:dp]` 마커에 `budget_spent_delta`/`total_delta`/`query_count` 추가
- **실행 타임아웃 하드닝**: `xazz-runner`가 서브프로세스 최대 실행 시간 제한(기본 300초, `XAZZ_EXEC_TIMEOUT_SECS`), 미세 프로세스 격리 ≠ OS 샌드박스 명시
- **God runtime 해체**: `xazz-exec`를 도메인별 모듈로 분리 — `lower`(DataOp→Polars), `dl`(Burn), `dp`, `chart`(시각화), `runtime`(얇은 오케스트레이션, 1433줄 → 720줄)
- 전 크레이트 버전 통일: workspace 단일 버전(0.3.0), `xazz-server`도 workspace 버전·edition(2024) 흡수, 내부 path 의존성 version 하드코딩 제거

### Added — (이전 [Unreleased] 기능 전부, v0.3.0 에 포함)

- **Policy-as-Code 정적 보안 가드레일** (`xazz-compiler/src/policy/`, issue #2): `.xzz` 파이프라인이 실행되기 전에 개인정보 유출·보안 컴플라이언스 위반을 정적으로 탐지·차단
  - 규칙 12종: 직접 식별자 노출(`XZP001`), 민감 속성 행 단위 노출(`XZP002`), 준식별자 결합 재식별(`XZP003`), 민감 집계 DP 미적용(`XZP004`), ε 상한 초과(`XZP005`), PII/비밀키 하드코딩(`XZP010`·`XZP011`), 민감 경로 접근(`XZP012`), 경로 탈출(`XZP013`), 스키마 미해석(`XZP014`), 파싱 실패(`XZP000`), 정책 로딩 실패(`XZP999`)
  - **출력 컬럼 추론**(`PipelineShape`): 집계 결과 컬럼을 식별자와 구분해 정상 통계 쿼리의 오탐을 제거 (`groupBy("region") |> count("patient_id")` 는 통과)
  - **리터럴 스캐너**: 주민등록번호 체크섬·신용카드 Luhn+IIN·API 키 접두사·PEM 개인키 검증. 정규식 크레이트 없이 구현해 CLI 경량성 유지, 탐지값은 항상 마스킹해 보고
  - **fail-closed**: 정책 로딩 실패·파싱 실패는 실행 허용이 아니라 실행 거부
  - **Policy-as-Code JSON**: `XAZZ_POLICY_PATH` 또는 `xazz.policy.json` 으로 컬럼 분류·임계치·ε 상한·규칙별 심각도를 교체
  - **Domain Policy Pack 3종**: 의료(`healthcare_policy.json`) · 금융(`finance_policy.json`) · 공공(`public_sector_policy.json`)
  - **감사 증빙(Compliance Evidence)**: 위반마다 `rule_id` · `source_ref` · `policy_version` · `domain` · `risk_level` 기록
- **3중 실행 게이트**: CLI(`xazz run`) · 실행 엔진(`xazz-exec` STEP 3.6) · API 서버(`POST /execute`)
- **자동 보정 (결정적)** (`policy/remediate.rs`) + **AST → `.xzz` 프린터** (`policy/printer.rs`, 왕복 파싱 보장)
- **온프레미스 sLM 보정 어댑터** (`xazz-server/src/slm.rs`): Qwen2.5-Coder-1.5B (Ollama), sLM 제안은 같은 정책 엔진 재검증 통과 시에만 채택
- 신규 CLI `xazz policy` / 신규 API `GET /security/policy`, `POST /security/policy/check`, `POST /security/remediate` (위반 시 HTTP 422)
- `[xazz:policy]` stdout 마커, **sLM 학습·평가 스캐폴드** (`experiments/slm_guardrail/`), 보안 예제 (`examples/security/`)
- 문서: [`docs/SECURITY_GUARDRAIL.md`](docs/SECURITY_GUARDRAIL.md)
- **Burn 딥러닝 실행 엔진** (`xazz-exec/src/dl.rs`): `model {}` → `train()` (Adam + MSE), 특성 표준화, train/validation 분할, in-sample 예측, 체크포인트 저장
- **train/predict 파이프라인 연산자**: `train(Model, ...)` / `predict(model, as: "col")`, `TrainedModel` (모델 + 표준화 통계) 바인딩, 레거시 `run |> train` 유지, `ModelDecl` 위치 무관 사용
- `emit rust` 딥러닝 코드 생성
- **정적 의미 분석기 (Type Checker)** (`xazz-compiler/checker.rs`): 미선언 변수/모델/스키마, 스키마 컬럼 존재성(`SafeLoadViolation` + Did-you-mean), groupBy→집계 누락, 문자열 집계 경고, train/predict 참조 검증. `xazz check` CLI 교체. 실행 전 preflight + `[xazz:diagnostics]`
- **정적 진단 소스 위치(Span)**: `check_source`가 명령문 단위 토큰 분할로 `[N행:M열]` 위치 표시
- **감사 로그** (`xazz-server/audit_log.rs`): SHA-256 append-only JSONL + `/security/audit`, `/security/verify`
- `DivisionByZero` 검출, `xazz run --json`, 서버 `/execute` → `[xazz:train]` 파싱, `diagnostics` 필드
- 신규 연산자: `sample(n)`, `median()`, `variance()`, `std()`
- **Visual IDE** (Vite + React + @xyflow/react): 컴파일러 캔버스(Burn ML 단계), Monitoring 뷰, 실서버(`xazz-server`) 연결, 크로스플랫폼 실행
- 테스트/CI: checker 유닛 20개+IR 테스트, 폴리시 실행 통합테스트, 클리피 강제, 프런트엔드 빌드 잡

### Changed

- **컴파일러 파이프라인**: AST → Typed IR → (최적화) → lowering 구조로 전환 (문자열 codegen 실행 경로 제거)
- 프로젝트명/패키지 리브랜딩: `x1zz` → `xazz` (README, 크레이트명, CI 전반)
- `chart { type: ... }` 필드명 통일 (`kind` → `type`)
- 예제를 목적별 디렉토리로 재구조화: `examples/{data, deep_learning, end_to_end, preprocessing, visualization}`
- 리포지토리 재구조화: 문서를 `docs/` 하위로 통합, `ui-prototype` → `visual-ide` 리네이밍
- README 기능/로드맵 상태 테이블을 실제 구현 상태로 정리

### Fixed

- `LayerKind::to_burn_str` Dense 매핑 오류 (입력 차원 스키마 기반 추론)
- `PipelineOp`에 `Train`/`Predict` variant 누락 컴파일 에러
- checker/emitter/integration 테스트 입력 문법 오류 수정
- CI `cargo fmt`/클리피 회귀, CI 컴포넌트 설치 오류
- `<0.3.0` 내부 path 의존성 버전 불일치 (workspace 단일 버전으로 통일)

### Architecture

- `xazz-core`: `ir` 모듈 (Typed IR) 신설
- `xazz-compiler`: `checker` → IR 생성, `opt` 최적화 패스 신설
- `xazz-exec`: `lower`/`dl`/`dp`/`chart` 도메인 분리, IR 단일 소비
- `xazz-runner`: 실행 타임아웃 하드닝
- 전 워크스페이스 버전·edition 통일

---

## [v0.2.8] — 2026

### Changed
- CI: removed macOS x64 release target (arm64 only)

---

## [v0.2.7] — 2026

### Fixed
- CI: removed bash-only shell command from Windows packaging step

---

## [v0.2.5 / v0.2.4] — 2026

### Fixed
- CI: stabilized multi-platform packaging and archive validation

---

## [v0.2.3] — 2026

### Fixed
- Cargo workspace configuration
- CI pipeline fixes

---

## [v0.2.2] — 2026

### Added
- GitHub Actions release pipeline (`.github/workflows/release.yml`)
- Multi-platform build matrix: Windows x64, Linux x64, macOS arm64
- Automated archive packaging and checksum generation

---

## [v0.2.1] — 2026

### Added
- Initial release pipeline
- Binary separation: `xazz` CLI + `xazz-runner` + `xazz-exec`

---

## [v0.2.0] — 2026

### Added
- MVP release
- `xazz new` — project scaffolding with sample CSV
- `xazz import` — CSV schema auto-inference (EUC-KR/CP949 support)
- `xazz run` — pipeline execution via `xazz-runner` subprocess
- `xazz emit rust` — transpile `.xzz` to Rust (Polars LazyFrame)
- `xazz check` — experimental NQP static analysis stub
- `xazz sde` — synthetic data engine integration stub
- Chart visualization: `chart {}` block (bar, line, pie, scatter)
- Pipeline operators: `filter`, `groupBy`, `join`, `withColumn`, `cast`, `rename`, `sort`, `select`, `mean`, `fillNull`
- `Option<T>` null-safe type system
- Dependency isolation: Polars removed from CLI binary, isolated to `xazz-exec`
- Multi-crate workspace: `xazz-core`, `xazz-compiler`, `xazz-exec`, `xazz-runner`, `xazz-server`
- CSV LFS migration for large example data files
- Benchmark: up to 2.62× speedup over pandas at 228K rows; 1.93× at 4.09M rows (see README Performance)

### Architecture
- `xazz` CLI binary: no Polars, no Tokio (~2–5 MB)
- `xazz-runner` spawned as subprocess for pipeline execution
- `xazz-exec` carries Polars LazyFrame runtime (~30+ MB)

---

*Earlier development history is available via `git log`.*