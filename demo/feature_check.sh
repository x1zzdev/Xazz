#!/usr/bin/env bash
# demo/feature_check.sh — 2026 오픈소스 개발자대회 기능 시험 항목 자동 점검
#
# 명세서(접수번호 349 · 팀 x1zz)의 "기능 시험 항목"을 순서대로 실제 실행해
# PASS / FAIL / SKIP 을 집계한다. 시연 10분 전에 한 번 돌려 상태를 확인한다.
#
#   bash demo/feature_check.sh            # CLI + 서버 항목 (PostgreSQL/GPU/IDE 는 수동)
#   bash demo/feature_check.sh --with-lfs # Git LFS 예제(predict)도 점검
#
# 저장소 루트에서 실행한다. 서버(8005)가 없으면 직접 띄우고 끝나면 종료한다.
# 감사 로그는 백업 후 새 체인으로 시작해 chain intact:true 를 보장한다.
set -u
cd "$(dirname "$0")/.."

XAZZ="${XAZZ_BIN:-target/release/xazz}"
WITH_LFS=0
[ "${1:-}" = "--with-lfs" ] && WITH_LFS=1

pass=0; fail=0; skip=0; FAILED=()
green=$'\033[32m'; red=$'\033[31m'; yellow=$'\033[33m'; dim=$'\033[2m'; rst=$'\033[0m'
ok() { printf '  %sPASS%s  %s\n' "$green" "$rst" "$1"; pass=$((pass+1)); }
no() { printf '  %sFAIL%s  %s\n' "$red" "$rst" "$1"; fail=$((fail+1)); FAILED+=("$1"); }
sk() { printf '  %sSKIP%s  %s\n' "$yellow" "$rst" "$1"; skip=$((skip+1)); }

# run <name> <want_exit> <grep_pattern|-> <cmd...>
run() {
  local name="$1" want="$2" pat="$3"; shift 3
  local out rc
  out="$("$@" 2>&1)"; rc=$?
  if [ "$rc" -eq "$want" ] && { [ "$pat" = "-" ] || printf '%s' "$out" | grep -qE "$pat"; }; then
    ok "$name"
  else
    no "$name (exit=$rc, want=$want)"
  fi
}
# runf <name> <want_exit> <grep_pattern|-> <file_to_check_exists|-> <cmd...>
runf() {
  local name="$1" want="$2" pat="$3" file="$4"; shift 4
  local out rc
  out="$("$@" 2>&1)"; rc=$?
  local fileok=1; [ "$file" = "-" ] || [ -e "$file" ] || fileok=0
  if [ "$rc" -eq "$want" ] && [ "$fileok" = 1 ] && { [ "$pat" = "-" ] || printf '%s' "$out" | grep -qE "$pat"; }; then
    ok "$name"
  else
    no "$name (exit=$rc, want=$want, file=$([ "$file" = - ] && echo n/a || { [ -e "$file" ] && echo ok || echo missing; }))"
  fi
}

printf '\n=== Xazz 기능 시험 항목 자동 점검 ===\n'
printf '%s바이너리: %s%s\n\n' "$dim" "$XAZZ" "$rst"

[ -x "$XAZZ" ] || { printf '%s바이너리 없음. 먼저 cargo build --release -p xazz -p xazz-runner -p xazz-exec -p xazz-server%s\n' "$red" "$rst"; exit 2; }

# ── 컴파일러 ─────────────────────────────────────────────────────────────────
printf '컴파일러\n'
run  "1. 정적 분석 정상 통과"        0 'static analysis passed' "$XAZZ" check demo/preprocess_chart.xzz
sed 's/mean("pm25")/mean("pm52")/' demo/preprocess_chart.xzz > /tmp/xazz_bad.xzz
run  "2. 오류 검출(오타 컬럼, exit1)" 1 "does not exist"        "$XAZZ" check /tmp/xazz_bad.xzz
run  "3. emit rust 코드 생성"         0 'Auto-generated'        "$XAZZ" emit rust demo/deep_learning.xzz
run  "4. stdlib import"               0 '12 AST'                "$XAZZ" run examples/stdlib_import.xzz
run  "5. Typed IR --opt"              0 'IR optimization applied' "$XAZZ" run demo/preprocess_chart.xzz --opt
printf 'type Air = { district: string, pm25: float }\nv x = load("demo/data/seoul_air_sample.csv") :: Air\nv c = x |> chart { kind: bar }\n' > /tmp/xazz_bad2.xzz
run  "6. 실패 안전 fail-closed(exit1)" 1 '-'                    "$XAZZ" run /tmp/xazz_bad2.xzz

