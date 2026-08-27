#!/usr/bin/env python3
"""experiments/slm_guardrail/evaluate.py — sLM 보정 품질 평가 (issue #2)

파인튜닝된 모델이 낸 보정 코드를 **가드레일 엔진으로 다시 채점한다.**
사람의 눈이 아니라 정책이 정답지다.

측정 지표
    정책 준수율 (policy_pass_rate)
        보정 코드가 정책을 통과한 비율. sLM 의 1차 목표.
    구문 유효율 (parse_rate)
        보정 코드가 파싱되는 비율. 통과율의 상한이다.
    과잉 수정률 (over_edit_rate)
        위반과 무관한 구문(type 선언 수, 파이프라인 수)까지 바꿔버린 비율.
        "다 지우면 안전하다"는 퇴화 해법을 잡아내기 위한 지표다.
    의도 보존율 (intent_retention_rate)
        원본의 load 경로와 집계 연산이 보정 후에도 남아 있는 비율.
    평균 지연 (mean_latency_ms)

사용법:
    # 결정적 보정 기준선 (모델 없이도 실행된다)
    python3 experiments/slm_guardrail/evaluate.py --mode deterministic

    # Ollama 로 서빙 중인 파인튜닝 모델
    ollama serve &
    python3 experiments/slm_guardrail/evaluate.py --mode slm --model xazz-guardrail
"""

from __future__ import annotations

import argparse
import json
import pathlib
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

EVAL_SET = pathlib.Path("experiments/slm_guardrail/data/seed_pairs.jsonl")


