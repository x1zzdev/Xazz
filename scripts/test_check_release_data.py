import io
from pathlib import Path
import tarfile
import tempfile
import unittest
import zipfile

from check_release_data import LFS_HEADER, check_archive


class ReleaseDataTests(unittest.TestCase):
    def make_archive(self, root, kind, entries):
        path = Path(root) / f"release.{kind}"
        if kind == "zip":
            with zipfile.ZipFile(path, "w") as archive:
                for name, data in entries.items():
                    archive.writestr(name, data)
        else:
            with tarfile.open(path, "w:gz") as archive:
                for name, data in entries.items():
                    info = tarfile.TarInfo(name)
                    info.size = len(data)
                    archive.addfile(info, io.BytesIO(data))
        return path

    def test_release_data(self):
        cases = [
            ({"xazz/examples/data/air.csv": b"id,value\n1,2\n"}, None),
            ({"xazz/examples/data/air.csv": LFS_HEADER + b"\noid sha256:123\nsize 100\n"}, "air.csv"),
            ({"xazz/xazz.exe": b"binary"}, "no example CSV"),
            ({"xazz/examples/data/good.csv": b"id\n1\n", "xazz/examples/data/bad.csv": LFS_HEADER + b"\r\noid sha256:123\r\n"}, "bad.csv"),
        ]
        for kind in ("zip", "tar.gz"):
            for entries, error in cases:
                with self.subTest(kind=kind, error=error), tempfile.TemporaryDirectory() as root:
                    path = self.make_archive(root, kind, entries)
                    if error:
                        with self.assertRaisesRegex(ValueError, error):
                            check_archive(path)
                    else:
                        self.assertEqual(check_archive(path), 1)


if __name__ == "__main__":
    unittest.main()
