"""Installed CLI processes, terminal detection and frozen Rust migration data."""

from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import platform
import select
import sqlite3
import subprocess
import tempfile


def frozen_source(root, source):
    vault = source / "vault"
    vault.mkdir(parents=True)
    fixture = json.loads(
        (root / "internal/v1migration/testdata/v3.json").read_text()
    )
    for name, value in fixture["vault"].items():
        (vault / name).write_text(json.dumps(value))
    payload = (
        source
        / "profile-data-generations"
        / fixture["generation"]
        / "v3-payloads"
    )
    payload.mkdir(parents=True)
    dbpath = payload / "profile.sqlite"
    with closing(sqlite3.connect(dbpath)) as db, db:
        db.executescript("""
    CREATE TABLE clipboard_entry(entry_id TEXT PRIMARY KEY,event_id TEXT,created_at_ms INTEGER,active_time_ms INTEGER,content_category TEXT,deleted_at_ms INTEGER);
    CREATE TABLE clipboard_snapshot_representation(id TEXT PRIMARY KEY,event_id TEXT,mime_type TEXT,format_id TEXT,inline_data BLOB,blob_id TEXT,size_bytes INTEGER);
    CREATE TABLE entry_file_set(entry_id TEXT,line_index INTEGER,kind TEXT,blob_id TEXT,size_bytes INTEGER,content_hash TEXT,original_text_ct BLOB,relative_path_ct BLOB,root_name_ct BLOB,kind_tag TEXT,root_index INTEGER);
    CREATE TABLE blob(blob_id TEXT PRIMARY KEY,content_hash TEXT,size_bytes INTEGER,storage_path TEXT);
    CREATE TABLE history_tag(tag_id TEXT PRIMARY KEY,payload_ct BLOB);
    CREATE TABLE history_tag_assignment(entry_id TEXT PRIMARY KEY,tags_ct BLOB);
    INSERT INTO clipboard_entry VALUES('entry-text','event-text',1000,1100,'text',NULL),('entry-bad','event-bad',4000,4100,'text',NULL);
    """)
        db.execute(
            "INSERT INTO clipboard_snapshot_representation VALUES('rep-text','event-text','text/plain','text/plain',?,NULL,12)",
            [bytes(fixture["text"])],
        )
        db.execute(
            "INSERT INTO history_tag VALUES('tag-one',?)",
            [bytes(fixture["tag"])],
        )
        db.execute(
            "INSERT INTO history_tag_assignment VALUES('entry-text',?)",
            [bytes(fixture["assignment"])],
        )
    return fixture, dbpath


