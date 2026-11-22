# Sync integration contract

- Status: Selected target contract with the initial SyncClipboard text and image slice implemented. Section 7 identifies current behavior. Generic management, file streaming, and forwarding remain planned.
- Decision: [0029](../decisions/proposed/0029-extensible-sync-backend-integration.md).
- Existing interfaces: [plugin](plugin-contract.md), [settings](plugin-settings-contract.md), [backend](sync-backend-contract.md), [device](device-contract.md), [Engine](engine-contract.md), [pipeline](pipeline-contract.md), and [local service](local-service-contract.md).

## 1. Ownership

| Owner | Responsibilities |
|---|---|
| Plugin | Protocol transport, protocol-specific settings validation, external credentials, remote login state, target discovery, supported management operations, and evidence of transfer progress. |
| Engine | Identity, trust and binding authorization, operation coordination, channel registration, effective policy, content ownership and admission, forwarding, cancellation barriers, and stable errors. |
| Service host | Installed definitions, explicit builtin assembly, instance configuration persistence, secret transport, and the shared local API. |
| Shell | Generic capability/settings/operation rendering, user input, and explicit OS interaction such as copying a displayed value or opening a login link. |

Builtin and external implementations have equal business permissions. An external executable remains trusted local code, not an OS sandbox. No shell branches on a plugin implementation ID. Existing host-owned identity verification uses an explicit semantic purpose, not an ID check.

## 2. Definition, instance and target

An implementation ID identifies code and schema. An instance ID identifies one configuration and private runtime. A target ID identifies one addressable destination within an instance. Public identifiers are opaque and contain no URL, path, credential or remote account identifier. The same implementation can have multiple instances with isolated credentials and operations.

A static definition contains:

- Implementation ID, version, supported integration profile version, localized plain-text labels with a required fallback, and the settings schema/digest.
- Supported send and receive content kinds, target kinds, and management operation definitions.
- Identity profile: `unic_device` or `external_channel`. `unic_device` requires Engine-verifiable proofs; an external channel never becomes a device through a capability claim.
- Receipt stages the protocol can substantiate, and provenance support: `native` or `opaque`.

Replace the target interface's `TransportKind` plus `ExternalChannels` combination with one backend profile discriminator: `peer_sessions`, `object_store`, or `external_channels`. The first two require `unic_device` identity; `external_channels` requires `external_channel` identity. Actual HTTP/WebDAV/P2P transport details stay in the implementation. A SyncClipboard HTTP listener uses `external_channels` and is not forced to implement P2P connection methods or a verified repository. This is an intentional future SDK/RPC change with no compatibility adapter; current runtime types remain unchanged until implementation.

The target-kind vocabulary is `device`, `external_client`, `group`, and `repository`. A per-credential SyncClipboard client uses `external_client`; a service that only addresses an entire group uses `group`. Identity class is independent of addressability: a group must never expose member-selection controls unless it can actually address members. An external shared repository is an external channel targeting `repository`, not an authenticated native member set.

Static definitions are packaged beside external executables or embedded for builtins. Loading a definition does not start the plugin. Runtime handshake verifies the selected implementation, version and definition digest. Unsupported profile versions or inconsistent declarations make the management surface unavailable with a stable reason; never silently reinterpret fields. Existing schema limits continue to apply.

An instance snapshot contains its revision, lifecycle (`disabled`, `needs_configuration`, `starting`, `ready`, `degraded`, `unavailable`), effective capabilities, setting activation state and available operations. Dynamic capabilities can narrow the definition, not enlarge it. Lifecycle is not remote device reachability.

A target snapshot contains its revision, owner instance, target kind, identity class, display name, effective permissions, available operations, last activity and supported status evidence. Native devices can aggregate several authorized bindings. External targets are channels with no DeviceID or fingerprint. Unknown reachability stays unknown; recent HTTP requests do not establish a persistent online session.

Receipt evidence distinguishes local transport write completion, publication/server acceptance, remote receipt and remote application. A plugin may only report stages supported by its definition. No stage implies a later one. Backend failures and results remain separate even when one native device has several bindings.

## 3. Settings

Reuse the [settings schema subset](plugin-settings-contract.md); do not add executable validation or layout expressions. A schema belongs to an implementation version; values belong to an instance.

