# Phase 67: Setup Filter - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-03-27
**Phase:** 67-setup-filter
**Areas discussed:** Network startup timing, Filtering level, Setup completion criteria, Delayed startup

---

## Network Startup Timing

| Option            | Description                                                                                               | Selected |
| ----------------- | --------------------------------------------------------------------------------------------------------- | -------- |
| Distinguish roles | Sponsor starts network after setup; Joiner temporarily starts during setup but filtered at business layer | ✓        |
| Unified filtering | Network always starts, filter incomplete devices in get_discovered_peers                                  |          |
| Claude decides    | Let Claude choose based on code                                                                           |          |

**User's choice:** Distinguish roles — Sponsor completes setup then starts network; Joiner uses temporary network during setup flow
**Notes:** User explicitly wants role-based behavior. Sponsor must complete setup before being discoverable.

---

## Filtering Level

| Option                       | Description                                                              | Selected |
| ---------------------------- | ------------------------------------------------------------------------ | -------- |
| Daemon doesn't start network | Don't start PeerDiscoveryWorker/libp2p until setup complete              | ✓        |
| mDNS broadcast control       | Network always starts but control mDNS advertising separately            |          |
| Business layer filtering     | Network and mDNS unchanged, filter at peers.changed/get_discovered_peers |          |

**User's choice:** Daemon doesn't start network — control at daemon level by not starting libp2p
**Notes:** User's original suggestion. Simplest approach. Joiner setup flow already has EnsureDiscovery to temporarily start network.

---

## Joiner Visibility During Setup

| Option                    | Description                                        | Selected |
| ------------------------- | -------------------------------------------------- | -------- |
| Acceptable                | Joiner being briefly visible during setup is OK    | ✓        |
| Business layer supplement | Add filtering at peers.changed as defense-in-depth |          |
| Claude decides            | Let Claude choose                                  |          |

**User's choice:** Acceptable — brief visibility during joiner setup is a tolerable trade-off
**Notes:** Focus is on preventing idle/unconfigured devices from being discovered, not absolute invisibility.

---

## Setup Completion Criteria

| Option                      | Description                                          | Selected |
| --------------------------- | ---------------------------------------------------- | -------- |
| Encryption session unlocked | AutoUnlockEncryptionSession success = setup complete | ✓        |
| SpaceAccessState::Granted   | Check SpaceAccessState directly                      |          |
| Claude decides              | Let Claude choose                                    |          |

**User's choice:** Encryption session unlocked — reuses Phase 50 mechanism
**Notes:** Already checked in DaemonApp::run(). Uninitialized = no space yet. Initialized + unlocked = ready.

---

## Delayed Startup After Setup

| Option                 | Description                                                    | Selected |
| ---------------------- | -------------------------------------------------------------- | -------- |
| Setup completion event | Internal event notifies DaemonApp to start PeerDiscoveryWorker | ✓        |
| Restart daemon         | Restart daemon process after setup                             |          |
| Claude decides         | Let Claude choose                                              |          |

**User's choice:** Setup completion event — event-driven delayed startup without daemon restart
**Notes:** User wants seamless experience. New internal event mechanism needed.

---

## Claude's Discretion

- Internal event mechanism design details
- PeerDiscoveryWorker delayed start implementation approach
- How to refactor the unconditional start_network() call

## Deferred Ideas

- mDNS-level selective broadcast control for absolute joiner invisibility
- Business-layer peer filtering as defense-in-depth
