#!/usr/bin/env python3
"""Windows installer "already installed" page E2E (slice 17b2). STATUS: AUTHORED, NOT YET RUN.

No Windows host was available when this was written (runner uniclipboard-windows-x64-vm-01: offline, 2026-10-06), so
nothing here has executed. It drives the REAL setup.exe windows with pywinauto (UI Automation / Win32 backend, a
mature library), not stubs or static strings. Until a run produces artifacts, the page and the uninstall step stay
native-OPEN; macOS-side evidence is only the compiled installer and the template text diff
(`library/e2e-windows-17b2/`).

Run on a DEDICATED, disposable Windows session. It installs and uninstalls UniClipboard under HKCU, and the installer's
hook force-stops `UniClipboard.exe`/`uniclipd.exe` by image name, so it refuses to start unless
UC_GUI_GO_E2E_DEDICATED_HOST=1 and refuses when an installation or process already exists. The installed
application is never started (the Finish page "Run" box is cleared, no /R).

Three installers of the same packaging with different versions are needed (the exe/daemon may be the fixture: only
the installer is under test). Build them from a package dir made by package_windows.py:

    makensis -DPRODUCTNAME=UniClipboard -DVERSION=<v> -DVERSIONWITHBUILD=<v>.0 -DMANUFACTURER=uniclipboard \
      -DBUNDLEID=app.uniclipboard.desktop -DMAINBINARYNAME=UniClipboard.exe -DSRC_MAIN=<pkg>\\UniClipboard.exe \
      -DSRC_DAEMON=<daemon> -DICON=apps\\gui\\src-tauri\\icons\\icon.ico -DOUTFILE=<out>\\setup-<v>.exe \
      -DHOOKS=apps\\gui\\src-tauri\\windows\\installer-hooks.nsh -DPLUGINDIR=<pkg>\\plugins apps\\gui-go\\windows\\installer.nsi

    pip install pywinauto
    set UC_GUI_GO_E2E_DEDICATED_HOST=1
    python windows_installer_reinstall_run.py --out <new dir> --old setup-1.0.0.exe --same setup-1.1.0.exe --new setup-1.2.0.exe

A sentinel value in the Uninstall key tells "the uninstaller ran" (the key is deleted and rewritten) from "files were
overwritten" (the value survives).

  P1 same version, page shows add/reinstall (default) vs uninstall; add/reinstall keeps the sentinel
  P2 same version, choose uninstall: the uninstaller runs (sentinel gone) and, like the Tauri template, the install continues
  P3 older installed: default is "Uninstall before installing" (sentinel gone, login item kept); the second choice keeps the sentinel
  P4 newer installed (downgrade): the second choice is disabled; the first is the only way and replaces the installed version
  P5 Cancel on the page leaves the installation untouched
  P6 Back from the page returns to Welcome and Next returns to the page
  P7 /P /UPDATE over an installed same version: no page, no uninstall
  P8 /S upgrade: no page, sentinel kept; /S downgrade: refused (non-zero exit), installed version unchanged
  P9 cancelling the uninstaller wizard returns to the page and installs nothing
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

UNINST = r'HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\UniClipboard'
RUN = r'HKCU\Software\Microsoft\Windows\CurrentVersion\Run'
SENTINEL = 'E2ESentinel'
TEXT = {  # the English strings of the page (Tauri template)
    'same': ('Add/Reinstall components', 'Uninstall UniClipboard'),
    'older': ('Uninstall before installing', 'Do not uninstall'),
    'newer': ('Uninstall before installing', 'Do not uninstall (Downgrading without uninstall is disabled for this installer)'),
}


def reg(*a):
    return subprocess.run(['reg', *a], capture_output=True, text=True)


def reg_value(key, name):
    r = reg('query', key, '/v', name)
    if r.returncode != 0:
        return None
    for line in r.stdout.splitlines():
        parts = line.split(None, 2)
        if parts and parts[0] == name:
            return parts[2].strip() if len(parts) > 2 else ''
    return None


def running(image):
    return image.lower() in subprocess.run(['tasklist', '/FI', f'IMAGENAME eq {image}'], capture_output=True, text=True).stdout.lower()


class Setup:
    """One interactive run of a setup.exe, driven through its real windows. Only the Popen of this run is stopped."""

    def __init__(self, exe, inst, args=()):
        from pywinauto import Application
        self.proc = subprocess.Popen([str(exe), *args, f'/D={inst}'])
        self.app = Application(backend='win32').connect(process=self.proc.pid, timeout=60)
        self.win = self.app.window(class_name='#32770', top_level_only=True)
        self.win.wait('visible', timeout=60)

    def button(self, ident):  # NSIS: 1 = Next/Install/Finish, 2 = Cancel, 3 = Back
        return self.win.child_window(control_id=ident, class_name='Button')

    def click(self, ident):
        self.button(ident).wait('enabled', timeout=60).click()

    def radio(self, text):
        return self.win.child_window(title=text, class_name='Button')

    def page_is(self, text, timeout=30):
        try:
            self.radio(text).wait('exists', timeout=timeout)
            return True
        except Exception:
            return False

    def finish(self):
        """Walk the rest of the wizard (directory, install, finish) without starting the application."""
        for _ in range(10):
            if self.proc.poll() is not None:
                return
            try:
                run_box = self.win.child_window(title_re='.*Run UniClipboard.*', class_name='Button')
                if run_box.exists(timeout=1):
                    if run_box.get_check_state():
                        run_box.click()
                    self.click(1)
                    break
                self.click(1)
            except Exception:
                time.sleep(1)
        self.wait(120)

    def wait(self, timeout):
        try:
            return self.proc.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            self.proc.terminate()  # forced stop of this run's own installer process only
            return None


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    for name in ('old', 'same', 'new'):
        parser.add_argument(f'--{name}', type=Path, required=True, help=f'setup.exe whose version is {name} relative to --same')
    args = parser.parse_args()
    if os.name != 'nt' or os.environ.get('UC_GUI_GO_E2E_DEDICATED_HOST') != '1':
        sys.exit('refusing to run: needs a dedicated Windows test host (UC_GUI_GO_E2E_DEDICATED_HOST=1)')
    if reg('query', UNINST).returncode == 0 or running('UniClipboard.exe') or running('uniclipd.exe'):
        sys.exit('refusing to run: a UniClipboard installation or process already exists on this host')
    if args.out.exists() and any(args.out.iterdir()):
        sys.exit(f'{args.out} is not empty: pick a new directory, earlier artifacts are not overwritten')
    args.out.mkdir(parents=True, exist_ok=True)
    checks = []
    inst = Path(tempfile.mkdtemp(prefix='uc-gui-go-inst-')) / 'app'

    def check(name, ok, detail=None):
        checks.append({'check': name, 'ok': bool(ok), 'detail': detail})
        print(('PASS ' if ok else 'FAIL ') + name, flush=True)

    def fresh(setup):
        """Silent install of `setup`, then plant the sentinel and a login item."""
        reg('delete', UNINST, '/f')
        reg('delete', RUN, '/v', 'UniClipboard', '/f')
        subprocess.run([str(setup), '/S', f'/D={inst}'], timeout=300)
        reg('add', UNINST, '/v', SENTINEL, '/d', '1', '/f')
        reg('add', RUN, '/v', 'UniClipboard', '/d', str(inst / 'UniClipboard.exe'), '/f')

    def sentinel():
        return reg_value(UNINST, SENTINEL) == '1'

    def run_value():
        return reg_value(RUN, 'UniClipboard') is not None

    def version():
        return reg_value(UNINST, 'DisplayVersion')

    def uninstall_all():
        if (inst / 'uninstall.exe').exists():
            subprocess.run([str(inst / 'uninstall.exe'), '/S', f'_?={inst}'], timeout=300)

    def to_page(setup, key):
        s = Setup(setup, inst)
        s.click(1)  # Welcome -> reinstall page
        check(f'{key} page shows the choices', s.page_is(TEXT[kind_of[key]][0]) and s.page_is(TEXT[kind_of[key]][1]), TEXT[kind_of[key]])
        return s

    kind_of = {'P1': 'same', 'P2': 'same', 'P3a': 'older', 'P3b': 'older', 'P4': 'newer', 'P5': 'same', 'P6': 'same', 'P9': 'older'}
    try:
        # P1
        fresh(args.same)
        s = to_page(args.same, 'P1')
        check('P1 add/reinstall is the default', s.radio(TEXT['same'][0]).get_check_state() == 1)
        s.click(1)
        s.finish()
        check('P1 add/reinstall keeps the sentinel and the files', sentinel() and (inst / 'UniClipboard.exe').is_file(), s.proc.returncode)
        uninstall_all()
        # P2
        fresh(args.same)
        s = to_page(args.same, 'P2')
        s.radio(TEXT['same'][1]).click()
        s.click(1)
        s.finish()
        check('P2 uninstall chosen: the uninstaller ran (sentinel gone) and the install continued', not sentinel() and (inst / 'UniClipboard.exe').is_file() and version() is not None)
        uninstall_all()
        # P3a: default (uninstall first)
        fresh(args.old)
        s = to_page(args.new, 'P3a')
        check('P3a upgrade: "Uninstall before installing" is the default', s.radio(TEXT['older'][0]).get_check_state() == 1)
        s.click(1)
        s.finish()
        check('P3a uninstalled first: sentinel gone, new version installed, login item kept',
              not sentinel() and (inst / 'UniClipboard.exe').is_file() and run_value())
        uninstall_all()
        # P3b: do not uninstall
        fresh(args.old)
        s = to_page(args.new, 'P3b')
        s.radio(TEXT['older'][1]).click()
        s.click(1)
        s.finish()
        check('P3b "Do not uninstall": sentinel kept, files replaced', sentinel() and (inst / 'UniClipboard.exe').is_file())
        uninstall_all()
        # P4
        fresh(args.new)
        s = to_page(args.old, 'P4')
        check('P4 downgrade: second choice disabled, first checked', not s.radio(TEXT['newer'][1]).is_enabled() and s.radio(TEXT['newer'][0]).get_check_state() == 1)
        s.click(1)
        s.finish()
        check('P4 downgrade replaced the installed version after uninstalling', not sentinel() and (inst / 'UniClipboard.exe').is_file())
        uninstall_all()
        # P5
        fresh(args.same)
        s = to_page(args.same, 'P5')
        s.click(2)
        try:
            s.win.child_window(title_re='.*(Yes|OK).*', class_name='Button').click()  # "Are you sure you want to quit?"
        except Exception:
            pass
        rc = s.wait(60)
        check('P5 cancel leaves the installation untouched', rc not in (None, 0) and sentinel() and (inst / 'UniClipboard.exe').is_file(), rc)
        uninstall_all()
        # P6
        fresh(args.same)
        s = to_page(args.same, 'P6')
        s.click(3)
        s.click(1)
        check('P6 Back then Next returns to the page', s.page_is(TEXT['same'][0]))
        s.click(2)
        try:
            s.win.child_window(title_re='.*(Yes|OK).*', class_name='Button').click()
        except Exception:
            pass
        s.wait(60)
        uninstall_all()
        # P7
        fresh(args.same)
        p = subprocess.run([str(args.same), '/P', '/UPDATE', f'/D={inst}'], timeout=300)
        check('P7 /P /UPDATE same version: no page, no uninstall', p.returncode == 0 and sentinel(), p.returncode)
        uninstall_all()
        # P8
        fresh(args.old)
        p = subprocess.run([str(args.new), '/S', f'/D={inst}'], timeout=300)
        check('P8 /S upgrade: no page, no uninstall', p.returncode == 0 and sentinel(), p.returncode)
        uninstall_all()
        fresh(args.new)
        before = version()
        p = subprocess.run([str(args.old), '/S', f'/D={inst}'], timeout=300)
        check('P8 /S downgrade refused, installed version unchanged', p.returncode != 0 and version() == before and sentinel(), [p.returncode, before, version()])
        uninstall_all()
        # P9: cancel the uninstaller wizard (the uninstall step of an upgrade runs it interactively)
        fresh(args.old)
        s = to_page(args.new, 'P9')
        s.click(1)  # default: uninstall first -> the uninstaller wizard opens as its own process
        time.sleep(3)
        from pywinauto import Desktop
        un = Desktop(backend='win32').window(title_re='.*UniClipboard Uninstall.*', class_name='#32770')
        un.wait('visible', timeout=60)
        un.child_window(control_id=2, class_name='Button').click()
        try:
            un.child_window(title_re='.*(Yes|OK).*', class_name='Button').click()
        except Exception:
            pass
        check('P9 cancelled uninstaller: back on the page, nothing installed over', s.page_is(TEXT['older'][0], timeout=60) and sentinel())
        s.click(2)
        try:
            s.win.child_window(title_re='.*(Yes|OK).*', class_name='Button').click()
        except Exception:
            pass
        s.wait(60)
    finally:
        uninstall_all()
        reg('delete', UNINST, '/f')
        reg('delete', RUN, '/v', 'UniClipboard', '/f')
        shutil.rmtree(inst.parent, ignore_errors=True)
        (args.out / 'windows-installer-reinstall-assertions.json').write_text(json.dumps(checks, indent=2) + '\n')
    ok = all(c['ok'] for c in checks) and checks
    print(json.dumps({'passed': bool(ok), 'checks': len(checks)}))
    sys.exit(0 if ok else 1)


if __name__ == '__main__':
    main()