- Host configuration owns ordinary values and Engine standard policy. Plugin state must not duplicate them as a second authoritative configuration.
- The shared configuration path performs schema validation, plugin business validation and revision checks before persistence. Failed validation keeps the last valid values.
- A response distinguishes `saved` from `active` and reports restart requirements. Initial backend enablement and connection-setting changes require service restart. Standard policy retains existing live-update semantics. Do not promise live listener replacement in this profile.
- Existing secrets expose only `is_set`; callers choose retain, replace or clear. A mask is never a secret value. Plugin-owned credentials use private instance storage; host-held secret inputs use explicit secret references rather than plaintext ordinary configuration reads.
- Credential issuance, remote login and revocation are operations, not edits to a credential array in TOML.
- Replacing an account/repository context revokes the old context before installing new authorization. Stable instance IDs do not preserve old-context permission.

## 4. Management operations

Management operations are distinct from clipboard Actions and send Operations. They never implicitly transform, commit, apply or synchronize clipboard content.

The service exposes these semantics through the SDK/local API, with equivalent builtin and RPC adapters:

| Semantic operation | Required input/result |
|---|---|
| Describe | Definition, effective instance/target snapshots and currently available management operations. |
| Start | Instance, optional target, operation ID, expected subject revision and client request ID; returns a run snapshot. |
| Read | Run ID; returns the authoritative current snapshot. Events only signal changed run IDs/revisions. |
| Submit | Run ID, expected run revision, step ID, request ID and typed input; returns the next snapshot or a stable conflict. |
| Cancel | Run ID and request ID; requests cancellation and returns the actual state, not assumed rollback. |

The operation definition has a stable local ID, labels, subject scope and required step features. Availability and disabled reasons are runtime data. Definition registration binds its handler to either Engine or the owning plugin. A plugin cannot select Engine as its handler owner or register reserved identity/policy operations.

A run is scoped to one instance generation and subject. Its snapshot has a run ID, revision, status, current step, expiry and stable result. Status is `running`, `succeeded`, `failed`, `cancelled` or `interrupted`; a running step can await input or external progress. The owner advances the state machine. Shells store no transition graph.

### 4.1 Step profile

| Kind | Data and behavior |
|---|---|
| `form` | Existing schema subset and safe initial values. Secret input is write-only and excluded from subsequent snapshots. |
| `display` | Bounded plain-text fields, optional explicit copy affordances, QR payload or HTTP(S) login link. Sensitive fields use an expiring reveal handle, not ordinary snapshot data. Acknowledge is a step submission. |
| `confirm` | Plain-text effects and affirmative/negative choices. Engine identity approval additionally contains an Engine-produced fingerprint and exact device/binding revisions. |
| `wait` | Plain-text progress, optional bounded progress value and expiry. No input except cancellation. |

Render text as text. Do not execute HTML, scripts, shell commands or plugin templates. Open links and copy values only after explicit user action. CLI can show a link or QR payload as text; that presentation difference does not create a different protocol operation. Plugins declare required features before the run starts; unsupported shells return `unsupported` without starting side effects.

### 4.2 Lifecycle and side effects

- Engine validates the current user, instance, subject authorization and revision at Start and Submit. A plugin's generic confirmation cannot authorize devices, modify forwarding rules, or override global policy.
- Start/Submit/Cancel retries with the same request ID and same parameters refer to the same result; changed parameters conflict. Retain deduplication for the run lifetime plus its result-retention window. Do not rerun an external mutation after an uncertain RPC response.
- Runs have monotonically increasing revisions. Stale submissions, late callbacks and responses from old instance generations cannot change state or grant authorization.
- Independent runs may coexist, but mutations of one subject/context serialize. Do not hold Engine business locks while awaiting a plugin or user.
- Cancellation cancels pending work and waits for a bounded cleanup result. It does not undo completed external effects. Results identify whether no change occurred, a local resource was created, a remote change occurred, or the remote outcome is unknown; include only safe local object references.
- Losing a UI connection does not cancel a run. Reopening queries current state. Instance restart terminates running operations as `interrupted`; no automatic replay of non-idempotent mutations. Persist safe terminal/interruption receipts so a reconnect can distinguish interruption from success.
- A credential created before channel registration remains provisional and cannot authenticate content traffic. On registration failure remove it; retain a cleanup obligation if removal cannot be confirmed. Complete only after the channel and credential permission agree.
- Explicit secret reveal is single-consumption, scoped to the initiating local authorization and run generation, and expires. Store a generated secret only transiently until reveal/expiry. Ordinary events, reads, logs, receipts and screenshots contain no secret. Lost reveal responses do not replay the secret; offer explicit rotation, subject to the same authorization.

Use existing stable error classes where possible: invalid argument, conflict, unsupported, unavailable, resource exhausted, cancelled and expired. Never expose raw backend error text. A remote outcome that cannot be established is a separate result fact, not proof that retry is safe.

