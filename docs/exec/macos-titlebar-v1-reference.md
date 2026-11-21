# macOS unified window top: v1 reference and correction

Status: source-backed correction implemented; focused software and native observations recorded below.

## Exact source and active route

The fetched official `origin/v1` source is
`c6483d73c7a0234be68571fdb5fa3d38a17c4871` (v1.6.2).
`apps/gui-go/frontend/vite.config.ts` resolves all shipped macOS history/settings
routes to the kit pages. Do not use the legacy unselected page, WindowShell's
stale architecture comment, or the generic startup TitleBar as the main-window
reference. App passes `titleBar={null}` to WindowShell. LibraryMainLayout has no
independent title-bar row. SettingsKitPage exports `settingsOwnsWindowTop=true`.

The active macOS `ui-kit/skins/macos/pane.tsx` places real traffic lights over
the sidebar, beside the content toolbar. Its macos.css reserves 52px inside the
sidebar and panes; toolbar controls are top=10px/high=36px. The sidebar toggle
is top=13px and moves from sidebarWidth-40 to x=84 when hidden. The hidden
content toolbar starts at x=124, clearing both lights and toggle. Interactive
controls opt out of native dragging. There is no centered application-name row.

The actual host uses Wails MacTitleBarHiddenInset. HostService's
SetTrafficLightPosition is a no-op in this source. Frontend offset constants are
intent, not proof that native placement moves. mygo must use its real native
TitleBar safe area; this port does not reintroduce Wails/WebView or the old skin.

## Failure inventory before implementation

- An extra 32/52px application-name row remains above the toolbar.
- Sidebar items or its toggle overlap native traffic lights.
- Collapsing or overlaying the sidebar moves controls under native buttons.
- History is corrected but settings or startup loses safe window ownership.
- Native drag regions swallow toolbar controls.
- A different OS or BoardUI colors/type/motion changes during this macOS fix.

Extend canonical `window-chrome` E2E with actual history/settings views using
mygo Tester's native TitleBar safe-area input before implementation. Require one
52-DIP top band, sidebar clearance, collapse/narrow/drawer/Escape interactions
in both schemes. Retain the first red artifact and one diagnosed green replay.
Inspect the actual owned macOS preview; software geometry does not establish
physical edge resizing or Linux/Windows native behavior.

The first policy red rejected the old (14,9) traffic-light inset. The first
integration replay passed History but exposed Settings' Escape handler navigating
away before the open narrow drawer could close, in both themes. Both artifacts
are retained (`red`, `green`). The page-level shortcut now yields to an open
compact drawer. This preserves existing navigation after dismissal and does not
replace mygo's focus or keyboard routing. Run one corrected replay after this
diagnosed behavior repair; do not rerun unchanged failed source.

## Implemented correction and evidence

Normal macOS History/Settings now own the window top. The host bypasses its
fallback title row only for a ready, ordinary runtime. Setup/startup and Linux
keep the previous wrapper. SplitView consumes mygo Context.TitleBar rather than
an independent platform palette or runtime flag. Its sidebar has a 52-DIP
control/drag band; one persistent toggle sits outside inert sidebar content.
The content toolbar starts at y=0, leads with title/count and trails with
view/search/actions. During collapse, it continuously clears native controls.
Colors, type resources and control styles remain BoardUI.

- PASS: canonical `window-chrome` final source-bound replay, including
  History/Settings in both themes, collapse, narrow drawer and Escape.
- PASS: `production-boundary` build/dependency scenario and macOS production build.
- PASS: actual owned native History capture, native close/minimize/full-screen
  AX controls and absence of a separate application-name row. Native collapsed
  toggle begins at x=332.02 while the rightmost system control ends at x=325
  in the captured global geometry. These are observations, not user approval.
- Native Settings automation first reused a stale snapshot and did not deliver
  navigation. Later exact-label lookup found the sidebar was still hidden; the
  expected Settings state was absent. Those failed attempts are retained and
  are not reported as passing native Settings evidence. Software Settings
  assertions passed. Recover only with a fresh target snapshot.
- Lint first requested the equivalent De Morgan form for the drawer guard.
  The final equivalent expression passed pinned lint with zero issues. This
  style-only expression change follows the captured binary; no visual or
  keyboard behavior changed.

Owned artifacts live under `.build/boardui-v1-titlebar`, with actual native
images and the canonical manifests/checksums. The source-only v1 inspection
did not launch a v1 product or read a personal account/profile. Windows/Linux
native behavior, physical drag/edge-resize and user visual approval remain
NOT_RUN.
