#!/usr/bin/env python3
"""Real WebView/binding/storage acceptance. Run in a private Xvfb + D-Bus session.

Build frontend with VITE_GUI_GO_E2E=1 UC_UI_PLATFORM=linux, GUI with gtk4,e2e.
Pass --gui, --daemon, --cli and --out. No production profile or clipboard is used.
Capability fixtures exercise policy only, not macOS/Windows GPU support.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import uuid


def main():
    parser = argparse.ArgumentParser()
    for name in ('gui', 'daemon', 'cli', 'out'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    results = []
    with tempfile.TemporaryDirectory(prefix='uc-gui-go-effects-') as temporary:
        sandbox = Path(temporary)
        for source, name in ((args.gui, 'gui-go'), (args.daemon, 'uniclipd'), (args.cli, 'uniclip')):
            shutil.copy2(source, sandbox / name)
        profile = 'gui-go-effects-' + uuid.uuid4().hex[:12]
        profiles = [profile]
        env = dict(os.environ, HOME=str(sandbox), UC_PROFILE=profile, UC_PORTABLE='1',
                   UC_GUI_GO_ISOLATED='1', UC_DISABLE_SYSTEM_CLIPBOARD='1',
                   UNICLIPBOARD_ENV='development', UC_GPUI_QUICK_PANEL='0',
                   UC_GUI_GO_EXIT_MODE='full', WEBKIT_DISABLE_DMABUF_RENDERER='1',
                   GDK_BACKEND='x11')
        for name in ('DATA_HOME', 'CONFIG_HOME', 'CACHE_HOME', 'STATE_HOME'):
            env['XDG_' + name] = str(sandbox / name.lower())
        env.pop('WAYLAND_DISPLAY', None)
        env['PATH'] = str(sandbox) + os.pathsep + env['PATH']
        settings = sandbox / 'data' / ('app.uniclipboard.desktop-' + profile) / 'visual-effects.json'

        def launch(phase):
            evidence = out / (phase.replace(':', '-') + '.jsonl')
            evidence.write_text('')
            env.update(UC_GUI_GO_E2E_PHASE=phase, UC_GUI_GO_EVIDENCE=str(evidence))
            with (out / (phase.replace(':', '-') + '.log')).open('w') as log:
                process = subprocess.Popen([str(sandbox / 'gui-go')], env=env, stdout=log, stderr=subprocess.STDOUT)
                try:
                    process.wait(timeout=120)
                finally:
                    if process.poll() is None:
                        process.terminate()
                        process.wait(timeout=20)
            rows = [json.loads(line) for line in evidence.read_text().splitlines()]
            assert process.returncode == 0, (phase, process.returncode)
            assert any(row['step'] == 'effects-done' for row in rows), (phase, rows[-3:])
            assert all(row['ok'] for row in rows), (phase, [row for row in rows if not row['ok']])
            results.append({'phase': phase, 'assertions': len(rows), 'passed': True})
            return rows

        try:
            # Initialize synthetic business state through the real CLI/daemon so GUI bootstrap
            # and the settings/auth paths run against real services, never mocked HTTP.
            with (out / 'space-init.log').open('w') as log:
                subprocess.run([str(sandbox / 'uniclip'), 'space', 'init', '--passphrase',
                                'gui-effects-synthetic-passphrase', '--device-name', 'effects-test'],
                               env=env, stdout=log, stderr=subprocess.STDOUT, check=True, timeout=90)
            rows = launch('effects:fresh')
            secondary = [row['detail'] for row in rows if row['step'] == 'effects-secondary']
            assert any(s['mode'] == 'effects' and not s['reduceMotion'] for s in secondary)
            assert any(s['mode'] == 'smooth' and s['reduceMotion'] for s in secondary)
            assert json.loads(settings.read_text())['mode'] == 'smooth'
            launch('effects:restart')
            for kind, raw in (('corrupt', b'{broken'), ('oversize', b' ' * 4097),
                              ('future', b'{"schemaVersion":999,"mode":"effects"}')):
                settings.write_bytes(raw)
                launch('effects:readonly-' + kind)
                assert settings.read_bytes() == raw, 'readonly config was overwritten'
            settings.write_text('{"schemaVersion":1,"policyVersion":1,"mode":"auto",'
                                '"nextAuto":"effects","evidenceClass":"capable"}')
            launch('effects:migrate')
            stored = json.loads(settings.read_text())
            assert stored['policyVersion'] == 2 and stored['nextAuto'] is None
            (out / 'stored-config.json').write_text(json.dumps(stored, indent=2))
            original = settings.read_bytes()
            profiles.append(profile + '-other')
            env['UC_PROFILE'] = profiles[-1]
            launch('effects:isolated')
            assert settings.read_bytes() == original, 'another profile modified the original preference'
        finally:
            for active_profile in profiles:
                env['UC_PROFILE'] = active_profile
                subprocess.run([str(sandbox / 'uniclip'), 'stop'], env=env,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=30)
            (out / 'result.json').write_text(json.dumps(results, indent=2))
    print(json.dumps(results, indent=2))


if __name__ == '__main__':
    main()
