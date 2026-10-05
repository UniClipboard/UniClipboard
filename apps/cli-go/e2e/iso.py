#!/usr/bin/env python3
"""Run one uniclip binary inside the shared isolated test HOME.

Usage: iso.py <binary> [args...]   (profile: $ISO_PROFILE or compat-iso)
"""
import os
import subprocess
import sys
import tempfile

from isolated import isolated_env

home = os.path.join(tempfile.gettempdir(), "uniclip-iso-home")
os.makedirs(home, exist_ok=True)
env = isolated_env(home, os.environ.get("ISO_PROFILE", "compat-iso"))
sys.exit(subprocess.call(sys.argv[1:], env=env))
