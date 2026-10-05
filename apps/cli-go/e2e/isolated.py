"""Isolated execution environment for running uniclip binaries in tests.

Every invocation of a CLI under test MUST go through `isolated_env()`: a
throwaway HOME (so macOS `~/Library/...` and XDG dirs resolve inside it), a
unique UC_PROFILE, file-based secure storage (no system keychain), and a
no-op system clipboard in any spawned daemon. Running a CLI against the real
HOME can start a daemon on the user's real profile.
"""
import os
import tempfile

_REAL_HOME = os.path.expanduser("~")


def isolated_env(home, profile, extra=None):
    if os.path.realpath(home).startswith(os.path.realpath(_REAL_HOME) + os.sep + "Library"):
        raise RuntimeError("refusing to use a HOME inside the real Library")
    if os.path.realpath(home) == os.path.realpath(_REAL_HOME):
        raise RuntimeError("refusing to run against the real HOME")
    env = {
        "HOME": home,
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "TMPDIR": os.environ.get("TMPDIR", "/tmp"),
        "UC_PROFILE": profile,
        "UNICLIPBOARD_ENV": "development",
        "UC_DISABLE_SYSTEM_CLIPBOARD": "1",
        "NO_COLOR": "1",
        "XDG_DATA_HOME": os.path.join(home, ".local", "share"),
        "XDG_CACHE_HOME": os.path.join(home, ".cache"),
        "XDG_STATE_HOME": os.path.join(home, ".local", "state"),
        "XDG_CONFIG_HOME": os.path.join(home, ".config"),
    }
    if extra:
        env.update(extra)
    return env


def new_home(root, name):
    path = tempfile.mkdtemp(prefix=f"{name}-", dir=root)
    return path
