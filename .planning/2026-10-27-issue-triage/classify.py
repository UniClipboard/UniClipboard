"""Generate a complete, reproducible issue inventory from the captured API snapshot."""
import json
from collections import Counter
from pathlib import Path

root = Path(__file__).parent
issues = json.loads((root / 'issues.json').read_text())
# Each entry records a triage judgement, not a reproduced root cause.
rows = '''229|Medium|Compare macOS capture size, image conversion time and transfer time on both peers; old TIFF explanation needs current Engine evidence.
401|Medium|Favorites and pinning are separate requirements; verify existing favorites, then reconcile pinning with #1375 before implementation.
439|Medium|Old panel PRD overlaps current native-panel follow-ups; map each acceptance item to the current Linux and native panels before changing navigation.
481|High|Clipboard format ownership belongs to the platform adapter; WinRT migration changes write semantics and requires compatibility and echo-attribution design.
488|High|Transfer terminal state must have one owner; define timeout and disconnect semantics in Engine before altering sender projections.
489|Medium|Trace current dispatch and first-progress events; verify whether an initial state is still missing before adding an event.
516|High|Offline cloud mailbox conflicts with VISION's no store-and-forward rule; requires an explicit product decision before protocol or storage work.
548|High|Content risk classification spans capture, encrypted storage, search, display and deletion; define one policy model before implementation.
578|Medium|Inventory Maccy export formats and map to encrypted import without plaintext staging; needs a representative migration fixture.
583|Medium|Original path is retired; search DTO now exposes payloadState and the panel uses unified live search. Reproduce lost-payload search before deciding what remains.
624|Medium|Confirm current Engine error projection and UI status; reproduce a held Windows clipboard and define a user-visible recoverable state.
625|High|A negative acknowledgement changes delivery protocol and retry semantics; coordinate Engine sender/receiver outcomes and compatibility.
626|Medium|Historical remote-service failures need current endpoint timing and server evidence; do not add client retries from the old incident alone.
635|Medium|Check current upload safety limit and settings ownership; validate bounds and streaming rejection before exposing a configurable limit.
684|Medium|Reproduce on the reported Windows/security-software combination; distinguish daemon launch blocking from UI initialization timeout.
688|High|Reset must atomically invalidate trust, roster and presence; verify current reset contract in Engine before changing desktop projections.
689|Medium|Choose a diagnostic-only space identifier surface and disclosure policy; avoid treating an identifier as proof of shared trust.
755|High|Tracking issue spans runner orchestration, fixtures and multiple operating systems; reconcile retired Tauri assumptions before defining a new acceptance contract.
757|Medium|Existing fixture proposal targets retired Tauri paths; reuse current daemon/CLI fixture seams and produce an encrypted disposable-profile artifact.
758|Medium|Failure attribution needs a bounded evidence schema and privacy review; start with local reports before any external-agent comment integration.
759|Medium|Validate current dual-profile runner isolation and process ownership; replace old Tauri-driver assumptions before extending topology support.
760|Medium|Select current macOS host automation entry points; old tauri-driver limitation alone does not establish a missing Go/native-panel capability.
798|High|Virtual Shell items change file identity and extraction semantics; design Windows adapter ownership and secure path handling in Engine.
805|High|Received-file retention and Move actions span storage ownership, metadata and UI; define recovery and encrypted metadata semantics first.
809|Medium|Overlaps #1917; audit existing Flatpak manifest and validate sandbox clipboard, daemon and update behavior on SteamOS.
816|High|Clipboard retry/circuit-breaker state must be owned by one Windows adapter; validate outage and recovery semantics before changing backoff.
825|High|Mailbox credentials in pairing conflict with the current product direction; do not extend pairing without the decision required by #516.
826|High|Mailbox tracking proposal conflicts with current no store-and-forward policy; requires product and security architecture decisions.
851|Medium|Mobile platform behavior belongs to UniClip; collect Android lifecycle and clipboard-permission evidence before treating multiple-copy loss as desktop failure.
875|High|Directory transfer spans protocol, traversal safety and resumability; reconcile current implementation with #1325 before proposing missing architecture.
876|Medium|Separate IPv6 discovery, address publication and QUIC reachability; capture a two-peer IPv6-only reproduction before changing transport settings.
920|Medium|Compare address and discovery state before/after automatic update and restart; needs a real LAN-only lifecycle reproduction.
949|Medium|Run the isolated address-transferability experiment against the pinned Engine/iroh version; do not assume old addr_blob details still apply.
950|High|Peer attestation is a trust-admission decision; audit current membership design and supersession before drafting another ADR.
952|High|Discovery topology and address refresh affect membership convergence; define authoritative peer evidence and expiry in Engine.
953|High|Attestation admission changes identity/trust state across peers; requires Engine contract and a two-device admission-plus-sync acceptance scenario.
954|High|Roster piggyback changes pairing wire compatibility; coordinate Engine protocol evolution and mixed-version behavior.
955|High|Offline-join attestation scheduling affects convergence and retries; keep scheduling ownership in Engine and avoid desktop duplicate state.
957|Medium|Measure X11 capture multiplicity and retention accounting separately; reproduce with decoded-pixel identity and disk-usage artifacts.
958|High|Image identity must survive representation changes; define a single decoded-pixel fingerprint contract across capture, deduplication and echo suppression.
963|Medium|Define the usable milestone and clock boundaries for the Go host; avoid instrumenting the retired webview bootstrap path.
988|Medium|Mechanical naming request now crosses the Engine ownership boundary; locate current types and imports before any coordinated rename.
1008|Medium|No dependency/version evidence in the report; reproduce with the current locked Rust toolchain before pinning or upgrading packages.
1024|Medium|Current CLI has member commands; compare requested management operations with current command tree and verify missing operations end to end.
1047|High|Proxy routing spans transport types and environment resolution; define what traffic a proxy can carry before changing Windows networking.
1097|High|Non-blocking X11 ownership requires event-loop lifecycle changes; reproduce blocking and preserve selection ownership in the Engine adapter.
1111|High|Typed origin identity affects multiple consumers and echo attribution; replace the shared representation at its owner rather than interpreting prefixes elsewhere.
1140|Medium|First winget submission is an external publishing operation; verify package existence and current release artifacts before a Windows manifest submission.
1172|Medium|TestFlight invitation and OS compatibility belong to the mobile release channel; verify current invitation and minimum OS without desktop code changes.
1176|High|Chunked AEAD changes encrypted format and streaming integrity; requires Engine format versioning, migration and corruption/recovery acceptance.
1198|Medium|Trace current content-category projection for image files and legacy entries; validate classification and backfill separately.
1206|High|Captured metadata adds storage/privacy and cross-platform identity rules; define encrypted metadata ownership and cache lifecycle before adding fields.
1211|Medium|Inspect existing history selection/delete actions; define partial failure and confirmation behavior before adding batch deletion.
1222|High|Encryption-session permission enforcement is a security boundary; audit actual Engine operations and client gates before documenting guarantees.
1226|Medium|Re-run the current lockfile audit and upstream advisories; do not apply version overrides from an old dependency snapshot.
1230|Medium|Measure SQLite freelist, WAL and live bytes after deletion; choose a bounded compaction policy and verify locked/unlocked behavior.
1259|Medium|Reproduce failed daemon launch on Windows and trace current Go-host error projection; missing DLL and timeout must remain distinct.
1298|High|Image upload introduces a user-selected external content destination; needs privacy, credential storage and action-boundary design before integration.
1308|Medium|Mobile camera lifecycle belongs to UniClip; verify capture-session teardown when leaving pairing and reopening the app.
1310|Medium|Reproduce current Go host on Steam Deck; separate Wayland/WebKit startup from Flatpak packaging requests #809 and #1917.
1325|High|Phase tracking combines transfer state, settings and mobile degradation; reconcile each child with the current Engine/Desktop boundary.
1331|Medium|Map current Engine rejection reasons to a single desktop status projection; verify native and WebView display consistency.
1332|High|Rejection memory and noise suppression add durable protocol state; define identity, invalidation and storage ownership in Engine.
1333|Medium|Inventory current file-sync fields and settings ownership before exposing missing controls; requires UI design and real transfer acceptance.
1334|Medium|Documentation depends on final directory limits, mobile behavior and UI wording; verify #1331/#1333 behavior before publishing claims.
1350|Medium|iOS foreground limitations may be expected OS behavior; collect lifecycle evidence and document actual background capability in UniClip.
1351|Medium|Capture Windows interface selection and bind failures; distinguish invalid adapter addresses from permissions and firewall effects.
1362|Medium|Umbrella request mixes already documented features and new behavior; reconcile with #1365-#1375 before implementation.
1365|Low|Confirmed documentation defect. Correct numeric paste actions, add Ctrl+N/P and Tab/Shift+Tab, and scope the table to Linux WebView.
1366|Medium|Check source-application metadata availability and encrypted persistence; UI icon display needs a design and missing-icon behavior.
1367|Medium|Existing time-query support may cover part of the request; define timezone, inclusive bounds and single-day selection before UI changes.
1368|High|Pasted state needs a precise event definition across native/WebView paste and manual copy; design durable encrypted state and filtering ownership.
1369|Medium|Check native and Linux shortcut conflicts; define the current filter ordering and platform modifier behavior before adding bindings.
1370|Medium|Choose an accessible type-color design that respects themes; implement only after display design approval and visual acceptance.
1371|Medium|Opening content must preserve encrypted storage rules; use existing shell adapters for files/links and keep image plaintext decisions under #1783.
1372|Medium|Filter-menu layout is a product/UI decision; inventory current history controls and produce an approved design first.
1373|High|Settings have multiple owners in the report; audit current daemon/desktop settings split before consolidating authority and removing old paths.
1374|High|Edge docking and auto-hide create a cross-platform window lifecycle state machine; define focus, display changes and restore semantics first.
1375|High|Pinned order affects history ranking, persistence and numeric shortcuts; define one stable ordering contract and distinguish favorites.
1378|Low|Issue premise is stale/incomplete: release-health feature is compiled, but auto_session_tracking defaults false and no session is started. Clarify current disabled behavior.
1386|Medium|Antivirus classification needs the exact signed artifact and vendor verdict; avoid code changes or security bypasses from the report alone.
1417|Medium|Collect both mobile and desktop logs with exact versions; distinguish pairing, reachability, decryption and clipboard application.
1420|Medium|Current repository has build-server-image.yml and GHCR publication; compare requested Docker Hub delivery with current channel policy before adding another publisher.
1422|High|Handoff image echo overlaps #958/#1585; establish one image-identity and self-write attribution contract rather than adding platform-specific deduplication.
1425|Medium|Live Activity belongs to UniClip and iOS entitlement/lifecycle constraints; define actual user value and supported background states.
1454|Medium|Google Play delivery requires mobile signing, policy and store-account work; verify current mobile publication status before changing desktop packaging.
1458|Medium|Define immutable current-version interoperability and upgrade fixtures; reuse real-process E2E runners and retain provenance and result artifacts.
1473|High|Security remediation may require protocol/dependency architecture changes; verify current advisories and dependency paths before any staged upgrades.
1543|High|Centrally enforced LAN-only mode changes local policy authority and pairing propagation; requires Engine policy and conflict semantics.
1559|Medium|Explorer shell integration needs installation/uninstallation ownership and file-send reuse; validate quoting, multi-selection and updates on Windows.
1569|Medium|Win+V is system-owned; validate supported registration/replacement behavior and user opt-in before promising an override.
1571|High|Conflicting legacy/current caches need recoverable migration and authoritative profile ownership in Engine; do not merge/delete data from the UI.
1574|High|Category umbrella overlaps existing local tags and #2057-#2060; reconcile acceptance gaps before changing encrypted metadata or filtering models.
1578|Medium|Reuse existing encrypted capture/save and copy paths; define save-then-copy failure recovery so edits and original history remain intact.
1583|Medium|Old startup report needs current Go-host reproduction and daemon-discovery evidence; stale conn/PID handling is not proven by an unused port.
1584|Medium|Presence touch may be Engine-owned behind authenticate; trace current authentication and persistence before adding duplicate desktop writes.
1585|High|Mobile echo involves origin, SSE fan-out and image transcoding; coordinate Engine and shortcut suppression with #958/#1422.
1600|Medium|Old macOS host focus report needs a current Go-host reproduction; record activation sources and window lifecycle before changing reveal logic.
1602|High|Legacy membership removal and session recovery fail together; requires Engine convergence/recovery analysis with preserved upgrade data.
1607|Medium|No quantitative memory evidence; collect per-process release-build baseline and growth before choosing a component to optimize.
1611|Medium|Define stable small CLI projection from current daemon state; avoid deriving setup/unlock from health and avoid exposing internal tokens.
1614|Medium|Split pause control, optional shortcuts and Esc setting; reconcile existing settings and produce an approved display design before edits.
1615|High|Holding Ctrl+V requires global interception without breaking ordinary paste; design platform input ownership, timing and accessibility semantics.
1616|Medium|Trace current automatic versus manual lightweight transitions; notification behavior must follow entry cause, verified with the Go host.
1629|Medium|Engine already owns full invitation parsing per the report; expose its existing contract in desktop/CLI and test both forms without duplicating secrets.
1687|Medium|Go CLI already has service start/restart/status/stop for Linux/macOS; reconcile original install/uninstall and Windows expectations with actual implementation.
1688|Medium|Current Go CLI uses a custom command tree; generate completion from that tree and verify all four shells, hidden flags and profile options.
1692|Medium|Need both peers' exact relay config categories and connection logs; distinguish rendezvous redemption from authenticated QUIC dialing.
1705|Medium|Browser Basic auth reachability does not prove mobile client TLS/route compatibility; trace Caddy request/response and mobile connection tests.
1709|Medium|Use isolated immutable earlier/newer 1.0 artifacts and backup receipts; verify restored history after downgrade without touching production profiles.
1717|Medium|Compare main/native-panel queries and authorization under the same profile; separately test search-box paste interception.
1726|High|Capture-type exclusion spans local recording and sync semantics; define whether excluded types can be sent/received and enforce policy at one Engine boundary.
1729|High|Omarchy and Engine maintain independent histories; design CLI status/control and clipboard ownership without bypassing encrypted Engine persistence.
1735|High|Polling removal requires an authorization event/invalidation contract and pre-show check; preserve focus behavior and content-lock correctness.
1739|High|External-script triggers introduce content execution and credential/privacy boundaries; design sandbox/consent, origin rules and one action scheduler.
1740|Medium|Capture PixPin's offered formats and ownership timing on Windows; distinguish invalid writes from downstream application format preference.
1746|Medium|Compare UTF-16 capture, transport bytes and iOS representations with a Cyrillic fixture; Telegram-only failure does not prove transport corruption.
1748|Medium|Inspect Windows multi-file representations and Linux received manifest; identify where the list becomes a single image before editing classification.
1750|High|LAN-only over arbitrary VPNs changes discovery/address admission policy; define trusted overlay ranges and direct dialing without relay fallback.
1755|High|Cross-platform app lock spans authorization, secure store and background lifecycle; reconcile current content-lock scope before extending policy.
1761|Medium|Audit image dependency feature consumers first; narrow only with text/image/file clipboard E2E artifacts on all three desktop platforms.
1771|High|Automation/plugin RFC creates a new extension/security architecture; unify with #1739 and define stable capabilities before implementation.
1774|High|Tag intersection is a shared search contract owned by Engine; coordinate query semantics and clients rather than filtering incomplete result pages locally.
1775|High|Hold-to-cycle requires a global input/preview/commit state machine; preserve ordinary paste, focus and modifier rollback across platforms.
1776|High|Paste sequences require session state and focus/clipboard ownership between items; design cancellation and failure semantics before adding actions.
1777|High|Queued paste spans transfer readiness, target focus and cancellation; define a single coordinator and avoid introducing automatic history delivery.
1778|Medium|Panel pinning is bounded UI state, but needs explicit focus/Esc/preview-window behavior and approved design before implementation.
1779|Medium|Caret positioning needs OS accessibility evidence and permission fallback; validate editor, terminal and browser cases on each platform.
1780|High|Windows port report predates current native Windows sources; audit remaining platform, packaging and focus acceptance gaps before further port work.
1781|Medium|Verify signed release/helper lifecycle using current CI artifacts; separate signing, notarization, accessibility identity and update replacement evidence.
1782|Medium|Navigation requests need host-to-frontend route and entry-selection semantics; reconcile completed UI details before designing missing actions.
1783|High|Plaintext image temporary files conflict with the encryption baseline; requires an explicit product/security decision before implementation.
1784|Medium|Replace synthetic-only evidence with current real-daemon search and idle baselines; retain source pin, profile isolation and performance artifacts.
1785|High|Native-panel tracking issue references historical Engine/PR pins; reconcile children and immutable merged dependencies before declaring completion.
1787|High|CLI content-lock coverage is a deliberate authorization policy decision; retain current client boundary until product/security scope is decided.
1788|High|Upgrade storage failures may involve recoverable migration state; inspect preserved logs/backups and Engine ownership before any repair.
1789|Medium|Trace intentional daemon-restart state versus bootstrap failure in Go host; verify reconnection before changing error-page navigation.
1792|Medium|Reproduce two Windows peers on current builds and capture both pairing timelines; expiry message alone does not identify admission failure.
1808|Medium|Current tray-icon settings may overlap; inventory existing variants and approve additional theme-accessible designs before adding assets.
1812|Medium|CLI documentation already treats pending resend as success; reproduce immediate post-pair send and compare actual exit policy with that contract.
1818|Medium|Define Ctrl/Shift selection, range anchors and bulk actions against current history UI; needs approved interaction design and virtualized-list acceptance.
1830|Medium|Current command tree has a detailed flag on search; inspect get metadata output and full IDs before adding a distinct get contract.
1836|Medium|Collect updater phase and live process/file ownership on macOS/Linux; do not use broad process termination as a fix.
1904|High|Proxy/environment gaps include policy decisions and backend resolution ownership; separate supported resolvers, PAC and portable-path semantics.
1905|Medium|Existing issue is an acceptance gap; run bounded current-source lifecycle scenarios and preserve real focus/paste and sleep/wake evidence.
1906|Medium|Repeat real menu tracking and device updates on an immutable source; distinguish intermittent menu disappearance from pointer acceptance gaps.
1907|High|Native SIGSEGV suggests window/main-thread lifetime ownership; establish crash reproduction and audit replay ordering before accepting startup deferral.
1909|Medium|Real Go-host frontend acceptance remains separate from mocks; verify multi-image host-file, modifier rollback and live WebSocket updates.
1917|Medium|Existing Flatpak manifest has obsolete artifacts per report; update packaging only after current binary/runtime and SteamOS sandbox acceptance.
1923|Medium|No issue body; determine HarmonyOS edition, device and installer expectations through mobile repository context before choosing an implementation.
1926|Medium|Published Snap security state requires store/revision verification and a rebuild; retain patched-package and channel-promotion evidence separately.
1931|Medium|Use one desktop HUD preference for sync/transfer feedback and preserve failure visibility; requires approved settings design and lifecycle verification.
1932|High|File transfer timing changes eager/lazy fetch and paste readiness; Engine must own policy and compatibility across all clients.
1933|High|Missing content keys after successful connections indicates a security-state path to investigate in Engine; collect both sides and immutable version pins.
1948|Medium|Notification text and trigger remain unconfirmed; distinguish minimize, close and lightweight entry before associating with #1616.
1984|Medium|Verify socket membership, network interface and packet flow on both hosts; absence of mDNS traffic alone does not prove where discovery failed.
2009|Medium|Old Tauri logs cannot establish current Go-host failure; reproduce release startup on the reported WebKit/Wayland stack and capture helper CPU traces.
2011|Medium|Scrollbar disappearance needs current Windows native/WebView surface identification and pointer reproduction before CSS or host changes.
2025|High|Oneshot daemon lifetime and CLI attachment need one ownership contract; trace leases/discovery and eliminate attachment to a retiring process.
2026|Medium|Inspect current stop protocol and PID identity verification on Windows; reproduce graceful shutdown and distinguish timeout from forced termination.
2034|High|Admission latency spans protocol stages and acknowledgement ownership; obtain two-sided timing before changing timeouts or convergence scheduling.
2041|Medium|Compare current KDE client/server decorations and host window configuration; verify a single titlebar across supported window systems.
2044|Medium|Inspect macOS activation policy for silent startup versus visible main window; reproduce Dock behavior without changing lightweight tray requirements.
2057|Medium|Optional tag icons require one shared tag DTO/projection and approved icon picker; preserve encrypted names and locked-state behavior.
2058|Medium|Existing sidebar-order endpoint is the intended authority; design reorder interaction and persist through that endpoint without a second sort state.
2059|Medium|Native panel must resolve custom tag IDs through the existing daemon API; verify label availability, lock invalidation and filtering parity.
2060|Medium|Determine in-use count and add confirmation in the shared delete flow; requires approved dialog behavior and no deletion of clipboard entries.
2101|Medium|Existing native favorite filters are documented in the issue; first reproduce discoverability or failure on Windows before adding another shortcut/UI.
2110|Low|No defect established: VISION explicitly retains tray host plus daemon in lightweight mode. Do not remove the host from process-presence evidence alone.
2112|Low|Already implemented in current source: Compose invokes uniclip health and a real-process E2E runner exists. No duplicate fix; fresh runtime/package proof remains required.
2119|Medium|Theme contrast needs current KDE tray icon evidence; inspect available tray styles and rendering before approving a light/dark asset change.
'''
judgements = {}
for line in rows.strip().splitlines():
    number, difficulty, analysis = line.split('|', 2)
    judgements[int(number)] = (difficulty, analysis)
