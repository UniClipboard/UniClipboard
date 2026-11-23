#!/usr/bin/env python3
"""Construct and verify customized, BoardUI-styled ULFO macOS DMG installers."""

from __future__ import annotations

import ctypes
import hashlib
import os
from pathlib import Path
import plistlib
import shutil
import struct
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]

VOLUME_NAME = "UniClipboard"
DMG_FORMAT = "ULFO"
WINDOW_X = 220
WINDOW_Y = 300
WINDOW_WIDTH = 600
WINDOW_HEIGHT = 380
TITLEBAR_HEIGHT = 28
ICON_SIZE = 96.0
TEXT_SIZE = 13.0
GRID_SPACING = 100.0
APP_POSITION = (164, 176)
APPLICATIONS_POSITION = (436, 176)

BACKGROUND_REL = ".background/background.tiff"
BACKGROUND_ASSET = ROOT / "packaging/macos/dmg-background.tiff"
VOLUME_ICON_ASSET = ROOT / "app/desktop/internal/appicons/icon.icns"

# Mac HFS epoch starts 1904-01-01 00:00:00 UTC (2082844800 seconds before 1970-01-01).
MAC_EPOCH_OFFSET = 2082844800


def _run(args, **kwargs):
    return subprocess.run(
        [str(x) for x in args],
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        **kwargs,
    )


def _sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        while chunk := f.read(1 << 20):
            h.update(chunk)
    return h.hexdigest()


def _cf_library():
    cf = ctypes.CDLL("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation")
    cf.CFURLCreateFromFileSystemRepresentation.argtypes = [
        ctypes.c_void_p,
        ctypes.c_char_p,
        ctypes.c_long,
        ctypes.c_bool,
    ]
    cf.CFURLCreateFromFileSystemRepresentation.restype = ctypes.c_void_p
    cf.CFURLCreateBookmarkData.argtypes = [
        ctypes.c_void_p,
        ctypes.c_void_p,
        ctypes.c_ulong,
        ctypes.c_void_p,
        ctypes.c_void_p,
        ctypes.POINTER(ctypes.c_void_p),
    ]
    cf.CFURLCreateBookmarkData.restype = ctypes.c_void_p
    cf.CFDataCreate.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_long]
    cf.CFDataCreate.restype = ctypes.c_void_p
    cf.CFDataGetLength.argtypes = [ctypes.c_void_p]
    cf.CFDataGetLength.restype = ctypes.c_long
    cf.CFDataGetBytePtr.argtypes = [ctypes.c_void_p]
    cf.CFDataGetBytePtr.restype = ctypes.c_void_p
    cf.CFURLCreateByResolvingBookmarkData.argtypes = [
        ctypes.c_void_p,
        ctypes.c_void_p,
        ctypes.c_ulong,
        ctypes.c_void_p,
        ctypes.c_void_p,
        ctypes.POINTER(ctypes.c_bool),
        ctypes.POINTER(ctypes.c_void_p),
    ]
    cf.CFURLCreateByResolvingBookmarkData.restype = ctypes.c_void_p
    cf.CFURLGetFileSystemRepresentation.argtypes = [
        ctypes.c_void_p,
        ctypes.c_bool,
        ctypes.c_char_p,
        ctypes.c_long,
    ]
    cf.CFURLGetFileSystemRepresentation.restype = ctypes.c_bool
    cf.CFRelease.argtypes = [ctypes.c_void_p]
    return cf


def create_bookmark(target: Path) -> bytes:
    """Create a native macOS CoreFoundation bookmark for a mounted volume file."""
    cf = _cf_library()
    encoded = str(target.resolve()).encode("utf-8")
    url = cf.CFURLCreateFromFileSystemRepresentation(None, encoded, len(encoded), False)
    if not url:
        raise RuntimeError("CFURLCreateFromFileSystemRepresentation failed")
    try:
        err = ctypes.c_void_p(0)
        data = cf.CFURLCreateBookmarkData(None, url, 0, None, None, ctypes.byref(err))
        if not data:
            raise RuntimeError("CFURLCreateBookmarkData failed")
        try:
            length = cf.CFDataGetLength(data)
            ptr = cf.CFDataGetBytePtr(data)
            return ctypes.string_at(ptr, length)
        finally:
            cf.CFRelease(data)
    finally:
        cf.CFRelease(url)


