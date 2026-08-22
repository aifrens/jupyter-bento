#!/usr/bin/env python3
"""验证 tar.zst 提取边界不会写出目标目录。"""

from __future__ import annotations

import io
import os
import shutil
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path
from typing import Optional


REPO_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO_ROOT))

from runtime import tarzst  # noqa: E402


WORK_ROOT = REPO_ROOT / "test" / "work" / "tarzst"


def make_archive(*members: tuple[tarfile.TarInfo, bytes]) -> io.BytesIO:
    archive = io.BytesIO()
    with tarfile.open(fileobj=archive, mode="w") as tf:
        for member, payload in members:
            member.size = len(payload) if member.isfile() else 0
            tf.addfile(member, io.BytesIO(payload) if payload else None)
    archive.seek(0)
    return archive


def member(
    name: str,
    *,
    payload: bytes = b"",
    member_type: bytes = tarfile.REGTYPE,
    linkname: str = "",
) -> tuple[tarfile.TarInfo, bytes]:
    info = tarfile.TarInfo(name)
    info.type = member_type
    info.linkname = linkname
    info.mode = 0o755 if member_type == tarfile.DIRTYPE else 0o644
    return info, payload


class SafeTarExtractionTests(unittest.TestCase):
    def setUp(self) -> None:
        WORK_ROOT.mkdir(parents=True, exist_ok=True)
        self.temp_dir = tempfile.TemporaryDirectory(prefix="case-", dir=WORK_ROOT)
        self.root = Path(self.temp_dir.name)
        self.addCleanup(self.temp_dir.cleanup)

    def extract(
        self, archive: io.BytesIO, destination: Optional[Path] = None
    ) -> Path:
        destination = destination or self.root / "output"
        tarzst._extract_tar_stream(archive, str(destination))
        return destination

    def assert_rejected(self, name: str) -> None:
        outside = self.root / "outside.txt"
        archive = make_archive(member(name, payload=b"untrusted"))
        with self.assertRaises(tarzst.UnsafeArchiveError):
            self.extract(archive)
        self.assertFalse(outside.exists())

    def test_rejects_absolute_and_parent_member_paths(self) -> None:
        cases = (
            "../outside.txt",
            "python/../../outside.txt",
            "python\\..\\..\\outside.txt",
            str(self.root / "absolute.txt"),
            "C:\\outside.txt",
            "\\\\server\\share\\outside.txt",
            "python/.. /outside.txt",
            "python/NUL.txt",
            "python/file.txt:stream",
        )
        for name in cases:
            with self.subTest(name=name):
                self.assert_rejected(name)

    def test_rejects_symlink_and_hardlink_targets_outside_root(self) -> None:
        cases = (
            member(
                "python/link",
                member_type=tarfile.SYMTYPE,
                linkname="../../outside.txt",
            ),
            member(
                "python/link",
                member_type=tarfile.SYMTYPE,
                linkname="C:\\outside.txt",
            ),
            member(
                "python/link",
                member_type=tarfile.LNKTYPE,
                linkname="../outside.txt",
            ),
            member(
                "python/link",
                member_type=tarfile.LNKTYPE,
                linkname="/outside.txt",
            ),
        )
        for unsafe_member in cases:
            with self.subTest(linkname=unsafe_member[0].linkname):
                with self.assertRaises(tarzst.UnsafeArchiveError):
                    self.extract(make_archive(unsafe_member))

    def test_rejects_preexisting_symlink_ancestor(self) -> None:
        destination = self.root / "output"
        outside = self.root / "outside"
        destination.mkdir()
        outside.mkdir()
        try:
            (destination / "python").symlink_to(outside, target_is_directory=True)
        except OSError as exc:
            self.skipTest(f"当前平台不能创建测试 symlink: {exc}")

        archive = make_archive(member("python/payload.txt", payload=b"untrusted"))
        with self.assertRaises(tarzst.UnsafeArchiveError):
            self.extract(archive, destination)
        self.assertFalse((outside / "payload.txt").exists())

    def test_rejects_special_and_unknown_member_types(self) -> None:
        for member_type in (
            tarfile.CHRTYPE,
            tarfile.BLKTYPE,
            tarfile.FIFOTYPE,
            b"Z",
        ):
            with self.subTest(member_type=member_type):
                archive = make_archive(member("python/special", member_type=member_type))
                with self.assertRaises(tarzst.UnsafeArchiveError):
                    self.extract(archive)

    def test_extracts_regular_files_directories_and_hardlinks(self) -> None:
        data_member = member("python/data.txt", payload=b"trusted")
        data_member[0].mode = 0o6777
        archive = make_archive(
            member("./python", member_type=tarfile.DIRTYPE),
            data_member,
            member(
                "python/data-copy.txt",
                member_type=tarfile.LNKTYPE,
                linkname="python/data.txt",
            ),
        )

        destination = self.extract(archive)

        self.assertEqual((destination / "python" / "data.txt").read_bytes(), b"trusted")
        self.assertEqual(
            (destination / "python" / "data-copy.txt").read_bytes(), b"trusted"
        )
        self.assertEqual(
            (destination / "python" / "data.txt").stat().st_mode & 0o7000,
            0,
        )
        self.assertEqual(
            (destination / "python" / "data.txt").stat().st_mode & 0o022,
            0,
        )

    @unittest.skipIf(os.name == "nt", "Windows CI 默认不允许创建 symlink")
    def test_extracts_symlink_whose_parent_reference_stays_inside_root(self) -> None:
        archive = make_archive(
            member("python", member_type=tarfile.DIRTYPE),
            member("python/Lib", member_type=tarfile.DIRTYPE),
            member("python/Lib/module.py", payload=b"trusted"),
            member("python/bin", member_type=tarfile.DIRTYPE),
            member(
                "python/bin/module.py",
                member_type=tarfile.SYMTYPE,
                linkname="../Lib/module.py",
            ),
        )

        destination = self.extract(archive)

        link = destination / "python" / "bin" / "module.py"
        self.assertTrue(link.is_symlink())
        self.assertEqual(link.read_bytes(), b"trusted")


if __name__ == "__main__":
    try:
        raise SystemExit(unittest.main())
    finally:
        shutil.rmtree(WORK_ROOT, ignore_errors=True)
