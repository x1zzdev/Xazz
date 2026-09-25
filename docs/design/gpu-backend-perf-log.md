# GPU 백엔드 성능 정리 — 작업 로그 (NEXT #74/#75/#86 + ONNX GPU EP)

> 대규모 GPU 검증(wgpu / CUDA / ONNX GPU)에 앞서, 벤치 수치를 왜곡하던
> 핫패스를 정리하고 ONNX GPU 실행 경로를 구성한 작업의 과정·근거·검증 기록.
> 구현은 `xazz-exec` 크레이트.

---

## 1. 배경과 목표

여러 환경(Windows 노트북: RTX 4070 Laptop 8GB + Intel Arc iGPU / macOS M5)에서
`XAZZ_BACKEND=wgpu|cuda|onnx`를 실측하기 전, 현재 상태가 "컴퓨트 시간"을
측정하는지 "I/O·초기화·export"를 측정하는지 점검했다.

**진단 요약** — 골격(`MLOp` 경계의 `ComputeBackend` trait, 백엔드 중립 체크포인트,
fail-closed 폴백, 동일 체크포인트 패리티 검증)은 타당하다. 그러나 반복 호출 경로에
다음 비용이 섞여 있었다.

| # | 문제 | 위치(변경 전) | 벤치 영향 |
|---|---|---|---|
| A | GPU `predict`가 매 호출 체크포인트를 디스크에서 재로드 | `dl::predict_on_device` | 추론 반복의 최대 병목 |
| B | ONNX `predict`가 매 호출 `.onnx` 재export + `Session` 재생성 | `onnx_export::predict` | 추론 1회에 export+로드 비용 |
| C | WebGPU device 부재 시 폴백 없음 (`Device::default()` 패닉 가능) | `WgpuBackend` | headless/CI에서 즉시 실패 |
| D | device를 매 호출 재생성/재probe | `CudaBackend`/`WgpuBackend` | 호출당 고정 오버헤드 |

목표: **반복 추론이 웨이트를 한 번만 로드/export**하고, **device는 선택 시 1회
probe 후 CPU로 안전하게 폴백**하도록 정리한다. (학습 자체의 디스크 왕복, 스윕,
ONNX GPU EP 구성은 이번 범위 밖 — 6절 후속 참조.)

---

## 2. 변경 내용

### 2.1 추론 캐시 키 — `dl::ArtifactKey` (`dl.rs:692`)

경로 + mtime(나노초) + 파일 길이로 구성한 값. 재학습으로 같은 경로의 파일이
바뀌면 키가 달라져 캐시가 자동 무효화된다. 파일이 없으면 mtime/len = 0인 키가
되어 실제 파일 키와 절대 일치하지 않는다(→ 로드 시 실제 "not found" 오류 노출).

### 2.2 로드/추론 분리 (`dl.rs:1369`, `dl.rs:1404`)

- `load_inference_model<B>(trained, device) -> Mlp<B>` — 매니페스트 검증(신버전
  fail-closed) 후 `B`로 그래프 재구성 + 체크포인트 로드, `training=false`.
- `predict_with_model<B>(trained, model, device, df, as_col)` — **디스크 접근 없이**
  전처리 → forward → 예측 컬럼 부착.
- `predict_on_device`는 위 둘을 합친 것과 동일(하위호환, 캐시 없는 경로).

### 2.3 wgpu / cuda 모듈 캐시 (`backend.rs:339`, `backend.rs:459`)

`WgpuBackend`/`CudaBackend`가 `Mutex<Option<(ArtifactKey, Mlp<B>)>>` **단일슬롯
캐시**를 보유. `predict`는 키가 같으면 로드된 모듈을 재사용하고, 다르면 재로드한다.
device는 생성 시 1회 해석해 구조체에 보관 → `train`도 명시 device로 실행된다
(`train_on_device`). `active()`가 프로세스 전역 `OnceLock`이라 캐시 수명 = 프로세스.

### 2.4 ONNX export/Session 캐시 (`backend.rs:590`, `onnx_export.rs:393`)

- `ensure_export` — `.onnx`가 체크포인트보다 최신이면 재export 생략.
- `load_session` / `predict_with_session` — 세션 생성과 실행을 분리.
- `OnnxBackend`가 `ArtifactKey(onnx)` 기준으로 `Session`을 단일슬롯 캐시.
- `init_runtime`을 `pub`로 승격(세션 캐시 경로에서 1회 초기화).

