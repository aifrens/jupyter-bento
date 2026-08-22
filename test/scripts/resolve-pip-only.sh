#!/usr/bin/env bash
set -euo pipefail

TEST_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REQUIREMENTS="${REQUIREMENTS_FILE:-$TEST_ROOT/requirements-pip-only-candidate.txt}"
WORK_ROOT="${PIP_ONLY_WORK_ROOT:-$TEST_ROOT/work/pip-only}"
INDEX_URL="${PIP_INDEX_URL_OVERRIDE:-https://pypi.org/simple}"
PYTHON_VERSION="${PYTHON_VERSION_OVERRIDE:-3.9.7}"

mkdir -p "$WORK_ROOT/reports" "$WORK_ROOT/logs" "$WORK_ROOT/cache" "$WORK_ROOT/tmp"
export TMPDIR="$WORK_ROOT/tmp"
export PIP_CACHE_DIR="$WORK_ROOT/cache"
export PIP_DISABLE_PIP_VERSION_CHECK=1

printf 'candidate=%s\n' "$REQUIREMENTS"
printf 'index=%s\n' "$INDEX_URL"
printf 'python=%s\n' "$PYTHON_VERSION"

overall=0
for platform_name in macosx_12_0_arm64 macosx_12_0_x86_64 win_amd64; do
  report="$WORK_ROOT/reports/$platform_name.json"
  log="$WORK_ROOT/logs/$platform_name.log"
  printf '\n[%s] resolving\n' "$platform_name"
  if python3 -m pip install \
      --isolated \
      --dry-run \
      --ignore-installed \
      --report "$report" \
      --only-binary=:all: \
      --platform "$platform_name" \
      --python-version "$PYTHON_VERSION" \
      --implementation cp \
      --abi cp39 \
      --index-url "$INDEX_URL" \
      --disable-pip-version-check \
      --requirement "$REQUIREMENTS" >"$log" 2>&1; then
    printf 'PASS\t%s\t%s\n' "$platform_name" "$report"
  else
    printf 'FAIL\t%s\t%s\n' "$platform_name" "$log"
    overall=1
  fi
done

exit "$overall"
