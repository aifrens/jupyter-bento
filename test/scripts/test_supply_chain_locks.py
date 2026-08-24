#!/usr/bin/env python3
"""验证构建依赖锁与生产安装命令保持不可变内容边界。"""

from __future__ import annotations

import re
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
RUNTIME = REPO_ROOT / "runtime"
RUNTIME_LOCKS = {
    "macos-arm64": (
        RUNTIME / "requirements-macos-arm64.txt",
        RUNTIME / "requirements-macos-arm64.lock.txt",
    ),
    "macos-x64": (
        RUNTIME / "requirements-macos-x64.txt",
        RUNTIME / "requirements-macos-x64.lock.txt",
    ),
    "win-x64": (
        RUNTIME / "requirements-win-x64.txt",
        RUNTIME / "requirements-win-x64.lock.txt",
    ),
}
TOOL_LOCKS = (
    RUNTIME / "requirements-bootstrap-macos.lock.txt",
    RUNTIME / "requirements-bootstrap-linux-x64.lock.txt",
    RUNTIME / "requirements-bootstrap-win-x64.lock.txt",
    RUNTIME / "requirements-dmgbuild.lock.txt",
)
HASH_PATTERN = re.compile(r"--hash=sha256:[0-9a-f]{64}(?:\s|$)")
PIN_PATTERN = re.compile(r"^([A-Za-z0-9_.-]+)==([^\s\\]+)")


def logical_requirements(path: Path) -> list[str]:
    requirements: list[str] = []
    current = ""
    for raw_line in path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        current += (" " if current else "") + line.rstrip("\\").strip()
        if not line.endswith("\\"):
            requirements.append(current)
            current = ""
    if current:
        raise AssertionError(f"unterminated requirement in {path}")
    return requirements


def pinned_versions(path: Path) -> dict[str, str]:
    pins: dict[str, str] = {}
    for requirement in logical_requirements(path):
        match = PIN_PATTERN.match(requirement)
        if not match:
            raise AssertionError(f"requirement is not exact pinned: {requirement}")
        name, version = match.groups()
        key = re.sub(r"[-_.]+", "-", name).lower()
        if key in pins:
            raise AssertionError(f"duplicate requirement: {key}")
        pins[key] = version
    return pins


class SupplyChainLockTests(unittest.TestCase):
    def test_every_lock_entry_is_exact_and_hashed(self) -> None:
        for path in [lock for _, lock in RUNTIME_LOCKS.values()] + list(TOOL_LOCKS):
            with self.subTest(path=path.name):
                requirements = logical_requirements(path)
                self.assertTrue(requirements)
                for requirement in requirements:
                    self.assertRegex(requirement, PIN_PATTERN)
                    self.assertRegex(requirement, HASH_PATTERN)

    def test_runtime_locks_preserve_every_direct_version(self) -> None:
        for target, (direct, lock) in RUNTIME_LOCKS.items():
            with self.subTest(target=target):
                direct_pins = pinned_versions(direct)
                lock_pins = pinned_versions(lock)
                self.assertGreater(len(lock_pins), len(direct_pins))
                self.assertEqual(lock_pins.get("pip"), "25.3")
                self.assertEqual(
                    {name: lock_pins.get(name) for name in direct_pins}, direct_pins
                )

    def test_tool_locks_pin_expected_complete_closures(self) -> None:
        for platform in ("macos", "linux-x64", "win-x64"):
            lock = RUNTIME / f"requirements-bootstrap-{platform}.lock.txt"
            self.assertEqual(set(pinned_versions(lock)), {"pip", "zstandard"})
        self.assertEqual(
            set(pinned_versions(RUNTIME / "requirements-dmgbuild.lock.txt")),
            {"dmgbuild", "ds-store", "mac-alias"},
        )

    def test_production_scripts_enforce_hashes_and_offline_install(self) -> None:
        shell = (RUNTIME / "build-snapshot.sh").read_text(encoding="utf-8")
        powershell = (RUNTIME / "build-snapshot.ps1").read_text(encoding="utf-8")
        dmg = (RUNTIME / "make-dmg.sh").read_text(encoding="utf-8")

        for source in (shell, powershell, dmg):
            self.assertIn("--require-hashes", source)
            self.assertIn("--only-binary=:all:", source)
            self.assertIn("--disable-pip-version-check", source)
            self.assertIn("3.13.5", source)

        for source in (shell, powershell):
            self.assertIn("pip download", source)
            self.assertIn("--no-index", source)
            self.assertIn("--find-links", source)
            self.assertIn("--no-deps", source)

        self.assertNotRegex(shell, r"(?:^|\s)zstd(?:\s|$)")
        self.assertIn("--platform win_amd64", shell)
        self.assertGreaterEqual(shell.count("--platform win_amd64"), 2)

    def test_powershell_fails_closed_on_every_native_build_step(self) -> None:
        powershell = (RUNTIME / "build-snapshot.ps1").read_text(encoding="utf-8")
        operations = (
            "检查构建 Python 版本",
            "创建 bootstrap 虚拟环境",
            "安装 bootstrap 哈希锁",
            "解压 Python 归档",
            "下载运行时哈希锁 wheel",
            "离线安装运行时哈希锁 wheel",
            "导入运行时依赖",
            "预编译运行时字节码",
            "预建 matplotlib 字体缓存",
            "生成出厂包清单",
            "校验出厂包清单",
            "压缩出厂快照",
        )

        self.assertIn("function Assert-NativeCommandSucceeded", powershell)
        for operation in operations:
            with self.subTest(operation=operation):
                self.assertIn(
                    f'Assert-NativeCommandSucceeded -Operation "{operation}" '
                    "-ExitCode $LASTEXITCODE",
                    powershell,
                )
        self.assertEqual(
            powershell.count("Assert-NativeCommandSucceeded -Operation"),
            len(operations),
        )

    def test_factory_manifest_generated_by_every_snapshot_builder(self) -> None:
        """出厂包清单是应用端「内置 vs 用户安装」的唯一权威判定依据。

        回归保护：build-snapshot.ps1 曾漏掉该步骤，导致 Windows 快照没有清单、
        应用回退到 16 个核心包清单，其余出厂包全部被误判为用户安装。
        两个构建脚本必须都生成清单并写进快照树（python/install/ 下）。
        """
        shell = (RUNTIME / "build-snapshot.sh").read_text(encoding="utf-8")
        powershell = (RUNTIME / "build-snapshot.ps1").read_text(encoding="utf-8")

        for label, source in (("sh", shell), ("ps1", powershell)):
            with self.subTest(script=label):
                self.assertIn("factory-manifest.json", source)
                self.assertIn("pip list --format=json", source)
                self.assertIn("python/install", source.replace("\\", "/"))
        # PowerShell 重定向会按控制台编码写文本，清单必须显式 UTF-8 无 BOM 写盘
        self.assertIn("UTF8Encoding($false)", powershell)


if __name__ == "__main__":
    unittest.main()
