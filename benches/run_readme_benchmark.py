"""
benches/run_readme_benchmark.py — README 벤치마크 오케스트레이터
===============================================================
동일한 4단계 파이프라인(P2 정리·필터 → P3 그룹합계 → P4 Top-10 평균 → P7 fill+count)을
Python Pandas(eager)와 Xazz(Rust + Polars LazyFrame)로 실행해 측정한다.

측정 방법 (공평성):
  - 각 엔진 × 스케일 조합마다 워밍업 1회 + 측정 3회 실행, 중앙값(median) 보고
  - 지연 시간: **파이프라인 실행만** 측정 — 인터프리터 부팅은 제외
      * pandas: 스크립트가 내부에서 잰 total_latency_ms (부팅·import 제외)
      * xazz:   런타임의 [xazz:timing] 마커의 pipeline_ms (프로세스 부팅 제외)
  - 피크 RSS: **동일 기준** — 양쪽 모두 프로세스 트리(xazz는 xazz-runner 포함)를
    3ms 주기 폴링
  - 결과는 benches/benchmark_results.json 으로 저장 (`--out PATH`로 변경 가능)
  - 최상위 `provenance` 키에 측정 호스트 스펙(플랫폼/OS/CPU/코어/RAM/툴체인 버전)을
    기록한다 — 공개 벤치 수치를 머신에 귀속시키기 위함 (이슈 #262)

사용법:
    python benches/run_readme_benchmark.py [--quick] [--out PATH]
"""
from __future__ import annotations

import json
import os
import platform
import statistics
import subprocess
import sys
import time
from pathlib import Path

import psutil

ROOT = Path(__file__).parent.parent.resolve()
DATA = ROOT / "benches" / "data"
PANDAS_SCRIPT = ROOT / "benches" / "pandas_pipeline.py"
XAZZ_BIN = ROOT / "target" / "release" / "xazz"
TEMPLATE = ROOT / "benches" / "bench_scale_small.xzz"
RESULTS_PATH = ROOT / "benches" / "benchmark_results.json"

SCALES = ["small", "medium", "large"]
RUNS = 3
POLL_MS = 3
STDERR_TAIL_LINES = 20  # 서브프로세스 실패 시 예외에 싣는 stderr 꼬리 줄 수


def arg_value(flag: str, default: str) -> str:
    """`--flag VALUE` 형태의 값을 읽는다 (없으면 default)."""
    if flag in sys.argv:
        i = sys.argv.index(flag)
        if i + 1 < len(sys.argv):
            return sys.argv[i + 1]
    return default


def measure_tree(
    cmd: list[str], cwd: Path, capture: bool = False, env: dict | None = None
) -> tuple[float, float, str]:
    """서브프로세스 트리 전체의 wall-clock 지연과 피크 RSS(MB)를 측정한다.

    capture=True 면 stdout 을 돌려받는다 — [xazz:timing] 마커 파싱용.
    """
    proc_env = os.environ.copy()
    if env:
        proc_env.update(env)
    proc = subprocess.Popen(
        cmd,
        stdout=subprocess.PIPE if capture else subprocess.DEVNULL,
        # stderr 는 실패 진단용으로만 보관한다 — 성공 시 버려지고, 실패 시 꼬리를
        # 예외 메시지에 싣는다 (CI 로그에 exit code 만 남아 원인을 못 보던 문제, #147).
        stderr=subprocess.PIPE,
        cwd=cwd,
        env=proc_env,
    )
    parent = psutil.Process(proc.pid)
    peak_mb = 0.0
    t0 = time.perf_counter()
    while proc.poll() is None:
        total = 0.0
        try:
            for p in [parent, *parent.children(recursive=True)]:
                try:
                    total += p.memory_info().rss
                except (psutil.NoSuchProcess, psutil.AccessDenied):
                    pass
            peak_mb = max(peak_mb, total / 1_048_576)
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            pass
        time.sleep(POLL_MS / 1000.0)
    wall_ms = (time.perf_counter() - t0) * 1000.0
    stdout, stderr = proc.communicate()
    if proc.returncode != 0:
        tail = "\n".join(stderr.decode(errors="replace").splitlines()[-STDERR_TAIL_LINES:])
        raise RuntimeError(
            f"exit={proc.returncode}: {' '.join(cmd)}\n--- stderr (last {STDERR_TAIL_LINES} lines) ---\n{tail}"
        )
    return wall_ms, peak_mb, stdout.decode(errors="replace") if capture else ""


