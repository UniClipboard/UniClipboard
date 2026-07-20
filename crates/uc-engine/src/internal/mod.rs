//! Internal assembly modules exposed temporarily while desktop consumers move
//! to the stable `Engine` interface.

pub mod blob_tasks;
pub mod cancel_invitation;
pub mod clipboard;
pub(crate) mod clipboard_runtime;
pub mod create_space;
pub mod deps;
pub mod facade;
pub mod file_transfer;
pub mod host_adapters;
pub(crate) mod inbound_staging;
pub mod invitation;
pub mod join_space;
pub mod lifecycle;
pub mod network_policy;
pub mod platform;
pub mod reconcile;
pub(crate) mod runtime;
pub mod session_recovery;
pub mod sync_engine;
pub mod unlock;
pub mod wire;

#[cfg(test)]
mod tests {
    #[test]
    fn facade_assembly_is_owned_by_engine() {
        let _ = super::facade::build_app_facade_from_deps;
    }
}