def resolve_bookmark(raw: bytes) -> Path:
    """Resolve a CoreFoundation bookmark without UI or mounting."""
    cf = _cf_library()
    cdata = cf.CFDataCreate(None, raw, len(raw))
    if not cdata:
        raise RuntimeError("CFDataCreate failed")
    try:
        stale = ctypes.c_bool(False)
        err = ctypes.c_void_p(0)
        # kCFURLBookmarkResolutionWithoutUIMask (1<<8) | WithoutMountingMask (1<<9)
        url = cf.CFURLCreateByResolvingBookmarkData(
            None, cdata, (1 << 8) | (1 << 9), None, None, ctypes.byref(stale), ctypes.byref(err)
        )
        if not url:
            raise ValueError("bookmark could not be resolved on mounted volume")
        try:
            buf = ctypes.create_string_buffer(2048)
            if not cf.CFURLGetFileSystemRepresentation(url, True, buf, len(buf)):
                raise ValueError("resolved bookmark has no filesystem path")
            return Path(buf.value.decode("utf-8"))
        finally:
            cf.CFRelease(url)
    finally:
        cf.CFRelease(cdata)


def _pascal_fixed(text: str, total_len: int) -> bytes:
    raw = text.encode("mac_roman")
    max_chars = total_len - 1
    if len(raw) > max_chars:
        raise ValueError(f"alias field exceeds {max_chars} bytes: {text}")
    return bytes([len(raw)]) + raw + b"\x00" * (max_chars - len(raw))


def _alias_tag(tag: int, value: bytes) -> bytes:
    pad = b"\x00" if len(value) % 2 else b""
    return struct.pack(">HH", tag, len(value)) + value + pad


def create_hfs_alias(volume_root: Path, target: Path, volume_name: str) -> bytes:
    """Build a classic macOS HFS+ Alias Manager v2 record for Finder icvp."""
    rel = target.relative_to(volume_root)
    parent = target.parent
    vol_stat = os.stat(volume_root)
    parent_stat = os.stat(parent)
    target_stat = os.stat(target)

    vol_ctime = int(getattr(vol_stat, "st_birthtime", vol_stat.st_ctime)) + MAC_EPOCH_OFFSET
    file_ctime = int(getattr(target_stat, "st_birthtime", target_stat.st_ctime)) + MAC_EPOCH_OFFSET
    vol_ctime &= 0xFFFFFFFF
    file_ctime &= 0xFFFFFFFF

    filename = target.name
    parent_name = parent.name
    carbon_path = volume_name + ":" + ":".join(rel.parts)
    posix_rel = rel.as_posix()
    posix_mount = "/Volumes/" + volume_name

    name_u16 = filename.encode("utf-16-be")
    vol_u16 = volume_name.encode("utf-16-be")

    extras = b"".join(
        [
            _alias_tag(0x0000, parent_name.encode("mac_roman")),
            _alias_tag(0x0001, struct.pack(">I", parent_stat.st_ino & 0xFFFFFFFF)),
            _alias_tag(0x0002, carbon_path.encode("mac_roman")),
            _alias_tag(0x000E, struct.pack(">H", len(filename)) + name_u16),
            _alias_tag(0x000F, struct.pack(">H", len(volume_name)) + vol_u16),
            _alias_tag(0x0012, posix_rel.encode("utf-8")),
            _alias_tag(0x0013, posix_mount.encode("utf-8")),
            struct.pack(">HH", 0xFFFF, 0),
        ]
    )

    total_len = 150 + len(extras)
    header = (
        struct.pack(
            ">4sHHH",
            b"\x00\x00\x00\x00",
            total_len,
            2,  # Alias v2
            0,  # Regular file
        )
        + _pascal_fixed(volume_name, 28)
        + struct.pack(
            ">I2sHI",
            vol_ctime,
            b"H+",
            5,  # Other/ejectable disk
            parent_stat.st_ino & 0xFFFFFFFF,
        )
        + _pascal_fixed(filename, 64)
        + struct.pack(
            ">II4s4shhIH10s",
            target_stat.st_ino & 0xFFFFFFFF,
            file_ctime,
            b"\x00\x00\x00\x00",
            b"\x00\x00\x00\x00",
            -1,
            -1,
            0x00000D02,
            0,
            b"\x00" * 10,
        )
    )
    assert len(header) == 150
    return header + extras


