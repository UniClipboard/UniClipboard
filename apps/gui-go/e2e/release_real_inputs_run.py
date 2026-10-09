#!/usr/bin/env python3
"""The release gate's package-evidence checks against REAL workflow artifacts (read-only copies of earlier CI runs).

Inputs are the packaging-evidence artifacts of real runs, downloaded by the caller with `gh run download` (public JSON only; no
installer is needed). Nothing here is synthetic: the layouts (`windows-gui/shipped/package-manifest.json`,
`macos-gui-evidence-*/provenance.json`, `linux-gui-evidence-*/package-manifest.json`) are what the workflows really upload.

What it proves: the gate recognises each real layout, takes the source commit and version from the real records, rejects the
real test-mode and test-signed artifacts for the right reason, and (after renaming the directory only) shows the remaining
reason is the signing provider. What it does not prove: that a production-signed Windows package passes (none exists).

  --inputs  directory holding artifact directories under their real names, plus inputs.json:
            {"linux": {"run": 1, "artifact": "linux-gui-evidence-amd64"}, "windows": {...}, "macos": {...}}
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[3]
GATE = ROOT / 'scripts/ci/release_gate.py'


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def manifest_of(artifact):
    for name in ('shipped/package-manifest.json', 'package-manifest.json', 'provenance.json'):
        found = sorted(artifact.rglob(name))
        found = [f for f in found if 'newer' not in f.parts]
        if found:
            return found[0]
    sys.exit(f'no manifest in {artifact}')


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--inputs', type=Path, required=True)
    ap.add_argument('--out', type=Path, required=True)
    args = ap.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    inputs = json.loads((args.inputs / 'inputs.json').read_text())
    results, count = [], 0

    def case(label, platform, rename_to, sha, version, *, expect_in, expect_absent=(), derived_clean=False, forge_label=False, thumbprint=None):
        nonlocal count
        info = inputs[platform]
        with tempfile.TemporaryDirectory(prefix='uc-real-inputs-') as tmp:
            tree = Path(tmp) / 'artifacts'
            tree.mkdir()
            shutil.copytree(args.inputs / info['artifact'], tree / (rename_to or info['artifact']))
            if derived_clean:
                # DERIVED COPY (not a real artifact): the only edit is dirty=false, to show that the layout is otherwise accepted.
                for f in (tree / (rename_to or info['artifact'])).rglob('*.json'):
                    if f.name in ('package-manifest.json', 'provenance.json'):
                        d = json.loads(f.read_text())
                        (d['source'] if isinstance(d.get('source'), dict) else d['git'])['dirty'] = False
                        f.write_text(json.dumps(d))
            if forge_label:
                # FORGED LABEL on a real artifact: the package record is edited to claim a production SignPath signature (clean checkout,
                # production policy, pinned thumbprint). The real receipts next to it are left untouched and say what really happened.
                f = next((tree / (rename_to or info['artifact'])).rglob('shipped/package-manifest.json'))
                d = json.loads(f.read_text())
                d['source']['dirty'] = False
                d['signed'] = True
                d['signing'] = {'provider': 'signpath', 'evidence': {'provider': 'signpath', 'testCertificate': False, 'policy': 'forged-production-policy',
                                                                      'pinnedThumbprint': thumbprint}}
                f.write_text(json.dumps(d))
            extra_args = ['--windows-thumbprint', thumbprint] if thumbprint else []
            r = subprocess.run([sys.executable, '-I', str(GATE), 'evidence', '--version', version, '--source-sha', sha, '--artifacts', str(tree), *extra_args],
                               capture_output=True, text=True)
        lines = [l.removeprefix('error: ') for l in r.stderr.splitlines() if l.startswith('error: ')]
        (out / f'{label}.log').write_text(f'exit {r.returncode}\n{r.stdout}{r.stderr}')
        missing_others = {f'no package evidence from {p} was found in the artifacts' for p in ('macos', 'linux', 'windows') if p != platform}
        extra = [l for l in lines if l not in missing_others]
        ok = all(any(e in l for l in extra) for e in expect_in) and not any(a in l for l in extra for a in expect_absent)
        if not expect_in:
            ok = ok and not extra and r.returncode != 0  # only the other platforms are missing
        count += len(expect_in) + len(expect_absent) + 1
        results.append({'case': label, 'platform': platform, 'artifact': rename_to or info['artifact'], 'sourceRun': info['run'],
                        'exit': r.returncode, 'problems': extra, 'expectedText': list(expect_in), 'ok': ok})
        print(('PASS ' if ok else 'FAIL ') + label)
        if not ok:
            sys.exit(f'FAILED {label}: {extra}')

    for platform, info in inputs.items():
        art = args.inputs / info['artifact']
        mf = manifest_of(art)
        doc = json.loads(mf.read_text())
        src = doc.get('source') if isinstance(doc.get('source'), dict) else doc['git']
        head, version = src['head'], doc['version']
        info.update(porcelain=(src.get('porcelain') if isinstance(src, dict) else None), manifest=str(mf.relative_to(args.inputs)), manifestSha256=sha256(mf), head=head, version=version, dirty=src['dirty'])
        test_suffix = ('-signpath-test' if info['artifact'].endswith('-signpath-test') else '-test' if info['artifact'].endswith('-test') else None)
        clean_name = info['artifact'][:-len(test_suffix)] if test_suffix else info['artifact']
        if platform == 'linux':
            case('linux-own-commit-and-version', platform, None, head, version, expect_in=[])
            case('linux-wrong-commit', platform, None, '0' * 40, version, expect_in=['not the pinned source'])
            case('linux-wrong-version', platform, None, head, '9.9.9', expect_in=['is version'])
        elif platform == 'windows':
            case('windows-test-signed-artifact-as-is', platform, None, head, version,
                 expect_in=['test-mode or test-signed build', 'requires a production signature'])
            case('windows-renamed-real-record-is-dirty-and-test-signed', platform, clean_name, head, version,
                 expect_in=['requires a production signature', 'dirty checkout'], expect_absent=['test-mode or test-signed', 'not the pinned source', 'is version', 'records no source'])
            case('windows-renamed-derived-clean-only-provider-remains', platform, clean_name, head, version, derived_clean=True,
                 expect_in=['requires a production signature'], expect_absent=['dirty', 'test-mode or test-signed', 'not the pinned source', 'is version', 'records no source'])
            real_thumbprint = json.loads((art / 'windows-gui/signatures.json').read_text())['expectThumbprint']
            case('windows-forged-signpath-label-over-real-test-receipts', platform, clean_name, head, version, derived_clean=True, forge_label=True,
                 thumbprint=real_thumbprint, expect_in=['waived chain trust', 'not verified as a valid, trusted Authenticode signature'],
                 expect_absent=['requires a production signature', 'dirty', 'alone proves nothing'])
        elif platform == 'macos':
            case('macos-test-mode-artifact-as-is', platform, None, head, version, expect_in=['test-mode or test-signed build'])
            case('macos-renamed-real-record-is-dirty', platform, clean_name, head, version, expect_in=['dirty checkout'],
                 expect_absent=['not the pinned source', 'is version', 'records no source'])
            case('macos-renamed-derived-clean-layout-accepted', platform, clean_name, head, version, derived_clean=True, expect_in=[])
    (out / 'results.json').write_text(json.dumps({'inputs': inputs, 'cases': results, 'assertions': count,
                                                  'scope': 'real workflow artifacts (test-mode builds from earlier runs); no production-signed Windows package exists'},
                                                 indent=2, sort_keys=True) + '\n')
    (out / 'SHA256SUMS.txt').write_text(''.join(f'{sha256(f)}  {f.name}\n' for f in sorted(out.iterdir()) if f.is_file() and f.name != 'SHA256SUMS.txt'))
    print(f'{count} assertions over {len(results)} cases')


if __name__ == '__main__':
    main()
