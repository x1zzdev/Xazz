# macOS CPU 검증 기록 — 2026-10-04

최신 서버 결과는 아래 **2026-10-05 후속 검증**에 있다. 간헐 실패의 원인을 확인해
수정한 뒤 전체 테스트 674개와 Clippy·포맷 검사를 통과했다. 앞선 검증 기록과
남은 배포 문제도 함께 보존한다.

관련 이슈: #167, #128, #262. 작업 브랜치: `fix/windows-smoke-167`.
시작 커밋: `af39779`(Windows 수정 사항과 macOS 인계 문서).

## 검증 환경

- macOS 26.6.2(25G83), Apple Silicon(`aarch64-apple-darwin`).
- Rust 1.99.0(`b940084d7`, LLVM 23.1.1), rustfmt, Clippy.
- Apple clang 21.0.0, Command Line Tools: `/Library/Developer/CommandLineTools`.
- Node.js 24.14.0, npm 11.9.0. 프런트엔드 의존성은 `npm ci`로 설치.
- CPU만 검증했다. GPU·ONNX 실행은 검증 범위에 포함되지 않는다.
- 빌드 크기를 줄이기 위해 `CARGO_BUILD_JOBS=4`, `CARGO_PROFILE_DEV_DEBUG=0`,
  `CARGO_PROFILE_TEST_DEBUG=0`을 사용했다. 테스트의 검증 조건은 유지했다.
- 도구 설치와 원시 로그는 Git에서 제외되는 `target/` 아래에 보관했다.
  전역 셸 설정은 변경하지 않았다.

## SDK 호환성 문제와 해결

첫 전체 테스트는 프로젝트 테스트가 실행되기 전에 의존성 링크 단계에서 실패했다.
기본 선택된 SDK 27.0의 `libSystem.B.tbd`에 대해 설치된 링커가
`arm64e.x1-macos` 아키텍처를 인식하지 못했다.
이후 이미 설치된 SDK 26.5를 해당 프로세스에서만 선택해 실행했다.

```sh
export SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk
```

시스템 SDK나 Xcode 설정은 바꾸지 않았다. 최초 실패 로그는
`target/qa-167/workspace-test-sdk27-failure.log`에 남겼다.

## 1차 검증 결과

| 검사 | 결과 |
|---|---|
| `cargo fmt --all -- --check` | 통과 |
| `cargo test --workspace` | 671개 통과, 실패·무시 0개 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 통과 |
| 배포 압축 파일 검사기의 단위 테스트 | 테스트 1개, 내부 ZIP/tar.gz 사례 8개 통과 |
| Visual IDE 빌드·계약·감사 체인·차트 통계·표준 출력 파서 검사 | 통과 |
| Visual IDE 명암 대비·브라우저 E2E | 각각 3개, 70개 통과. E2E는 모의 서버 기반 UI 검사 |
| 실제 CLI·HTTP 실행 | 프로젝트 생성·가져오기·검사·실행, 자원 계측·다른 테넌트 접근 차단·과거 기록의 계측 없음 확인 |
| `cargo deny --workspace --all-features check` | 실패. 기존 의존성 보안·라이선스 문제가 남아 있음 |

전체 테스트·Clippy의 최종 로그는 `target/qa-167/workspace-test-final.log`,
`target/qa-167/clippy-final.log`에 있다. 이 표는 당시 검증 범위의 결과이며,
모든 기능과 배포 준비가 완료됐다는 의미가 아니다.

## 공개된 macOS v0.3.1 배포본

실제 공개 압축 파일 `xazz-v0.3.1-macos-arm64.tar.gz`을 내려받아 공개된
체크섬과 대조했다. SHA-256은 다음과 같이 일치했다.

```text
4c442727fd723fcaad079ea4912c4b77407ead74f6da9303326a33655ba8b8be
```

압축을 푼 실행 파일의 버전은 `xazz 0.3.1`이다. 그러나 배포 데이터 검사기는
`xazz/examples/data/seoul_air_2008_2011.csv`가 실제 CSV가 아닌 Git LFS 포인터라며
배포본을 거부했다. Windows에서 확인한 패키징 문제가 macOS 배포본에도 존재한다.
현재 브랜치의 워크플로 수정만으로 이미 공개된 배포 파일이 고쳐지지는 않는다.
수정된 배포본과 남은 운영체제별 검증을 확인하기 전에는 #167을 닫으면 안 된다.

## 실제 서버 실행 중 수정한 문제

