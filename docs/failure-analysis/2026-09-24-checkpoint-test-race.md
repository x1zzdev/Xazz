# 병렬 체크포인트 테스트 실패 분석 — 2026-09-24

## 목표와 반복 실패 단계

Ubuntu/macOS/Windows에서 `xazz-exec` 테스트가 체크포인트를 안정적으로 저장하게 한다. 사용자가 보낸 CI 실패 화면은 [main run 35981187856](https://github.com/x1zzdev/Xazz/actions/runs/35981187856)과 일치한다. 그 실행의 세 OS 테스트 job은 모두 실패했다. PR #226에서 Clippy 오류가 수정된 후 [run 35983778069](https://github.com/x1zzdev/Xazz/actions/runs/35983778069)의 Windows/macOS job은 `cargo test`의 체크포인트 저장에서 실패했다. SUT(실제 학습·파일 저장)는 도달했다.

## 관측된 failure signature

1. 기존 macOS CI의 `backend::tests::cpu_backend_sweep_sorts_report_by_axis`: `checkpoint save failed: Unknown("Invalid argument (os error 22)")` ([#167 기록](https://github.com/x1zzdev/Xazz/issues/167#issuecomment-5810791221)).
2. PR #226의 Windows/macOS CI `backend::tests::device_train_materializes_in_memory_and_predicts`: 각각 `checkpoint save failed: FileNotFound("The system cannot find the path specified. (os error 3)")`, `checkpoint save failed: FileNotFound("No such file or directory (os error 2)")`. 해당 `xazz-exec` lib 테스트는 `82 passed; 1 failed`였다. Clippy는 이 실행에서 이미 통과했다.

오류 문자열과 테스트명은 달라도 병렬 테스트의 동일한 체크포인트 저장 단계 실패로 센다. 같은 diff를 재실행해 우연한 통과를 성과로 보지 않는다.

## 지킬 불변조건과 원인 근거

- 생산 코드의 `checkpoints/<model>.json` 저장·로드 계약과 모델 이름을 바꾸지 않는다.
- 테스트는 자기 체크포인트·매니페스트·ONNX 파일만 지우고 다른 테스트의 파일이나 공용 디렉터리는 건드리지 않는다.
- `dl.rs`의 학습 경로는 `create_dir_all("checkpoints")` 뒤 Burn `save_file`을 호출한다. `backend.rs`의 공용 `cleanup`과 parity 테스트, `dl/onnx_export.rs` 테스트는 모두 `remove_dir("checkpoints")`를 호출한다. 마지막 파일을 지운 테스트가 공용 디렉터리를 제거하면 다른 병렬 테스트가 디렉터리 생성 후 저장 전 틈에서 `FileNotFound`를 볼 수 있다. `remove_dir`는 실패를 무시하므로 경쟁 상태가 은폐된다.

## 대안과 판별

| 순위 | 대안 | 지지 증거 | 반대 증거·위험·비용 | 예상 실패와 관측값 |
|---|---|---|---|---|
| 1 | 테스트 정리 코드의 공용 `remove_dir("checkpoints")` 세 호출을 제거 | 삭제 경쟁을 직접 없앤다. 테스트별 파일 정리는 유지된다. | 테스트 후 빈 디렉터리가 남는다. 세 줄 삭제, 생산 동작 영향 없음. | 원인이 맞으면 CI 체크포인트 저장 실패가 사라진다. |
| 2 | 테스트마다 별도 작업 디렉터리/체크포인트 경로 | 완전 격리 | 현재 학습 API가 상대 경로를 고정하고 `current_dir`은 병렬 테스트의 전역 상태다. API 변경 위험 큼. | 다른 경로·전역 상태 경쟁 가능. 미실행. |
| 3 | `--test-threads=1` | 병렬 경쟁을 피할 수 있음 | 원인을 숨기고 CI 전체 테스트 시간을 늘리며 사용자의 기본 실행은 여전히 실패 가능. | 직렬에서만 통과할 수 있음. 미실행. |
| 4 | 저장 직전 디렉터리 재생성/재시도 | `FileNotFound`에 대응 | 다른 테스트가 다시 지울 수 있고 생산 코드에 우회 로직이 생긴다. | 간헐 실패가 남을 수 있음. 미실행. |

## 선택 가설과 사전등록 실험

선택 가설 하나: 테스트 정리 코드의 공용 디렉터리 삭제 세 호출이 경쟁 상태를 만들며, 이를 제거하면 병렬 체크포인트 저장 실패가 사라진다. 위험도는 LOW(테스트 전용 3줄 제거, 되돌릴 수 있음)로 판정한다. 프로세스 스킬 수를 재평가했고 완료 게이트는 관련 `cargo test -p xazz-exec --lib` 1회와 PR CI의 Windows/macOS 결과 확인으로 제한한다. reviewer는 요구하지 않는다.

- 성공: 로컬 `cargo test -p xazz-exec --lib` exit 0; 새 PR CI의 Windows/macOS `Test` job에서 해당 `xazz-exec` lib 테스트 통과. 한 번의 통과를 모든 플랫폼의 영구 무결성 증명으로 확대하지 않는다.
- kill: 같은 체크포인트 저장 실패가 새 PR CI에 재발하거나 로컬 관련 suite가 실패하면 후보를 채택하지 않고 새 증거로 분석을 갱신한다. 노이즈 바닥은 해당 단계 실패 0건.
- 변경 상한: 병렬 테스트 파일 정리 경합만 해소하며 생산 기능·기존 플랫폼 파일시스템 결함까지 해결했다고 주장하지 않는다.
- 롤백: 편집 전 anchor branch에서 `git restore --source anchor/checkpoint-test-race-20260924 -- xazz-exec/src/backend.rs xazz-exec/src/dl/onnx_export.rs` 한 명령.
- 사람만 답할 사항: 새 CI에서도 원인 판별이 안 되거나 실제 OS 파일시스템 접근이 필요할 때 테스트 호스트 제공.
