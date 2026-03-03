# UniClipboard Desktop

## What This Is

A cross-platform clipboard synchronization tool built with Tauri 2, React, and Rust. It enables real-time clipboard sharing between devices on LAN via WebSocket, with XChaCha20-Poly1305 encryption for security. This milestone focuses on evolving from a basic clipboard sync tool into a daily-driver productivity tool with quick-paste, image support, and robust data transfer.

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

### Active

- [ ] Quick-paste floating window triggered by global hotkey
- [ ] Auto-paste selected item to previously active application
- [ ] Image clipboard capture and display
- [ ] Image synchronization across devices
- [ ] Chunked data transfer for large payloads
- [ ] Transfer reliability (retry on failure, resume on disconnect)
- [ ] Clipboard history search
- [ ] Clipboard entry favorites/pinning
- [ ] Global hotkey system for quick-paste and other shortcuts
- [ ] History record enhancements (categories, filtering)

### Out of Scope

- WebDAV cross-internet sync — deferred to next milestone
- File synchronization — deferred to next milestone, focus on text + image first
- Architecture migration completion — not the focus, only migrate as needed for new features
- Mobile app — desktop-first
- OAuth/third-party login — no account system needed
- Cloud sync service — self-hosted approach via WebDAV (future)

## Context

- Brownfield project with existing Tauri 2 + React + Rust codebase
- Backend mid-migration from Clean Architecture to Hexagonal Architecture (~60% complete)
- Current sync only supports text via WebSocket on LAN
- No chunked transfer implementation exists — all data sent as single payloads
- Frontend uses React 18 + Redux Toolkit + Shadcn/ui + Tailwind CSS
- Existing main window shows clipboard history list — this remains unchanged
- The new floating window is a separate Tauri window, not a replacement for the main window
- Target: daily-driver quality for personal use

## Constraints

- **Tech stack**: Tauri 2 + React + Rust — established, not changing
- **Sync protocol**: WebSocket on LAN for this milestone — WebDAV deferred
- **Encryption**: XChaCha20-Poly1305 — must encrypt image data the same way as text
- **Platform support**: macOS primary, Windows and Linux secondary
- **Data format**: Must handle both text and image clipboard representations
- **Performance**: Quick-paste window must appear in <200ms after hotkey press

## Key Decisions

| Decision                                            | Rationale                                                                                    | Outcome   |
| --------------------------------------------------- | -------------------------------------------------------------------------------------------- | --------- |
| Separate floating window (not popup in main window) | Minimizes disruption to user workflow, can appear at cursor position                         | — Pending |
| Image sync before file sync                         | Images are the most common non-text clipboard content, lower complexity than arbitrary files | — Pending |
| Chunked transfer as infrastructure layer            | Supports both image sync reliability and future file sync needs                              | — Pending |
| WebDAV deferred to next milestone                   | Focus on core experience first, WebDAV adds significant complexity                           | — Pending |
| Global hotkey triggers quick-paste                  | Fastest workflow — no mouse needed, instant access                                           | — Pending |

---

_Last updated: 2026-03-03 after v0.1.0 milestone_
