# Windows CI 실패 로그 조회 반복 실패 분석 — 2026-09-24

## 목표와 실패 단계

PR #224의 Windows CI 실패 원인을 확인한다. 앞서 PR #213의 Windows CI 로그를 가져오려는 같은 목적의 사전 단계에서 연속 두 번 실패했다. 두 시도 모두 테스트 본문이나 실패 로그를 읽는 단계에 도달하지 못했다(`pre-SUT`). PR #224는 2026-09-24 09:26:59 UTC `Test · windows-latest=FAILURE`였으며 실패 job ID는 `107568811717`이다.

## 두 failure signature와 도달 여부

1. `gh api` 호출에 맞지 않는 flag를 사용해 CLI 옵션 처리 단계에서 실패. 정확한 오류 문자열은 이전 도구 출력이 현재 기록에 남아 있지 않다. SUT(Windows job 실패 로그) 미도달.
2. 로그 조회 호출의 ANSI escape 처리/파싱 단계에서 실패. 정확한 오류 문자열은 이전 도구 출력이 현재 기록에 남아 있지 않다. SUT 미도달.

오류 문자열이 달라도 같은 로그 조회 사전 단계의 연속 실패로 세며, 이 문서 작성 전에는 세 번째 조회를 실행하지 않았다.

## 불변조건

- CI 실패를 제품 결함이나 기존 환경 결함으로 추측해 단정하지 않는다.
- workflow 재실행, CI 설정 변경, 코드 수정으로 실패 증거를 덮지 않는다.
- 인증 정보와 원시 로그의 민감값을 공개 이슈나 PR 댓글에 그대로 싣지 않는다.
- 정확히 한 조회 방법만 선택해 실패 단계에 도달했는지 판별한다.

## 대안과 근거

| 순위 | 대안 | 지지 증거 | 반대 증거·위험·비용 | 예상 실패와 관측값 |
|---|---|---|---|---|
| 1 | `gh run view --job 107568811717 --log-failed` | 설치된 `gh run view --help`가 job별 `--log-failed`를 명시한다. 잘못된 API flag와 수동 ANSI 파싱을 피한다. | GitHub의 zip 로그 연결 제한으로 조회 자체가 실패할 수 있다. 읽기 전용, 비용 낮음. | 성공: 실패 step의 로그가 나온다. kill: 같은 pre-SUT 조회 오류가 난다. |
| 2 | GitHub Actions REST job log URL 직접 요청 | job ID가 확인됐다. | 이전 `gh api` flag 실패와 같은 계열이며 리다이렉트/인코딩 처리가 필요하다. 비용 중간. | 명령 단계 또는 리다이렉트 단계 오류 가능. 미실행. |
| 3 | 브라우저의 Actions job 화면에서 실패 step 확인 | 링크가 있다. | 화면 로그인/렌더/접근성 상태에 의존하고 수동 추출이 필요하다. 비용 중간. | 화면 접근 실패 또는 로그 접힘 가능. 미실행. |

## 선택 가설과 사전등록 판별

선택 가설 하나: CLI의 공식 `gh run view --job ... --log-failed` 경로는 수동 API flag·ANSI 파싱 없이 해당 실패 step 로그를 제공한다. 문서 저장 후 이 조회를 한 번 실행한다. 성공 기준은 실패 step의 명령과 오류 줄을 확인해 SUT에 도달한 것이다. 조회가 다시 pre-SUT에서 실패하면 실행을 동결하고 이 문서에 관측값을 더한다. rollback은 불필요한 읽기 전용 조회이며 CI·코드·설정은 바꾸지 않는다. 사람만 답할 사항은 계정 권한 부족 또는 로그 비공개가 확인될 때의 접근 권한이다.

## 판별 실행과 동결

