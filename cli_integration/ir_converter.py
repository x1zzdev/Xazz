"""
ir_converter.py — Xazz → IR JSON converter (minimal regex-based stub)

Converts .xzz source code into a simple pipeline IR JSON that the NQP model
can understand. This is a glue-layer stub; it does NOT execute Xazz.
"""

import re
import json


def convert_xzz_to_ir(source: str) -> dict:
    """
    Parse Xazz source (minimal regex) and return IR dict.

    IR format:
    {
      "source": "<original source>",
      "pipeline_text": "<YAML-like pipeline for NQP>",
      "pipeline": [
        {"op": "load",  "args": {"path": "data.csv"}},
        {"op": "filter","args": {"column": "pm10", "condition": "< 120"}},
        ...
      ]
    }
    """
    ops = []
    pipeline_lines = []

    # ── 1. load() calls ──────────────────────────────────────────────────────
    for m in re.finditer(r'load\(\s*"([^"]+)"\s*\)', source):
        path = m.group(1)
        ops.append({"op": "load", "args": {"path": path}})
        pipeline_lines.append(f"  - load: {path}")

    # ── 2. select() ──────────────────────────────────────────────────────────
    for m in re.finditer(r'\|>\s*select\(\[([^\]]+)\]\)', source):
        cols = [c.strip() for c in m.group(1).split(",")]
        ops.append({"op": "select", "args": {"columns": cols}})
        pipeline_lines.append(f"  - op: select")
        pipeline_lines.append(f"    columns: {json.dumps(cols)}")

    # ── 3. dropNull() ────────────────────────────────────────────────────────
    for m in re.finditer(r'\|>\s*dropNull\(\s*"([^"]+)"\s*\)', source):
        col = m.group(1)
        ops.append({"op": "drop_null", "args": {"column": col}})
        pipeline_lines.append(f"  - op: drop_null")
        pipeline_lines.append(f"    column: {col}")

    # ── 4. fillNull() ────────────────────────────────────────────────────────
    for m in re.finditer(r'\|>\s*fillNull\(\s*"([^"]+)"\s*,\s*([^)]+)\)', source):
        col = m.group(1)
        val = m.group(2).strip()
        ops.append({"op": "fill_null", "args": {"column": col, "value": val}})
        pipeline_lines.append(f"  - op: fill_null")
        pipeline_lines.append(f"    column: {col}")
        pipeline_lines.append(f"    value: {val}")

    # ── 5. filter(col("x") OP value) ─────────────────────────────────────────
    for m in re.finditer(r'\|>\s*filter\(\s*col\(\s*"([^"]+)"\s*\)\s*([<>=!]+)\s*([^)]+)\)', source):
        col = m.group(1)
        op_sym = m.group(2).strip()
        val = m.group(3).strip()
        ops.append({"op": "filter", "args": {"column": col, "condition": f"{op_sym} {val}"}})
        pipeline_lines.append(f"  - op: filter")
        pipeline_lines.append(f"    column: {col}")
        pipeline_lines.append(f"    condition: {op_sym} {val}")

    # ── 6. groupBy() ─────────────────────────────────────────────────────────
    for m in re.finditer(r'\|>\s*groupBy\(\s*"([^"]+)"\s*\)', source):
        col = m.group(1)
        ops.append({"op": "group_by", "args": {"key": col}})
        pipeline_lines.append(f"  - op: group_by")
        pipeline_lines.append(f"    key: {col}")

    # ── 7. aggregations: sum/mean/min/max/count ───────────────────────────────
    for agg in ("sum", "mean", "min", "max", "count"):
        for m in re.finditer(r'\|>\s*' + agg + r'\(\s*"([^"]+)"\s*\)', source):
            col = m.group(1)
            ops.append({"op": agg, "args": {"column": col}})
            pipeline_lines.append(f"  - op: {agg}")
            pipeline_lines.append(f"    column: {col}")

    # ── 8. orderBy() ─────────────────────────────────────────────────────────
    for m in re.finditer(r'\|>\s*orderBy\(\s*"([^"]+)"\s*(?:,\s*desc:\s*(true|false))?\s*\)', source):
        col = m.group(1)
        desc = m.group(2) == "true" if m.group(2) else False
        ops.append({"op": "order_by", "args": {"column": col, "desc": desc}})
        pipeline_lines.append(f"  - op: order_by")
        pipeline_lines.append(f"    column: {col}")
        pipeline_lines.append(f"    desc: {str(desc).lower()}")

    # ── 9. take() ────────────────────────────────────────────────────────────
    for m in re.finditer(r'\|>\s*take\(\s*(\d+)\s*\)', source):
        n = int(m.group(1))
        ops.append({"op": "take", "args": {"n": n}})
        pipeline_lines.append(f"  - op: take")
        pipeline_lines.append(f"    n: {n}")

    # ── Fallback: if nothing extracted, add a generic op ─────────────────────
    if not ops:
        ops.append({"op": "unknown", "args": {"raw": source[:200]}})
        pipeline_lines.append("  - op: unknown")
        pipeline_lines.append(f"    raw: {source[:200]!r}")

    pipeline_text = "pipeline:\n" + "\n".join(pipeline_lines)

    return {
        "source": source,
        "pipeline_text": pipeline_text,
        "pipeline": ops,
    }


def build_nqp_instruction(ir: dict) -> tuple[str, str]:
    """
    Build (instruction, input_text) for the NQP model from the IR dict.
    Returns a tuple compatible with run_inference(instruction, input_text).
    """
    instruction = (
        "Predict the semantic output of the following Xazz pipeline. "
        "Describe what data transformations will occur, what the expected "
        "result structure will be, and flag any potential data quality issues."
    )
    input_text = ir["pipeline_text"]
    return instruction, input_text


if __name__ == "__main__":
    # Quick self-test
    sample = """
v cleaned = load("examples/seoul_air_2026.csv")
  |> dropNull("pm10")
  |> filter(col("pm10") < 120)
  |> groupBy("station")
  |> mean("pm10")
  |> orderBy("pm10", desc: true)
  |> take(10);
"""
    ir = convert_xzz_to_ir(sample)
    print(json.dumps(ir, indent=2, ensure_ascii=False))