### 2.5 WebGPU device 선택 + probe/폴백 (`backend.rs:273`, `backend.rs:498`)

- `XAZZ_WGPU_DEVICE` 문법: `default`|`auto`|`cpu`|`dgpu[:N]`|`igpu[:N]`|`vgpu[:N]`
  (`DiscreteGpu(N)` 표기 허용, 대소문자 무시). 파서는 feature 비의존
  `WgpuDeviceSpec`으로 분리해 단위 테스트 가능.
- `probe_device` — cubecl에 비패닉 어댑터 probe가 없어, 최소 텐서 연산을
  `catch_unwind`로 감싸 어댑터 부재 패닉을 오류로 변환한다(패닉 훅은 잠시 무음).
  provider 생성 시 1회만 실행되므로 전역 훅 교체가 동시 GPU 작업과 경쟁하지 않는다.
- `resolve()`/`build()` — `build`가 `Result`를 반환하도록 바꾸고, 컴파일됐지만
  device가 없는 경우 `device_warning`과 함께 CPU로 폴백한다. CUDA도 생성 시
  probe로 이동해 "선택 시점 폴백"으로 통일.

---

## 3. 검증

| 구성 | 명령 | 결과 |
|---|---|---|
| default | `cargo clippy -p xazz-exec --all-targets -- -D warnings` | 통과 |
| wgpu | `cargo clippy -p xazz-exec --features wgpu --all-targets -- -D warnings` | 통과 |
| cuda | `cargo clippy -p xazz-exec --features cuda --all-targets -- -D warnings` | 통과 |
| onnx | `cargo clippy -p xazz-exec --features onnx --all-targets -- -D warnings` | 통과 |
| workspace | `cargo clippy --workspace --all-targets -- -D warnings` | 통과 |
| 테스트 | `cargo test -p xazz-exec` | 74 + 12 통과 |
| wgpu 실기 | `cargo test -p xazz-exec --features wgpu -- --ignored` | `wgpu_matches_cpu_losses` 통과 (lavapipe 소프트웨어 Vulkan) |

추가 단위 테스트: `parse_wgpu_device` 별칭/인덱스/오류, `artifact_key` 부재/변경
무효화, CUDA device 부재 시 폴백.

> 한계: WSL은 `ort` prebuilt C++ 정적 링크(zig C++) 문제로 ONNX **실기 실행**은
> 불가하다(NEXT.md). CUDA는 2026-09-25 `burn-cuda`(네이티브 CubeCL)로 교체되어
> LibTorch ABI 제약이 사라졌다 — `check`/`clippy`는 전 조합 통과했고,
> 실기 acceptance는 CUDA 드라이버가 있는 호스트에서 `-- --ignored`로 실행한다.

---

## 4. 설계 메모

- **단일슬롯 vs 다중슬롯**: 서버가 다수 모델을 교대 예측하면 단일슬롯은 재로드가
  잦다. 무한 성장을 피하려 단일슬롯으로 시작했고, 필요 시 소형 LRU로 확장한다.
- **캐시 무효화**: mtime+len은 값싸고 충분하다. 같은 초에 같은 길이로 재작성하는
  극단적 경우만 놓칠 수 있어, 필요 시 해시로 강화한다.
- **probe의 패닉 훅**: 프로세스 시작 시 1회, 단일 스레드 구간이라 안전하다.
  향후 cubecl에 비패닉 probe가 생기면 교체한다.
- **`resolve()`의 순수성**: device probe 때문에 더 이상 순수하지 않다. CPU 선택은
  여전히 순수하며, 테스트는 cuda feature 유무/device 유무를 모두 처리하도록 수정.

---

## 5. 파일별 변경 요약

- `xazz-exec/src/dl.rs` — `ArtifactKey`/`artifact_key`, `load_inference_model`,
  `predict_with_model`, `predict_on_device` 리팩터 + 단위 테스트 2종
- `xazz-exec/src/backend.rs` — `WgpuDeviceSpec`/`parse_wgpu_device`, wgpu/cuda
  device+모듈 캐시, onnx 세션 캐시, `build`→`Result`, `device_warning`, 테스트 갱신
- `xazz-exec/src/dl/onnx_export.rs` — `ensure_export`/`onnx_is_fresh`/
  `load_session`/`predict_with_session`, `init_runtime` pub, ONNX EP 해석/적용(8절)
