#!/usr/bin/env python3
"""demo/make_video.py — Xazz 시연 영상 생성기

실제로 실행한 명령의 **진짜 출력**(demo/_capture/*.txt)과 나레이션 대본
(demo/narration.json)을 합쳐 시연 영상(MP4)과 자막(SRT)을 만든다.

동작 방식
    1. 나레이션 타임라인을 기준으로 전체 영상 길이를 계산한다.
    2. 매 프레임의 "화면 상태"(막 제목 · 입력 중인 명령 · 노출된 출력 줄 · 자막)를
       계산하고, 연속으로 같은 상태는 하나로 합친다 → 렌더 프레임 수를 크게 줄인다.
    3. Chromium(Playwright)으로 각 상태를 스크린샷한다.
    4. ffmpeg concat 으로 프레임별 지속시간을 그대로 살려 MP4 로 굽는다.

    자막은 화면에 새겨 넣고(burn-in), 동일한 타임라인으로 .srt 도 따로 낸다.
    → 나중에 AI 음성을 얹을 때 이 SRT 타임코드에 맞추면 그대로 싱크가 맞는다.

사용법
    python3 demo/make_video.py                 # 전체 생성
    python3 demo/make_video.py --preview       # 첫 프레임 1장만 (레이아웃 확인용)
    python3 demo/make_video.py --fps 12
"""

from __future__ import annotations

import argparse
import html
import json
import pathlib
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
DEMO = ROOT / "demo"
CAPTURE = DEMO / "_capture"
BUILD = DEMO / "_build"
CHROMIUM = "/opt/pw-browsers/chromium"

WIDTH, HEIGHT = 1920, 1080
TERM_ROWS = 19          # 터미널에 한 번에 보이는 최대 줄 수
TYPE_CPS = 26.0         # 명령 타이핑 속도 (chars/sec)
TITLE_HOLD = 1.1        # 막 제목 카드 노출 시간
OUTPUT_REVEAL = 0.055   # 출력 한 줄이 나타나는 간격

# 명령이 없는 장면(오프닝·클로징)에 터미널 대신 띄울 카드
SCENE_POSTER = {
    "open": [
        ("Xazz", "겉은 스크립트, 핵심은 컴파일러"),
        ("전처리", "Polars LazyFrame"),
        ("딥러닝", "Burn 텐서 · 제로카피"),
        ("보안", "Policy-as-Code 가드레일 · 차등 프라이버시"),
    ],
    "close": [
        ("한 장의 .xzz", "전처리 → 학습 → 보안까지"),
        ("정적 검증", "타입 · 널 · 개인정보 노출"),
        ("실행 전 차단", "위반 코드는 엔진에 닿지 않는다"),
        ("감사 증빙", "규제 근거 · SHA-256 감사 로그"),
    ],
}

# 장면별로 어떤 캡처를 몇 줄까지 보여줄지
SCENE_CAPTURE = {
    # (캡처 이름, 보여줄 줄 수, 어느 쪽을 보여줄지)
    #   head — 출력 앞부분이 핵심 (검사 결과 · 차단 사유)
    #   tail — 출력 뒷부분이 핵심 (학습 손실 · 보정된 코드)
    "check": ("check", 19, "head"),
    "emit": ("emit", 19, "head"),
    "preprocess": ("preprocess", 19, "head"),
    "train": ("train", 19, "tail"),
    "guardrail_block": ("policy_block", 19, "head"),
    "guardrail_fix": ("policy_fix", 19, "tail"),
    "dp": ("dp", 19, "head"),
}


# ── 캡처 로딩 ────────────────────────────────────────────────────────────────


def _dp_summary(raw: str) -> str:
    """`[xazz:dp]` 다음 줄의 예산 리포트 JSON을 한 줄로 줄여 보여 준다.

    JSON 원문은 1200픽셀을 넘겨 두 줄로 접히는데, 화면에서 읽히지 않으면
    "예산을 남긴다"는 논지가 전달되지 않는다. 값은 그대로 쓰고 표현만 줄인다.
    """
    try:
        d = json.loads(raw)
    except (ValueError, TypeError):
        return f"[xazz:dp] {raw[:110]}"
    cols = ", ".join(d.get("noised_columns") or [])
    delta = d.get("delta")
    eps = f"ε={d.get('epsilon')}" + (f", δ={delta}" if delta is not None else "")
    return (
        f"[xazz:dp] {d.get('mechanism')} ({eps}) · 컬럼 [{cols}] · "
        f"예산 {d.get('budget_spent'):.2f}/{d.get('budget_total'):.2f} 사용"
    )


