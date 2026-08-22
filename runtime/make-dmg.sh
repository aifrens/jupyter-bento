#!/bin/bash
# 制作带中文安装指引的 DMG（傻瓜式：背景图 + 图标定位 + 拖拽说明）
# 用法: make-dmg.sh <app路径> <输出dmg路径>
# 依赖: Python 3.13.5（脚本自动创建隔离 venv 安装哈希锁定的 dmgbuild）
set -euo pipefail

APP_PATH="${1:?用法: make-dmg.sh <app路径> <输出dmg路径>}"
OUT_DMG="${2:?缺少输出路径}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BG_PNG="$SCRIPT_DIR/dmg-background.png"
APP_NAME="$(basename "$APP_PATH")"
VOL_NAME="朱比特和它的朋友们"
MIRROR="https://mirrors.aliyun.com/pypi/simple/"
BOOTSTRAP_PYTHON="${BOOTSTRAP_PYTHON:-python3}"
BOOTSTRAP_VERSION="3.13.5"
DMGBUILD_LOCK="$SCRIPT_DIR/requirements-dmgbuild.lock.txt"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# dmgbuild 会执行 Python 代码并写入发布产物；本地和 CI 必须使用同一精确
# 解释器与完整传递依赖哈希锁，版本或锁文件不满足时直接终止。
actual_bootstrap_version="$($BOOTSTRAP_PYTHON -c 'import platform; print(platform.python_version())')" || {
  echo "无法执行 DMG 构建 Python: $BOOTSTRAP_PYTHON" >&2
  exit 1
}
if [ "$actual_bootstrap_version" != "$BOOTSTRAP_VERSION" ]; then
  echo "DMG 构建 Python 版本不匹配：需要 $BOOTSTRAP_VERSION，实际 $actual_bootstrap_version ($BOOTSTRAP_PYTHON)" >&2
  exit 1
fi
[ -f "$DMGBUILD_LOCK" ] || { echo "缺少 dmgbuild 哈希锁: $DMGBUILD_LOCK" >&2; exit 1; }

# 输出文件名支持 @VERSION@ 占位符：从 tauri.conf.json（版本权威源）动态解析
if [[ "$OUT_DMG" == *"@VERSION@"* ]]; then
  APP_VERSION="$("$BOOTSTRAP_PYTHON" -c "import json; print(json.load(open('$SCRIPT_DIR/../app/src-tauri/tauri.conf.json'))['version'])")"
  OUT_DMG="${OUT_DMG//@VERSION@/$APP_VERSION}"
fi

echo "==> 准备 dmgbuild（隔离 venv，直接写 .DS_Store，无需 Finder 脚本权限）"
"$BOOTSTRAP_PYTHON" -m venv "$WORK/venv"
"$WORK/venv/bin/python" -m pip install -q --disable-pip-version-check \
  --require-hashes --only-binary=:all: --no-deps --no-cache-dir \
  --index-url "$MIRROR" -r "$DMGBUILD_LOCK"

"$BOOTSTRAP_PYTHON" - "$WORK/settings.py" "$APP_PATH" "$APP_NAME" "$BG_PNG" <<'PYEOF'
from pathlib import Path
import sys

settings_path = Path(sys.argv[1])
app_path, app_name, background = sys.argv[2:]
config = {
    "files": [(app_path, app_name)],
    "symlinks": {"应用程序": "/Applications"},
    "background": background,
    "window_rect": ((300, 120), (640, 400)),
    "icon_size": 100,
    "icon_locations": {
        app_name: (160, 170),
        "应用程序": (480, 170),
    },
    "format": "UDZO",
}

# 动态值只能通过 repr 写入 Python 字面量，不得拼接为可执行源码。
content = "\n".join(f"{name} = {value!r}" for name, value in config.items())
settings_path.write_text(
    "# dmgbuild 配置：中文指引背景 + 双图标定位\n" + content + "\n",
    encoding="utf-8",
)
PYEOF

echo "==> 构建 DMG"
mkdir -p "$(dirname "$OUT_DMG")"
"$WORK/venv/bin/dmgbuild" -s "$WORK/settings.py" "$VOL_NAME" "$OUT_DMG"
echo "==> 完成: $OUT_DMG ($(du -h "$OUT_DMG" | cut -f1 | xargs))"
