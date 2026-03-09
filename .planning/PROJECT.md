# UniClipboard Desktop

## What This Is

A cross-platform clipboard synchronization app built with Tauri 2, React, and Rust. It provides encrypted LAN clipboard sync for text and images, with streaming transfer paths and optimized large-image handling for practical daily use across desktop platforms.

## Core Value

Seamless clipboard synchronization across devices — users can copy on one device and paste on another without interrupting their workflow.

## Current State

- **Latest shipped milestone:** v0.1.0 (consolidated phases 1-9)
- **Current capability level:** Daily-driver baseline for encrypted text/image sync
- **Architecture status:** Hexagonal migration partially complete; additional boundary hardening remains

## Requirements

### Validated

- ✓ Clipboard text capture and history — existing
- ✓ Device pairing and LAN sync baseline — existing
- ✓ V2 unified transfer and streaming decode foundation — v0.1.0
- ✓ At-rest blob format optimization and migration — v0.1.0
- ✓ Windows image clipboard capture reliability — v0.1.0
- ✓ Dashboard image display compatibility across platforms — v0.1.0
- ✓ Setup flow UX consistency improvements — v0.1.0
- ✓ V3 binary sync protocol, compression, and zero-copy fanout — v0.1.0
- ✓ Large-image clipboard read pipeline memory/latency improvements — v0.1.0

### Active

- [ ] Quick-paste floating window with global hotkey
- [ ] Auto-paste selected history item into previous app
- [ ] Clipboard history search/favorites/filtering enhancements
- [ ] Architecture boundary remediation (issue #214 clusters A-D)
- [ ] Lifecycle/task shutdown governance improvements
- [ ] Command DTO/error contract hardening

### Out of Scope

- WebDAV cross-internet sync — deferred
- File synchronization — deferred
- Mobile app — desktop-first
- OAuth/third-party login — not required for current product model

## Next Milestone Goals

- Address highest-risk architecture and lifecycle defects from deep review findings.
- Keep user-facing clipboard sync experience stable while refactoring boundaries.
- Prepare cleaner contracts for faster future feature iteration.

## Context

Shipped v0.1.0 across phases 1-9 with major transfer, image, and UX improvements.
Tech stack remains Tauri 2 + React 18 + Rust + libp2p + XChaCha20-Poly1305.
Large-payload transfer and large-image capture paths were materially optimized in this milestone.

## Key Decisions

| Decision                                                        | Rationale                                                     | Outcome    |
| --------------------------------------------------------------- | ------------------------------------------------------------- | ---------- |
| Two-segment framing for clipboard wire format                   | Reduce overhead and enable stream decode                      | ✓ Good     |
| V3 binary protocol with Arc fanout                              | Improve large payload performance and memory behavior         | ✓ Good     |
| Manual uc:// URL resolution strategy                            | Ensure Windows/WebView compatibility                          | ✓ Good     |
| Background TIFF conversion                                      | Keep clipboard capture path responsive                        | ✓ Good     |
| Architecture deep-review remediation deferred to next milestone | Prevent mixing large refactor with delivery-focused milestone | ⚠️ Revisit |

## Constraints

- **Tech stack:** Tauri 2 + React + Rust (fixed)
- **Sync domain:** LAN-first with libp2p
- **Security:** XChaCha20-Poly1305 remains mandatory
- **Platform support:** macOS primary; Windows/Linux supported

---

_Last updated: 2026-03-06 after v0.1.0 milestone completion_
