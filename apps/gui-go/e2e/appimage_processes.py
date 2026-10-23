"""AppImage process ownership across WebKit's private mount namespace."""
import os
from pathlib import Path
import re

WEBKIT_HELPERS = ('WebKitWebProcess', 'WebKitNetworkProcess', 'WebKitGPUProcess')


def appdir_from_executable(executable):
    match = re.match(r'(/.+?/(?:\.mount_[^/]+|appimage_extracted_[^/]+))/', executable)
    return match[1] if match else None


def same_file(left, right):
    try:
        return os.path.samefile(left, right)
    except OSError:
        return False


def belongs_to_appimage(pid, executable, appdir, compare=same_file):
    if executable.startswith(str(appdir) + '/'):
        return True
    name = Path(executable).name
    return name in WEBKIT_HELPERS and compare(f'/proc/{pid}/exe', Path(appdir) / 'usr/h' / name)


def library_from_mapping(fields, appdir):
    """Resolve namespace aliases only when the kernel's mapped inode matches."""
    path = fields[5].replace(' (deleted)', '')
    runtime_root = (Path(appdir) / 'usr/share/uniclipboard/webkit-runtime-root').read_text().strip()
    if not path.startswith(runtime_root + '/'):
        return path
    candidate = (Path(appdir) / 'usr' / path[len(runtime_root) + 1:]).resolve(strict=True)
    candidate.relative_to(Path(appdir).resolve())
    stat = candidate.stat()
    device = ':'.join(f'{n:x}' for n in (os.major(stat.st_dev), os.minor(stat.st_dev)))
    mapped_device = ':'.join(f'{int(n, 16):x}' for n in fields[3].split(':'))
    if device != mapped_device or stat.st_ino != int(fields[4]):
        raise ValueError(f'mapped library does not belong to the AppImage: {path}')
    return str(candidate)