def display_filter(lines: list[str]) -> list[str]:
    """화면에 띄울 줄만 남긴다.

    걸러내는 것은 **읽을 수 없거나 데모의 논지와 무관한 줄**뿐이다.
      · `[xazz] …`        — 내부 진행 로그(stderr). 파이프라인 단계 카운터 등.
      · `[xazz WARN] 스키마 필드 … 찾을 수 없음`
                          — 집계 파이프라인에서 스키마 검증이 결과 프레임을
                            대상으로 도는 기존 버그(issue #15). 이번 데모 논지와
                            무관한 잡음이라 화면에서만 감춘다.
      · `[xazz:result] {…}` / `[xazz:train] {…}`
                          — 프런트엔드용 한 줄짜리 대용량 JSON. 사람이 읽을 수 없다.
                            같은 내용이 바로 위 표/학습 로그에 이미 사람이 읽는
                            형태로 나와 있다.

    `[xazz:dp]` 예산 리포트와 `[xazz:policy]` 판정은 **감추지 않는다** —
    데모가 증명하려는 대상이기 때문이다. 다만 두 마커 모두 한 줄이 너무 길어
    핵심 필드만 요약해 보여 준다.
    """
    out: list[str] = []
    want_dp_json = False
    for ln in lines:
        if want_dp_json:
            want_dp_json = False
            out.append(_dp_summary(ln))
            continue
        if ln.startswith("__EXIT__"):
            continue
        if ln.strip() == "[xazz:dp]":
            want_dp_json = True
            continue
        if ln.startswith("[xazz] "):
            continue
        if "찾을 수 없음" in ln:
            continue
        if ln.startswith("[xazz:result] ") or ln.startswith("[xazz:train] "):
            continue
        if ln.startswith("[xazz:policy] "):
            try:
                p = json.loads(ln[len("[xazz:policy] "):])
                verdict = "통과" if p.get("safe_to_execute") else "차단"
                out.append(
                    f"[xazz:policy] {p.get('policy_id')} · {p.get('domain')} · "
                    f"위험도 {p.get('risk_level')} → {verdict}"
                )
            except (ValueError, TypeError):
                out.append(ln[:120])
            continue
        out.append(ln)
    return out


def load_capture(name: str, limit: int, mode: str = "head") -> list[str]:
    """캡처 파일을 읽어 화면에 띄울 줄만 남긴다.

    `limit` 은 터미널이 한 화면에 담는 줄 수(TERM_ROWS) 이하로 잡는다 —
    그래야 스크롤로 머리말이 잘려 나가지 않는다.
    """
    path = CAPTURE / f"{name}.txt"
    if not path.exists():
        return [f"(캡처 없음: {path.name} — demo/capture.sh 를 먼저 실행하세요)"]
    lines = display_filter(
        path.read_text(encoding="utf-8", errors="replace").splitlines()
    )
    while lines and not lines[0].strip():
        lines.pop(0)
    while lines and not lines[-1].strip():
        lines.pop()
    if len(lines) > limit:
        lines = ["…"] + lines[-(limit - 1):] if mode == "tail" else lines[: limit - 1] + ["…"]
    return lines


# ── 타임라인 ─────────────────────────────────────────────────────────────────


