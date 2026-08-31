#!/usr/bin/env bash
# demo/capture.sh — 시연 영상에 쓸 "진짜 명령 출력"을 캡처한다.
#
# 영상은 이 캡처 파일만 읽어 그리므로, 화면에 나오는 모든 글자는
# 실제로 실행한 결과다. 손으로 쓴 가짜 출력은 들어가지 않는다.
#
#   bash demo/capture.sh          # 릴리즈 바이너리 사용 (기본)
#   XAZZ_BIN=target/debug/xazz bash demo/capture.sh
set -euo pipefail
cd "$(dirname "$0")/.."

XAZZ="${XAZZ_BIN:-target/release/xazz}"
OUT=demo/_capture
mkdir -p "$OUT"

[ -x "$XAZZ" ] || { echo "빌드가 필요합니다: cargo build --release -p xazz -p xazz-runner -p xazz-exec"; exit 1; }

# 보안 데모용 합성 데이터 (실제 개인정보 아님)
python3 examples/security/generate_patients.py --rows 500 >/dev/null

cap() { local name=$1; shift; { "$@" || true; } > "$OUT/$name.txt" 2>&1; printf '  %-16s %s줄\n' "$name" "$(wc -l < "$OUT/$name.txt")"; }

echo "캡처 중…"
cap check        "$XAZZ" check demo/deep_learning.xzz

# 오타 감지 시연 — did-you-mean 제안이 나오는 check 출력 (README demo_check.png 용)
cat > "$OUT/typo_scene.xzz" <<'EOF'
type AirData = {
    temperature_c: float,
    pm25:          Option<float>,
}

v d = load("air_data.csv") :: AirData
    |> filter(temperture_c > 20)
EOF
cap check_typo    "$XAZZ" check "$OUT/typo_scene.xzz"

cap emit         "$XAZZ" emit rust demo/deep_learning.xzz
cap preprocess   "$XAZZ" run demo/preprocess_chart.xzz
cap train        "$XAZZ" run demo/deep_learning.xzz
cap dp           "$XAZZ" run demo/dp.xzz
cap policy_block "$XAZZ" run examples/security/patient_unsafe.xzz
cap policy_fix   "$XAZZ" policy examples/security/patient_unsafe.xzz --fix
echo "완료 → $OUT"
