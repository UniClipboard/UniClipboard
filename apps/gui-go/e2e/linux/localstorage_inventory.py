#!/usr/bin/env python3
"""List every WebKit localStorage database under the home directory: where it lives and which keys it holds.

Only key names and value lengths are exported; the value of a key whose name contains "token" is never read into the output.
"""

import json
import sqlite3
import sys
from pathlib import Path

home = Path.home()
found = {}
for base in (home / ".local/share", home / ".cache", home / ".config"):
    for path in sorted(base.rglob("*.localstorage")) if base.exists() else []:
        try:
            db = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
            rows = db.execute("select key, length(value) from ItemTable").fetchall()
            db.close()
        except sqlite3.Error as error:
            found[str(path.relative_to(home))] = {"error": str(error)}
            continue
        found[str(path.relative_to(home))] = {k: n for k, n in sorted(rows)}
Path(sys.argv[1]).write_text(json.dumps(found, indent=2, sort_keys=True) + "\n")
print(f"localstorage databases={len(found)} keys={sum(len(v) for v in found.values())}")
