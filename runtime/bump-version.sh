#!/bin/bash
# 版本一键升级脚本（单一权威源：src-tauri/tauri.conf.json）
# 用法: bump-version.sh <新版本号>   例如: bump-version.sh 1.1.0
set -euo pipefail

NEW="${1:?用法: bump-version.sh <新版本号>（如 1.1.0 或 1.1.0-alpha.1）}"
# 支持语义化预发布后缀：1.1.0 / 1.1.0-alpha.1 / 1.1.0-beta.2 / 1.1.0-rc.1
if ! [[ "$NEW" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z]+(\.[0-9]+)?)?$ ]]; then
  echo "版本号格式应为 x.y.z 或 x.y.z-预发布标识（如 1.1.0-alpha.1），收到: $NEW" >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TAURI_CONF="$ROOT/app/src-tauri/tauri.conf.json"
CARGO_TOML="$ROOT/app/src-tauri/Cargo.toml"
PKG_JSON="$ROOT/app/package.json"

OLD=$(python3 -c "import json; print(json.load(open('$TAURI_CONF'))['version'])")
SKIP_EDITS=0
if [ "$OLD" = "$NEW" ]; then
  echo "版本已是 $NEW，跳过文件修改，仅同步锁文件"
  SKIP_EDITS=1
fi

if [ "$SKIP_EDITS" = "0" ]; then

# 1) 权威源
python3 - "$TAURI_CONF" "$NEW" << 'EOF'
import json, sys
p, v = sys.argv[1], sys.argv[2]
d = json.load(open(p))
d["version"] = v
json.dump(d, open(p, "w"), indent=2, ensure_ascii=False)
open(p, "a").write("\n")
EOF

# 2) Cargo.toml（仅替换 [package] 的首个 version 行；用 Python 保证 macOS 兼容）
python3 - "$CARGO_TOML" "$NEW" << 'EOF'
import re, sys
p, v = sys.argv[1], sys.argv[2]
s = open(p).read()
s2 = re.sub(r'(?m)^version = "[0-9]+\.[0-9]+\.[0-9]+[^"]*"', f'version = "{v}"', s, count=1)
if s2 == s:
    sys.exit("Cargo.toml 中未找到 version 行")
open(p, "w").write(s2)
EOF

# 3) package.json
python3 - "$PKG_JSON" "$NEW" << 'EOF'
import json, sys
p, v = sys.argv[1], sys.argv[2]
d = json.load(open(p))
d["version"] = v
json.dump(d, open(p, "w"), indent=2, ensure_ascii=False)
open(p, "a").write("\n")
EOF
fi

echo "版本已升级: $OLD → $NEW"
echo "  ✓ src-tauri/tauri.conf.json（权威源）"
echo "  ✓ src-tauri/Cargo.toml"
echo "  ✓ app/package.json"

# 4) 同步锁文件（保持与版本号一致，避免漂移）
echo "==> 同步 package-lock.json"
( cd "$ROOT/app" && npm install --package-lock-only --ignore-scripts -q )
echo "==> 同步 Cargo.lock"
( cd "$ROOT/app/src-tauri" && cargo check -q 2>/dev/null || true )

echo "界面版本号由 app_version 命令动态读取，无需手动改 UI。"
