#!/usr/bin/env python3
"""Key-loss unlock E2E: wrong passphrase refusal, then the right one, in the real Wails WebView.

The profile is created and filled through the CLI, then the daemon is stopped and the file-backed key facility
entry (the test profile's only master key copy, inside the throwaway HOME) is deleted. The next GUI launch boots
a fresh daemon that cannot unlock from the keyring, so the shared unlock page has to fall back to the passphrase
form. The in-WebView driver submits a wrong passphrase (localized refusal, content still locked) and then the
right one. A second launch shows whether the successful unlock left the keyring usable.
"""
import argparse
import glob
import json
import os
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from config_package_run import launch  # noqa: E402
from file_preview_run import PASSPHRASE, cli  # noqa: E402
from receipt import write_receipt  # noqa: E402
from run import ROOT, isolated_env  # noqa: E402

MARKER = 'unlock-e2e-marker-entry'
STEPS = ['locked-screen',
         # the generated binding itself, before any UI interaction
         'binding-wrong-passphrase-typed', 'wrapper-wrong-passphrase-user-facing',
         'binding-malformed-call-is-system-error', 'binding-still-locked-after-probes',
         # the shared pages on top of it
         'wrong-passphrase-rejected', 'still-locked-after-wrong-passphrase', 'right-passphrase-unlocked',
         'content-lock-changed-event']


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--mode', choices=['key-deleted', 'keychain-denied'], required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    home = tempfile.mkdtemp(prefix='uc-gui-go-')
    profile = 'gui-go-' + os.path.basename(home)
    path = str(ROOT / 'target/debug') + ':' + os.environ['PATH']
    cli_env = isolated_env(home, profile, {'PATH': path})
    base = {'PATH': path, 'UC_GPUI_QUICK_PANEL': '0', 'UC_GUI_GO_ISOLATED': '1', 'UC_GUI_GO_EXIT_MODE': 'full', 'UC_GUI_GO_E2E_SECRET': PASSPHRASE}
    results = {'home': home, 'profile': profile, 'passed': False}
    try:
        cli(cli_env, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'unlock-a')
        cli(cli_env, 'send', MARKER, check=False)  # no peers: exit 1, but the entry lands in local history
        cli(cli_env, '--json', 'stop')
        keys = glob.glob(os.path.join(home, 'Library/Application Support', '*', 'keyring', '*.bin'))
        assert len(keys) == 1 and home in keys[0], f'unexpected key facility layout: {keys}'
        if args.mode == 'key-deleted':
            os.remove(keys[0])
        else:
            base['UC_GUI_GO_E2E_KEYRING_UNLOCK'] = 'denied'  # e2e-only host seam: the keychain prompt is refused
        results['keyringEntry'] = {'path': os.path.relpath(keys[0], home), 'mode': args.mode}
        ev1 = out / f'unlock-{args.mode}-native.jsonl'
        ev1.write_text('')
        seen = launch(isolated_env(home, profile, dict(base, UC_GUI_GO_EVIDENCE=str(ev1), UC_GUI_GO_E2E_PHASE='unlock-wrong')),
                      ev1, out / f'unlock-{args.mode}-gui.log', STEPS, 150)
        results['screen'] = seen['locked-screen']['detail']['screen']
        results['steps'] = {s: seen[s].get('detail') for s in STEPS}
        if args.mode == 'key-deleted':
            # The first launch quit fully, so this daemon is new again: the recovered key must be in the keyring.
            ev2 = out / 'unlock-restart-native.jsonl'
            ev2.write_text('')
            seen2 = launch(isolated_env(home, profile, dict(base, UC_GUI_GO_EVIDENCE=str(ev2), UC_GUI_GO_E2E_PHASE='unlock-restart', UC_GUI_GO_E2E_SECRET=MARKER)),
                           ev2, out / 'unlock-restart-gui.log', ['restart-first-screen', 'history-restored-after-recovery'], 150)
            results['restart'] = {k: seen2[k].get('detail') for k in ('restart-first-screen', 'history-restored-after-recovery')}
            results['keyringRestoredByRecovery'] = os.path.exists(keys[0])
            assert results['keyringRestoredByRecovery'], 'recovery did not store the key back'
        results['passed'] = True
    finally:
        cli(cli_env, '--json', 'stop', check=False, timeout=80)
        (out / f'unlock-{args.mode}-assertions.json').write_text(json.dumps(results, indent=2) + '\n')
        if results['passed']:
            write_receipt(out, f'unlock-{args.mode}',
                          scope={'commands': ['get_content_unlocked', 'unlock_content'], 'events': ['content-lock-changed'],
                                 'layers': ['shared pages', 'ipc wrapper', 'generated binding', 'Go service', 'daemon']},
                          binaries={'gui': ROOT / 'target/gui-go/UniClipboardGoE2E.app/Contents/MacOS/gui-go', 'daemon': ROOT / 'target/debug/uniclipd'},
                          before={'locked': results['steps']['locked-screen']}, after={'steps': results['steps']},
                          assertions=results)
    print(json.dumps(results, indent=2))
    assert results['passed']


if __name__ == '__main__':
    main()
