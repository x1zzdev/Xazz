# GPU 백엔드 실기 검증 — Windows 11 / RTX 4070 + Intel Arc (issue #103)

> D1(#62)·D2(#63)의 `#[ignore]` 실기 acceptance를 실제 GPU가 있는 Windows 호스트에서 실행한 기록.
> 측정 방법론과 캐시·chunk 설계는 `gpu-backend-perf-log.md`, 검증 항목은 `ROADMAP.md` Track D 참조.

---

## 1. 요약

| 백엔드 | 명령 | 결과 |
|---|---|---|
| **wgpu** acceptance | `cargo test --release -p xazz-exec --features wgpu -- --ignored` | **통과** — `wgpu_matches_cpu_losses` 1 passed (5.36s) |
| **wgpu** `XAZZ_DEVICE=dgpu:0` | `xazz run` (학습·예측·차트) | **통과** — RTX 4070에서 실행됨을 GPU 엔진 카운터로 확인 |
| **wgpu** `XAZZ_DEVICE=igpu:0` | `xazz run` | **통과** — Intel Arc에서 실행됨을 확인 (NVIDIA 사용률 0%) |
| **cuda** (burn-tch) | `cargo test --release -p xazz-exec --features cuda -- --ignored` | **실패 (툴체인)** — torch-sys C++ 셰임이 GNU g++에서 컴파일 불가 |
| **onnx** (ort) | `cargo test --release -p xazz-exec --features onnx -- --ignored` | **실패 (툴체인)** — `x86_64-pc-windows-gnu` 타깃용 prebuilt ONNX Runtime 없음 |
| **onnx-coreml** (macOS M5) | — | 미실시 (별도 기기, 후속) |

한 줄 결론: **wgpu 경로는 실제 dGPU/iGPU 양쪽에서 동작하고 `XAZZ_DEVICE` 어댑터 선택이 정확하다.**
CUDA·ONNX는 이 호스트의 Rust 툴체인이 `x86_64-pc-windows-gnu`라서 빌드 단계에서 막혔다.
두 provider 모두 Windows에서는 **MSVC 툴체인(`stable-x86_64-pc-windows-msvc` + VS Build Tools)이 필수**이며,
실기 검증은 그 환경에서 다시 수행해야 한다(§6).

---

## 2. 검증 환경

| 항목 | 값 |
|---|---|
| 기기 | Samsung Galaxy Book4 Ultra (NT960XGL) |
| OS | Windows 11 Enterprise 10.0.26200 |
| CPU | Intel Core Ultra 9 185H |
| dGPU | NVIDIA GeForce RTX 4070 Laptop GPU, 8GB — driver 591.44 (CUDA 13.1 지원) |
| iGPU | Intel Arc Graphics (Meteor Lake) — driver 32.0.101.8424 |
| CUDA Toolkit | 미설치 (드라이버만 존재) |
| Rust | 1.98.0, `stable-x86_64-pc-windows-gnu` — MSVC Build Tools 미설치 |
| C/C++ | gcc 16.1.0 (WinLibs UCRT POSIX, MinGW-w64) |
| 저장소 | `x1zzdev/Xazz` main `466a424` (2026-09-24) |
| 실행 파일 | `target/release/{xazz,xazz-runner,xazz-exec}.exe`, xazz-exec는 `--features wgpu` |

---

## 3. wgpu

### 3.1 Acceptance 테스트

```
cargo test --release -p xazz-exec --features wgpu -- --ignored --nocapture
```

```
test backend::acceptance::wgpu_matches_cpu_losses ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 83 filtered out; finished in 5.36s
```

- CPU 학습 체크포인트를 CPU와 wgpu에서 각각 추론 → 최대 편차 < 1e-3 (parity 통과)
- wgpu 장치 학습 loss finite, 장치 학습 체크포인트로 추론 가능
- 첫 release 빌드 약 34분 (이 중 `xazz-exec` crate 최종 컴파일 약 20분)

> `--release`가 필요한 이유는 §5.1 참조. 테스트는 `XAZZ_DEVICE` 미설정(auto)이므로
> 어떤 어댑터가 선택됐는지는 아래 §3.2에서 별도 확인했다.

### 3.2 어댑터 선택 확인 — dGPU vs iGPU

**문제.** cubecl-wgpu는 선택된 어댑터를 `log::info!("Using adapter {:?}", adapter.get_info())`로
남기지만 xazz-exec에 로거가 설치돼 있지 않아 이 줄이 출력되지 않는다. `[xazz] ML backend: wgpu`만
찍히고 어댑터는 알 수 없다.

**대안.** `xazz run` 실행 중 xazz-exec 프로세스의 Windows 성능 카운터
`\GPU Engine(pid_<pid>_*)\Utilization Percentage`를 1초 간격으로 샘플링하고, 인스턴스 이름의 LUID를
`HKLM\SOFTWARE\Microsoft\DirectX\{adapter}\AdapterLuid`로 어댑터 이름에 매핑했다. nvidia-smi를 병행했다.

| LUID | 어댑터 |
|---|---|
| `0x00000000_0x000131C8` | NVIDIA GeForce RTX 4070 Laptop GPU |
| `0x00000000_0x00012D1A` | Intel(R) Arc(TM) Graphics |
| `0x00000000_0x00013164` | Microsoft Basic Render Driver |

**워크로드.** 번들 예제(`air_pipeline.xzz`, 97 params)는 GPU 시간이 1초 미만이라 카운터에 잡히지
않아, 같은 데이터(서울 대기질 2024, 228,383행)에 큰 MLP를 올린 스크립트를 썼다:

```
model AirBench { Dense(512) -> ReLU() -> Dense(512) -> ReLU() -> Dense(256) -> ReLU() -> Dense(1) }
dataset |> train(AirBench, target: "pm10", epochs: 200, lr: 0.001)      // 395,265 params, 단일 배치
```

**결과.**

| 설정 | xazz-exec가 사용한 어댑터 (3D 엔진) | nvidia-smi 피크 | 진행 |
|---|---|---|---|
| `wgpu` + `dgpu:0` | **RTX 4070** — max 120.9% / avg 53.8% (48 samples), VRAM 3.7GB | 100% | 200/200 epoch, 68.5s |
| `wgpu` + `igpu:0` | **Intel Arc** — max 164.1% / avg 95.2% (146 samples) | **0%** | 96/200 epoch에서 xazz-runner 300s 타임아웃 |

`dgpu:0`는 RTX 4070만, `igpu:0`는 Intel Arc만 사용했다. (100% 초과 값은 Windows GPU Engine 카운터가
동일 엔진 타입의 인스턴스를 합산하는 특성으로, 상대 비교용이다.)

### 3.3 `xazz run` 실측 — 번들 예제 (`examples/deep_learning/air_pipeline.xzz`)

Dense(32)→ReLU→Dense(1), 97 params, 228,383행 단일 배치, 10 epoch. 세 실행 모두 guardrail 통과 →
학습 → 예측 → 차트 HTML 생성까지 완료.

| 설정 | `[xazz] ML backend` | epoch 1 loss | final loss (MSE) | MAE | `pipeline_ms` |
|---|---|---|---|---|---|
| `XAZZ_BACKEND=wgpu XAZZ_DEVICE=dgpu:0` | wgpu | 1595.63 | 1513.08 | 30.04 | 7,320 |
| `XAZZ_BACKEND=wgpu XAZZ_DEVICE=igpu:0` | wgpu | 1591.67 | 1543.91 | 30.52 | 9,331 |
| `XAZZ_BACKEND=cpu` | (기본) | 1569.97 | 1499.89 | 29.68 | 1,891 |

- loss가 실행마다 다른 것은 초기 가중치 난수가 백엔드·실행마다 달라서이며 정상이다. 동일 가중치
  기준 parity는 §3.1의 acceptance 테스트가 검증한다(acceptance가 독립 학습 두 번의 loss를 비교하지
  않고 공유 체크포인트의 추론을 비교하도록 설계된 이유이기도 하다).
- 이 크기(97 params, 단일 배치)에서는 GPU 업로드·커널 실행 오버헤드가 지배해 GPU가 CPU보다 느리다.
  큰 모델(§3.2 워크로드)에서의 epoch당 시간은 dGPU ≈ 0.34s / iGPU ≈ 3.1s / CPU ≈ 14s
  (CPU는 3 epoch ≈ 43s에서 중단, 200 epoch 추정 ≈ 47분).

---

## 4. CUDA (burn-tch) — 실패, 툴체인

```
TORCH_CUDA_VERSION=cu128 cargo test --release -p xazz-exec --features cuda -- --ignored --nocapture
```

**LibTorch 다운로드는 성공.** torch-sys 0.22.0의 `download-libtorch`가
`libtorch-win-shared-with-deps-2.9.0+cu128.zip`(2,779MB, 압축 해제 후 7.6GB)를 받았고 CUDA 런타임
DLL이 포함돼 있었다 (`torch_cuda.dll` 860MB, `cudart64_12.dll`, `cudnn64_9.dll` 계열, `c10_cuda.dll`).
CUDA Toolkit이 없는 호스트에서도 드라이버만으로 시도 가능한 구성이다.

**실패 지점.** torch-sys build script가 `cc-rs`로 C++ 셰임(`libtch/torch_api.cpp`,
`torch_api_generated.cpp`)을 컴파일하는 단계. build.rs가 Windows = MSVC로 가정해 g++에 MSVC 플래그를
넘기고, LibTorch 헤더 자체도 MSVC ABI를 전제한다.

```
"g++.exe" "-O3" "-ffunction-sections" "-fdata-sections" "-fPIC" "-m64"
  "-I" ".../libtorch/include" "-I" ".../libtorch/include/torch/csrc/api/include"
  "-w" "/std:c++17" "/p:DefineConstants=GLOG_USE_GLOG_EXPORT"
  "-o" ".../torch_api_generated.o" "-c" "libtch/torch_api_generated.cpp"
```

```
c10/core/impl/LocalDispatchKeySet.h:68:52: error: thread-local variable 'c10::impl::raw_local_dispatch_key_set' declared as dllimport
torch/csrc/jit/ir/ir.h:302:42: error: function 'std::shared_ptr<torch::jit::Wrap<torch::jit::Value> > torch::jit::Value::wrap()' definition is marked dllimport
torch/headeronly/macros/Macros.h:465:5: error: '__assert_fail' was not declared in this scope; did you mean '__fastfail'?

error occurred in cc-rs: command did not execute successfully (status code exit code: 1)
error: failed to run custom build command for `torch-sys v0.22.0`
```

**결론.** `--features cuda`는 Windows에서 `stable-x86_64-pc-windows-msvc` + VS Build Tools가 필수다.
GNU 툴체인은 LibTorch prebuilt(MSVC ABI)와 근본적으로 호환되지 않으며, 이는 WSL zig 툴체인에서
막혔던 것과 같은 계열의 제약이다. RTX 4070 자체는 준비돼 있으므로(§2) 툴체인만 바꾸면 재시도 가능하다.

---

## 5. ONNX Runtime (ort) — 실패, 툴체인

```
cargo test --release -p xazz-exec --features onnx -- --ignored --nocapture
```

```
   Compiling ort-sys v2.0.0-rc.13
error: ort-sys@2.0.0-rc.13: no prebuilt binaries available for target x86_64-pc-windows-gnu
error: build script logged errors
```

`ort-sys-2.0.0-rc.13/build/download/dist.tsv`의 Windows 배포본은 전부 `x86_64-pc-windows-msvc`
(plain / directml / webgpu / nvrtx / cuda13,tensorrt,nvrtx,directml)와 `aarch64-pc-windows-msvc`뿐이며,
`resolve.rs`가 `TARGET`과 정확히 일치하는 행만 고르므로 gnu 타깃은 후보가 0개다.
`ORT_LIB_LOCATION`으로 직접 빌드한 런타임을 지정하는 우회가 있지만 그 빌드 역시 MSVC가 필요해
실효성이 없다.

**결론.** `--features onnx` / `onnx-cuda`도 CUDA와 같은 이유로 Windows에서는 MSVC 툴체인이 필수다.

---

## 6. 발견 사항과 후속

### 6.1 이번에 드러난 제약

| # | 내용 | 영향 | 제안 |
|---|---|---|---|
| 1 | **GNU 툴체인 debug 테스트 바이너리 4GB 초과** — `cargo test --features wgpu`(debug)가 4,045,037,500 bytes exe를 만들어 `os error 193`(올바른 Win32 응용 프로그램이 아님)으로 실행 불가. GNU 링커는 DWARF를 exe에 포함하는데 wgpu(cubecl/burn)+Polars+DuckDB 조합이 PE 한도를 넘김 (wgpu 없는 빌드는 1.75GB) | Windows-gnu에서 GPU feature 테스트는 debug로 불가 | 문서에 `--release` 안내 (또는 `[profile.test] debug = 0`) |
| 2 | **어댑터 선택 로그 부재** — cubecl-wgpu의 `Using adapter` info 로그가 로거 미설치로 소실 | `XAZZ_DEVICE` 결과를 로그로 검증 불가 | xazz-exec에 `XAZZ_LOG`(또는 `RUST_LOG`) 기반 로거 설치, `[xazz] ML backend: wgpu (adapter: …)` 형태로 노출 |
| 3 | **xazz-runner 기본 타임아웃 300s** — iGPU/CPU로 큰 학습 시 96 epoch에서 종료 | 장시간 학습이 조용히 잘림 | `XAZZ_EXEC_TIMEOUT_SECS` 안내 강화, 또는 `train` 존재 시 기본값 상향 검토 |
| 4 | **Windows에서 cuda/onnx는 MSVC 전용** (§4, §5) | gnu 툴체인 사용자는 빌드 불가 | `xazz-exec/Cargo.toml` feature 주석과 README에 명시 |

> 위 제안 1·4는 반영됐다: `xazz-exec/build.rs`가 `windows-gnu` + `cuda`/`onnx*` 조합을
> 빌드 초입에서 MSVC 설치 안내와 함께 차단하고(`XAZZ_ALLOW_WINDOWS_GNU_GPU=1`로 우회),
> `CONTRIBUTING.md` "Optional GPU backends"에 `--release`·MSVC 요건을 문서화했다.

### 6.2 남은 검증

- [ ] **CUDA 실기** — 같은 호스트에서 VS Build Tools 설치 후 `rustup default stable-msvc`로 전체 재빌드하여 §4 재실행
- [ ] **ONNX 실기 (Windows)** — 위 MSVC 환경에서 `--features onnx` 및 `onnx-cuda`(`XAZZ_ORT_EP=cuda` fail-closed 경로 포함)
- [ ] **ONNX coreml (macOS M5)** — `cargo test -p xazz-exec --features onnx-coreml -- --ignored`, `XAZZ_BACKEND=onnx XAZZ_DEVICE=coreml:0`
- [ ] **chunk/cache 튜닝 A/B** — `XAZZ_INFER_CHUNK`, `XAZZ_INFER_CACHE_SLOTS` (선택)
- [ ] 3종(dGPU/iGPU/CPU) 동일 워크로드 완주 비교 — 20 epoch 축약본으로 CPU·iGPU 완주 후 epoch당 시간 정식 기록

---

## 7. 재현

```powershell
# 툴체인: stable-x86_64-pc-windows-gnu (gcc 16.1). GPU feature는 반드시 --release (§6.1-1)
cargo test --release -p xazz-exec --features wgpu -- --ignored --nocapture
cargo build --release -p xazz -p xazz-runner
cargo build --release -p xazz-exec --features wgpu

$env:XAZZ_BACKEND="wgpu"; $env:XAZZ_DEVICE="dgpu:0"; .\target\release\xazz.exe run examples/deep_learning/air_pipeline.xzz
$env:XAZZ_BACKEND="wgpu"; $env:XAZZ_DEVICE="igpu:0"; .\target\release\xazz.exe run examples/deep_learning/air_pipeline.xzz
$env:XAZZ_BACKEND="cpu";                            .\target\release\xazz.exe run examples/deep_learning/air_pipeline.xzz

# 어댑터 귀속 확인: 실행 중 다른 창에서
Get-Counter '\GPU Engine(pid_<xazz-exec pid>_*)\Utilization Percentage'
nvidia-smi --query-gpu=utilization.gpu,memory.used --format=csv

# CUDA / ONNX (gnu 툴체인에서는 §4·§5의 오류로 종료)
$env:TORCH_CUDA_VERSION="cu128"; cargo test --release -p xazz-exec --features cuda -- --ignored --nocapture
cargo test --release -p xazz-exec --features onnx -- --ignored --nocapture
```
