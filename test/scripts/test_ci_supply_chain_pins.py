#!/usr/bin/env python3
"""验证 CI 只执行固定内容的 Action 与构建工具链。"""

from __future__ import annotations

import re
import unittest
from collections import Counter
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "build.yml"
FULL_SHA = re.compile(r"^[0-9a-f]{40}$")

EXPECTED_ACTIONS = {
    "actions/checkout": ("3d3c42e5aac5ba805825da76410c181273ba90b1", "v7.0.1"),
    "actions/setup-node": ("820762786026740c76f36085b0efc47a31fe5020", "v7.0.0"),
    "actions/setup-python": ("5fda3b95a4ea91299a34e894583c3862153e4b97", "v7.0.0"),
    "actions/cache": ("55cc8345863c7cc4c66a329aec7e433d2d1c52a9", "v6.1.0"),
    "actions/upload-artifact": (
        "043fb46d1a93c77aae656e7c1c64a875d1fc6a0a",
        "v7.0.1",
    ),
    "actions/download-artifact": (
        "3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c",
        "v8.0.1",
    ),
    "dtolnay/rust-toolchain": (
        "4360b52568e2003a75bf9bc1d59f33a8e3fc893c",
        "stable (2026-08-05)",
    ),
}

# 各 Action 在 build.yml 中的使用次数：三平台 job 各一整套，
# check-release-tag 与 release job 各含一次 checkout，release job 另含一次 download-artifact
EXPECTED_COUNTS = {
    "actions/checkout": 5,
    "actions/setup-node": 3,
    "actions/setup-python": 3,
    "actions/cache": 3,
    "actions/upload-artifact": 3,
    "actions/download-artifact": 1,
    "dtolnay/rust-toolchain": 3,
}


class CiSupplyChainPinsTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.workflow = WORKFLOW.read_text(encoding="utf-8")
        cls.uses = re.findall(
            r"^\s*-?\s*uses:\s*([^@\s]+)@([^\s#]+)(?:\s+#\s*(.+))?$",
            cls.workflow,
            flags=re.MULTILINE,
        )

    def test_every_action_is_pinned_to_the_expected_commit(self) -> None:
        self.assertEqual(len(self.uses), sum(EXPECTED_COUNTS.values()))
        counts = Counter(action for action, _revision, _comment in self.uses)
        self.assertEqual(counts, Counter(EXPECTED_COUNTS))
        self.assertEqual(set(counts), set(EXPECTED_ACTIONS))

        for action, revision, comment in self.uses:
            with self.subTest(action=action):
                self.assertRegex(revision, FULL_SHA)
                expected_revision, expected_comment = EXPECTED_ACTIONS[action]
                self.assertEqual(revision, expected_revision)
                self.assertEqual(comment, expected_comment)

    def test_workflow_has_no_floating_action_or_toolchain_reference(self) -> None:
        self.assertNotRegex(self.workflow, r"uses:\s*[^\n#]+@(v\d+|stable)\b")
        self.assertEqual(self.workflow.count("node-version: 20.20.2"), 3)
        self.assertEqual(self.workflow.count("toolchain: 1.88.0"), 3)
        self.assertNotRegex(self.workflow, r"toolchain:\s*(stable|beta|nightly)\b")
        self.assertEqual(self.workflow.count("python-version: 3.13.5"), 3)

    def test_checkout_credentials_and_workflow_permissions_are_minimal(self) -> None:
        self.assertRegex(
            self.workflow,
            r"(?m)^permissions:\n  contents: read\n",
        )
        # 每个 checkout 的 with 块（其后 1~4 行）都必须显式关闭凭证持久化
        checkout_blocks = re.findall(
            r"(?ms)^\s*- uses: actions/checkout@[^\n]+\n(?:\s+[^\n]*\n){1,4}",
            self.workflow,
        )
        self.assertEqual(len(checkout_blocks), EXPECTED_COUNTS["actions/checkout"])
        for block in checkout_blocks:
            self.assertIn("persist-credentials: false", block)

    def test_existing_platform_targets_remain_configured(self) -> None:
        for target in (
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "x86_64-pc-windows-msvc",
        ):
            with self.subTest(target=target):
                self.assertEqual(self.workflow.count(f"targets: {target}"), 1)

        self.assertNotIn("brew install zstd", self.workflow)


if __name__ == "__main__":
    unittest.main()
