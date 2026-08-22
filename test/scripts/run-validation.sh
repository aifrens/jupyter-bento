#!/usr/bin/env bash
set -euo pipefail

TEST_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK_ROOT="$TEST_ROOT/work"
ENV_ROOT="$WORK_ROOT/conda/envs/jupyter39-fixed"
LOG_ROOT="$WORK_ROOT/logs"
mkdir -p "$WORK_ROOT" "$LOG_ROOT" "$WORK_ROOT/tmp" "$WORK_ROOT/pip-cache" "$WORK_ROOT/xdg-cache"

export TMPDIR="$WORK_ROOT/tmp"
export XDG_CACHE_HOME="$WORK_ROOT/xdg-cache"
export PIP_CACHE_DIR="$WORK_ROOT/pip-cache"
export PIP_DISABLE_PIP_VERSION_CHECK=1
export CONDA_NO_PLUGINS=false
export CONDARC="$TEST_ROOT/condarc"
export CONDA_PKGS_DIRS="$WORK_ROOT/conda/pkgs"
export CONDA_ENVS_PATH="$WORK_ROOT/conda/envs"
export CONDA_BLD_PATH="$WORK_ROOT/conda/conda-bld"
export CONDA_NOTICES=false
export CONDA_SOLVER=libmamba
mkdir -p "$CONDA_PKGS_DIRS" "$CONDA_ENVS_PATH" "$CONDA_BLD_PATH"

python3 - <<'PY' > "$WORK_ROOT/host-before.json"
import json, os, platform, sys
print(json.dumps({"executable": sys.executable, "version": sys.version, "machine": platform.machine(), "path": os.environ.get("PATH", "")}, indent=2))
PY

if [[ "${SKIP_WHEEL_CHECK:-0}" != "1" ]]; then
  printf '%s\n' '[1/4] Checking PyPI wheel coverage'
  PYPI_PLATFORM="${PYPI_PLATFORM:-macosx_12_0_arm64}" \
    PYTHON_VERSION="${PYTHON_VERSION:-39}" \
    "$TEST_ROOT/scripts/check-pypi-wheels.sh"
else
  printf '%s\n' '[1/4] Skipping PyPI wheel coverage (SKIP_WHEEL_CHECK=1)'
fi

if [[ "${SKIP_CONDA:-0}" != "1" ]]; then
  printf '%s\n' '[2/4] Creating or reusing isolated native conda environment'
  if [[ ! -x "$ENV_ROOT/bin/python" ]]; then
    /opt/homebrew/bin/conda create \
    --yes \
    --override-channels \
    --channel conda-forge \
    --prefix "$ENV_ROOT" \
    python=3.9.7 \
    pip \
    pandas=1.3.4 \
    numpy=1.22.4 \
    scipy=1.7.1 \
    matplotlib=3.4.3 \
    seaborn=0.11.2 \
    openpyxl=3.0.9 \
    xlrd=2.0.1 \
    pillow=8.4.0 \
    scikit-learn=0.24.2 \
    imbalanced-learn=0.8.1 \
    notebook=6.4.8 \
    traitlets=5.1.1 \
    matplotlib-inline=0.1.3 \
    ipykernel=6.9.1 \
    nbclient=0.5.13 \
    jupyter_client=7.1.2 \
    jupyter_core=4.9.1 \
    nbconvert=6.4.5 \
    nbformat=5.1.3 \
    tornado=6.1 \
    coloredlogs=15.0.1 \
    flatbuffers=2.0.7 \
    protobuf=3.20.3 \
    sympy=1.9 \
    2>&1 | tee "$LOG_ROOT/conda-create.log"
  else
    printf '%s\n' "Reusing existing environment: $ENV_ROOT"
  fi

  printf '%s\n' '[3/4] Installing PyPI-only native packages into the isolated environment'
  "$ENV_ROOT/bin/python" -m pip install \
    --isolated \
    --no-cache-dir \
    --no-deps \
    --only-binary=:all: \
    --index-url https://pypi.org/simple \
    opencv-python==4.10.0.84 \
    xgboost==2.1.1 \
    onnxruntime==1.12.1 \
    flatbuffers==2.0.7 \
    2>&1 | tee "$LOG_ROOT/pip-native.log"

  "$ENV_ROOT/bin/python" -m pip check 2>&1 | tee "$LOG_ROOT/pip-check.log"
  "$ENV_ROOT/bin/python" "$TEST_ROOT/scripts/smoke-runtime.py" \
    --env "$ENV_ROOT" \
    --requirements "$TEST_ROOT/requirements-direct.txt" \
    --output "$WORK_ROOT/smoke.json"

  "$ENV_ROOT/bin/python" "$TEST_ROOT/scripts/test-jupyter-server.py" \
    --env "$ENV_ROOT" \
    --root "$WORK_ROOT/notebooks"
else
  printf '%s\n' '[2/4] Skipping conda environment (SKIP_CONDA=1)'
  printf '%s\n' '[3/4] Skipping runtime install and smoke tests'
fi

printf '%s\n' '[4/4] Testing reset semantics and recording isolation metadata'
python3 "$TEST_ROOT/scripts/test-reset.py" --root "$WORK_ROOT/reset-fixture"
python3 - <<'PY' > "$WORK_ROOT/host-after.json"
import json, os, platform, sys
print(json.dumps({"executable": sys.executable, "version": sys.version, "machine": platform.machine(), "path": os.environ.get("PATH", "")}, indent=2))
PY
printf '%s\n' "Validation artifacts are under $WORK_ROOT"
