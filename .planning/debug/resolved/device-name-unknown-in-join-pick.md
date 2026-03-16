---
status: awaiting_human_verify
trigger: 'device-name-unknown-in-join-pick: Device name always shows unknown in JoinPickDeviceStep'
created: 2026-03-16T00:00:00Z
updated: 2026-03-16T14:00:00Z
---

## Current Focus

hypothesis: CONFIRMED - DeviceAnnounce messages were never received due to two bugs: (1) wire format mismatch (raw JSON sent but framed format expected), (2) Business protocol policy blocks unpaired peers
test: Fixed wire format to use frame_to_bytes() and bypassed policy check for DeviceAnnounce on both send/receive sides
expecting: Device names visible during JoinPickDeviceStep because announce now uses correct wire format and is allowed for unpaired peers
next_action: Await human verification

## Symptoms

expected: Device name should display the actual device name in JoinPickDeviceStep
actual: Device name always shows "未知设备" (unknown device), device ID shows correctly
errors: No error messages
reproduction: Navigate to setup page's JoinPickDeviceStep
started: Always been this way

## Eliminated

- hypothesis: upsert_discovered overwrites device_name with None on mDNS re-discovery
  evidence: Previous fix (preserving device_name in upsert_discovered) was correct but insufficient - the name was never set in the first place during the discovery phase
  timestamp: 2026-03-16

- hypothesis: DeviceAnnounce only fires during lifecycle boot, NOT when peers are discovered
  evidence: Previous fix added announce-on-discovery in wiring.rs, and Seq logs confirmed "broadcasting device announce to discovered peers" fires. But device names still not appearing because the message never ARRIVES at the remote peer.
  timestamp: 2026-03-16

## Evidence

- timestamp: 2026-03-16
  checked: JoinPickDeviceStep.tsx line 135
  found: Displays peer.deviceName with fallback to unknownDevice
  implication: Frontend correctly handles null/undefined deviceName

- timestamp: 2026-03-16
  checked: useDeviceDiscovery.ts hook
  found: Gets peers from getP2PPeers() and listens to discovery/name events
  implication: Hook correctly wires both initial load and event updates

- timestamp: 2026-03-16
  checked: PeerCaches::upsert_discovered in libp2p_network.rs
  found: Always creates DiscoveredPeer with device_name: None and replaces existing entry via HashMap::insert
  implication: Previous fix preserved names, but names were never set during discovery phase

- timestamp: 2026-03-16
  checked: AppLifecycleCoordinator::ensure_ready() in uc-app/src/usecases/app_lifecycle/mod.rs
  found: DeviceAnnounce (announcer.announce()) is called once at step 3.5, AFTER network starts
  implication: If no peers are discovered at boot time, announce skips ("discovered peer list is empty")

- timestamp: 2026-03-16
  checked: BusinessCommand::AnnounceDeviceName handler in libp2p_network.rs line 1855
  found: Checks discovered_peers cache - if empty, logs "skip device announce because discovered peer list is empty" and returns
  implication: Announce is a no-op when called before any mDNS discovery

- timestamp: 2026-03-16
  checked: Seq logs from user report
  found: Timeline shows: (1) init device name, (2) mDNS discovers peers with device_name:None, (3) JoinPickDeviceStep renders "unknown", (4) pairing completes, (5) DeviceAnnounce finally broadcasts
  implication: DeviceAnnounce happens too late - only after pairing, not during discovery

- timestamp: 2026-03-16
  checked: run_pairing_event_loop in wiring.rs
  found: PeerDiscovered event handler emits frontend event but does NOT trigger DeviceAnnounce
  implication: No mechanism to announce device name when new peers appear

- timestamp: 2026-03-16T14:00
  checked: Seq logs after previous fix (announce-on-discovery added)
  found: "broadcasting device announce to discovered peers" fires immediately after mDNS discovery, BUT no logs for RECEIVING a device announce on either side
  implication: Announce is SENT but never RECEIVED - points to wire format or policy issue

- timestamp: 2026-03-16T14:00
  checked: Wire format - sender vs receiver in libp2p_network.rs
  found: SENDER (line 1876) used message.to_bytes() producing raw JSON. RECEIVER (line 1032-1042) reads 4-byte LE length prefix first then json_len bytes. The first 4 bytes of raw JSON (e.g. `{"De`) are interpreted as a u32 length = garbage value, causing the read to fail silently.
  implication: ROOT CAUSE #1 - Wire format mismatch. DeviceAnnounce was never received because sender sent raw JSON but receiver expected framed format (4-byte LE length + JSON).

- timestamp: 2026-03-16T14:00
  checked: Policy check in BusinessCommand::AnnounceDeviceName send path (line 1904) and receive handler (line 1006)
  found: Both send and receive sides call check_business_allowed() which checks ProtocolKind::Business. ConnectionPolicy::allowed_protocols() returns business=false for Pending/Revoked peers. During JoinPickDeviceStep peers are NOT yet paired (Pending state).
  implication: ROOT CAUSE #2 - Even with correct wire format, DeviceAnnounce is blocked by pairing policy for unpaired peers. The send side silently skips (continue), the receive side silently drops the stream (return).

## Resolution

root_cause: Three-layer issue preventing DeviceAnnounce from working during JoinPickDeviceStep:
(Layer 1 - fixed previously) mDNS re-discovery overwrote cached device names with None.
(Layer 2 - fixed previously) DeviceAnnounce only fired at boot, not on peer discovery.
(Layer 3 - fixed now) DeviceAnnounce messages were never received due to TWO bugs:
(a) Wire format mismatch: sender used to_bytes() (raw JSON) but receiver expects frame_to_bytes() format (4-byte LE length prefix + JSON). The first 4 bytes of JSON get misinterpreted as a length, corrupting the read.
(b) Business protocol policy: check_business_allowed() blocks Business protocol for unpaired peers (Pending state). DeviceAnnounce was sent via Business protocol, so both outbound (send) and inbound (receive) policy checks reject it for peers that haven't paired yet.
fix: Three changes to libp2p_network.rs:
(1) Changed message.to_bytes() to message.frame_to_bytes(None) in AnnounceDeviceName handler to match the framed wire format the receiver expects.
(2) Removed check_business_allowed() from the AnnounceDeviceName send loop - DeviceAnnounce is non-sensitive and must work before pairing.
(3) Restructured inbound business stream handler to defer policy check until after reading the message type. DeviceAnnounce is allowed from any peer; all other messages still require pairing.
verification: All 155 uc-platform tests pass, all 190 uc-tauri tests pass, all 537 uc-core+uc-app tests pass
files_changed:

- src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs (wire format fix + policy bypass for DeviceAnnounce on send and receive)
- src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs (previous fix: trigger announce on peer discovery)
