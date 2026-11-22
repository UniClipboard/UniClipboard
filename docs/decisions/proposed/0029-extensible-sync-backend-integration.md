# 0029 · Extensible sync backend integration

- Date: 2026-11-13.
- Status: The independent SyncClipboard text and image slice has passed process acceptance. Generic management, file streaming, and forwarding remain planned.
- Scope: Capability discovery, management operations, and explicit forwarding. SyncClipboard is the second concrete integration alongside tailcat.
- Specification: [Sync integration contract](../../contracts/sync-integration-contract.md).
- Related: [Plugin settings](0010-plugin-settings-schema.md), [multiple backends](0007-multiple-sync-backends.md), [external channels](0009-external-channels.md).

## Problem

A backend is not independently extensible if adding it requires a new desktop page, a protocol-specific CLI command, or a forwarding branch in Engine. Static settings schemas cover configuration fields, but cannot express credential issuance, login, pairing progress, or credential rotation.

The existing receive path never forwards content. Adding forwarding through history observers would bypass receive policy, lose provenance, repeat work, and create loops. SyncClipboard also lacks the authenticated device identity used by tailcat.

## Decisions

1. Separate plugin definitions, configured instances, and synchronization targets. Discover static definitions without executing an unenabled third-party program. Query runtime state only through the authorized instance.
2. Use one integration interface for builtin and external backends. Builtins are assembled with the host; external backends use the existing process/RPC model. Installation and process loading remain separate from this interface.
3. Reuse the existing settings schema subset. Keep ordinary user configuration in host configuration, standard sync policy in Engine settings, and credentials/runtime protocol state in private instance storage. Publish schemas and their digests with plugin definitions.
4. Introduce management operations separate from clipboard Actions. Expose four step kinds: form, display, confirm, and wait. The operation owner holds the state machine. Shells render steps and never execute plugin UI code or workflow scripts.
5. Keep identity, trust, binding approval, policy, and resource admission in Engine. Engine-owned operations use the same presentation interface as plugin-owned operations. A plugin cannot register an Engine-owned handler or approve device trust through a generic confirmation.
6. Add explicit Engine forwarding rules. Default to no forwarding. Forward committed received content without repeating processing, history insertion, or local application. Treat forwarding as automatic sending and apply every destination restriction.
7. Start with one gateway and at most one automatic forwarding hop. A forwarded native receipt is never forwarded again. Legacy protocols without provenance have explicitly weaker echo protection; do not claim reliable loop prevention for multiple gateways or overlapping legacy/native paths.
8. Distribute the SyncClipboard server as the independent, opt-in Go process plugin `unic.syncclipboard` in `UniClipboard/unic-plugin-syncclipboard`. Use one external channel per client credential. Its normal success means publication for pull. Do not introduce a remote WebDAV client role under the server's settings.

The planned management operations extend 0010 beyond settings schemas. The planned forwarding rules extend 0007 after their implementation. The initial SyncClipboard slice uses the external-channel runtime. The broad cross-backend deduplication statement in 0009 does not establish identity equivalence between an external channel and a native device.

## Scenario analysis

| Scenario | Operation owner | Generic steps | Authorization and result |
|---|---|---|---|
| tailcat device pairing | Engine | form or display, wait; Engine identity approval when required | Existing ticket, fingerprint, device revision and complete binding revision checks. No automatic trust on incoming pairing codes. |
| SyncClipboard client creation | Plugin, with Engine channel registration | form, confirm, display | Confirm local channel access; create a disabled provisional credential; enable only after Engine registers the channel; display the new secret once. |
| Remote service account login | Plugin, with Engine context replacement | form or display, wait | Remote authentication establishes an external account, not unic device trust. Replacing account context revokes old target authorizations before enabling new ones. |

All three scenarios fit the same renderer. The third is a design example, not a claim of an implemented remote backend or verified third-party login protocol. Security approval is a host-defined purpose of a confirm step, not a fifth arbitrary plugin step type.

## Alternatives rejected

- Dedicated pages and commands per backend: protocol details spread into shells.
- Static schema for every interaction: cannot represent an expiring login attempt or distinguish a saved setting from an issued credential.
- Plugin HTML, executable UI, or a workflow expression language: expands the interface and requires a separate UI runtime without a demonstrated need.
- A generic plugin command that mutates Engine trust or forwarding: defeats Engine authorization and makes capability declarations grant privileges.
- Automatic forwarding of every history commit: sends unrelated history mutations and can turn automatic work into explicit sends.
- Global content-hash deduplication: conflates separate intentional copies and cannot establish a legacy sender's identity.
- A generic object-store implementation as a prerequisite: SyncClipboard server integration does not define a verified-member repository protocol.

## Consequences

Plugin authors implement protocol behavior, a definition/schema, supported management operations, and target/status reporting. Existing-shell rendering is sufficient when the plugin fits the supported profile. New interaction primitives require an explicit host contract extension; zero shell changes for arbitrary future protocols is not promised.

Engine gains a bounded operation coordinator and a forwarding dispatcher. It does not gain a general workflow engine. Existing pairing, receive admission, resource accounting, and submission mechanisms remain authoritative and are reused.

The exact Go declarations, local service routes, and RPC schema/vector changes must implement the linked contract in one coherent interface change. They are not present in this decision and must not be inferred from illustrative names.

## Implementation order

1. Carry definitions, instance IDs, settings schemas and capability views through SDK, RPC and the local service. Connect existing configuration persistence and report activation separately from saving.
2. Implement management operations and one generic desktop/CLI renderer. Exercise tailcat pairing and SyncClipboard client creation through the same renderer.
3. Implement bounded streaming channel reception and the SyncClipboard server through Engine-owned content leases. Verify bidirectional base-protocol behavior with real processes.
4. Implement explicit one-hop forwarding, then verify the phone/gateway/tailcat topology. Do not advertise gateway support before this acceptance passes.
5. Extend client compatibility only against an explicit client/version matrix. SSE and history extensions are independent capabilities, not implied by the base protocol.

After implementation and acceptance, move current facts to architecture pages, update affected contracts and their implementation status, and only then archive this decision.
