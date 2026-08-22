#!/usr/bin/env python3
"""Validate user-package installation and golden-runtime reset semantics."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
from pathlib import Path


def run(command: list[str], *, env: dict[str, str]) -> str:
    completed = subprocess.run(
        command,
        check=True,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    return completed.stdout


def remove_path(path: Path) -> None:
    if path.is_symlink() or path.is_file():
        path.unlink()
    elif path.exists():
        shutil.rmtree(path)


def copy_runtime(source: Path, destination: Path) -> None:
    remove_path(destination)
    shutil.copytree(source, destination, symlinks=True)


def replace_runtime(base: Path, current: Path) -> None:
    staging = current.with_name(current.name + ".staging")
    old = current.with_name(current.name + ".old")
    remove_path(staging)
    remove_path(old)
    copy_runtime(base, staging)
    if current.exists() or current.is_symlink():
        current.rename(old)
    staging.rename(current)
    remove_path(old)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source-env", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument(
        "--index-url",
        default="https://mirrors.aliyun.com/pypi/simple/",
    )
    args = parser.parse_args()

    source = args.source_env.resolve()
    root = args.root.resolve()
    golden = root / "golden"
    current = root / "current"
    root.mkdir(parents=True, exist_ok=True)
    remove_path(golden)
    remove_path(current)
    copy_runtime(source, golden)
    copy_runtime(golden, current)

    env = os.environ.copy()
    env.update(
        {
            "PYTHONNOUSERSITE": "1",
            "PIP_DISABLE_PIP_VERSION_CHECK": "1",
            "PIP_CACHE_DIR": str(root / "pip-cache"),
            "TMPDIR": str(root / "tmp"),
        }
    )
    (root / "pip-cache").mkdir(exist_ok=True)
    (root / "tmp").mkdir(exist_ok=True)

    python = current / "bin/python"
    before_numpy = run(
        [str(python), "-c", "import importlib.metadata as m; print(m.version('numpy'))"],
        env=env,
    ).strip()
    run(
        [
            str(python),
            "-m",
            "pip",
            "install",
            "--isolated",
            "--no-cache-dir",
            "--only-binary=:all:",
            "--index-url",
            args.index_url,
            "tomli==2.0.1",
        ],
        env=env,
    )
    user_version = run(
        [str(python), "-c", "import tomli; print(tomli.__version__)"], env=env
    ).strip()
    run([str(python), "-m", "pip", "check"], env=env)

    replace_runtime(golden, current)
    python = current / "bin/python"
    after_numpy = run(
        [str(python), "-c", "import importlib.metadata as m; print(m.version('numpy'))"],
        env=env,
    ).strip()
    tomli_present = subprocess.run(
        [str(python), "-c", "import tomli"],
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    ).returncode == 0
    run([str(python), "-m", "pip", "check"], env=env)
    if tomli_present:
        raise AssertionError("reset retained the user-installed tomli package")
    if before_numpy != after_numpy:
        raise AssertionError(f"reset changed numpy: {before_numpy} -> {after_numpy}")

    report = {
        "source": str(source),
        "runtime": str(current),
        "python": run(
            [str(python), "-c", "import platform; print(platform.python_version())"],
            env=env,
        ).strip(),
        "user_install": "tomli==2.0.1",
        "user_install_version": user_version,
        "index_url": args.index_url,
        "reset": "copy golden to staging, then atomically swap directories",
        "numpy_before": before_numpy,
        "numpy_after": after_numpy,
        "tomli_after_reset": tomli_present,
        "pip_check_after_reset": "passed",
    }
    report_path = root / "user-install-reset.json"
    report_path.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
