# Windows ONNX 실기 검증 — 2026-10-06

이 기록은 #236의 Windows MSVC ONNX 검증이다. 기준 소스는 main의 `7f8e987c0e7097ba942c8d8ade2bb9b614bd4fcb`이며 기본 GNU CPU 벤치마크 산출물과 별도 디렉터리에서 빌드했다. #127·#129는 보류를 유지하고 기존 burn-cuda 실측을 반복하지 않는다.

## 준비한 환경

- Windows 11 Enterprise build 26200, Core Ultra 9 185H, 시스템 RAM 63.5GiB.
- NVIDIA GeForce RTX 4070 Laptop GPU, VRAM 8,188MiB, 드라이버 591.44.
- Rust 1.98.0, `stable-x86_64-pc-windows-msvc`. 기본 GNU 툴체인은 바꾸지 않았다.
- Visual Studio Build Tools 2022 17.14.41, MSVC 14.44.35207, Windows SDK 10.0.26100.0.
- 설치 전 C++ 도구와 SDK가 없어 Microsoft 공식 Build Tools 부트스트래퍼의 서명을 확인하고 `Microsoft.VisualStudio.Workload.VCTools`와 권장 구성요소를 설치했다. 설치 종료 코드 0이며 자동 재시작하지 않았다.
- 부트스트래퍼 SHA-256: `985969f472caad75d993a5cb4c35a6a4271460cc12b343e2433b994d173aa990`.

