"""Shared Windows portable archive layout for local and CI builds."""
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]


def write_portable_archive(destination, product, exe, daemon, helper):
    with zipfile.ZipFile(destination, 'w', zipfile.ZIP_DEFLATED) as archive:
        archive.write(exe, f'{product}.exe')
        archive.write(daemon, 'uniclipd.exe')
        archive.write(helper, 'uniclip-quick-panel.exe')
        archive.writestr('portable.dat', '')
        archive.write(ROOT / 'packaging/windows/portable/README.txt', 'README.txt')
