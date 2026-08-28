# 보안 모델 — 프로세스 격리 vs OS 샌드박스

상태: 문서 v1 · 구현: 타임아웃 하드닝 (v0.3.0) · 코드: [`xazz-runner/src/main.rs`](../../xazz-runner/src/main.rs)

---

## 용어를 엄격하게

**subprocess isolation ≠ sandbox.**

- `xazz` CLI → `xazz-runner` → `xazz-exec` 의 서브프로세스 체인은
  **프로세스 격리(process isolation) + 의존성 격리**일 뿐이다.
- 이것만으로 "untrusted Xazz 프로그램을 안전하게 실행하는 sandbox"라고
  주장할 수 없다.

---

## 현재 격리 계층

| 계층 | 무엇을 제공하는가 | 무엇을 제공하지 않는가 |
|------|------------------|------------------------|
| 크레이트 격리 | CLI(Polars 없음) vs 실행 엔진(Polars+Burn) 바이너리 분리 | — |
| 서브프로세스 격리 | 실행 실패/크래시가 CLI 프로세스 셸로 영향 차단 · 실행 경로 고정(`XAZZ_RUNNER_PATH`/`XAZZ_EXEC_PATH`, PATH 폴백 제거) | OS 리소스·syscall 제한 없음 |
| 실행 타임아웃 | `XAZZ_EXEC_TIMEOUT_SECS`(기본 300초) 초과 시 kill, fail-closed | CPU/memory/캐시 제한 아님 |
| 가드레일 게이트 | 실행 전 PII/시크릿/경로 검사 (CLI + exec + API 3중) | 런타임 행동 통제 아님 |
| 입력 검증 | `xazz new` 프로젝트명, IDE 파일 경로, CSV 공식 주입 등 | 파일 크기 상한 아님 |

---

## OS 샌드박스(후속 마일스톤) — 구현 시 요구사항

진짜 샌드박스로 가려면 운영체제 수준에서 다음을 제한한다:

- **filesystem restriction** — 작업 디렉터리 고정, 읽기 범위 한정 (landlock)
- **network restriction** — outbound block (seccomp filter)
- **CPU / memory limit** — rlimit / cgroup
- **process / resource isolation** — 별도 uid/gid, 프로세스 수 제한
- **syscall restriction** — seccomp allowlist

각 항목은 플랫폼 의존적이며 정식 보안 리뷰가 필요하다. 그 전까지는
"프로세스 격리"라는 표현을 고수한다.

---

## 문서 규칙

README 등 사용자 문서에서 "샌드박스 런타임"이라는 표현은 사용하지 않는다.
대신 정확한 표현을 쓴다:

- 서브프로세스 실행 경계 → **프로세스 격리(process isolation)**
- 사전 실행 검사 → **정적 가드레일(guardrail) 게이트**
- 실행 시간 제한 → **타임아웃 하드닝**