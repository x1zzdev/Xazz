#!/usr/bin/env python3
"""demo/make_voice.py — 자막(SRT) 타임코드에 정확히 맞춘 한국어 나레이션 음성을 만든다.

영상에 새겨진 자막과 음성이 **한 컷도 어긋나지 않게** 하는 것이 목적이다.
그래서 나레이션 전체를 한 번에 읽지 않고, 자막 컷 하나하나를 따로 합성한 뒤
각 컷의 시작 시각(SRT `-->` 왼쪽 값)에 그대로 얹는다.

    pip install edge-tts
    python3 demo/make_voice.py                 # → demo/narration.wav
    python3 demo/make_voice.py --mux           # → demo/xazz_demo_voiced.mp4 까지

컷 하나가 배정된 시간보다 길게 읽히면 그 컷만 살짝 빠르게 조정한다
(기본 1.25배까지. 그 이상 필요하면 경고를 띄우고 대본을 줄이라고 알려 준다).

이 저장소가 도는 환경에서는 TTS 엔드포인트가 조직 정책으로 막혀 있어
음성 합성만 로컬에서 돌려야 한다. 나머지(영상·자막)는 전부 재현 가능하다.
"""

from __future__ import annotations

import argparse
import asyncio
import pathlib
import re
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
DEMO = ROOT / "demo"
SRT = DEMO / "xazz_demo.srt"
VIDEO = DEMO / "xazz_demo.mp4"
WORK = DEMO / "_voice"

# ko-KR 뉴럴 보이스. 남성으로 바꾸려면 ko-KR-InJoonNeural.
DEFAULT_VOICE = "ko-KR-SunHiNeural"
DEFAULT_RATE = "+0%"
MAX_SPEEDUP = 1.25  # 컷이 넘칠 때 허용하는 최대 배속
SR = 48000

TS = re.compile(r"(\d\d):(\d\d):(\d\d),(\d\d\d)")


def parse_srt(path: pathlib.Path) -> list[dict]:
    """SRT → [{start, end, text}] (초 단위)."""

    def secs(m: re.Match) -> float:
        h, mnt, s, ms = (int(g) for g in m.groups())
        return h * 3600 + mnt * 60 + s + ms / 1000

    cues, block = [], []
    for line in path.read_text(encoding="utf-8").splitlines() + [""]:
        if line.strip():
            block.append(line)
            continue
        if len(block) >= 3:
            times = list(TS.finditer(block[1]))
            cues.append(
                {
                    "start": secs(times[0]),
                    "end": secs(times[1]),
                    # 자막은 화면 폭에 맞춰 줄바꿈돼 있다 — 읽을 때는 한 문장으로.
                    "text": " ".join(x.strip() for x in block[2:]),
                }
            )
        block = []
    return cues


async def synth(cues: list[dict], voice: str, rate: str) -> None:
    import edge_tts

    for i, c in enumerate(cues):
        dst = WORK / f"cue{i:03d}.mp3"
        await edge_tts.Communicate(c["text"], voice, rate=rate).save(str(dst))
        print(f"  {i + 1:2d}/{len(cues)}  {c['text'][:38]}…", file=sys.stderr)


def dur(path: pathlib.Path) -> float:
    out = subprocess.run(
        ["ffprobe", "-v", "error", "-show_entries", "format=duration",
         "-of", "csv=p=0", str(path)],
        capture_output=True, text=True, check=True,
    )
    return float(out.stdout.strip())


def fit(src: pathlib.Path, dst: pathlib.Path, slot: float) -> str | None:
    """컷 음성을 배정된 시간 안에 맞춘다. 경고가 필요하면 문자열로 돌려준다."""
    d = dur(src)
    warn = None
    tempo = 1.0
    if d > slot:
        tempo = d / slot
        if tempo > MAX_SPEEDUP:
            warn = f"{d:.1f}초 → {slot:.1f}초 칸 (×{tempo:.2f} 필요)"
            tempo = MAX_SPEEDUP
    af = [f"atempo={tempo:.4f}"] if tempo > 1.001 else []
    af.append(f"apad=whole_dur={slot}")  # 남는 구간은 무음으로 채운다
    subprocess.run(
        ["ffmpeg", "-y", "-loglevel", "error", "-i", str(src),
         "-af", ",".join(af), "-t", f"{slot}", "-ar", str(SR), "-ac", "1", str(dst)],
        check=True,
    )
    return warn


