#!/usr/bin/env python3
"""Validate the exact runtime versions and import the native modules."""

from __future__ import annotations

import argparse
import importlib
import importlib.metadata
import json
import platform
import sys
from pathlib import Path


DIST_TO_MODULE = {
    "pandas": "pandas",
    "numpy": "numpy",
    "scipy": "scipy",
    "matplotlib": "matplotlib",
    "seaborn": "seaborn",
    "openpyxl": "openpyxl",
    "xlrd": "xlrd",
    "Pillow": "PIL",
    "opencv-python": "cv2",
    "scikit-learn": "sklearn",
    "xgboost": "xgboost",
    "imbalanced-learn": "imblearn",
    "onnxruntime": "onnxruntime",
    "notebook": "notebook",
    "traitlets": "traitlets",
    "matplotlib-inline": "matplotlib_inline",
}


def read_requirements(path: Path) -> dict[str, str]:
    result: dict[str, str] = {}
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        name, version = line.split("==", 1)
        result[name] = version
    return result


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--env", type=Path, required=True)
    parser.add_argument("--requirements", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()

    expected = read_requirements(args.requirements)
    results: dict[str, object] = {
        "python": platform.python_version(),
        "executable": str(Path(sys.executable).resolve()),
        "machine": platform.machine(),
        "prefix": str(Path(sys.prefix).resolve()),
        "packages": {},
        "errors": [],
    }

    env_path = args.env.resolve()
    # A venv may intentionally symlink its interpreter to the base runtime;
    # sys.prefix is the stable boundary for both venvs and embedded runtimes.
    if Path(sys.prefix).resolve() != env_path:
        results["errors"].append(
            f"sys.prefix is outside the requested environment: {sys.prefix}"
        )
    if sys.version_info[:3] != (3, 9, 7):
        results["errors"].append(f"expected Python 3.9.7, got {platform.python_version()}")

    for dist_name, expected_version in expected.items():
        item: dict[str, object] = {"expected": expected_version}
        try:
            actual = importlib.metadata.version(dist_name)
            item["installed"] = actual
            if actual != expected_version:
                results["errors"].append(
                    f"{dist_name}: expected {expected_version}, got {actual}"
                )
        except importlib.metadata.PackageNotFoundError:
            item["installed"] = None
            results["errors"].append(f"{dist_name}: distribution not installed")
            results["packages"][dist_name] = item
            continue

        module_name = DIST_TO_MODULE[dist_name]
        try:
            module = importlib.import_module(module_name)
            item["module"] = getattr(module, "__version__", "imported")
        except Exception as exc:  # import diagnostics are part of this check
            item["import_error"] = f"{type(exc).__name__}: {exc}"
            results["errors"].append(f"{dist_name}: import failed: {exc}")
        results["packages"][dist_name] = item

    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(results, indent=2, sort_keys=True) + "\n")
    print(json.dumps(results, indent=2, sort_keys=True))
    return 1 if results["errors"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
