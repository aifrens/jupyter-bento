#!/usr/bin/env python3
"""验证构建脚本在解压前校验固定的 Python 归档摘要。"""

from __future__ import annotations

import hashlib
import os
import re
import stat
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
BUILD_SCRIPT = REPO_ROOT / "runtime" / "build-snapshot.sh"
CHECKSUMS = REPO_ROOT / "runtime" / "python-build-standalone-20211017.sha256"
WORK_ROOT = REPO_ROOT / "test" / "work" / "build-snapshot-checksums"

EXPECTED_ASSETS = {
    "cpython-3.9.7-aarch64-apple-darwin-pgo+lto-20211017T1616.tar.zst": (
        "d75cfebb74df7c5195970e4ea77466e16cc0057ffd9c8442477b941b517f1639"
    ),
    "cpython-3.9.7-x86_64-apple-darwin-pgo+lto-20211017T1616.tar.zst": (
        "12c5bce1b48d2b896049ec2524648d5429eba982539bc571d43c1a2ec3997630"
    ),
    "cpython-3.9.7-x86_64-pc-windows-msvc-shared-pgo-20211017T1616.tar.zst": (
        "634a4ed5a05c1bc9f158954bc4849de69d6b7c2c42d9483a875006f33eb0f17c"
    ),
    "cpython-3.9.7-x86_64-unknown-linux-gnu-pgo+lto-20211017T1616.tar.zst": (
        "33cb6a4895418b9de2b770f8ab72d1fd2dbebb95747a81f6d1824c1900062df8"
    ),
}

ASSET_PATTERN = re.compile(
    r'(?:^|[;)])\s*(?:ASSET|HOST_ASSET|\$Asset)\s*=\s*"([^"]+)"', re.MULTILINE
)


def write_executable(path: Path, content: str) -> None:
    path.write_text(textwrap.dedent(content).lstrip(), encoding="utf-8")
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


def load_checksums(path: Path = CHECKSUMS) -> dict[str, str]:
    checksums: dict[str, str] = {}
    for raw_line in path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        digest, asset = line.split(None, 1)
        if asset in checksums:
            raise AssertionError(f"duplicate checksum entry: {asset}")
        checksums[asset] = digest
    return checksums