ids = {i['issue_number'] for i in issues}
assert ids == set(judgements), (ids-set(judgements), set(judgements)-ids)
counts = Counter(level for level, _ in judgements.values())
report = '''# Open issue triage — 2026-10-27

## Scope and evidence

Repository: UniClipboard/UniClipboard. Source base: `20c5ddcaf52e3504964b348908d34d3ccf09307e`.

The connected GitHub issue search returned 87 open issues created before 2026-08-01 and 84 on or after that date. Neither partition reached its 100-result cap. The deduplicated snapshot contains **171 open issues**. Pull requests are excluded. This is a dated snapshot, not a live backlog.

- **Low**: bounded work supported by direct source evidence; can be handled independently. Includes source-already-implemented and no-defect dispositions, which are explicitly identified.
- **Medium**: bounded feature or investigation with uncertain reproduction, platform evidence, external publishing prerequisites, or UI decisions. Initial analysis only.
- **High**: shared protocol, persistent state, trust/security, ownership/lifecycle or architecture decisions. Initial analysis only. High does not mean an architecture change is already proven necessary.

Each row combines the issue report with a proposed investigation boundary. Unless a row explicitly states a current source observation, its explanation is a **triage hypothesis**, not a verified root cause. Historical reports and references to old Tauri or Engine paths need current reproduction. No issue is closed or relabeled by this work. No issue comments are posted.

''' + f'Classification: **{counts["Low"]} Low / {counts["Medium"]} Medium / {counts["High"]} High**.\n\n'
report += '''## Low-complexity dispositions

| Issue | Current-source evidence | Disposition |
| --- | --- | --- |
| #1365 | `apps/gui-go/frontend/src/quick-panel/ClipboardHistoryPanel.tsx` numeric actions call `handleSelect`; Tab cycles the order from `constants.ts`; Ctrl+N/P move selection. | Correct both language versions of the Linux WebView keyboard table. Native panel bindings remain separate. |
| #1378 | `crates/uc-bootstrap/Cargo.toml` enables `release-health`; initialization uses SDK defaults; locked `sentry-core` 0.48.5 defaults `auto_session_tracking` to false; no manual session start exists in repository Rust code. | Clarify that backend release-health tracking is currently disabled. Do not add the proposed inaccurate “one session upload per process” claim. The final envelope transport gate already covers session envelopes. |
| #2110 | `VISION.md` explicitly says lightweight mode keeps the tray host and daemon. | No code change. Process presence does not prove a bug. Any remaining issue needs actual window/resource behavior evidence. |
| #2112 | `deploy/vps/docker-compose.yml` invokes `uniclip health`; `apps/cli-go/internal/commands/health.go` is read-only; `apps/cli-go/e2e/healthcheck.py` covers profiles, stale/corrupt records, hung process and restart. | No duplicate patch. Current source is ahead of the report. Existing E2E runner was not executed during this documentation-only task; released-image status is unverified. |

## Initial analysis by issue

| Issue | Difficulty | Initial analysis / next step |
| --- | --- | --- |
'''
for issue in issues:
    n=issue['issue_number']
    level, analysis=judgements[n]
    title=issue['title'].replace('|', r'\|').replace('\n',' ')
    report += f'| [#{n}]({issue["url"]}) {title} | {level} | {analysis} |\n'