- `xazz-exec/Cargo.toml` — `onnx-cuda`/`onnx-tensorrt`/`onnx-directml`/`onnx-coreml` feature
- `CHANGELOG.md` / `NEXT.md` — 기록

---

## 6. 후속 (이번 범위 밖, NEXT.md 백로그)

1. **학습 시 디스크 왕복** — `train_on_device`가 GPU 학습 후 pretty-JSON
   체크포인트로 `save→load`해 CPU 아티팩트를 만든다(학습 시간에 포함).
2. **스윕** — 기본 impl이 조합마다 `train` → 조합마다 저장/매니페스트/왕복.
   winner만 materialize하도록 검토.
3. ~~**ONNX GPU EP** — `ort` feature에 `cuda`/`tensorrt`/`directml` 없음 +
   `with_execution_providers` 없음 → 현재 `onnx`는 CPU EP로 실행된다.
   `XAZZ_ORT_EP` + EP 주입 필요~~ → **8절에서 완료**
4. **추론 chunking** — `[n, feature_count]` 단일 업로드 → 대량 n OOM/전송 병목.
5. **device/EP 선택 인터페이스 통일** — wgpu는 `XAZZ_WGPU_DEVICE`, CUDA는
   `XAZZ_CUDA_DEVICE`, ONNX는 EP. 3종 공통 인터페이스 검토.

---

## 8. 이어서 — ONNX GPU 실행 프로바이더 (D2 #63)

6절 3번 후속. "ONNX GPU 검증"의 전제 조건이었다.

### 8.1 문제

`ort` feature에 GPU EP가 없고 세션 생성 시 `with_execution_providers`도 쓰지
않아, `XAZZ_BACKEND=onnx`는 **CPU EP**로 실행됐다. 즉 ONNX를 벤치하면 GPU가
아니라 CPU를 측정하게 된다.

### 8.2 배경 조사

`ort-sys`의 `download-binaries`는 feature set에 맞는 **GPU 빌드**를 내려받는다
(`build/download/dist.tsv`):
- `x86_64-pc-windows-msvc` + `cuda,tensorrt` → `+cuda13,tensorrt,nvrtx,directml`
- `aarch64-apple-darwin` + `coreml` → `+coreml`
- `x86_64-unknown-linux-gnu` + `cuda,tensorrt` → `+cuda13,tensorrt,nvrtx`

따라서 `ort/cuda` 등 feature를 켜는 것만으로 GPU ONNX Runtime이 확보된다.

### 8.3 변경

- **feature 4종** — `onnx-cuda`(`ort/cuda`), `onnx-tensorrt`, `onnx-directml`,
  `onnx-coreml`. 모두 `onnx`를 포함한다.
- **`XAZZ_ORT_EP`** — `auto`(기본)는 컴파일된 GPU EP를 우선순위
  (cuda→tensorrt→directml→coreml)로 등록, 실패 시 다음/CPU로 조용히 폴백.
  명시 목록은 각 EP에 `error_on_failure()`를 걸어 **fail-closed**(요청 EP가
  미가용이면 CPU로 몰래 도는 대신 오류).
- **`XAZZ_ORT_DEVICE`** — CUDA/TensorRT/DirectML device index(기본 0).
- `onnx_export::load_session`이 세션 생성 시 EP 목록을 적용(세션은 캐시되어 1회).
- 파서(`OrtEpKind`/`OrtEpSpec`/`parse_ort_ep_spec`)는 `ort` 비의존으로 분리해
  default feature에서 단위 테스트.

### 8.4 검증

| 구성 | 결과 |
|---|---|
| `cargo check/clippy -p xazz-exec --features onnx --all-targets -- -D warnings` | 통과 |
| `... --features onnx-cuda` | 통과 (ort가 CUDA dist 다운로드) |
| `... --features onnx-tensorrt` / `onnx-directml` / `onnx-coreml` | 통과 |
| 4종 동시 `--all-targets -- -D warnings` | 통과 |
| `parse_ort_ep_spec` 단위 테스트 | 통과 |

> 한계: WSL은 ort prebuilt C++ 정적 링크(zig) 제약으로 **실기 EP 실행**은 불가.
> Windows(RTX 4070, `onnx-cuda`)와 macOS M5(`onnx-coreml`)에서 실측한다.

