#!/usr/bin/env python3
"""One Desktop acceptance entry; case files remain the source of the catalogue."""
import argparse
import datetime
import hashlib
import html
import json
import os
import re
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import uuid
from urllib.parse import quote

ROOT = Path(__file__).resolve().parents[3]
HERE = ROOT / 'app/desktop/e2e'
FIELDS = ('Case', 'Mode', 'Purpose', 'Steps', 'Expected', 'Forbidden')


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def cases(base=HERE):
    result = []
    for path in sorted((base / 'cases').glob('*.txtar')):
        if not re.fullmatch(r'[a-z][a-z0-9-]*', path.stem):
            raise ValueError('case IDs must use lowercase letters, digits and hyphens')
        fields, current = {}, None
        for line in path.read_text().splitlines():
            if not line.startswith('#'):
                break
            text = line[1:].strip()
            name, sep, value = text.partition(':')
            if sep and name in FIELDS:
                current = name
                fields.setdefault(name, [])
                if value.strip():
                    fields[name].append(value.strip())
            elif current and text:
                fields[current].append(text)
        if any(not fields.get(name) for name in FIELDS):
            raise ValueError(f'{path.name}: all catalogue fields are required')
        spec = {name.lower(): ' '.join(fields[name]) for name in FIELDS}
        if spec['mode'] not in ('native', 'process', 'view'):
            raise ValueError(f'{path.name}: invalid Mode')
        result.append(dict(spec, id=path.stem, script=path.read_text()))
    if not result:
        raise ValueError('no Desktop cases registered')
    # A registry entry without a declarative case (or vice versa) must fail preflight.
    registered = set()
    packages = {'desktopui': 'internal/desktopui', 'quickpanel': 'internal/quickpanel',
                'gui': 'cmd/unic-desktop', 'startup': 'e2e'}
    for group, directory in packages.items():
        source = (base.parent / directory / 'acceptance_test.go').read_text()
        registered.update((group, name) for name in re.findall(r't.Run\("([a-z][a-z0-9-]*)", scenario', source))
    declared = []
    for spec in result:
        commands = re.findall(r'^acceptance (\w+) ([a-z][a-z0-9-]*)$', spec['script'], re.M)
        if spec['mode'] != 'native' and len(commands) != 1:
            raise ValueError(f"{spec['id']}: exactly one registered acceptance scenario required")
        if commands and commands[0][1] != spec['id']:
            raise ValueError('scenario ID must match the case ID')
        declared.extend(commands)
    if len(set(declared)) != len(declared) or set(declared) != registered:
        raise ValueError('acceptance registry and declarative catalogue do not match')
    return result


def catalogue(specs):
    text = '# Desktop E2E cases\n\nGenerated from `cases/*.txtar`. Run `make desktop-e2e`; select with `CASE=<id>`.\n\n'
    text += '| Case | Mode | Purpose |\n|---|---|---|\n'
    for s in specs:
        text += f"| [{s['id']}](#{s['id']}) | {s['mode']} | {s['purpose']} |\n"
    for s in specs:
        text += f"\n## {s['id']}\n\n{s['case']}\n"
        for field in ('steps', 'expected', 'forbidden'):
            text += f"\n{field.capitalize()}: {s[field]}\n"
        text += '\n```text\n' + s['script'].split('\n\n', 1)[1].rstrip() + '\n```\n'
    return text


def doctor(native):
    for name in ('go', 'python3') + (('ffmpeg', 'ffprobe') if native else ()):
        if not shutil.which(name):
            raise RuntimeError(f'required executable unavailable: {name}')
    if native and sys.platform.startswith('linux') and not (os.getenv('DISPLAY') or os.getenv('WAYLAND_DISPLAY')):
        raise RuntimeError('native cases require a desktop session; on CI use xvfb-run')


def freeze(destination):
    names = subprocess.check_output(['git', 'ls-files', '-co', '--exclude-standard', '-z'], cwd=ROOT).split(b'\0')
    hashes = {}
    for name in sorted(set(names)):
        if not name:
            continue
        relative = Path(os.fsdecode(name))
        source = ROOT / relative
        if not source.exists():
            continue
        resolved = source.resolve()
        if not resolved.is_relative_to(ROOT) or not resolved.is_file():
            raise RuntimeError(f'unsupported source link: {relative}')
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(resolved, target)
        hashes[relative.as_posix()] = digest(target)
    return hashes


