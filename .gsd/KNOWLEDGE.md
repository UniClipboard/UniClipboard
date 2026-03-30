# KNOWLEDGE.md — Project Patterns and Lessons Learned

> Append-only. Lessons that save future agents from repeating investigation.

---

## Rust pub(crate) visibility does not cross crate boundaries

**Pattern:** When a function is declared `pub(crate)` in module `foo` of crate `A`, it is NOT accessible from crate `B` that depends on `A`. `pub(crate)` means "public within crate A" — crate B is a different crate.

**Lesson:** If uc-daemon needs a utility from uc-app, and it's only `pub(crate)`, either:

1. Make it `pub` in uc-app (if appropriate), or
2. Re-implement inline in uc-daemon (chosen in D001)

**Seen in:** M002-zldd9y / S03 — `uc-app/usecases/storage::dir_size` was `pub(crate)`, so `compute_dir_size()` was re-implemented in `storage.rs`.

---

## L4 destructive operation pattern: JsonRejection + explicit false check

**Pattern:** When an HTTP endpoint requires explicit user confirmation for a destructive action:

```rust
// 1. JsonRejection catches missing body
let Json(req) = body_result else {
    return (StatusCode::BAD_REQUEST, Json(confirmation_error())).into_response();
};

// 2. Explicit false check
if !req.confirmed {
    return (StatusCode::BAD_REQUEST, Json(confirmation_error())).into_response();
}
```

**Lesson:** Both missing body (JsonRejection) and explicit `confirmed: false` must return 400. JsonRejection alone only catches missing/malformed JSON — a body of `{}` would parse as valid JSON but have no `confirmed` field.

**Seen in:** M002-zldd9y / S03 — `POST /storage/clear-cache`.
