# 朱比特和它的朋友们

**开箱即用的 Jupyter Notebook 桌面版** —— 内置 Python 3.9.7 与全部数据科学依赖，双击即用，无需安装 Python、无需配置环境、不影响系统。

面向不会配置环境的用户（数据分析师、学生、课程学员）：下载 → 拖入应用程序 → 打开 → 点「启动 Notebook」，全程没有命令行。

## 功能特性

- 🚀 **一键启动**：内置 Jupyter Notebook 6.4.8，自动处理端口、token 认证并调起浏览器
- 📦 **出厂自带 16 个常用库**：pandas / numpy / scipy / matplotlib / seaborn / scikit-learn / xgboost / opencv / onnxruntime / openpyxl / xlrd / Pillow / imbalanced-learn ……均为锁定版本
- 🔒 **完全沙箱隔离**：环境位于应用私有目录，不改 PATH、不碰系统/Homebrew/conda Python；同时禁用用户站点目录与 pip 用户配置，**双向隔离**
- 📥 **界面化装包**：填包名即可安装，默认走阿里云镜像（可切清华/官方源），安装日志实时可见
- ♻️ **一键重置**：用户把环境搞坏后，10 秒内从本地出厂快照恢复（纯本地操作，无需联网）
- ✅ **默认信任**：读取 .ipynb 时由应用私有的 ContentsManager 使用官方签名机制标记可信，首次打开无需刷新
- 🕘 **真实最近访问**：在 Notebook 页面确认成功打开后，通过应用私有的回环回调记录文件
- ⚡ **首跑加速**：预编译字节码 + 预建 matplotlib 字体缓存 + 启动后后台预热，首个单元格从 ~30 秒降至秒级
- 🛡 **生命周期自管理**：退出应用自动停止服务，强杀也不留孤儿进程；工作目录在外接硬盘且未挂载时自动回退默认目录

> **自动信任的安全边界**：本应用将工作目录视为受信任工作区——目录内 notebook 中存储的 HTML/JavaScript 输出会在打开时直接渲染。服务仅监听 127.0.0.1、携带随机 token、仅服务该目录，风险可控；但请勿在不知情时将**来路不明的 .ipynb 文件**放入工作目录后直接用本应用打开（这与 Jupyter 官方 "Trust Notebook" 按钮的语义一致，只是自动化了）。

## 下载安装

| 平台 | 文件 | 说明 |
|---|---|---|
| macOS Apple Silicon (M 系列) | `朱比特和它的朋友们_x.x.x_macOS-AppleSilicon.dmg` | 4 个依赖为 arm64 兼容小版本（见下文） |
| macOS Intel | `朱比特和它的朋友们_x.x.x_macOS-Intel.dmg` | 全部原始锁定版本 |
| Windows 10 x64 | `朱比特和它的朋友们_x.x.x_x64-setup.exe` | 全部原始锁定版本 |

**macOS**：双击 DMG → 按窗口内图示把应用拖入「应用程序」。首次打开如提示「无法验证开发者」，右键点击应用图标选择「打开」。

**Windows**：运行安装程序，如遇 SmartScreen 提示选择「仍要运行」。

> 安装包当前未做代码签名（如需消除警告：macOS 需 Apple 开发者账号做公证，Windows 需代码签名证书）。

## 依赖版本矩阵

| 平台 | 版本策略 |
|---|---|
| Windows x64 / macOS Intel | **100% 原始锁定版本**（pandas 1.3.4、numpy 1.22.4、scipy 1.7.1、matplotlib 3.4.3、scikit-learn 0.24.2 等 16 个） |
| macOS Apple Silicon | 12 个原始版本 + 4 个微调：pandas→**1.4.0**、scipy→**1.7.3**、matplotlib→**3.5.0**、scikit-learn→**1.0.2**（这 4 个老版本官方未发布 arm64 安装包，取最邻近兼容小版本；已验证 imbalanced-learn 0.8.1 等相互兼容） |

直接依赖清单为 `runtime/requirements-{win-x64,macos-x64,macos-arm64}.txt`；构建实际使用对应的 `.lock.txt`，其中固定完整传递依赖和目标 wheel 的 SHA-256。

## 技术架构

```
Tauri 2 (Rust + 系统 WebView)
├── 前端：原生 HTML/JS + Tailwind v4（app/ui/）
├── 后端：Rust 管理环境生命周期（初始化/pip/notebook 进程/重置）
├── 内置 Python：python-build-standalone CPython 3.9.7（可重定位发行版）
└── 出厂快照：env-factory.tar.zst（Python + 全部依赖预装压缩，~165MB）
    ├── 首次启动解压到用户私有目录（一次性）
    ├── 安装新包 = 快照内 python -m pip（默认阿里云镜像）
    └── 一键重置 = 删除 env 目录 + 重新解压快照
```

关键设计：

