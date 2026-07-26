"""
Quick integration test: verify stdin→nqp_predict.py JSON protocol.
Run from xazz-lang root:  python cli_integration/_test_protocol.py
"""
import subprocess
import json
import sys
import os

SCRIPT = os.path.join(os.path.dirname(__file__), "nqp_predict.py")

sample_source = (
    'v raw = load("examples/seoul_air_2026.csv")\n'
    '  |> dropNull("pm10")\n'
    '  |> filter(col("pm10") < 120)\n'
    '  |> groupBy("station")\n'
    '  |> mean("pm10")\n'
    '  |> orderBy("pm10", desc: true)\n'
    '  |> take(10);\n'
)

payload = json.dumps({"source": sample_source})

print("=" * 60)
print("xazz NQP Predict — stdin/stdout protocol test")
print("=" * 60)
print(f"Script : {SCRIPT}")
print(f"Payload: {payload[:80]}...")
print()
print("Spawning subprocess (model loading may take several minutes)...")
print("Press Ctrl+C to abort.")
print()

try:
    result = subprocess.run(
        [sys.executable, SCRIPT],
        input=payload,
        capture_output=False,   # let stderr stream live (model loading progress)
        stdout=subprocess.PIPE,
        text=True,
        timeout=600,            # 10-minute safety timeout
    )
except KeyboardInterrupt:
    print("\n[ABORTED by user]")
    sys.exit(0)
except subprocess.TimeoutExpired:
    print("\n[TIMEOUT] subprocess did not finish within 600 seconds.")
    sys.exit(1)

stdout_raw = result.stdout.strip()
print()
print("─" * 60)
print(f"Exit code : {result.returncode}")
print(f"Stdout    : {stdout_raw[:300] if stdout_raw else '(empty)'}")
print()

# Try to parse JSON
json_start = stdout_raw.find("{")
if json_start >= 0:
    try:
        data = json.loads(stdout_raw[json_start:])
        print("JSON parse: OK")
        print(f"  status     : {data.get('status')}")
        print(f"  confidence : {data.get('confidence')}")
        result_text = data.get("result", "")
        print(f"  result     : {result_text[:200]}")
        if data.get("warnings"):
            for w in data["warnings"]:
                print(f"  warning    : {w}")
        ir = data.get("ir_summary", {})
        print(f"  ops_count  : {ir.get('ops_count')}")
        print(f"  ops        : {ir.get('ops')}")
    except json.JSONDecodeError as e:
        print(f"JSON parse FAILED: {e}")
        print(f"Raw: {stdout_raw[:500]}")
else:
    print("No JSON found in stdout.")
    print(f"Raw stdout: {stdout_raw[:500]}")

print("─" * 60)