def accept_migration(root, installed, temp, env, execute, check, report):
    original = dict(env)
    cli = installed / "node_modules/.bin/unic"
    home = temp / "upgrade-home"
    env.update(
        HOME=str(home),
        USERPROFILE=str(home),
        XDG_DATA_HOME=str(home / "data"),
        LOCALAPPDATA=str(home / "local"),
        XDG_CONFIG_HOME=str(home / "config"),
        UNIC_HOME=str(home / "v2"),
    )
    env.pop("UC_PROFILE", None)
    base = (
        home / "Library/Application Support"
        if platform.system() == "Darwin"
        else Path(env["LOCALAPPDATA"] if os.name == "nt" else env["XDG_DATA_HOME"])
    )
    legacy = base / "app.uniclipboard.desktop"

    def terminal(args, expected=0):
        master, slave = os.openpty()
        try:
            result = subprocess.run(
                [str(cli), *args],
                cwd=root,
                env=env,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=slave,
                text=True,
            )
            data = b""
            # macOS discards queued PTY output when its last slave is closed.
            # Keep our slave open until all finite notice output is captured.
            while select.select([master], [], [], 0.05)[0]:
                chunk = os.read(master, 4096)
                if not chunk:
                    break
                data += chunk
        finally:
            os.close(slave)
            os.close(master)
        assert result.returncode == expected
        report["commands"].append(
            {
                "argv": [str(cli).replace(str(temp), "<TEMP>"), *args],
                "terminal_stderr": True,
                "exit_code": result.returncode,
                "stdout": result.stdout,
                "stderr": data.decode(),
            }
        )
        return result.stdout, data.decode()

    try:
        if os.name != "nt":
            _, notice = terminal(["--version"])
            check(
                "fresh installation has no v1 warning", not notice and not home.exists()
            )
            legacy.parent.mkdir(parents=True)
            legacy.write_text("not a data directory")
            _, notice = terminal(["--version"])
            check("a non-directory v1 path has no warning", not notice)
            legacy.unlink()
            legacy.mkdir()
            _, notice = terminal(["--version"])
            check("version stays informational when v1 exists", not notice)
            guide = execute([cli, "migrate", "--help"])
            check(
                "migration guide explains the required explicit initialization",
                "--initialize-empty" in guide.stdout
                and "migration_required" in guide.stdout
                and "unic init --fresh" in guide.stdout,
            )
            value, notice = terminal(["--version", "--json"])
            check(
                "terminal JSON remains clean when v1 exists",
                json.loads(value)["program"] == "unic" and not notice,
            )
            value, notice = terminal(
                [
                    "migrate",
                    "--dry-run",
                    "--json=true",
                    "--source",
                    str(temp / "missing-source"),
                    "--passphrase-stdin",
                ],
                1,
            )
            check(
                "invalid source returns actionable JSON before target initialization",
                not value and json.loads(notice)["reason"] == "source_invalid"
                and not Path(env["UNIC_HOME"]).exists(),
            )
            quiet = execute([cli, "--version"])
            check("noninteractive output omits migration warning", not quiet.stderr)
            legacy.rmdir()
            env["UC_PROFILE"] = "testing"
            named = base / "app.uniclipboard.desktop-testing"
            named.mkdir()
            _, notice = terminal(["--version"])
            check(
                "version stays informational for a named v1 profile",
                not notice,
            )
            env.pop("UC_PROFILE")
        else:
            report["not_run"].append("Windows terminal informational version acceptance")
        guide = execute([cli, "migrate", "--help"])
        check(
            "offline migration guide help",
            "--passphrase-stdin" in guide.stdout
            and "pair devices again" in guide.stdout
            and "--initialize-empty" in guide.stdout
            and not Path(env["UNIC_HOME"]).exists(),
        )
        bare_missing = execute([cli, "migrate"], expected=1)
        check(
            "bare migrate without v1 source fails cleanly without creating target",
            "unic init" in bare_missing.stderr
            and not Path(env["UNIC_HOME"]).exists(),
        )

        # Use canonical private paths, not macOS's symlinked /var temp root.
        with tempfile.TemporaryDirectory(
            prefix="unic-v1-import-",
            dir="/private/tmp" if platform.system() == "Darwin" else None,
        ) as work:
            work = Path(work)
            source, target = work / "source", work / "target"
            fixture, dbpath = frozen_source(root, source)

            def snapshot():
                return {
                    str(p.relative_to(source)): hashlib.sha256(
                        p.read_bytes()
                    ).hexdigest()
                    for p in source.rglob("*")
                    if p.is_file()
                }

            before = snapshot()
            command = [
                cli,
                "migrate",
                "--source",
                source,
                "--target",
                target,
                "--passphrase-stdin",
                "--json",
            ]

            def migrate(extra=(), expected=0):
                result = execute(
                    [*command, *extra], expected, fixture["passphrase"] + "\n"
                )
                return json.loads(result.stdout.splitlines()[-1])

            dry = migrate(["--dry-run"], 3)
            check(
                "installed migration preflight reports partial without creating target",
                dry["status"] == "partial" and not target.exists(),
            )
            partial = migrate(expected=3)
            check(
                "installed migration preserves partial exit code and verifies imported content",
                partial["status"] == "partial"
                and partial["imported"] == 1
                and partial["verified"] == 1,
            )
            check("migration does not modify v1 source", before == snapshot())
            with sqlite3.connect(dbpath) as db:
                db.execute(
                    "INSERT INTO clipboard_snapshot_representation VALUES('rep-bad','event-bad','text/plain','text/plain',?,NULL,9)",
                    [bytes(fixture["repaired"])],
                )
            before = snapshot()
            resumed = migrate()
            check(
                "installed migration resumes only the repaired record",
                resumed["status"] == "complete"
                and resumed["imported"] == 1
                and resumed["skipped"] == 1
                and resumed["verified"] == 2,
            )
            again = migrate()
            check(
                "installed migration retry does not duplicate history",
                again["imported"] == 0
                and again["skipped"] == 2
                and again["verified"] == 2,
            )
            check(
                "migration retries leave repaired source unchanged",
                before == snapshot(),
            )
    finally:
        env.clear()
        env.update(original)
