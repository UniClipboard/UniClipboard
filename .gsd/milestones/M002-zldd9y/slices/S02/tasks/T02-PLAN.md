---
estimated_steps: 4
estimated_files: 2
skills_used: []
---

# T02: Register settings and encryption routers in routes.rs, update mod.rs

1. In mod.rs, add pub mod encryption; pub mod settings;
2. In routes.rs router_l2_plus(), merge settings::router() and encryption::router()
3. Update routes.rs doc comment re: L3/L4 enforcement
4. Run full daemon test suite

## Inputs

- `src-tauri/crates/uc-daemon/src/api/routes.rs`
- `src-tauri/crates/uc-daemon/src/api/mod.rs`

## Expected Output

- `mod.rs with new module declarations`
- `routes.rs with merged routers`

## Verification

cd src-tauri && cargo test -p uc-daemon -- --nocapture 2>&1 | tail -20
