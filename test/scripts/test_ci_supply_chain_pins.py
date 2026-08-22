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
    "actions/checkout": ("11d5960a326750d5838078e36cf38b85af677262", "v4.4.0"),
    "actions/setup-node": ("49933ea5288caeca8642d1e84afbd3f7d6820020", "v4.4.0"),
    "actions/setup-python": ("ece7cb06caefa5fff74198d8649806c4678c61a1", "v6.3.0"),
    "actions/upload-artifact": (
        "ea165f8d65b6e75b540449e92b4886f43607fa02",
        "v4.6.2",
    ),
    "dtolnay/rust-toolchain": (
        "4360b52568e2003a75bf9bc1d59f33a8e3fc893c",
        "stable (2026-08-05)",
    ),
}


class CiSupplyChainPinsTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.workflow = WORKFLOW.read_text(encoding="utf-8")
        cls.uses = re.findall(
            r"^\s*- uses:\s*([^@\s]+)@([^\s#]+)(?:\s+#\s*(.+))?$",
            cls.workflow,
            flags=re.MULTILINE,
        )

    def test_every_action_is_pinned_to_the_expected_commit(self) -> None:
        self.assertEqual(len(self.uses), 15)
        counts = Counter(action for action, _revision, _comment in self.uses)
        self.assertEqual(counts, Counter({action: 3 for action in EXPECTED_ACTIONS}))

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
        checkout_blocks = re.findall(
            r"(?ms)^\s*- uses: actions/checkout@[^\n]+\n"
            r"\s+with:\n"
            r"\s+persist-credentials: false\s*$",
            self.workflow,
        )
        self.assertEqual(len(checkout_blocks), 3)

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
