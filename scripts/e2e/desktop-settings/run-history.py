#!/usr/bin/env python3
"""Run both existing migration suites with real historical inputs and an isolated HOME."""
import argparse
import json
from pathlib import Path
import subprocess
import sys
import tomllib

from adoption import ROOT, digest
sys.path.insert(0, str(ROOT / 'apps/cli-go/e2e'))
from isolated import isolated_env


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--inputs', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve(); out.mkdir(parents=True, exist_ok=False)
    inputs = args.inputs.resolve()
    result = {'passed': False, 'suites': [], 'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
              'engineSha': tomllib.loads((ROOT / 'Cargo.toml').read_text())['workspace']['dependencies']['uc-engine']['rev'],
              'historySource': json.loads((inputs / 'source.json').read_text()),
              'sourceSha256': {str(p.relative_to(ROOT)): digest(p.read_bytes()) for p in [Path(__file__).resolve(), ROOT / 'tests/e2e/tests/desktop_settings.rs', ROOT / 'tests/e2e/tests/settings_migration.rs', ROOT / 'tests/e2e/Cargo.lock']},
              'daemonSha256': digest((ROOT / 'target/debug/uniclipd').read_bytes()),
              'cliSha256': digest((ROOT / 'target/debug/uniclip').read_bytes())}
    try:
        command = ['cargo', 'test', '--manifest-path', 'tests/e2e/Cargo.toml', '--locked', '--test', 'desktop_settings', '--test', 'settings_migration', '--no-run', '--message-format=json']
        with (out / 'build.log').open('wb') as log:
            build = subprocess.run(command, cwd=ROOT, stdout=subprocess.PIPE, stderr=log)
        result['buildExitCode'] = build.returncode
        (out / 'build.jsonl').write_bytes(build.stdout)
        assert build.returncode == 0
        home = out / 'home'; home.mkdir()
        env = isolated_env(str(home), 'migration')
        env.update(json.loads((inputs / 'environment.json').read_text()))
        env.update({'UC_E2E_ARTIFACT_DIR': str(out / 'artifacts'), 'UC_E2E_KEEP_PROFILES': '1'})
        suites = []
        for line in build.stdout.splitlines():
            item = json.loads(line)
            if item.get('reason') == 'compiler-artifact' and item.get('executable') and item['target']['name'] in ['desktop_settings', 'settings_migration']:
                suites.append(item)
        assert len(suites) == 2
        for item in suites:
            name = item['target']['name']; binary = Path(item['executable'])
            with (out / f'{name}.log').open('wb') as log:
                code = subprocess.run([str(binary), '--ignored', '--test-threads=1', '--nocapture'], cwd=ROOT, env=env, stdout=log, stderr=log).returncode
            result['suites'].append({'suite': name, 'binarySha256': digest(binary.read_bytes()), 'exitCode': code})
        result['passed'] = all(s['exitCode'] == 0 for s in result['suites'])
    finally:
        (out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    assert result['passed'], result['suites']
    print(out / 'result.json')


if __name__ == '__main__': main()
