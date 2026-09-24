#!/usr/bin/env python3
"""examples/semiconductor/generate_wafer_data.py — synthetic wafer metrology (issue #173)

Writes data/wafer_metrology.csv for the SPC pipeline in main.xzz. Every value is
synthesised from a fixed seed: no fab, tool vendor or customer data is used.

What the data is built to show:
  · ~1.5% of thickness readings are missing (a site the tool skipped),
  · ~0.5% are sensor glitches (0.0 or 999.9 nm) that no process can produce,
  · ETCH-02 chamber B drifts: its critical dimension (CD) creeps up lot by lot,
    which the lot-mean X-bar chart makes visible while the other tools stay flat.

The CSV is generated rather than committed because this repository tracks *.csv
with Git LFS (.gitattributes), as examples/security/generate_patients.py does.

Usage:
    python3 examples/semiconductor/generate_wafer_data.py
    python3 examples/semiconductor/generate_wafer_data.py --lots 24 --seed 7
"""

from __future__ import annotations

import argparse
import csv
import datetime as dt
import pathlib
import random

HEADER = [
    "lot_id",
    "wafer_id",
    "site",
    "tool",
    "chamber",
    "measured_at",
    "thickness_nm",
    "cd_nm",
    "temperature_c",
    "particle_count",
]
TOOLS = ["ETCH-01", "ETCH-02", "ETCH-03"]
CHAMBERS = ["A", "B"]
WAFERS_PER_LOT = 25
SITES = 5

THICKNESS_TARGET, THICKNESS_SIGMA = 100.0, 1.2  # nm
CD_TARGET, CD_SIGMA = 45.0, 0.35  # nm
CD_DRIFT_PER_LOT = 0.12  # nm per lot, ETCH-02 chamber B only
TEMP_TARGET, TEMP_SIGMA = 60.0, 0.8  # °C


def poisson(rng: random.Random, lam: float) -> int:
    # Knuth; lam is small, so the loop stays short.
    limit, k, p = pow(2.718281828459045, -lam), 0, 1.0
    while True:
        p *= rng.random()
        if p <= limit:
            return k
        k += 1


def rows(lots: int, seed: int):
    rng = random.Random(seed)
    start = dt.datetime(2026, 9, 1, 8, 0, 0)
    for lot in range(lots):
        lot_id = f"LOT-2609-{lot + 1:02d}"
        for wafer in range(1, WAFERS_PER_LOT + 1):
            tool = TOOLS[(lot + wafer) % len(TOOLS)]
            chamber = CHAMBERS[wafer % len(CHAMBERS)]
            drift = CD_DRIFT_PER_LOT * lot if (tool, chamber) == ("ETCH-02", "B") else 0.0
            when = start + dt.timedelta(hours=6 * lot, minutes=4 * wafer)
            for site in range(1, SITES + 1):
                roll = rng.random()
                if roll < 0.015:
                    thickness = ""  # site skipped by the tool
                elif roll < 0.020:
                    thickness = rng.choice(["0.0", "999.9"])  # sensor glitch
                else:
                    thickness = f"{rng.gauss(THICKNESS_TARGET, THICKNESS_SIGMA):.2f}"
                particles = poisson(rng, 2.5) + (rng.randint(15, 40) if rng.random() < 0.01 else 0)
                yield [
                    lot_id,
                    wafer,
                    site,
                    tool,
                    chamber,
                    (when + dt.timedelta(seconds=20 * site)).isoformat(),
                    thickness,
                    f"{rng.gauss(CD_TARGET + drift, CD_SIGMA):.3f}",
                    f"{rng.gauss(TEMP_TARGET, TEMP_SIGMA):.2f}",
                    particles,
                ]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--lots", type=int, default=12)
    parser.add_argument("--seed", type=int, default=173)
    parser.add_argument(
        "--out",
        type=pathlib.Path,
        default=pathlib.Path(__file__).resolve().parent / "data" / "wafer_metrology.csv",
    )
    args = parser.parse_args()
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.writer(handle)
        writer.writerow(HEADER)
        count = 0
        for row in rows(args.lots, args.seed):
            writer.writerow(row)
            count += 1
    print(f"wrote {count} rows to {args.out}")


if __name__ == "__main__":
    main()
