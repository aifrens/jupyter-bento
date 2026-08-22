#!/usr/bin/env python3
"""Exercise the reset algorithm with a local runtime fixture."""

from __future__ import annotations

import argparse
import json
import shutil
from pathlib import Path


def replace_from_base(base: Path, current: Path) -> None:
    staging = current.with_name(current.name + ".staging")
    old = current.with_name(current.name + ".old")
    shutil.rmtree(staging, ignore_errors=True)
    shutil.rmtree(old, ignore_errors=True)
    shutil.copytree(base, staging)
    if current.exists():
        current.rename(old)
    staging.rename(current)
    shutil.rmtree(old, ignore_errors=True)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=True)
    base = root / "base"
    current = root / "current"
    shutil.rmtree(base, ignore_errors=True)
    shutil.rmtree(current, ignore_errors=True)
    base.mkdir()
    (base / "manifest.json").write_text(
        json.dumps({"python": "3.9.7", "revision": "initial"})
    )
    replace_from_base(base, current)
    (current / "user-package.marker").write_text("installed-by-user")
    replace_from_base(base, current)
    if (current / "user-package.marker").exists():
        raise AssertionError("reset retained a user file")
    if json.loads((current / "manifest.json").read_text())["revision"] != "initial":
        raise AssertionError("reset did not restore the base manifest")
    print(f"Reset fixture OK: {current}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
