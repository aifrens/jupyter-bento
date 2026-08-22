"""tar.zst 解压/压缩辅助工具（跨平台，供快照构建脚本调用）

用法:
  python tarzst.py extract  <输入.tar.zst> <输出目录>
  python tarzst.py compress <输入目录>     <输出.tar.zst>
"""
import copy
import ntpath
import os
import sys
import tarfile


class UnsafeArchiveError(ValueError):
    """归档成员会越过目标目录，或使用了不允许提取的成员类型。"""


_WINDOWS_RESERVED_NAMES = {
    "CON",
    "PRN",
    "AUX",
    "NUL",
    *(f"COM{index}" for index in range(1, 10)),
    *(f"LPT{index}" for index in range(1, 10)),
}


def _validate_portable_part(part: str, *, field: str, value: str) -> None:
    windows_part = part.rstrip(" .")
    if windows_part in ("", ".", ".."):
        raise UnsafeArchiveError(f"{field} 包含不安全的 Windows 路径组件: {value!r}")
    if any(ord(char) < 32 or char in '<>:"|?*' for char in windows_part):
        raise UnsafeArchiveError(f"{field} 包含不安全的 Windows 路径字符: {value!r}")
    stem = windows_part.split(".", 1)[0].upper()
    if stem in _WINDOWS_RESERVED_NAMES:
        raise UnsafeArchiveError(f"{field} 包含 Windows 设备名: {value!r}")


def _path_parts(value: str, *, field: str, reject_parent: bool) -> list[str]:
    normalized = value.replace("\\", "/")
    drive, _ = ntpath.splitdrive(normalized)
    if drive or normalized.startswith("/"):
        raise UnsafeArchiveError(f"{field} 不允许使用绝对路径: {value!r}")

    parts = []
    for part in normalized.split("/"):
        if part in ("", "."):
            continue
        if part == ".." and reject_parent:
            raise UnsafeArchiveError(f"{field} 不允许包含 '..': {value!r}")
        if part != "..":
            _validate_portable_part(part, field=field, value=value)
        parts.append(part)
    return parts


def _resolve_link_parts(member_parts: list[str], linkname: str, *, symlink: bool) -> list[str]:
    if not linkname:
        raise UnsafeArchiveError("链接目标不能为空")

    target_parts = _path_parts(linkname, field="链接目标", reject_parent=False)
    resolved = list(member_parts[:-1] if symlink else [])
    for part in target_parts:
        if part == "..":
            if not resolved:
                raise UnsafeArchiveError(f"链接目标越过输出目录: {linkname!r}")
            resolved.pop()
        else:
            resolved.append(part)
    return resolved


def _assert_realpath_within(root: str, path: str, *, field: str) -> None:
    root_key = os.path.normcase(root)
    path_key = os.path.normcase(os.path.realpath(path))
    try:
        contained = os.path.commonpath((root_key, path_key)) == root_key
    except ValueError:
        contained = False
    if not contained:
        raise UnsafeArchiveError(f"{field} 越过输出目录: {path!r}")


def _validate_member(member: tarfile.TarInfo, root: str) -> None:
    member_parts = _path_parts(member.name, field="归档成员路径", reject_parent=True)
    if not member_parts and not member.isdir():
        raise UnsafeArchiveError(f"归档成员路径为空: {member.name!r}")

    destination = os.path.join(root, *member_parts)
    _assert_realpath_within(root, destination, field="归档成员路径")

    if member.issym() or member.islnk():
        target_parts = _resolve_link_parts(
            member_parts,
            member.linkname,
            symlink=member.issym(),
        )
        target = os.path.join(root, *target_parts)
        _assert_realpath_within(root, target, field="链接目标")
    elif not (member.isfile() or member.isdir()):
        raise UnsafeArchiveError(
            f"归档成员类型不允许提取: {member.name!r} ({member.type!r})"
        )


def _validated_members(tf: tarfile.TarFile, root: str):
    for member in tf:
        _validate_member(member, root)
        safe_member = copy.copy(member)
        safe_member.uid = None
        safe_member.gid = None
        safe_member.uname = None
        safe_member.gname = None
        if safe_member.isdir():
            safe_member.mode = 0o755
        elif safe_member.issym():
            safe_member.mode = 0o777
        else:
            mode = (safe_member.mode or 0) & 0o755
            if not mode & 0o100:
                mode &= ~0o111
            safe_member.mode = mode | 0o600
        yield safe_member


def _extract_tar_stream(fileobj, dst: str) -> None:
    os.makedirs(dst, exist_ok=True)
    root = os.path.realpath(os.path.abspath(dst))
    if not os.path.isdir(root):
        raise NotADirectoryError(dst)

    with tarfile.open(fileobj=fileobj, mode="r|") as tf:
        kwargs = {"filter": "data"} if hasattr(tarfile, "data_filter") else {}
        tf.extractall(root, members=_validated_members(tf, root), **kwargs)


def extract(src: str, dst: str) -> None:
    """把 tar.zst 解压到 dst，并拒绝任何越过目标目录的归档成员。"""
    import zstandard

    with open(src, "rb") as f:
        dctx = zstandard.ZstdDecompressor()
        with dctx.stream_reader(f) as reader:
            _extract_tar_stream(reader, dst)


def compress(src: str, dst: str) -> None:
    import zstandard

    # 快照内顶层目录统一命名为 python/
    with open(dst, "wb") as f:
        cctx = zstandard.ZstdCompressor(level=19)
        with cctx.stream_writer(f) as writer:
            with tarfile.open(fileobj=writer, mode="w|") as tf:
                tf.add(src, arcname="python")


if __name__ == "__main__":
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    cmd, a, b = sys.argv[1], sys.argv[2], sys.argv[3]
    {"extract": extract, "compress": compress}[cmd](a, b)
