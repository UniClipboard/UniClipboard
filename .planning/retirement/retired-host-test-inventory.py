#!/usr/bin/env python3
"""Recover test names from the last revision of the retired Rust GUI host."""
import argparse
import json
import re
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--revision', default='de8381d54')
args = parser.parse_args()
files = subprocess.check_output(
    ['git', 'ls-tree', '-r', '--name-only', args.revision, 'crates/uc-tauri/src'], text=True
).splitlines()
rows = []
for path in files:
    source = subprocess.check_output(['git', 'show', f'{args.revision}:{path}'], text=True)
    names = re.findall(
        r'#\[(?:tokio::)?test(?:\([^\]]*\))?\]\s*(?:#\[[^\]]*\]\s*)*(?:async\s+)?fn\s+(\w+)',
        source,
    )
    if names:
        rows.append({'file': path, 'count': len(names), 'tests': names})
print(json.dumps({'revision': args.revision, 'files': len(rows),
                  'tests': sum(row['count'] for row in rows), 'inventory': rows}, indent=2))
