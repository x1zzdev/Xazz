#!/usr/bin/env python3
"""Copy the editable SVG sources into the paths used by the documentation."""

import argparse
from pathlib import Path
from xml.etree import ElementTree


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "docs" / "figures" / "src"
OUTPUT = SOURCE.parent


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="report stale generated figures")
    args = parser.parse_args()

    stale = []
    for source in sorted(SOURCE.glob("*.svg")):
        data = source.read_bytes()
        ElementTree.fromstring(data)
        target = OUTPUT / source.name
        if not target.exists() or target.read_bytes() != data:
            stale.append(source.name)
            if not args.check:
                target.write_bytes(data)

    missing_sources = sorted(
        path.name for path in OUTPUT.glob("*.svg") if not (SOURCE / path.name).exists()
    )
    if missing_sources:
        print("Missing sources: " + ", ".join(missing_sources))
        return 1
    if stale:
        print("Stale figures: " + ", ".join(stale))
        return int(args.check)
    print("All figures match their editable sources.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
