# pip-only 依赖验证报告

验证日期：2026-08-18  
目标 Python：`3.9.7`  
目标平台：macOS 12.0+ ARM64、macOS 12.0+ Intel x86_64、Windows 10/11 x86_64

## 结论

候选版本集合在三个目标平台上均能由 pip 解析出完整的二进制 wheel 依赖树，没有版本冲突或缺失 wheel。解析分别得到：macOS 12 ARM64 114 个包、macOS 12 Intel 114 个包、Windows x64 115 个包。

解析同时使用了 PyPI 和产品默认的阿里云镜像；阿里云结果也全部通过。阿里云镜像当前对 `idna` 选择 `3.18`，其余锁定结果一致，且满足所有上游约束。

本机 macOS ARM64 已用真实 Python 3.9.7 venv 安装并验证：16 个直接依赖全部导入成功、`pip check` 通过、Notebook 回环请求 HTTP 200。`xgboost` 需要 macOS 的 OpenMP 动态库，因此已验证把对应架构、最低系统为 macOS 12.0 的 `libomp.dylib` 随应用携带并通过 `@loader_path` rpath 加载；不需要用户安装 Homebrew 或 conda。

本机 venv 最终按阿里云生成的 ARM64 锁文件同步（包括 `idna==3.18`）后再次完成上述检查。

## 版本变更

| 包 | 原始版本 | pip-only 候选版本 | 说明 |
| --- | ---: | ---: | --- |
| pandas | 1.3.4 | 2.0.3 | ARM64 wheel 与 Python 3.9.7 兼容 |
| numpy | 1.22.4 | 1.26.4 | 满足 scipy、scikit-learn、opencv 约束 |
| scipy | 1.7.1 | 1.10.1 | ARM64 wheel 可用，兼容 numpy 1.26 |
| matplotlib | 3.4.3 | 3.7.5 | ARM64 wheel 可用 |
| seaborn | 0.11.2 | 0.13.2 | 兼容 pandas 2.0 / matplotlib 3.7 |
| openpyxl | 3.0.9 | 3.1.5 | Python 3.9 wheel 可用 |
| xlrd | 2.0.1 | 2.0.1 | 保持不变 |
| Pillow | 8.4.0 | 10.4.0 | ARM64 wheel 可用 |
| opencv-python | 4.10.0.84 | 4.10.0.84 | 保持不变，三平台 wheel 可用 |
| scikit-learn | 0.24.2 | 1.3.2 | 与 imbalanced-learn 0.12.4 兼容 |
| xgboost | 2.1.1 | 2.1.1 | Python 依赖无冲突；需随包携带 `libomp` |
| imbalanced-learn | 0.8.1 | 0.12.4 | 与 scikit-learn 1.3.2 兼容 |
| onnxruntime | 1.12.1 | 1.18.1 | numpy 约束为 `<2.0`，与 1.26.4 兼容 |
| notebook | 6.4.8 | 6.5.7 | 与 traitlets 5.14.3 及最新可用传递依赖兼容 |
| traitlets | 5.1.1 | 5.14.3 | 避免旧 Notebook/Jupyter 传递依赖冲突 |
| matplotlib-inline | 0.1.3 | 0.1.7 | 与 matplotlib 3.7.5 兼容 |

候选直接依赖文件：`requirements-pip-only-candidate.txt`。

## 平台解析结果

使用 uv `0.12.5` 仅作为解析工具（不是运行时依赖）：

```text
uv pip compile
  --python-version 3.9.7
  --python-platform <target>
  --only-binary=:all:
  --no-python-downloads
```

| 目标 | marker/平台解析 | 包数量 | 结果 |
| --- | --- | ---: | --- |
| macOS ARM64 | `aarch64-apple-darwin` | 114 | PASS |
| macOS Intel | `x86_64-apple-darwin` | 114 | PASS |
| Windows x64 | `x86_64-pc-windows-msvc` | 115 | PASS |

Windows 解析正确选择了 `pywin32`、`pywinpty`、`colorama` 等 Windows 依赖；macOS 解析选择了 `appnope`、`pexpect`、`ptyprocess`。因此最终结论没有使用 pip 在宿主 macOS 上错误复用 Windows 环境标记。

随后对阿里云锁文件执行了全量 `pip download --no-deps --only-binary=:all:`：macOS ARM64 `114/114`、macOS Intel `114/114`、Windows x64 `115/115`，所有传递依赖的目标 wheel 均可取得。

