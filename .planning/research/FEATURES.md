# Feature Research

**Domain:** Cross-platform clipboard synchronization tool (desktop, LAN-first)
**Researched:** 2026-03-02
**Confidence:** MEDIUM-HIGH (competitor features from multiple sources; Tauri-specific performance from official docs + community)

---

## Feature Landscape

### Table Stakes (Users Expect These)

Features users assume exist. Missing these = product feels incomplete.

| Feature                                        | Why Expected                                                                            | Complexity | Notes                                                                                  |
| ---------------------------------------------- | --------------------------------------------------------------------------------------- | ---------- | -------------------------------------------------------------------------------------- |
| Clipboard history list (text)                  | Every clipboard manager has this; it IS the product                                     | LOW        | Already shipped                                                                        |
| Global hotkey to open quick-paste window       | Ditto, Maccy, CopyQ, Paste — all use hotkey as primary trigger; no-hotkey = no workflow | LOW        | `tauri-plugin-global-shortcut` handles this; default `Ctrl+`` or `Cmd+Shift+V`         |
| Quick-paste window appears fast (<200ms)       | Users are mid-workflow; a slow popup breaks the context switch                          | MEDIUM     | NSPanel on macOS for fullscreen overlay; pre-warm window vs. create-on-demand tradeoff |
| Auto-paste to previously active application    | The whole point of quick-paste — select item, it lands in the app you came from         | MEDIUM     | Must save frontmost app before showing window; restore focus then send Cmd/Ctrl+V      |
| Fuzzy search in quick-paste window             | Maccy's signature feature; users type immediately without clicking                      | LOW        | Inline filter on keydown; no separate search step                                      |
| Image clipboard capture (local, single device) | Images are the second most common clipboard content type after text                     | MEDIUM     | Platform-specific image format handling; PNG/JPEG/BMP normalization needed             |
| Image display in history list                  | Thumbnail preview; text-only history feels incomplete once images are captured          | LOW        | Bounded thumbnail size; lazy rendering                                                 |
| Pin/favorite clipboard entries                 | Ditto "sticky clips", Maccy pinning, 1Clipboard favorites — ubiquitous                  | LOW        | Pinned items sort above recency; never auto-expire                                     |
| Paste without formatting                       | Ditto `Ctrl+Shift+V`, Maccy `Opt+Shift+Enter` — power users expect it                   | LOW        | Strip HTML/RTF; deliver plain text to target app                                       |
| Keyboard-only navigation in quick-paste window | Mouse-free is the whole point of quick-paste                                            | LOW        | Arrow keys to select, Enter to paste, number shortcuts for top N items                 |
| History entry count limit (configurable)       | Unbounded history = memory leak; users want control                                     | LOW        | Default 200-500 entries; configurable max                                              |
| App exclusion list (blacklist)                 | Password managers, banking apps copy sensitive data; must not be stored                 | LOW        | Maccy and Ditto both require this; without it, enterprise users won't adopt            |

### Differentiators (Competitive Advantage)

Features that set the product apart. Not required, but valuable.

| Feature                                          | Value Proposition                                                                                                     | Complexity | Notes                                                                                                                         |
| ------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------- | ---------- | ----------------------------------------------------------------------------------------------------------------------------- |
| Image synchronization across LAN devices         | Most clipboard managers are single-device only or require cloud; LAN image sync with encryption is rare in open tools | HIGH       | Requires chunked transfer (images often 1-10MB), XChaCha20 encryption of binary payload, and reliable delivery over WebSocket |
| Chunked transfer with retry/resume               | Enables reliable large-payload sync without atomicity failures; foundation for future file sync                       | HIGH       | Split payload into chunks (e.g., 64KB), ACK per chunk, resume on reconnect; already identified as infra need in PROJECT.md    |
| Quick-paste window at cursor position            | Reduces mouse travel when triggered in text context; Maccy supports this, but it is opt-in and unreliable across apps | LOW        | On macOS: get caret position via Accessibility API; fallback to mouse position; fallback to center                            |
| Clipboard history search with type filter        | Filter by text vs image vs all; Ditto and CopyQ offer this; most simple managers don't                                | LOW        | Tag entries by content type at capture time; filter in UI is then trivial                                                     |
| Categories / manual organization                 | CopyQ tabs, Paste pinboards — users organizing research, code snippets, templates                                     | MEDIUM     | Named user-defined collections; drag-and-drop or context-menu "move to collection"                                            |
| Encrypted sync (XChaCha20) for all content types | Most LAN sync tools are unencrypted or use TLS only; end-to-end encryption at content level is a trust differentiator | MEDIUM     | Already done for text; extend same path to image binary payload                                                               |
| Cross-device image sync with progress indicator  | Large images take time; showing transfer progress prevents users from thinking sync is broken                         | LOW        | Frontend progress event from chunked transfer layer                                                                           |
| Window position memory (reopen at last position) | Maccy added this in recent update and users appreciated it; feels polished                                            | LOW        | Persist window frame to settings                                                                                              |

### Anti-Features (Commonly Requested, Often Problematic)

Features that seem good but create problems.

| Anti-Feature                          | Why Requested                                                     | Why Problematic                                                                                                                                               | Alternative                                                                                 |
| ------------------------------------- | ----------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| Cloud sync (non-LAN)                  | "I want it everywhere"                                            | Requires external server, authentication, privacy policy, 1MB image cap seen in cloud clipboard tools, complex key management, goes against self-hosted ethos | Stay LAN-first this milestone; WebDAV already scoped for next milestone                     |
| Text expansion / snippet macros       | CopyQ and Clipboard Master bundle this; users ask for it          | Separate product category; doubles implementation scope; conflicts with "clipboard manager" mental model                                                      | Explicitly defer; recommend TextExpander/Espanso as companion tools                         |
| Plugin/scripting engine               | CopyQ has scripting; power users love it                          | Massive surface area; security risk (arbitrary code execution); maintenance burden; CopyQ is notable for its steep learning curve as a result                 | Expose well-designed Tauri commands instead; let users script around the app, not inside it |
| File sync (arbitrary files)           | Natural extension of image sync                                   | File metadata, permissions, large payloads, partial transfers, conflict resolution — this is a separate milestone-level problem                               | Explicitly scoped to next milestone in PROJECT.md; do not creep in                          |
| Cloud-based OCR search                | Paste app's OCR lets you search text inside images; feels magical | Requires external API (privacy concern) or large local model; image payloads already complex                                                                  | Defer to v2+; local OCR (Apple Vision / Tesseract) is feasible later but not now            |
| AI clipboard suggestions              | Emerging feature in 2025 tools; "smart paste"                     | Requires LLM integration; latency; model size; privacy exposure; distraction from sync reliability                                                            | Not this milestone; core reliability matters more than AI features                          |
| Mobile app                            | Users want clipboard on phones                                    | iOS clipboard API restrictions make background monitoring impossible; Android requires accessibility service; entirely separate platform engineering          | Explicitly out of scope; this is desktop-first                                              |
| Built-in password manager integration | Some tools auto-exclude password manager apps                     | Detecting and excluding specific app patterns is acceptable; building password management features is not                                                     | Implement app exclusion list (table stakes) rather than deep integration                    |

---

## Feature Dependencies

```
[Global Hotkey System]
    └──requires──> [Tauri global-shortcut plugin]
                       └──enables──> [Quick-paste floating window]
                                         └──requires──> [Window focus save/restore]
                                         └──requires──> [Keyboard navigation]
                                         └──requires──> [Fuzzy search filter]
                                         └──enhances──> [Pin/favorites]

