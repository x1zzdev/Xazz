#!/usr/bin/env bash
# demo/add_voice.sh — 준비한 나레이션 오디오를 시연 영상에 입힌다.
#
#   bash demo/add_voice.sh narration.mp3
#   bash demo/add_voice.sh narration.mp3 demo/xazz_demo_voiced.mp4
#
# 영상 길이(약 2분 5초)와 오디오 길이가 다르면 경고만 하고 그대로 합친다.
# 영상 쪽 자막은 이미 화면에 새겨져 있으므로 싱크는 SRT 타임코드를 따르면 맞는다.
set -euo pipefail
cd "$(dirname "$0")/.."

AUDIO="${1:?사용법: bash demo/add_voice.sh <오디오파일> [출력.mp4]}"
VIDEO=demo/xazz_demo.mp4
OUT="${2:-demo/xazz_demo_voiced.mp4}"

[ -f "$VIDEO" ] || { echo "먼저 영상을 만드세요: python3 demo/make_video.py"; exit 1; }

vdur=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$VIDEO")
adur=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$AUDIO")
printf '영상 %.1f초 / 오디오 %.1f초\n' "$vdur" "$adur"
awk -v v="$vdur" -v a="$adur" 'BEGIN{ if ((v-a)^2 > 9) print "⚠️  길이 차이가 3초를 넘습니다 — 나레이션 속도나 narration.json 의 sec 값을 조정하세요." }'

ffmpeg -y -loglevel error -i "$VIDEO" -i "$AUDIO" \
  -c:v copy -c:a aac -b:a 192k -shortest -movflags +faststart "$OUT"
echo "완성: $OUT"