- **沙箱**：所有子进程设置 `PYTHONNOUSERSITE=1`、`PIP_CONFIG_FILE=/dev/null`、清除 `PYTHONPATH`；Jupyter 数据/运行时目录收进应用沙箱
- **默认信任**：自定义 `FileContentsManager` 在读取 .ipynb 时使用 Notebook 官方签名机制标记可信
- **事件系统**：前端使用 data-action + 事件委托（Tauri 注入的 CSP hash 会使 `'unsafe-inline'` 失效，内联 onclick 会被静默拦截——不要回退到内联事件）

## 项目结构

```
├── app/                     # Tauri 应用
│   ├── ui/                  #   前端（index.html / app.js / trap.js）
│   ├── tailwind.input.css   #   样式源（Tailwind v4，npm run css 构建）
│   └── src-tauri/           #   Rust 后端（src/lib.rs 为核心逻辑）
│       └── resources/       #   出厂快照（构建时生成，不入库）
├── runtime/                 # 构建链
│   ├── requirements-*.txt   #   三平台直接依赖清单与完整 wheel 哈希锁
│   ├── requirements-bootstrap-*.lock.txt # Python 3.13.5 构建工具哈希锁
│   ├── requirements-dmgbuild.lock.txt    # DMG 工具完整传递依赖哈希锁
│   ├── build-snapshot.sh    #   快照构建（macOS/Linux，含交叉 Windows）
│   ├── build-snapshot.ps1   #   快照构建（Windows）
│   ├── make-dmg.sh          #   带中文指引的 DMG 制作（dmgbuild）
│   └── snapshots/           #   预构建快照缓存
├── .github/workflows/       # 三平台 CI（macos-14 / macos-13 / windows-2022）
├── dist/                    # 最终安装包
└── prototype.html           # 已确认的产品原型（交互稿）
```

## 本地开发

```bash
# 准备：Rust 1.77+ / Node 20+ / Python 3.13.5
# 可通过 BOOTSTRAP_PYTHON 指向精确的 Python 3.13.5；无需系统 zstd。

# 1. 构建本机平台的出厂快照（首次约 5 分钟）
./runtime/build-snapshot.sh macos-arm64        # 或 macos-x64

#    本地迭代可加 --fast：跳过 SHA-256 校验与依赖哈希锁定，
#    并启用 pip / Python 归档缓存（重复构建快一倍以上）。仅限本地，CI 保持全量校验。
./runtime/build-snapshot.sh macos-arm64 --fast

# 2. 启动开发模式
cd app
npm install
npm run tauri dev
```

仅调试界面（mock 后端，不依赖 Rust）：浏览器直接打开 `app/ui/index.html`。

## 构建安装包

```bash
# macOS（在对应架构机器上，或本机加 rustup target 交叉）
# 运行时快照和 DMG 工具都会拒绝非 3.13.5 的构建 Python，且只安装哈希锁允许的 wheel。
./runtime/build-snapshot.sh macos-arm64        # 产出快照到 app/src-tauri/resources/
cd app && npm ci && npx tauri build --bundles app
cd .. && ./runtime/make-dmg.sh "app/src-tauri/target/release/bundle/macos/朱比特和它的朋友们.app" \
        "dist/朱比特和它的朋友们_1.0.0_macOS-AppleSilicon.dmg"

# Windows：在 Windows 上运行 runtime\build-snapshot.ps1 后 npx tauri build（产出 NSIS 安装包）
```

**推荐：直接用 CI**。推送后进入 Actions → 「构建多平台安装包」→ Run workflow，约 15 分钟后在 Artifacts 下载三个平台安装包（或打 `v*` 标签自动触发）。

## 排障与诊断

- 界面出现任何前端异常时，顶部会显示红色横幅说明原因
- 从终端启动应用二进制可查看 `[UI-DIAG]` 诊断日志（含每次启动的自动化自检结果：点击链路、包计数、服务启停）
- 设置 `JUPITER_SELFTEST=1` 环境变量启动，可额外执行 notebook 启停自检（不打开浏览器）
- Notebook 服务日志：`<应用数据目录>/notebook.log`

## 常见问题

**Q：会影响我电脑里已有的 Python 吗？**
不会。应用只读写自己的私有目录，与你的系统 Python、conda、Homebrew 完全无关；卸载只需删除应用图标。

**Q：重置会删我的笔记文件吗？**
不会。重置只恢复 Python 环境；工作目录里的 .ipynb 文件不受任何影响。

**Q：Apple Silicon 上为什么有 4 个包版本不一样？**
pandas 1.3.4 / scipy 1.7.1 / matplotlib 3.4.3 / scikit-learn 0.24.2 官方没有发布 Apple Silicon 安装包，强行安装会触发源码编译（极易失败）。微调到的版本是 arm64 可用的最邻近版本，API 兼容。
