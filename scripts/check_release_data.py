#!/usr/bin/env python3
"""Reject release archives containing Git LFS pointers instead of example CSVs."""

import argparse
from pathlib import PurePosixPath
import tarfile
import zipfile

LFS_HEADER = b"version https://git-lfs.github.com/spec/v1"


def check_archive(path):
    """Return the number of checked CSVs; raise ValueError for unusable bundles."""
    checked = 0

    def check_member(name, stream):
        nonlocal checked
        member = PurePosixPath(name)
        if "examples" not in member.parts or member.suffix.lower() != ".csv":
            return
        checked += 1
        if stream.read(200).splitlines()[0:1] == [LFS_HEADER]:
            raise ValueError(f"Git LFS pointer bundled as example data: {name}")

    if zipfile.is_zipfile(path):
        with zipfile.ZipFile(path) as archive:
            for member in archive.infolist():
                if not member.is_dir():
                    with archive.open(member) as stream:
                        check_member(member.filename, stream)
    else:
        with tarfile.open(path, "r:*") as archive:
            for member in archive:
                if member.isfile():
                    with archive.extractfile(member) as stream:
                        check_member(member.name, stream)

    if not checked:
        raise ValueError("Release archive contains no example CSV files")
    return checked


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive")
    args = parser.parse_args()
    try:
        count = check_archive(args.archive)
    except (OSError, ValueError, tarfile.TarError, zipfile.BadZipFile) as exc:
        parser.exit(1, f"Release data check failed: {exc}\n")
    print(f"Release data check passed: {count} example CSV files contain no LFS pointers")


if __name__ == "__main__":
    main()
