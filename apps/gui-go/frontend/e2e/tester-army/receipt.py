#!/usr/bin/env python3
"""Index only public test evidence; never include the isolated daemon's HOME."""
import hashlib
import json
import platform
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[5]


def git(*args):
    return subprocess.check_output(['git', *args], cwd=ROOT, text=True, encoding='utf-8').strip()


def digest(path):
    with path.open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


paths = ['package.json', 'bun.lock', 'e2e.config.ts', 'e2e.daemon.config.ts', 'e2e.layouts.config.ts', 'e2e.native.config.ts', 'e2e.backend.config.ts',
         'apps/gui-go/e2e/tester-army-native', 'apps/gui-go/cmd/webview-probe', 'docs/testing',
         '.github/workflows/tester-army-e2e.yml', '.github/workflows/tester-army-native.yml', 'apps/gui-go/frontend']
tracked = git('ls-files', '-z', '--', *paths).split('\0')
untracked = git('ls-files', '--others', '--exclude-standard', '-z', '--', *paths).split('\0')
source = {name: digest(ROOT / name) for name in sorted(set(tracked + untracked))
          if name and (ROOT / name).is_file()}
reports = {}
report_paths = [ROOT / 'target' / name / 'report.json' for name in ['tester-army', 'tester-army-daemon']]
report_paths += list((ROOT / 'target/tester-army-layout-report').glob('*/*/report.json'))
report_paths += list((ROOT / 'target/tester-army-backend-report').rglob('report.json'))
for path in report_paths:
    if path.exists():
        report = json.loads(path.read_text(encoding='utf-8'))
        reports[str(path.parent.relative_to(ROOT / 'target'))] = {
            'sha256': digest(path), 'runId': report['run']['id'],
            'vcs': report['run'].get('vcs'), 'summary': report['run']['summary']}
receipt = {
    'head': git('rev-parse', 'HEAD'),
    'dirty': bool(git('status', '--porcelain')),
    'platform': platform.platform(),
    'sourceSha256': source,
    'binariesSha256': {name: digest(ROOT / name) for name in
                       ['target/debug/uniclipd', 'target/gui-go/uniclip'] if (ROOT / name).is_file()},
    'reports': reports,
    'commands': ['bun install --frozen-lockfile',
                 'node node_modules/@e2e-dev/web/dist/bin.js install chromium',
                 'bun run e2e:browser', 'bun run e2e:browser:layouts', 'bun run e2e:browser:daemon', 'bun run e2e:browser:backends'],
    'nativeGuiAcceptance': False,
    'systemClipboardAccess': False,
    'modelConfigured': False,
}
(ROOT / 'target').mkdir(exist_ok=True)
(ROOT / 'target/tester-army-receipt.json').write_text(json.dumps(receipt, indent=2) + '\n', encoding='utf-8')
artifacts = [ROOT / 'target/tester-army-receipt.json']
for directory in ['tester-army', 'tester-army-daemon', 'tester-army-layout', 'tester-army-layout-report', 'tester-army-backend', 'tester-army-backend-report']:
    artifacts.extend(p for p in sorted((ROOT / 'target' / directory).rglob('*')) if p.is_file())
(ROOT / 'target/tester-army-SHA256SUMS').write_text(''.join(
    f'{digest(p)}  {p.relative_to(ROOT).as_posix()}\n' for p in artifacts), encoding='utf-8')
print('Recorded public evidence and checksums')
