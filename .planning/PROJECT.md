# UniClipboard Desktop

## What This Is

A cross-platform clipboard synchronization tool built with Tauri 2, React, and Rust. It enables real-time clipboard sharing between devices on LAN via libp2p, with XChaCha20-Poly1305 per-chunk encryption for security. Supports multi-representation payloads (text, image, rich text) with unified chunked transfer and streaming decode.

## Core Value

Seamless clipboard synchronization across devices — users can copy on one device and paste on another without interrupting their workflow.

## Requirements

### Validated

- ✓ Clipboard text capture and history — existing
- ✓ XChaCha20-Poly1305 encryption for clipboard content — existing
- ✓ Device pairing via LAN discovery — existing
- ✓ WebSocket-based LAN sync for text — existing
- ✓ Clipboard history list view in main window — existing
- ✓ Dark/light theme support — existing
- ✓ Settings management (general, network, security) — existing
- ✓ System tray integration — existing
- ✓ Auto-start on boot — existing
- ✓ Single instance enforcement — existing
- ✓ Hexagonal architecture (uc-core, uc-infra, uc-platform, uc-app) — existing (~60%)
- ✓ Download progress display in update dialogs — v0.1.0 (Phase 01)
- ✓ V2 payload version with backward compatibility — v0.1.0 (UTL-01)
- ✓ V2 protocol types (multi-rep payload, wire repr, AAD) — v0.1.0 (UTL-02)
- ✓ Streaming chunked encoder — v0.1.0 (UTL-03)
- ✓ Streaming chunked decoder — v0.1.0 (UTL-04)
- ✓ V2 outbound sync (all reps, chunk-encrypted) — v0.1.0 (UTL-05)
- ✓ V2 inbound sync (V1/V2 dispatch, priority select) — v0.1.0 (UTL-06)
- ✓ Transport limits for large payloads (300MB/120s) — v0.1.0 (UTL-07)
- ✓ Two-segment wire framing (33% less overhead) — v0.1.0 (Phase 03)
- ✓ True inbound streaming (~1x chunk memory) — v0.1.0 (Phase 03)

### Active

- [ ] Quick-paste floating window triggered by global hotkey
- [ ] Auto-paste selected item to previously active application
- [ ] Image clipboard capture and display
- [ ] Image synchronization across devices
- [ ] Transfer reliability (retry on failure, resume on disconnect)
- [ ] Clipboard history search
- [ ] Clipboard entry favorites/pinning
- [ ] Global hotkey system for quick-paste and other shortcuts
- [ ] History record enhancements (categories, filtering)

### Out of Scope

- WebDAV cross-internet sync — deferred to future milestone
- File synchronization — deferred, focus on text + image first
- Architecture migration completion — not the focus, only migrate as needed
- Mobile app — desktop-first
- OAuth/third-party login — no account system needed
- Cloud sync service — self-hosted approach via WebDAV (future)

## Context

Shipped v0.1.0 with 330K LOC Rust (total codebase).
Tech stack: Tauri 2, React 18, Rust, libp2p, XChaCha20-Poly1305, blake3.
Backend mid-migration from Clean Architecture to Hexagonal Architecture (~60% complete).
V2 chunked transfer protocol operational for multi-representation payloads.
True inbound streaming eliminates read_to_end bottleneck.
V1 backward compatibility maintained for old devices.

## Key Decisions

| Decision                                            | Rationale                                                               | Outcome   |
| --------------------------------------------------- | ----------------------------------------------------------------------- | --------- |
| Separate floating window (not popup in main window) | Minimizes disruption to user workflow, can appear at cursor position    | — Pending |
| Image sync before file sync                         | Images are the most common non-text clipboard content, lower complexity | — Pending |
| Chunked transfer as infrastructure layer            | Supports both image sync reliability and future file sync needs         | ✓ Good    |
| WebDAV deferred to next milestone                   | Focus on core experience first, WebDAV adds significant complexity      | ✓ Good    |
| Global hotkey triggers quick-paste                  | Fastest workflow — no mouse needed, instant access                      | — Pending |
| serde_with Base64 for JSON encoding                 | serde_bytes only optimizes binary formats, not JSON                     | ✓ Good    |
| Option B for outbound streaming (Vec<u8>)           | Defer transport streaming to avoid ClipboardTransportPort changes       | ✓ Good    |
| V2 dedup by message.id only                         | OS clipboard holds only highest-priority rep, snapshot_hash fragile     | ✓ Good    |
| Two-segment wire framing (4-byte LE prefix)         | Eliminates ~33% base64 overhead, enables true streaming                 | ✓ Good    |
| EncryptionSessionPort as constructor param          | Clean DI, production wiring creates session before adapter              | ✓ Good    |
| 64KB JSON header cap                                | Prevents oversized header allocation attacks                            | ✓ Good    |

## Constraints

- **Tech stack**: Tauri 2 + React + Rust — established, not changing
- **Sync protocol**: libp2p on LAN — WebDAV deferred
- **Encryption**: XChaCha20-Poly1305 per-chunk AEAD — unified for all content types
- **Platform support**: macOS primary, Windows and Linux secondary
- **Data format**: Multi-representation payloads (text, image, rich text)
- **Performance**: Quick-paste window must appear in <200ms after hotkey press

---

_Last updated: 2026-03-03 after v0.1.0 milestone_
