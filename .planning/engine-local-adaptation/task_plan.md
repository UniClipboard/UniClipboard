# Local Engine adaptation plan

## Goal

Make the desktop workspace resolve Engine crates from `../engine`, then complete the desktop, daemon, and CLI cutover to the current Engine device-group contract without legacy product paths.

## Completion criteria

- [x] All relevant Engine crates are overridden from one machine-level Cargo patch.
- [x] Cargo metadata proves the selected packages come from `../engine`.
- [x] Recent Engine interface changes and documentation are identified from current local history.
- [x] Every affected desktop call site is mapped to a required adaptation and validation step.
- [x] Existing unrelated worktree changes are preserved.
- [x] Daemon contract, server, client, CLI, and frontend use only the new device-group flow.
- [x] Product-facing workspace-convergence paths are removed; dev-only diagnostics remain feature-gated.
- [x] Generated API artifacts match the new wire contract.
- [x] Focused tests, full builds, and real dual-window UI flow verification are complete.

## Phases

1. **Repository and dependency inventory** - complete
2. **Apply and verify local Cargo patch** - complete
3. **Analyze Engine interface changes** - complete
4. **Map desktop adaptation work** - complete
5. **Final verification and report** - complete
6. **Backend contract RED tests** - complete
7. **Backend and CLI cutover** - complete
8. **Frontend RED/GREEN cutover** - complete
9. **Generated API refresh** - complete
10. **Full verification and review** - complete

## Errors encountered

| Error | Attempt | Resolution |
| --- | --- | --- |
| `cargo metadata --locked` rejected the local patch because `Cargo.lock` needs a source update | 1 | Regenerate the lock offline, then rerun locked verification |
| Parallel Cargo queries briefly contended on the package-cache lock | 1 | Run the lock update and subsequent Cargo proof sequentially |
| zsh reserves `status` as read-only | 1 | Avoid that shell variable in subsequent commands |
| Offline lock refresh lacked four new third-party crates required by local Engine | 1 | Allow Cargo to fetch the missing published dependencies, then verify with `--locked` |
| Planning-file patch had a malformed hunk boundary | 1 | Corrected the patch structure; no project files were affected |
| Workspace `target` symlink pointed to a removed external directory | 1 | Recreate only the missing project-specific target directory and rerun the check there |
| One memory search command had an unmatched shell quote | 1 | Reissued the read-only search with a single-quoted pattern |
| TDD skill references `testing-anti-patterns.md`, but no such file is installed | 1 | Follow the complete core TDD instructions and existing repository test patterns |
| Initial DTO test patch did not match the exact existing assertions | 1 | Read the current test block and reapplied a current-range patch |
| First production DTO patch used an outdated comment as context | 1 | Read exact ranges and reapplied a narrower current-range patch |
| Planning progress patch had a malformed hunk boundary | 1 | Corrected the patch structure; no production files were affected |
| Initial OpenAPI cardinality patch targeted the assertion instead of its constant | 1 | Located and updated `EXPECTED_PATHS` |
| OpenAPI generation expected 73 paths after two legacy routes became one query/command route | 1 | Updated the guard to the new complete surface of 72 paths |
| First locale patch assumed one Traditional Chinese phrase variant | 1 | Read every locale's current keys and applied exact-context updates |
| Full Rust tests found a second OpenAPI path-count guard still set to 73 | 1 | Updated the independent smoke-test guard and explanatory comments to 72 |
| OpenAPI smoke test still froze the removed operation IDs | 1 | Replaced them with `getDeviceGroupChoices` and `chooseDeviceGroup` |
| React Doctor package download made no progress | 1 | A later retry completed; changed-file scan passes with only four existing complexity warnings |
| In-app browser control tool was unavailable after required discovery | 1 | Verify interaction through rendered component tests and validate the live development server response |
| Targeted `cargo update -p uc-engine` could not match the path-sourced lock entry after disabling the patch | 1 | Re-resolve metadata without the patch, then inspect the lock diff before validation |
| Existing dual-peer test expected cancelled invitations to fail synchronously | 1 | Added a focused valid-invitation dual-window test for the current asynchronous contract |
| Joiner stayed pending after Engine removed the completed join projection | 1 | Confirm completion from the newly active peer relative to the pre-join device baseline |
| Engine architecture check rejected a second membership observation entry | 1 | Kept one observation entry and shared its decorated membership committer with sponsor admission |
| Engine repository `target` symlink destination was unavailable | 1 | Ran architecture and Engine verification in isolated temporary target directories |