def write_json(path, value):
    temporary = path.with_name(path.name + '.tmp')
    temporary.write_text(json.dumps(value, indent=2) + '\n')
    temporary.replace(path)


def report(out, manifest, specs):
    esc = html.escape
    body = ['<!doctype html><meta charset="utf-8"><title>Desktop E2E</title>',
            '<style>body{font:16px system-ui;max-width:1100px;margin:40px auto;padding:0 20px;color:#20242b}section{border-top:1px solid #ddd;margin-top:28px;padding-top:16px}img{max-width:340px;border:1px solid #ddd;margin:8px}video{max-width:100%;background:#eee}pre{white-space:pre-wrap}a{color:#1565c0}.PASS{color:#137333}.FAIL,.ERROR{color:#b3261e}</style>',
            f"<h1>Desktop E2E: <span class='{esc(manifest['status'])}'>{esc(manifest['status'])}</span></h1>",
            f"<p>Run {esc(manifest['run_id'])} · Source {esc(manifest.get('source_head','unknown'))}</p>",
            '<p>Native evidence captures a running window’s content. OS decorations, physical input, system clipboard and cross-application paste are not established by this run.</p>',
            '<p><a href="manifest.json">Manifest</a> · <a href="SHA256SUMS">Evidence checksums</a></p>']
    if manifest.get('failure'):
        body.append('<pre>' + esc(manifest['failure']) + '</pre>')
    for spec in specs:
        result = manifest['cases'].get(spec['id'], {'status': 'NOT_RUN'})
        body += [f"<section id='{esc(spec['id'])}'><h2>{esc(spec['id'])}: <span class='{esc(result['status'])}'>{esc(result['status'])}</span></h2>",
                 f"<p>{esc(spec['purpose'])}</p><p>Mode: {esc(spec['mode'])}</p>"]
        prefix = 'cases/' + quote(spec['id']) + '/'
        if (out / prefix / 'result.json').is_file():
            body.append(f'<p><a href="{prefix}result.json">Assertions and steps</a></p>')
        if (out / prefix / 'scenario-receipt.json').is_file():
            body.append(f'<p><a href="{prefix}scenario-receipt.json">Scenario receipt</a></p>')
        for receipt in sorted((out / prefix).glob('*.json')):
            if receipt.name not in ('result.json', 'scenario-receipt.json'):
                body.append(f'<p><a href="{prefix}{quote(receipt.name)}">{esc(receipt.name)}</a></p>')
        if spec['mode'] == 'view':
            body.append('<p>Software-rendered view checkpoints; no native window or continuous recording.</p>')
        for assertion in result.get('assertions') or []:
            body.append('<p>✓ ' + esc(assertion) + '</p>')
        if result.get('failure'):
            body.append('<pre>' + esc(result['failure']) + '</pre>')
        for picture in result.get('screenshots') or []:
            url = prefix + quote(picture)
            label = picture
            for assertion in result.get('assertions') or []:
                if hashlib.sha256(assertion.encode()).hexdigest() in picture:
                    label = assertion.removeprefix('TestDesktopAcceptance/')
                    break
            body.append(f'<figure><a href="{url}"><img src="{url}" alt="{esc(label)}"></a><figcaption>{esc(label)}</figcaption></figure>')
        if result.get('recording'):
            body.append(f'<p>Continuous window-content recording: {result.get("frames",0)} frames, {result.get("duration_seconds",0):.2f} seconds.</p>')
            body.append(f'<video controls preload="metadata" src="{prefix}{quote(result["recording"])}"></video>')
        steps = result.get('steps') or []
        if steps:
            body.append('<h3>Executed steps</h3><table><tr><th>Time</th><th>Action</th><th>Result</th></tr>')
            for step in steps:
                at = max(0, step['at_ms'] / 1000 - result.get('recording_started_seconds', 0))
                label = step['command'] + ' ' + ' '.join(step.get('args') or [])
                body.append(f"<tr><td>{at:.2f}s</td><td>{esc(label)}</td><td>{esc(step['status'])}</td></tr>")
            body.append('</table>')
        body.append('</section>')
    (out / 'index.html').write_text('\n'.join(body))


