#!/usr/bin/env python3
"""CPU CLI에서 비연속 검증 분할·분류 보고서를 독립 계산으로 대조한다."""

import argparse
import csv
import json
import math
import os
from pathlib import Path
import subprocess
import tempfile


def check_metrics(report, labels, predictions, indices, classes):
    size = len(classes)
    counts = [[0] * size for _ in classes]
    for index in indices:
        counts[classes.index(labels[index])][classes.index(predictions[index])] += 1
    assert report["confusion_matrix"] == sum(counts, []), (report, counts)
    precision = recall = f1 = 0.0
    for category in range(size):
        true_positive = counts[category][category]
        actual = sum(counts[category])
        predicted = sum(row[category] for row in counts)
        precision += true_positive / predicted if predicted else 0.0
        recall += true_positive / actual if actual else 0.0
        f1 += 2 * true_positive / (actual + predicted) if actual + predicted else 0.0
    expected = {
        "accuracy": sum(counts[c][c] for c in range(size)) / len(indices),
        "precision": precision / size,
        "recall": recall / size,
        "f1": f1 / size,
    }
    for name, value in expected.items():
        assert math.isclose(report[name], value, abs_tol=1e-10), (name, report, expected)


def run_case(binary, output, name, rows, options, width, expected_success=True):
    work = output / name
    work.mkdir()
    with (work / "data.csv").open("w", newline="") as handle:
        writer = csv.writer(handle)
        writer.writerow(["x", "time", "y"])
        writer.writerows(rows)
    source = (
        "type S = { x: float, time: int, y: float }; "
        f"model M {{ Dense(8) -> Dropout(0.3) -> Dense({width}) }} "
        'v data = load("data.csv") :: S; '
        f'v trained = data |> train(M, target: "y", epochs: 2, {options}); '
        'v predicted = data |> predict(trained, as: "prediction");'
    )
    (work / "case.xzz").write_text(source)
    process = subprocess.run(
        [str(binary), "run", "case.xzz", "--json"],
        cwd=work,
        env=dict(os.environ, XAZZ_BACKEND="cpu"),
        capture_output=True,
        text=True,
        timeout=90,
        check=False,
    )
    (work / "stdout.json").write_text(process.stdout)
    (work / "stderr.log").write_text(process.stderr)
    result = json.loads(process.stdout)
    if expected_success:
        assert process.returncode == 0 and result["success"], (name, result, process.stderr)
        assert len(result["rows"]) == len(rows), (name, result)
    else:
        assert process.returncode == 1 and not result["success"], (name, process.returncode, result)
    return result


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin-dir", type=Path, default=root / "target/debug")
    parser.add_argument("--output-dir", type=Path, default=root / "target/qa-integration")
    args = parser.parse_args()
    binary = args.bin_dir.resolve() / "xazz"
    args.output_dir.mkdir(parents=True, exist_ok=True)
    output = Path(tempfile.mkdtemp(prefix="classification-split-", dir=args.output_dir.resolve()))
    labels = [7, -3, 7, 7, -3, -3, 7, -3]
    timestamps = [8, 1, 7, 2, 6, 3, 5, 4]
    rows = [(i + 1, timestamp, label) for i, (timestamp, label) in enumerate(zip(timestamps, labels))]
    cases = [
        ("random", 'split: "random"', [5, 7, 4, 6], [3, 1, 2, 0]),
        ("stratified", 'split: "stratified"', [0, 1, 2, 4], [3, 5, 6, 7]),
        ("time", 'split: "sequential", time_column: "time"', [1, 3, 5, 7], [6, 4, 2, 0]),
    ]
    checks = []
    for name, split, train, validation in cases:
        result = run_case(binary, output, name, rows,
            f'{split}, validation_split: 0.5, batch_size: 1, lr: [0.01, 0.02], metric: "f1"', 2)
        report = result["training"]["report"]
        predictions = [row["prediction"] for row in result["rows"]]
        for key, indices in [("classification", train), ("validation_classification", validation)]:
            check_metrics(report[key], labels, predictions, indices, [-3, 7])
        assert report["validation"]["train_rows"] == len(train)
        assert report["validation"]["validation_rows"] == len(validation)
        assert report["validation"]["time_column"] == ("time" if name == "time" else None)
        sweep = result["sweep"]["report"]
        winner = sweep["combos"][sweep["best_index"]]
        assert winner["validation_classification"]["f1"] == max(
            combo["validation_classification"]["f1"] for combo in sweep["combos"])
        assert winner["validation_classification"] == report["validation_classification"]
        for key in ["final_train_loss", "final_val_loss"]:
            assert math.isclose(winner[key], report[key], abs_tol=1e-6)
            assert math.isfinite(report[key])
        checks.append({"case": name, "independent_metrics": "통과", "sweep": "통과"})

    rare_labels = [10, 10, 10, 20, 20, 20, 99]
    rare_rows = [(i, i, label) for i, label in enumerate(rare_labels)]
    rare = run_case(binary, output, "rare", rare_rows,
        'split: "stratified", validation_split: 0.5, batch_size: 3, lr: [0.01, 0.02]', 3)
    report = rare["training"]["report"]
    singleton = next(item for item in report["validation"]["classes"] if item["label"] == 99)
    assert (singleton["train_rows"], singleton["validation_rows"]) == (1, 0)
    predictions = [row["prediction"] for row in rare["rows"]]
    check_metrics(report["classification"], rare_labels, predictions, [0, 3, 4, 6], [10, 20, 99])
    check_metrics(report["validation_classification"], rare_labels, predictions, [1, 2, 5], [10, 20, 99])
    sweep = rare["sweep"]["report"]
    assert sweep["metric"] == "cross_entropy"
    assert sweep["combos"][sweep["best_index"]]["final_val_loss"] == min(
        combo["final_val_loss"] for combo in sweep["combos"])
    assert report["validation_classification"]["auc"] is None
    checks.append({"case": "rare", "singleton": "학습에 유지", "macro_metrics": "통과"})

    regression = run_case(binary, output, "regression", rows,
        'split: "random", validation_split: 0.5, batch_size: 3, lr: [0.01, 0.02]', 1)
    report = regression["training"]["report"]
    predictions = [row["prediction"] for row in regression["rows"]]
    for key, indices in [("final_train_loss", [5, 7, 4, 6]), ("final_val_loss", [3, 1, 2, 0])]:
        expected = sum((predictions[i] - labels[i]) ** 2 for i in indices) / len(indices)
        assert math.isclose(report[key], expected, rel_tol=1e-5, abs_tol=1e-5), (key, report[key], expected)
    checks.append({"case": "regression", "returned_model_mse": "통과"})

    negative = [
        ("invalid-fraction", rows, "validation_split: 0.95", 2, "between 0 and 0.9"),
        ("time-random", rows, 'validation_split: 0.5, split: "random", time_column: "time"', 2, "time_column requires"),
        ("equal-time", [(x, 1, y) for x, _, y in rows], 'validation_split: 0.5, time_column: "time"', 2, "strictly increasing"),
        ("label-precision", [(0, 0, 16777216), (1, 1, 16777217), (2, 2, 0), (3, 3, 0)], "validation_split: 0.5", 2, "exactly as f32"),
        ("rare-capacity", rare_rows, 'split: "stratified", validation_split: 0.9', 3, "retain every class"),
        ("undefined-auc", [(i, i, int(i == 0)) for i in range(6)], 'split: "stratified", validation_split: 0.5, metric: "auc"', 2, "both classes"),
    ]
    for name, data, options, width, expected_error in negative:
        result = run_case(binary, output, name, data, options, width, expected_success=False)
        assert expected_error in json.dumps(result), (name, result)
        checks.append({"case": name, "explicit_error": "통과"})
    summary = {"checks": checks, "evidence_directory": str(output)}
    (output / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2))
    print(json.dumps(summary, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
