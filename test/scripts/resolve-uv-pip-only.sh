#!/usr/bin/env bash
set -euo pipefail

TEST_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
UV_BIN="${UV_BIN:-$TEST_ROOT/work/tools/uv/bin/uv}"
REQUIREMENTS="${REQUIREMENTS_FILE:-$TEST_ROOT/requirements-pip-only-candidate.txt}"
WORK_ROOT="${PIP_ONLY_WORK_ROOT:-$TEST_ROOT/work/pip-only}"
INDEX_URL="${PIP_INDEX_URL_OVERRIDE:-https://pypi.org/simple}"
PYTHON_VERSION="${PYTHON_VERSION_OVERRIDE:-3.9.7}"

if [[ ! -x "$UV_BIN" ]]; then
  printf 'Missing uv executable: %s\n' "$UV_BIN" >&2
  exit 2
fi

mkdir -p "$WORK_ROOT/uv-locks" "$WORK_ROOT/uv-logs" "$WORK_ROOT/uv-cache" "$WORK_ROOT/tmp"
export UV_CACHE_DIR="$WORK_ROOT/uv-cache"
export TMPDIR="$WORK_ROOT/tmp"
export UV_NO_PROGRESS=1

overall=0
for spec in \
  'macos-arm64:aarch64-apple-darwin' \
  'macos-intel:x86_64-apple-darwin' \
  'windows-x64:x86_64-pc-windows-msvc'; do
  label="${spec%%:*}"
  platform="${spec##*:}"
  output="$WORK_ROOT/uv-locks/$label.txt"
  log="$WORK_ROOT/uv-logs/$label.log"
  printf '[%s] resolving for %s\n' "$label" "$platform"
  if "$UV_BIN" pip compile \
      "$REQUIREMENTS" \
      --python-version "$PYTHON_VERSION" \
      --python-platform "$platform" \
      --only-binary=:all: \
      --no-python-downloads \
      --no-annotate \
      --no-header \
      --default-index "$INDEX_URL" \
      --output-file "$output" >"$log" 2>&1; then
    count="$(awk 'NF && $1 !~ /^#/ { n++ } END { print n + 0 }' "$output")"
    printf 'PASS\t%s\t%s packages\t%s\n' "$label" "$count" "$output"
  else
    printf 'FAIL\t%s\t%s\n' "$label" "$log" >&2
    overall=1
  fi
done

exit "$overall"
