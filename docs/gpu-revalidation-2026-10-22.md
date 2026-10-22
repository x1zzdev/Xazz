# CUDA·ONNX 실기 재검증 — 2026-10-22

최신 main에서 ONNX 선택 기능의 컴파일 회귀를 재현·수정했다. 수정본에서 **CUDA, ONNX CPU, ONNX CUDA 실기 검사와 실패 대조 2건이 모두 통과**했다. ONNX CUDA는 프로파일에서 실제 CUDA 계산 노드 3개를 확인했다.

- PR: [#329](https://github.com/x1zzdev/Xazz/pull/329)
- 기반 main: `b347688b3b4906a53ddb0b4323aed9041e117628` — 최종 확인 시에도 동일.
- 최종 실기·검사 도구 커밋: `4ebba41559e4b0c58ea0229ecaee51df26afbd6c`. 이후 커밋은 검증 문서와 근거 정리다.
- 과거 검증: CUDA [#263](https://github.com/x1zzdev/Xazz/pull/263), Windows ONNX [#289](https://github.com/x1zzdev/Xazz/pull/289). 과거 통과와 최신 코드 검증은 구분한다.

## 실기 결과

각 성공 검사는 정확한 테스트 이름으로 실행했고 **1 passed, 0 failed, 0 ignored**였다. 입력은 테스트의 6행 소형 MLP 데이터다. 빌드 시간과 아래 실행 시간을 구분하며 성능 벤치마크로 해석하지 않는다.

| 검사 | 결과 | 수치·실행 증거 | 테스트 프로세스 시간 |
|---|---|---|---|
| CUDA 학습·추론 (GNU, `cuda`) | 통과 | backend=`cuda`, 유한한 학습 loss, 동일 체크포인트 CPU·CUDA 최대 편차 0 | 4.485초 |
| ONNX CPU 추론 (MSVC, `onnx`) | 통과 | CPU 계산 노드 2개, 최대 편차 0 | 0.110초 |
| ONNX CUDA 추론 (MSVC, `onnx-cuda`) | 통과 | CUDA 계산 노드 3개, 최대 편차 `1.1920928955078125e-7` | 1.297초 |
| CUDA 미포함 ONNX 빌드에 CUDA 요청 | 예상 거부 확인 | 종료 101, 정확히 1개 실패, `ONNX execution provider is not compiled into this binary` | 0.031초 |
| ONNX CUDA에 `cuda:999` 요청 | 예상 거부 확인 | 종료 101, 정확히 1개 실패, `Invalid device ID: 999` | 0.734초 |

음성 대조 두 건은 실행 실패가 기대 결과다. 다른 DLL 오류나 테스트 0개 실행을 이 결과로 인정하지 않는다. ONNX의 학습 단계는 CPU이며 GPU 학습을 검증했다고 주장하지 않는다. CUDA 학습 체크포인트의 장치 추론도 별도로 확인했다.

### 일반 검사와 자동 검사

- 기본 workspace: **746 passed, 0 failed, 0 ignored**. GPU 기능은 포함하지 않는다.
- CUDA 기능 라이브러리: **138 passed, 0 failed, 1 ignored**. ignored 1개는 위 실기 검사로 별도 실행했다.
- ONNX CUDA 기능 라이브러리: **140 passed, 0 failed, 1 ignored**. ignored 1개 역시 위에서 별도 실행했다.
- Python 증거 판정·DLL 정합성: **13개 통과**.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `git diff --check` 통과.
- [최종 코드의 기능별 CI](https://github.com/x1zzdev/Xazz/actions/runs/39713950242): CUDA/wgpu Linux 컴파일, ONNX/ONNX CUDA Windows MSVC 컴파일, Windows/Linux Python 판정 검사 **모두 통과**.
- 문서 빌드·링크 검사 통과. 최종 문서 커밋의 CI 상태는 PR에서 확인한다.
- 전체 workspace 의존성 정책은 기존 **37개 오류**로 실패: 라이선스 16, 취약점 14, 건전성 3, 유지보수 중단 2, 철회된 버전 2. 이번 PR은 이를 숨기거나 예외 처리하지 않는다. 현재 기본 CI의 cargo-deny는 루트 CLI 그래프만 보므로 그 성공을 전체 workspace 성공으로 해석하지 않는다.

## 실행 환경

- Windows, NVIDIA GeForce RTX 4070 Laptop GPU, 드라이버 591.44.
- Rust 1.98.0. CUDA는 `stable-x86_64-pc-windows-gnu`, ONNX는 `stable-x86_64-pc-windows-msvc`.
- MSVC Build Tools 2022, x64 환경. 기본 Rust 툴체인은 바꾸지 않았다.
- CUDA NVRTC 패키지 13.1.115, CUDA runtime 헤더 패키지 13.1.80. NVRTC DLL을 프로세스 PATH에 추가하고 헤더 위치를 `CUDA_PATH`로 지정했다.
- ONNX: `ort/ort-sys 2.0.0-rc.14`, ORT 1.31.0 배포본, `ORT_CUDA_VERSION=13`, cuBLAS 13.1.1.3.
- release 설정(`opt-level=3`, LTO, codegen-units=1)으로 실기 검사. CUDA 최초 빌드는 37분 36초가 걸렸고 이후 검사는 캐시를 재사용했다.
- 기본 GNU 검사에서는 기존 PE 크기 문제를 피하도록 `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`, `CARGO_BUILD_JOBS=4`를 사용했다.

## 반복됐던 이유와 수정

### 1. 검증 기록과 이슈·문서가 어긋남

2026-10-01 CUDA, 2026-10-06 Windows ONNX 검증이 병합됐지만 #62/#63의 미완료 표시, 영문·한국어 README, 로드맵 일부가 이전 상태였다. CUDA의 요구사항도 일부 문서에서 드라이버만으로 충분한 것처럼 읽혔다. 이번에 근거 링크, 실행 조건과 검증 범위를 맞췄다.

원래 이슈의 “CPU·CUDA 학습 loss가 동일”이라는 표현은 실제 검사와 달랐다. 10월 1일 기록과 기존 테스트가 사용하던 기준은 **같은 체크포인트의 추론 편차 `< 1e-3`와 실제 CUDA 학습의 유한한 loss**다. 서로 독립적으로 초기화한 학습 결과의 loss 일치를 주장하지 않는다.

### 2. 최신 의존성 업데이트의 ONNX 회귀가 기본 검사에서 제외됨

첫 MSVC ONNX 빌드가 5개 컴파일 오류로 실패했다. 새 선택 기능 CI도 [수정 전 오류를 그대로 검출](https://github.com/x1zzdev/Xazz/actions/runs/39710751666)했다.

- ORT rc.14에서 `ort::init().commit()`이 없어지고 환경을 `Session::builder(&environment)`에 전달해야 한다. [공식 변경 안내](https://github.com/pykeio/ort/releases/tag/v2.0.0-rc.14)에 맞춰 프로세스의 단일 환경을 유지하고 세션에 명시적으로 전달하며 초기화 오류를 반환하도록 수정했다. 지원 API 버전도 rc.14로 명시했다.
- `rlx-onnx-proto 0.2.18`의 생성 타입은 protobuf 1.7의 `Message`를 구현하지만 직접 의존성만 3.7로 바뀌어 `write_to_bytes`·`merge_from_bytes`를 사용할 수 없었다. 생성 타입과 동일한 기존 1.7 의존성으로 맞추고 불필요한 3.7·protobuf-support를 제거했다. transitive protobuf 1.7은 기존에도 존재했다. 그 보안 경고는 해결되지 않았으며 exporter/생성기 이관은 #63·#127의 별도 작업이다.

### 3. Windows 런타임 캐시·DLL 버전 불일치

ORT 1.31.0 CUDA 다운로드는 체크섬 검증·압축 해제 후 캐시 폴더 이름 변경에서 `os error 5`로 실패했다. 소스에서 체크섬 검증이 먼저 끝나는 순서를 확인하고, 이번 실행의 임시 폴더만 같은 캐시의 최종 이름으로 옮겨 복구했다. 실패 로그를 보존했다.

또한 `ort-sys`의 복사 코드는 기존 일반 DLL 파일을 교체하지 않았다. 기존 `release/onnxruntime_providers_cuda.dll`은 62,276,096 bytes, 새 링크 경로의 DLL은 93,791,744 bytes였다. 새 검사 도구는 Cargo JSON의 **현재 ORT 링크 경로**에서 DLL을 선택해 테스트 실행 파일 옆에 배치하고 해시를 남긴다. 이전 캐시를 가리키는 심볼릭 링크의 대상을 덮어쓰지 않는다. 이 조치는 테스트용이며 설치된 CLI의 DLL을 자동 교체하지 않는다.

## 재발 방지와 재현

[GPU 실행 안내의 재현 절차](GPU_BACKENDS.md#repeatable-hardware-verification)를 따른다. `scripts/verify_gpu_acceptance.py`는 빌드 중 소스 변경, 0개 실행, CPU 대체, 비유한 값, 빈 출력, 잘못된 provider와 예상하지 않은 실패를 거부한다. ONNX는 실제 계산 노드가 있어야 통과하며 메모리 복사만으로는 충분하지 않다.

- PR마다 `wgpu`, `cuda`, `onnx`, `onnx-cuda`를 별도로 컴파일한다. 하드웨어 없는 hosted runner에서는 실기 성공을 주장하지 않는다.
- `gpu-hardware.yml`은 신뢰한 ref를 선택해 실행하는 수동 workflow다. `self-hosted, Windows, X64, xazz-gpu` runner를 별도 준비해야 하며 **이번 작업에서 runner를 등록하지 않았다**.
- PR 양식과 기여 안내에 기능별 검증·근거 및 이슈 상태 갱신을 명시했다.
- wgpu 실기는 이번에 재실행하지 않았다. macOS CoreML, 대형 모델, 모든 연산자·그래프의 GPU 배치, CLI 전체 실행·배포본 설치, 성능 보장은 이번 검증 범위가 아니다.

## 검증 근거

[로그·실행 결과·ORT 프로파일 묶음](evidence/gpu-windows-2026-10-22.zip): 실행·환경 파일 33개와 `manifest.json`, 368,791 bytes.

SHA-256: `13a3a0fa82c282145f37eed6207d4d477000f3adf8ddfd6d674b73d668fee4c7`.

최초 ONNX 컴파일 실패, CI의 같은 실패, 캐시 추출 실패, 최종 성공 3건, 예상 거부 2건, 일반 검사·정책 오류를 함께 보존했다. 개인 절대 경로와 ANSI 제어 문자를 정규화했으며 manifest에는 원본·공개본 해시를 각각 기록했다. 실행 파일·DLL·모델 체크포인트는 포함하지 않았다. 최종 결과 JSON의 소스·실행 파일·런타임 해시로 검사 대상을 확인할 수 있다.

## 이슈 정리

- #62: 기존 wgpu 검증과 이번 CUDA 실기 결과를 근거로 완료 처리한다. 자동 검사 보강은 PR #329의 병합으로 반영된다.
- #63: Windows CPU·CUDA 실기 항목은 완료로 정리하되, 현재 main의 호환성 수정 PR 병합과 `burn-onnx` exporter 이관은 미완료로 유지한다. 전체 이슈를 닫지 않는다.