[Image Clipboard Capture]
    └──requires──> [Platform-specific clipboard image reading]
    └──enables──> [Image display in history]
    └──enables──> [Image sync across devices]
                      └──requires──> [Chunked transfer infrastructure]
                                         └──requires──> [Reliable WebSocket framing]
                                         └──enables──> [Transfer progress indicator]
                                         └──enables──> [Future: File sync]

[Clipboard entry favorites/pinning]
    └──enhances──> [Quick-paste window] (pinned items at top)
    └──enhances──> [History search] (filter to favorites)

[App exclusion list]
    └──independent──> [Clipboard capture pipeline]

[History search]
    └──enhances──> [Quick-paste window] (same search box)
    └──enhances──> [History list view] (main window search)

[Chunked transfer]
    └──blocks──> [Image sync] (images too large for atomic WebSocket messages)
    └──blocks──> [Future file sync]
```

### Dependency Notes

- **Quick-paste window requires window focus save/restore:** The window must record the frontmost application before stealing focus, then restore focus + send paste key after user selects an item. Without this the "paste to active app" feature silently pastes to the wrong window.
- **Image sync requires chunked transfer:** A 4MP PNG is ~6MB. Single-frame WebSocket messages of that size are unreliable on LAN (buffer limits, fragmentation). Chunked transfer with per-chunk ACK is the reliable path and is already identified in PROJECT.md as infrastructure.
- **Chunked transfer blocks file sync:** The chunked layer built for images is exactly the foundation for file sync in the next milestone — build it right the first time.
- **Fuzzy search enhances both windows:** The same search implementation serves the quick-paste floating window and the main history list. Build once, use twice.
- **Pinning enhances quick-paste:** Pinned items should float to the top of the quick-paste list. The pinning data model is simple (boolean + sort key) but must be in place before the quick-paste window is finalized.

---

## MVP Definition

### Launch With (v1 — This Milestone)

Minimum viable for this milestone to qualify as a "daily-driver productivity tool upgrade."

- [ ] **Global hotkey triggers quick-paste floating window** — without this nothing else matters; it is the entry point to the entire feature set
- [ ] **Quick-paste window appears in <200ms** — the performance constraint from PROJECT.md; users notice latency above this threshold
- [ ] **Auto-paste to previously active application on selection** — the core workflow payoff; selecting an item must land it in the right app without extra keystrokes
- [ ] **Fuzzy search in quick-paste window** — type immediately, results narrow; no mouse required
- [ ] **Image clipboard capture and display** — images as first-class content type in history
- [ ] **Image synchronization over LAN** — cross-device image sync is the primary new sync feature
- [ ] **Chunked transfer infrastructure** — prerequisite for image sync reliability; also enables future file sync
- [ ] **Pin/favorite entries** — retention mechanism; prevents useful items from rolling off history
- [ ] **App exclusion list** — security hygiene; password managers must not be logged
- [ ] **Paste without formatting** — power users block on this; one of the most-requested clipboard manager features

### Add After Validation (v1.x)

Add once core is stable and used daily.

- [ ] **Clipboard history search (main window)** — reuse the fuzzy search built for quick-paste window; trigger: main window usage data shows users scrolling excessively
- [ ] **Type filter in search (text vs image)** — low effort on top of existing search; trigger: history grows to include both text and images
- [ ] **Transfer progress indicator** — low effort on top of chunked transfer events; trigger: users report confusion about whether image sync happened
- [ ] **Window position memory** — polish feature; trigger: user feedback about window appearing in wrong place

### Future Consideration (v2+)

Defer until product-market fit is established.

- [ ] **Manual categories / collections** — significant UX design work; defer until users explicitly request organization beyond pinning
- [ ] **WebDAV cross-internet sync** — already scoped to next milestone in PROJECT.md
- [ ] **Local OCR search in images** — compelling but non-trivial; needs Apple Vision or Tesseract integration
- [ ] **File sync** — chunked transfer foundation built this milestone enables this; defer implementation to next milestone

---

## Feature Prioritization Matrix

| Feature                            | User Value | Implementation Cost | Priority                     |
| ---------------------------------- | ---------- | ------------------- | ---------------------------- |
| Global hotkey + quick-paste window | HIGH       | MEDIUM              | P1                           |
| <200ms window appearance           | HIGH       | MEDIUM              | P1                           |
| Auto-paste to previous active app  | HIGH       | MEDIUM              | P1                           |
| Fuzzy search in quick-paste window | HIGH       | LOW                 | P1                           |
| Image capture + display            | HIGH       | MEDIUM              | P1                           |
| Image LAN sync                     | HIGH       | HIGH                | P1                           |
| Chunked transfer infrastructure    | HIGH       | HIGH                | P1 (required for image sync) |
| Pin/favorite entries               | MEDIUM     | LOW                 | P1                           |
| App exclusion list                 | MEDIUM     | LOW                 | P1                           |
| Paste without formatting           | MEDIUM     | LOW                 | P1                           |
| History search (main window)       | MEDIUM     | LOW                 | P2                           |
| Type filter in search              | LOW        | LOW                 | P2                           |
| Transfer progress indicator        | MEDIUM     | LOW                 | P2                           |
| Window position memory             | LOW        | LOW                 | P2                           |
| Manual categories / collections    | MEDIUM     | MEDIUM              | P3                           |
| Local OCR search                   | MEDIUM     | HIGH                | P3                           |

**Priority key:**

- P1: Must have for this milestone
- P2: Should have, add when P1 is stable
- P3: Future milestone consideration

---

## Competitor Feature Analysis

| Feature                       | Maccy                             | Ditto                             | CopyQ                  | Paste                         | Our Approach                                       |
| ----------------------------- | --------------------------------- | --------------------------------- | ---------------------- | ----------------------------- | -------------------------------------------------- |
| Global hotkey trigger         | Yes (`Cmd+Shift+V` default)       | Yes (`Ctrl+\`` default)           | Yes (configurable)     | Yes                           | Yes — configurable; default platform-appropriate   |
| Quick-paste window type       | Menu-bar dropdown                 | Popup window                      | Main window + tray     | Visual timeline panel         | Separate Tauri window; not menu-bar                |
| Appear at cursor position     | Optional (`popupPosition=cursor`) | Optional (caret or last position) | Configurable           | Fixed bottom of screen        | Default: near cursor; fallback to center           |
| Auto-paste to active app      | Yes (Option+Enter)                | Yes (double-click or Enter)       | Yes (Enter)            | Yes                           | Yes — Enter or click; hotkey dismiss returns focus |
| Fuzzy search                  | Yes — type immediately            | Yes — type immediately            | Yes — with scripting   | Yes                           | Yes — type immediately; no separate field click    |
| Image support (single device) | Yes                               | Yes (thumbnails)                  | Yes                    | Yes                           | Yes — PNG/JPEG/BMP; thumbnail in history           |
| Image sync (cross-device)     | No                                | Partial (network sync, LAN)       | Via shared folder only | iCloud only (Apple ecosystem) | Yes — LAN WebSocket with chunked transfer          |
| Pin/favorite entries          | Yes (Option+P)                    | Yes (sticky clips)                | Yes (tabs)             | Yes (pinboards)               | Yes — simple boolean flag + sort key               |
| Paste without formatting      | Yes (Opt+Shift+Enter)             | Yes (Ctrl+Shift+V)                | Yes                    | Yes                           | Yes                                                |
| Search filter by type         | No                                | Yes (text/image/file)             | Yes (tabs)             | No                            | v1.x — low effort on top of search                 |
| App exclusion list            | Yes                               | Yes                               | Yes                    | Yes                           | Yes — P1 requirement                               |
| Encryption of synced content  | Not applicable (local only)       | Optional (local network)          | Not built-in           | iCloud encryption             | XChaCha20-Poly1305 — applies to all content types  |
| Categories / manual org       | No                                | Groups                            | Tabs                   | Pinboards                     | v2+ — pinning covers the core need for now         |
| Cross-platform (Win/Linux)    | macOS only                        | Windows only                      | Yes                    | macOS/iOS only                | Yes — Tauri targets all three; macOS primary       |

