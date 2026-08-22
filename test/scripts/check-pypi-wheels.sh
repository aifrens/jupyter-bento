#!/usr/bin/env bash
set -euo pipefail

TEST_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
platform_name="${PYPI_PLATFORM:-macosx_12_0_arm64}"
python_version="${PYTHON_VERSION:-39}"
WORK_ROOT="$TEST_ROOT/work/pypi-wheel-check/$platform_name"
WORK_ROOT="${WHEEL_WORK_ROOT:-$WORK_ROOT}"
REQUIREMENTS_FILE="${REQUIREMENTS_FILE:-$TEST_ROOT/requirements-direct.txt}"
mkdir -p "$WORK_ROOT/logs" "$TEST_ROOT/work/tmp" "$TEST_ROOT/work/pip-cache"

export TMPDIR="$TEST_ROOT/work/tmp"
export PIP_CACHE_DIR="$TEST_ROOT/work/pip-cache"
export PIP_DISABLE_PIP_VERSION_CHECK=1

REPORT="$WORK_ROOT/report.tsv"
printf 'package\tversion\tplatform\tstatus\tdetail\n' > "$REPORT"

while IFS= read -r spec; do
  [[ -z "$spec" || "$spec" == \#* ]] && continue
  name="${spec%%==*}"
  version="${spec##*==}"
  log="$WORK_ROOT/logs/${name//-/_}-${version}.log"
  package_dir="$WORK_ROOT/wheels/${name//-/_}-${version}"
  mkdir -p "$package_dir"
  if python3 -m pip download \
      --no-deps \
      --no-cache-dir \
      --only-binary=:all: \
      --platform "$platform_name" \
      --python-version "$python_version" \
      --implementation cp \
      --abi cp39 \
      --index-url https://pypi.org/simple \
      --dest "$package_dir" \
      "$spec" >"$log" 2>&1; then
    detail="$(tail -n 1 "$log" | tr '\t' ' ' | tr '\n' ' ')"
    status="PASS"
  else
    detail="$(tail -n 1 "$log" | tr '\t' ' ' | tr '\n' ' ')"
    status="MISSING"
  fi
  printf '%s\t%s\t%s\t%s\t%s\n' "$name" "$version" "$platform_name" "$status" "$detail" >> "$REPORT"
done < "$REQUIREMENTS_FILE"

printf '%s\n' "PyPI wheel report: $REPORT"
sed -n '1,40p' "$REPORT"