def run(specs, selected, output):
    run_id = datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%SZ') + '-' + uuid.uuid4().hex[:8]
    out = Path(output).resolve() if output else ROOT / '.build/desktop-e2e' / run_id
    if out.exists():
        raise ValueError('output directory must be new')
    out.mkdir(parents=True, mode=0o700)
    manifest = dict(version=1, run_id=run_id, status='RUNNING', cases={},
                    native_os_focus='NOT_RUN', system_clipboard='NOT_RUN', cross_application_paste='NOT_RUN',
                    runner_sha256=digest(Path(__file__)))
    for s in specs:
        manifest['cases'][s['id']] = {'status': 'NOT_RUN', 'reason': 'not selected' if s['id'] not in selected else 'not executed'}
    private = None
    process = None
    snapshot_hashes = {}
    try:
        wanted = [s for s in specs if s['id'] in selected]
        native = any(s['mode'] == 'native' for s in wanted)
        doctor(native)
        if native:
            manifest['recording_tools'] = {name: {'version': subprocess.check_output([name, '-version'], text=True).splitlines()[0], 'sha256': digest(Path(shutil.which(name)).resolve())} for name in ('ffmpeg', 'ffprobe')}
        print(f'Desktop E2E: {len(wanted)} selected cases; freezing source', flush=True)
        manifest['source_head'] = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
        manifest['worktree_dirty'] = bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT))
        private = Path(tempfile.mkdtemp(prefix='de-', dir='/tmp' if os.name == 'posix' else None)).resolve()
        snapshot = private / 'source'
        snapshot_hashes = freeze(snapshot)
        manifest['source_sha256'] = snapshot_hashes
        if snapshot_hashes.get('app/desktop/e2e/run.py') != manifest['runner_sha256']:
            raise RuntimeError('runner source changed while taking the snapshot')
        specs = cases(snapshot / 'app/desktop/e2e')
        if {s['id'] for s in specs} != set(manifest['cases']):
            raise RuntimeError('case catalogue changed while taking the snapshot')
        wanted = [s for s in specs if s['id'] in selected]
        if len(wanted) != len(selected):
            raise RuntimeError('case selection changed while taking the snapshot')
        manifest['go'] = subprocess.check_output(['go', 'version'], text=True).strip()
        manifest['platform'] = subprocess.check_output(['go', 'env', 'GOOS', 'GOARCH'], text=True).splitlines()
        # The pinned Go toolchain has no race detector for Windows arm64.
        race_flags = [] if manifest['platform'] == ['windows', 'arm64'] else ['-race']
        manifest['race_detector'] = bool(race_flags)
        binaries = private / 'bin'
        binaries.mkdir()
        manifest['binary_sha256'] = {}
        if native:
            for name, tags in [('unicd', []), ('unic-desktop', ['-tags', 'desktop_e2e'])]:
                print('BUILD ' + name, flush=True)
                target = binaries / (name + ('.exe' if os.name == 'nt' else ''))
                subprocess.run(['go', 'build', *race_flags, '-buildvcs=false', *tags, '-o', str(target),
                                './app/' + ('service' if name == 'unicd' else 'desktop') + '/cmd/' + name], cwd=snapshot, check=True)
                manifest['binary_sha256'][name] = digest(target)
        manifest['desktop_build_tags'] = ['desktop_e2e'] if native else []
        environment = {k: v for k, v in os.environ.items() if not k.startswith(('UNIC_', 'MYGO_AUTOMATION'))}
        environment.update(TMPDIR=str(private), UNIC_DESKTOP_E2E='1', UNIC_DESKTOP_E2E_BIN=str(binaries),
                           UNIC_DESKTOP_E2E_SOURCE_HEAD=manifest['source_head'], UNIC_DESKTOP_E2E_ARTIFACTS=str(out / 'cases'), UNIC_DESKTOP_E2E_PRIVATE=str(private / 'cases'))
        expression = '^TestDesktopScenarios$/^(' + '|'.join(selected) + ')$'
        command = ['go', 'test', *race_flags, './app/desktop/e2e', '-run', expression, '-parallel', '1', '-count=1', '-timeout=45m', '-v']
        with (private / 'go-test.log').open('w') as log:
            process = subprocess.Popen(command, cwd=snapshot, env=environment, stdout=log, stderr=subprocess.STDOUT,
                                       start_new_session=os.name == 'posix')
            print('RUN ' + ', '.join(selected), flush=True)
            deadline = time.monotonic() + 2750
            announced = set()
            while process.poll() is None:
                if time.monotonic() >= deadline:
                    raise TimeoutError('Desktop suite exceeded its execution deadline')
                for spec in wanted:
                    result_path = out / 'cases' / spec['id'] / 'result.json'
                    if spec['id'] in announced or not result_path.is_file():
                        continue
                    try:
                        current = json.loads(result_path.read_text())
                    except (OSError, json.JSONDecodeError):
                        continue
                    print(current['status'] + ' ' + spec['id'], flush=True)
                    announced.add(spec['id'])
                time.sleep(0.25)
            code = process.returncode
        for spec in wanted:
            result_path = out / 'cases' / spec['id'] / 'result.json'
            if result_path.is_file():
                manifest['cases'][spec['id']] = json.loads(result_path.read_text())
            else:
                manifest['cases'][spec['id']] = {'status': 'FAIL', 'failure': 'case emitted no result; a skipped test cannot pass'}
        if code != 0 or any(manifest['cases'][s['id']]['status'] != 'PASS' for s in wanted):
            raise RuntimeError('one or more Desktop cases failed; inspect case evidence')
        (out / 'CASES.md').write_text(catalogue(specs))
        manifest['status'] = 'PASS'
    except (Exception, KeyboardInterrupt) as error:
        manifest['status'] = 'FAIL'
        manifest['failure'] = str(error) or 'interrupted'
    finally:
        if process and process.poll() is None:
            if os.name == 'posix':
                os.killpg(process.pid, signal.SIGTERM)
            else:
                process.terminate()
            try:
                process.wait(timeout=20)
            except subprocess.TimeoutExpired:
                if os.name == 'posix':
                    os.killpg(process.pid, signal.SIGKILL)
                else:
                    process.kill()
                process.wait(timeout=10)
        if private:
            manifest['snapshot_unchanged'] = all((private / 'source' / p).is_file() and digest(private / 'source' / p) == sha
                                                 for p, sha in snapshot_hashes.items())
            if not manifest['snapshot_unchanged']:
                manifest['status'] = 'FAIL'
                manifest['failure'] = 'frozen source changed during execution'
            try:
                shutil.rmtree(private)
                manifest['private_cleanup'] = 'PASS'
            except OSError:
                manifest['private_cleanup'] = 'FAIL'
                manifest['status'] = 'FAIL'
        write_json(out / 'manifest.json', dict(manifest, status='FINALIZING'))
        try:
            report(out, manifest, specs)
            final_bytes = (json.dumps(manifest, indent=2) + '\n').encode()
            files = sorted(p for p in out.rglob('*') if p.is_file() and p.name not in ('SHA256SUMS', 'manifest.json'))
            sums = ''.join(digest(p) + '  ' + p.relative_to(out).as_posix() + '\n' for p in files)
            sums += hashlib.sha256(final_bytes).hexdigest() + '  manifest.json\n'
            (out / 'SHA256SUMS').write_text(sums)
            # Publish the final verdict only after the report and evidence checksums exist.
            write_json(out / 'manifest.json', manifest)
        except Exception as error:
            manifest['status'] = 'FAIL'
            manifest['failure'] = 'evidence publication failed: ' + str(error)
            write_json(out / 'manifest.json', manifest)
            (out / 'index.html').write_text('<!doctype html><h1>Desktop E2E: FAIL</h1><p>' + html.escape(manifest['failure']) + '</p><a href="manifest.json">Manifest</a>')
        print(f"{manifest['status']}: {out / 'index.html'}")
    return 0 if manifest['status'] == 'PASS' else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('run', 'list', 'doctor', 'catalog'), nargs='?', default='run')
    parser.add_argument('--case', default='')
    parser.add_argument('--out')
    parser.add_argument('--check', action='store_true')
    parser.add_argument('--update', action='store_true')
    args = parser.parse_args()
    specs = cases()
    if args.action == 'catalog':
        content = catalogue(specs)
        path = HERE / 'CASES.md'
        if args.update:
            path.write_text(content)
        elif not path.exists() or path.read_text() != content:
            raise RuntimeError('CASES.md is stale; run python3 app/desktop/e2e/run.py catalog --update')
        return 0
    if args.action == 'list':
        for s in specs:
            print(f"{s['id']:24} {s['mode']:8} {s['case']}")
        return 0
    selected = [args.case] if args.case else [s['id'] for s in specs]
    if not set(selected).issubset({s['id'] for s in specs}):
        raise ValueError('unknown Desktop case')
    if args.action == 'doctor':
        doctor(any(s['mode'] == 'native' and s['id'] in selected for s in specs))
        print('READY: basic prerequisites found; native startup is verified by the case')
        return 0
    return run(specs, selected, args.out)


if __name__ == '__main__':
    try:
        sys.exit(main())
    except Exception as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
