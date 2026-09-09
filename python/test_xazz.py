"""Tests for the xazz Python bindings (issue C4).

Runs with pytest (`python -m pytest python/test_xazz.py`) or directly
(`python python/test_xazz.py`). Requires a built xazz CLI
(cargo build -p xazz -p xazz-runner).
"""

from __future__ import annotations

import sys
import os
import unittest

sys.path.insert(0, os.path.dirname(__file__))
import xazz  # noqa: E402

SAFE = (
    "type AQ = { station: string, pm10: float };\n"
    "v x = load(\"duckdb://:memory:?sql=SELECT 'seoul' AS station, 45.5 AS pm10 UNION ALL SELECT 'busan', 52.1\") :: AQ\n"
    "  |> groupBy(\"station\")\n"
    "  |> mean(\"pm10\");"
)

UNSAFE = (
    "type P = { temp: float };\n"
    "v a = load(\"d.csv\") :: P |> select([temperture_c]);"
)


class XazzBindingTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # Skip everything if the CLI isn't built.
        try:
            xazz.find_xazz()
        except xazz.XazzError as e:
            raise unittest.SkipTest(f"xazz binary not available: {e}")

    def test_check_clean(self):
        r = xazz.check(
            'type AQ = { station: string, pm10: float };\n'
            'v x = load("d.csv") :: AQ |> select([station, pm10]);'
        )
        self.assertTrue(r.success)
        self.assertEqual(r.error_count, 0)

    def test_check_detects_typo(self):
        r = xazz.check(UNSAFE)
        self.assertFalse(r.success)
        self.assertEqual(r.error_count, 1)
        msg = r.errors[0]["message"]
        self.assertIn("temperture_c", msg)
        # did-you-mean hint present (same as CLI).
        self.assertTrue(
            any("temp" in str(s) for s in r.errors[0].values()),
            f"no suggestion: {r.errors[0]}",
        )

    def test_run_returns_rows(self):
        r = xazz.run(SAFE)
        self.assertTrue(r.success, r.error)
        self.assertEqual(len(r.rows), 2)
        stations = {row["station"] for row in r.rows}
        self.assertIn("seoul", stations)

    def test_run_error_surfaces(self):
        r = xazz.run('v x = load("no_such_file.csv") :: P;')
        # No such type P → static analysis fails → execution not successful.
        self.assertFalse(r.success)

    def test_policy_report(self):
        p = xazz.policy(SAFE)
        # The report nests under 'policy' in the CLI JSON.
        inner = p.get("policy", p)
        self.assertTrue(inner.get("safe_to_execute"))


if __name__ == "__main__":
    unittest.main()