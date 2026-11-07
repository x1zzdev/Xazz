#!/usr/bin/env bash
# demo/preflight.sh — 기능테스트 시연 전 환경을 한 번에 준비한다.
#
#   1) PostgreSQL 기동            (항목 14)
#   2) 감사 로그 초기화            (항목 25 intact:true)
#   3) REST 서버 :8005 기동        (항목 25·26·27 + IDE)
#   4) Visual IDE :5173 기동       (항목 31)
#   5) 차트 HTML 사전 생성          (미리 탭을 열어두기 위함)
#   6) feature_check.sh 실행       (PASS/FAIL 집계)
#
# 서버·IDE는 이 스크립트가 끝나도 계속 살아 있다. 로그는 ~/.xazz-run/ 에 남는다.
#
#   bash demo/preflight.sh          # 준비
#   bash demo/preflight.sh stop     # 서버·IDE·DB 종료(정리)
set -u
cd "$(dirname "$0")/.." || exit 1

RUNDIR="${XAZZ_RUN_DIR:-$HOME/.xazz-run}"
mkdir -p "$RUNDIR"
XAZZ="target/release/xazz"
SERVER_PORT="${XAZZ_SERVER_PORT:-8005}"
IDE_PORT=5173

log()  { printf '\033[36m[xazz]\033[0m %s\n' "$1"; }
ok()   { printf '  \033[32m✓\033[0m %s\n' "$1"; }
warn() { printf '  \033[33m!\033[0m %s\n' "$1"; }
port_open() { ss -ltn 2>/dev/null | grep -q ":$1 "; }
wait_port() { for _ in $(seq 1 "${2:-40}"); do port_open "$1" && return 0; sleep 0.3; done; return 1; }

[ -x "$XAZZ" ] || { echo "바이너리 없음: $XAZZ — 먼저 cargo build --release -p xazz -p xazz-runner -p xazz-exec -p xazz-server"; exit 1; }

if [ "${1:-}" = "stop" ]; then
    pkill -f 'target/release/xazz-server' 2>/dev/null && ok "서버 종료" || warn "서버 미실행"
    pkill -f 'vite --host 127.0.0.1'       2>/dev/null && ok "IDE 종료"  || warn "IDE 미실행"
    bash demo/start_postgres.sh stop 2>/dev/null || true
    exit 0
fi

echo "══════════ Xazz 시연 사전 준비 ══════════"

log "1) PostgreSQL (항목 14)"
if bash demo/start_postgres.sh start >/dev/null 2>&1; then ok "127.0.0.1:5432"; else warn "기동 실패 — 항목 14만 생략"; fi

log "2) 감사 로그 초기화 (항목 25)"
: > audit_log/audit.jsonl && ok "audit_log/audit.jsonl 비움"

log "3) REST 서버 (항목 25·26·27)"
if port_open "$SERVER_PORT"; then
    warn "이미 $SERVER_PORT 사용 중 — 기존 서버 사용"
else
    ( setsid "$XAZZ-server" >"$RUNDIR/server.log" 2>&1 </dev/null & )
    if wait_port "$SERVER_PORT" 40; then ok "http://127.0.0.1:$SERVER_PORT"; else warn "기동 실패: $RUNDIR/server.log"; fi
fi

log "4) Visual IDE (항목 31)"
if port_open "$IDE_PORT"; then
    warn "이미 $IDE_PORT 사용 중 — 기존 IDE 사용"
else
    ( cd visual-ide && setsid env VITE_API_BASE_URL="http://127.0.0.1:$SERVER_PORT" npm run dev >"$RUNDIR/ide.log" 2>&1 </dev/null & )
    if wait_port "$IDE_PORT" 80; then ok "http://127.0.0.1:$IDE_PORT"; else warn "기동 실패: $RUNDIR/ide.log"; fi
fi

log "5) 차트 HTML 사전 생성 (미리 탭 열기용)"
"$XAZZ" run demo/preprocess_chart.xzz >/dev/null 2>&1 && ok "result_chart_chart.html" || warn "preprocess 실패"
cat > "$RUNDIR/piescat.xzz" <<'EOF'
type Air = { observed_at: string, district: string, pm10: Option<float>, pm25: Option<float> }
v raw = load("demo/data/seoul_air_sample.csv") :: Air
v c = raw |> fillNull("pm25", strategy: "mean")
v s = c |> groupBy("district") |> mean("pm25") |> take(5)
v pie = s |> chart { type: pie, label: district, value: pm25, title: "PM25 pie" }
v scatter = c |> take(30) |> chart { type: scatter, x: pm10, y: pm25, title: "pm10 vs pm25" }
EOF
"$XAZZ" run "$RUNDIR/piescat.xzz" >/dev/null 2>&1 && ok "pie_chart.html · scatter_chart.html" || warn "pie/scatter 실패"
"$XAZZ" run examples/deep_learning/air_pipeline.xzz >/dev/null 2>&1 && ok "predicted_chart_chart.html" || warn "predict 실패"

log "6) 기능 시험 항목 자동 점검"
bash demo/feature_check.sh 2>&1 | sed 's/\x1b\[[0-9;]*m//g' | tail -3

echo "═════════════════════════════════════════"
echo "준비 완료. 브라우저 탭 순서:"
echo "  1. http://127.0.0.1:$IDE_PORT"
echo "  2. result_chart_chart.html"
echo "  3. pie_chart.html · scatter_chart.html"
echo "  4. predicted_chart_chart.html"
echo "로그: $RUNDIR/  (server.log, ide.log)"
echo "정리: bash demo/preflight.sh stop"
