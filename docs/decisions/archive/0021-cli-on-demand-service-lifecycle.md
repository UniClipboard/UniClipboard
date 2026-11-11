# 0021: On-demand service lifecycle for CLI commands

- 状态：已归档（2026-11-11）
- 落地：[headless architecture](../../architecture/headless.md), [local service contract](../../contracts/local-service-contract.md)
- Note: The remaining gaps (cross-platform process runs, file release barrier, C03 race) are listed in the headless architecture gaps. Resource thresholds were fixed after implementation and cannot be repaired. Gaps in section 11 are historical.
- Date: 2026-11-10
- Requirement: Declare whether each CLI command needs unicd. Start it when needed and absent. Stop a CLI-created temporary service after its users finish.
- Execution plan: [0021 implementation plan](../../exec/0021-cli-on-demand-service-lifecycle.md).
- Current behavior: [headless architecture](../../architecture/headless.md), [local service contract](../../contracts/local-service-contract.md), and [observability contract](../../contracts/observability-contract.md).
- Related decisions: [0015 headless CLI and desktop integration](0015-headless-cli-and-desktop-integration.md) and [0018 independent product builds and releases](0018-independent-product-build-and-release.md).
- Intended landing: the headless architecture and local service contract; update observability and Engine contracts only where their behavior or interfaces change.

## 1. Problem and scope

Most current CLI operations require the user to start unicd first. This exposes a service-management step before ordinary history, configuration, pairing and submission operations.
Some commands already work without a service. These include help, config path, local plugin management and local log export. Starting a service for them adds cost and side effects without providing a capability they need.

The requirement is reasonable, but a literal start-call-stop wrapper is insufficient:

- An existing service may belong to the desktop or to a user running unicd explicitly.
- Several CLI processes may use the same temporary service concurrently.
- A command response may precede network connection or send completion.
- An invitation or temporary setting may need a live service after its value is printed.
- Offline migration requires exclusive access to its target profile.
- Starting unicd does not provide a system clipboard adapter.

Adopt one model: explicitly declared command requirements, automatic acquisition of a suitable service, and generation-scoped release after the command's defined completion condition. A temporary service exits when its last user finishes. Existing persistent services retain their current owner.

