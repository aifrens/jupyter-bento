#!/usr/bin/env python3
"""Record that the host Python does not see the app-only user package."""

from __future__ import annotations

import importlib.util
import json
import platform
import subprocess
import sys
from pathlib import Path


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    current = root / "work" / "runtime" / "current"
    host_spec = importlib.util.find_spec("tomli")
    result = {
        "host_python": sys.executable,
        "host_version": platform.python_version(),
        "host_prefix": sys.prefix,
        "host_tomli_visible": host_spec is not None,
        "host_tomli_location": str(host_spec.origin) if host_spec else None,
        "current_runtime": str(current),
        "current_python": str((current / "bin" / "python").resolve()),
        "current_runtime_is_distinct": Path(sys.executable).resolve() != (current / "bin" / "python").resolve(),
    }
    if host_spec is not None:
        raise SystemExit("host Python unexpectedly sees tomli")
    output = root / "work" / "host-isolation.json"
    output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
