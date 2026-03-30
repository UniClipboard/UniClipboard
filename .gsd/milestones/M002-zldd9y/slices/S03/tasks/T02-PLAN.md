---
estimated_steps: 3
estimated_files: 2
skills_used: []
---

# T02: Register storage router in routes.rs and update mod.rs

1. In mod.rs, add pub mod storage; (if not already present)
2. In routes.rs router_l2_plus(), merge storage::router()
3. Run full daemon test suite to verify no regressions

## Inputs

- `src-tauri/crates/uc-daemon/src/api/routes.rs`
- `src-tauri/crates/uc-daemon/src/api/mod.rs`

## Expected Output

- `mod.rs with storage module`
- `routes.rs with storage router merged`

## Verification

cd src-tauri && cargo test -p uc-daemon -- --nocapture 2>&1 | tail -20
