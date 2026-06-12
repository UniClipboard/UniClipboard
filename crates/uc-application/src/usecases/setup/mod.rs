//! `setup` domain use cases — first-run space creation and post-setup
//! unlock. Both are intentionally kept as cross-port orchestration inside
//! a single use case (AGENTS.md §8.1): the passphrase-mismatch check,
//! ordered port calls and "don't mark complete if an earlier step failed"
//! invariant all belong to one atomic application action.

pub(crate) mod initialize_space;
pub(crate) mod switch_space;
pub(crate) mod unlock_space;
