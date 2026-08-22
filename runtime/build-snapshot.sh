#!/bin/bash
# 出厂环境快照构建脚本（macOS / Linux 下运行）
# 用法: build-snapshot.sh <macos-arm64|macos-x64|win-x64> [输出目录] [--fast]
# 产物: env-factory.tar.zst （python-build-standalone 3.9.7 + 锁定依赖预装）
#   --fast / -f ：本地快速模式。跳过 SHA-256 校验与依赖哈希锁定，启用 pip 与
#                 Python 归档缓存（大幅加速重复构建）。仅限本地使用，CI 请勿开启。
set -euo pipefail

TARGET=""
OUT_ARG=""
FAST=0
for a in "$@"; do
  case "$a" in
    --fast|-f) FAST=1 ;;
    -*) echo "未知参数: $a" >&2; exit 1 ;;
    *) if [ -z "$TARGET" ]; then TARGET="$a"; elif [ -z "$OUT_ARG" ]; then OUT_ARG="$a"; else echo "多余参数: $a" >&2; exit 1; fi ;;
  esac
done
[ "${JUPITER_FAST_BUILD:-}" = "1" ] && FAST=1
[ -n "$TARGET" ] || { echo "用法: build-snapshot.sh <macos-arm64|macos-x64|win-x64> [输出目录] [--fast]" >&2; exit 1; }

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
OUT_DIR="${OUT_ARG:-$ROOT/app/src-tauri/resources}"
MIRROR="${JUPITER_PYPI_MIRROR:-https://mirrors.aliyun.com/pypi/simple/}"
# 网络兜底：跨国下载大 wheel 时防 ReadTimeout（CI 在美国机房访问国内镜像时常见）
export PIP_DEFAULT_TIMEOUT=120
export PIP_RETRIES=10
BOOTSTRAP_PYTHON="${BOOTSTRAP_PYTHON:-python3}"
BOOTSTRAP_VERSION="3.13.5"

PBS_TAG="20211017"
PBS_TS="20211017T1616"
PBS_CHECKSUMS="$SCRIPT_DIR/python-build-standalone-20211017.sha256"
PBS_CACHE="$SCRIPT_DIR/.cache"

if [ "$FAST" = "1" ]; then
  echo "⚠️  快速模式：跳过 SHA-256 校验与依赖哈希锁定，启用缓存（仅限本地构建）"
fi

case "$TARGET" in
  macos-arm64)
    ASSET="cpython-3.9.7-aarch64-apple-darwin-pgo+lto-$PBS_TS.tar.zst"
    REQ="requirements-macos-arm64.lock.txt"; PIP_PLATFORM="macosx_12_0_arm64"
    PYBIN="python/install/bin/python3"; RUN=() ;;
  macos-x64)
    ASSET="cpython-3.9.7-x86_64-apple-darwin-pgo+lto-$PBS_TS.tar.zst"
    REQ="requirements-macos-x64.lock.txt"; PIP_PLATFORM="macosx_12_0_x86_64"
    PYBIN="python/install/bin/python3"; RUN=(arch -x86_64) ;;
  win-x64)
    ASSET="cpython-3.9.7-x86_64-pc-windows-msvc-shared-pgo-$PBS_TS.tar.zst"
    REQ="requirements-win-x64.lock.txt"; PIP_PLATFORM="win_amd64"
    PYBIN="python/install/python.exe";  RUN=() ;;
  *) echo "未知目标: $TARGET" >&2; exit 1 ;;
esac

case "$(uname -s)" in
  Darwin) BOOTSTRAP_LOCK="$SCRIPT_DIR/requirements-bootstrap-macos.lock.txt" ;;
  Linux)
    [ "$(uname -m)" = "x86_64" ] || {
      echo "Linux 构建工具哈希锁当前仅支持 x86_64" >&2
      exit 1
    }
    BOOTSTRAP_LOCK="$SCRIPT_DIR/requirements-bootstrap-linux-x64.lock.txt"
    ;;
  *) echo "不支持的构建宿主: $(uname -s)" >&2; exit 1 ;;
esac

