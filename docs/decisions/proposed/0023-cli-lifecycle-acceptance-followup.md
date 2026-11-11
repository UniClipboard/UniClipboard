# 0023: CLI lifecycle acceptance follow-up

- Status: Draft
- Date: 2026-11-11
- Requirement: Close the acceptance items that the archived on-demand service lifecycle work left open.
- Current behavior: [headless architecture](../../architecture/headless.md), gaps section.

## 1. Context

The lifecycle behavior is implemented and accepted on macOS against one frozen binary set (30 PASS, 8 NOT_RUN, 0 FAIL). Two items remain as separate work.

## 2. Scope

1. **C36, other platforms.** Run the process matrix on Linux and Windows. Today these platforms are compile-checked only.
2. **File release barrier.** After a send deadline, assert that the file resource is released (no held lease, no residue in the transfer directory).
3. **C03 race.** Find the cause of the single C03 failure ("temporary service outlived its last user"). Repeat the case under load until it fails or a bound is reached.

## 3. Choice

Add process tests for each item. Do not change lifecycle behavior unless a test exposes a defect.

## 4. Exit criteria

Each item has a passing process test or a documented, reproduced defect. Move the result into the headless architecture gaps, then land and archive this decision.
