#!/usr/bin/env python3
"""Run the real Linux Wails history against an isolated daemon with synthetic entries.

Build frontend with VITE_GUI_GO_E2E=1 and host with gtk4,e2e.
No physical input is generated. This measures scripted scrolling, not touchpad feel.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import uuid

from run import ROOT, isolated_env, wait_for_step


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--gui', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--cli', type=Path, default=ROOT / 'target/gui-go/uniclip')
    parser.add_argument('--daemon', type=Path, default=ROOT / 'target/debug/uniclipd')
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    evidence = out / 'native.jsonl'
    evidence.write_text('')
    result_path = out / 'result.json'
    result_path.write_text(json.dumps({'passed': False, 'status': 'running'}) + '\n')
    with tempfile.TemporaryDirectory(prefix='uc-gui-go-gtk-trial-') as home:
        profile = 'gui-go-gtk-trial-' + uuid.uuid4().hex[:10]
        binary = Path(home) / 'uniclipboard'
        shutil.copy2(args.gui.resolve(), binary)
        cli = Path(home) / 'uniclip'
        shutil.copy2(args.cli.resolve(), cli)
        shutil.copy2(args.daemon.resolve(), Path(home) / 'uniclipd')
        desktop_keys = ('DISPLAY', 'WAYLAND_DISPLAY', 'XDG_RUNTIME_DIR', 'XDG_SESSION_TYPE',
                        'DBUS_SESSION_BUS_ADDRESS', 'GDK_BACKEND', 'LD_LIBRARY_PATH',
                        'FRONTEND_DEVSERVER_URL', 'WEBKIT_DISABLE_DMABUF_RENDERER')
        env = isolated_env(home, profile, {
            **{key: os.environ[key] for key in desktop_keys if key in os.environ},
            'PATH': home + os.pathsep + os.environ['PATH'],
            'UC_PORTABLE': '1', 'UC_GUI_GO_ISOLATED': '1', 'UC_GUI_GO_EVIDENCE': str(evidence),
            'UC_GUI_GO_E2E_PHASE': 'linux-scroll', 'UC_GPUI_QUICK_PANEL': '0',
            'UC_GUI_GO_EXIT_MODE': 'full',
        })
        process = None
        try:
            with (out / 'seed.log').open('w') as log:
                subprocess.run([str(cli), 'space', 'init', '--passphrase', 'gui-go-synthetic-passphrase',
                                '--device-name', 'gtk-trial'], env=env, stdout=log,
                               stderr=subprocess.STDOUT, check=True, timeout=90)
                subprocess.run([str(cli), 'start'], env=env, stdout=log,
                               stderr=subprocess.STDOUT, check=True, timeout=90)
                conn_path = next(Path(home).rglob('daemon.conn'))
                conn = json.loads(conn_path.read_text())
                base = f"http://{conn.get('host', '127.0.0.1')}:{conn['port']}"
                subprocess.run(['node', str(ROOT / 'apps/gui-go/frontend/e2e/history-kit-seed.mjs'),
                                str(out), base, str(conn_path), 'gui-go-synthetic-passphrase', 'paging', '140'],
                               env=env, stdout=log, stderr=subprocess.STDOUT, check=True, timeout=420)
            with (out / 'gui.log').open('w') as log:
                process = subprocess.Popen([str(binary)], env=env, stdout=log, stderr=subprocess.STDOUT)
                wait_for_step(process, evidence, 0, 'linux-scroll-complete', timeout=120)
                process.wait(timeout=30)
            rows = [json.loads(line) for line in evidence.read_text().splitlines()]
            failures = [row['step'] for row in rows if not row['ok']]
            completed = any(row['step'] == 'linux-scroll-complete' for row in rows)
            result = {'profile': profile, 'exitCode': process.returncode,
                      'passed': process.returncode == 0 and completed and not failures,
                      'failedSteps': failures, 'steps': len(rows)}
            result_path.write_text(json.dumps(result, indent=2) + '\n')
            print(json.dumps(result))
            if not result['passed']:
                raise SystemExit(1)
        finally:
            try:
                if process is not None and process.poll() is None:
                    process.terminate()
                    process.wait(timeout=20)
            finally:
                subprocess.run([str(cli), 'stop'], env=env, stdout=subprocess.DEVNULL,
                               stderr=subprocess.DEVNULL, timeout=30)


if __name__ == '__main__':
    main()