서버에서 CLI 경로를 지정하는 `XAZZ_EXEC_PATH`가 자식 프로세스에 전달되면,
러너가 이를 엔진 경로로 해석해 잘못된 실행 파일을 호출했다. HTTP 상태는 200이지만
본문은 `success:false`인 문제가 발생했다. 서버가 자식 환경에서 이 변수를 제거해
러너가 옆에 있는 `xazz-exec`를 찾도록 수정했다.

다음 명령은 별도 임시 프로젝트·데이터베이스로 실제 실행을 검증한다.

```sh
python3 scripts/smoke_cli_server.py --bin-dir target/debug
```

1차 실행에서는 10개 결과 행과 실행 시간 323ms, 사용자 CPU 224ms, 시스템 CPU 9ms,
최대 RSS 45,632KiB를 확인했다. 이는 기능 확인용 한 번의 측정이지 성능 보장이 아니다.
다른 테넌트는 404를 받았고, 계측 값이 없는 과거 기록은 `available:false`를 반환했다.
#128의 값은 실행 전체의 합계이며 단계별·GPU 계측으로 해석하면 안 된다.

## 남은 배포 차단 항목

- cargo-deny 0.20.2에서는 기능 옵션을 `check` 앞에 둬야 한다.
  인계 문서의 `cargo deny check --all-features`는 검사 전에 종료 코드 2로 끝난다.
- 올바른 명령으로 검사하면 라이선스 거부 16건, 유지보수 중단 항목을 포함한
  보안 권고 오류 11건, 배포 철회 버전 2건이 나온다. 금지 패키지·소스 검사는 통과했다.
  원시 근거는 `target/qa-167/deny.log`에 있다. 예외 추가나 정책 완화로 숨기지 않았다.
- 디버그 엔진 링크 시 `__eh_frame`이 16MiB를 초과한다는 링커 경고가 발생한다.
  CPU 실행은 성공했지만 릴리스 성능이나 스택 해제 동작까지 검증한 것은 아니다.
- 수정된 공개 배포본, GPU·ONNX, 두 기능 브랜치의 결합 상태는 아직 검증하지 않았다.

## 주말 작업의 1차 결과

팀장의 #105·#162·#163 코멘트를 확인했다. #127·#129는 보류 유지한다.
기존 초안 PR의 각 브랜치에 현재 `origin/main`을 반영하고 충돌을 로컬에서 해결했다.