def silence(dst: pathlib.Path, sec: float) -> None:
    subprocess.run(
        ["ffmpeg", "-y", "-loglevel", "error", "-f", "lavfi",
         "-i", f"anullsrc=r={SR}:cl=mono", "-t", f"{sec}", str(dst)],
        check=True,
    )


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--voice", default=DEFAULT_VOICE)
    ap.add_argument("--rate", default=DEFAULT_RATE, help="예: -5%% (조금 천천히)")
    ap.add_argument("--mux", action="store_true", help="영상에 바로 입힌다")
    ap.add_argument("--out", default=str(DEMO / "narration.wav"))
    args = ap.parse_args()

    if not SRT.exists():
        print(f"자막이 없습니다. 먼저: python3 demo/make_video.py", file=sys.stderr)
        return 1
    try:
        import edge_tts  # noqa: F401
    except ImportError:
        print("edge-tts 가 필요합니다:  pip install edge-tts", file=sys.stderr)
        return 1

    cues = parse_srt(SRT)
    total = dur(VIDEO) if VIDEO.exists() else cues[-1]["end"]
    print(f"자막 {len(cues)}컷 · 영상 {total:.1f}초 · 보이스 {args.voice}", file=sys.stderr)

    if WORK.exists():
        shutil.rmtree(WORK)
    WORK.mkdir(parents=True)

    asyncio.run(synth(cues, args.voice, args.rate))

    # 컷을 타임코드 순서대로 늘어놓고, 사이의 빈 구간은 무음으로 메운다.
    parts: list[pathlib.Path] = []
    warns: list[str] = []
    cursor = 0.0
    for i, c in enumerate(cues):
        if c["start"] - cursor > 0.02:
            gap = WORK / f"gap{i:03d}.wav"
            silence(gap, c["start"] - cursor)
            parts.append(gap)
        slot = max(c["end"] - c["start"], 0.3)
        piece = WORK / f"fit{i:03d}.wav"
        w = fit(WORK / f"cue{i:03d}.mp3", piece, slot)
        if w:
            warns.append(f"  컷 {i + 1}: {w}\n    “{c['text']}”")
        parts.append(piece)
        cursor = c["start"] + slot
    if total - cursor > 0.02:
        tail = WORK / "tail.wav"
        silence(tail, total - cursor)
        parts.append(tail)

    listing = WORK / "concat.txt"
    listing.write_text(
        "".join(f"file '{p.resolve()}'\n" for p in parts), encoding="utf-8"
    )
    out = pathlib.Path(args.out)
    subprocess.run(
        ["ffmpeg", "-y", "-loglevel", "error", "-f", "concat", "-safe", "0",
         "-i", str(listing), "-ar", str(SR), "-ac", "1", str(out)],
        check=True,
    )
    print(f"음성 완성: {out}  ({dur(out):.1f}초)", file=sys.stderr)

    if warns:
        print(
            "\n⚠️  아래 컷은 배정 시간보다 대본이 깁니다 "
            f"(×{MAX_SPEEDUP} 까지만 조였습니다).\n"
            "   narration.json 에서 문장을 줄이거나 sec 을 늘린 뒤 "
            "make_video.py 를 다시 돌리세요.",
            file=sys.stderr,
        )
        print("\n".join(warns), file=sys.stderr)

    if args.mux:
        voiced = DEMO / "xazz_demo_voiced.mp4"
        subprocess.run(
            ["ffmpeg", "-y", "-loglevel", "error", "-i", str(VIDEO), "-i", str(out),
             "-c:v", "copy", "-c:a", "aac", "-b:a", "192k", "-shortest",
             "-movflags", "+faststart", str(voiced)],
            check=True,
        )
        print(f"영상 완성: {voiced}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
