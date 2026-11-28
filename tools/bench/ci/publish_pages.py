#!/usr/bin/env python3
"""Publish rendered desktop benchmark dashboards into `<pages-dir>/bench/`."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import html
import json
import os
from pathlib import Path
import re
import shutil
import sys
import tempfile
from typing import Any
from urllib.parse import urlsplit

RUN_ID_RE = re.compile(r"^[A-Za-z0-9._-]+$")

HISTORY_CSP = (
    "default-src 'none'; "
    "style-src 'unsafe-inline'; "
    "img-src 'none'; "
    "script-src 'none'; "
    "base-uri 'none'; "
    "form-action 'none';"
)


class PublishError(ValueError):
    """Raised when fail-closed preflight or safety validation fails."""


def validate_run_id(run_id: str) -> str:
    value = run_id.strip()
    if not value or value in (".", "..") or not RUN_ID_RE.fullmatch(value):
        raise PublishError(
            f"invalid run_id {run_id!r}: must match ^[A-Za-z0-9._-]+$ and not be '.' or '..'"
        )
    return value


def validate_run_url(run_url: str) -> str:
    value = run_url.strip()
    if not value:
        return ""
    if any(ch.isspace() or ord(ch) < 32 for ch in value):
        raise PublishError(f"invalid run_url {run_url!r}: contains whitespace or control characters")
    parsed = urlsplit(value)
    if parsed.scheme.lower() not in ("http", "https") or not parsed.netloc:
        raise PublishError(f"invalid run_url {run_url!r}: must use http:// or https:// with host")
    return value


def parse_timestamp_utc(raw: str) -> tuple[str, dt.datetime]:
    value = raw.strip()
    if not value:
        now = dt.datetime.now(dt.timezone.utc).replace(microsecond=0)
        return now.strftime("%Y-%m-%dT%H:%M:%SZ"), now
    normalized = value[:-1] + "+00:00" if value.endswith("Z") else value
    try:
        parsed = dt.datetime.fromisoformat(normalized)
    except ValueError as exc:
        raise PublishError(f"invalid timestamp {raw!r}: {exc}") from exc
    if parsed.tzinfo is None:
        raise PublishError(f"invalid timestamp {raw!r}: timezone offset or 'Z' required")
    return value, parsed.astimezone(dt.timezone.utc)


def ensure_safe_subpath(root_resolved: Path, target: Path, expect_dir: bool | None = None) -> None:
    if target.is_symlink():
        raise PublishError(f"refusing symlink path inside pages tree: {target}")
    resolved = target.resolve()
    try:
        resolved.relative_to(root_resolved)
    except ValueError as exc:
        raise PublishError(f"path escapes pages directory: {target} -> {resolved}") from exc
    if target.exists():
        if expect_dir is True and not target.is_dir():
            raise PublishError(f"expected directory at {target}")
        if expect_dir is False and not target.is_file():
            raise PublishError(f"expected regular file at {target}")


def ensure_summary_outside_pages(pages_resolved: Path, summary_out: Path) -> None:
    if summary_out.is_symlink():
        raise PublishError(f"refusing symlink summary-out path: {summary_out}")
    for candidate in (summary_out.resolve(), summary_out.parent.resolve()):
        if candidate.is_relative_to(pages_resolved):
            raise PublishError(
                f"summary-out must be outside pages-dir ({pages_resolved}): {summary_out}"
            )


def read_dashboard_html(path: Path) -> tuple[bytes, str]:
    if not path.exists() or not path.is_file():
        raise PublishError(f"dashboard html missing or not a file: {path}")
    try:
        raw = path.read_bytes()
    except OSError as exc:
        raise PublishError(f"unable to read dashboard html {path}: {exc}") from exc
    if not raw.strip():
        raise PublishError(f"dashboard html is empty: {path}")
    try:
        raw.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise PublishError(f"dashboard html is not valid UTF-8 ({path}): {exc}") from exc
    digest = hashlib.sha256(raw).hexdigest()
    return raw, digest


def _require_opt_str(item: dict[str, Any], key: str, idx: int) -> str:
    val = item.get(key, "")
    if not isinstance(val, str):
        raise PublishError(f"existing manifest runs[{idx}].{key} must be JSON string")
    return val


def load_existing_manifest(manifest_path: Path) -> list[dict[str, Any]]:
    if not manifest_path.exists():
        return []
    try:
        data = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise PublishError(f"existing manifest is invalid ({manifest_path}): {exc}") from exc
    if not isinstance(data, dict):
        raise PublishError(f"existing manifest root must be JSON object: {manifest_path}")
    schema_version = data.get("schema_version")
    if type(schema_version) is not int or schema_version != 1:
        raise PublishError(
            f"existing manifest schema_version must be integer 1, got {schema_version!r}: {manifest_path}"
        )
    if "runs" not in data or not isinstance(data["runs"], list):
        raise PublishError(f"existing manifest must contain 'runs' JSON array: {manifest_path}")
    raw_runs = data["runs"]
    cleaned: list[dict[str, Any]] = []
    for idx, item in enumerate(raw_runs):
        if not isinstance(item, dict):
            raise PublishError(f"existing manifest runs[{idx}] must be JSON object")
        raw_id = item.get("run_id")
        if not isinstance(raw_id, str):
            raise PublishError(f"existing manifest runs[{idx}].run_id must be non-empty JSON string")
        old_id = validate_run_id(raw_id)
        ts_raw = item.get("timestamp")
        if not isinstance(ts_raw, str) or not ts_raw.strip():
            raise PublishError(f"existing manifest runs[{idx}].timestamp must be non-empty ISO-8601 string")
        ts_norm, _ = parse_timestamp_utc(ts_raw)
        old_url = validate_run_url(_require_opt_str(item, "run_url", idx))
        targets_raw = item.get("targets", [])
        if isinstance(targets_raw, str):
            targets_list = [t.strip() for t in targets_raw.split(",") if t.strip()]
        elif isinstance(targets_raw, list) and all(isinstance(t, str) for t in targets_raw):
            targets_list = [t.strip() for t in targets_raw if t.strip()]
        else:
            raise PublishError(f"existing manifest runs[{idx}].targets has invalid type")
        cleaned.append(
            {
                "run_id": old_id,
                "timestamp": ts_norm,
                "commit": _require_opt_str(item, "commit", idx),
                "branch": _require_opt_str(item, "branch", idx),
                "event": _require_opt_str(item, "event", idx),
                "run_url": old_url,
                "targets": targets_list,
                "path": f"runs/{old_id}/index.html",
                "dashboard_sha256": _require_opt_str(item, "dashboard_sha256", idx),
            }
        )
    return cleaned


def atomic_write_bytes(target: Path, data: bytes) -> None:
    target.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp_name = tempfile.mkstemp(prefix=".tmp-", dir=str(target.parent))
    try:
        with os.fdopen(fd, "wb") as handle:
            handle.write(data)
        os.chmod(tmp_name, 0o644)
        os.replace(tmp_name, target)
    except Exception:
        try:
            os.unlink(tmp_name)
        except OSError:
            pass
        raise


def render_history_html(manifest: dict[str, Any]) -> str:
    e = lambda value: html.escape(str(value), quote=True)
    runs = manifest["runs"]
    rows_html: list[str] = []
    for idx, run in enumerate(runs):
        rid = run["run_id"]
        ts = run["timestamp"]
        commit = run["commit"]
        short_commit = commit[:12] if commit else "-"
        branch = run["branch"] or "-"
        event = run["event"] or "-"
        targets = ", ".join(run["targets"]) if run["targets"] else "-"
        run_path = f"runs/{rid}/index.html"
        sha = run["dashboard_sha256"]
        short_sha = sha[:12] if sha else "-"
        badge = ' <span class="badge">latest</span>' if idx == 0 else ""
        if run["run_url"]:
            ci_cell = (
                f'<a href="{e(run["run_url"])}" target="_blank" '
                f'rel="noopener noreferrer">Workflow run</a>'
            )
        else:
            ci_cell = "-"
        rows_html.append(
            "        <tr>\n"
            f'          <td><a href="{e(run_path)}">{e(rid)}</a>{badge}</td>\n'
            f"          <td>{e(ts)}</td>\n"
            f"          <td><code>{e(short_commit)}</code></td>\n"
            f"          <td>{e(branch)}</td>\n"
            f"          <td>{e(event)}</td>\n"
            f"          <td>{e(targets)}</td>\n"
            f"          <td><code>{e(short_sha)}</code></td>\n"
            f"          <td>{ci_cell}</td>\n"
            "        </tr>"
        )

    rows_block = "\n".join(rows_html) if rows_html else (
        '        <tr><td colspan="8">No benchmark runs published yet.</td></tr>'
    )

    return (
        "<!doctype html>\n"
        '<html lang="en">\n'
        "<head>\n"
        '  <meta charset="utf-8">\n'
        '  <meta name="viewport" content="width=device-width, initial-scale=1">\n'
        f'  <meta http-equiv="Content-Security-Policy" content="{HISTORY_CSP}">\n'
        "  <title>Desktop Resource Benchmark History</title>\n"
        "  <style>\n"
        "    :root { color-scheme: light dark; font-family: system-ui, -apple-system, sans-serif; }\n"
        "    body { margin: 24px auto; max-width: 1120px; padding: 0 16px; line-height: 1.5; }\n"
        "    header { display: flex; flex-wrap: wrap; gap: 12px; align-items: baseline; justify-content: space-between; margin-bottom: 16px; }\n"
        "    nav a { margin-right: 12px; }\n"
        "    .meta { opacity: 0.8; font-size: 0.92rem; margin-bottom: 16px; }\n"
        "    .table-wrap { overflow-x: auto; border: 1px solid color-mix(in srgb, currentColor 18%, transparent); border-radius: 8px; }\n"
        "    table { width: 100%; border-collapse: collapse; font-size: 0.92rem; }\n"
        "    th, td { text-align: left; padding: 10px 12px; border-bottom: 1px solid color-mix(in srgb, currentColor 12%, transparent); white-space: nowrap; }\n"
        "    th { font-weight: 600; }\n"
        "    tr:last-child td { border-bottom: none; }\n"
        "    .badge { display: inline-block; margin-left: 6px; padding: 1px 6px; border-radius: 999px; font-size: 0.75rem; border: 1px solid currentColor; opacity: 0.8; }\n"
        "  </style>\n"
        "</head>\n"
        "<body>\n"
        "  <header>\n"
        "    <h1>Desktop Resource Benchmark History</h1>\n"
        "    <nav>\n"
        '      <a href="index.html">Latest Dashboard</a>\n'
        '      <a href="manifest.json">manifest.json</a>\n'
        "    </nav>\n"
        "  </header>\n"
        f'  <p class="meta">Retained runs: {len(runs)} / max {e(manifest["max_runs"])} · '
        f'Latest run: <code>{e(manifest["latest_run_id"])}</code> · '
        f'Generated at: <code>{e(manifest["generated_at"])}</code></p>\n'
        '  <div class="table-wrap">\n'
        "    <table>\n"
        "      <thead>\n"
        "        <tr>\n"
        "          <th>Run ID</th>\n"
        "          <th>Timestamp (UTC)</th>\n"
        "          <th>Commit</th>\n"
        "          <th>Branch</th>\n"
        "          <th>Event</th>\n"
        "          <th>Targets</th>\n"
        "          <th>Dashboard SHA-256</th>\n"
        "          <th>CI Run</th>\n"
        "        </tr>\n"
        "      </thead>\n"
        "      <tbody>\n"
        f"{rows_block}\n"
        "      </tbody>\n"
        "    </table>\n"
        "  </div>\n"
        "</body>\n"
        "</html>\n"
    )


def render_summary_markdown(entry: dict[str, Any], retained_count: int, max_runs: int) -> str:
    targets_text = ", ".join(entry["targets"]) if entry["targets"] else "(unspecified)"
    lines = [
        "## GitHub Pages Benchmark Publish",
        "",
        f"- **Run ID**: `{entry['run_id']}`",
        f"- **Timestamp (UTC)**: `{entry['timestamp']}`",
        f"- **Commit**: `{entry['commit'] or '(unspecified)'}`",
        f"- **Branch**: `{entry['branch'] or '(unspecified)'}`",
        f"- **Event**: `{entry['event'] or '(unspecified)'}`",
        f"- **Targets**: `{targets_text}`",
        f"- **Retained runs**: `{retained_count}` (max `{max_runs}`)",
        f"- **Dashboard SHA-256**: `{entry['dashboard_sha256']}`",
        f"- **Published paths**: `bench/index.html`, `bench/history.html`, `bench/manifest.json`, `bench/{entry['path']}`",
    ]
    if entry["run_url"]:
        lines.append(f"- **Workflow run**: {entry['run_url']}")
    return "\n".join(lines) + "\n"


def publish_pages(
    *,
    pages_dir: Path,
    dashboard_html: Path,
    run_id: str,
    timestamp: str = "",
    commit: str = "",
    branch: str = "",
    event: str = "",
    run_url: str = "",
    targets: str = "",
    max_runs: int = 30,
    summary_out: Path | None = None,
) -> dict[str, Any]:
    if not pages_dir.exists() or not pages_dir.is_dir():
        raise PublishError(f"pages-dir does not exist or is not a directory: {pages_dir}")
    if max_runs < 1:
        raise PublishError(f"max-runs must be >= 1, got {max_runs}")

    clean_run_id = validate_run_id(run_id)
    clean_run_url = validate_run_url(run_url)
    clean_ts, _ = parse_timestamp_utc(timestamp)
    targets_list = [t.strip() for t in targets.split(",") if t.strip()]

    dashboard_bytes, dashboard_sha256 = read_dashboard_html(dashboard_html)

    pages_resolved = pages_dir.resolve()
    if summary_out is not None:
        ensure_summary_outside_pages(pages_resolved, summary_out)

    bench_dir = pages_dir / "bench"
    runs_dir = bench_dir / "runs"
    manifest_path = bench_dir / "manifest.json"
    index_path = bench_dir / "index.html"
    history_path = bench_dir / "history.html"
    run_dir = runs_dir / clean_run_id
    run_index_path = run_dir / "index.html"

    ensure_safe_subpath(pages_resolved, bench_dir, expect_dir=True)
    if bench_dir.exists():
        ensure_safe_subpath(pages_resolved, runs_dir, expect_dir=True)
        ensure_safe_subpath(pages_resolved, manifest_path, expect_dir=False)
        ensure_safe_subpath(pages_resolved, index_path, expect_dir=False)
        ensure_safe_subpath(pages_resolved, history_path, expect_dir=False)
    if runs_dir.exists():
        ensure_safe_subpath(pages_resolved, run_dir, expect_dir=True)
        ensure_safe_subpath(pages_resolved, run_index_path, expect_dir=False)

    existing_runs = load_existing_manifest(manifest_path)

    # Validate any candidate directories that may be pruned before modifying anything.
    new_entry: dict[str, Any] = {
        "run_id": clean_run_id,
        "timestamp": clean_ts,
        "commit": commit.strip(),
        "branch": branch.strip(),
        "event": event.strip(),
        "run_url": clean_run_url,
        "targets": targets_list,
        "path": f"runs/{clean_run_id}/index.html",
        "dashboard_sha256": dashboard_sha256,
    }
    merged = [item for item in existing_runs if item["run_id"] != clean_run_id]
    merged.append(new_entry)
    merged.sort(key=lambda item: (parse_timestamp_utc(item["timestamp"])[1], item["run_id"]), reverse=True)

    retained = merged[:max_runs]
    pruned = merged[max_runs:]
    runs_resolved = (pages_resolved / "bench" / "runs")
    for old in pruned:
        old_id = validate_run_id(old["run_id"])
        old_dir = runs_dir / old_id
        if old_dir.is_symlink():
            raise PublishError(f"refusing to prune symlink directory: {old_dir}")
        if old_dir.exists():
            resolved_old = old_dir.resolve()
            if resolved_old.parent != runs_resolved:
                raise PublishError(f"refusing to prune unexpected path: {old_dir} -> {resolved_old}")

    latest_run_id = retained[0]["run_id"]
    if latest_run_id == clean_run_id:
        latest_bytes = dashboard_bytes
    else:
        latest_dir = runs_dir / latest_run_id
        latest_candidate = latest_dir / "index.html"
        ensure_safe_subpath(runs_resolved, latest_dir, expect_dir=True)
        ensure_safe_subpath(runs_resolved, latest_candidate, expect_dir=False)
        latest_bytes, _ = read_dashboard_html(latest_candidate)

    # All preflight checks succeeded; perform isolated mutations strictly inside `<pages-dir>/bench/`.
    runs_dir.mkdir(parents=True, exist_ok=True)
    atomic_write_bytes(run_index_path, dashboard_bytes)
    atomic_write_bytes(index_path, latest_bytes)

    for old in pruned:
        old_dir = runs_dir / old["run_id"]
        if old_dir.exists() and old_dir.is_dir() and not old_dir.is_symlink():
            shutil.rmtree(old_dir)

    generated_at = dt.datetime.now(dt.timezone.utc).replace(microsecond=0).strftime("%Y-%m-%dT%H:%M:%SZ")
    manifest = {
        "schema_version": 1,
        "generated_at": generated_at,
        "max_runs": max_runs,
        "latest_run_id": latest_run_id,
        "runs": retained,
    }
    atomic_write_bytes(manifest_path, (json.dumps(manifest, indent=2) + "\n").encode("utf-8"))
    atomic_write_bytes(history_path, render_history_html(manifest).encode("utf-8"))

    if summary_out is not None:
        summary_md = render_summary_markdown(new_entry, len(retained), max_runs)
        atomic_write_bytes(summary_out, summary_md.encode("utf-8"))

    return manifest


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Publish desktop benchmark dashboard into <pages-dir>/bench/"
    )
    parser.add_argument("--pages-dir", required=True, help="Root directory of checked-out gh-pages")
    parser.add_argument("--dashboard-html", required=True, help="Rendered HTML dashboard input file")
    parser.add_argument("--run-id", required=True, help="Unique workflow run identifier")
    parser.add_argument("--timestamp", default="", help="UTC RFC3339 timestamp (default: now)")
    parser.add_argument("--commit", default="", help="Git commit SHA")
    parser.add_argument("--branch", default="", help="Git branch/ref name")
    parser.add_argument("--event", default="", help="GitHub event name")
    parser.add_argument("--run-url", default="", help="HTTP(S) URL of workflow run")
    parser.add_argument("--targets", default="", help="Comma-separated benchmark targets")
    parser.add_argument("--max-runs", type=int, default=30, help="Maximum retained runs (default: 30)")
    parser.add_argument("--summary-out", default="", help="Optional Markdown summary output file")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    summary_path = Path(args.summary_out) if args.summary_out else None
    try:
        publish_pages(
            pages_dir=Path(args.pages_dir),
            dashboard_html=Path(args.dashboard_html),
            run_id=args.run_id,
            timestamp=args.timestamp,
            commit=args.commit,
            branch=args.branch,
            event=args.event,
            run_url=args.run_url,
            targets=args.targets,
            max_runs=args.max_runs,
            summary_out=summary_path,
        )
    except PublishError as exc:
        sys.stderr.write(f"publish_pages error: {exc}\n")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