def load_eval_set(path: pathlib.Path, limit: int | None) -> list[dict]:
    if not path.exists():
        raise SystemExit(
            f"평가셋이 없습니다: {path}\n"
            f"먼저 생성하세요: python3 experiments/slm_guardrail/build_dataset.py"
        )
    records = [
        json.loads(line)
        for line in path.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    return records[:limit] if limit else records


def original_code(prompt: str) -> str:
    """프롬프트에서 원본 코드 블록만 되꺼낸다."""
    start = prompt.find("=== 원본 코드 ===\n")
    end = prompt.find("=== 보정된 코드 ===")
    if start == -1 or end == -1:
        return ""
    return prompt[start + len("=== 원본 코드 ===\n") : end].strip()


# ── 보정 전략 ────────────────────────────────────────────────────────────────


def remediate_deterministic(xazz: str, code: str, policy: str | None) -> tuple[str, float]:
    """`xazz policy --fix --json` 의 결정적 보정 — 기준선(baseline)."""
    started = time.perf_counter()
    result = run_guardrail(xazz, code, policy, fix=True)
    elapsed = (time.perf_counter() - started) * 1000
    if not result:
        return "", elapsed
    return (result.get("remediation") or {}).get("code", ""), elapsed


def remediate_slm(endpoint: str, model: str, prompt: str, timeout: float) -> tuple[str, float]:
    """Ollama `/api/generate` 호출 — xazz-server/src/slm.rs 와 동일한 형식."""
    payload = json.dumps(
        {
            "model": model,
            "prompt": prompt,
            "stream": False,
            "options": {"temperature": 0.1, "top_p": 0.9, "num_predict": 768},
        }
    ).encode("utf-8")

    request = urllib.request.Request(
        f"{endpoint.rstrip('/')}/api/generate",
        data=payload,
        headers={"content-type": "application/json"},
    )
    started = time.perf_counter()
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            body = json.loads(response.read().decode("utf-8"))
    except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as e:
        sys.stderr.write(f"[warn] sLM 호출 실패: {e}\n")
        return "", (time.perf_counter() - started) * 1000
    elapsed = (time.perf_counter() - started) * 1000
    return extract_code(body.get("response", "")), elapsed


def extract_code(text: str) -> str:
    """모델 출력에서 코드펜스를 벗겨낸다 (slm.rs::extract_code 와 동일 규칙)."""
    trimmed = text.strip()
    if "```" in trimmed:
        after = trimmed.split("```", 1)[1]
        body = after.split("\n", 1)[1] if "\n" in after else after
        return body.split("```", 1)[0].strip()
    return trimmed


# ── 채점 ─────────────────────────────────────────────────────────────────────


def run_guardrail(xazz: str, code: str, policy: str | None, fix: bool = False) -> dict | None:
    import os

    env = dict(os.environ, XAZZ_POLICY_PATH=policy) if policy else None
    args = [xazz, "policy", "<file>", "--json"] + (["--fix"] if fix else [])

    with tempfile.NamedTemporaryFile("w", suffix=".xzz", delete=False, encoding="utf-8") as fp:
        fp.write(code)
        path = fp.name
    args[2] = path
    try:
        completed = subprocess.run(args, capture_output=True, text=True, env=env, timeout=60)
        if not completed.stdout.strip():
            return None
        return json.loads(completed.stdout)
    except (subprocess.TimeoutExpired, json.JSONDecodeError):
        return None
    finally:
        pathlib.Path(path).unlink(missing_ok=True)


AGGREGATIONS = ("groupBy(", "count(", "mean(", "sum(", "min(", "max(", "median(")


def score(original: str, fixed: str, report: dict | None) -> dict:
    """보정 결과 한 건을 채점한다."""
    if not fixed.strip():
        return {"parsed": False, "passed": False, "over_edited": True, "intent_kept": False}

    parsed = report is not None and not (report.get("policy") or {}).get("parse_error")
    passed = bool(report and (report.get("policy") or {}).get("safe_to_execute"))

    # 과잉 수정: 원본의 파이프라인(v 선언) 개수가 줄었는가.
    original_pipelines = original.count("\nv ") + original.startswith("v ")
    fixed_pipelines = fixed.count("\nv ") + fixed.startswith("v ")
    over_edited = fixed_pipelines < original_pipelines

    # 의도 보존: load 경로가 유지되고, 원본에 집계가 있었다면 집계도 남아 있는가.
    load_kept = all(
        segment in fixed
        for segment in {
            line.strip()
            for line in original.splitlines()
            if "load(" in line
        }
    )
    had_agg = any(a in original for a in AGGREGATIONS)
    agg_kept = (not had_agg) or any(a in fixed for a in AGGREGATIONS)
    intent_kept = load_kept and agg_kept

    return {
        "parsed": parsed,
        "passed": passed,
        "over_edited": over_edited,
        "intent_kept": intent_kept,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Xazz 보안 보정 sLM 평가")
    parser.add_argument("--mode", choices=["deterministic", "slm"], default="deterministic")
    parser.add_argument("--xazz", default="target/release/xazz")
    parser.add_argument("--policy", default=None)
    parser.add_argument("--data", type=pathlib.Path, default=EVAL_SET)
    parser.add_argument("--limit", type=int, default=None)
    parser.add_argument("--endpoint", default="http://127.0.0.1:11434")
    parser.add_argument("--model", default="xazz-guardrail")
    parser.add_argument("--timeout", type=float, default=60.0)
    parser.add_argument("--report", type=pathlib.Path, default=None, help="결과 JSON 저장 경로")
    args = parser.parse_args()

    if not pathlib.Path(args.xazz).exists():
        parser.error(f"xazz 실행 파일이 없습니다: {args.xazz}")

    records = load_eval_set(args.data, args.limit)
    results: list[dict] = []
    latencies: list[float] = []

    for i, record in enumerate(records, 1):
        original = original_code(record["prompt"])
        if not original:
            continue

        if args.mode == "deterministic":
            fixed, elapsed = remediate_deterministic(args.xazz, original, args.policy)
        else:
            fixed, elapsed = remediate_slm(
                args.endpoint, args.model, record["prompt"], args.timeout
            )

        report = run_guardrail(args.xazz, fixed, args.policy) if fixed.strip() else None
        entry = score(original, fixed, report)
        entry["rule_ids"] = record.get("rule_ids", [])
        entry["latency_ms"] = round(elapsed, 1)
        results.append(entry)
        latencies.append(elapsed)

        print(
            f"[{i}/{len(records)}] {'PASS' if entry['passed'] else 'FAIL'} "
            f"({', '.join(entry['rule_ids'])}) {elapsed:.0f}ms",
            file=sys.stderr,
        )

    if not results:
        print("평가할 항목이 없습니다.", file=sys.stderr)
        return 1

    total = len(results)
    summary = {
        "mode": args.mode,
        "model": args.model if args.mode == "slm" else "deterministic-engine",
        "samples": total,
        "parse_rate": round(sum(r["parsed"] for r in results) / total, 4),
        "policy_pass_rate": round(sum(r["passed"] for r in results) / total, 4),
        "over_edit_rate": round(sum(r["over_edited"] for r in results) / total, 4),
        "intent_retention_rate": round(sum(r["intent_kept"] for r in results) / total, 4),
        "mean_latency_ms": round(sum(latencies) / total, 1),
    }

    print()
    print("── 평가 결과 ──────────────────────────────")
    for key, value in summary.items():
        print(f"  {key:24} {value}")

    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(
            json.dumps({"summary": summary, "results": results}, ensure_ascii=False, indent=2),
            encoding="utf-8",
        )
        print(f"\n결과를 저장했습니다: {args.report}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
