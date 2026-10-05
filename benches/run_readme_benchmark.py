"""동일한 CPU 파이프라인의 pandas·Xazz 지연 및 프로세스 트리 RSS를 측정한다.

기본 실행은 기존 small/medium/large 및 pandas/xazz 결과 형태를 유지한다.
--quick은 small만, --scale은 지정한 스케일만 측정한다. --xlarge는 200M 행
파일을 추가 요청하며, 파일이 없으면 실패한다. 내부 실행시간 마커가 없는 실행은
wall-clock으로 대체하지 않는다. 원시 로그와 실패 기록은 출력 파일 옆에 보존한다.
"""
from __future__ import annotations

import argparse
import csv as csv_module
from datetime import datetime, timezone
import hashlib
import json
import math
import os
import platform
import signal
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import psutil

ROOT = Path(__file__).parent.parent.resolve()
DATA = ROOT / "benches" / "data"
PANDAS_SCRIPT = ROOT / "benches" / "pandas_pipeline.py"
XAZZ_BIN = ROOT / "target" / "release" / ("xazz.exe" if os.name == "nt" else "xazz")
TEMPLATE = ROOT / "benches" / "bench_scale_small.xzz"
RESULTS_PATH = ROOT / "benches" / "benchmark_results.json"
SCALES = ["small", "medium", "large"]
RUNS = 3
POLL_MS = 3
STDERR_TAIL_LINES = 20


def positive_number(value: str) -> float:
    number = float(value)
    if not math.isfinite(number) or number <= 0:
        raise argparse.ArgumentTypeError("유한한 양수여야 합니다")
    return number


def positive_integer(value: str) -> int:
    number = int(value)
    if number < 1:
        raise argparse.ArgumentTypeError("1 이상의 정수여야 합니다")
    return number


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__, allow_abbrev=False)
    selection = parser.add_mutually_exclusive_group()
    selection.add_argument("--quick", action="store_true", help="small만 측정")
    selection.add_argument("--scale", action="append", choices=[*SCALES, "xlarge"],
                           help="측정 스케일 선택; 여러 번 지정 가능")
    parser.add_argument("--xlarge", action="store_true", help="선택 목록에 xlarge 추가")
    parser.add_argument("--data-dir", type=Path, default=DATA)
    parser.add_argument("--xazz-bin", type=Path, default=XAZZ_BIN)
    parser.add_argument("--out", type=Path, default=RESULTS_PATH)
    parser.add_argument("--streaming", choices=["auto", "on", "off", "both"], default="auto",
                        help="CPU 스트리밍 요청 모드; 기본 auto는 기존 결과 형태 유지")
    parser.add_argument("--runs", type=positive_integer, default=RUNS)
    parser.add_argument("--timeout-seconds", type=positive_number, default=120.0,
                        help="워밍업을 포함한 개별 실행 제한시간; 기본 120초")
    parser.add_argument("--max-memory-mb", type=positive_number, default=4096.0,
                        help="개별 프로세스 트리 RSS 제한(MiB); 기본 4096")
    args = parser.parse_args(argv)
    args.scales = list(dict.fromkeys(args.scale or (["small"] if args.quick else SCALES)))
    if args.xlarge and "xlarge" not in args.scales:
        args.scales.append("xlarge")
    args.streaming_modes = ["on", "off"] if args.streaming == "both" else [args.streaming]
    return args


def _stop_tree(proc: subprocess.Popen, parent: psutil.Process | None,
               observed_children: dict) -> None:
    """동일 프로세스 그룹과 관측한 분리 세션 자식을 종료하고 회수한다.

    관측되기 전에 완전히 세션을 분리하고 부모까지 종료한 임의의 프로세스는
    폴링만으로 추적할 수 없다. 측정 대상은 사전 확인한 Xazz 번들이다.
    """
    children = dict(observed_children)
    try:
        if parent is not None:
            children.update((child.pid, child) for child in parent.children(recursive=True))
    except (psutil.NoSuchProcess, psutil.AccessDenied):
        pass
    if os.name == "posix":
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    for child in children.values():
        try:
            child.kill()
        except psutil.NoSuchProcess:
            pass
    if proc.poll() is None:
        proc.kill()
    proc.wait(timeout=10)
    psutil.wait_procs(list(children.values()), timeout=2)


