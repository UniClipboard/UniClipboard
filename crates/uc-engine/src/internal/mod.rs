//! Internal assembly modules exposed temporarily while desktop consumers move
//! to the stable `Engine` interface.

pub mod blob_tasks;
pub mod cancel_invitation;
pub mod capture;
pub mod clipboard;
pub(crate) mod clipboard_runtime;
pub mod create_space;
pub mod deps;
pub mod device;
pub mod encryption;
pub mod facade;
pub mod factory_reset;
pub mod file_transfer;
pub mod history;
pub mod host_adapters;
pub(crate) mod inbound_staging;
pub mod invitation;
pub mod join_space;
pub mod lifecycle;
pub mod member;
pub mod migration_progress;
pub mod network_policy;
pub mod platform;
pub mod receive;
pub mod reconcile;
pub mod reset_space;
pub mod restore;
pub(crate) mod runtime;
pub mod search;
pub mod session_recovery;
pub mod setup_state;
pub mod storage;
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
