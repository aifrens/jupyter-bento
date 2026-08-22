#!/usr/bin/env bash
set -euo pipefail

TEST_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$TEST_ROOT"
for platform_name in macosx_12_0_arm64 macosx_12_0_x86_64 win_amd64; do
  PYPI_PLATFORM="$platform_name" PYTHON_VERSION=39 ./scripts/check-pypi-wheels.sh
done