def _group_remains(pid: int) -> bool:
    if os.name != "posix":
        return False
    try:
        os.killpg(pid, 0)
    except ProcessLookupError:
        return False
    return True


def measure_tree(
    cmd: list[str], cwd: Path, capture: bool = False, env: dict | None = None,
    *, timeout_seconds: float = 120.0, max_memory_mb: float = 4096.0,
    log_dir: Path | None = None, log_name: str = "run",
    failure_markers: tuple[str, ...] = (),
) -> tuple[float, float, str]:
    """두 엔진 모두 3ms 주기로 동일한 프로세스 트리 RSS를 측정한다.

    stdout/stderr는 파이프 대신 파일로 배출해 출력량에 따른 교착을 방지한다.
    반환 wall-clock은 진단값이며 파이프라인 지연의 대체값으로 사용하지 않는다.
    """
    if log_dir is None:
        log_dir = Path(tempfile.mkdtemp(prefix="xazz-benchmark-"))
    log_dir.mkdir(parents=True, exist_ok=True)
    stdout_path, stderr_path = log_dir / f"{log_name}.stdout.log", log_dir / f"{log_name}.stderr.log"
    proc_env = os.environ.copy()
    for key, value in (env or {}).items():
        if value is None:
            proc_env.pop(key, None)
        else:
            proc_env[key] = value
    record = {"command": cmd, "cwd": str(cwd), "status": "failed",
              "timeout_seconds": timeout_seconds, "max_memory_mb": max_memory_mb,
              "stdout": str(stdout_path), "stderr": str(stderr_path), "env_overrides": env or {}}
    proc = parent = None
    observed_children = {}
    peak_mb = 0.0
    t0 = time.perf_counter()
    try:
        with stdout_path.open("wb") as stdout_log, stderr_path.open("wb") as stderr_log:
            proc = subprocess.Popen(cmd, stdout=stdout_log, stderr=stderr_log, cwd=cwd,
                                    env=proc_env, start_new_session=os.name == "posix")
            parent = psutil.Process(proc.pid)
            while proc.poll() is None:
                total = 0
                try:
                    children = parent.children(recursive=True)
                    observed_children.update((child.pid, child) for child in children)
                    processes = [parent, *children]
                except psutil.NoSuchProcess:
                    processes = []
                for process in processes:
                    try:
                        total += process.memory_info().rss
                    except psutil.NoSuchProcess:
                        pass
                peak_mb = max(peak_mb, total / 1_048_576)
                if peak_mb > max_memory_mb:
                    raise RuntimeError(f"프로세스 트리 RSS 제한 초과: {peak_mb:.1f} > {max_memory_mb:.1f} MiB")
                if time.perf_counter() - t0 > timeout_seconds:
                    raise RuntimeError(f"실행 제한시간 초과: {timeout_seconds:g}초")
                time.sleep(POLL_MS / 1000.0)
            proc.wait()
        for child in observed_children.values():
            try:
                if child.is_running() and child.status() != psutil.STATUS_ZOMBIE:
                    raise RuntimeError(f"주프로세스 종료 후 자식 프로세스가 남았습니다: pid={child.pid}")
            except psutil.NoSuchProcess:
                pass
        if _group_remains(proc.pid):
            raise RuntimeError("주프로세스 종료 후 같은 프로세스 그룹의 자식이 남았습니다")
        if proc.returncode != 0:
            tail = "\n".join(stderr_path.read_text(errors="replace").splitlines()[-STDERR_TAIL_LINES:])
            raise RuntimeError(f"exit={proc.returncode}: {' '.join(cmd)}\n{tail}")
        for log in [stdout_path, stderr_path]:
            with log.open(errors="replace") as handle:
                for line in handle:
                    if any(marker in line for marker in failure_markers):
                        raise RuntimeError(f"성공 종료 코드와 함께 실행 오류가 출력됐습니다: {line.strip()}")
        if peak_mb <= 0:
            raise RuntimeError("프로세스 트리 RSS 표본을 얻지 못했습니다")
        record["status"] = "ok"
    except BaseException as error:
        record["error"] = str(error)
        if proc is not None:
            _stop_tree(proc, parent, observed_children)
        raise
    finally:
        wall_ms = (time.perf_counter() - t0) * 1000.0
        record.update(wall_ms=wall_ms, peak_mb=peak_mb,
                      exit_code=proc.returncode if proc is not None else None)
        (log_dir / f"{log_name}.measurement.json").write_text(
            json.dumps(record, ensure_ascii=False, indent=2), encoding="utf-8")
    return wall_ms, peak_mb, stdout_path.read_text(errors="replace") if capture else ""


