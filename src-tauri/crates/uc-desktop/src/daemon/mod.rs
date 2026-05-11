//! daemon 运行模式。

pub(crate) mod app;
pub(crate) mod app_assembly;
pub(crate) mod app_facade_assembly;
pub(crate) mod bootstrap;
pub(crate) mod handle;
pub(crate) mod host;
pub(crate) mod mobile_lan_lifecycle;
pub(crate) mod ownership;
pub(crate) mod peers;
pub(crate) mod run_loop;
pub mod run_mode;
pub(crate) mod runtime_assembly;
pub(crate) mod runtime_controls;
pub(crate) mod search;
pub(crate) mod search_assembly;
pub(crate) mod service;
pub(crate) mod service_assembly;
pub(crate) mod service_plan;
pub(crate) mod startup_recovery;
pub(crate) mod state;
pub(crate) mod tokio_runtime;
pub(crate) mod workers;

pub use handle::DaemonHandle;
pub(crate) use host::start_in_process;
pub use host::{run, ProcessRuntimeHandles};
pub use ownership::DaemonOwnership;
