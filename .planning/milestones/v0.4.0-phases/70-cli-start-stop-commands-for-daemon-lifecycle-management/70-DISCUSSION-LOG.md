# Phase 70: CLI start/stop commands for daemon lifecycle management - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-03-28
**Phase:** 70-cli-start-stop-commands-for-daemon-lifecycle-management
**Areas discussed:** Command interface, Start behavior, Stop mechanism, Output & feedback
**Mode:** --auto (all decisions auto-selected)

---

## Command Interface

| Option                            | Description                                                          | Selected |
| --------------------------------- | -------------------------------------------------------------------- | -------- |
| Top-level subcommands             | `start` and `stop` as Commands enum variants, same as Status/Devices | auto     |
| Nested under lifecycle subcommand | `lifecycle start` / `lifecycle stop` grouping                        |          |

**User's choice:** [auto] Top-level subcommands — matches existing flat CLI structure
**Notes:** No nesting needed for just two commands

---

## Start Behavior

| Option                                | Description                              | Selected |
| ------------------------------------- | ---------------------------------------- | -------- |
| Background default, --foreground flag | Spawn detached, foreground opt-in via -f | auto     |
| Foreground default, --background flag | Run in foreground by default             |          |

**User's choice:** [auto] Background default — user explicitly requested "default background, optional foreground"

| Option                         | Description                           | Selected |
| ------------------------------ | ------------------------------------- | -------- |
| Idempotent (exit 0 if running) | Print "already running" and succeed   | auto     |
| Error if already running       | Exit non-zero if daemon is already up |          |

**User's choice:** [auto] Idempotent — standard daemon convention

---

## Stop Mechanism

| Option                     | Description                          | Selected |
| -------------------------- | ------------------------------------ | -------- |
| PID file + SIGTERM         | Read PID, send signal, poll for exit | auto     |
| HTTP API shutdown endpoint | POST /shutdown to daemon API         |          |
| Socket-based shutdown      | Send shutdown command via RPC socket |          |

**User's choice:** [auto] PID file + SIGTERM — leverages existing process_metadata.rs, no API dependency

| Option                     | Description                        | Selected |
| -------------------------- | ---------------------------------- | -------- |
| No SIGKILL escalation      | Warn on timeout, leave to user     | auto     |
| Auto SIGKILL after timeout | Force kill if SIGTERM doesn't work |          |

**User's choice:** [auto] No SIGKILL escalation — safer default, user can manually kill

---

## Output & Feedback

| Option                 | Description                                   | Selected |
| ---------------------- | --------------------------------------------- | -------- |
| Human + JSON dual mode | Human-friendly default, --json for structured | auto     |

**User's choice:** [auto] Dual mode — consistent with existing CLI commands

---

## Claude's Discretion

- Exact timeout values for polling
- Internal error message wording
- Whether to add a --timeout flag

## Deferred Ideas

None