class BuildSnapshotChecksumTests(unittest.TestCase):
    def setUp(self) -> None:
        WORK_ROOT.mkdir(parents=True, exist_ok=True)
        self.temp_dir = tempfile.TemporaryDirectory(prefix="case-", dir=WORK_ROOT)
        self.root = Path(self.temp_dir.name)
        self.addCleanup(self.temp_dir.cleanup)

    def run_script(
        self,
        payload: bytes,
        *,
        build_script: Path = BUILD_SCRIPT,
    ) -> tuple[subprocess.CompletedProcess[str], Path]:
        fake_bin = self.root / "fake-bin"
        fake_bin.mkdir()
        events = self.root / "events.log"
        payload_path = self.root / "payload.tar.zst"
        payload_path.write_bytes(payload)

        script_runtime = build_script.parent
        original_runtime = BUILD_SCRIPT.parent
        for support_name in (
            "requirements-bootstrap-macos.lock.txt",
            "requirements-bootstrap-linux-x64.lock.txt",
            "requirements-macos-arm64.lock.txt",
        ):
            support_path = script_runtime / support_name
            if not support_path.exists():
                support_path.write_bytes((original_runtime / support_name).read_bytes())

        write_executable(
            fake_bin / "curl",
            """
            #!/bin/sh
            set -eu
            output=""
            while [ "$#" -gt 0 ]; do
              if [ "$1" = "-o" ]; then
                output="$2"
                shift 2
              else
                shift
              fi
            done
            printf 'curl\\n' >> "$TEST_EVENTS"
            cp "$TEST_PAYLOAD" "$output"
            """,
        )
        write_executable(
            fake_bin / "zstd",
            """
            #!/bin/sh
            printf 'zstd\\n' >> "$TEST_EVENTS"
            exit 86
            """,
        )
        write_executable(
            fake_bin / "tar",
            """
            #!/bin/sh
            printf 'tar\\n' >> "$TEST_EVENTS"
            exit 87
            """,
        )
        write_executable(
            fake_bin / "python3",
            """
            #!/bin/sh
            set -eu
            if [ "$#" -eq 2 ] && [ "$1" = "-c" ]; then
              printf '3.13.5\\n'
              exit 0
            fi
            if [ "$#" -eq 3 ] && [ "$1" = "-m" ] && [ "$2" = "venv" ]; then
              mkdir -p "$3/bin"
              cp "$FAKE_BOOTSTRAP_PYTHON" "$3/bin/python"
              exit 0
            fi
            exit 65
            """,
        )
        fake_bootstrap = self.root / "fake-bootstrap-python"
        write_executable(
            fake_bootstrap,
            """
            #!/bin/sh
            set -eu
            if [ "$#" -ge 3 ] && [ "$1" = "-m" ] && [ "$2" = "pip" ] && [ "$3" = "install" ]; then
              exit 0
            fi
            printf 'bootstrap\\n' >> "$TEST_EVENTS"
            exit 88
            """,
        )

        environment = os.environ.copy()
        environment.update(
            {
                "PATH": f"{fake_bin}{os.pathsep}{environment['PATH']}",
                "TEST_EVENTS": str(events),
                "TEST_PAYLOAD": str(payload_path),
                "FAKE_BOOTSTRAP_PYTHON": str(fake_bootstrap),
            }
        )
        result = subprocess.run(
            ["/bin/bash", str(build_script), "macos-arm64", str(self.root / "out")],
            cwd=REPO_ROOT,
            env=environment,
            capture_output=True,
            text=True,
            check=False,
        )
        return result, events

    def test_manifest_pins_every_asset_used_by_the_build_scripts(self) -> None:
        self.assertEqual(load_checksums(), EXPECTED_ASSETS)

        variables = {"$PBS_TS": "20211017T1616", "$PbsTs": "20211017T1616"}
        referenced_assets: set[str] = set()
        for script in (BUILD_SCRIPT, REPO_ROOT / "runtime" / "build-snapshot.ps1"):
            source = script.read_text(encoding="utf-8")
            for raw_asset in ASSET_PATTERN.findall(source):
                asset = raw_asset
                for variable, value in variables.items():
                    asset = asset.replace(variable, value)
                referenced_assets.add(asset)

        self.assertEqual(referenced_assets, set(EXPECTED_ASSETS))

    def test_tampered_download_fails_before_decompression(self) -> None:
        result, events = self.run_script(b"tampered archive")

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("SHA-256 校验失败", result.stderr)
        self.assertEqual(events.read_text(encoding="utf-8").splitlines(), ["curl"])

    def test_matching_download_reaches_decompression(self) -> None:
        payload = b"trusted test archive"
        digest = hashlib.sha256(payload).hexdigest()
        asset = next(iter(EXPECTED_ASSETS))
        original = CHECKSUMS.read_text(encoding="utf-8")
        replacement = original.replace(EXPECTED_ASSETS[asset], digest, 1)

        isolated_runtime = self.root / "runtime"
        isolated_runtime.mkdir()
        isolated_script = isolated_runtime / BUILD_SCRIPT.name
        isolated_script.write_bytes(BUILD_SCRIPT.read_bytes())
        isolated_script.chmod(BUILD_SCRIPT.stat().st_mode)
        isolated_checksums = isolated_runtime / CHECKSUMS.name
        isolated_checksums.write_text(replacement, encoding="utf-8")

        result, events = self.run_script(payload, build_script=isolated_script)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("SHA-256 校验通过", result.stdout)
        self.assertEqual(result.returncode, 88)
        self.assertEqual(
            events.read_text(encoding="utf-8").splitlines(), ["curl", "bootstrap"]
        )

    def test_duplicate_checksum_entry_fails_before_decompression(self) -> None:
        payload = b"trusted test archive"
        digest = hashlib.sha256(payload).hexdigest()
        asset = next(iter(EXPECTED_ASSETS))
        replacement = CHECKSUMS.read_text(encoding="utf-8").replace(
            EXPECTED_ASSETS[asset], digest, 1
        )
        replacement += f"{digest}  {asset}\n"

        isolated_runtime = self.root / "runtime"
        isolated_runtime.mkdir()
        isolated_script = isolated_runtime / BUILD_SCRIPT.name
        isolated_script.write_bytes(BUILD_SCRIPT.read_bytes())
        isolated_script.chmod(BUILD_SCRIPT.stat().st_mode)
        (isolated_runtime / CHECKSUMS.name).write_text(replacement, encoding="utf-8")

        result, events = self.run_script(payload, build_script=isolated_script)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("有且仅有一个 SHA-256", result.stderr)
        self.assertEqual(events.read_text(encoding="utf-8").splitlines(), ["curl"])


if __name__ == "__main__":
    unittest.main()
