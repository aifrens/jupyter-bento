#!/bin/bash
# 一键发布：同步版本号 → 提交 → 推送 main → 打 tag → 推送 tag
# 推送 v* tag 触发 CI：校验 tag 与版本一致 → 三平台构建 → 自动创建 GitHub Release。
# 用法: ./runtime/release.sh 1.0.1-beta.1
set -euo pipefail

NEW="${1:?用法: release.sh <新版本号>（如 1.1.0 或 1.1.0-beta.1）}"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT"
TAG="v$NEW"

# ---- 前置检查：任一不满足即中止，不发半成品版本 ----
if [ -n "$(git status --porcelain)" ]; then
  echo "工作区有未提交改动，请先提交或 stash" >&2
  exit 1
fi
if [ "$(git rev-parse --abbrev-ref HEAD)" != "main" ]; then
  echo "请在 main 分支上发版（当前：$(git rev-parse --abbrev-ref HEAD)）" >&2
  exit 1
fi
git fetch origin main --quiet
if [ "$(git rev-parse HEAD)" != "$(git rev-parse origin/main)" ]; then
  echo "本地 main 与 origin/main 不一致，请先 pull/push 对齐" >&2
  exit 1
fi
if git rev-parse --verify --quiet "refs/tags/$TAG" >/dev/null; then
  echo "tag $TAG 已存在，如需重发请先删除本地与远端 tag" >&2
  exit 1
fi

# ---- 1) 同步版本号（tauri.conf.json 权威源 → Cargo.toml / package.json / 两个 lock 文件） ----
"$SCRIPT_DIR/bump-version.sh" "$NEW"

# ---- 2) 提交版本变更（版本号未变化时锁文件可能也无改动，无改动则跳过提交） ----
git add app/src-tauri/tauri.conf.json app/src-tauri/Cargo.toml app/src-tauri/Cargo.lock \
  app/package.json app/package-lock.json
if ! git diff --cached --quiet; then
  git commit -m "chore(release): $NEW"
fi

# ---- 3) 推送 main 与 tag（tag 推送触发 CI 发布流水线） ----
git push origin main
git tag "$TAG"
git push origin "$TAG"

echo "==> 已发布 $TAG：CI 校验通过后构建三平台安装包，并自动创建 GitHub Release"