URL="https://github.com/astral-sh/python-build-standalone/releases/download/$PBS_TAG/$ASSET"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# 快速模式：剥离 lock 中的哈希（pip 仅在 --require-hashes 模式下接受 --hash 条目），
# 并启用 pip 缓存（重复构建时数百 MB 的 wheel 不再重新下载）。
REQ_FILE="$SCRIPT_DIR/$REQ"
PIP_HASH_FLAGS="--require-hashes --only-binary=:all: --no-deps --no-cache-dir"
if [ "$FAST" = "1" ]; then
  REQ_FILE="$WORK/requirements-fast.txt"
  sed 's/[[:space:]]*--hash=sha256:[0-9a-fA-F]*//g' "$SCRIPT_DIR/$REQ" > "$REQ_FILE"
  PIP_HASH_FLAGS="--only-binary=:all: --no-deps"
fi

# 带本地缓存的 pbs 归档下载（仅快速模式使用缓存；严格模式始终下载并校验）
download_pbs() {
  asset="$1"; dest="$2"
  if [ "$FAST" = "1" ] && [ -f "$PBS_CACHE/$asset" ]; then
    echo "==> 使用本地缓存: $asset"
    cp "$PBS_CACHE/$asset" "$dest"
  else
    curl -fL --retry 3 --connect-timeout 30 -o "$dest" \
      "https://github.com/astral-sh/python-build-standalone/releases/download/$PBS_TAG/$asset"
    if [ "$FAST" = "1" ]; then mkdir -p "$PBS_CACHE"; cp "$dest" "$PBS_CACHE/$asset"; fi
  fi
}

# 构建工具也会处理并产出可执行内容，因此本地和 CI 都必须使用同一精确
# Python 版本及哈希锁；不允许悄悄回退到宿主的其他 python3 / zstd。
actual_bootstrap_version="$($BOOTSTRAP_PYTHON -c 'import platform; print(platform.python_version())')" || {
  echo "无法执行构建 Python: $BOOTSTRAP_PYTHON" >&2
  exit 1
}
if [ "$actual_bootstrap_version" != "$BOOTSTRAP_VERSION" ]; then
  echo "构建 Python 版本不匹配：需要 $BOOTSTRAP_VERSION，实际 $actual_bootstrap_version ($BOOTSTRAP_PYTHON)" >&2
  exit 1
fi
[ -f "$BOOTSTRAP_LOCK" ] || { echo "缺少构建工具哈希锁: $BOOTSTRAP_LOCK" >&2; exit 1; }
[ -f "$SCRIPT_DIR/$REQ" ] || { echo "缺少运行时依赖哈希锁: $SCRIPT_DIR/$REQ" >&2; exit 1; }

"$BOOTSTRAP_PYTHON" -m venv "$WORK/bootstrap"
BOOTSTRAP_PY="$WORK/bootstrap/bin/python"
"$BOOTSTRAP_PY" -m pip install -q --disable-pip-version-check \
  --require-hashes --only-binary=:all: --no-deps --no-cache-dir \
  --index-url "$MIRROR" -r "$BOOTSTRAP_LOCK"

# 供应链完整性边界：下载的 Python 会在后续构建中被直接执行，必须在解压前
# 与仓库内固定的 SHA-256 匹配。摘要缺失、工具不可用或不匹配均终止构建。
expected_sha256() {
  asset_name="$1"
  [ -f "$PBS_CHECKSUMS" ] || {
    echo "缺少 Python 归档校验清单: $PBS_CHECKSUMS" >&2
    return 1
  }
  checksum="$(awk -v asset="$asset_name" '
    $0 ~ /^[[:space:]]*(#|$)/ { next }
    NF == 2 && $2 == asset { count++; value = $1 }
    END {
      if (count == 1) print value
      else exit 1
    }
  ' "$PBS_CHECKSUMS")" || {
    echo "校验清单中必须有且仅有一个 SHA-256: $asset_name" >&2
    return 1
  }
  case "$checksum" in
    *[!0-9a-fA-F]*|'')
      echo "校验清单中缺少或包含无效 SHA-256: $asset_name" >&2
      return 1
      ;;
  esac
  [ "${#checksum}" -eq 64 ] || {
    echo "校验清单中包含无效 SHA-256: $asset_name" >&2
    return 1
  }
  printf '%s\n' "$checksum" | tr 'A-F' 'a-f'
}

file_sha256() {
  archive_path="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$archive_path" | awk '{ print $1 }'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$archive_path" | awk '{ print $1 }'
  else
    echo "无法校验 Python 归档：未找到 sha256sum 或 shasum" >&2
    return 1
  fi
}