def parse_xazz_timing(stdout: str) -> float | None:
    """[xazz:timing] {"pipeline_ms": N} 마커에서 파이프라인 실행 지연을 추출한다."""
    for line in stdout.splitlines():
        if line.startswith("[xazz:timing] "):
            try:
                return float(json.loads(line[len("[xazz:timing] ") :])["pipeline_ms"])
            except (json.JSONDecodeError, KeyError, TypeError):
                return None
    return None


def bench_pandas(csv: Path) -> dict:
    """pandas 베이스라인: 워밍업 1회 + 측정 3회.

    지연은 스크립트가 내부에서 측정한 total_latency_ms (인터프리터 부팅·import 제외).
    RSS 는 프로세스 트리 기준 (pandas 는 자식이 없어 부모와 동일).
    """
    runs = []
    for i in range(RUNS + 1):
        _, peak, stdout = measure_tree(
            [sys.executable, str(PANDAS_SCRIPT), str(csv)],
            ROOT,
            capture=True,
        )
        metrics = json.loads(stdout.strip().splitlines()[-1])
        if i == 0:
            continue  # 워밍업
        runs.append({
            "latency_ms": round(metrics["total_latency_ms"], 1),
            "peak_mb": round(peak, 1),
        })
    return summarize(runs)


def bench_xazz(csv: Path) -> dict:
    """Xazz 정품 바이너리: 워밍업 1회 + 측정 3회.

    지연은 [xazz:timing] 마커의 pipeline_ms (프로세스 부팅 제외). 마커가 없으면
    wall-clock 으로 폴백하고 note 를 남긴다 (호환성 방어).

    NOTE: Policy-as-Code 가드레일이 절대경로 load() 를 차단하므로 스크립트가 놓인
    디렉토리를 기준으로 한 **상대경로**를 템플릿에 넣는다. 스크립트는
    benches/_bench_<scale>.xzz, 데이터는 benches/data/scale_<scale>.csv 이므로
    스크립트 기준 경로는 `data/scale_<scale>.csv` 다.
    """
    script = ROOT / "benches" / f"_bench_{csv.stem}.xzz"
    # ROOT 기준 상대경로 → policy 가드레일의 절대경로 차단을 회피하면서 데이터를 찾는다.
    rel_csv = os.path.relpath(csv, ROOT).replace("\\", "/")
    script.write_text(TEMPLATE.read_text(encoding="utf-8").replace("SCALE_CSV", rel_csv), encoding="utf-8")
    # cmd 도 ROOT 상대경로 (cwd=ROOT)
    cmd = [str(XAZZ_BIN), "run", os.path.join("benches", f"_bench_{csv.stem}.xzz")]
    runs = []
    for i in range(RUNS + 1):
        wall_ms, peak, stdout = measure_tree(cmd, ROOT, capture=True)
        if i == 0:
            continue  # 워밍업
        pipeline_ms = parse_xazz_timing(stdout)
        runs.append({
            # 우선 파이프라인 실행만 사용 (부팅 제외). 마커가 없으면 wall-clock 폴백.
            "latency_ms": round(pipeline_ms if pipeline_ms is not None else wall_ms, 1),
            "peak_mb": round(peak, 1),
            "fallback_to_wallclock": pipeline_ms is None,
        })
    script.unlink(missing_ok=True)
    return summarize(runs)


def summarize(runs: list[dict]) -> dict:
    return {
        "latency_ms": round(statistics.median(r["latency_ms"] for r in runs), 1),
        "peak_mb": round(statistics.median(r["peak_mb"] for r in runs), 1),
        "runs": runs,
    }


def _cmd_stdout(cmd: list[str]) -> str | None:
    try:
        return subprocess.check_output(cmd, text=True, stderr=subprocess.DEVNULL).strip()
    except (OSError, subprocess.CalledProcessError):
        return None


