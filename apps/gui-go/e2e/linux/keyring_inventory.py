#!/usr/bin/env python3
"""List the Secret Service items of the session keyring: label, attribute names and values, and a salted digest of the secret.

The digest uses a per-container salt (--salt-file) that never leaves the container, so two inventories taken in the same run can be
compared for secret equality while the exported file carries no value that identifies a secret. Secret values are never written.
"""

import hashlib
import json
import os
import sys
from pathlib import Path

import secretstorage

salt_file = Path(sys.argv[2])
if not salt_file.exists():
    salt_file.write_bytes(os.urandom(32))
salt = salt_file.read_bytes()
connection = secretstorage.dbus_init()
items = []
for collection in secretstorage.get_all_collections(connection):
    if collection.is_locked():
        collection.unlock()
    for item in collection.get_all_items():
        items.append(
            {
                "collection": collection.get_label(),
                "label": item.get_label(),
                "attributes": dict(sorted(item.get_attributes().items())),
                "secretDigest": hashlib.sha256(salt + item.get_secret()).hexdigest(),
            }
        )
items.sort(key=lambda i: (i["collection"], i["label"], json.dumps(i["attributes"], sort_keys=True)))
Path(sys.argv[1]).write_text(json.dumps(items, indent=2) + "\n")
print(f"keyring items={len(items)}")