verify_archive() {
  archive_path="$1"
  asset_name="$2"
  if [ "$FAST" = "1" ]; then
    echo "==> 快速模式：跳过 SHA-256 校验 ($asset_name)"
    return 0
  fi
  expected="$(expected_sha256 "$asset_name")" || return 1
  actual="$(file_sha256 "$archive_path")" || return 1
  actual="$(printf '%s\n' "$actual" | tr 'A-F' 'a-f')"
  if [ "$actual" != "$expected" ]; then
    echo "Python 归档 SHA-256 校验失败: $asset_name" >&2
    echo "  期望: $expected" >&2
    echo "  实际: $actual" >&2
    return 1
  fi
  echo "==> SHA-256 校验通过: $asset_name"
}

echo "==> [1/4] 下载独立 Python 3.9.7 ($TARGET)"
echo "    $URL"
download_pbs "$ASSET" "$WORK/pbs.tar.zst"
verify_archive "$WORK/pbs.tar.zst" "$ASSET"

echo "==> [2/4] 解压"
mkdir -p "$WORK/root"
"$BOOTSTRAP_PY" "$SCRIPT_DIR/tarzst.py" extract "$WORK/pbs.tar.zst" "$WORK/root"
PY="$WORK/root/$PYBIN"
if [ "$TARGET" = "win-x64" ]; then
  SP_DIR="$WORK/root/python/install/Lib/site-packages"
else
  SP_DIR="$WORK/root/python/install/lib/python3.9/site-packages"
fi

# bash 3.2 兼容：空数组不能直接 "${RUN[@]}"
run_py() {
  if [ "${#RUN[@]}" -gt 0 ]; then "${RUN[@]}" "$PY" "$@"; else "$PY" "$@"; fi
}

echo "==> [3/4] 安装锁定依赖（仅使用官方 wheel，绝不源码编译）"
WHEELHOUSE="$WORK/wheels"
mkdir -p "$WHEELHOUSE"
if [ "$TARGET" = "win-x64" ]; then
  # macOS/Linux 上无法运行 python.exe，需跨平台安装 win_amd64 wheel。
  # 注意：pip 的环境标记（python_version>=...）按「运行中的解释器」求值，
  # 必须用 Python 3.9 执行安装，否则 >=3.10/3.11 的标记会误判生效导致 numpy 解析冲突。
  echo "==> [3/4] 安装锁定依赖（跨平台模式，使用 Python 3.9.7 安装器）"
  case "$(uname -sm)" in
    "Darwin arm64")  HOST_ASSET="cpython-3.9.7-aarch64-apple-darwin-pgo+lto-$PBS_TS.tar.zst" ;;
    "Darwin x86_64") HOST_ASSET="cpython-3.9.7-x86_64-apple-darwin-pgo+lto-$PBS_TS.tar.zst" ;;
    "Linux x86_64")  HOST_ASSET="cpython-3.9.7-x86_64-unknown-linux-gnu-pgo+lto-$PBS_TS.tar.zst" ;;
    *) echo "win-x64 跨平台构建仅支持 macOS / Linux x86_64 主机" >&2; exit 1 ;;
  esac
  download_pbs "$HOST_ASSET" "$WORK/host-pbs.tar.zst"
  verify_archive "$WORK/host-pbs.tar.zst" "$HOST_ASSET"
  mkdir -p "$WORK/host"
  "$BOOTSTRAP_PY" "$SCRIPT_DIR/tarzst.py" extract "$WORK/host-pbs.tar.zst" "$WORK/host"
  HOSTPY="$WORK/host/python/install/bin/python3"
  "$HOSTPY" -m pip download --disable-pip-version-check \
    $PIP_HASH_FLAGS \
    --platform win_amd64 --implementation cp --abi cp39 --python-version 3.9 \
    --dest "$WHEELHOUSE" --index-url "$MIRROR" \
    -r "$REQ_FILE"
  "$HOSTPY" -m pip install --disable-pip-version-check \
    $PIP_HASH_FLAGS \
    --platform win_amd64 --implementation cp --abi cp39 --python-version 3.9 \
    --no-index --find-links "$WHEELHOUSE" --target "$SP_DIR" \
    -r "$REQ_FILE"

  echo "==> [3.6/4] 性能优化：预编译字节码（字体缓存由 CI 在 Windows 原生生成）"
  "$HOSTPY" -m compileall -q "$SP_DIR" || true