| 작업 | 1차 결과 |
|---|---|
| [#162 / PR #248](https://github.com/x1zzdev/Xazz/pull/248) | 단일 타깃 층화, 희소 클래스 정책, 시간 경계 검사, 학습 데이터만 사용하는 전처리, 분할 JSON. 전체 테스트 678개·Clippy 통과 |
| [#163 / PR #249](https://github.com/x1zzdev/Xazz/pull/249) | 교차 엔트로피 학습, 원래 클래스 값 보존, macro 지표·AUC, 스윕 선택·CLI JSON. 전체 테스트 682개·Clippy 통과 |

각 브랜치의 설계 문서는 `docs/design/validation-split.md`,
`docs/design/classification-metrics.md`이다. 두 브랜치는 각각 검증했다.
#248 반영 후 #249와 겹치는 학습 코드를 통합하고 다시 검증해야 한다.
지원하지 않는 참조 Rust 내보내기 옵션은 명시적으로 거부하며 #129 구현을 재개하지 않았다.
이 1차 검증 시점에는 커밋·푸시·PR 수정·이슈 댓글·배포 게시를 하지 않았다.

## 2차 재검증 — 기존 검사와 다른 기준

사용자 요청에 따라 1차 테스트를 반복하는 것에 그치지 않고, 입력 전수 검사·독립 수식·
반환된 모델·실제 프로세스의 결과를 대조했다. **1차 테스트 통과는 사실이지만 검증 범위가
부족했다. 아래 문제를 추가로 발견해 수정했으므로 1차 통과만으로 완료를 판단하면 안 된다.**

### 추가로 발견하고 수정한 문제

| 문제 | 재현과 수정 |
|---|---|
| 잘못된 검증 비율을 조용히 보정 | `-0.1`, `0.95`가 허용되던 것을 오류로 처리. 양수 비율인데 검증 행이 0개인 경우도 거부 |
| 분류 스윕에서 명시한 지표 정보 유실 | 조합 확장 중 명시 여부가 사라져 `metric: "mse"`가 분류에 허용됨. 명시 여부를 보존해 회귀 지표를 거부 |
| 배치 크기 1에서 분류 학습 비정상 종료 | `[1,1]` 타깃 텐서의 모든 크기 1 축을 없애던 코드를 수정해 타깃 축만 제거 |
| 보고 손실과 실제 반환 모델의 손실 불일치 | 검증 중 Dropout을 끄고, 분류 보고서의 최종 학습·검증 손실은 반환 모델의 평가 출력으로 다시 계산 |
| 실행 오류가 성공으로 보고됨 | 파이프라인 오류를 로그만 남기고 계속 진행하던 것을 즉시 실패 반환으로 변경. 뒤의 파이프라인을 실행하지 않으며 CLI·HTTP도 실패를 보고 |
| 큰 정수 시간값의 정밀도 손실 | 시간 정렬 시 Float64로 변환하지 않고 원래 타입을 보존. `2^53`보다 큰 인접 정수와 `-0.0/+0.0` 경계를 검사 |

### 독립 대조 결과

| 기준 | 결과 |
|---|---|
| 분할 규칙 전수 검사 | 행 수 1~7, 클래스 값 3종, 검증 비율 9종, 분할 방식 3종의 **88,533건** 통과. 행 누락·중복, 클래스 유지, 정확한 검증 크기 검사 |
| AUC의 다른 계산 방식 | 순위 합 구현을 양성·음성 모든 쌍의 대소 비교와 대조한 **4,096건** 통과. 동점·단일 클래스 포함 |
| macro 지표 직접 계수 | 2~4개 클래스의 정답·예측 조합 **72,353건**에서 정밀도·재현율·F1·정확도 대조 통과 |
| 반환 모델의 손실 재계산 | 모델 출력으로 MSE·교차 엔트로피를 별도 계산해 보고서와 대조. Dropout과 배치 크기 1 포함 |
| 실제 CLI의 잘못된 입력 | 잘못된 비율·시간 옵션 조합·분류 지표·출력 폭 등 **7건 모두 종료 코드 1과 `success:false`**, 패닉 없음 |
| 실제 CLI의 정상 분류 | 이진·다중 클래스, 배치 크기 1·3, Dropout, F1·교차 엔트로피 스윕 **3개 시나리오** 통과. 예측 행에서 지표를 직접 재계산하고 우승 조합을 대조 |
| 실제 서버 | 실행 경로 명시/기본 경로 **두 방식**에서 정상 실행·자원 계측·테넌트 격리·계측 없는 기록·오류 전파 통과 |
| 공개 배포본 직접 검사 | 검사기를 사용하지 않고 압축 내부 CSV 앞부분을 읽어도 LFS 포인터. 체크섬 재계산도 동일. 배포 문제는 여전히 남음 |

### 수정 후 전체 검사

| 브랜치 | 전체 테스트 | Clippy·포맷 |
|---|---|---|
| `fix/windows-smoke-167` | 672개 통과, 실패·무시 0개 | 통과 |
| `scaffold/162-stratified-timeseries-split` | 683개 통과, 실패·무시 0개 | 통과 |
| `scaffold/163-classification-metrics` | 688개 통과, 실패·무시 0개 | 통과 |

중간 실행에서는 서버의 모의 러너 시작 신호를 5초 안에 기다리는 테스트가
간헐적으로 실패했다. 단독 서버 검사와 최종 전체 재실행은 통과했지만,
**간헐 실패의 원인은 확정하지 못했다. 최종 통과가 테스트 안정성까지 보장하지는 않는다.**
실패 기록을 지우거나 해당 테스트를 무시 처리하지 않았다.

### 근거와 재실행

주요 원시 근거는 Git에서 제외되는 `target/qa-167/recheck/`에 모았다.

- `negative-before.json`, `negative-after.json`: 수정 전·후 잘못된 CLI 입력 결과.
- `positive-after.json`, `feature-cli.log`: 정상 분류의 독립 대조와 기존 분할·스윕 실행.
- `split-independent-before.log`, `classification-independent-before.log`: 추가한 검사가 수정 전 실패한 기록.
- `server-override.log`, `server-sibling.log`: 현재 소스로 빌드한 서버의 실제 실행 결과.
- `server-startup-failure.log`: 모의 러너 시작 대기 테스트의 간헐 실패 기록.
- `release-independent.json`: 압축 내부 직접 검사와 체크섬 재계산.
- `workspace-test.log`, `split-workspace-test.log`, `classification-workspace-test.log`: 최종 전체 테스트.
- `clippy.log`, `split-clippy.log`, `classification-clippy.log`: 최종 Clippy.

각 브랜치의 Rust 테스트에 전수·독립 재계산·오류 전파 검사를 남겼다.
서버 검사도 아래처럼 두 경로에서 재실행할 수 있다.

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
python3 scripts/smoke_cli_server.py --bin-dir target/debug --cli-path-mode override
python3 scripts/smoke_cli_server.py --bin-dir target/debug --cli-path-mode sibling
```

검증 범위는 macOS CPU의 각 개별 브랜치이다. GPU·ONNX, #162와 #163의 통합 상태,
새 공개 배포본, 의존성 보안·라이선스 문제의 해결은 여전히 확인하지 않았다.
이 2차 검증 시점에는 커밋·푸시하지 않았다.

## 2026-10-05 후속 검증 — 서버 시작 실패

macOS 시스템 로그에서 실패한 테스트의 임시 셸 파일에 대한 보안 검사가
진행되던 중 5초 시작 대기가 끝나 파일이 삭제되고, 이후 파일 없음과 실행 거절이
발생한 것을 확인했다. 독립 대조에서도 임시 파일 직접 실행은 한 번에 7.006초가
걸렸고, 같은 내용을 시스템 셸에 입력한 6회는 각각 0.005~0.012초였다.
시스템 보안 설정을 변경하거나 대기 한도를 늘리지 않았다.

모의 실행은 `/bin/sh`에 코드 데이터를 전달하도록 바꾸었다. 제어 소켓으로
시작·요청 취소·잠금 보유 확인·종료 순서를 맞추며, 고정 시간 sleep에 기대지 않는다.
검사 중 단언이 실패해도 입력 대기를 해제해 작업이 남지 않게 했다.
실제 서버의 CLI 실행 인수는 유지하고 종료 코드·종료 신호를 응답과 실행 기록에 보존한다.

| 검사 | 결과 |
|---|---|
| 서버 전체 테스트 | 120개 통과 |
| 동시 실행 대조 | 4개 프로세스로 서버 전체 검사 40회, 총 4,800개 통과. 실패·무시 0개 |
| 오류 정보 대조 | 종료 코드 23, 종료 신호 15, stderr와 종료 코드 24가 응답·DB에 동일하게 기록됨 |
| 중간 정리 | 모의 실행 객체를 제거하면 대기를 해제하고 실행 슬롯 반환 |
| 전체 작업공간 | 674개 통과. 실패·무시 0개 |
| Clippy·포맷 | `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check` 통과 |
| 실제 CLI·HTTP | 새로 빌드한 서버에서 정상 실행·계측·격리·오류 전파 통과. 실패에 종료 상태 포함 |

전체 검사 첫 실행은 샌드박스가 로컬 TCP 포트 생성을 막아 20개가 `PermissionDenied`로
실패했다. 해당 로그를 보존하고 로컬 포트 권한을 허용한 동일 명령으로 위 결과를 얻었다.
이 환경 오류를 제품 결함이나 기존 간헐 실패와 섞어서 판단하지 않았다.

추가 근거는 `target/qa-167/recheck/`의 `server-startup-system-policy.log`,
`server-spawn-comparison.jsonl`, `server-handshake-repeat.jsonl`,
`server-handshake-smoke.log`, `workspace-test-sandbox-bind-failure.log`,
`workspace-test-server-fix.log`, `clippy-server-fix.log`에 있다.

기존 검증 완료 변경은 커밋·푸시했으며 [PR #282](https://github.com/x1zzdev/Xazz/pull/282)를
초안으로 등록했다. [PR #248](https://github.com/x1zzdev/Xazz/pull/248)과
[PR #249](https://github.com/x1zzdev/Xazz/pull/249)의 코드·한국어 설명도 갱신했다.
이 서버 결과는 macOS CPU 검증이며 공개 배포본, GPU·ONNX, 의존성 보안·라이선스 문제의
해결까지 의미하지 않는다.

### 원격 의존성 검사가 성공한 이유

PR #282의 [의존성 정책 검사](https://github.com/x1zzdev/Xazz/actions/runs/37212553893/job/111466509352)는
성공했지만 전체 작업공간 검사가 아니다. CI와 로컬 모두 cargo-deny 0.20.2이며,
CI의 실제 명령은 `cargo deny --manifest-path ./Cargo.toml --all-features check`였다.
`--workspace`가 없고 루트 manifest에 `[package] xazz`가 있으므로 CLI의 의존성 그래프만
대상으로 삼는다. CLI가 직접 의존하지 않는 실행 엔진·서버의 의존성은 빠진다.

따라서 원격 성공은 로컬 `cargo deny --workspace --all-features check`에서 발견한
전체 작업공간의 라이선스·보안 권고·배포 철회 문제를 해결했다는 근거가 아니다.
CI 검사 범위를 전체로 넓히는 변경과 기존 의존성 문제 해결은 별도 후속 작업으로 남아 있다.
