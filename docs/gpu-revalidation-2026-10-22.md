# CUDA·ONNX 실기 재검증 — 2026-10-22

## 진행 계획

예상 45~90분. 네이티브 의존성 전체 재빌드가 필요하면 약 2시간까지 걸릴 수 있다.

- [x] 최신 main에서 전용 브랜치 생성: `codex/gpu-acceptance-gates`
- [x] Windows RTX 4070 Laptop GPU, 드라이버 591.44 및 GNU/MSVC Rust 툴체인 확인
- [x] CUDA 런타임 구성과 기존 빌드 캐시 확인
- [ ] 최신 코드에서 CUDA 학습·추론 실기 acceptance 실행
- [ ] MSVC ONNX CPU·CUDA 추론 acceptance 및 실패 대조 실행
- [x] GPU 기능 컴파일 CI와 재현 가능한 실기 검사 절차 구현 (실행 검증 진행 중)
- [ ] 실제 결과·환경·실행 개수·미검증 범위 기록
- [ ] #62·#63 본문 정리 및 근거 연결
- [ ] 필수 검사 후 PR 생성

## 판정 기준

기본 기능의 workspace 테스트 통과를 GPU 실기 통과로 간주하지 않는다.
CUDA는 요청한 backend 선택, 실제 장치 학습의 유한한 loss,
동일 체크포인트의 CPU·CUDA 추론 편차 및 장치 학습 산출물의 추론을 확인한다.
ONNX 학습은 CPU에서 수행하므로 ONNX GPU 학습 검증으로 표현하지 않는다.
ONNX CPU·CUDA EP를 각각 명시하고, 잘못된 장치 및 미컴파일 EP 요청의 실패도 확인한다.
실기 테스트는 정확한 이름으로 실행하고 0개 실행을 성공으로 기록하지 않는다.

## 현재 기준

- 기반 main: `b347688b3b4906a53ddb0b4323aed9041e117628`
- 과거 CUDA 근거: PR #263 (2026-10-01 병합)
- 과거 ONNX 근거: PR #289 (2026-10-06 병합)
- #62의 오래된 미완료 표시와 #63의 검증/이관 범위를 새 결과에 맞춰 정리한다.
- 실기 결과: **진행 중**. ONNX 호환성 수정 후 첫 CPU acceptance 통과.
  강화한 최종 판정 버전과 CUDA 결과는 아래에 이어 기록한다.

## 중간 진행

- [Draft PR #329](https://github.com/x1zzdev/Xazz/pull/329) 생성. 새 기능 컴파일 CI 실행 중.
- Python 검증 결과 판정 테스트: 기존 11개 Windows/Linux CI 통과, DLL 버전 정합성 검사까지 보강한 13개 로컬 Windows 통과.
- 최신 의존성으로 GNU CUDA·MSVC ONNX release 빌드·실기 실행 중. CUDA 13.1 NVRTC·헤더와 cuBLAS 기존 설치본을 사용한다.
- 문서 CI에서 새 문서의 목차 등록 누락을 발견해 `SUMMARY.md`에 추가했고, 수정 후 문서 빌드·링크 검사가 통과했다.
- 새 CI가 수정 전 ONNX 컴파일 오류를 실제로 검출했다: [실행 기록](https://github.com/x1zzdev/Xazz/actions/runs/39710751666).
- ONNX API·protobuf 수정 후 첫 CPU acceptance: **1 passed, 0 failed, 0 ignored**, 예측 최대 편차 0, CPU 실행 노드 2개, 실행 0.35초. 최종 증거는 강화한 계산 노드 판정으로 다시 수집 중이다.

## 새로 재현한 ONNX 호환성 오류

최신 main의 의존성 조합을 사용하는 첫 MSVC 실기 빌드가 실패했다.
기본 workspace 검사에서는 `onnx` 기능이 비활성이라 컴파일되지 않던 경로다.

1. `ort 2.0.0-rc.14`에서는 `ort::init().commit()`이 없어지고
   환경을 만들어 `Session::builder(&environment)`에 전달해야 한다.
   [공식 변경 안내](https://github.com/pykeio/ort/releases/tag/v2.0.0-rc.14).
2. `rlx-onnx-proto 0.2.18`의 생성 타입은 `protobuf 1.7`의 `Message`를 구현하지만,
   직접 의존성만 `protobuf 3.7`로 올려 서로 다른 trait를 사용하고 있었다.
   `write_to_bytes`와 `merge_from_bytes` 호출에서 컴파일 오류가 났다.

수정: ORT 환경을 한 번 생성해 모든 세션에 명시적으로 전달하고 초기화 오류를 전파한다.
ORT의 지원 API를 rc.14로 명시한다. protobuf는 생성 타입과 같은 기존 1.7 의존성에
맞추고 사용되지 않는 3.7·protobuf-support를 제거한다. transitive protobuf 1.7은
기존에도 남아 있었으며, 이번 수정은 해당 라이브러리의 보안 경고를 해결하지 않는다.
생성기/스키마 마이그레이션은 #63·#127의 별도 범위다.

원래 실패와 중간 성공을 보존한 뒤 최종 소스의 결과를 수집한다.

## 런타임 준비에서 확인한 추가 문제

- ORT 1.31.0 CUDA 배포본은 다운로드 체크섬 검증·추출 후 캐시 폴더 이름 변경에서
  Windows `os error 5`로 실패했다. `ort-sys`의 검증 순서를 확인하고, 이번에 생성한
  임시 폴더를 같은 캐시의 최종 이름으로 옮겨 복구했다. 원래 실패 로그를 보존한다.
- `ort-sys`의 DLL 복사 코드는 기존 일반 파일을 교체하지 않았다. 실제로
  `release/onnxruntime_providers_cuda.dll`은 62,276,096 bytes인 구버전이고,
  이번 1.31.0 링크 경로의 DLL은 93,791,744 bytes였다. 검사 도구는 이제 `release/`를
  원본으로 삼지 않고 Cargo가 보고한 현재 ORT 링크 경로에서 DLL을 가져온다.
  파일 해시를 기록하며, 오래된 캐시를 가리키는 심볼릭 링크도 덮어쓰지 않는다.
- CUDA 최종 실기: 1개 통과, 예측 편차 0, 유한한 장치 학습 loss. CUDA 일반 검사:
  138개 통과, 실기 1개는 별도 실행했으므로 일반 목록에서는 ignored로 표시된다.
- 기본 workspace 테스트 746개 통과, 실패·제외 0개. Clippy 통과. 이 수치에는 GPU
  기능 검증이 포함되지 않으며 위의 실기 결과와 별도로 해석한다.