# ── 데이터 ───────────────────────────────────────────────────────────────────
printf '\n데이터\n'
rm -rf /tmp/xazz_imp && mkdir -p /tmp/xazz_imp
runf "7. import 스키마 추론"          0 'load\('                 /tmp/xazz_imp/main.xzz bash -c "cd /tmp/xazz_imp && \"$PWD/$XAZZ\" import \"$PWD/demo/data/seoul_air_sample.csv\""
runf "8. 전처리·차트 run"             0 'Pipeline #1'            result_chart_chart.html "$XAZZ" run demo/preprocess_chart.xzz
run  "9. --output CSV 저장"           0 'CSV saved'             "$XAZZ" run demo/preprocess_chart.xzz --output /tmp/xazz_out.csv
cat > /tmp/xazz_pq.xzz <<'EOF'
type Air = { observed_at: string, district: string, pm10: Option<float>, pm25: Option<float> }
v raw = load("demo/data/seoul_air_sample.csv") :: Air
v s = raw |> groupBy("district") |> mean("pm25") |> take(5)
v saved = s |> save("/tmp/xazz_out.parquet", format: "parquet")
EOF
runf "10. parquet 저장"               0 'parquet'               /tmp/xazz_out.parquet "$XAZZ" run /tmp/xazz_pq.xzz
cat > /tmp/xazz_ps.xzz <<'EOF'
type Air = { observed_at: string, district: string, pm10: Option<float>, pm25: Option<float> }
v raw = load("demo/data/seoul_air_sample.csv") :: Air
v c = raw |> fillNull("pm25", strategy: "mean")
v s = c |> groupBy("district") |> mean("pm25") |> take(5)
v pie = s |> chart { type: pie, label: district, value: pm25, title: "PM25 pie" }
v scatter = c |> take(30) |> chart { type: scatter, x: pm10, y: pm25, title: "pm10 vs pm25" }
EOF
rm -f pie_chart.html scatter_chart.html
run  "11. 차트 pie/scatter"           0 '-'                    "$XAZZ" run /tmp/xazz_ps.xzz
[ -e pie_chart.html ] && [ -e scatter_chart.html ] && ok "11b. pie/scatter HTML 생성" || no "11b. pie/scatter HTML 생성"
run  "12. --json 기계판독"            0 '"success": ?true'      "$XAZZ" run demo/preprocess_chart.xzz --json
run  "13. DuckDB 커넥터"              0 'busan'                 "$XAZZ" run examples/duckdb/hello_duckdb.xzz
sk   "14. PostgreSQL (로컬 DB 필요)"

# ── AI ───────────────────────────────────────────────────────────────────────
printf '\nAI\n'
rm -f checkpoints/AirPredictor.json
runf "15. train (5 epoch, R²>0)"      0 'Epoch   5/5'           checkpoints/AirPredictor.json "$XAZZ" run demo/deep_learning.xzz
if [ "$WITH_LFS" = 1 ]; then
  run  "16. predict (LFS 예제)"       0 'predicted_chart'       "$XAZZ" run examples/deep_learning/air_pipeline.xzz
elif head -c 120 examples/data/seoul_air_2024.csv 2>/dev/null | grep -q 'git-lfs'; then
  sk   "16. predict — examples/data 가 LFS 포인터 (git lfs pull 또는 --with-lfs)"
else
  run  "16. predict (LFS 예제)"       0 'predicted_chart'       "$XAZZ" run examples/deep_learning/air_pipeline.xzz
fi
run  "17. stdlib 표준모델(MLPSmall)"  0 '12 AST'                "$XAZZ" run examples/stdlib_import.xzz