def _latency(value: object, name: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise RuntimeError(f"{name}는 숫자여야 합니다: {value!r}")
    number = float(value)
    if not math.isfinite(number) or number <= 0:
        raise RuntimeError(f"{name}는 유한한 양수여야 합니다: {value!r}")
    return number


def _unique_object(pairs: list[tuple]) -> dict:
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"JSON 중복 키는 허용하지 않습니다: {key}")
        result[key] = value
    return result


def parse_xazz_timing(stdout: str) -> float:
    markers = [line[len("[xazz:timing] "):] for line in stdout.splitlines()
               if line.startswith("[xazz:timing] ")]
    if len(markers) != 1:
        raise RuntimeError(f"정확히 1개의 [xazz:timing] 마커가 필요합니다; 발견={len(markers)}")
    try:
        value = json.loads(markers[0], object_pairs_hook=_unique_object)["pipeline_ms"]
    except (ValueError, KeyError, TypeError) as error:
        raise RuntimeError("[xazz:timing] pipeline_ms 마커가 올바르지 않습니다") from error
    return _latency(value, "pipeline_ms")


def summarize(runs: list[dict]) -> dict:
    latency = round(statistics.median(run["latency_ms"] for run in runs), 1)
    if latency <= 0:
        raise RuntimeError("지연 중앙값을 기존 0.1ms 결과 해상도로 표현할 수 없습니다")
    return {"latency_ms": latency,
            "peak_mb": round(statistics.median(run["peak_mb"] for run in runs), 1), "runs": runs}


def bench_pandas(csv: Path, *, runs: int = RUNS, **measurement) -> dict:
    samples = []
    for index in range(runs + 1):
        _, peak, stdout = measure_tree([sys.executable, str(PANDAS_SCRIPT), str(csv)], ROOT,
            capture=True, log_name=f"{csv.stem}-pandas-{index}", **measurement)
        try:
            metrics = json.loads(stdout.strip().splitlines()[-1], object_pairs_hook=_unique_object)
            latency = _latency(metrics["total_latency_ms"], "pandas total_latency_ms")
        except (IndexError, KeyError, ValueError, TypeError) as error:
            raise RuntimeError("pandas 내부 실행시간 결과가 올바르지 않습니다") from error
        if index:
            samples.append({"latency_ms": latency, "peak_mb": peak})
    return summarize(samples)


