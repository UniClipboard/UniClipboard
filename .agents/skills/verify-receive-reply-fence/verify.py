#!/usr/bin/env python3
"""Check the finite receive lifecycle model and replay its await boundaries in the GUI."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[3]
SKILL = Path(__file__).resolve().parent
JAR_SHA = '936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88'
JAR_URL = 'https://github.com/tlaplus/tlaplus/releases/download/v1.7.4/tla2tools.jar'


def main():
    assert Path.cwd() == ROOT, 'Run from repository root'
    output = ROOT / 'target/formal-receive-reply'
    output.mkdir(parents=True, exist_ok=True)
    (output / 'receipt.json').unlink(missing_ok=True)
    shutil.copy(SKILL / 'model/ReceiveReply.tla', output)
    shutil.copy(SKILL / 'model/view.mjs', output)
    jar = output / 'tla2tools-1.7.4.jar'
    if not jar.exists():
        with urllib.request.urlopen(JAR_URL, timeout=60) as response:
            payload = response.read()
        assert hashlib.sha256(payload).hexdigest() == JAR_SHA, 'TLC download checksum mismatch'
        jar.write_bytes(payload)
    assert hashlib.sha256(jar.read_bytes()).hexdigest() == JAR_SHA, 'TLC checksum mismatch'
    started = time.monotonic()
    records = []
    with tempfile.TemporaryDirectory(prefix='uc-receive-formal-') as scratch:
        work = Path(scratch)
        shutil.copy(SKILL / 'model/ReceiveReply.tla', work)
        config = (SKILL / 'model/ReceiveReply.cfg').read_text()
        for variant, text, expected in [('current', config, 0),
                ('missing-fence', config.replace('FenceReplies = TRUE', 'FenceReplies = FALSE'), 12)]:
            cfg = work / f'{variant}.cfg'
            cfg.write_text(text)
            (output / cfg.name).write_text(text)
            result = subprocess.run(['java', '-XX:+UseParallelGC', '-cp', str(jar), 'tlc2.TLC',
                '-workers', '1', '-config', str(cfg), 'ReceiveReply'], cwd=work,
                capture_output=True, text=True, timeout=60)
            (output / f'{variant}.log').write_text(result.stdout + result.stderr)
            assert result.returncode == expected, f'{variant}: unexpected TLC exit {result.returncode}; see {output}'
            assert ('NoRetiredCommit is violated' if expected else 'Model checking completed. No error has been found.') in result.stdout
            records.append({'variant': variant, 'exitCode': result.returncode})
    # Use the repository's unified GUI entrypoint and its existing fixture ownership.
    gui = subprocess.run(['node', 'scripts/gui-e2e.mjs', 'layouts', 'receive-generation', '--platform', 'macos'],
        cwd=ROOT, timeout=180)
    hook = 'apps/gui-go/frontend/src/hooks/useEntryReceive.ts'
    receipt = {'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
        'hookSha256': hashlib.sha256((ROOT / hook).read_bytes()).hexdigest(),
        'tlcVersion': '1.7.4', 'tlcSha256': JAR_SHA, 'java': subprocess.check_output(['java', '-version'], stderr=subprocess.STDOUT, text=True).strip(),
        'bounds': {'generations': 3, 'reads': 2, 'inFlightReadsPerEffect': 1},
        'models': records, 'guiExitCode': gui.returncode, 'passed': gui.returncode == 0,
        'elapsedSeconds': round(time.monotonic() - started, 2)}
    (output / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    assert gui.returncode == 0, 'GUI replay failed; read the unified E2E report'
    print(json.dumps(receipt, indent=2))


if __name__ == '__main__':
    main()
