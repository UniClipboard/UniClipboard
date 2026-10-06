#!/usr/bin/env python3
"""Text-fidelity check of the ported Tauri "already installed" page against the pinned Tauri CLI template.

  python apps/gui-go/e2e/installer_template_diff.py --out <new dir>

Extracts the NSIS template embedded in the Tauri CLI binary (the version in bun.lock, installed under node_modules),
cuts out PageReinstall / PageLeaveReinstall and the English strings the page uses, cuts the same text out of
apps/gui-go/windows/installer.nsi, and compares the two unified diffs against `installer_template_diff.expected`,
the reviewed list of intended adaptations (no WiX branch, HKCU instead of SHCTX, main binary name already has
`.exe`, no ALLOWDOWNGRADES switch, restore of the login item). Any other difference fails.

This is text evidence only. It does not run the installer: the page and the uninstall step were never executed on
Windows by this script.
"""
import argparse
import difflib
import hashlib
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
CLI_NODE = ROOT / 'node_modules/@tauri-apps/cli-darwin-arm64/cli.darwin-arm64.node'
NSI = ROOT / 'apps/gui-go/windows/installer.nsi'
EXPECTED = Path(__file__).with_name('installer_template_diff.expected')
STRINGS = ['addOrReinstall', 'alreadyInstalled', 'alreadyInstalledLong', 'chooseMaintenanceOption', 'choowHowToInstall',
           'dontUninstall', 'dontUninstallDowngrade', 'newerVersionInstalled', 'older', 'olderOrUnknownVersionInstalled',
           'unableToUninstall', 'uninstallApp', 'uninstallBeforeInstalling', 'unknown']


def cli_version():
    lock = (ROOT / 'bun.lock').read_text()
    return re.search(r'"@tauri-apps/cli@([0-9.]+)"', lock).group(1)


def template_text(blob):
    """The template text: from `Unicode true` to the last NSIS macro of the file, then the English strings."""
    start = blob.index(b'Unicode true')
    end = blob.index(b'!macroend', blob.index(b'!macro SetShortcutTarget', start))
    return blob[start:end].decode('utf-8'), start


def page_block(text):
    a = text.index('Function PageReinstall\n')
    b = text.index('reinst_done:\nFunctionEnd', a) + len('reinst_done:\nFunctionEnd')
    return text[a:b].splitlines()


def english_strings(blob):
    out = {}
    for name in STRINGS:
        m = re.search(rb'LangString ' + name.encode() + rb' \$\{LANG_ENGLISH\} "[^\n]*"', blob)
        if not m:
            sys.exit(f'English string {name} not found in the Tauri CLI binary')
        out[name] = m.group(0).decode('utf-8')
    return out


def ported_strings(text):
    out = {}
    for name in STRINGS:
        m = re.search(r'^LangString ' + name + r' \$\{LANG_ENGLISH\} "[^\n]*"$', text, re.M)
        if not m:
            sys.exit(f'English string {name} not found in installer.nsi')
        out[name] = m.group(0)
    return out


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--update-expected', action='store_true', help='rewrite the expected diff (review it before committing)')
    args = parser.parse_args()
    if args.out.exists() and any(args.out.iterdir()):
        sys.exit(f'{args.out} is not empty: pick a new directory, earlier artifacts are not overwritten')
    args.out.mkdir(parents=True, exist_ok=True)
    blob = CLI_NODE.read_bytes()
    ttext, offset = template_text(blob)
    tpl = page_block(ttext) + [''] + list(english_strings(blob).values())
    nsi = NSI.read_text()
    ours = page_block(nsi) + [''] + list(ported_strings(nsi).values())
    diff = '\n'.join(difflib.unified_diff(tpl, ours, 'tauri-cli-template', 'installer.nsi', lineterm='', n=0)) + '\n'
    (args.out / 'template-page.txt').write_text('\n'.join(tpl) + '\n')
    (args.out / 'ported-page.txt').write_text('\n'.join(ours) + '\n')
    (args.out / 'diff.txt').write_text(diff)
    (args.out / 'source.json').write_text(json.dumps({
        'tauriCli': cli_version(), 'binary': str(CLI_NODE.relative_to(ROOT)), 'binarySha256': hashlib.sha256(blob).hexdigest(),
        'templateByteOffset': offset, 'installerNsiSha256': hashlib.sha256(NSI.read_bytes()).hexdigest(),
        'note': 'text-fidelity evidence only; the installer page was not run'}, indent=2) + '\n')
    if args.update_expected:
        EXPECTED.write_text(diff)
        print('expected diff rewritten; review it')
        return
    if diff != EXPECTED.read_text():
        sys.exit(f'FAIL: the page differs from the template in ways not listed in {EXPECTED.name}; see {args.out}/diff.txt')
    print(f'PASS: only the reviewed adaptations differ (tauri-cli {cli_version()}); see {args.out}/diff.txt')


if __name__ == '__main__':
    main()