else
  # 下载阶段显式限定目标平台；安装阶段完全离线，避免镜像在下载和执行之间漂移。
  run_py -m pip download --disable-pip-version-check \
    $PIP_HASH_FLAGS \
    --platform "$PIP_PLATFORM" --implementation cp --abi cp39 --python-version 3.9 \
    --dest "$WHEELHOUSE" --index-url "$MIRROR" \
    -r "$REQ_FILE"
  run_py -m pip install --disable-pip-version-check \
    $PIP_HASH_FLAGS \
    --no-index --find-links "$WHEELHOUSE" \
    -r "$REQ_FILE"
  # macOS: 修复 xgboost 的 OpenMP 运行时。
  # xgboost 的 macOS wheel 不自带 libomp.dylib（rpath 指向 Homebrew 路径），
  # 终端用户机器没有 Homebrew 时会 import 失败。
  # 复用 scikit-learn wheel 自带的 libomp（delocate 打包，同架构匹配），
  # 并给 libxgboost.dylib 增加 @loader_path rpath 使其加载捆绑副本。
  XGB_LIB="$SP_DIR/xgboost/lib"
  SKL_OMP="$SP_DIR/sklearn/.dylibs/libomp.dylib"
  if [ -f "$XGB_LIB/libxgboost.dylib" ] && [ -f "$SKL_OMP" ]; then
    cp "$SKL_OMP" "$XGB_LIB/libomp.dylib"
    install_name_tool -add_rpath @loader_path "$XGB_LIB/libxgboost.dylib" 2>/dev/null || true
    # 修改后需重新 adhoc 签名（Apple Silicon 强制要求有效签名）
    codesign --force --sign - "$XGB_LIB/libomp.dylib" "$XGB_LIB/libxgboost.dylib" >/dev/null 2>&1 || true
  fi
  # 全量导入验证（所有锁定包）
  run_py -c "import numpy, pandas, scipy, matplotlib, seaborn, sklearn, openpyxl, xlrd, PIL, cv2, xgboost, imblearn, onnxruntime, notebook; print('全部 14 个依赖库导入校验通过')"

  echo "==> [3.6/4] 性能优化：预编译字节码 + 预建 matplotlib 字体缓存"
  # 1) 预编译全部 .pyc：消除用户首次 import 时的字节码编译开销
  run_py -m compileall -q "$SP_DIR" || true
  # 2) 预建 matplotlib 字体缓存：避免用户首次绘图时扫描全系统字体（卡顿 10~30 秒）
  #    缓存打进快照内的 python/mpl-config，运行时通过 MPLCONFIGDIR 指向它
  export MPLCONFIGDIR="$WORK/root/python/install/mpl-config"
  run_py -c "import matplotlib.pyplot as plt; print('matplotlib 字体缓存已预建')" || true
  unset MPLCONFIGDIR
fi

echo "==> [3.5/4] 生成出厂包清单 factory-manifest.json（应用以此区分 内置/用户安装）"
if [ "$TARGET" = "win-x64" ]; then
  "$HOSTPY" -m pip list --format=json --path "$SP_DIR" > "$WORK/root/python/install/factory-manifest.json"
else
  run_py -m pip list --format=json > "$WORK/root/python/install/factory-manifest.json"
fi
run_py -c "import json; d=json.load(open('$WORK/root/python/install/factory-manifest.json')); print('出厂清单共', len(d), '个包')" 2>/dev/null || "$BOOTSTRAP_PYTHON" -c "import json; d=json.load(open('$WORK/root/python/install/factory-manifest.json')); print('出厂清单共', len(d), '个包')"

echo "==> [4/4] 压缩出厂快照"
mkdir -p "$OUT_DIR"
# 只保留 install 发行树，快照内顶层目录统一命名为 python/
mv "$WORK/root/python/install" "$WORK/python"
# 防止 macOS bsdtar 把扩展属性写成 AppleDouble (._*) 文件污染快照：
# 这些文件会被 Python site 机制误读为 .pth 导致解释器启动崩溃
export COPYFILE_DISABLE=1
xattr -rc "$WORK/python" 2>/dev/null || true
find "$WORK/python" -name "._*" -delete 2>/dev/null || true
"$BOOTSTRAP_PY" "$SCRIPT_DIR/tarzst.py" compress "$WORK/python" "$OUT_DIR/env-factory.tar.zst.tmp"
mv "$OUT_DIR/env-factory.tar.zst.tmp" "$OUT_DIR/env-factory.tar.zst"
echo "==> 完成: $OUT_DIR/env-factory.tar.zst ($(du -h "$OUT_DIR/env-factory.tar.zst" | cut -f1 | xargs))"
