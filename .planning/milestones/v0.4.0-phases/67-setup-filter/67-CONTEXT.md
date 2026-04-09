# Phase 67: Setup Filter - Context

**Gathered:** 2026-03-27
**Status:** Ready for planning

<domain>
## Phase Boundary

Prevent devices that have not completed setup from being discovered by other devices on the network. The daemon should not start the libp2p/mDNS network until encryption session is successfully unlocked, ensuring only fully-setup devices advertise themselves.

</domain>

<decisions>
## Implementation Decisions

### Network Startup Timing

- **D-01:** Daemon startup must check encryption session state BEFORE starting PeerDiscoveryWorker. If encryption session is not unlocked (i.e., `AutoUnlockEncryptionSession` fails or state is `Uninitialized`), PeerDiscoveryWorker must NOT be started.
- **D-02:** For devices that have completed setup, daemon starts PeerDiscoveryWorker normally after successful `AutoUnlockEncryptionSession`.

### Role-Based Behavior

- **D-03:** Sponsor (device that creates encrypted space) — libp2p network starts only AFTER setup completes and encryption session is available.
- **D-04:** Joiner (device joining an existing space) — libp2p network is temporarily started during setup flow via existing `SetupAction::EnsureDiscovery` mechanism. Joiner being briefly visible to other devices during setup is acceptable.

### Setup Completion Criteria

- **D-05:** "Setup complete" is determined by encryption session being unlocked (`AutoUnlockEncryptionSession` succeeds). This reuses the existing Phase 50 mechanism already in `DaemonApp::run()`.
- **D-06:** `EncryptionState::Uninitialized` (first run, no space created yet) → do NOT start network.
- **D-07:** `EncryptionState::Initialized` + successful unlock → start network.
- **D-08:** `EncryptionState::Initialized` + unlock failure → daemon already refuses to start (Phase 50 behavior, no change needed).

### Delayed Startup After Setup Completion

- **D-09:** When setup completes during a running daemon session (new space created or successfully joined), an internal event must trigger PeerDiscoveryWorker to start. Daemon should NOT need a restart.
- **D-10:** The event mechanism should notify DaemonApp to start PeerDiscoveryWorker dynamically after setup flow completes.

### Filtering Level

- **D-11:** Filtering is done at the daemon level (don't start libp2p) — NOT at the mDNS layer or business layer. This is the simplest approach.
- **D-12:** No additional business-layer filtering of discovered peers is needed for this phase. The existing `EnsureDiscovery` setup action in the setup flow continues to work as-is for joiner discovery.

### Claude's Discretion

- Internal event mechanism design (channel type, event structure) for notifying DaemonApp of setup completion
- Whether PeerDiscoveryWorker gets a new `start_delayed()` method or DaemonApp manages the delayed spawn externally
- How to handle the PeerDiscoveryWorker's current unconditional `start_network()` call

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Daemon Startup and Workers

- `src-tauri/crates/uc-daemon/src/main.rs` — Daemon entry point, service registration (PeerDiscoveryWorker at line ~161)
- `src-tauri/crates/uc-daemon/src/app.rs` — DaemonApp::run() with auto-unlock and service lifecycle
- `src-tauri/crates/uc-daemon/src/workers/peer_discovery.rs` — PeerDiscoveryWorker with unconditional start_network()

### Encryption State Recovery

- `src-tauri/crates/uc-app/src/usecases/setup/auto_unlock_encryption_session.rs` — AutoUnlockEncryptionSession use case
- `src-tauri/crates/uc-core/src/security/encryption.rs` — EncryptionState enum (Uninitialized/Initialized)

### Network Layer

- `src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs` — start_network() with idempotent state machine, spawn_swarm(), mDNS config
- `src-tauri/crates/uc-core/src/ports/network_control.rs` — NetworkControlPort trait (start_network method)

### Setup Flow

- `src-tauri/crates/uc-app/src/usecases/setup/action_executor.rs` — SetupAction::EnsureDiscovery calls start_network() during setup

</canonical_refs>

<code_context>

## Existing Code Insights

### Reusable Assets

- `AutoUnlockEncryptionSession` use case already runs in `DaemonApp::run()` — its result can gate PeerDiscoveryWorker startup
- `NetworkControlPort::start_network()` is idempotent (atomic state machine with IDLE/STARTING/STARTED/FAILED) — safe to call multiple times
- `CancellationToken` pattern used by all DaemonService implementations — can be used for delayed startup signaling

### Established Patterns

- DaemonApp starts all services uniformly via `JoinSet` in `run()` — delayed service startup will need a new pattern
- `SetupAction::EnsureDiscovery` already conditionally calls `start_network()` during setup flow — this remains unchanged
- Phase 50 auto-unlock in `DaemonApp::run()` returns the encryption state — this return value can drive the startup decision

### Integration Points

- `DaemonApp::run()` — Must conditionally skip PeerDiscoveryWorker registration based on auto-unlock result
- `DaemonApp` — Needs to listen for setup completion event and dynamically start PeerDiscoveryWorker
- Setup orchestrator completion path — Needs to emit an event/signal when setup flow completes successfully

</code_context>

<specifics>
## Specific Ideas

- User explicitly wants this controlled at the daemon/libp2p level, NOT at the business filtering layer
- The primary concern is devices appearing in "discovered devices" list before they are ready to participate in pairing
- The joiner's brief visibility during setup is an acceptable trade-off for implementation simplicity

</specifics>

<deferred>
## Deferred Ideas

### Reviewed Todos (not folded)

- "修复 setup 配对确认提示缺失" (score: 0.9) — UI-level fix, separate concern from network-level filtering

### Potential Future Improvements

- mDNS-level selective broadcast control (only listen, don't advertise) — more complex but provides absolute invisibility for joiners
- Business-layer peer filtering as defense-in-depth — would add secondary filtering at get_discovered_peers level

</deferred>

---

_Phase: 67-setup-filter_
_Context gathered: 2026-03-27_
