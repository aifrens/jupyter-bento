# Repository Guidelines

## 项目结构与模块组织

`app/` 是 Tauri 2 桌面应用。原生 HTML、JavaScript 前端位于 `app/ui/`，Tailwind 源文件为 `app/tailwind.input.css`；Rust 生命周期、依赖管理及 Notebook 命令位于 `app/src-tauri/src/`。正式图标源文件保存在 `app/app-icon.svg`，生成的平台图标位于 `app/src-tauri/icons/`。

`runtime/` 负责生成 Python 3.9.7 运行时快照，存放锁定依赖、DMG 工具和各平台缓存。`test/` 包含运行时、重置、Notebook HTTP、wheel 覆盖率及宿主隔离检查。CI 打包配置位于 `.github/workflows/build.yml`，发布产物输出到 `dist/`。

## 构建、测试与开发命令

- `cd app && npm ci`：安装锁定版本的 Node.js 依赖。
- `cd app && npm run build`：编译 Tailwind，并生成不含 `mock.js` 的 `dist-ui/`（ES Modules）。
- `cd app && npm run preview`：本地 http 服务预览界面（ES Modules 不支持 file:// 直开）。
- `cd app && npm run tauri dev`：以开发模式启动桌面应用。
- `./runtime/build-snapshot.sh macos-arm64`：生成嵌入式运行时；Intel Mac 使用 `macos-x64`；本地迭代可加 `--fast`（跳过哈希校验、启用缓存，仅限本地）。
- `./runtime/bump-version.sh 1.1.0`：一键升级版本号（权威源为 `tauri.conf.json`）。
- `./runtime/release.sh 1.1.0`：一键发布——bump 版本、提交、推送 main、打 `v*` tag 并推送（tag 推送触发 CI 三平台构建与 GitHub Release）。
- `cd app && npm run tauri -- build --bundles app`：构建当前 macOS `.app`。
- `./test/scripts/run-validation.sh`：在 `test/work/` 中执行依赖网络的完整运行时验证。
- `cd app/src-tauri && cargo fmt --check && cargo check`：检查 Rust 格式和编译状态。

Windows NSIS 安装包必须在 Windows 环境或现有 `windows-2022` CI 任务中构建，不要依赖未经支持的 macOS 交叉打包。

## 编码风格与命名约定

JavaScript、HTML 和 CSS 使用两个空格缩进；Rust 以 `rustfmt` 输出为准。JavaScript 使用 `camelCase`，Rust 和 Python 使用 `snake_case`，常量使用 `SCREAMING_SNAKE_CASE`。保留前端现有的 `data-action` 事件委托；内联 `onclick` 会违反 CSP。公共命令、数据契约及不直观的安全边界应添加简洁的中文文档。不要手工修改生成的 `ui/styles.css`、`dist-ui/`、`target/` 或运行时归档。

## 测试规范

项目暂未设置覆盖率门槛，也没有传统单元测试套件。新增检查使用 `test_*.py` 或 `test-*.sh` 命名。运行时变更必须覆盖精确依赖导入、回环地址 Notebook 启动、重置行为及宿主隔离。缓存、日志、临时环境和测试 Notebook 必须保存在 `test/work/` 内。

## 发布流程

版本号权威源为 `app/src-tauri/tauri.conf.json`，git tag 只作标记。发版统一执行 `./runtime/release.sh <版本号>`（如 `./runtime/release.sh 1.0.1-beta.1`），脚本依次完成：bump-version.sh 同步四个版本文件 → 提交 → 推送 main → 打 `v*` tag → 推送 tag。推送 tag 触发 CI：先校验 tag 与 `tauri.conf.json` 版本一致且 tag 位于 main 分支（不一致立即失败），再三平台构建并自动创建 GitHub Release（tag 含 `-` 时标记 Pre-release）。

**被要求「打 tag / 推送 tag / 发版 / 发布」时，必须执行 `release.sh` 完成整个流程，禁止单独执行 `git tag` 或 `git push origin v*`**——手工打 tag 会跳过版本同步，被 CI 一致性校验拦截，或发出版本号与内容不符的包。

## 提交与合并请求规范

提交信息采用 Conventional Commits：类型使用英文，主题使用简洁中文，例如 `fix(runtime): 修复重置后的包清单`。合并请求需说明变更范围、已验证平台、执行命令、签名或公证状态，以及尚未完成的真机验证。UI 变更应附截图；不要提交无关生成物或密钥。

## 安全与配置提示

Notebook 必须绑定 `127.0.0.1` 并使用随机令牌。保留 `PYTHONNOUSERSITE`、应用私有的 Jupyter/pip 目录，以及 Notebook 文件与可重置运行时之间的隔离。自动信任 Notebook 属于明确的安全边界，相关改动必须在合并请求中说明。

包列表的「内置 / 直接安装 / 依赖」三态判定：`env/factory-manifest.json` 是内置的唯一权威依据（`build-snapshot.sh` 与 `build-snapshot.ps1` 必须都生成该清单，ps1 曾漏生成导致 Windows 出厂包被误判为用户安装）；`env/user-packages.json` 记录用户显式安装（随 env 目录整体重置）；其余靠 importlib.metadata 依赖图启发式兜底。包名比较一律使用 PEP 503 规范化，被其他包依赖的包禁止卸载（卸载保护）。
