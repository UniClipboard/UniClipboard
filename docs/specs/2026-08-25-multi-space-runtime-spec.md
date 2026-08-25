# Multi-Space Runtime and Admission Recovery

## Status

Draft

## Overview

Multi-space support is implemented by supervising multiple isolated Engine
instances. The Engine's existing one-active-space invariant remains local to an
instance; the clients no longer try to make one instance represent several
spaces.

The same work removes reset-based pairing recovery. Admission persistence must
report the result of the actual durable operation correctly, and an interrupted
admission must be recoverable within its owning runtime.

## Goals

- Keep multiple spaces online concurrently on desktop and HarmonyOS.
- Preserve cryptographic and storage isolation between spaces.
- Make invitation and join operations explicitly space-scoped.
- Recover interrupted admissions without global state deletion.
- Preserve current single-space installations through an idempotent migration.

## Design

### Space catalog

The host owns a small catalog outside every Engine profile. Each entry contains:

- a stable local `profile_id`;
- the admitted `space_id` when known;
- a user-visible name;
- the profile data/cache directories;
- lifecycle status and last non-secret error category; and
- whether the space is the default outbound destination.

The catalog never stores space secrets. Secure-storage keys remain namespaced by
`profile_id`. Catalog updates use atomic replacement and are independent from an
Engine database transaction.

### Runtime supervisor

`SpaceRuntimeSupervisor` owns a map from `profile_id` to `SpaceRuntime`. A runtime
contains one Engine instance, one event subscription, and its background tasks.
Starting, stopping, unlocking, joining, inviting, and resetting are all scoped to
one profile.

One runtime failure changes only that catalog entry's status. The supervisor
continues servicing healthy runtimes and exposes per-space retry actions.

On desktop, the daemon API gains a space/profile selector and the GUI keeps one
authenticated daemon session. On HarmonyOS, the process-wide `SPACE_RUNTIME`,
task slots, event queues, and mutable flags are replaced by a supervisor map.
Native exports accept a `profile_id` or an opaque runtime handle. Compatibility
wrappers route legacy calls to the adopted default profile during migration.

### Clipboard routing

Every incoming event is tagged with `profile_id` and `space_id` before it reaches
the host. Its stable local identity is `(space_id, event_id)` so events from
different spaces cannot collide.

A local capture is published to the selected outbound profile by default. If the
user explicitly selects multiple destinations, the host submits an independent
publish operation to each runtime. Failure in one destination does not roll back
successful destinations and is reported per space.

Loop prevention includes the source space in its key. Applying an event received
from one space must not automatically publish it into another space.

### Join and invitation flows

Joining an additional space allocates a fresh profile and runs admission there.
The existing profile remains online. If admission fails before the new profile
becomes active, the incomplete catalog entry is retained as retryable or can be
removed without affecting any admitted profile.

Issuing an invitation requires a selected admitted profile. The invitation adds
the joiner to that space; it never creates or switches the sponsor's space.

The destructive switch-space endpoint remains available only as an explicit
legacy migration operation. It is not used by normal multi-space join UI.

### Admission durability and recovery

The Windows generation writer already uses `MoveFileExW` with
`MOVEFILE_WRITE_THROUGH`. After that succeeds, opening the parent directory with
`std::fs::File::open` is not a valid Windows durability operation and must not
turn the successful replacement into a storage failure. Parent-directory fsync
remains required on platforms where opening and syncing directories is
supported.

Admission recovery is scoped to the owning runtime:

- pre-commit attempts may be durably rejected or superseded;
- post-commit attempts must resume activation or append the protocol-defined
  pending-member removal before another admission begins; and
- a new invitation must surface a recoverable pending-admission state instead of
  recommending a profile reset.

Recovery operations are idempotent and must preserve membership history
monotonicity.

### Existing-installation migration

On first startup with no catalog, the supervisor adopts the existing data root as
the `default` profile. It does not copy, rename, re-encrypt, or reset the existing
database, blob store, identity, or secure-storage namespace. The migration writes
the catalog only after the current profile can be inspected successfully.

Repeated startup is idempotent. If catalog creation fails, the legacy single
runtime remains usable and startup reports a retryable migration error.

### Title-bar safe inset

The desktop custom window-control group uses a 16 px end inset instead of 8 px.
The inset applies only where custom controls are rendered and does not alter the
macOS native traffic-light position.

## Data Model

```text
SpaceCatalog
  version
  selected_outbound_profile_id
  entries[]

SpaceCatalogEntry
  profile_id
  space_id?
  display_name
  data_root
  cache_root
  lifecycle_status
  default_outbound
```

Runtime-only handles, decrypted keys, invitation secrets, and passphrases are
never serialized into the catalog.

## Invariants

- One Engine instance has at most one active space.
- One profile belongs to at most one admitted space.
- A space's database, blobs, identity, key namespace, and admission journal are
  never shared with another profile.
- A host reset or recovery command names exactly one profile.
- Receiving from one space never implicitly publishes to another.
- Catalog adoption never mutates existing profile contents.

## Edge Cases

- Two simultaneous joins allocate different profile IDs and cannot share a data
  directory.
- Rejoining a previously removed space creates a new admission attempt within
  the retained or newly allocated profile according to its terminal state.
- Duplicate incoming content from two spaces remains distinguishable by source
  even when its payload hash is equal.
- A locked space remains listed while other unlocked spaces continue running.
- Mobile background suspension marks affected runtimes suspended and resumes
  each independently when the ability returns to the foreground.

## Failure Handling

- Catalog errors never trigger Engine profile deletion.
- A per-space startup, unlock, network, or admission error is attached to that
  entry and is retryable independently.
- Partial multi-destination send results report success and failure per space.
- No automatic path recursively deletes profile roots.
- Destructive removal of a retained space requires explicit user confirmation
  and is outside the admission retry path.

## Verification

- Windows unit test: a new generation file is written and `write_new_file`
  returns success after atomic replacement.
- Engine regression: add devices B and C consecutively to A's space without
  resetting A, B, or C.
- Engine recovery regression: interrupt an admission at each durable stage,
  restart, recover, and then admit another device.
- Supervisor tests: two isolated profiles start, receive, send, and stop
  independently.
- Routing tests: default sends to one selected space; explicit fan-out reports
  per-space outcomes; inbound content is not bridged.
- Migration test: a populated legacy profile is adopted byte-for-byte and the
  second startup is a no-op.
- Desktop component test: custom window controls use a 16 px right inset.
- Device test: the installed desktop build and HarmonyOS app pair, then another
  test device joins the same space without clearing either existing client.

## Related Decisions

- [ADR-013: Supervise one isolated Engine runtime per space](../adr/adr-013-supervise-one-engine-runtime-per-space.md)
