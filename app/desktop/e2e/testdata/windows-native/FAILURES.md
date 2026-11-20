# Windows native process acceptance failure inventory

Write this inventory before the native driver. Do not execute against a VM
until its current owner explicitly hands over the identified guest.

1. Tray candidates are absent, ambiguous, hidden, stale, or belong to another app.
2. A menu item has the expected text but its native owner PID is not the verified
   task tray process. Never click it.
3. A running independent unicd prevents native tray GUI activation.
4. WM_CLOSE hides a resident GUI or targets a reused HWND/PID. Normal close
   wrongly ends tray/service, or lightweight close leaves any UI process running.
5. A second tray activation retains the closed GUI process or creates duplicates.
6. Saving lightweight mode does not survive GUI closure and manual reopening.
   A manual no-argument lightweight launch incorrectly omits its GUI, or explicit
   background lightweight launch creates tray, panel or GUI.
7. The panel shares another role PID, has no native window, or needs a running GUI.
8. Ctrl+Alt+V is unregistered, conflicted, duplicated, or delivered to the wrong
   foreground process. Never replay input with an unknown outcome.
9. Copy testing accidentally uses a system clipboard API or shared VM clipboard.
10. Explicit native menu Quit stops the independent S0 generation or leaks roles.
11. Lightweight GUI closure restarts or kills S1 instead of releasing its same
    PID/generation as standalone. Later native Quit kills that detached service.
    Native Quit before any detach leaves a fresh tray-owned service running.
12. Evidence confuses native mouse/WM_CLOSE/SendInput with role IPC or app-scoped
    automation. Screenshot capture includes unrelated desktop content.
13. A stale PID, binary, source revision or prior artifact is reported as current.
14. Cleanup stops another task, hides a failure, or deletes a still-live profile.

The short lightweight phase first verifies manual no-argument GUI opening,
WM_CLOSE of all UI roles, repeated manual GUI recreation and explicit background
core-only startup against driver-created standalone S0. It does not inject the
global hotkey or click a tray business menu. Its full-matrix entries stay NOT_RUN.

The full phase starts in normal mode so GUI closure must leave tray and S0
running. Its first panel creation comes from actual Ctrl+Alt+V with the GUI
absent. Only after this native panel/foreground/Escape gate does the driver save
lightweight mode and verify all-UI shutdown plus manual reopening. The owned S1
profile then verifies a one-way, same-generation release on lightweight GUI
closure. Native Quit preserves detached S1. A separate fresh owned S2 profile
verifies native Quit before detach stops its service. The driver cleans each
remaining exact generation itself, including after a failed assertion.

## Preserved native RED evidence

`native18` observed panel role status `visible=true` while its Win32 panel HWND
was hidden. This is a product acceptance failure, not a missing role fixture.
Full01/Full02 then reproduced the service-only SW_HIDE process launch policy
suppressing the first native display. Commit ce4a0a8 separates UI creation flags
from daemon flags. Full04 passed native lifecycle/focus; its pre-paint crops did
not establish rendered content. Full05 rendered its first panel but failed a
later menu-foreground guard. Full06 passed all scenarios with tree/idle/compositor
readiness and menu-foreground observation. Neither role status nor the short
lightweight phase can replace those native assertions.

Native tray discovery is observational first. A candidate must be unique and
visible. A native menu must be bound to the task tray PID before any business
menu item is clicked. Inability to prove ownership is a failed prerequisite,
not permission to use role IPC as a substitute for tray activation.

15. The native panel owns a visible foreground HWND but has no rendered search
    field. A pre-paint crop must not count as a usable panel. Require the existing
    automation tree search field, drain pending frames, and preserve both the
    app-scoped frame and the native foreground crop for separate inspection.
