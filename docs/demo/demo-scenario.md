# 2차 평가 데모 시나리오 — Visual IDE 5분 (#154)

2026 오픈소스 개발자대회 2차 평가(발표·데모 완성도·정보 전달력)를 위한 라이브 데모 대본이다.
화면에 보이는 값은 모두 실제 xazz-server 응답이다. 서버가 돌려주지 않는 값은 IDE가
`Not available in this version`으로 표시하므로, 발표에서도 그 표시를 숨기지 않는다.

- 녹화본: `docs/assets/demo/xazz-demo-ko.webm` (한국어 자막, 약 56초 — 라이브 데모가 막히면 이 영상으로 대체).
  영어본은 `node scripts/demo.mjs --lang en`으로 만든다. 바이너리가 이력에 쌓이지 않도록 한국어 최종본 한 벌만 커밋한다.
- 재현 스크립트: `visual-ide/scripts/demo.mjs` (아래 "재현" 절)
- 장면별 스크린샷: `docs/assets/ide_*.png` (아래 표의 "증빙")

## 사전 준비 (발표 30분 전)

1. 저장소 루트에서 빌드하고 서버를 **저장소 루트에서** 실행한다. `load("visual-ide/data/...")`
   경로가 루트 기준이기 때문이다.

   ```bash
   cargo build --release -p xazz -p xazz-server -p xazz-runner -p xazz-exec
   ./target/release/xazz-server
   ```

   `XAZZ_EXEC_PATH`는 설정하지 않는다. xazz-runner도 같은 변수로 xazz-exec를 찾기 때문에,
   이 변수를 `xazz`로 지정하면 러너가 `xazz`를 다시 호출해 `unrecognized subcommand`로 실패한다.
2. IDE를 연다: `cd visual-ide && VITE_API_BASE_URL=http://127.0.0.1:8005 npm run dev`
3. 상단 `Location · xazz-server connected`를 확인한다.
4. 정책 팩을 설치해 둔 상태라면 Monitor → Policy packs에서 제거해 전역 정책으로 시작한다.

## 5분 타임라인

| 시각 | 화면 | 말할 것 (핵심 한 문장) | 증빙 |
|---|---|---|---|
| 0:00–0:30 | 작업 공간 Split 보기, 왼쪽 목록에서 `Filter threshold` 선택 | "그래프의 노드를 고르면 `.xzz` 코드의 해당 줄과 영향 범위가 함께 강조됩니다. 파이썬 노트북과 달리 타입이 먼저 검사됩니다." | `ide_workspace.png` |
| 0:30–1:10 | `Full Run` → 확인 체크 → `Start full run` | "진행 표시는 브라우저가 잰 경과 시간과 요청 단계만 보여줍니다. 서버가 에폭 이벤트를 보내지 않으므로 에폭 진행률은 '제공되지 않음'으로 둡니다." | `ide_run_progress.png` |
| 1:10–1:30 | Result dock → `Receipt` | "실행 기록에 서버가 발급한 Run ID가 남습니다. 프로세스 종료와 파이프라인 성공은 서로 다른 축으로 표시합니다." | — |
| 1:30–2:10 | `Monitor` → Burn 패널, DP 리포트 → 아래 Governance | "Burn 학습 결과와 이번 실행의 ε 사용량이 실측으로 바뀌고, 테넌트 전체 ε 원장에도 누적됩니다." | `ide_dp_ledger.png` |
| 2:10–2:50 | Audit hash chain → (선택) 디스크의 `audit_log/audit.jsonl` 한 줄 수정 → `Verify chain` | "모든 실행이 SHA-256 해시 체인에 남습니다. 한 글자만 바꿔도 서버가 체인 불일치를 판정하고, IDE가 몇 번 레코드인지 짚어냅니다." | `ide_audit_chain.png`, `ide_audit_tamper.png` |
| 2:50–3:20 | Result dock → `History` → 방금 실행 선택 → `Restore result in Preview` | "실행 이력은 서버를 재시작해도 남습니다. 이번 브라우저 세션에서 받은 결과는 다시 펼칠 수 있고, 그렇지 않은 실행은 메타데이터만 있다고 정직하게 표시합니다." | `ide_run_history.png` |
| 3:20–3:45 | Split → 인스펙터 `Trace columns` → `pm25` 선택 | "집계 결과 컬럼이 어느 원본 컬럼에서 왔는지 정적 컴파일만으로 추적합니다. 아무것도 실행하지 않습니다." | `ide_lineage.png` |
| 3:45–4:30 | Monitor → Policy packs에서 `examples/security/finance_policy.json` 설치 → Edit에서 Select에 `phone` 추가 → `Check policy` → `Remediate` | "금융 정책 팩을 이 테넌트에만 설치하고, 변경은 이력에 남습니다. 식별자 컬럼이 결과로 나가면 실행 전에 차단되고, 검증된 보정안이 diff로 나옵니다." | `ide_policy_packs.png`, `ide_policy_block.png` |
| 4:30–5:00 | Edit → File Input에 CSV 업로드 | "CSV를 올리면 서버가 UTF-8·EUC-KR을 판별해 스키마를 추론하고 노드를 채웁니다. 오프라인이면 브라우저 추론으로 대신하고 그 사실을 표시합니다." | `ide_schema_inference.png` |

한국어 발표에서는 우상단 `한국어` 토글로 전환한 상태에서 같은 순서로 진행한다
(`ide_governance_ko.png`). 상태축 어휘(`Verified`, `Mismatch`, `Exited` 등)는 계약상 영어로 유지된다.

## 예상 질문

- **에폭 진행률이 왜 없나?** 현재 `/execute`는 프로세스가 끝난 뒤 한 번에 응답하고 스트리밍하지 않는다.
  없는 값을 추정해 그리지 않는 것이 IDE의 원칙이다. 서버가 진행 이벤트를 내보내면 오버레이가 받는다.
- **감사 체인 판정은 누가 하나?** 서버(`GET /security/audit/chain`)가 기준이다. 브라우저는 같은 계산을
  재현해 끊긴 위치만 알려 준다. 브라우저 재현은 HTTPS나 localhost에서만 동작한다.
- **다른 테넌트 데이터가 보이지 않나?** 서버가 테넌트별로 격리한다. 다른 테넌트의 실행은 삭제된 실행과
  똑같이 404로 응답한다. IDE의 Server access 값은 메모리에만 두고 저장하지 않는다.

## 재현

```bash
# 저장소 루트에서 xazz-server 실행 후
cd visual-ide
node scripts/demo.mjs                                   # 영어·한국어 영상 + 스크린샷
node scripts/demo.mjs --lang ko                         # 한 언어만
node scripts/demo.mjs --tamper ../audit_log/audit.jsonl # 감사 로그 변조 장면 포함(자동 원복)
```

스크립트는 실행 기록과 감사 레코드를 새로 남긴다(append-only). 설치한 정책 팩은 마지막에 제거한다.