def build_timeline(narration: dict, fps: int) -> tuple[list[dict], list[dict]]:
    """(프레임 목록, 자막 목록) 을 만든다."""
    step = 1.0 / fps
    frames: list[dict] = []
    subs: list[dict] = []
    clock = 0.0

    for scene in narration["scenes"]:
        sid = scene["id"]
        act = scene["act"]
        command = scene.get("command")
        duration = sum(ln["sec"] for ln in scene["lines"])

        cap_name, limit, mode = SCENE_CAPTURE.get(sid, (None, 0, "head"))
        out_lines = load_capture(cap_name, limit, mode) if cap_name else []

        # 자막 구간 기록
        cursor = clock
        for ln in scene["lines"]:
            subs.append({"start": cursor, "end": cursor + ln["sec"], "text": ln["t"]})
            cursor += ln["sec"]

        # 장면 내부 시각별 화면 상태
        type_dur = (len(command) / TYPE_CPS) if command else 0.0
        reveal_dur = len(out_lines) * OUTPUT_REVEAL

        t = 0.0
        while t < duration - 1e-9:
            if t < TITLE_HOLD or not command:
                typed, shown = 0, 0
            elif t < TITLE_HOLD + type_dur:
                typed = int((t - TITLE_HOLD) * TYPE_CPS) + 1
                shown = 0
            else:
                typed = len(command)
                after = t - TITLE_HOLD - type_dur
                shown = min(len(out_lines), int(after / OUTPUT_REVEAL) + 1)

            frames.append(
                {
                    "scene": sid,
                    "act": act,
                    "command": command,
                    "typed": typed,
                    "shown": shown,
                    "out": out_lines,
                    "sub": subtitle_at(subs, clock + t),
                    "progress": 0.0,  # 아래에서 채운다
                    "dur": step,
                }
            )
            t += step

        clock += duration
        del reveal_dur

    total = clock
    acc = 0.0
    for f in frames:
        f["progress"] = acc / total if total else 0.0
        acc += f["dur"]

    return merge_frames(frames), subs


def subtitle_at(subs: list[dict], t: float) -> str:
    for s in subs:
        if s["start"] <= t < s["end"]:
            return s["text"]
    return ""


def merge_frames(frames: list[dict]) -> list[dict]:
    """연속으로 화면이 같은 프레임을 하나로 합쳐 렌더 횟수를 줄인다.

    진행 막대는 계속 움직이지만 1픽셀 미만 변화는 눈에 띄지 않으므로
    2% 단위로 양자화해 병합 대상에 포함한다.
    """

    def key(f: dict) -> tuple:
        return (f["scene"], f["typed"], f["shown"], f["sub"], round(f["progress"] * 50))

    merged: list[dict] = []
    for f in frames:
        if merged and key(merged[-1]) == key(f):
            merged[-1]["dur"] += f["dur"]
        else:
            merged.append(dict(f))
    return merged


# ── HTML ─────────────────────────────────────────────────────────────────────