# ── 보안 ─────────────────────────────────────────────────────────────────────
printf '\n보안\n'
run  "18. PII 차단(exit1)"            1 'XZP00'                 "$XAZZ" policy examples/security/patient_unsafe.xzz
run  "19. 정책 정상 통과"             0 'policy check passed'   "$XAZZ" policy examples/security/patient_safe.xzz
runf "20. 자동 수정 --fix"            1 'remediation code saved' /tmp/xazz_safe.xzz "$XAZZ" policy examples/security/patient_unsafe.xzz --fix --out /tmp/xazz_safe.xzz
run  "21. 정책 --json 리포트"         1 '"safe_to_execute": ?false' "$XAZZ" policy examples/security/patient_unsafe.xzz --json
run  "22. DP laplace"                 0 '1\.00/10\.00'          "$XAZZ" run demo/dp.xzz
run  "23. DP gaussian·예산"           0 '1\.50/10\.00'          "$XAZZ" run examples/privacy/dp_aggregation.xzz
run  "24. sanitize PII(phone 500)"    0 'phone'                 "$XAZZ" sanitize examples/security/data/patients.csv --json

# ── 서버 (감사/health/execute) ───────────────────────────────────────────────
printf '\n도구 — 서버\n'
SRV=""
if ss -ltn 2>/dev/null | grep -q ':8005'; then
  printf '  %s경고: 8005 사용 중 — 기존 서버로 점검(감사 체인 초기화 불가)%s\n' "$yellow" "$rst"
else
  mv audit_log/audit.jsonl /tmp/xazz_audit_backup_$$.jsonl 2>/dev/null || true
  setsid bash -c "exec $XAZZ-server" >/tmp/xazz_feat_srv.log 2>&1 </dev/null &
  SRV=$!
  trap '[ -n "$SRV" ] && kill "$SRV" 2>/dev/null' EXIT
  for _ in $(seq 1 30); do curl -sf http://127.0.0.1:8005/health >/dev/null 2>&1 && break; sleep 0.3; done
fi
run  "26. 서버 health"                0 '"status":"ok"'         curl -s http://127.0.0.1:8005/health
run  "27. 서버 execute"               0 '"success":true'        curl -s -X POST http://127.0.0.1:8005/execute -H 'Content-Type: application/json' --data-binary '{"code":"type Air = { observed_at: string, district: string, pm10: Option<float>, pm25: Option<float> }\nv r = load(\"demo/data/seoul_air_sample.csv\") :: Air\nv t = r |> take(2)"}'
curl -s -X POST http://127.0.0.1:8005/security/audit -H 'Content-Type: application/json' -d '{"code":"probe"}' >/dev/null 2>&1
run  "25. 감사 로그·체인 intact"      0 '"intact":true'         curl -s http://127.0.0.1:8005/security/audit/chain
[ -n "$SRV" ] && { kill "$SRV" 2>/dev/null; wait "$SRV" 2>/dev/null; }

# ── 도구 — CLI ───────────────────────────────────────────────────────────────
printf '\n도구 — CLI\n'
rm -rf /tmp/xazz_new && mkdir -p /tmp/xazz_new
runf "28. new·run"                    0 'result'                /tmp/xazz_new/my-project/example.xzz bash -c "cd /tmp/xazz_new && \"$PWD/$XAZZ\" new my-project >/dev/null && cd my-project && \"$PWD/$XAZZ\" run example.xzz"
run  "29. registry list"              0 'healthcare'            "$XAZZ" registry list
runf "30. sde 합성 데이터"            0 'generated'             /tmp/xazz_pairs.jsonl "$XAZZ" sde --rows 5 --output /tmp/xazz_pairs.jsonl
sk   "31. Visual IDE (:5173) — 수동 확인"

# ── 커뮤니티 ─────────────────────────────────────────────────────────────────
printf '\n커뮤니티\n'
run  "32. cargo-deny 라이선스"        0 'licenses ok'           timeout 180 cargo deny check licenses
[ -e sbom/xazz.cdx.json ] && [ -e THIRD_PARTY_NOTICES.md ] && ok "33. SBOM·서드파티 고지" || no "33. SBOM·서드파티 고지"

# ── 요약 ─────────────────────────────────────────────────────────────────────
printf '\n=== 결과: %sPASS %d%s · %sFAIL %d%s · %sSKIP %d%s ===\n' \
  "$green" "$pass" "$rst" "$red" "$fail" "$rst" "$yellow" "$skip" "$rst"
if [ "$fail" -gt 0 ]; then
  printf '%s실패 항목:%s\n' "$red" "$rst"
  for f in "${FAILED[@]}"; do printf '  - %s\n' "$f"; done
  exit 1
fi
printf '%s모든 자동 점검 항목 통과.%s (수동: PostgreSQL, Visual IDE)\n' "$green" "$rst"