Initial bounds: at most 8 running management operations per instance and 32 per Engine; at most 32 operation definitions per implementation; 30-minute maximum run lifetime; retain at most 256 terminal receipts per instance for at most 24 hours. Evicted/expired runs return not found/expired and are never restarted by querying. Secret reveal expires after at most 5 minutes. Existing schema/RPC size limits apply; cap a display payload at 64 KiB and a QR payload at 2 KiB. These are host-enforced limits, not plugin settings. Native ticket/proof expiry may be shorter.

## 5. Shell composition and scenario mapping

Settings lists installed backends and their instance configuration. The peer list displays the Engine's native-device/external-target projection. Available operations appear beside their subject. Forwarding settings are an Engine-owned page. Shells use one implementation-independent renderer and the same service API as CLI.

| Scenario | Sequence | Required authority |
|---|---|---|
| Native pairing | Start Engine pairing operation; form for code/ticket or display generated ticket; wait for handshake; expose Engine approval on the receiving side. | Existing fingerprint, revision and binding-set checks. An incoming code never auto-approves. |
| Create compatibility client | Form for name; confirm access scope; create provisional credentials and register channel; display connection fields/QR through reveal; acknowledge. | User enables the external channel; no native identity is invented. Closing the display does not remove an already enabled channel. |
| External account login | Form for endpoint/credentials, or display login link/QR; wait; register authorized external targets. | Remote login does not grant unic trust. Account replacement invalidates old target authorization. |

Creating a SyncClipboard client authorizes a distinct credential, not a provably distinct physical phone. Clients sharing a credential share one channel. Third-party account enrollment is a scenario requirement, not an implemented backend.

## 6. Engine forwarding

### 6.1 Rule and policy

A rule has an opaque ID, revision, enabled flag, source instance/optional source-target selector, destination instance/optional destination-target selector and allowed content kinds. A selector is either explicit IDs or the explicit choice `all_authorized`; it never implies a subset the backend cannot address. `all_authorized` includes future authorized targets and must say so in confirmation. Rules default off. Same-instance automatic forwarding and temporary-transfer ingress are unsupported in the initial profile.

Engine validates rules against effective capabilities and target authorization. Removing an instance/target or changing its account context suspends affected rules; recreating a target does not revive permission. Rule updates use revision checks. Disabling a rule immediately rejects new grants and cancels its active work before the cancellation barrier succeeds. A rule never creates a device trust relationship or enables a channel.

Forwarding requires active runtime, global sync, `sync.automatic`, source receive authorization, item sync permission, an enabled rule, destination authorization, type policy, automatic file threshold and size/storage limits. Turning off automatic sync prevents automatic forwarding. `AutoApply` and `persist` remain independent: an accepted nonpersistent receipt may forward while an Engine lease owns its bytes. A processor's sync prohibition prevents forwarding even if history is retained.

### 6.2 Commit and dispatch

1. Authenticate/admit the source and reserve content resources through the shared receive path.
2. Validate complete content, run received processing once and publish the receipt with optional history/deduplication once.
3. Match enabled rules against the committed receive result. Union matching destinations so overlapping rules do not create duplicate sends.
4. Exclude the ingress path and the gateway itself. Build independently filtered destination views under normal backend policy.
5. Acquire a bounded content lease, create automatic forwarding work and recheck authorization before transport bytes or publication become visible.
6. Report per-target results separately from receive and local application. Failure does not roll back received history, rerun processors or imply remote non-receipt.

Forwarding emits no new capture, history commit or local apply. It must not dispatch through a general history observer. Do not automatically retry unknown upload outcomes or replay interrupted forwarding after restart. Existing queue/resource bounds apply; do not create an unbounded durable relay queue.

Global disable, suspension, source revocation, destination revocation and rule disable cancel related grants and content work. An HTTP pull must check the current authorization before serving metadata or file bytes and participate in the same cancellation barrier. Previously transmitted bytes or remote copies cannot be revoked.

### 6.3 Provenance and supported topology

The initial supported bridge topology is one gateway between a compatibility channel and native peers. Every automatic receipt can cross at most one forwarding hop. Native forwarding carries an authenticated gateway envelope identifying the gateway as the sender, a stable relay-transfer ID, and a forwarded marker. Native receivers never automatically forward a marked receipt. The source description inside that envelope is attribution, not proof that an external client owns a native identity.

Native receivers accept forwarding metadata only within the authenticated gateway message. Define the envelope and positive/negative protocol vectors before implementation. Do not insert the external sender into the existing authenticated-origin field. Do not silently reinterpret the current unforwarded format; the protocol remains version 1 under the project's no-backward-compatibility rule.

Native receive deduplication remains keyed by authenticated source, transfer and view. A relay retry uses the same relay-transfer ID; a separate accepted user upload uses a new ID. External and native sources are not globally merged by equal body hash.

