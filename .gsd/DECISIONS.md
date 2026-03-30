# DECISIONS.md — Project Decision Register

> Append-only. Decisions are immutable once recorded.

---

## D001 — Inline compute_dir_size in uc-daemon instead of importing from uc-app

**Scope:** architecture  
**Decision:** Inline `compute_dir_size()` in `uc-daemon/src/api/storage.rs` using `tokio::fs` rather than importing `uc-app/usecases/storage::dir_size`  
**Choice:** Inline implementation in uc-daemon  
**Rationale:** `uc-app/usecases/storage::dir_size` is `pub(crate)` visibility, which is not accessible from `uc-daemon` (different crate boundary). Rust visibility applies at crate level, not module level.  
**Revisable:** Yes — if `dir_size` is promoted to `pub` in uc-app, the inline impl can be replaced with the import.  
**When:** M002-zldd9y / S03 (2026-03-30)  
**Made by:** agent

---

## D002 — L4 confirmation pattern for destructive HTTP endpoints

**Scope:** pattern  
**Decision:** Destructive operations (e.g., clear cache) require an explicit `confirmed: bool` field in the request body; HTTP 400 returned if absent (JsonRejection) or false.  
**Choice:** Confirmation field in request body, not query param or separate step  
**Rationale:** Matches the established pattern used elsewhere in the codebase (e.g., reset endpoints). JSON body is more structured and self-documenting than query params. JsonRejection handling catches both "missing body" and "malformed body" cases cleanly.  
**Revisable:** Yes — could be replaced with a two-phase confirm endpoint if UX requires a separate confirmation step.  
**When:** M002-zldd9y / S03 (2026-03-30)  
**Made by:** agent
