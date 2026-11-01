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


paths = ['scripts/gui-e2e.mjs', 'package.json', 'bun.lock', 'e2e.config.ts', 'e2e.daemon.config.ts', 'e2e.layouts.config.ts', 'e2e.native.config.ts', 'e2e.backend.config.ts',
         'apps/gui-go/e2e/tester-army-native', 'apps/gui-go/cmd/webview-probe', 'docs/testing',
         '.github/workflows/tester-army-e2e.yml', '.github/workflows/tester-army-native.yml', 'apps/gui-go/frontend']
tracked = git('ls-files', '-z', '--', *paths).split('\0')
untracked = git('ls-files', '--others', '--exclude-standard', '-z', '--', *paths).split('\0')
source = {name: digest(ROOT / name) for name in sorted(set(tracked + untracked))
          if name and (ROOT / name).is_file()}
head = git('rev-parse', 'HEAD')
reports = {}
selections = {}
missing_reports = []
report_paths = []
artifact_roots = []
excluded_stale_reports = []
for name in ['tester-army', 'tester-army-daemon']:
    report_path = ROOT / 'target' / name / 'report.json'
    if not report_path.exists():
        continue
    report = json.loads(report_path.read_text(encoding='utf-8'))
    if (report['run'].get('vcs') or {}).get('commit') != head:
        excluded_stale_reports.append(str(report_path.relative_to(ROOT)))
        continue
    report_paths.append(report_path)
    artifact_roots.append(report_path.parent)
selection_paths = []
for kind in ['layout', 'backend']:
    report_root = ROOT / f'target/tester-army-{kind}-report'
    selection_path = report_root / 'selection.json'
    if not selection_path.exists():
        continue
    selection = json.loads(selection_path.read_text(encoding='utf-8'))
    selections[kind] = selection
    if selection.get('head') != head:
        continue
    selection_paths.append(selection_path)
    for run in selection['runs']:
        folder = Path(run['platform']) / run['suite']
        if kind == 'backend':
            folder = folder / run['seed'] / ('all' if run['phase'] == 'all-original-phases' else run['phase'])
        report = report_root / folder / 'report.json'
        report_paths.append(report)
        artifact_roots.extend([report.parent, ROOT / f'target/tester-army-{kind}' / folder])
        if not report.exists():
            missing_reports.append(str(report.relative_to(ROOT)))
for path in report_paths:
    if path.exists():
        report = json.loads(path.read_text(encoding='utf-8'))
        reports[str(path.parent.relative_to(ROOT / 'target'))] = {
            'sha256': digest(path), 'runId': report['run']['id'],
            'vcs': report['run'].get('vcs'), 'summary': report['run']['summary']}
receipt = {
    'head': head,
    'dirty': bool(git('status', '--porcelain')),
    'platform': platform.platform(),
    'sourceSha256': source,
    'binariesSha256': {name: digest(ROOT / name) for name in
                       ['target/debug/uniclipd', 'target/gui-go/uniclip'] if (ROOT / name).is_file()},
    'reports': reports,
    'selections': selections,
    'missingReports': missing_reports,
    'excludedStaleReports': excluded_stale_reports,
    'commands': ['bun install --frozen-lockfile',
                 'node node_modules/@e2e-dev/web/dist/bin.js install chromium',
                 'bun run e2e:gui browser', 'bun run e2e:gui layouts', 'bun run e2e:gui daemon', 'bun run e2e:gui backends'],
    'nativeGuiAcceptance': False,
    'systemClipboardAccess': False,
    'modelConfigured': False,
}
(ROOT / 'target').mkdir(exist_ok=True)
(ROOT / 'target/tester-army-receipt.json').write_text(json.dumps(receipt, indent=2) + '\n', encoding='utf-8')
artifacts = [ROOT / 'target/tester-army-receipt.json', *selection_paths]
for directory in artifact_roots:
    artifacts.extend(p for p in sorted(directory.rglob('*')) if p.is_file())
(ROOT / 'target/tester-army-SHA256SUMS').write_text(''.join(
    f'{digest(p)}  {p.relative_to(ROOT).as_posix()}\n' for p in artifacts), encoding='utf-8')
print('Recorded public evidence and checksums')
