# Independent desktop processes

Status: updated implementation design; process and native acceptance are separate gates.
Baseline implementation: `a99ccc0bcf2309eb8cf0bb3468802c4f740fd786`.

## Ownership

Use the existing `unic-desktop` executable with explicit `--role tray`, `--role gui` and `--role quick-panel` entry points. Each role is a real OS process. Keep `unicd` UI-free as the fourth process. The no-role application entry resolves user activation versus login/background startup before requesting a session.

| Process | Lifetime | Exclusive responsibility |
|---|---|---|
| Tray | Normal desktop session; temporary while a lightweight GUI is open | Tray, global shortcut, foreground capture, clipboard adapter, notifications, login intent, owned-service handle |
| GUI | Open until window close | Setup, migration and main history/settings rendering |
| Quick panel | Created on request; hidden between requests until tray shutdown | Panel rendering, target validation, focus restoration and synthetic paste |
| unicd | Owned parent pipe or standalone lifetime | Engine, history, settings, network and adapter protocol |

The tray uses the existing desktop native-integration pattern. No second core or host history store is introduced.

## User activation and background startup

A manual application launch always opens or activates GUI, regardless of `desktop.mode` or `desktop.start_in_background`. Desktop shortcuts, Start menu entries and application icons retain their ordinary no-argument application entry. A running service never consumes user activation.

Explicit `--background` and the operating system login-launch receipt identify background startup. Normal background startup keeps tray and unicd. Lightweight background startup leaves only unicd. Fresh or pending setup opens GUI in either mode; background startup cannot bypass initialization or migration admission.

Normal GUI window close terminates the GUI process and leaves tray, shortcut and adapter available. Lightweight GUI window close terminates GUI, tray and quick panel and releases shortcut/adapter; only unicd remains. The user reopens GUI through the application entry. No shortcut or tray is advertised while only unicd is running. The saved mode applies to GUI close, including a mode changed in the current GUI.

## Service handoff and exit

A tray adopts an existing standalone service without ownership. It never stops that generation. A service newly started for an interactive desktop remains owned until explicit Quit, a normal owner failure, or an acknowledged lightweight handoff.

Lightweight close transfers an owned service to standalone lifetime through its existing exclusive owner pipe. One fixed release message followed by owner-pipe EOF changes the service ownership mode. The tray confirms the same generation, PID and profile through the existing local service endpoint before exiting. No service restart or new history store is needed. EOF without a valid handoff retains the existing owned-service shutdown behavior. An uncertain handoff is reconciled against that generation; it must not trigger a stop or replacement of an independently running service.

GUI close notification occurs after its view and role endpoint have closed. Tray serializes GUI launch and close admission. A close notification cannot shut down a newly opened GUI. Explicit application Quit remains a separate action: close all UI roles, shortcut and adapter, then stop only a still-owned service. After lightweight handoff the service is independently owned. Running unicd alone never creates graphical integration.

## IPC and failures

Keep one live role lock and private endpoint per canonical profile. Verify OS peer executable identity and the profile, rather than trusting a PID file. GUI and panel delegate clipboard writes to the tray's single tracked adapter. Foreground targets stay ephemeral and profile-private.

A GUI crash does not masquerade as an intentional lightweight close. Tray and its shortcut remain recoverable until user activation or explicit Quit. A panel crash leaves tray integration available. A tray crash releases shortcut/adapter and closes an owned service through its parent pipe; an already detached or adopted service survives. A service restart renews the adapter baseline without applying disconnected backlog.

Migration pauses the tray integration, closes panel and unregisters the shortcut before offline import. An independent service blocks import without being stopped. Explicit completion requires service readiness and a completed setup marker.

## Acceptance

Use canonical Desktop E2E and real subprocess identities. The first updated-semantics case must fail before implementation: saved lightweight plus background preference, existing service, manual application launch, GUI close, all UI processes gone, same service generation alive, manual reopen with a new GUI identity. Add owned-service handoff and background startup as subsequent slices.

Native Windows tray clicks, actual window visibility/activation, global shortcut input and process fixtures are distinct evidence. Record source/binary hashes and cleanup. Do not claim clipboard or paste acceptance from directory-board runs.
