# Multi-Space Device Groups

## Status

Draft

## Context

The current clients treat joining a space as a destructive replacement of the
profile's current space. This makes two distinct user tasks look identical:

- adding another phone or computer to an existing device group; and
- joining an additional, independent device group.

The result is that users can accidentally replace their current membership and
can be forced into reset-based recovery after an interrupted admission. The
Windows client also has a persistence defect that can report a failed admission
after its snapshot file was written, leaving durable admission state unfinished.

## Goals

- A space can contain multiple phones and computers.
- A phone or computer can retain membership in multiple spaces.
- All retained spaces can remain online and receive updates concurrently.
- Adding a device or space never resets an unrelated space.
- Existing single-space installations become the first catalog entry without
  moving or deleting their data.
- The client clearly identifies the target space when issuing an invitation.

## Requirements

- Each space has isolated storage, key material, network identity, lifecycle,
  admission state, and failure state.
- A local space catalog records retained memberships and the currently selected
  outbound space.
- Incoming clipboard events from every running space remain eligible for local
  presentation and include their source space identity.
- A local clipboard capture is sent only to the currently selected space by
  default. The user may explicitly select additional destinations.
- A new join creates a new catalog entry and runtime. It does not invoke the
  existing destructive switch-space flow.
- An invitation is always issued from a selected space and adds the joining
  device to that space.
- Admission recovery must converge or safely cancel the affected admission. A
  global client reset is not an accepted recovery mechanism.

## Non-goals

- Automatically forwarding clipboard content from one space to another.
- Sharing encryption keys, databases, or network identities between spaces.
- Combining membership histories from independent spaces.
- Treating a failed runtime as a reason to stop healthy runtimes.

## Constraints

- The Engine currently supports one active space per Engine instance.
- HarmonyOS currently exposes its Engine runtime and event queues as process-wide
  singletons, so its native API must become handle- or profile-addressable.
- Mobile background resource use must remain bounded. Idle runtimes may reduce
  polling activity, but they must keep their network receive path recoverable.
- Existing space data and secure-storage identifiers must remain valid during
  catalog adoption.

## Acceptance Criteria

- A three-device space can add the second and third devices consecutively
  without resetting any participant.
- The same client can join two independent spaces and receive from both while
  both runtimes are online.
- Sending from the selected space reaches only that space unless extra
  destinations were explicitly selected.
- Stopping or corrupting one test runtime does not stop or reset another runtime.
- Upgrading an existing single-space installation preserves its device identity,
  history, files, and space membership as the first catalog entry.
- A failed admission can be retried or superseded without clearing the client.
