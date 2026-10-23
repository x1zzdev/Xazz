import datetime as dt
import json
from pathlib import Path
import tempfile
import unittest

from check_dependency_exceptions import validate


class ExceptionEnforcement(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        (self.root / "security").mkdir()
        (self.root / "deny.toml").write_text(
            '[advisories]\nignore = [{id="RUSTSEC-2025-0141", reason="bridge"}]\n'
        )
        (self.root / "Cargo.lock").write_text('[[package]]\nname="bincode"\nversion="2.0.1"\n')
        self.entry = dict(id="RUSTSEC-2025-0141", package="bincode", version="2.0.1",
                          kind="unmaintained", expires="2026-11-21", reason="bridge",
                          removal_plan="migrate checkpoints")

    def check(self, entries=None, today=dt.date(2026, 10, 22)):
        (self.root / "security/dependency-exceptions.json").write_text(
            json.dumps({"exceptions": entries if entries is not None else [self.entry]})
        )
        validate(self.root, today)

    def test_current_maintenance_exception(self):
        self.check()

    def test_expiry_day_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "expired"):
            self.check(today=dt.date(2026, 11, 21))

    def test_unregistered_ignore_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "exactly match"):
            self.check(entries=[])

    def test_vulnerability_cannot_use_maintenance_exception(self):
        self.entry["kind"] = "vulnerability"
        with self.assertRaisesRegex(ValueError, "only maintenance"):
            self.check()

    def test_changed_version_requires_review(self):
        self.entry["version"] = "2.0.2"
        with self.assertRaisesRegex(ValueError, "changed package"):
            self.check()

    def test_duplicate_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "duplicate"):
            self.check(entries=[self.entry, self.entry])

    def test_missing_removal_plan_is_rejected(self):
        self.entry["removal_plan"] = ""
        with self.assertRaisesRegex(ValueError, "removal plan"):
            self.check()


if __name__ == "__main__":
    unittest.main()