直接 wheel 的最低系统标签检查结果：

- macOS 12 ARM64：16/16 PASS。
- macOS 12 Intel：16/16 PASS。
- macOS 11 ARM64：缺少 scipy 1.10.1、scikit-learn 1.3.2、xgboost 2.1.1。
- macOS 11 Intel：缺少 opencv-python 4.10.0.84。

因此这组版本应把两个 macOS 安装包的最低系统统一设为 macOS 12.0。

阿里云镜像的重跑命令和原始日志位于 `work/pip-only-aliyun/`。可直接用于安装的按平台锁文件为：

- `requirements-pip-only-lock-macos-arm64.txt`
- `requirements-pip-only-lock-macos-intel.txt`
- `requirements-pip-only-lock-windows-x64.txt`

`work/conda/` 和上一份 `VALIDATION_REPORT.md` 是前一轮历史验证产物；本轮 pip-only 脚本不读取它们，最终方案不依赖 conda。

## 本机运行时验证

- Python：精确为 `3.9.7`，运行时位于 `work/pip-only/`。
- 直接依赖：16/16 版本匹配并成功导入。
- `pip check`：`No broken requirements found.`
- Notebook 6.5.7：绑定 `127.0.0.1`，回环访问 HTTP 200。
- 用户安装：从 `https://mirrors.aliyun.com/pypi/simple/` 安装 `tomli==2.0.1` 成功。
- 重置：从 golden venv 复制到 staging 后原子替换；`tomli` 被移除，`numpy==1.26.4` 恢复，重置后 `pip check` 通过。
- 宿主隔离：宿主 Python 3.13 中不可见 `tomli`，宿主环境未安装候选依赖。

## xgboost 的 macOS 原生依赖

`xgboost==2.1.1` 的 macOS wheel 会加载 `@rpath/libomp.dylib`。这不是 pip 的版本冲突，但如果不处理，用户机器没有 OpenMP 时导入会失败。

验证方案：

1. 为 ARM64 和 x86_64 分别准备 LLVM OpenMP `libomp.dylib`。
2. 将它放入 `xgboost/lib/`，并把 `libxgboost.dylib` 的 rpath 设置为 `@loader_path`。
3. 在最终 macOS app 签名之前对所有 dylib 统一 codesign。

本次测试使用 Homebrew 历史 `libomp 18.1.8` 的 Monterey bottles 作为可分发的 LLVM OpenMP 二进制来源，不调用 conda。两个 dylib 均经 `otool` 确认为对应架构且 `minos 12.0`：

- ARM64 bottle：SHA-256 `a1953d256b69b0b29cb1b2f933a15878ef9cfa8caf5f8c439ce4d4a8d6892ca3`
- Intel bottle：SHA-256 `0f1883c4651a04259281cc0fb4791effe7ab409a0d55d62fdd16435bf4d7e61e`

资源和许可证均保存在 `work/pip-only/native/libomp-18.1.8/`。xgboost 2.1.4 的 macOS wheel 仍然外部依赖 `libomp`，而 xgboost 3.x 要求 Python 3.10+，所以升级 xgboost 不能在保留 Python 3.9.7 的同时消除这一步。

## 验证边界

- Windows x64 和 macOS Intel 本轮验证的是 Python 3.9.7 的 resolver、marker 和 wheel 元数据；没有在对应真机执行导入和 Notebook 启动。macOS 12 最低系统标签也只做了静态检查。应在 Windows 10 x64、macOS 12 ARM64、macOS 12 Intel CI/真机上各跑一次相同烟测。
- 没有验证安装包签名、公证、安装器升级/卸载和干净机器上的系统权限行为。
- Python 3.9.7 已停止上游安全支持；如果业务允许，建议后续把运行时升级到仍受支持的 Python 小版本，并重新生成三平台锁文件。若必须保持 3.9.7，需要自行承担运行时安全补丁和证书维护。
- `pip check` 只能检查 Python 元数据依赖，不能替代原生 ABI、GPU 驱动或用户代码兼容性测试；因此应用仍应保留 golden runtime 和一键重置入口。
- Windows 上执行目录替换前必须先停止 Notebook、kernel 和 pip 子进程，否则文件锁可能阻止 staging/current 交换；本轮尚未做 Windows 文件锁真机验收。