Opaque protocols cannot preserve trustworthy hop markers. A SyncClipboard gateway can suppress an exact echo of its current published snapshot within the same credential/channel and bounded publication lifetime. This can conflate a deliberate identical re-upload and must be documented as echo suppression, not exact transfer deduplication. Do not use a long-lived global hash cache to hide the limitation.

No loop-freedom or single-history-entry guarantee is made for multiple gateways, a client simultaneously using native and opaque paths, or clients that transform and re-upload content. The product must show this limit when enabling an opaque bridge. This is a supported-topology restriction, not a claim that one node can detect every remote gateway. Multi-hop routing requires a separate decision.

## 7. SyncClipboard server profile

The first implementation uses the existing external-channel profile and
supports bounded text and image representations. The generic local channel
registration and live authorization method are documented in the
[local service contract](local-service-contract.md#local-external-channel-registration)
and [backend RPC contract](sync-backend-contract.md).
The remaining requirements below describe the target profile.

- Implementation `unic.syncclipboard` is distributed independently from `UniClipboard/unic-plugin-syncclipboard`. Install it explicitly and enable a separate plugin process. The process uses the service’s Engine over the plugin RPC connection.
- Use the external-channel profile. Issue one credential per channel. Persist only a password verifier where possible. Pause/removal/rotation must reject old requests and cancel related streams; private credential cleanup must finish or remain a reported cleanup failure.
- Ordinary settings describe listener binding and advertised endpoint separately; suggest port 42720. Basic Auth over plaintext HTTP does not encrypt credentials/content. Display that property during local-network setup; public access requires a separately configured secure endpoint.
- Engine dispatch publishes a latest immutable, permitted snapshot per channel. Register a publication target for an enabled channel even when no HTTP client is currently connected. No enabled targets means no publication. Successful publication is not remote delivery.
- The plugin does not expose arbitrary latest history. Only authorized local submissions and explicitly forwarded receipts update a channel's publication. An upload does not implicitly publish to other channels. Independent credentials do not imply shared clipboard permission.
- Upload optional file bytes into bounded staging before accepting the document commit. Use streaming Engine admission, file-policy checks, quotas and private ownership. Partial uploads and failed documents release staging; metadata must never publish before its files are complete.
- Serialize accepted document commits. Bind metadata and subsequent file requests to an immutable publication using unique data names and Engine content leases. Never serve the next publication's bytes under an older name. Retain at most the current and one previous snapshot per channel for at most 30 minutes, subject to shared resource limits and in-flight leases. Expired file requests fail rather than return another snapshot.
- Base interoperability covers GET/PUT document and GET/PUT file, Text/Image/File plus externally stored text, and the required authentication behavior. Unsupported Group/directory/rich-format payloads fail or skip as a whole with an explicit capability result; never silently send a partial directory. Exact hash/encoding/normalization rules and supported client versions must be frozen before coding the adapter.
- SSE, history APIs, content-availability extensions and remote-server client mode are separate capabilities. Do not copy v1 no-op history mutations or claim full compatibility from four route handlers.

## 8. Acceptance and implementation boundary

Before runtime code, specify failure cases and extend SDK DTOs, local API, RPC methods/schema/vectors and content ownership together. Reuse current libraries and receive/dispatch mechanisms. Current architecture remains factual until implementation passes acceptance.

Required end-to-end evidence:

1. One generic renderer exercises all three scenario shapes; an external fixture backend adds fields, targets and operations without changing shell code. This proves the interface shape, not real third-party login.
2. Native pairing rejects a bad fingerprint, stale approval, incomplete binding set and late old-generation response.
3. Operation retries do not duplicate external effects; cancellation, UI reconnect, instance restart, lost reveal response, expiry and capacity limits have explicit results.
4. Real service processes and HTTP clients verify both SyncClipboard directions, streamed files, metadata/file consistency, client isolation, rotation, pause, global disable and interrupted upload cleanup.
5. A real native transport plus HTTP client verifies both gateway directions, independent failures, overlapping rules, one-hop native suppression, repeated intentional copies and the documented opaque-echo limit.
6. Disabled rules/automatic sync, item prohibition, per-backend types/thresholds and revocation prevent unauthorized forwarding. Nonpersistent and AutoApply-off receipts preserve their independent semantics.

Generate a repeatable sanitized receipt with source revision, commands, safe configuration, operation states, history counts, target outcomes and file SHA-256 values. Keep secrets, addresses, clipboard text and private files out of that artifact. Native UI, mobile background behavior and real-client compatibility require separate authorized acceptance; process fixtures do not establish them.
