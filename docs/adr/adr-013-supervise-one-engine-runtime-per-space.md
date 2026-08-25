# ADR-013: Supervise One Isolated Engine Runtime per Space

## Status

Proposed

## Context

The Engine intentionally models one active space in an in-memory session and in
its active-space manifest. Desktop and HarmonyOS clients currently expose that
single runtime as the entire application's space state. Supporting retained,
simultaneously online memberships therefore requires either weakening the
Engine's security boundary or supervising multiple boundaries.

## Options Considered

### One Engine instance with multiple active spaces

Pros:

- A single lifecycle and API surface.

Cons:

- Requires pervasive changes to session, encryption, repositories, network
  identity, admission journals, and every current-space call site.
- Makes accidental cross-space key or clipboard routing substantially easier.
- Invalidates the Engine's current `ActiveSpace` semantic contract.

### Multiple retained profiles with one active runtime

Pros:

- Smaller change and lower mobile resource use.
- Preserves the current Engine invariant.

Cons:

- Other spaces are offline until the user switches.
- Does not satisfy concurrent background synchronization.

### One isolated Engine runtime per space

Pros:

- Preserves the existing one-active-space Engine invariant.
- Gives storage, key, identity, admission, and failure isolation by construction.
- Allows all retained spaces to receive concurrently.
- Supports incremental adoption through the existing `profile_id` boundary.

Cons:

- Requires a host supervisor and space-scoped APIs.
- HarmonyOS native singletons must become a runtime map.
- Resource usage grows with the number of online spaces and needs lifecycle
  controls.

## Decision

Use one isolated Engine runtime per retained space and supervise those runtimes
at the host boundary. Local clipboard capture is sent to the selected space by
default; explicit multi-space fan-out is a host routing operation, not implicit
Engine bridging.

## Rationale

This is the only option that meets simultaneous multi-space synchronization
without weakening cryptographic isolation or rewriting the Engine's core space
semantics. The existing `profile_id` concept provides a compatible namespace for
keys and repositories, while host-owned directories complete physical storage
isolation.

## Consequences

- Desktop daemon and HarmonyOS native APIs become profile-addressable.
- The host owns catalog, selection, routing, and aggregate status behavior.
- Engine correctness fixes remain reusable by every runtime.
- Mobile clients need bounded idle behavior and per-runtime resume handling.
- Existing switch-space behavior remains explicit and is no longer the normal
  path for joining another space.

## References

- [Multi-Space Device Groups PRD](../prd/2026-08-25-multi-space-device-groups.md)
- [Multi-Space Runtime and Admission Recovery spec](../specs/2026-08-25-multi-space-runtime-spec.md)