def bench_xazz(csv: Path, *, xazz_bin: Path = XAZZ_BIN, streaming: str = "auto",
               runs: int = RUNS, **measurement) -> dict:
    """기존 xazz 키는 auto, 명시 모드는 별도 키로 저장한다."""
    with tempfile.NamedTemporaryFile(prefix="_bench_", suffix=".xzz", dir=csv.parent,
                                     mode="w", encoding="utf-8", delete=False) as handle:
        script = Path(handle.name)
        handle.write(TEMPLATE.read_text(encoding="utf-8").replace("SCALE_CSV", csv.name))
    samples = []
    try:
        for index in range(runs + 1):
            _, peak, stdout = measure_tree([str(xazz_bin), "run", script.name], csv.parent,
                capture=True, env={"XAZZ_BACKEND": "cpu", "XAZZ_STREAMING":
                                   {"auto": None, "on": "1", "off": "0"}[streaming],
                                   "XAZZ_RUNNER_PATH": str(xazz_bin.with_name("xazz-runner" + (".exe" if os.name == "nt" else ""))),
                                   "XAZZ_EXEC_PATH": str(xazz_bin.with_name("xazz-exec" + (".exe" if os.name == "nt" else ""))),
                                   "XAZZ_EXEC_TIMEOUT_SECS": str(math.ceil(measurement.get("timeout_seconds", 120.0)) + 1)},
                failure_markers=("[xazz RUNTIME ERROR]",),
                log_name=f"{csv.stem}-xazz-{streaming}-{index}", **measurement)
            latency = parse_xazz_timing(stdout)
            if index:
                samples.append({"latency_ms": latency, "peak_mb": peak,
                                "fallback_to_wallclock": False})
    finally:
        script.unlink(missing_ok=True)
    return summarize(samples)


def _cmd_stdout(cmd: list[str]) -> str | None:
    try:
        return subprocess.check_output(cmd, text=True, stderr=subprocess.DEVNULL, timeout=10).strip()
    except (OSError, subprocess.CalledProcessError, subprocess.TimeoutExpired):
        return None


def _cpu_model() -> str | None:
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
    except ImportError:
        return None


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def collect_provenance(xazz_bin: Path = XAZZ_BIN, *, streaming_modes: list[str] | None = None) -> dict:
    version = _cmd_stdout([str(xazz_bin), "--version"])
    siblings = {path.name: _sha256(path) for path in _bundle_paths(xazz_bin).values() if path.is_file()}
    return {"recorded": True, "platform": platform.platform(), "system": platform.system(),
        "release": platform.release(), "machine": platform.machine(),
        "processor": platform.processor() or None, "cpu_model": _cpu_model(),
        "cpu_physical_cores": psutil.cpu_count(logical=False), "cpu_logical_cores": psutil.cpu_count(),
        "total_ram_gb": round(psutil.virtual_memory().total / 1_073_741_824, 1),
        "python": platform.python_version(), "pandas": _pkg_version("pandas"),
        "polars": _pkg_version("polars"), "numpy": _pkg_version("numpy"),
        "xazz": version, "xazz_version_verified": bool(version),
        "xazz_binary": str(xazz_bin), "binary_sha256": siblings,
        "streaming_requested_modes": streaming_modes or ["auto"],
        "streaming_actual_engine_verified": False,
        "streaming_note": "스트리밍 요청 실패 시 런타임이 메모리 엔진으로 전환할 수 있어 실제 엔진은 이 측정기로 확인할 수 없습니다.",
        "rss_method": "프로세스 트리 RSS 합계의 3ms 폴링 최댓값(MiB)",
        "latency_method": "각 엔진 내부 파이프라인 실행시간; wall-clock 대체 금지",
        "recorded_at_utc": datetime.now(timezone.utc).isoformat(),
        "git_head": _cmd_stdout(["git", "-C", str(ROOT), "rev-parse", "HEAD"]),
        "benchmark_script_sha256": _sha256(Path(__file__)),
        "pipeline_template_sha256": _sha256(TEMPLATE),
        "pandas_pipeline_sha256": _sha256(PANDAS_SCRIPT),
        "summary_latency_resolution_ms": 0.1}


def _bundle_paths(binary: Path) -> dict[str, Path]:
    suffix = ".exe" if os.name == "nt" else ""
    return {"xazz": binary, "xazz-runner": binary.with_name("xazz-runner" + suffix),
            "xazz-exec": binary.with_name("xazz-exec" + suffix)}