def _cpu_model() -> str | None:
    """CPU 모델명 — 프로비넌스 캡션에 필요 (이슈 #262)."""
    if sys.platform.startswith("linux"):
        try:
            for line in Path("/proc/cpuinfo").read_text(encoding="utf-8").splitlines():
                if line.lower().startswith("model name"):
                    return line.split(":", 1)[1].strip()
        except OSError:
            pass
    elif sys.platform == "darwin":
        return _cmd_stdout(["sysctl", "-n", "machdep.cpu.brand_string"])
    elif sys.platform.startswith("win"):
        return os.environ.get("PROCESSOR_IDENTIFIER")
    return None


def _pkg_version(name: str) -> str | None:
    try:
        module = __import__(name)
        return getattr(module, "__version__", None)
    except Exception:
        return None


def _xazz_version() -> str | None:
    out = _cmd_stdout([str(XAZZ_BIN), "--version"])
    if out:
        parts = out.split()
        return parts[-1] if parts else out
    try:
        for line in (ROOT / "Cargo.toml").read_text(encoding="utf-8").splitlines():
            stripped = line.strip()
            if stripped.startswith("version") and "=" in stripped:
                return stripped.split("=", 1)[1].strip().strip('"')
    except OSError:
        pass
    return None


def collect_provenance() -> dict:
    """측정 호스트/툴체인 메타데이터.

    README 벤치 수치를 머신 스펙에 귀속시키기 위해 `benchmark_results.json`의
    최상위 `provenance` 키로 기록한다 (이슈 #262 공개 게이트: 측정 머신 스펙·프로비넌스 명시).
    """
    vm = psutil.virtual_memory()
    return {
        "recorded": True,
        "platform": platform.platform(),
        "system": platform.system(),
        "release": platform.release(),
        "machine": platform.machine(),
        "processor": platform.processor() or None,
        "cpu_model": _cpu_model(),
        "cpu_physical_cores": psutil.cpu_count(logical=False),
        "cpu_logical_cores": psutil.cpu_count(logical=True),
        "total_ram_gb": round(vm.total / 1_073_741_824, 1),
        "python": platform.python_version(),
        "pandas": _pkg_version("pandas"),
        "polars": _pkg_version("polars"),
        "numpy": _pkg_version("numpy"),
        "xazz": _xazz_version(),
    }


def main() -> None:
    quick = "--quick" in sys.argv
    out_path = arg_value("--out", str(RESULTS_PATH))
    scales = ["small"] if quick else SCALES
    # 200M 행 스케일은 선택 — make_scale_data.py --xlarge 로 데이터 생성 후 --xlarge 로 측정
    if "--xlarge" in sys.argv and DATA.joinpath("scale_xlarge.csv").exists():
        scales = scales + ["xlarge"]
    results: dict = {}
    for scale in scales:
        csv = DATA / f"scale_{scale}.csv"
        if not csv.exists():
            raise SystemExit(f"{csv} 없음 — 먼저 make_scale_data.py 를 실행하세요")
        rows = sum(1 for _ in csv.open("rb")) - 1
        print(f"\n──── scale = {scale.upper()} ({rows:,} rows, {csv.stat().st_size/1_048_576:.1f} MB)")
        print("  [pandas] …", flush=True)
        results.setdefault(scale, {})["pandas"] = bench_pandas(csv)
        p = results[scale]["pandas"]
        print(f"  [pandas] median latency = {p['latency_ms']:>10,.1f} ms | peak RSS = {p['peak_mb']:,.1f} MB", flush=True)
        print("  [xazz] …", flush=True)
        results.setdefault(scale, {})["xazz"] = bench_xazz(csv)
        x = results[scale]["xazz"]
        print(f"  [xazz] median latency = {x['latency_ms']:>10,.1f} ms | peak RSS = {x['peak_mb']:,.1f} MB", flush=True)
        results[scale]["rows"] = rows

    results["provenance"] = collect_provenance()
    Path(out_path).write_text(json.dumps(results, indent=2, ensure_ascii=False), encoding="utf-8")
    print(f"\n결과 저장 → {out_path}")
    prov = results["provenance"]
    print(
        "  host: "
        f"{prov['cpu_model']} | {prov['cpu_physical_cores']}c/{prov['cpu_logical_cores']}t"
        f" | {prov['total_ram_gb']} GB | {prov['system']} {prov['release']}"
    )
    lg = results[scales[-1]]
    print(f"Speedup (last scale): {lg['pandas']['latency_ms'] / lg['xazz']['latency_ms']:.2f}x")


if __name__ == "__main__":
    main()