def _encode_ds_record(filename: str, code: bytes, dtype: bytes, value) -> bytes:
    assert len(code) == 4 and len(dtype) == 4
    name_bytes = filename.encode("utf-16-be")
    prefix = struct.pack(">I", len(filename)) + name_bytes + code + dtype
    if dtype == b"long":
        return prefix + struct.pack(">I", int(value))
    if dtype == b"shor":
        return prefix + struct.pack(">I", int(value) & 0xFFFF)
    if dtype == b"bool":
        return prefix + struct.pack(">B", 1 if value else 0)
    if dtype == b"blob":
        raw = bytes(value)
        return prefix + struct.pack(">I", len(raw)) + raw
    if dtype == b"ustr":
        u16 = str(value).encode("utf-16-be")
        return prefix + struct.pack(">I", len(str(value))) + u16
    raise ValueError(f"unsupported DS_Store type: {dtype!r}")


def build_ds_store(records: list[tuple[str, bytes, bytes, object]]) -> bytes:
    """Build a valid macOS .DS_Store Buddy-Allocator + single-leaf B-Tree file."""
    # Finder sorts B-tree keys by case-insensitive UTF-16 filename, then 4-byte code.
    ordered = sorted(records, key=lambda r: (r[0].lower(), r[1]))
    body = b"".join(_encode_ds_record(*rec) for rec in ordered)

    page_size = 0x1000  # 4096 bytes (standard Finder B-tree page size)
    leaf_payload = struct.pack(">II", 0, len(ordered)) + body
    if len(leaf_payload) > page_size:
        raise ValueError(
            f".DS_Store leaf page overflow ({len(leaf_payload)} > {page_size} bytes)"
        )
    leaf_block = leaf_payload.ljust(page_size, b"\x00")

    # Layout inside the Buddy Allocator address space (relative to file offset 4):
    #   0x0000..0x0020 (32 B, k=5):   Allocator header (reserved)
    #   0x0020..0x0040 (32 B, k=5):   Free
    #   0x0040..0x0060 (32 B, k=5):   Block 1 -> DSDB superblock
    #   0x0060..0x0080 (32 B, k=5):   Free
    #   0x0080..0x0100 (128 B, k=7):  Free
    #   0x0100..0x0200 (256 B, k=8):  Free
    #   0x0200..0x0400 (512 B, k=9):  Free
    #   0x0400..0x0800 (1024 B, k=10): Free
    #   0x0800..0x1000 (2048 B, k=11): Free
    #   0x1000..0x2000 (4096 B, k=12): Block 2 -> B-Tree leaf node
    #   0x2000..0x2800 (2048 B, k=11): Block 0 -> Buddy root directory
    #   0x2800..0x3000 (2048 B, k=11): Free
    #   0x3000..0x4000 (4096 B, k=12): Free
    # Total allocator space = 0x4000 (16384 B, 2^14).
    root_offset = 0x2000
    root_size = 0x0800
    dsdb_offset = 0x0040
    leaf_offset = 0x1000
    total_alloc = 0x4000

    # Block 1: DSDB superblock (32 bytes)
    dsdb_block = struct.pack(
        ">IIIII",
        2,  # root node block id
        0,  # tree height (0 = single leaf)
        len(ordered),  # total records
        1,  # total nodes
        page_size,
    ).ljust(0x20, b"\x00")

    # Block 0: Buddy root directory (2048 bytes)
    offsets = [0] * 256
    offsets[0] = root_offset | 11  # 2^11 = 2048
    offsets[1] = dsdb_offset | 5  # 2^5 = 32
    offsets[2] = leaf_offset | 12  # 2^12 = 4096

    free_lists: dict[int, list[int]] = {
        5: [0x0020, 0x0060],
        7: [0x0080],
        8: [0x0100],
        9: [0x0200],
        10: [0x0400],
        11: [0x0800, 0x2800],
        12: [0x3000],
    }

    root_parts = [
        struct.pack(">II", 3, 0),
        struct.pack(">256I", *offsets),
        struct.pack(">IB4sI", 1, 4, b"DSDB", 1),
    ]
    for k in range(32):
        entries = free_lists.get(k, [])
        root_parts.append(struct.pack(">I", len(entries)))
        if entries:
            root_parts.append(struct.pack(f">{len(entries)}I", *entries))
    root_block = b"".join(root_parts)
    assert len(root_block) <= root_size
    root_block = root_block.ljust(root_size, b"\x00")

    # Assemble file: 4-byte alignment prefix + allocator space (header at allocator offset 0).
    space = bytearray(total_alloc)
    # Allocator header at offset 0..32 inside allocator space (file offsets 4..36):
    space[0:32] = struct.pack(">4sIII16s", b"Bud1", root_offset, root_size, root_offset, b"\x00" * 16)
    space[dsdb_offset : dsdb_offset + len(dsdb_block)] = dsdb_block
    space[leaf_offset : leaf_offset + len(leaf_block)] = leaf_block
    space[root_offset : root_offset + len(root_block)] = root_block
    return struct.pack(">I", 1) + bytes(space)


