#!/usr/bin/env python3
"""17c6: the AppImage runtime recorded by every package manifest is the pinned one and identical across packages.
The full prefix differs per package only in the runtime's own .digest_md5 section (appimagetool fills it); with that section zeroed the
prefix must equal the pinned runtime file, so its SHA-256 is the pin. Usage: runtime_pin_check.py <package-manifest.json>..."""
import json
import re
import sys
from pathlib import Path

pin = re.search(r"'runtime-aarch64', '([0-9a-f]{64})'", (Path(__file__).resolve().parents[1] / 'package_linux.py').read_text()).group(1)
bad = 0
for m in sys.argv[1:]:
    r = json.loads(Path(m).read_text())['appimage']['runtime']
    e = r['embedded']
    ok = r['fileSha256'] == pin and e['prefixWithDigestZeroedSha256'] == pin and e['squashfsOffset'] == r['fileBytes'] and r['elfMachine'] == 183 \
        and r['revision'][:7] in e['versionReportedByImage']
    print(m, 'PASS' if ok else 'FAIL', json.dumps({'fileSha256': r['fileSha256'], 'prefixZeroed': e['prefixWithDigestZeroedSha256'], 'prefixFull': e['prefixSha256'],
                                                  'digest': e['digestMd5Section'], 'offset': e['squashfsOffset'], 'reported': e['versionReportedByImage']}))
    bad += not ok
print('runtime identity:', 'all packages carry the pinned runtime' if not bad else f'{bad} FAILED')
sys.exit(1 if bad else 0)