### 8.5 사용법 (벤치)

```bash
# Windows RTX 4070 — CUDA EP (명시, fail-closed)
XAZZ_BACKEND=onnx XAZZ_ORT_EP=cuda XAZZ_ORT_DEVICE=0 xazz run model.xzz

# macOS M5 — CoreML EP
XAZZ_BACKEND=onnx XAZZ_ORT_EP=coreml xazz run model.xzz

# 자동(컴파일된 GPU EP 우선, 없으면 CPU)
XAZZ_BACKEND=onnx XAZZ_ORT_EP=auto xazz run model.xzz
```

## 7. 벤치 하네스 권고

- backend별 **별도 프로세스**로 실행(`active()`가 `OnceLock`이라 한 프로세스에서
  A/B 불가).
- **warmup 후 steady-state** 측정(캐시 채운 뒤 반복 추론), init/export 비용은
  첫 호출로 분리 기록.
- `[xazz:timing]`(파이프라인)과 device probe/체크포인트 로드 시간을 구분해 로깅.

---

## 9. 이어서 — 측정 왜곡 제거 (NEXT GPU 벤치 unblock)

6절 후속과 "발견한 후속"에 남아 있던 벤치 왜곡 요소를 정리했다. 구현은 여전히
`xazz-exec`.

### 9.1 변경

| # | 문제 | 변경 |
|---|---|---|
| 1 | `train_on_device`의 GPU→디스크→CPU 왕복 | `BinBytesRecorder`(bincode) 인메모리 전송 `materialize_cpu_model`. 디스크 체크포인트는 pretty-JSON 유지 |
| 2 | 스윕 조합마다 저장/매니페스트/왕복 | `train_unpersisted`(+`persist:` 인자) 도입, 스윕은 조합에 unpersisted·우승만 저장. ONNX도 조합별 export 제거 |
| 3 | 추론 `[n, feature]` 단일 업로드 | `XAZZ_INFER_CHUNK` + `chunk_ranges`/`forward_predictions`, wgpu/cuda/CPU·ONNX 공통 |
| 4 | 단일슬롯 추론 캐시 | `dl::LruCache` + `XAZZ_INFER_CACHE_SLOTS`(기본 4) |
| 5 | device/EP 선택 3종 분리 | 통합 `XAZZ_DEVICE` + `parse_device_spec`, 레거시 변수 폴백 |

### 9.2 검증

| 구성 | 명령 | 결과 |
|---|---|---|
| default 테스트 | `cargo test -p xazz-exec` | 83 통과 (신규 5종 포함) |
| wgpu | `cargo clippy -p xazz-exec --features wgpu --all-targets -- -D warnings` | 통과 |
| cuda | `... --features cuda ...` | 통과 |
| onnx / onnx-cuda | `... --features onnx` / `onnx-cuda ...` | 통과 |
| workspace | `cargo clippy --workspace --all-targets -- -D warnings` | 통과 |

추가 단위 테스트: `chunk_ranges` 경계, `LruCache` 승격/퇴출/최소 용량,
`train_on_device`(CPU 백엔드로 인메모리 핸드오프 경로), `train_unpersisted`
(체크포인트 미기록), `parse_device_spec` 별칭/오류.

> 한계: 이 WSL은 zig C++ 툴체인이라 wgpu test 바이너리 **링크가 극단적으로
> 느리고**(수 분+) CUDA/ONNX 실기 링크는 불가하다. 실기 acceptance는 표준
> 툴체인(Windows/macOS)에서 `-- --ignored`로 실행한다.

### 9.3 사용법 (벤치)

```bash
# 통합 device 선택 (wgpu: dGPU vs iGPU 비교)
XAZZ_BACKEND=wgpu XAZZ_DEVICE=dgpu:0 xazz run model.xzz
XAZZ_BACKEND=wgpu XAZZ_DEVICE=igpu:0 xazz run model.xzz

# CUDA / ONNX EP
XAZZ_BACKEND=cuda XAZZ_DEVICE=cuda:0 xazz run model.xzz
XAZZ_BACKEND=onnx XAZZ_DEVICE=cuda:0 xazz run model.xzz

# 청크/캐시 슬롯 튜닝 (기본 4096행 / 4슬롯)
XAZZ_INFER_CHUNK=8192 XAZZ_INFER_CACHE_SLOTS=8 xazz run model.xzz
```

