#!/usr/bin/env python3
"""Validate the production app and share profiles before archive signing."""

import json
from pathlib import Path
import plistlib
import subprocess
import sys

from mobile import mobile_metadata


app = mobile_metadata("ios")["application_id"]
group = f"group.{app}.incoming-share"
profiles = {}
team = None
directory = Path.home() / "Library/Developer/Xcode/UserData/Provisioning Profiles"
directory.mkdir(parents=True, exist_ok=True)
for bundle, path in zip((app, app + ".Share"), sys.argv[1:], strict=True):
    profile = plistlib.loads(subprocess.check_output(["security", "cms", "-D", "-i", path]))
    identifier, = profile["TeamIdentifier"]
    if team is not None and identifier != team:
        raise ValueError("iOS production profiles must use the same team")
    team = identifier
    entitlements = profile["Entitlements"]
    if entitlements["application-identifier"] != team + "." + bundle:
        raise ValueError("iOS production profile application identifier mismatch")
    if group not in entitlements.get("com.apple.security.application-groups", []):
        raise ValueError("iOS production share group is missing from the profile")
    if entitlements.get("get-task-allow") or profile.get("ProvisionedDevices") or profile.get("ProvisionsAllDevices"):
        raise ValueError("iOS archive export requires App Store distribution profiles")
    profiles[bundle] = profile["Name"]
    (directory / (profile["UUID"] + ".mobileprovision")).write_bytes(Path(path).read_bytes())
print(json.dumps({"team": team, "profiles": profiles}))
