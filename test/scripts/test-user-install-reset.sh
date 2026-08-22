#!/usr/bin/env bash
set -euo pipefail

TEST_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK_ROOT="$TEST_ROOT/work"
SOURCE_ENV="$WORK_ROOT/conda/envs/jupyter39-fixed"
RUNTIME_ROOT="$WORK_ROOT/runtime"
GOLDEN_ENV="$RUNTIME_ROOT/golden"
CURRENT_ENV="$RUNTIME_ROOT/current"
STAGING_ENV="$RUNTIME_ROOT/current.staging"
OLD_ENV="$RUNTIME_ROOT/current.old"
REPORT="$RUNTIME_ROOT/user-install-reset.json"

export CONDA_NO_PLUGINS=false
export CONDA_SOLVER=libmamba
export CONDARC="$TEST_ROOT/condarc"
export CONDA_PKGS_DIRS="$WORK_ROOT/conda/pkgs"
export CONDA_ENVS_PATH="$WORK_ROOT/conda/envs"
export TMPDIR="$WORK_ROOT/tmp"
export PIP_CACHE_DIR="$WORK_ROOT/pip-cache"
export PIP_DISABLE_PIP_VERSION_CHECK=1
mkdir -p "$RUNTIME_ROOT" "$WORK_ROOT/tmp" "$WORK_ROOT/pip-cache"

if [[ ! -x "$SOURCE_ENV/bin/python" ]]; then
  printf '%s\n' "Missing source environment: $SOURCE_ENV" >&2
  exit 1
fi

if [[ ! -x "$GOLDEN_ENV/bin/python" ]]; then
  /opt/homebrew/bin/conda create --yes --copy --prefix "$GOLDEN_ENV" --clone "$SOURCE_ENV" \
    2>&1 | tee "$RUNTIME_ROOT/clone-golden.log"
fi

if [[ ! -x "$CURRENT_ENV/bin/python" ]]; then
  /opt/homebrew/bin/conda create --yes --copy --prefix "$CURRENT_ENV" --clone "$GOLDEN_ENV" \
    2>&1 | tee "$RUNTIME_ROOT/clone-current.log"
fi

before_version="$($CURRENT_ENV/bin/python -c 'import importlib.metadata as m; print(m.version("numpy"))')"
if $CURRENT_ENV/bin/python -c 'import importlib.metadata as m; m.version("tomli")' >/dev/null 2>&1; then
  printf '%s\n' 'tomli is already present; use a fresh runtime directory for a clean run.' >&2
  exit 1
fi

$CURRENT_ENV/bin/python -m pip install \
  --isolated \
  --no-cache-dir \
  --only-binary=:all: \
  --index-url https://mirrors.aliyun.com/pypi/simple/ \
  tomli==2.0.1 \
  2>&1 | tee "$RUNTIME_ROOT/user-install.log"
$CURRENT_ENV/bin/python -m pip check > "$RUNTIME_ROOT/user-pip-check.log"
$CURRENT_ENV/bin/python -c 'import tomli, sys; print("user package OK", tomli.__version__, sys.executable)'

rm -rf "$STAGING_ENV" "$OLD_ENV"
/opt/homebrew/bin/conda create --yes --copy --prefix "$STAGING_ENV" --clone "$GOLDEN_ENV" \
  2>&1 | tee "$RUNTIME_ROOT/reset-clone.log"
mv "$CURRENT_ENV" "$OLD_ENV"
mv "$STAGING_ENV" "$CURRENT_ENV"
rm -rf "$OLD_ENV"

after_version="$($CURRENT_ENV/bin/python -c 'import importlib.metadata as m; print(m.version("numpy"))')"
if $CURRENT_ENV/bin/python -c 'import importlib.metadata as m; m.version("tomli")' >/dev/null 2>&1; then
  printf '%s\n' 'Reset failed: tomli is still present.' >&2
  exit 1
fi
if [[ "$before_version" != "$after_version" ]]; then
  printf '%s\n' "Reset changed numpy: $before_version -> $after_version" >&2
  exit 1
fi
$CURRENT_ENV/bin/python -m pip check > "$RUNTIME_ROOT/reset-pip-check.log"

python3 - "$REPORT" "$CURRENT_ENV" "$before_version" "$after_version" <<'PY'
import json, pathlib, sys
out, env, before, after = sys.argv[1:]
pathlib.Path(out).write_text(json.dumps({
    "source": "conda-forge locked environment",
    "user_install": "tomli==2.0.1",
    "index_url": "https://mirrors.aliyun.com/pypi/simple/",
    "reset": "clone golden environment and atomic directory swap",
    "runtime": env,
    "numpy_before": before,
    "numpy_after": after,
    "tomli_after_reset": False,
}, indent=2) + "\n")
PY
printf '%s\n' "User install and reset OK: $REPORT"
