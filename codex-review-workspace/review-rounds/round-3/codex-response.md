# 41-04-PLAN Round 3 — Codex Review Response

## Verdict: NEEDS_REVISION

## Findings

### FINDING-1: MAJOR

Task 1 still mixes the corrected byte-limit rule with the old `path.as_os_str().len() <= 103` wording in both the length-guard description and `test_socket_path_length`. That is the exact class of mistake round 2 was supposed to eliminate: the accepted fix was to use `std::os::unix::ffi::OsStrExt::as_bytes().len()` explicitly on Unix. Leaving the old expression in the plan makes the implementation guidance ambiguous and weakens the regression tests around multibyte / raw Unix path bytes.

**SUGGESTION:** Rewrite every Unix length-check instruction in Task 1 and its tests to use one exact helper, for example `socket_path_len_bytes(path) <= 103`, implemented via `OsStrExt::as_bytes().len()` under `#[cfg(unix)]`. Use that helper in production code and all boundary tests so the off-by-one and byte-counting fix is enforced consistently.