PAGE = """<!doctype html><html><head><meta charset="utf-8"><style>
* { margin:0; padding:0; box-sizing:border-box; }
html,body { width:1920px; height:1080px; overflow:hidden;
  background:#080b12; font-family:'NanumSquareRound','Nanum Gothic',sans-serif; }
#stage { position:relative; width:1920px; height:1080px;
  background:radial-gradient(1200px 700px at 50% -10%,#152034 0%,#080b12 70%); }

#top { position:absolute; top:44px; left:64px; right:64px; display:flex;
  align-items:center; gap:22px; }
#brand { font-size:30px; font-weight:800; letter-spacing:-.5px; color:#e8eefc; }
#brand span { color:#5b9dff; }
#act { font-size:23px; color:#cfe0ff; background:rgba(91,157,255,.13);
  border:1px solid rgba(91,157,255,.32); padding:8px 20px; border-radius:999px; }

#bar { position:absolute; top:108px; left:64px; right:64px; height:3px;
  background:rgba(255,255,255,.08); border-radius:2px; overflow:hidden; }
#fill { height:100%; width:0%; background:linear-gradient(90deg,#5b9dff,#7ee0c0); }

#term { position:absolute; top:150px; left:64px; right:64px; bottom:250px;
  background:#0d1117; border:1px solid rgba(255,255,255,.09); border-radius:16px;
  box-shadow:0 30px 90px rgba(0,0,0,.55); overflow:hidden; }
#tbar { height:44px; background:#161b26; display:flex; align-items:center;
  padding:0 18px; gap:9px; border-bottom:1px solid rgba(255,255,255,.07); }
.dot { width:13px; height:13px; border-radius:50%; }
#tt { margin-left:14px; font-size:16px; color:#7d8899;
  font-family:'NanumGothicCoding',monospace; }
#body { padding:24px 30px; font-family:'DejaVu Sans Mono','NanumGothicCoding',monospace;
  font-size:19px; line-height:1.45; color:#c9d5e6; white-space:pre-wrap;
  word-break:break-all; }
.prompt { color:#7ee0c0; }
.cmd { color:#ffffff; font-weight:700; }
.caret { display:inline-block; width:11px; height:23px; background:#7ee0c0;
  vertical-align:-4px; }
.out { color:#aab8cc; }
.blk { color:#ff8a8a; }
.ok  { color:#7ee0c0; }
.dim { color:#6b7891; }
.hl  { color:#ffd479; }

#poster { position:absolute; inset:0; display:none; flex-direction:column;
  align-items:center; justify-content:center; gap:26px; padding:0 120px; }
#poster.on { display:flex; }
.card { width:100%; max-width:1180px; display:flex; align-items:baseline; gap:28px;
  background:rgba(255,255,255,.035); border:1px solid rgba(255,255,255,.09);
  border-radius:14px; padding:22px 34px; }
.card b { font-size:34px; color:#7ee0c0; min-width:230px; font-weight:800; }
.card i { font-size:30px; color:#cfdcf0; font-style:normal; }
.card:first-child b { color:#5b9dff; font-size:46px; }
.card:first-child i { font-size:34px; color:#e8eefc; }

#subwrap { position:absolute; left:0; right:0; bottom:74px; display:flex;
  justify-content:center; }
#sub { max-width:1660px; font-size:37px; line-height:1.42; font-weight:700;
  color:#f2f6ff; text-align:center; background:rgba(6,10,18,.82);
  border:1px solid rgba(255,255,255,.09); padding:20px 40px; border-radius:16px;
  text-shadow:0 3px 14px rgba(0,0,0,.85); }
#sub:empty { display:none; }
</style></head><body><div id="stage">
<div id="top"><div id="brand">Xa<span>zz</span></div><div id="act"></div></div>
<div id="bar"><div id="fill"></div></div>
<div id="poster"></div>
<div id="term"><div id="tbar">
  <div class="dot" style="background:#ff5f57"></div>
  <div class="dot" style="background:#febc2e"></div>
  <div class="dot" style="background:#28c840"></div>
  <div id="tt">xazz — demo</div></div>
  <div id="body"></div></div>
<div id="subwrap"><div id="sub"></div></div>
</div><script>
function cls(s){
  if(/차단|✖|ERROR|에러|위반/.test(s)) return 'blk';
  if(/✅|통과|완료|OK|성공/.test(s))    return 'ok';
  if(/^\\s*[─═#·ⓘ]|^\\s*\\/\\//.test(s)) return 'dim';
  if(/보정|근거|withDp|epsilon/.test(s)) return 'hl';
  return 'out';
}
function esc(s){return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');}
window.render = function(f){
  document.getElementById('act').textContent = f.act;
  document.getElementById('fill').style.width = (f.progress*100).toFixed(2)+'%';
  document.getElementById('sub').textContent = f.sub || '';
  const term = document.getElementById('term');
  const poster = document.getElementById('poster');
  if(f.poster){
    poster.classList.add('on'); term.style.display='none';
    poster.innerHTML = f.poster.map(function(c){
      return '<div class="card"><b>'+esc(c[0])+'</b><i>'+esc(c[1])+'</i></div>';
    }).join('');
    return;
  }
  poster.classList.remove('on'); term.style.display='block';
  let h = '';
  if(f.command){
    h += '<span class="prompt">$</span> <span class="cmd">'
       + esc(f.command.slice(0, f.typed)) + '</span>';
    if(f.typed < f.command.length) h += '<span class="caret"></span>';
    h += '\\n';
  }
  const vis = f.out.slice(0, f.shown);
  const tail = vis.length > f.rows ? vis.slice(vis.length - f.rows) : vis;
  for(const ln of tail) h += '<span class="'+cls(ln)+'">'+esc(ln)+'</span>\\n';
  document.getElementById('body').innerHTML = h;
};
</script></body></html>"""


