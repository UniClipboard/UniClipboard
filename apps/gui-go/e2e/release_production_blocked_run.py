#!/usr/bin/env python3
"""Records, without any write, that the production release path is fail-closed and exactly what is missing.

1. Runs `release_gate.py prerequisites` the way the `validate` job would on this repository's configuration: the variables
   visible through the read-only API (names and values of non-secret variables) and no secret values at all. It must fail.
2. Lists, read-only, which production-signing names exist: repository variables, repository secret NAMES, environments.
   Values of secrets are never readable and never requested.

The output is evidence of a blocked state, not of any signing: it says nothing about whether a production certificate or SignPath
policy would work once configured.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]
WANTED = {
    'variables': ['SIGNPATH_PRODUCTION_POLICY_SLUG', 'SIGNPATH_PRODUCTION_CERT_THUMBPRINT', 'UPDATE_SERVER_URL'],
    'secrets': ['WINDOWS_SIGN_BACKEND', 'TAURI_SIGNING_PRIVATE_KEY', 'TAURI_SIGNING_PRIVATE_KEY_PASSWORD', 'MINISIGN_RELEASE_PRIVATE_KEY',
                'AZURE_SIGN_ENDPOINT', 'AZURE_SIGN_ACCOUNT', 'AZURE_SIGN_PROFILE', 'AZURE_SIGN_DLIB'],
    'environments': ['signpath-production', 'signpath-test'],
}


def gh(path, jq):
    r = subprocess.run(['gh', 'api', path, '--paginate', '--jq', jq], capture_output=True, text=True)
    return r.stdout.split() if r.returncode == 0 else None


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--out', type=Path, required=True)
    p.add_argument('--repo', default='UniClipboard/UniClipboard')
    a = p.parse_args()
    a.out.mkdir(parents=True, exist_ok=False)
    present = {
        'variables': gh(f'repos/{a.repo}/actions/variables', '.variables[].name'),
        'secrets': gh(f'repos/{a.repo}/actions/secrets', '.secrets[].name'),
        'environments': gh(f'repos/{a.repo}/environments', '.environments[].name'),
    }
    env = {k: v for k, v in os.environ.items() if k in ('PATH', 'HOME', 'LANG')}
    r = subprocess.run([sys.executable, '-I', str(ROOT / 'scripts/ci/release_gate.py'), 'prerequisites'], env=env, capture_output=True, text=True)
    (a.out / 'prerequisites.log').write_text(f'exit {r.returncode}\n{r.stdout}{r.stderr}')
    missing = {k: [n for n in WANTED[k] if present[k] is not None and n not in present[k]] for k in WANTED}
    result = {'repository': a.repo, 'prerequisitesExit': r.returncode, 'failsClosed': r.returncode != 0,
              'readOnlyApiAvailable': all(v is not None for v in present.values()),
              'namesPresent': {k: sorted(set(WANTED[k]) & set(v or [])) for k, v in present.items()}, 'namesMissing': missing,
              'productionWindowsSigningAcceptance': 'blocked (not run)',
              'neededFromTheMaintainer': [
                  'ONE production Windows backend: either WINDOWS_SIGN_BACKEND=azure|pfx with its AZURE_SIGN_* / SIGN_PFX_* secrets, '
                  'or SignPath production: variables SIGNPATH_PRODUCTION_POLICY_SLUG and SIGNPATH_PRODUCTION_CERT_THUMBPRINT, Environment '
                  '`signpath-production` holding the secret SIGNPATH_API_TOKEN, and the artifact configurations go-stage1 / go-stage2-setup '
                  'saved in SignPath for that policy',
                  'TAURI_SIGNING_PRIVATE_KEY (and password) for the updater key whose public half is apps/gui-go/app.json',
                  'UPDATE_SERVER_URL variable and the R2 / FlareRelease credentials (not exercised here)']}
    (a.out / 'blocked.json').write_text(json.dumps(result, indent=2, sort_keys=True) + '\n')
    print(json.dumps(result, indent=2, sort_keys=True))
    sys.exit(0 if result['failsClosed'] else 1)


if __name__ == '__main__':
    main()