설치 명령은 [Microsoft 공식 명령행 문서](https://learn.microsoft.com/en-us/visualstudio/install/use-command-line-parameters-to-install-visual-studio?view=vs-2022)의 Visual Studio 2022 Build Tools 배포를 사용했다. 설치 명령은 `vs_buildtools.exe --quiet --wait --norestart --nocache --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended`다.

## 검증 계약

기존 `backend::acceptance::onnx_matches_cpu_losses`를 실제로 실행한다. 이 경로의 학습은 CPU에서 수행하고 ONNX로 내보낸 뒤, ONNX Runtime으로 추론한다. ONNX GPU 학습 검증으로 해석하지 않는다. 같은 CPU 체크포인트의 CPU·ONNX 예측 최대 편차 `< 1e-3`, 유한한 학습 손실, 내보낸 별도 모델의 예측 행 수를 기존 단언으로 검사한다.

`XAZZ_DEVICE`는 `XAZZ_ORT_EP`보다 우선하므로 EP 검증에서는 해당 변수를 명시적으로 해제한다. CPU 검증은 `XAZZ_ORT_EP=cpu`, CUDA 검증은 `XAZZ_ORT_EP=cuda`를 사용한다. `auto` 모드의 CPU 대체를 CUDA 성공으로 기록하지 않는다. 사용한 `ort-sys 2.0.0-rc.13`의 Windows GPU 배포는 CUDA 13이므로 CUDA 빌드에는 `ORT_CUDA_VERSION=13`을 명시한다.

## 실행 결과

| 대상 | 환경 | 결과 |
| --- | --- | --- |
| ONNX CPU acceptance | `--features onnx`, `XAZZ_ORT_EP=cpu` | 1개 통과, 실패·제외 0개, 실행 0.09초 |
| ONNX 일반 라이브러리 검사 | 같은 CPU 빌드 | 99개 통과, 실패 0개, 하드웨어 검사 1개는 위에서 별도 실행 |
| 미컴파일 CUDA 요청 | `--features onnx`, `XAZZ_ORT_EP=cuda` | 예상 거부: 종료 101, CUDA EP 미포함 오류 원문 확인 |
| CUDA 최초 실행 | `--features onnx-cuda`, `XAZZ_ORT_EP=cuda` | 종료 101, provider DLL 위치 오류로 1개 실패 |
| CUDA 런타임 준비 후 acceptance | 같은 기능·EP·검사, DLL 배치와 프로세스 PATH 보완 | 1개 통과, 실패·제외 0개, 실행 2.15초 |
| CUDA 일반 라이브러리 검사 | `--features onnx-cuda`, `XAZZ_ORT_EP=cuda` | 99개 통과, 실패 0개, 하드웨어 검사 1개는 위에서 별도 실행 |
| 잘못된 CUDA 장치 | CUDA 빌드, `XAZZ_DEVICE=cuda:999` | 예상 거부: 종료 101, `Invalid device ID: 999` 원문 확인 |

첫 CPU acceptance 명령은 `cargo +stable-x86_64-pc-windows-msvc test --release --locked -p xazz-exec --features onnx -j 8 -- --ignored --nocapture`다. 저장소의 release 설정인 `opt-level=3`, `lto=true`, `codegen-units=1`을 유지했고 첫 빌드는 26분 29초가 걸렸다. 숫자 대조에 쓰인 테스트 실행 파일 SHA-256은 `c51ce39dcb5d872470c14a6aaeeb049f57581385e22a97cabe79747ed429589b`다.

일반 라이브러리 검사는 위 명령에서 `--lib`를 지정하고 `-- --ignored --nocapture`를 제외해 실행했다. 출력된 `1 ignored`는 앞선 별도 실행에서 통과한 ONNX 하드웨어 acceptance이며, 이를 합쳐 실패나 미검증을 숨기지 않는다. CUDA 기능 검증도 하드웨어 검사가 정의된 라이브러리 대상 `--lib`로 실행한다. 이 빌드에서 활성화되는 하드웨어 acceptance는 `backend.rs`의 ONNX 검사 한 개다.

CPU 전용 빌드에서 `XAZZ_ORT_EP=cuda`로 같은 acceptance를 실행하자 `backend predict: "ONNX execution provider is not compiled into this binary (cuda: --features onnx-cuda)"`로 실패했다. 원시 결과는 `0 passed; 1 failed`, 종료 코드 101이다. 이것은 CUDA 실행 성공이 아니라 의도한 미지원 요청 거부를 확인한 실패 대조 실행이다.

MSVC 링크 과정의 `.lib`·`.exp` 생성 안내가 Rust의 `linker_messages` 경고로 각각 기록됐다. 경고를 숨기는 옵션은 사용하지 않았다. 모델 실행의 성공 여부와 이 빌드 경고를 구분한다.

## CUDA 최초 실패와 환경 보완

첫 CUDA 기능 빌드와 실행은 569.76초 후 종료 코드 101로 끝났다. 빌드는 성공했지만 ORT가 테스트 실행 파일이 있는 `release/deps`에서 `onnxruntime_providers_shared.dll`을 찾지 못해 acceptance가 실패했다. `ort-sys` 빌드 출력에도 Windows Developer Mode가 없거나 다른 드라이브의 캐시 때문에 DLL을 복사했으며, examples·tests에서 접근할 수 없다는 경고가 있었다. 이 호스트에서는 provider DLL이 `release`에 있고 `release/deps`에는 없었다.

다음 실행 전에 적용한 환경 보완은 두 가지다.

1. 내려받은 배포본의 `onnxruntime_providers_shared.dll`과 `onnxruntime_providers_cuda.dll`을 `release`에서 테스트 실행 파일 옆 `release/deps`로 복사했다. 원본과 복사본의 해시를 기록했다.
2. `dumpbin /DEPENDENTS`로 CUDA provider의 `cublas64_13.dll`·`cublasLt64_13.dll` 의존성을 확인했다. `nvidia-cublas==13.1.1.3` Windows wheel을 다운로드해 압축만 풀고, 해당 프로세스의 PATH에 `nvidia/cu13/bin/x86_64`를 추가했다. 시스템 PATH·드라이버는 바꾸지 않았다. cuBLAS 누락만을 따로 실행해 재현한 것은 아니며, 첫 관측 실패는 provider DLL 위치 오류다.

보완 후 같은 CUDA acceptance는 1개 통과, 종료 0이었다. 검사 실행 2.15초, 캐시된 빌드 확인을 포함한 전체 명령 3.49초다. 실행 파일 SHA-256은 `a6242cb45fb7675ddd5115a2a6dff82a9d046d89d8710c9484a3ec6afd5b6c0b`다. 검사를 자동 재시도하거나 CPU EP로 바꿔 통과시키지 않았다.

`XAZZ_DEVICE=cuda:999` 대조 실행은 같은 CUDA 실행 파일에서 `Invalid device ID: 999, must be between 0 (inclusive) and 1 (exclusive)`로 실패했다. 첫 DLL 실패와 이 장치 오류 모두 `ONNX: execution provider setup failed`를 반환했고 acceptance 프로세스는 종료 101이었다. 이 결과는 ONNX 세션·백엔드의 명시적 EP 요청이 실패 시 CPU로 대체되지 않음을 확인한다. `xazz run` 전체 CLI의 종료 코드나 개별 그래프 노드의 GPU 배치·사용률은 이번 검사 범위가 아니다.

## 런타임과 재현

`ort-sys 2.0.0-rc.13`이 받은 ORT 배포본 버전은 1.28.0이다. CPU 검증 배포본에도 DirectML이 포함돼 있지만 요청 EP는 `cpu`로 고정했다. CUDA 배포본에 함께 든 TensorRT·DirectML 등을 검증한 것으로 해석하지 않는다.

| 배포 파일 | SHA-256 |
| --- | --- |
| `x86_64-pc-windows-msvc+directml.tar.lzma2` | `f7c654b3729cb9e5ad2a36a0c38e5b48e63bf4eed22968931aed33a0ad0b527d` |
| `x86_64-pc-windows-msvc+cuda13,tensorrt,nvrtx,directml.tar.lzma2` | `efb686cb49318e476cbb65d8ef40d4005f541a40bdee66afb7b60e3714e78293` |
| `nvidia_cublas-13.1.1.3-py3-none-win_amd64.whl` | `b6cdce694e47ff6aadf0a69df1cab6628d696f5ff56e8d16af50309d855fa20f` |

Visual Studio Developer PowerShell의 x64 환경에서 저장소 루트를 기준으로 실행한다. 다음 명령은 사용자 지정 `CARGO_TARGET_DIR`와 DLL 배치를 포함한다. CPU 빌드 후 CUDA 기능을 추가하면 최종 최적화·링크가 다시 필요하다.

```powershell
$env:CARGO_TARGET_DIR = "$PWD/target/onnx-msvc"
$env:XAZZ_LANG = "en"
Remove-Item Env:XAZZ_DEVICE -ErrorAction SilentlyContinue
$env:XAZZ_ORT_EP = "cpu"
cargo +stable-x86_64-pc-windows-msvc test --release --locked -p xazz-exec --features onnx -j 8 -- --ignored --nocapture

# CUDA 기능을 빌드만 한 뒤 테스트 실행 파일 옆에 provider DLL을 준비한다.
$env:ORT_CUDA_VERSION = "13"
cargo +stable-x86_64-pc-windows-msvc test --release --locked -p xazz-exec --features onnx-cuda -j 8 --lib --no-run
Copy-Item "$env:CARGO_TARGET_DIR/release/onnxruntime_providers_shared.dll" "$env:CARGO_TARGET_DIR/release/deps/"
Copy-Item "$env:CARGO_TARGET_DIR/release/onnxruntime_providers_cuda.dll" "$env:CARGO_TARGET_DIR/release/deps/"

python -m pip download --only-binary=:all: --no-deps --dest target/onnx-wheels nvidia-cublas==13.1.1.3
python -m zipfile -e target/onnx-wheels/nvidia_cublas-13.1.1.3-py3-none-win_amd64.whl target/onnx-runtime
$env:PATH = "$PWD/target/onnx-runtime/nvidia/cu13/bin/x86_64;$env:PATH"
$env:XAZZ_ORT_EP = "cuda"
cargo +stable-x86_64-pc-windows-msvc test --release --locked -p xazz-exec --features onnx-cuda -j 8 --lib -- --ignored --nocapture

# 이 명령은 실패해야 한다: 종료 101, Invalid device ID: 999
$env:XAZZ_DEVICE = "cuda:999"
cargo +stable-x86_64-pc-windows-msvc test --release --locked -p xazz-exec --features onnx-cuda -j 8 --lib -- --ignored --nocapture
Remove-Item Env:XAZZ_DEVICE
```

## 검토용 근거와 남은 범위

[검증 근거 압축 파일](evidence/windows-onnx-2026-10-06.zip)은 29개 실행·환경 파일과 `manifest.json`을 담는다. SHA-256은 `75a26e69b036e96dd256fdf0365cbcdc8bf9a70dfd8d3a73b22f681bde1dd3f3`다. 성공 로그뿐 아니라 최초 DLL 실패, 미컴파일 EP 요청, 잘못된 GPU 지정 실패, 일반 검사 결과를 함께 보존했다. 개인 절대 경로만 `<WORKSPACE>`·`<QA_ROOT>`·`<USER_HOME>`으로 치환했으며, 로컬 원본은 그대로 남겼다. manifest는 공개본과 원본의 해시를 각각 기록한다. 설치 프로그램·wheel·DLL·실행 파일은 용량 때문에 첨부하지 않고 버전과 해시를 기록했다.

#236의 Windows ONNX 항목은 이 기록으로 검증을 마쳤다. macOS CoreML은 별도 호스트 검증으로 남고, #127·#129는 보류다. CUDA 성능 튜닝, 큰 모델 속도 비교, 추가 연산자 지원을 이번 결과로 주장하지 않는다.