def _input_metadata(path: Path) -> dict:
    columns = ("date", "station", "pm10", "pm25")
    count = 0
    try:
        with path.open("r", encoding="utf-8", newline="") as handle:
            reader = csv_module.reader(handle, strict=True)
            header = next(reader, None)
            if header is None or tuple(header) != columns:
                raise RuntimeError(f"{path}: 헤더와 순서가 {list(columns)}여야 합니다: {header}")
            for record in reader:
                if len(record) != len(columns):
                    raise RuntimeError(
                        f"{path}: CSV {reader.line_num}번째 줄의 열 개수 {len(record)}가 헤더 {len(columns)}와 다릅니다"
                    )
                count += 1
    except (UnicodeError, csv_module.Error) as error:
        raise RuntimeError(f"{path}: 올바른 UTF-8 CSV가 아닙니다: {error}") from error
    if count <= 0:
        raise RuntimeError(f"측정할 데이터 행이 없습니다: {path}")
    return {"file": path.name, "rows": count, "bytes": path.stat().st_size,
            "sha256": _sha256(path)}


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    data_dir, binary, out = args.data_dir.resolve(), args.xazz_bin.resolve(), args.out.resolve()
    out.parent.mkdir(parents=True, exist_ok=True)
    logs = Path(tempfile.mkdtemp(prefix=out.stem + "-logs-", dir=out.parent))
    results: dict = {}
    try:
        files = {scale: data_dir / f"scale_{scale}.csv" for scale in args.scales}
        for csv in files.values():
            if not csv.is_file():
                raise RuntimeError(f"요청한 데이터 파일이 없습니다: {csv}")
        for name, path in _bundle_paths(binary).items():
            if not path.is_file() or not os.access(path, os.X_OK):
                raise RuntimeError(f"실행 가능한 {name} 바이너리가 없습니다: {path}")
        inputs = {scale: _input_metadata(csv) for scale, csv in files.items()}
        measurement = {"timeout_seconds": args.timeout_seconds, "max_memory_mb": args.max_memory_mb,
                       "log_dir": logs}
        for scale, csv in files.items():
            rows = inputs[scale]["rows"]
            print(f"\n{scale}: {rows:,}행, {csv.stat().st_size / 1_048_576:.1f} MiB", flush=True)
            entry = {"rows": rows, "pandas": bench_pandas(csv, runs=args.runs, **measurement)}
            for mode in args.streaming_modes:
                key = "xazz" if mode == "auto" else f"xazz_streaming_{mode}"
                entry[key] = bench_xazz(csv, xazz_bin=binary, streaming=mode, runs=args.runs, **measurement)
                print(f"  {key}: {entry[key]['latency_ms']:.1f} ms, {entry[key]['peak_mb']:.1f} MiB", flush=True)
            results[scale] = entry
        results["provenance"] = collect_provenance(binary, streaming_modes=args.streaming_modes)
        results["provenance"].update(warmup_runs=1, measured_runs=args.runs,
            timeout_seconds=args.timeout_seconds, max_memory_mb=args.max_memory_mb,
            raw_logs=str(logs), inputs=inputs)
        with tempfile.NamedTemporaryFile(mode="w", dir=out.parent, encoding="utf-8", delete=False) as handle:
            temporary = Path(handle.name)
            json.dump(results, handle, ensure_ascii=False, indent=2, allow_nan=False)
        temporary.replace(out)
        print(f"결과 저장: {out}\n원시 로그: {logs}")
        return 0
    except (Exception, KeyboardInterrupt) as error:
        failure = {"status": "failed", "error": str(error), "requested_scales": args.scales,
            "streaming_requested_modes": args.streaming_modes, "completed_scales": list(results),
            "success_output_updated": False, "raw_logs": str(logs)}
        (logs / "failure.json").write_text(json.dumps(failure, ensure_ascii=False, indent=2), encoding="utf-8")
        print(f"측정 실패: {error}\n성공 결과는 갱신하지 않았습니다. 실패 근거: {logs}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