Sections 1 to 10 record the design. Section 11 records the concrete rules adopted at implementation. The exact wire shapes and budgets are in the [local service contract](../../contracts/local-service-contract.md#service-lifecycle).
This decision grants no native or GUI launch permission and does not establish native acceptance.

## 2. Verified starting point

Source inspection at `516c9f951cf45b2d1203748f27b0e887d2a2f215` found:

- `app/service/cmd/unic/main.go` dispatches history, plugin and migration commands separately from `runCLI`. Dependency handling placed only in `runCLI` would miss commands.
- `runCLI` constructs a socket client, but does not start unicd. Connection absence is reported through `api.ErrNoDaemon`.
- `plugin.go` operates on local files. `logs export` reads local segments. `config path` resolves a local path.
- `internal/servicelifecycle` exposes service identity, profile identity, generation-matched stop and owner-pipe monitoring.
- `app/desktop/internal/lifecycle` locates a sibling unicd, serializes startup, verifies readiness and bounds shutdown. Normal desktop startup deliberately replaces an existing service. That replacement policy must not become the CLI's reuse policy.
- Service metadata currently distinguishes desktop-owned and standalone processes. It cannot identify a CLI-temporary generation or track several CLI users.
- `Engine.Start` explicitly does not guarantee peer reachability. The peer backend sends to currently connected peers and queues work without waiting for network completion.
- `SubmissionResult` separates local commit, sync disposition and operation identity. The service's cached submission response is not a terminal network-operation receipt.
- No adapter means clipboard capabilities are unavailable. The CLI must not substitute history for the current system clipboard.

These facts describe the baseline. New requirements below are not current behavior.

## 3. Declare requirements on leaf commands

Keep dependency declaration beside the leaf command's argument parsing and handler. Resolve aliases to the same declaration. Generate dependency badges in help from that declaration instead of maintaining an unrelated help-only list.

The declaration has two independent concerns: service requirements and completion policy. Desktop-adapter capability is an additional requirement, not proof that a service must be launched through the desktop.

| Requirement | Current examples | Proposed behavior |
|---|---|---|
| Local | All help; `config path`; `logs export`; `plugin list/enable/disable` | Execute without service acquisition. |
| Service | Status, capabilities, cache, devices, pairing, history, received, submissions, configuration operations other than path, log-level controls, telemetry state and watch | Reuse a valid service or acquire a temporary one. Apply the command-specific completion policy. |
| Service plus desktop adapter | `copy`, `paste`, `history use`, Action `--apply` | Acquire the service, then check the required adapter capabilities. Return existing capability errors if absent. Never launch a GUI or create a second clipboard implementation. |
| Offline exclusive | `migrate` | Use the migration helper's exclusive target-profile lock. Do not start a service or stop an existing owner. |

Help and invalid arguments must not start a service. Flags may change a leaf's requirements, as with Action `--apply`. Migration's offline requirement applies to its actual target directory, which may differ from the CLI's default profile.

Keep this a small command requirement declaration and one lifecycle module. Do not add a plugin framework, per-command launch configuration or a new CLI framework solely for this feature.

## 4. Separate service mode from client usage

Recognize three service lifetime modes:

- **Standalone:** explicitly started by the user or an existing persistent service owner. CLI commands borrow it and never stop it on completion.
- **Desktop-owned:** controlled by the normal desktop's existing owner lifetime. CLI commands borrow it and never stop it on completion.
- **CLI-temporary:** started on demand for CLI work. Its lifetime is shared by active CLI users.

A second CLI finding a temporary service is another temporary user. It must not classify that process as persistent merely because another CLI started it.

Track temporary users through live, generation-bound leases. Acquiring a lease admits one invocation. Releasing it ends that invocation's use. Multiple commands can run concurrently. The last release closes admission and initiates graceful shutdown atomically.

The launcher must hold initial ownership from process creation through readiness and first-user admission. A ready service with zero admitted users must not exit before the initiating CLI can acquire it. A later client racing with last-user shutdown must either acquire the still-live generation or retry acquisition before starting its business operation.

Do not rely on a persistent integer reference count or a reusable PID. A killed CLI cannot reliably run deferred cleanup. Use a maintained live IPC/owner-pipe mechanism so process/connection loss releases its lease. Bound pending acquisition and lease bookkeeping. Do not create a durable lease database or add an arbitrary idle-retention setting.

Lease identity is local control material. It does not enter analytics, logs, metrics or ordinary status output. The exact request/response shapes, budgets and crash behavior belong in the local service contract before coding.

A temporary service stops immediately after its last user releases it, subject to bounded graceful cleanup. Do not keep it running for a convenience idle timeout after command completion. Command-scoped temporary mode provides no continuous reception between invocations. Standalone unicd and desktop-owned operation remain the persistent alternatives.

## 5. Acquisition, startup and shutdown

The lifecycle module owns these steps:

1. Validate command arguments and resolve the selected profile and socket.
2. For a service-dependent operation, inspect the recognized service endpoint.
3. Validate its profile identity, generation and required lifecycle capabilities. Reuse it according to its lifetime mode.
4. Start a sibling unicd only when the endpoint authoritatively reports that no service is listening. A timeout, permission failure, unrecognized listener or wrong-profile response is an error, not permission to replace a process or unlink a socket.
5. Serialize creation and retirement with a short profile-scoped lifecycle lock shared with desktop transitions. Do not hold this lock throughout command execution; a long-running watch must not block another CLI's acquisition.
6. Locate unicd beside the resolved CLI executable, including `.exe` on Windows. Preserve the canonical profile and selected socket explicitly. Use the shipped tested CLI/service pair rather than silently selecting another executable from PATH.
7. Wait for readiness matching the launched process, profile, generation and temporary lifetime mode. Socket existence alone is insufficient. Reuse the existing finite startup budget and typed directory-in-use retry boundary.
8. Execute the operation while retaining its lease and any command-owned content resources needed for completion.
9. Release the lease on success, failure or cancellation. If this was the last temporary user, wait for that generation's process termination and profile-lock release within the shutdown budget.

When other temporary users remain, a completing command must not wait for the shared process to exit. The initiating CLI may finish before a later borrower; the borrower must keep the service alive independently.

Cleanup uses its own bounded context even when the command context is already cancelled. Forced cleanup may target only the verified temporary generation, with no other admitted users. Never kill a process using a stale PID or stop a replacement generation.

Do not automatically replay a business mutation after a timeout, uncertain response or service replacement. Acquisition retries occur before business execution. Existing request-id and uncertainty rules continue to apply; in-memory request records and operation handles do not become durable across restart.

The CLI retains its stdin for text/file commands. A separate ownership channel controls unicd. Startup output must not contaminate command stdout or structured JSON. Missing executables, invalid configuration, startup failure and failed cleanup must produce actionable errors without exposing credentials or arbitrary transport error text.

## 6. Command completion is part of the requirement

The following policies are proposed defaults, not an assertion that every command should wait for every background task.

### 6.1 Finite local/service operations

Queries complete after their response or stream has been fully consumed. File export completes after its output is closed successfully. State mutations complete at their authoritative local commit.

Approval/revocation changes do not claim that a peer has connected or received content. Pending pairing state is not successful mutual pairing. A new service's status describes its current process; it does not prove that a service was running before this command.

The plain `status` command becomes service-dependent and may start a temporary service. Its help must state this. Do not silently retain the former absence-as-startup-error behavior while advertising automatic service acquisition.

### 6.2 Submissions and command-originated sends

A successful service startup is not a connected-peer barrier. A `queued` submission is not a completed send. Closing a temporary service at either point can prevent useful synchronization.

For explicit submission and other operations that initiate sends:

- Define a finite connection opportunity for already eligible peers before the one-time dispatch decision. Do not wait indefinitely for offline devices or pending approval.
- Preserve policy-disabled, no-backend and no-target outcomes. A successful local commit remains independent of network results. Do not manufacture a successful send when the connection opportunity expires.
- Submit the content once. If an operation is queued, retain the command's lease until that operation reaches a terminal outcome or the command's bounded deadline/cancellation policy resolves it.
- Wait only for work initiated by this command. Do not drain unrelated traffic or wait for every background retry in the service.
- Keep local commit, local send outcome and optional clipboard-application outcome separate in output and exit semantics. Successful local sending still does not prove remote receipt, application or paste.
- Keep source-file resources valid until this command's send work ends. Do not exit and tell the caller that a referenced temporary file is safe to delete while it remains queued.

Propose the same explicit send-completion policy whether the command borrowed or started the service. Avoid defining send success differently based on service presence. This changes current commit/queue-only CLI completion and requires the local service contract to define operation observation and final result/exit mapping before implementation. Repeated reads of a cached submission result are insufficient.

### 6.3 Sessions, invitations and temporary settings

| Command intent | Proposed completion policy |
|---|---|
| `watch` | Retain the service for the event stream. Release on stream termination, cancellation or error. |
| `pair` / `join` awaiting approval | Treat pairing as a bounded session. Distinguish confirmed pairing, still-pending approval, expiry and interruption. A pending record may remain; do not promise it will complete after temporary service shutdown. |
| `code` / `addr` invitation | An invitation needs a live listener. With a CLI-temporary service, print the invitation and retain the command's lease for its validity window or until cancellation/authoritative completion. With a persistent owner, the command may return after publication because that owner preserves availability. |
| Log-level query/reset | Complete after the authoritative response. |
| `logs level ... --for DURATION` | With a temporary service, retain the lease through the override window or its authoritative replacement/cancellation. With a persistent owner, retain current setter behavior and return after acceptance. |

These invitation and temporary-setting policies deliberately affect foreground duration when a temporary service is used. Help and lifecycle messages on stderr must make that duration visible. A script capturing an invitation for later use must arrange a persistent service; command substitution cannot preserve a temporary listener after the invocation finishes.

Do not identify pairing completion from an unrelated peer event or cancel a newer invitation/override when an older command exits. Window expiration and replacement must refer to the state created by that invocation. Do not invent arbitrary sleep durations or infer completion from printed text.

Cancellation releases only that command's usage. It does not stop other users or undo already committed history/trust changes. Any cancellation of the command's own session state must respect its current generation and ownership.

## 7. Desktop and migration interaction

Preserve the current normal-desktop replacement rule. CLI auto-start does not replace a desktop-owned service and does not launch or promote a GUI.

A normal desktop may explicitly replace a CLI-temporary generation under the shared transition lock. Old CLI leases then become invalid. Affected commands report interruption or uncertain mutation results. Their cleanup must not stop the new desktop-owned generation, and writes must not be retried automatically.

Lightweight desktop mode requires a persistent standalone service. It must not silently borrow a CLI-temporary generation as if it were persistent. Use bounded retirement waiting or a clear ownership conflict; introducing ownership promotion is outside this proposal.

A direct standalone unicd invocation still follows the exclusive profile-lock rule. Do not turn it into an implicit takeover of active temporary clients. Migration likewise reports an in-use target instead of closing desktop, standalone or temporary owners.

## 8. Architecture and alternatives

Reuse the current service-generation stop barrier, exclusive profile identity, sibling executable resolution and process readiness concepts. Put reusable process/lifecycle behavior in a shared internal module. Keep command-specific validation and completion at the CLI leaf. Keep service mode and temporary-user admission authoritative in unicd.

The CLI must remain free of graphical dependencies and must not create an Engine or open the live history database itself. Do not make it depend on desktop-internal startup policy. No system clipboard implementation is added to CLI/service.

On-demand startup and process reuse are established patterns; Git's [credential-cache documentation](https://git-scm.com/docs/git-credential-cache) describes contacting an existing local daemon or starting one when absent. Git's cache timeout is not adopted here: this requirement releases the temporary service after its last command user completes.

Rejected alternatives:

- Requiring manual startup for every service-dependent command: retains the usability problem.
- Unconditionally stopping after an HTTP response: can stop another owner or interrupt queued work.
- Giving ownership only to the first CLI: a later borrower loses service when the starter exits.
- Serializing whole invocations under one startup lock: a watch/session blocks unrelated commands.
- Always leaving an automatically started daemon running: does not satisfy command-scoped lifetime.
- Automatically launching the desktop for clipboard operations: changes the headless boundary.
- Embedding a second Engine into CLI: violates profile ownership and duplicates service assembly.
- Adding compatibility branches for older lifecycle metadata: the project is unreleased; change the contract directly and retain protocol version 1.

## 9. Consequences and adoption boundary

The benefit is a direct CLI workflow for ordinary operations while preserving explicit persistent service ownership. The cost is a shared temporary-usage protocol and precise command-completion semantics. Repeated cold invocations pay service startup and reconnection costs; no latency or reliability improvement is claimed before measurement.

Temporary service startup must not redefine each invocation as a new installation. Existing analytics identity persists while enabled, and startup remains a process-start signal. Do not add a high-cardinality event or identifier for every lease.

No new user configuration is required for the core model. Keep finite internal startup, shutdown and operation budgets; freeze their exact values in the contract before coding. Do not silently change upload defaults, sync policy, service-manager registration or platform login behavior.

The user requested a decision document, not runtime implementation. Record the foreground-duration and send-completion changes as proposals requiring design adoption. Before implementation, write an execution plan and exact lifecycle/operation contract. Keep the current living architecture's behavior descriptions unchanged until implementation is verified.

## 10. Required process acceptance

Use real CLI/service processes. Define the failure inventory before runtime code. Do not add unit tests after implementation.

The execution plan must cover:

1. Local/help/invalid commands create no daemon; every leaf/alias/conditional dependency has a deliberate declaration.
2. Cold startup, warm standalone reuse and desktop-owned reuse select the correct profile and preserve the original owner.
3. Concurrent CLI startup creates one temporary generation; the starter can exit while another borrower continues; the last release closes it.
4. CLI crash, force-kill, stream disconnect and cancellation release usage without relying on deferred CLI cleanup.
5. Last-release/new-acquire races, stale sockets, wrong profiles, unknown listeners, permission errors and startup timeout never cause an unrelated unlink or kill.
6. Missing sibling binary, bad configuration and directory-lock contention fail with bounded cleanup and accurate output.
7. Cold send waits for its defined connection opportunity, handles no-target outcomes, observes only its own operation and does not stop at queued admission.
8. Invitations, approval sessions and temporary overrides retain the required availability window; replacement/cancellation affects only their own state.
9. Clipboard-dependent commands without an adapter report capability errors and never launch a GUI.
10. Normal desktop replacement invalidates old CLI leases; old cleanup cannot stop the replacement. Lightweight/migration conflicts follow their explicit rules.
11. stdin pipelines, JSON stdout, command errors and cleanup errors retain usable script semantics. Uncertain writes are never automatically replayed.
12. Production headless dependencies and existing CLI/desktop-lifecycle process regressions remain valid.

Retain a sanitized reproducible artifact containing source/binary identity, build flags, exact commands, scenario expectations/results, relative fixture timing, observed generation/lease transitions, cleanup assertions and checksums. Fixture correlation must not become production telemetry. Keep PASS, FAIL and NOT_RUN distinct. A documentation check is not runtime or native acceptance.

## 11. Adopted rules and remaining gaps

These rules close the open points above. They changed current CLI behavior and are now implemented.

1. **Lease mechanism.** A lease is an open HTTP response on the private socket, bound to one generation. Connection loss releases it. There is no durable counter. Capacity is 64 leases per generation.
2. **Initial ownership.** The launcher passes an anonymous pipe as the child's stdin. The child does not retire while that claim exists and no lease was admitted. EOF with no lease retires it.
3. **Last release.** The last release closes admission, answers `last:true`, then cancels the service. Only the releasing command that receives `last:true` waits for the generation to stop. A client that meets `retiring` retries acquisition before any business request.
4. **Forced cleanup.** Only the launcher that created the generation may kill its own child, after the graceful budget. Any other command reports the failure with exit code 5.
5. **Send completion.** A send-initiating command observes its own Engine operation through `GET /v1/operations/{id}` and reports `success`, `partial`, `failed`, `superseded`, `timeout`, `interrupted`, `unknown` or `none`. Exit code 4 means committed but not fully completed. `submit --json` changes to `{submission, send}`.
6. **Connection opportunity.** A command that sends gives offline paired devices until 10 s after the service generation started. The rule is the same for every service. It is not tied to who started the service.
7. **Pairing.** Under a temporary service, `pair` and `join` wait up to 2 minutes for the other device. Under a persistent owner they return after the request is recorded.
8. **Invitation and override windows.** Under a temporary service, `code` and `logs level LEVEL --for` hold the service until their own window ends, is reset or is replaced. `addr` has no window: it prints, and stderr states that the address needs a running service.
9. **Desktop.** Normal desktop keeps its replacement rule, including for a temporary generation. Lightweight desktop refuses a temporary generation with a clear error. Neither adopts it.
10. **Telemetry.** No lease identifier enters telemetry. A temporary start is a process start, as before.

Remaining gaps, recorded in the execution receipt: partial and slow-file sends (C23) are not tested, a service replaced during a submit is not tested (C24), and platforms other than macOS are compile-checked only (C36). One behavior is open: after a restart, a peer trusts the previous process's direct path for 6.5 s, so reconnecting can take longer than the 10 s window and a cold-start send may deliver nothing (receipt, defect 4).