# ── SRT ──────────────────────────────────────────────────────────────────────


def ts(sec: float) -> str:
    ms = int(round(sec * 1000))
    h, ms = divmod(ms, 3600000)
    m, ms = divmod(ms, 60000)
    s, ms = divmod(ms, 1000)
    return f"{h:02d}:{m:02d}:{s:02d},{ms:03d}"


def write_srt(subs: list[dict], path: pathlib.Path) -> None:
    out = []
    for i, s in enumerate(subs, 1):
        out.append(f"{i}\n{ts(s['start'])} --> {ts(s['end'])}\n{s['text']}\n")
    path.write_text("\n".join(out), encoding="utf-8")


# ── 렌더링 ───────────────────────────────────────────────────────────────────


def render(frames: list[dict], preview: bool) -> None:
    from playwright.sync_api import sync_playwright

    if BUILD.exists():
        shutil.rmtree(BUILD)
    BUILD.mkdir(parents=True)
    page_path = BUILD / "page.html"
    page_path.write_text(PAGE, encoding="utf-8")

    with sync_playwright() as p:
        browser = p.chromium.launch(executable_path=CHROMIUM)
        page = browser.new_page(viewport={"width": WIDTH, "height": HEIGHT})
        page.goto(page_path.as_uri())

        targets = frames[:1] if preview else frames
        for i, f in enumerate(targets):
            payload = {
                "act": f["act"],
                "command": f["command"],
                "typed": f["typed"],
                "shown": f["shown"],
                "out": f["out"],
                "sub": f["sub"],
                "progress": f["progress"],
                "rows": TERM_ROWS,
                "poster": SCENE_POSTER.get(f["scene"]),
            }
            page.evaluate("f => window.render(f)", payload)
            page.screenshot(path=str(BUILD / f"f{i:05d}.png"))
            if not preview and i % 50 == 0:
                print(f"  프레임 {i}/{len(targets)}", file=sys.stderr)
        browser.close()


def encode(frames: list[dict], out_path: pathlib.Path) -> None:
    lines = []
    for i, f in enumerate(frames):
        lines.append(f"file 'f{i:05d}.png'")
        lines.append(f"duration {f['dur']:.4f}")
    lines.append(f"file 'f{len(frames) - 1:05d}.png'")  # 마지막 프레임 고정용
    (BUILD / "concat.txt").write_text("\n".join(lines), encoding="utf-8")

    subprocess.run(
        [
            "ffmpeg", "-y", "-loglevel", "error",
            "-f", "concat", "-safe", "0", "-i", str(BUILD / "concat.txt"),
            "-vsync", "vfr", "-pix_fmt", "yuv420p",
            "-c:v", "libx264", "-preset", "medium", "-crf", "20",
            "-movflags", "+faststart",
            str(out_path),
        ],
        check=True,
    )


def main() -> int:
    ap = argparse.ArgumentParser(description="Xazz 시연 영상 생성")
    ap.add_argument("--fps", type=int, default=12)
    ap.add_argument("--preview", action="store_true", help="첫 프레임만 렌더")
    ap.add_argument("--out", type=pathlib.Path, default=DEMO / "xazz_demo.mp4")
    args = ap.parse_args()

    narration = json.loads((DEMO / "narration.json").read_text(encoding="utf-8"))
    frames, subs = build_timeline(narration, args.fps)
    total = sum(f["dur"] for f in frames)

    print(f"길이 {total:.1f}초 ({int(total // 60)}분 {total % 60:.0f}초) · "
          f"자막 {len(subs)}컷 · 렌더 프레임 {len(frames)}장")

    render(frames, args.preview)
    if args.preview:
        print(f"미리보기: {BUILD / 'f00000.png'}")
        return 0

    encode(frames, args.out)
    srt = args.out.with_suffix(".srt")
    write_srt(subs, srt)
    print(f"영상: {args.out}")
    print(f"자막: {srt}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
