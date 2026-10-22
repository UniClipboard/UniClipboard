#!/usr/bin/env python3
"""Assemble the independently identified UI child before signing the outer app."""
import argparse
import plistlib
import shutil
from pathlib import Path


def assemble_ui_bundle(bundle):
    contents = bundle / 'Contents'
    child = contents / 'Helpers/UniClipboardUI.app/Contents'
    (child / 'MacOS').mkdir(parents=True, exist_ok=True)
    (child / 'Resources').mkdir(parents=True, exist_ok=True)
    shutil.copy2(contents / 'MacOS/gui-go', child / 'MacOS/gui-go')
    (child / 'MacOS/gui-go').chmod(0o755)
    plist = plistlib.loads((contents / 'Info.plist').read_bytes())
    plist['CFBundleIdentifier'] += '.ui'
    # The child renders the product's windows and Dock icon, so keep the product's visible name.
    plist['CFBundleExecutable'] = 'gui-go'
    (child / 'Info.plist').write_bytes(plistlib.dumps(plist))
    icon = contents / 'Resources/icon.icns'
    if icon.exists():
        shutil.copy2(icon, child / 'Resources/icon.icns')
    (child / 'PkgInfo').write_text('APPL????')
    return child.parent


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('bundle', type=Path)
    print(assemble_ui_bundle(parser.parse_args().bundle.resolve()))