- `gh run view --job 107568811717 --log-failed` 1회 실행, exit 1.
- 관측값: `run 35979829706 is still in progress; logs will be available when it is complete`.
- GitHub run 전체가 끝나지 않아 실패 job의 로그를 받지 못했다. SUT 미도달이며 성공 기준 미충족. 현재 원인은 조회 도구의 flag나 파서가 아니라 **run 완료 전 로그 비공개**라는 외부 상태다.
- kill 기준에 따라 로그 조회를 다시 실행하지 않고 동결한다. CI 실패를 코드 결함으로 판정하지 않는다. 새 run 완료 상태와 사용자 재개 지시가 있으면 이 문서의 가설·기준을 갱신한 뒤 한 번만 다시 판별한다.

## 사용자 재개와 새 증거 (2026-09-24)

- 사용자가 "다시 진행"을 명시했다. GitHub run `35979829706`은 `completed/failure`로 바뀌었고 Windows job `107568811717`도 완료됐다. 이전 차단 원인인 run 진행 중 상태가 해소됐다.
- 같은 공식 CLI 경로 `gh run view --job 107568811717 --log-failed`를 한 번만 다시 조회한다. 성공 기준은 실제 실패 step과 오류 줄을 읽는 것이다. 동일한 사전 단계 오류가 나면 다시 동결하고 새 대안으로 자동 전환하지 않는다.

## 로그 조회 결과와 국소 수정 실험

- 재개 후 동일 명령 1회는 exit 0이며 Windows 실패 step에 도달했다. 실패는 `cargo clippy`의 `src/schema.rs:311:1`에서 `items after a test module`; 테스트 모듈 뒤 `infer_columnar_schema_via_runner`가 `src/schema.rs:365`에 남아 있고 `-D warnings`로 실패했다. 이전 `pre-SUT` 실패와 구분된다.
- 위험도 재판정: 테스트 모듈의 위치만 옮기는 되돌릴 수 있는 국소 수정이므로 LOW. 사용자 기능·API·테스트 기대값은 바꾸지 않는다. 프로세스 스킬이 3개 겹치므로 LOW의 targeted check 1회만 완료 게이트로 삼고 reviewer를 추가하지 않는다.
- 가설 하나: `#[cfg(test)] mod tests`를 파일의 마지막 항목으로 이동하면 Clippy의 `items_after_test_module` 실패가 사라진다.
- 사전등록 성공: 저장소 CI와 같은 `cargo clippy --workspace --all-targets -- -D warnings`가 exit 0. kill: 동일 lint가 남거나 다른 새 경고가 발생하면 변경을 채택하지 않고 원인을 재분석한다. 노이즈 바닥은 경고 0개이며 1개도 허용하지 않는다. 적용 상한은 이 한 lint/CI 차단 해소이고 다른 플랫폼 테스트 실패 해결을 주장하지 않는다.
- 롤백: 편집 전 anchor branch를 만들고 `git restore --source <anchor> -- src/schema.rs` 한 명령으로 원본 bytes를 복구한다.

## 실험 영수증

- 원본 anchor: `anchor/ci-clippy-schema-20260924` = `2ae07cffed32187e7be013925b337dbb5e134814`.
- 테스트 모듈을 `src/schema.rs`의 마지막 항목으로 이동했다. 테스트 내용과 `infer_columnar_schema_via_runner` 본문은 그대로다.
- 첫 로컬 검사 호출은 `zsh:1: command not found: cargo`로 SUT 미도달. 설치된 Cargo가 `/Users/gibeom/.cargo/bin/cargo`에 있음을 확인하고 agent-owned PATH 입력만 수정했다.
- `/Users/gibeom/.cargo/bin/cargo clippy --workspace --all-targets -- -D warnings`: exit 0, `Finished dev profile ... in 3.17s`. 사전등록 성공 충족, 경고 0개. Windows runner에서의 재검증은 PR CI 결과로 별도 확인한다.
- ledger: 테스트 모듈을 파일 끝으로 이동 → Clippy 실패 1개에서 로컬 Clippy 경고 0개 → 채택.
