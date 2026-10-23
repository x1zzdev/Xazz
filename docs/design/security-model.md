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

## OS 샌드박스 계획 — 마일스톤

"프로세스 격리"에서 "샌드박스"로 넘어가는 경로를 플랫폼별로 정의한다. 원칙은
**최소 침습(minimal intrusion)**: root 자격 증명이나 커널 설정 변경 없이
비특권으로 적용 가능한 제한부터 켠다.

### 플랫폼별 가용 수단

| 제한 | Linux | macOS | Windows | 비고 |
|------|-------|-------|---------|------|
| 파일시스템 | landlock LSM (5.13+, 비특권) | seatbelt `sandbox_init` (비공개 API) | AppContainer / low-integrity token | landlock이 이식성·안정성 우위 |
| syscall | seccomp-bpf (비특권, `no_new_privs`) | seatbelt profile | — (Job Object는 syscall 필터가 아님) | seccomp allowlist는 유지비용 큼 |
| 네트워크 | seccomp로 `socket`/`connect` 차단, 또는 netns | seatbelt | AppContainer network capability | 실행 엔진은 기본 네트워크 불필요 |
| CPU / 메모리 | rlimit(`RLIMIT_AS`/`RLIMIT_CPU`) 또는 cgroup v2 | rlimit | Job Object 메모리/CPU 회계·상한 | Windows는 이미 Job Object 회계 사용(#128) |
| 프로세스 수 | rlimit `RLIMIT_NPROC` / cgroup pids | rlimit | Job Object active-process limit | |
| 자격 증명 | 별도 uid/gid (`setuid`) — root 필요 | — | AppContainer SID | 최후순위(특권 필요) |

### 최소 침습 경로 (권장 순서)

1. **M1 — Linux landlock(파일시스템).** 실행 디렉터리 밖 읽기/쓰기를 차단한다.
   비특권, 커널 5.13+. 구커널/미지원이면 경고 후 미적용(현 동작 유지, fail-closed 아님).
2. **M2 — rlimit(CPU/메모리/프로세스).** 전 플랫폼 공통 상한. 벽시계
   `XAZZ_EXEC_TIMEOUT_SECS`와 별개로 `RLIMIT_AS`/`RLIMIT_CPU`/`RLIMIT_NPROC` 추가.
3. **M3 — seccomp-bpf(네트워크/syscall).** `no_new_privs` + allowlist로 outbound 차단.
   네트워크가 불필요한 파이프라인에 opt-in.
4. **M4 — cgroup v2 / Job Object 하드 상한.** 컨테이너·서버 배포용.

### 완료 기준(마일스톤별)

- **M1**: 작업 디렉터리 밖 `load()`가 실패하고, 정상 파이프라인은 그대로 통과.
- **M2**: 메모리 폭주 파이프라인이 OOM kill 대신 명시적 오류로 종료.
- **M3**: 네트워크 접근 파이프라인이 차단되고, 허용 모드에서만 통과.
- **M4**: cgroup/Job Object 상한 초과가 런타임 오류로 보고.

### 정식 보안 리뷰 요건

landlock/seccomp/seatbelt/AppContainer는 **플랫폼 의존적이고 우회 표면이 넓다**.
구현 시 다음을 요구한다.

- 플랫폼별 위협모델과 우회 시나리오 문서화.
- 제한의 적용/미적용 상태를 실행 결과(`resources`/로그)에 명시(fail-open 금지).
- 각 제한의 "적용됨/미적용"을 검증하는 회귀 테스트.
- 외부 보안 리뷰(최소 1인) 통과 전에는 사용자 문서에서 "샌드박스" 표현 금지.

그 전까지는 "프로세스 격리"라는 표현을 고수한다.

---

## 문서 규칙

README 등 사용자 문서에서 "샌드박스 런타임"이라는 표현은 사용하지 않는다.
대신 정확한 표현을 쓴다:

- 서브프로세스 실행 경계 → **프로세스 격리(process isolation)**
- 사전 실행 검사 → **정적 가드레일(guardrail) 게이트**
- 실행 시간 제한 → **타임아웃 하드닝**