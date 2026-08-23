"""朱比特和它的朋友们 · 运行时兼容性补丁（Windows）

修复场景：Notebook 内核 sys.path 中的空字符串条目（代表当前目录）经
pkg_resources 初始化触发 os.path.realpath 时，Python 3.9.7 会生成带
\\?\ 前缀且尾部残留 "\." 的 verbatim 路径：
  - 部分机器上 realpath 直接抛出 WinError 161（路径无效）
  - 另一些机器上 realpath 不抛错但返回带病路径，后续 os.listdir
    抛出 WinError 123（文件名语法不正确）
两者均导致 pkg_resources 在 import 阶段崩溃，连带 sklearn 等库无法导入。

处理（仅 Windows）：
  1. realpath 抛 OSError 时回退为纯规范化绝对路径；
  2. 清除返回值尾部残留的 "\." 组件。
"""
import os

if os.name == "nt":
    _orig_realpath = os.path.realpath

    def _safe_realpath(path):
        try:
            result = _orig_realpath(path)
        except OSError:
            # WinError 161 等真实解析失败：回退为 abspath（不触发 verbatim 解析）
            result = os.path.abspath(path)
        # 清除 verbatim 路径尾部残留的 "\." 组件（WinError 123/161 根源）
        if isinstance(result, str):
            while result.endswith("\\.") or result.endswith("/."):
                stripped = result[:-2]
                # 保护盘符根目录："\\?\D:\." 不能削成 "\\?\D:"（驱动器相对路径，不可用），
                # 应归一为 "\\?\D:\"
                if stripped.endswith(":"):
                    result = stripped + "\\"
                    break
                result = stripped
        return result

    os.path.realpath = _safe_realpath
