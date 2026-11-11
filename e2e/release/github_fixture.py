#!/usr/bin/env python3
"""Disposable gh command fixture. This does not establish GitHub acceptance."""

import json
import os
from pathlib import Path
import shutil
import sys

root = Path(os.environ["UNIC_GITHUB_FIXTURE"])
state_path = root / "state.json"
state = json.loads(state_path.read_text()) if state_path.exists() else {}
args = sys.argv[1:]


def option(name):
    return args[args.index(name) + 1]


def missing():
    print("HTTP 404: fixture resource missing", file=sys.stderr)
    sys.exit(1)


if args[0] == "api":
    if "--method" in args:
        assert option("--method") == "POST"
        fields = dict(
            value.split("=", 1) for value in args if value.startswith(("ref=", "sha="))
        )
        state["tag"] = {"type": "commit", "sha": fields["sha"]}
        print(json.dumps({"object": state["tag"]}))
    elif "/git/ref/tags/" in args[1]:
        if "tag" not in state:
            missing()
        print(json.dumps({"object": state["tag"]}))
    elif "/releases/tags/" in args[1]:
        if not state.get("release"):
            missing()
        print(json.dumps({"id": 1}))
    else:
        raise AssertionError(args)
elif args[:2] == ["release", "create"]:
    assert "--verify-tag" in args and "tag" in state
    state["release"] = True
elif args[:2] == ["release", "upload"]:
    source = Path(args[3])
    (root / "assets").mkdir(exist_ok=True)
    shutil.copyfile(source, root / "assets" / source.name)
elif args[:2] == ["release", "view"]:
    print(
        json.dumps({"assets": [{"name": p.name} for p in (root / "assets").glob("*")]})
    )
elif args[:2] == ["release", "download"]:
    source = root / "assets" / option("--pattern")
    if not source.exists():
        missing()
    destination = Path(option("--dir"))
    destination.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination / source.name)
elif args[:2] == ["release", "edit"]:
    state["finalized"] = True
else:
    raise AssertionError(args)
state_path.write_text(json.dumps(state))