---

## Sources

- Maccy features: [https://github.com/p0deje/Maccy](https://github.com/p0deje/Maccy) (HIGH confidence — official GitHub)
- Maccy popup position behavior: [https://github.com/p0deje/Maccy/issues/1061](https://github.com/p0deje/Maccy/issues/1061) (MEDIUM confidence — GitHub issue thread)
- Ditto features: [https://sabrogden.github.io/Ditto/](https://sabrogden.github.io/Ditto/) (HIGH confidence — official site)
- Ditto popup position: [https://sourceforge.net/p/ditto-cp/discussion/287511/thread/8de3734e/](https://sourceforge.net/p/ditto-cp/discussion/287511/thread/8de3734e/) (MEDIUM confidence — community forum)
- CopyQ features: [https://hluk.github.io/CopyQ/](https://hluk.github.io/CopyQ/) (HIGH confidence — official docs)
- Paste app review: [https://josephnilo.com/blog/paste-setapp-review/](https://josephnilo.com/blog/paste-setapp-review/) (MEDIUM confidence — independent review)
- CrossPaste features: [https://crosspaste.com/en/](https://crosspaste.com/en/) and XDA Developers coverage (MEDIUM confidence)
- Tauri global shortcut plugin: [https://v2.tauri.app/plugin/global-shortcut/](https://v2.tauri.app/plugin/global-shortcut/) (HIGH confidence — official Tauri v2 docs)
- NSPanel floating window approach: [https://github.com/dannysmith/tauri-template](https://github.com/dannysmith/tauri-template) (MEDIUM confidence — community template demonstrating pattern)
- Image size limitations in cross-device sync: WebSearch findings across multiple sources indicating ~1MB as common cap for cloud-based tools (MEDIUM confidence)
- Quick-paste UX patterns (hotkey trigger, cursor position, auto-paste): Multiple sources including Zapier, TechSpot, NearHub comparison articles (MEDIUM confidence — corroborated across sources)

---

_Feature research for: Cross-platform clipboard synchronization tool (uniclipboard-desktop)_
_Researched: 2026-03-02_