report += '''
## Recommended next investigations

1. Preserve data and security evidence first: #1571, #1602, #1788, #1933, and the current advisories behind #1473. Repair Engine-owned behavior in Engine, then adopt an immutable merged release.
2. Establish real reproductions for current desktop regressions: #1717, #1789, #1984, #2009, #2025 and #2026. Use the current host and exact platform before interpreting old logs.
3. Reconcile umbrella and old implementation requests with current code: #401/#1375, #755-#760, #1325, #1574/#2057-#2060, #1687, #1780/#1785 and #2112.
4. Request product decisions before implementing proposals that change locked policy: #516/#825/#826, #1755/#1787, #1783 and #1932.

## Verification boundary

This PR changes documentation only. Existing document mirror/anchor checks, MDX/type checks and the production site build are the relevant checks. No unit tests are added. No app/browser/device acceptance, real clipboard operations, production-profile access, release dispatch, or package promotion is performed. Build output and check results are recorded separately from source observations.

Current verification: mirror/anchor checks, existing deployment-configuration check, type checking and the Webpack production build PASS. The default Turbopack build FAILS with 102 MDX loader/meta-resolution errors, including unchanged pages. Both Bun and direct Node launch fail. The cause is unresolved; this task does not claim the failure predates this diff. See `validation.json` for commands and source hashes. No app, device or healthcheck-process E2E was run.

Reproduce the inventory from `issues.json` with `python3 .planning/2026-10-27-issue-triage/classify.py`. The generator rejects missing or additional issue IDs, so every captured issue has exactly one classification.
'''
(root/'triage.md').write_text(report)
(root/'classification-summary.json').write_text(json.dumps({'total':len(issues),'counts':dict(counts),'base':'20c5ddcaf52e3504964b348908d34d3ccf09307e'},indent=2)+'\n')
print(json.dumps({'total':len(issues),'counts':dict(counts)}))