def parse_ds_store(raw: bytes) -> dict[tuple[str, bytes], tuple[bytes, object]]:
    """Parse a .DS_Store file back into {(filename, code): (dtype, value)} for verification."""
    if len(raw) < 36 or raw[:8] != b"\x00\x00\x00\x01Bud1":
        raise ValueError("invalid .DS_Store magic")
    space = raw[4:]
    root_offset, root_size, root_offset_dup = struct.unpack_from(">III", space, 4)
    if root_offset != root_offset_dup or root_offset + root_size > len(space):
        raise ValueError("corrupted .DS_Store root header")

    root = space[root_offset : root_offset + root_size]
    block_count, _ = struct.unpack_from(">II", root, 0)
    padded_blocks = ((block_count + 255) // 256) * 256
    block_addrs = struct.unpack_from(f">{padded_blocks}I", root, 8)
    pos = 8 + 4 * padded_blocks
    (toc_count,) = struct.unpack_from(">I", root, pos)
    pos += 4
    toc = {}
    for _ in range(toc_count):
        (nlen,) = struct.unpack_from(">B", root, pos)
        pos += 1
        name = root[pos : pos + nlen].decode("ascii")
        pos += nlen
        (bid,) = struct.unpack_from(">I", root, pos)
        pos += 4
        toc[name] = bid

    def read_block(bid: int) -> bytes:
        addr = block_addrs[bid]
        k = addr & 0x1F
        off = addr & ~0x1F
        size = 1 << k
        return space[off : off + size]

    dsdb = read_block(toc["DSDB"])
    root_node, levels, _record_count, _node_count, _page_size = struct.unpack_from(
        ">IIIII", dsdb, 0
    )

    result: dict[tuple[str, bytes], tuple[bytes, object]] = {}

    def walk(node_id: int):
        node = read_block(node_id)
        p_flag, count = struct.unpack_from(">II", node, 0)
        cursor = 8
        for _ in range(count):
            if p_flag != 0:
                (child_id,) = struct.unpack_from(">I", node, cursor)
                cursor += 4
                walk(child_id)
            (nchars,) = struct.unpack_from(">I", node, cursor)
            cursor += 4
            fname = node[cursor : cursor + 2 * nchars].decode("utf-16-be")
            cursor += 2 * nchars
            code = node[cursor : cursor + 4]
            dtype = node[cursor + 4 : cursor + 8]
            cursor += 8
            if dtype == b"long":
                (val,) = struct.unpack_from(">I", node, cursor)
                cursor += 4
            elif dtype == b"shor":
                (val,) = struct.unpack_from(">I", node, cursor)
                val &= 0xFFFF
                cursor += 4
            elif dtype == b"bool":
                (val,) = struct.unpack_from(">B", node, cursor)
                val = bool(val)
                cursor += 1
            elif dtype == b"blob":
                (blen,) = struct.unpack_from(">I", node, cursor)
                cursor += 4
                val = node[cursor : cursor + blen]
                cursor += blen
            elif dtype == b"ustr":
                (ulen,) = struct.unpack_from(">I", node, cursor)
                cursor += 4
                val = node[cursor : cursor + 2 * ulen].decode("utf-16-be")
                cursor += 2 * ulen
            else:
                raise ValueError(f"unsupported DS_Store record type: {dtype!r}")
            result[(fname, code)] = (dtype, val)
        if p_flag != 0:
            walk(p_flag)

    walk(root_node)
    return result


def _detach(mountpoint: Path):
    last_error = None
    for _ in range(12):
        res = subprocess.run(
            ["hdiutil", "detach", "-quiet", str(mountpoint)],
            capture_output=True,
            text=True,
        )
        if res.returncode == 0:
            return
        last_error = res.stderr or res.stdout
        time.sleep(0.5)
    res = subprocess.run(
        ["hdiutil", "detach", "-force", "-quiet", str(mountpoint)],
        capture_output=True,
        text=True,
    )
    if res.returncode != 0:
        raise RuntimeError(f"failed to detach DMG: {last_error or res.stderr}")


def _set_file_attr(attr: str, path: Path):
    setfile = shutil.which("SetFile")
    if setfile:
        _run([setfile, "-a", attr, path])


def build_dmg(
    bundle: Path,
    output_dmg: Path,
    *,
    volume_name: str = VOLUME_NAME,
    background: Path = BACKGROUND_ASSET,
    volume_icon: Path = VOLUME_ICON_ASSET,
) -> Path:
    """Create a BoardUI-styled, ULFO-compressed macOS DMG with Applications drop link."""
    bundle = Path(bundle).resolve()
    output_dmg = Path(output_dmg).resolve()
    background = Path(background).resolve()
    volume_icon = Path(volume_icon).resolve()

    if not bundle.is_dir() or bundle.suffix != ".app":
        raise ValueError(f"expected .app bundle: {bundle}")
    if not background.is_file():
        raise ValueError(f"missing DMG background asset: {background}")
    if not volume_icon.is_file():
        raise ValueError(f"missing DMG volume icon asset: {volume_icon}")
    if output_dmg.exists():
        raise ValueError(f"DMG output already exists: {output_dmg}")

    with tempfile.TemporaryDirectory(prefix="unic-dmg-", dir=output_dmg.parent) as tmp:
        work = Path(tmp)
        staging = work / "staging"
        staging.mkdir()

        # ditto preserves macOS signatures, stapled tickets, xattrs, and symlinks faithfully.
        _run(["ditto", bundle, staging / bundle.name])
        (staging / "Applications").symlink_to("/Applications")

        bg_dir = staging / ".background"
        bg_dir.mkdir()
        shutil.copyfile(background, bg_dir / "background.tiff")
        shutil.copyfile(volume_icon, staging / ".VolumeIcon.icns")
        (staging / ".metadata_never_index").write_bytes(b"")

        # Reserve space on HFS+ for the real .DS_Store and .fseventsd/no_log before sizing.
        (staging / ".DS_Store").write_bytes(b"\x00" * 65536)

        rw_dmg = work / "rw.dmg"
        _run(
            [
                "hdiutil",
                "create",
                "-quiet",
                "-fs",
                "HFS+",
                "-fsargs",
                "-c c=64,a=16,e=16",
                "-volname",
                volume_name,
                "-srcfolder",
                staging,
                "-format",
                "UDRW",
                rw_dmg,
            ]
        )

        mountpoint = work / "mnt"
        mountpoint.mkdir()
        _run(
            [
                "hdiutil",
                "attach",
                "-quiet",
                "-readwrite",
                "-noverify",
                "-noautoopen",
                "-mountpoint",
                mountpoint,
                rw_dmg,
            ]
        )
        try:
            fsevents = mountpoint / ".fseventsd"
            fsevents.mkdir(exist_ok=True)
            (fsevents / "no_log").write_bytes(b"")

            mounted_bg = mountpoint / BACKGROUND_REL
            _set_file_attr("V", mountpoint / ".background")
            _set_file_attr("V", mountpoint / ".VolumeIcon.icns")
            _set_file_attr("C", mountpoint)

            bg_alias = create_hfs_alias(mountpoint, mounted_bg, volume_name)
            bg_bookmark = create_bookmark(mounted_bg)

            win_h = WINDOW_HEIGHT + TITLEBAR_HEIGHT
            bwsp = plistlib.dumps(
                {
                    "ContainerShowSidebar": False,
                    "ShowPathbar": False,
                    "ShowSidebar": False,
                    "ShowStatusBar": False,
                    "ShowTabView": False,
                    "ShowToolbar": False,
                    "SidebarWidth": 0,
                    "WindowBounds": f"{{{{{WINDOW_X}, {WINDOW_Y}}}, {{{WINDOW_WIDTH}, {win_h}}}}}",
                },
                fmt=plistlib.FMT_BINARY,
            )
            icvp = plistlib.dumps(
                {
                    "arrangeBy": "none",
                    "backgroundColorBlue": 247.0 / 255.0,
                    "backgroundColorGreen": 247.0 / 255.0,
                    "backgroundColorRed": 247.0 / 255.0,
                    "backgroundImageAlias": bg_alias,
                    "backgroundImageBookmark": bg_bookmark,
                    "backgroundType": 2,
                    "gridOffsetX": 0.0,
                    "gridOffsetY": 0.0,
                    "gridSpacing": GRID_SPACING,
                    "iconSize": ICON_SIZE,
                    "labelOnBottom": True,
                    "showIconPreview": True,
                    "showItemInfo": False,
                    "textSize": TEXT_SIZE,
                    "viewOptionsVersion": 1,
                },
                fmt=plistlib.FMT_BINARY,
            )
            fwi0 = struct.pack(
                ">HHHH4s4s",
                WINDOW_Y,
                WINDOW_X,
                WINDOW_Y + win_h,
                WINDOW_X + WINDOW_WIDTH,
                b"icnv",
                b"\x00\x00\x00\x00",
            )

            def iloc(x: int, y: int) -> bytes:
                return struct.pack(">II6sH", x, y, b"\xff\xff\xff\xff\xff\xff", 0)

            ds_bytes = build_ds_store(
                [
                    (".", b"bwsp", b"blob", bwsp),
                    (".", b"fwi0", b"blob", fwi0),
                    (".", b"icvp", b"blob", icvp),
                    (".", b"vSrn", b"long", 1),
                    (bundle.name, b"Iloc", b"blob", iloc(*APP_POSITION)),
                    ("Applications", b"Iloc", b"blob", iloc(*APPLICATIONS_POSITION)),
                ]
            )
            (mountpoint / ".DS_Store").write_bytes(ds_bytes)
            os.sync()
        finally:
            _detach(mountpoint)

        _run(
            [
                "hdiutil",
                "convert",
                "-quiet",
                rw_dmg,
                "-format",
                DMG_FORMAT,
                "-o",
                output_dmg,
            ]
        )
    return output_dmg


def verify_dmg(
    dmg_path: Path,
    *,
    expected_app_name: str = "UniClipboard.app",
    expected_bundle_files: dict[str, str] | None = None,
    background: Path = BACKGROUND_ASSET,
    volume_icon: Path = VOLUME_ICON_ASSET,
) -> dict:
    """Mount a built DMG read-only and verify ULFO format, layout, artwork, and .DS_Store."""
    dmg_path = Path(dmg_path).resolve()
    info = _run(["hdiutil", "imageinfo", dmg_path]).stdout
    fmt = ""
    for line in info.splitlines():
        if line.strip().startswith("Format:"):
            fmt = line.split(":", 1)[1].strip()
            break
    if fmt != DMG_FORMAT:
        raise ValueError(f"expected DMG format {DMG_FORMAT}, got {fmt!r}")

    with tempfile.TemporaryDirectory(prefix="unic-dmg-verify-") as tmp:
        mnt = Path(tmp) / "mnt"
        mnt.mkdir()
        _run(
            [
                "hdiutil",
                "attach",
                "-quiet",
                "-readonly",
                "-nobrowse",
                "-noverify",
                "-mountpoint",
                mnt,
                dmg_path,
            ]
        )
        try:
            app_dir = mnt / expected_app_name
            if not app_dir.is_dir():
                raise ValueError(f"missing {expected_app_name} in DMG root")
            if expected_bundle_files is not None:
                for rel_name, expected_digest in expected_bundle_files.items():
                    candidate = app_dir / rel_name
                    if (
                        candidate.is_symlink()
                        or not candidate.is_file()
                        or _sha256(candidate) != expected_digest
                    ):
                        raise ValueError(f"DMG bundle file mismatch: {rel_name}")
            apps_link = mnt / "Applications"
            if not apps_link.is_symlink() or os.readlink(apps_link) != "/Applications":
                raise ValueError("missing or invalid Applications symlink in DMG root")
            mounted_bg = mnt / BACKGROUND_REL
            if not mounted_bg.is_file() or _sha256(mounted_bg) != _sha256(background):
                raise ValueError("DMG background image mismatch")
            mounted_icon = mnt / ".VolumeIcon.icns"
            if not mounted_icon.is_file() or _sha256(mounted_icon) != _sha256(volume_icon):
                raise ValueError("DMG volume icon mismatch")

            ds_records = parse_ds_store((mnt / ".DS_Store").read_bytes())
            _, bwsp_raw = ds_records[(".", b"bwsp")]
            _, icvp_raw = ds_records[(".", b"icvp")]
            bwsp = plistlib.loads(bwsp_raw)
            icvp = plistlib.loads(icvp_raw)

            if icvp.get("backgroundType") != 2 or icvp.get("iconSize") != ICON_SIZE:
                raise ValueError("unexpected Finder icon view settings in .DS_Store")
            if not isinstance(icvp.get("backgroundImageAlias"), bytes):
                raise ValueError("missing backgroundImageAlias in .DS_Store")
            bookmark = icvp.get("backgroundImageBookmark")
            if not isinstance(bookmark, bytes):
                raise ValueError("missing backgroundImageBookmark in .DS_Store")
            resolved = resolve_bookmark(bookmark)
            if resolved.resolve() != mounted_bg.resolve():
                raise ValueError(
                    f"background bookmark resolved to {resolved}, expected {mounted_bg}"
                )

            app_x, app_y = struct.unpack_from(">II", ds_records[(expected_app_name, b"Iloc")][1], 0)
            apps_x, apps_y = struct.unpack_from(">II", ds_records[("Applications", b"Iloc")][1], 0)
            if (app_x, app_y) != APP_POSITION or (apps_x, apps_y) != APPLICATIONS_POSITION:
                raise ValueError("unexpected icon coordinates in .DS_Store")

            return {
                "format": fmt,
                "window_bounds": bwsp.get("WindowBounds"),
                "app_position": [app_x, app_y],
                "applications_position": [apps_x, apps_y],
                "background_sha256": _sha256(mounted_bg),
                "volume_icon_sha256": _sha256(mounted_icon),
            }
        finally:
            _detach(mnt)
